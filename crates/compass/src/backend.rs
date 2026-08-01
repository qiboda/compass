//! Async backend: receives `FetchRequest` off the signal bus, dispatches
//! to the DuckDbProvider (local Parquet-backed) on a Tokio runtime, and
//! returns `FetchResponse` through the result slot.
//!
//! `wire_backend` builds the full pipeline and returns the UI-side handles:
//! a `Signal<FetchRequest>` the UI uses to submit work, and a `BackendHandle`
//! that owns the `AsyncDispatcher` so the Tokio runtime stays alive.
//!
//! Pattern reference: `citizen_signal_async` example from egui-mobius.

use std::sync::Arc;

use chrono::Utc;
use compass_core::data::parquet::ParquetReader;
use egui_lens::ReactiveEventLogger;
use egui_mobius::dispatching::AsyncDispatcher;
use egui_mobius::factory;
use egui_mobius::signals::Signal;

use compass_core::data::{duckdb::DuckDbProvider, provider::DataProvider};
use compass_core::model::AppConfig;
use compass_strategy::run_screener;

use crate::messages::{FetchRequest, FetchResponse, RunScreenerRequest, RunScreenerResponse};
use crate::state::SharedState;

/// Owns the `AsyncDispatcher`s so the Tokio runtimes stay alive for the
/// program duration. Dropping this shuts down the runtimes.
pub struct BackendHandle {
    _dispatcher: AsyncDispatcher<FetchRequest, FetchResponse>,
    _screener_dispatcher: AsyncDispatcher<RunScreenerRequest, RunScreenerResponse>,
}

/// Build the async work pipeline. Returns:
/// - `Signal<FetchRequest>` — UI thread submits bar fetches via `.send(req)`
/// - `Signal<RunScreenerRequest>` — UI thread submits screener runs
/// - `BackendHandle` — keep alive on the App struct
///
/// The result slots are started internally — they write response values
/// into the reactive `SharedState` and request a UI repaint.
pub fn wire_backend(
    config: AppConfig,
    state: Arc<SharedState>,
    egui_ctx: egui::Context,
) -> (
    Signal<FetchRequest>,
    Signal<RunScreenerRequest>,
    BackendHandle,
) {
    let (work_signal, work_slot) = factory::create_signal_slot::<FetchRequest>();
    let (result_signal, mut result_slot) = factory::create_signal_slot::<FetchResponse>();

    let (screener_signal, screener_slot) = factory::create_signal_slot::<RunScreenerRequest>();
    let (screener_result_signal, mut screener_result_slot) =
        factory::create_signal_slot::<RunScreenerResponse>();

    let parquet_dir = std::path::PathBuf::from(&config.parquet.dir);

    let dispatcher = AsyncDispatcher::<FetchRequest, FetchResponse>::new();
    dispatcher.attach_async(work_slot, result_signal, move |req: FetchRequest| {
        let parquet_dir = parquet_dir.clone();
        async move {
            let provider = match DuckDbProvider::new(parquet_dir.exists().then_some(parquet_dir)) {
                Ok(p) => p,
                Err(e) => {
                    return FetchResponse {
                        bars: vec![],
                        error: Some(format!("failed to open duckdb: {e}")),
                    };
                }
            };

            match provider
                .fetch_bars(&req.symbol, &req.timeframe, req.range_start, req.range_end)
                .await
            {
                Ok(bars) if bars.is_empty() => FetchResponse {
                    bars: vec![],
                    error: Some(format!("no data for {}", req.symbol)),
                },
                Ok(bars) => FetchResponse { bars, error: None },
                Err(e) => FetchResponse {
                    bars: vec![],
                    error: Some(format!("{e}")),
                },
            }
        }
    });

    // Start the result slot — runs on a dedicated worker thread, writes
    // response values into the reactive `Dynamic<T>` fields, then wakes
    // the UI so the next frame picks up the new data.
    let bars_dyn = state.bars.clone();
    let loading = state.loading.clone();
    let error = state.error.clone();
    let log_dyn = state.log.clone();
    let repaint_ctx = egui_ctx.clone();

    result_slot.start(move |resp: FetchResponse| {
        let logger = ReactiveEventLogger::new(&log_dyn);
        let bar_count = resp.bars.len();
        bars_dyn.set(resp.bars);
        loading.set(false);
        if let Some(ref err) = resp.error {
            error.set(Some(err.clone()));
            logger.log_error(&format!("fetch failed: {err}"));
        } else {
            error.set(None);
            logger.log_info(&format!("fetch completed: {bar_count} bars"));
        }
        repaint_ctx.request_repaint();
    });

    let screener_parquet_dir = std::path::PathBuf::from(&config.parquet.dir);
    let screener_dispatcher = AsyncDispatcher::<RunScreenerRequest, RunScreenerResponse>::new();
    screener_dispatcher.attach_async(
        screener_slot,
        screener_result_signal,
        move |req: RunScreenerRequest| {
            let parquet_dir = screener_parquet_dir.clone();
            async move {
                let reader = match ParquetReader::new(&parquet_dir) {
                    Ok(r) => r,
                    Err(e) => {
                        return RunScreenerResponse {
                            rows: vec![],
                            total: 0,
                            error: Some(format!("failed to open parquet: {e}")),
                        };
                    }
                };
                match run_screener(&req.query, &reader, Utc::now().date_naive()) {
                    Ok(res) => RunScreenerResponse {
                        rows: res.rows,
                        total: res.total,
                        error: None,
                    },
                    Err(e) => RunScreenerResponse {
                        rows: vec![],
                        total: 0,
                        error: Some(format!("{e}")),
                    },
                }
            }
        },
    );

    let screener_result_dyn = state.screener_result.clone();
    let screener_total_dyn = state.screener_total.clone();
    let screener_loading = state.screener_loading.clone();
    let screener_error = state.screener_error.clone();
    let screener_repaint_ctx = egui_ctx.clone();

    screener_result_slot.start(move |resp: RunScreenerResponse| {
        screener_result_dyn.set(resp.rows);
        screener_total_dyn.set(resp.total);
        screener_loading.set(false);
        screener_error.set(resp.error);
        screener_repaint_ctx.request_repaint();
    });

    (
        work_signal,
        screener_signal,
        BackendHandle {
            _dispatcher: dispatcher,
            _screener_dispatcher: screener_dispatcher,
        },
    )
}

// ===========================================================================
// Integration tests — wire_backend + SharedState + parquet (ref #79)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::FetchRequest;
    use crate::state::SharedState;
    use chrono::DateTime;
    use compass_core::model::AppConfig;
    use duckdb::Connection;
    use std::sync::Arc;
    use std::time::Duration;

    /// Helper: build an `AppConfig` with a custom `parquet.dir`.
    fn config_with_parquet_dir(dir: String) -> AppConfig {
        let mut config = AppConfig::default();
        config.parquet.dir = dir;
        config
    }

    /// Helper: build a `FetchRequest` covering a wide date-range.
    fn fetch_request(symbol: &str) -> FetchRequest {
        FetchRequest {
            symbol: symbol.to_string(),
            timeframe: "1d".to_string(),
            range_start: DateTime::from_timestamp(0, 0).expect("valid epoch"),
            range_end: DateTime::from_timestamp(4_000_000_000, 0).expect("valid end timestamp"),
        }
    }

    /// Poll `state.loading` until it flips from `true` to `false` (or timeout).
    ///
    /// The caller must set `loading.set(true)` *before* sending work so that
    /// the response handler's `loading.set(false)` acts as the arrival signal.
    /// This loop gives the async dispatcher + `result_slot` thread up to 10 s.
    fn wait_for_response(state: &SharedState) {
        for _ in 0..100 {
            if !state.loading.get() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timeout waiting for backend response");
    }

    /// Helper: create a minimal `stock_daily.parquet` inside `dir` with the
    /// schema expected by `DuckDbProvider::fetch_bars` parquet fallback:
    /// `symbol`, `tradedate`, `open`, `high`, `low`, `close`, `volume`,
    /// `adjclose`, `amount`.
    ///
    /// Uses a standalone DuckDB connection — closed before the test provider
    /// reads the file, avoiding any lock contention.
    fn write_test_parquet(dir: &std::path::Path, rows: &[&str]) {
        let conn = Connection::open_in_memory().expect("failed to open in-memory DuckDB");

        conn.execute_batch(
            "CREATE TABLE stock_daily (
                symbol    VARCHAR,
                tradedate DATE,
                open      DOUBLE,
                high      DOUBLE,
                low       DOUBLE,
                close     DOUBLE,
                volume    DOUBLE,
                adjclose  DOUBLE,
                amount    DOUBLE
            );",
        )
        .expect("failed to create table");

        for row in rows {
            conn.execute_batch(&format!("INSERT INTO stock_daily VALUES {row};"))
                .expect("failed to insert row");
        }

        let parquet_path = dir.join("stock_daily.parquet");
        let export_sql = format!(
            "COPY stock_daily TO '{}' (FORMAT PARQUET);",
            parquet_path.to_string_lossy().replace('\'', "''")
        );
        conn.execute_batch(&export_sql)
            .expect("failed to export parquet");

        drop(conn);
    }

    // ------------------------------------------------------------------
    // Test 1: ERROR path — nonexistent parquet dir
    // ------------------------------------------------------------------

    /// When `parquet.dir` points to a nonexistent directory, the backend
    /// creates an in-memory-only `DuckDbProvider`.  No data exists, so
    /// `fetch_bars` returns empty → `SharedState.error` is set to a
    /// "no data" message and `loading` flips back to `false`.
    #[test]
    fn error_path_nonexistent_parquet_dir() {
        let config = config_with_parquet_dir("/tmp/compass_test_nonexistent_xyz".into());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (work_signal, _screener_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx);

        // Signal that work is in flight so wait_for_response can detect
        // the handler's loading.set(false).
        state.loading.set(true);
        work_signal
            .send(fetch_request("000001"))
            .expect("failed to send request");

        wait_for_response(&state);

        assert!(
            !state.loading.get(),
            "loading should be false after response arrives"
        );
        assert!(
            state.error.get().is_some(),
            "error should be set when no data is available"
        );
    }

    // ------------------------------------------------------------------
    // Test 2: SUCCESS path — valid parquet with matching symbol
    // ------------------------------------------------------------------

    /// With a `stock_daily.parquet` file containing data for the requested
    /// symbol, the backend returns non-empty bars and clears the error.
    #[test]
    fn success_path_valid_parquet() {
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");

        write_test_parquet(
            temp_dir.path(),
            &[
                "('000001', '2025-01-02', 10.0, 10.5,  9.8, 10.2, 100000.0, 10.2, 1020000.0)",
                "('000001', '2025-01-03', 10.2, 10.8, 10.1, 10.6, 120000.0, 10.6, 1272000.0)",
                "('000001', '2025-01-06', 10.6, 11.0, 10.4, 10.9, 110000.0, 10.9, 1199000.0)",
            ],
        );

        let config = config_with_parquet_dir(temp_dir.path().to_string_lossy().to_string());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (work_signal, _screener_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx);
        state.loading.set(true);
        work_signal
            .send(fetch_request("000001"))
            .expect("failed to send request");

        wait_for_response(&state);

        assert!(!state.loading.get());
        assert!(
            state.error.get().is_none(),
            "error should be none for valid data"
        );
        assert!(!state.bars.get().is_empty(), "bars should not be empty");
        assert_eq!(state.bars.get().len(), 3, "expected 3 bars for 000001");
    }

    // ------------------------------------------------------------------
    // Test 3: NO-DATA path — valid parquet but symbol absent
    // ------------------------------------------------------------------

    /// When the parquet file exists but does not contain the requested
    /// symbol, the backend returns an error containing "no data".
    #[test]
    fn nodata_path_symbol_not_in_parquet() {
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");

        write_test_parquet(
            temp_dir.path(),
            &["('000001', '2025-01-02', 10.0, 10.5, 9.8, 10.2, 100000.0, 10.2, 1020000.0)"],
        );

        let config = config_with_parquet_dir(temp_dir.path().to_string_lossy().to_string());
        let state = Arc::new(SharedState::new("999999", "1d"));
        let egui_ctx = egui::Context::default();

        let (work_signal, _screener_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx);
        state.loading.set(true);
        work_signal
            .send(fetch_request("999999"))
            .expect("failed to send request");

        wait_for_response(&state);

        assert!(!state.loading.get());
        assert!(
            state.bars.get().is_empty(),
            "bars should be empty for unknown symbol"
        );
        let err = state.error.get();
        assert!(err.is_some(), "error should be set for unknown symbol");
        assert!(
            err.as_deref().unwrap().contains("no data"),
            "error should contain 'no data': got {:?}",
            err
        );
    }
}
