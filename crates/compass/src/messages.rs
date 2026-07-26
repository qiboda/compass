use chrono::{DateTime, Utc};
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
