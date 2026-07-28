use egui_charts::model::Bar;
use egui_lens::ReactiveEventLoggerState;
use egui_mobius_reactive::Dynamic;

/// Reactive shared state for the egui-mobius citizen app.
///
/// All fields are `Dynamic<T>` — the egui-mobius runtime wires
/// them to widgets automatically. Callers read/write via
/// `get()`, `set()`, and `subscribe()`.
pub struct SharedState {
    /// Currently displayed stock symbol (e.g. "000001", "600519").
    pub symbol: Dynamic<String>,
    /// Current timeframe (e.g. "1d", "1w", "1M").
    pub timeframe: Dynamic<String>,
    /// OHLCV bars for the current chart.
    pub bars: Dynamic<Vec<Bar>>,
    /// `true` while a data fetch is in flight.
    pub loading: Dynamic<bool>,
    /// Last error message, if any.
    pub error: Dynamic<Option<String>>,
    /// Logger state for the egui_lens reactive event logger panel.
    pub log: Dynamic<ReactiveEventLoggerState>,
}

impl SharedState {
    /// Creates a new `SharedState` with the given default symbol.
    ///
    /// All fields are initialized to sensible defaults — empty bars, not
    /// loading, no error, and an empty log.
    pub fn new(default_symbol: &str, default_timeframe: &str) -> Self {
        Self {
            symbol: Dynamic::new(default_symbol.to_string()),
            timeframe: Dynamic::new(default_timeframe.to_string()),
            bars: Dynamic::new(Vec::new()),
            loading: Dynamic::new(false),
            error: Dynamic::new(None),
            log: Dynamic::new(ReactiveEventLoggerState::new()),
        }
    }
}
