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

use compass_core::data::{duckdb::DuckDbProvider, provider::DataError, provider::DataProvider};
use compass_core::llm::{LlmClient, LlmConfig, LlmError};
use compass_core::model::AppConfig;
use compass_strategy::run_screener;
use compass_strategy::sepa::run_sepa;
use compass_types::SepaQuery;

use crate::llm_screener::{build_screener_prompt, parse_filter_response};
use crate::messages::{
    FetchRequest, FetchResponse, RunIndexSnapshotRequest, RunIndexSnapshotResponse, RunLlmRequest,
    RunLlmResponse, RunScreenerRequest, RunScreenerResponse, RunSepaRequest, RunSepaResponse,
};
use crate::state::SharedState;
use compass_i18n::t;

/// SEPA backend result cap — the engine always returns the full TOP-N list;
/// the panel truncates further (TOP 50/30) as pure GUI state.
const DEFAULT_SEPA_TOP_N: usize = 50;

/// Owns the `AsyncDispatcher`s so the Tokio runtimes stay alive for the
/// program duration. Dropping this shuts down the runtimes.
pub struct BackendHandle {
    _dispatcher: AsyncDispatcher<FetchRequest, FetchResponse>,
    _screener_dispatcher: AsyncDispatcher<RunScreenerRequest, RunScreenerResponse>,
    _sepa_dispatcher: AsyncDispatcher<RunSepaRequest, RunSepaResponse>,
    _index_dispatcher: AsyncDispatcher<RunIndexSnapshotRequest, RunIndexSnapshotResponse>,
    _llm_dispatcher: AsyncDispatcher<RunLlmRequest, RunLlmResponse>,
}

/// Build the async work pipeline. Returns:
/// - `Signal<FetchRequest>` — UI thread submits bar fetches via `.send(req)`
/// - `Signal<RunScreenerRequest>` — UI thread submits screener runs
/// - `Signal<RunSepaRequest>` — UI thread submits SEPA scoring runs
/// - `Signal<RunIndexSnapshotRequest>` — UI thread submits index snapshot runs
/// - `BackendHandle` — keep alive on the App struct
///
/// The result slots are started internally — they write response values
/// into the reactive `SharedState` and request a UI repaint.
#[allow(clippy::type_complexity)]
pub fn wire_backend(
    config: AppConfig,
    state: Arc<SharedState>,
    egui_ctx: egui::Context,
    llm_config: Option<LlmConfig>,
) -> (
    Signal<FetchRequest>,
    Signal<RunScreenerRequest>,
    Signal<RunSepaRequest>,
    Signal<RunIndexSnapshotRequest>,
    Signal<RunLlmRequest>,
    BackendHandle,
) {
    let (work_signal, work_slot) = factory::create_signal_slot::<FetchRequest>();
    let (result_signal, mut result_slot) = factory::create_signal_slot::<FetchResponse>();

    let (screener_signal, screener_slot) = factory::create_signal_slot::<RunScreenerRequest>();
    let (screener_result_signal, mut screener_result_slot) =
        factory::create_signal_slot::<RunScreenerResponse>();

    let (sepa_signal, sepa_slot) = factory::create_signal_slot::<RunSepaRequest>();
    let (sepa_result_signal, mut sepa_result_slot) =
        factory::create_signal_slot::<RunSepaResponse>();

    let (index_signal, index_slot) = factory::create_signal_slot::<RunIndexSnapshotRequest>();
    let (index_result_signal, mut index_result_slot) =
        factory::create_signal_slot::<RunIndexSnapshotResponse>();

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
                        error: Some(t!("error.duckdb_open", e = e).to_string()),
                    };
                }
            };

            match provider
                .fetch_bars(&req.symbol, &req.timeframe, req.range_start, req.range_end)
                .await
            {
                Ok(bars) if bars.is_empty() => FetchResponse {
                    bars: vec![],
                    error: Some(t!("error.no_data", symbol = req.symbol).to_string()),
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
            logger.log_error(&t!("logger.log_fetch_failed", e = err));
        } else {
            error.set(None);
            logger.log_info(&t!("logger.log_fetch_completed", count = bar_count));
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
                            error: Some(t!("error.parquet_open", e = e).to_string()),
                        };
                    }
                };
                match run_screener(&req.filter, &reader, Utc::now().date_naive()) {
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
    let screener_log_dyn = state.log.clone();
    let screener_repaint_ctx = egui_ctx.clone();

    screener_result_slot.start(move |resp: RunScreenerResponse| {
        let logger = ReactiveEventLogger::new(&screener_log_dyn);
        screener_result_dyn.set(resp.rows);
        screener_total_dyn.set(resp.total);
        screener_loading.set(false);
        if let Some(ref err) = resp.error {
            screener_error.set(Some(err.clone()));
            logger.log_error(&t!("logger.log_screener_failed", e = err));
        } else {
            screener_error.set(None);
            logger.log_info(&t!("logger.log_screener_completed", count = resp.total));
        }
        screener_repaint_ctx.request_repaint();
    });

    let sepa_parquet_dir = std::path::PathBuf::from(&config.parquet.dir);
    let sepa_dispatcher = AsyncDispatcher::<RunSepaRequest, RunSepaResponse>::new();
    sepa_dispatcher.attach_async(
        sepa_slot,
        sepa_result_signal,
        move |_req: RunSepaRequest| {
            let parquet_dir = sepa_parquet_dir.clone();
            async move {
                let reader = match ParquetReader::new(&parquet_dir) {
                    Ok(r) => r,
                    Err(e) => {
                        return RunSepaResponse {
                            data: compass_types::SepaData {
                                rows: vec![],
                                thermometer: compass_types::MarketThermometer {
                                    score: 0.0,
                                    position_key: "sepa.position.low",
                                    position_pct: 0.0,
                                    indicators: vec![],
                                },
                                date: String::new(),
                            },
                            error: Some(t!("error.parquet_open", e = e).to_string()),
                        };
                    }
                };
                match run_sepa(
                    &SepaQuery {
                        top_n: DEFAULT_SEPA_TOP_N,
                    },
                    &reader,
                    Utc::now().date_naive(),
                ) {
                    Ok(data) => RunSepaResponse { data, error: None },
                    Err(e) => RunSepaResponse {
                        data: compass_types::SepaData {
                            rows: vec![],
                            thermometer: compass_types::MarketThermometer {
                                score: 0.0,
                                position_key: "sepa.position.low",
                                position_pct: 0.0,
                                indicators: vec![],
                            },
                            date: String::new(),
                        },
                        error: Some(format!("{e}")),
                    },
                }
            }
        },
    );

    let sepa_data_dyn = state.sepa_data.clone();
    let sepa_loading = state.sepa_loading.clone();
    let sepa_error = state.sepa_error.clone();
    let sepa_log_dyn = state.log.clone();
    let sepa_repaint_ctx = egui_ctx.clone();

    sepa_result_slot.start(move |resp: RunSepaResponse| {
        let logger = ReactiveEventLogger::new(&sepa_log_dyn);
        let row_count = resp.data.rows.len();
        sepa_data_dyn.set(Some(resp.data));
        sepa_loading.set(false);
        if let Some(ref err) = resp.error {
            sepa_error.set(Some(err.clone()));
            logger.log_error(&t!("logger.log_sepa_failed", e = err));
        } else {
            sepa_error.set(None);
            logger.log_info(&t!("logger.log_sepa_completed", count = row_count));
        }
        sepa_repaint_ctx.request_repaint();
    });

    // Fourth channel (epic #255 C4): index/board market snapshot — SEPA
    // 同构. The handler reads index_daily.parquet + index_basic.parquet via
    // ParquetReader (window rn<=2 per symbol) and joins names locally; a
    // missing index file yields an empty snapshot (panel empty state), not
    // an error.
    let index_parquet_dir = std::path::PathBuf::from(&config.parquet.dir);
    let index_dispatcher =
        AsyncDispatcher::<RunIndexSnapshotRequest, RunIndexSnapshotResponse>::new();
    index_dispatcher.attach_async(
        index_slot,
        index_result_signal,
        move |_req: RunIndexSnapshotRequest| {
            let parquet_dir = index_parquet_dir.clone();
            async move {
                let reader = match ParquetReader::new(&parquet_dir) {
                    Ok(r) => r,
                    Err(e) => {
                        return RunIndexSnapshotResponse {
                            data: compass_types::IndexSnapshot {
                                rows: vec![],
                                date: String::new(),
                            },
                            error: Some(t!("error.parquet_open", e = e).to_string()),
                        };
                    }
                };
                match build_index_snapshot(&reader) {
                    Ok(data) => RunIndexSnapshotResponse { data, error: None },
                    Err(e) => RunIndexSnapshotResponse {
                        data: compass_types::IndexSnapshot {
                            rows: vec![],
                            date: String::new(),
                        },
                        error: Some(format!("{e}")),
                    },
                }
            }
        },
    );

    let index_snapshot_dyn = state.index_snapshot.clone();
    let index_loading = state.index_snapshot_loading.clone();
    let index_error = state.index_snapshot_error.clone();
    let index_log_dyn = state.log.clone();
    let index_repaint_ctx = egui_ctx.clone();

    index_result_slot.start(move |resp: RunIndexSnapshotResponse| {
        let logger = ReactiveEventLogger::new(&index_log_dyn);
        let row_count = resp.data.rows.len();
        index_snapshot_dyn.set(Some(resp.data));
        index_loading.set(false);
        if let Some(ref err) = resp.error {
            index_error.set(Some(err.clone()));
            logger.log_error(&t!("logger.log_index_failed", e = err));
        } else {
            index_error.set(None);
            logger.log_info(&t!("logger.log_index_completed", count = row_count));
        }
        index_repaint_ctx.request_repaint();
    });

    // Fifth channel (epic #243 Batch 4, ref #247): natural-language → Filter
    // AST generation. `llm_config` is `None` when no API key is configured —
    // the handler then answers with a translated error and never touches the
    // network. Success writes the filter into `SharedState.llm_result` for
    // the screener panel to consume on the loading → idle transition.
    let llm_dispatcher = AsyncDispatcher::<RunLlmRequest, RunLlmResponse>::new();
    let (llm_signal, llm_slot) = factory::create_signal_slot::<RunLlmRequest>();
    let (llm_result_signal, mut llm_result_slot) = factory::create_signal_slot::<RunLlmResponse>();
    llm_dispatcher.attach_async(llm_slot, llm_result_signal, move |req: RunLlmRequest| {
        let llm_config = llm_config.clone();
        async move {
            let seq = req.seq;
            let cfg = match llm_config {
                Some(c) => c,
                None => {
                    return RunLlmResponse {
                        filter: None,
                        error: Some(t!("error.llm_not_configured").to_string()),
                        seq,
                    };
                }
            };
            let client = match LlmClient::new(cfg) {
                Ok(c) => c,
                Err(e) => {
                    return RunLlmResponse {
                        filter: None,
                        error: Some(format!("{e}")),
                        seq,
                    };
                }
            };
            match client
                .chat_json(&build_screener_prompt(&req.prompt), &req.prompt)
                .await
            {
                Ok(value) => match parse_filter_response(&value.to_string()) {
                    Ok(filter) => RunLlmResponse {
                        filter: Some(filter),
                        error: None,
                        seq,
                    },
                    Err(e) => RunLlmResponse {
                        filter: None,
                        error: Some(
                            t!("screener.llm.error_parse").to_string() + &format!(" ({e})"),
                        ),
                        seq,
                    },
                },
                Err(LlmError::NoContent) => RunLlmResponse {
                    filter: None,
                    error: Some(t!("screener.llm.error_empty").to_string()),
                    seq,
                },
                Err(LlmError::InvalidJson(raw)) => RunLlmResponse {
                    filter: None,
                    error: Some(t!("screener.llm.error_parse").to_string() + &format!(" ({raw})")),
                    seq,
                },
                Err(e) => RunLlmResponse {
                    filter: None,
                    error: Some(t!("screener.llm.error_network", e = e).to_string()),
                    seq,
                },
            }
        }
    });

    let llm_loading = state.llm_loading.clone();
    let llm_error = state.llm_error.clone();
    let llm_result = state.llm_result.clone();
    let llm_seq = state.llm_seq.clone();
    let llm_repaint_ctx = egui_ctx.clone();
    llm_result_slot.start(move |resp: RunLlmResponse| {
        if resp.seq != llm_seq.get() {
            return;
        }
        llm_loading.set(false);
        llm_result.set(resp.filter);
        if let Some(ref err) = resp.error {
            llm_error.set(Some(err.clone()));
        } else {
            llm_error.set(None);
        }
        llm_repaint_ctx.request_repaint();
    });

    (
        work_signal,
        screener_signal,
        sepa_signal,
        index_signal,
        llm_signal,
        BackendHandle {
            _dispatcher: dispatcher,
            _screener_dispatcher: screener_dispatcher,
            _sepa_dispatcher: sepa_dispatcher,
            _index_dispatcher: index_dispatcher,
            _llm_dispatcher: llm_dispatcher,
        },
    )
}

/// Build the full index/board market snapshot from the local parquet files.
///
/// `load_index_daily_rows` returns the last two trading days per symbol
/// (window rn<=2, ordered by symbol + tradedate ASC); the name table comes
/// from `index_basic.parquet` (falling back to the symbol itself when the
/// file is missing). The snapshot date is the latest tradedate seen.
fn build_index_snapshot(reader: &ParquetReader) -> Result<compass_types::IndexSnapshot, DataError> {
    use compass_types::{IndexRow, IndexSnapshot};
    use std::collections::HashMap;

    let daily = reader.load_index_daily_rows()?;
    let basics = reader.load_all_index_basics()?;
    let name_map: HashMap<String, (String, String)> = basics
        .into_iter()
        .map(|b| (b.symbol, (b.name, b.index_type)))
        .collect();

    let mut rows: Vec<IndexRow> = Vec::with_capacity(daily.len());
    let mut snapshot_date = String::new();
    let mut iter = daily.into_iter().peekable();
    while let Some(first) = iter.next() {
        let symbol = first.symbol.clone();
        let mut group = vec![first];
        while let Some(next) = iter.peek()
            && next.symbol == symbol
        {
            group.push(iter.next().expect("peeked row is still present"));
        }
        // Group is tradedate ASC — the last row is the latest quote.
        let last = group.last().expect("group is non-empty");
        let prev = group.get(group.len().saturating_sub(2));
        let change_pct = match prev {
            Some(p) if p.close > 0.0 => (last.close - p.close) / p.close * 100.0,
            _ => 0.0,
        };
        let (name, index_type) = name_map
            .get(&symbol)
            .cloned()
            .unwrap_or_else(|| (symbol.clone(), last.index_type.clone()));
        let date_str = last.trade_date.to_string();
        if date_str > snapshot_date {
            snapshot_date = date_str;
        }
        rows.push(IndexRow {
            symbol,
            name,
            index_type,
            latest: last.close,
            change_pct,
            amount: last.amount,
        });
    }

    Ok(IndexSnapshot {
        rows,
        date: snapshot_date,
    })
}

// ===========================================================================
// Integration tests — wire_backend + SharedState + parquet (ref #79)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use crate::messages::FetchRequest;
    use crate::state::SharedState;
    use chrono::DateTime;
    use compass_core::llm::LlmConfig;
    use compass_core::model::AppConfig;
    use compass_types::{Filter, SeriesCond};
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("en");
        let config = config_with_parquet_dir("/tmp/compass_test_nonexistent_xyz".into());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (work_signal, _screener_signal, _sepa_signal, _index_signal, _llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);

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
        rust_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Test 2: SUCCESS path — valid parquet with matching symbol
    // ------------------------------------------------------------------

    /// With a `stock_daily.parquet` file containing data for the requested
    /// symbol, the backend returns non-empty bars and clears the error.
    #[test]
    fn success_path_valid_parquet() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("en");
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

        let (work_signal, _screener_signal, _sepa_signal, _index_signal, _llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);
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
        rust_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Test 3: NO-DATA path — valid parquet but symbol absent
    // ------------------------------------------------------------------

    /// When the parquet file exists but does not contain the requested
    /// symbol, the backend returns an error containing "no data".
    #[test]
    fn nodata_path_symbol_not_in_parquet() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("en");
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");

        write_test_parquet(
            temp_dir.path(),
            &["('000001', '2025-01-02', 10.0, 10.5, 9.8, 10.2, 100000.0, 10.2, 1020000.0)"],
        );

        let config = config_with_parquet_dir(temp_dir.path().to_string_lossy().to_string());
        let state = Arc::new(SharedState::new("999999", "1d"));
        let egui_ctx = egui::Context::default();

        let (work_signal, _screener_signal, _sepa_signal, _index_signal, _llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);
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
        // The error template is resolved on the backend worker thread, which
        // does not hold LANG_LOCK — its locale can differ from this test's en
        // under parallel runs. Assert the language-neutral parts instead: the
        // symbol always appears, and the template never degrades to the raw
        // missing-key fallback (the key string itself).
        let msg = err.as_deref().unwrap();
        assert!(
            msg.contains("999999"),
            "error must mention the requested symbol, got: {msg:?}"
        );
        assert!(
            !msg.contains("error.no_data"),
            "error must resolve via the key (not the missing-key fallback), got: {msg:?}"
        );
        rust_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Test 4: SCREENER path — full channel roundtrip
    // ------------------------------------------------------------------

    /// Write both `stock_daily.parquet` and `stock_basic.parquet` so the
    /// screener backend has everything `run_screener` needs.
    fn write_screener_parquet(dir: &std::path::Path) {
        let conn = Connection::open_in_memory().expect("failed to open in-memory DuckDB");

        conn.execute_batch(
            "CREATE TABLE daily (
                symbol    VARCHAR,
                tradedate DATE,
                open      DOUBLE,
                high      DOUBLE,
                low       DOUBLE,
                close     DOUBLE,
                volume    DOUBLE,
                adjclose  DOUBLE,
                amount    DOUBLE
            );
            INSERT INTO daily VALUES
                ('000001', '2026-07-27', 10.0, 10.5, 9.8, 10.2, 100000.0, 10.2, 1020000.0),
                ('000001', '2026-07-28', 10.2, 10.8, 10.1, 10.6, 120000.0, 10.6, 1272000.0),
                ('600519', '2026-07-27', 1500.0, 1510.0, 1490.0, 1505.0, 50000.0, 1505.0, 75250000.0),
                ('600519', '2026-07-28', 1505.0, 1520.0, 1500.0, 1515.0, 60000.0, 1515.0, 90900000.0);",
        )
        .expect("failed to create daily table");

        let parquet_path = dir.join("stock_daily.parquet");
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET);",
            parquet_path.to_string_lossy().replace('\'', "''")
        ))
        .expect("failed to export daily parquet");

        conn.execute_batch(
            "CREATE TABLE basic (
                symbol    VARCHAR,
                name      VARCHAR,
                exchange  VARCHAR,
                list_date DATE,
                delist_date DATE,
                board     VARCHAR,
                full_name VARCHAR,
                total_share DOUBLE,
                industry  VARCHAR,
                region    VARCHAR
            );
            INSERT INTO basic VALUES
                ('000001', '平安银行', 'SZ', '1991-04-03', NULL, '主板', '平安银行股份有限公司', 19405918198.0, '银行', '广东省'),
                ('600519', '贵州茅台', 'SH', '2001-08-27', NULL, '主板', '贵州茅台酒股份有限公司', 1256197800.0, '白酒', '贵州省');",
        )
        .expect("failed to create basic table");

        let basic_path = dir.join("stock_basic.parquet");
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET);",
            basic_path.to_string_lossy().replace('\'', "''")
        ))
        .expect("failed to export basic parquet");

        drop(conn);
    }

    /// Poll `state.screener_loading` until it flips false (or timeout).
    fn wait_for_screener_response(state: &SharedState) {
        for _ in 0..100 {
            if !state.screener_loading.get() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timeout waiting for screener backend response");
    }

    /// Full screener channel: query → run_screener → SharedState + display log.
    #[test]
    fn screener_path_returns_matched_rows_and_logs() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("en");
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
        write_screener_parquet(temp_dir.path());

        let config = config_with_parquet_dir(temp_dir.path().to_string_lossy().to_string());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work_signal, screener_signal, _sepa_signal, _index_signal, _llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);

        state.screener_loading.set(true);
        let query = compass_types::ScreenerQuery::default();
        screener_signal
            .send(RunScreenerRequest {
                filter: compass_types::Filter::from(query),
            })
            .expect("failed to send screener request");

        wait_for_screener_response(&state);

        assert!(!state.screener_loading.get());
        assert_eq!(
            state.screener_total.get(),
            2,
            "both stocks match empty query"
        );
        assert_eq!(state.screener_result.get().len(), 2);
        assert_eq!(
            state.screener_result.get()[0].symbol,
            "600519",
            "cap desc: 茅台 first"
        );

        // Display log entry visible in the GUI logger panel: the result slot
        // writes at least one log line per screener run.
        assert!(
            state.log.get().log_count() > 0,
            "result slot must write a display log entry"
        );
        rust_i18n::set_locale("zh");
    }

    /// Screener with an industry filter narrows the result set.
    #[test]
    fn screener_path_applies_industry_filter() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("en");
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
        write_screener_parquet(temp_dir.path());

        let config = config_with_parquet_dir(temp_dir.path().to_string_lossy().to_string());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work_signal, screener_signal, _sepa_signal, _index_signal, _llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);

        state.screener_loading.set(true);
        let query = compass_types::ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ..compass_types::ScreenerQuery::default()
        };
        screener_signal
            .send(RunScreenerRequest {
                filter: compass_types::Filter::from(query),
            })
            .expect("failed to send screener request");

        wait_for_screener_response(&state);

        assert_eq!(state.screener_total.get(), 1);
        assert_eq!(state.screener_result.get()[0].symbol, "600519");
        rust_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Test 5: SEPA path — full third-channel roundtrip
    // ------------------------------------------------------------------

    /// Poll `state.sepa_loading` until it flips false (or timeout).
    fn wait_for_sepa_response(state: &SharedState) {
        for _ in 0..100 {
            if !state.sepa_loading.get() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timeout waiting for sepa backend response");
    }

    /// Full SEPA channel: request → `run_sepa` → SharedState + display log.
    ///
    /// Reuses the screener fixture (stock_daily + stock_basic only): the five
    /// SEPA tables are absent, which the engine degrades to empty vecs — the
    /// modules score 0 and the hard-filter survivors still rank.
    #[test]
    fn sepa_path_returns_ranked_rows_and_logs() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("en");
        let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
        write_screener_parquet(temp_dir.path());

        let config = config_with_parquet_dir(temp_dir.path().to_string_lossy().to_string());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work_signal, _screener_signal, sepa_signal, _index_signal, _llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);

        state.sepa_loading.set(true);
        sepa_signal
            .send(RunSepaRequest {})
            .expect("failed to send sepa request");

        wait_for_sepa_response(&state);

        assert!(!state.sepa_loading.get());
        assert!(
            state.sepa_error.get().is_none(),
            "valid parquet must not produce a sepa error"
        );
        let data = state.sepa_data.get().expect("sepa data written back");
        assert!(
            !data.rows.is_empty(),
            "600519 passes the hard filters and must be ranked"
        );
        assert_eq!(data.rows[0].rank, 1, "rows carry 1-based official ranks");
        assert!(
            state.log.get().log_count() > 0,
            "result slot must write a display log entry"
        );
        rust_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // LLM channel (epic #243 Batch 4, ref #247) — 5th dispatcher
    // ------------------------------------------------------------------

    /// Poll `state.llm_loading` until it flips false (or timeout).
    fn wait_for_llm_response(state: &SharedState) {
        for _ in 0..100 {
            if !state.llm_loading.get() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timeout waiting for llm backend response");
    }

    fn llm_config_at(server: &httpmock::MockServer) -> LlmConfig {
        LlmConfig {
            base_url: format!("{}/v1", server.base_url()),
            api_key: "sk-test".to_string(),
            model: "gpt-test".to_string(),
        }
    }

    /// Mock one OpenAI-compatible chat completion whose content is the given
    /// Filter JSON (what the LLM client parses into a `Value`).
    fn mock_chat_content<'a>(
        server: &'a httpmock::MockServer,
        content: &str,
    ) -> httpmock::Mock<'a> {
        use httpmock::Method::POST;
        server.mock(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200).json_body(serde_json::json!({
                "choices": [{"message": {"content": content}}]
            }));
        })
    }

    #[test]
    fn llm_path_success_roundtrip() {
        let server = httpmock::MockServer::start();
        let _mock = mock_chat_content(
            &server,
            "{\"Series\":{\"UpDays\":{\"n\":5,\"min_pct\":3.0}}}",
        );
        let config = config_with_parquet_dir("/tmp/compass_test_nonexistent_llm_xyz".into());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work, _screener, _sepa, _index, llm_signal, _backend) = wire_backend(
            config,
            state.clone(),
            egui_ctx,
            Some(llm_config_at(&server)),
        );
        state.llm_seq.set(1);

        state.llm_loading.set(true);
        llm_signal
            .send(RunLlmRequest {
                prompt: "最近5天每天涨超3%".to_string(),
                seq: 1,
            })
            .expect("failed to send llm request");
        wait_for_llm_response(&state);

        assert!(!state.llm_loading.get());
        assert!(state.llm_error.get().is_none());
        assert_eq!(
            state.llm_result.get(),
            Some(Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }))
        );
    }

    #[test]
    fn llm_path_not_configured_returns_error_without_panic() {
        let config = config_with_parquet_dir("/tmp/compass_test_nonexistent_llm_xyz".into());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work, _screener, _sepa, _index, llm_signal, _backend) =
            wire_backend(config, state.clone(), egui_ctx, None);
        state.llm_seq.set(1);

        state.llm_loading.set(true);
        llm_signal
            .send(RunLlmRequest {
                prompt: "x".to_string(),
                seq: 1,
            })
            .expect("failed to send llm request");
        wait_for_llm_response(&state);

        assert!(!state.llm_loading.get());
        assert!(state.llm_result.get().is_none());
        let err = state.llm_error.get().expect("llm_error must be set");
        assert!(
            err.contains("LLM"),
            "unconfigured message must mention LLM, got: {err}"
        );
    }

    #[test]
    fn llm_path_server_5xx_sets_error() {
        use httpmock::Method::POST;
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(500).body("boom");
        });
        let config = config_with_parquet_dir("/tmp/compass_test_nonexistent_llm_xyz".into());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work, _screener, _sepa, _index, llm_signal, _backend) = wire_backend(
            config,
            state.clone(),
            egui_ctx,
            Some(llm_config_at(&server)),
        );
        state.llm_seq.set(1);

        state.llm_loading.set(true);
        llm_signal
            .send(RunLlmRequest {
                prompt: "x".to_string(),
                seq: 1,
            })
            .expect("failed to send llm request");
        wait_for_llm_response(&state);

        assert!(!state.llm_loading.get());
        assert!(state.llm_result.get().is_none());
        assert!(state.llm_error.get().is_some(), "5xx must set llm_error");
    }

    #[test]
    fn llm_path_stale_response_is_dropped_after_cancel() {
        // Design §5/§7: after an Esc cancel bumps `llm_seq`, the in-flight
        // response carrying the old seq must be dropped — the cancelled
        // filter must never land in `llm_result`.
        let server = httpmock::MockServer::start();
        let _mock = mock_chat_content(
            &server,
            "{\"Series\":{\"UpDays\":{\"n\":5,\"min_pct\":3.0}}}",
        );
        let config = config_with_parquet_dir("/tmp/compass_test_nonexistent_llm_xyz".into());
        let state = Arc::new(SharedState::new("000001", "1d"));
        let egui_ctx = egui::Context::default();

        let (_work, _screener, _sepa, _index, llm_signal, _backend) = wire_backend(
            config,
            state.clone(),
            egui_ctx,
            Some(llm_config_at(&server)),
        );

        // Submit with seq 1, then cancel (Esc): seq bumps to 2, loading false.
        state.llm_seq.set(1);
        state.llm_loading.set(true);
        llm_signal
            .send(RunLlmRequest {
                prompt: "x".to_string(),
                seq: 1,
            })
            .expect("failed to send llm request");
        state.llm_seq.set(2);
        state.llm_loading.set(false);

        // Give the in-flight response time to arrive and be filtered by the
        // seq guard.
        std::thread::sleep(Duration::from_millis(300));

        assert!(
            state.llm_result.get().is_none(),
            "cancelled response must be dropped by the seq guard"
        );
        assert!(!state.llm_loading.get());
    }
}
