use chrono::{DateTime, Utc};
use compass_types::{Filter, ScreenerRow};
use egui_charts::model::Bar;

/// Application-level message for egui-mobius citizen pattern.
///
/// Typed signals are used per direction (`work_signal` / `result_slot`)
/// rather than routing through this enum. It exists as a namespace anchor
/// and for potential future single-enum routing.
pub enum AppMessage {
    FetchBars,
}

/// Sent from the UI to the backend worker via `work_signal.send(req)`.
#[derive(Clone)]
pub struct FetchRequest {
    pub symbol: String,
    pub timeframe: String,
    pub range_start: DateTime<Utc>,
    pub range_end: DateTime<Utc>,
}

/// Sent from the backend worker to the UI via `result_slot.start(handler)`.
#[derive(Clone)]
pub struct FetchResponse {
    pub bars: Vec<Bar>,
    pub error: Option<String>,
}

/// Sent from the screener panel to the backend via the screener signal.
///
/// Carries the Batch 1 `Filter` AST directly (epic #243) — the backend no
/// longer compiles a legacy `ScreenerQuery` on its side.
#[derive(Clone)]
pub struct RunScreenerRequest {
    pub filter: Filter,
}

/// Sent from the backend worker back to the UI after a screener run.
#[derive(Clone)]
pub struct RunScreenerResponse {
    pub rows: Vec<ScreenerRow>,
    /// Total matches before the 100-row cap (for the "共 N 只" label).
    pub total: usize,
    pub error: Option<String>,
}

/// Sent from the SEPA panel to the backend via the sepa signal. No payload —
/// the TOP-N cap is pure GUI state and never triggers a backend recompute.
#[derive(Clone)]
pub struct RunSepaRequest {}

/// Sent from the backend worker back to the UI after a SEPA run.
#[derive(Clone)]
pub struct RunSepaResponse {
    /// Full TOP-N scoring snapshot (rows + thermometer in one payload so the
    /// panel never observes a half-updated state).
    pub data: compass_types::SepaData,
    pub error: Option<String>,
}

/// Sent from the market panel to the backend via the index signal. No
/// payload — the snapshot always covers the full `index_daily.parquet`.
#[derive(Clone)]
pub struct RunIndexSnapshotRequest {}

/// Sent from the backend worker back to the UI after an index snapshot run.
#[derive(Clone)]
pub struct RunIndexSnapshotResponse {
    /// Latest-quote snapshot of every index/board symbol; `rows` is empty
    /// when `index_daily.parquet` is missing (the panel shows the empty
    /// state instead of an error — plan T6).
    pub data: compass_types::IndexSnapshot,
    pub error: Option<String>,
}
