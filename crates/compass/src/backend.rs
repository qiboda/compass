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

use egui_lens::ReactiveEventLogger;
use egui_mobius::dispatching::AsyncDispatcher;
use egui_mobius::factory;
use egui_mobius::signals::Signal;

use compass_core::data::{duckdb::DuckDbProvider, provider::DataProvider};
use compass_core::model::AppConfig;

use crate::messages::{FetchRequest, FetchResponse};
use crate::state::SharedState;

/// Owns the `AsyncDispatcher` so the Tokio runtime stays alive for the
/// program duration. Dropping this shuts down the runtime.
pub struct BackendHandle {
    _dispatcher: AsyncDispatcher<FetchRequest, FetchResponse>,
}

/// Build the async work pipeline. Returns:
/// - `Signal<FetchRequest>` — UI thread submits work via `.send(req)`
/// - `BackendHandle` — keep alive on the App struct
///
/// The result slot is started internally — it writes response values
/// into the reactive `SharedState` and requests a UI repaint.
pub fn wire_backend(
    config: AppConfig,
    state: Arc<SharedState>,
    egui_ctx: egui::Context,
) -> (Signal<FetchRequest>, BackendHandle) {
    let (work_signal, work_slot) = factory::create_signal_slot::<FetchRequest>();
    let (result_signal, mut result_slot) = factory::create_signal_slot::<FetchResponse>();

    let parquet_dir = std::path::PathBuf::from(&config.database.parquet_dir);

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
        egui_ctx.request_repaint();
    });

    (
        work_signal,
        BackendHandle {
            _dispatcher: dispatcher,
        },
    )
}
