use compass_types::{Filter, IndexSnapshot, ScreenerRow, SepaData};
use egui_charts::model::Bar;
use egui_lens::ReactiveEventLoggerState;
use egui_mobius_reactive::Dynamic;

/// Reactive shared state for the egui-mobius citizen app.
///
/// All fields are `Dynamic<T>` — the egui-mobius runtime wires
/// them to widgets automatically. Callers read/write via
/// `get()`, `set()`, and `subscribe()`.
pub struct SharedState {
    /// Currently displayed stock symbol (e.g. "SZ000001", "SH600519").
    pub symbol: Dynamic<String>,
    /// Current timeframe (e.g. "1d", "1w", "1M").
    pub timeframe: Dynamic<String>,
    /// Current price adjustment mode (复权方式): "qfq"/"hfq"/"none".
    pub adjust: Dynamic<String>,
    /// OHLCV bars for the current chart.
    pub bars: Dynamic<Vec<Bar>>,
    /// `true` while a data fetch is in flight.
    pub loading: Dynamic<bool>,
    /// Last error message, if any.
    pub error: Dynamic<Option<String>>,
    /// Logger state for the egui_lens reactive event logger panel.
    pub log: Dynamic<ReactiveEventLoggerState>,
    /// Latest screener result rows.
    pub screener_result: Dynamic<Vec<ScreenerRow>>,
    /// Total matches before the 100-row cap.
    pub screener_total: Dynamic<usize>,
    /// `true` while a screener run is in flight.
    pub screener_loading: Dynamic<bool>,
    /// Last screener error message, if any.
    pub screener_error: Dynamic<Option<String>>,
    /// `true` while an LLM condition generation is in flight.
    pub llm_loading: Dynamic<bool>,
    /// Last LLM generation error message, if any.
    pub llm_error: Dynamic<Option<String>>,
    /// Pending LLM-generated filter, consumed by the screener panel on the
    /// loading → idle transition (`None` after consumption or on failure).
    pub llm_result: Dynamic<Option<Filter>>,
    /// Natural-language prompt draft — kept in shared state so the draft
    /// survives tab switches and panel rebuilds (design §3).
    pub llm_input: Dynamic<String>,
    /// Latest request sequence — bumped on every send and on Esc-cancel so
    /// stale backend responses are dropped (design §3/§5).
    pub llm_seq: Dynamic<u64>,
    /// Latest SEPA scoring snapshot (rows + thermometer in one `Option` so
    /// the panel never observes a half-updated state).
    pub sepa_data: Dynamic<Option<SepaData>>,
    /// `true` while a SEPA run is in flight.
    pub sepa_loading: Dynamic<bool>,
    /// Last SEPA error message, if any.
    pub sepa_error: Dynamic<Option<String>>,
    /// Latest index/board market snapshot (epic #255 C4) — mirrors `sepa_data`.
    pub index_snapshot: Dynamic<Option<IndexSnapshot>>,
    /// `true` while an index snapshot run is in flight.
    pub index_snapshot_loading: Dynamic<bool>,
    /// Last index snapshot error message, if any.
    pub index_snapshot_error: Dynamic<Option<String>>,
    /// Industry zh→en name map (epic #266 B3f): built from `stock_basic`
    /// (`industry` → `industry_en`); the screener industry dropdown labels
    /// resolve through it while the stored filter keys stay Chinese.
    pub industry_names: Dynamic<std::collections::HashMap<String, String>>,
    /// Watchlist (自选股) — exchange-prefixed symbols in display order.
    pub watchlist: Dynamic<Vec<String>>,
}

impl SharedState {
    /// Creates a new `SharedState` with the given defaults.
    ///
    /// All fields are initialized to sensible defaults — empty bars, not
    /// loading, no error, and an empty log. `default_adjust` seeds the price
    /// adjustment mode ("qfq"/"hfq"/"none", default "qfq").
    pub fn new(default_symbol: &str, default_timeframe: &str, default_adjust: &str) -> Self {
        Self {
            symbol: Dynamic::new(default_symbol.to_string()),
            timeframe: Dynamic::new(default_timeframe.to_string()),
            adjust: Dynamic::new(default_adjust.to_string()),
            bars: Dynamic::new(Vec::new()),
            loading: Dynamic::new(false),
            error: Dynamic::new(None),
            log: Dynamic::new(ReactiveEventLoggerState::new()),
            screener_result: Dynamic::new(Vec::new()),
            screener_total: Dynamic::new(0),
            screener_loading: Dynamic::new(false),
            screener_error: Dynamic::new(None),
            llm_loading: Dynamic::new(false),
            llm_error: Dynamic::new(None),
            llm_result: Dynamic::new(None),
            llm_input: Dynamic::new(String::new()),
            llm_seq: Dynamic::new(0),
            sepa_data: Dynamic::new(None),
            sepa_loading: Dynamic::new(false),
            sepa_error: Dynamic::new(None),
            index_snapshot: Dynamic::new(None),
            index_snapshot_loading: Dynamic::new(false),
            index_snapshot_error: Dynamic::new(None),
            industry_names: Dynamic::new(std::collections::HashMap::new()),
            watchlist: Dynamic::new(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Issue #345 — `SharedState::new` takes a default_adjust param and seeds
    // `adjust` with it; "qfq" is the documented default.
    // -----------------------------------------------------------------------

    #[test]
    fn shared_state_new_seeds_adjust_from_param() {
        assert_eq!(
            SharedState::new("SZ000001", "1d", "qfq").adjust.get(),
            "qfq"
        );
        assert_eq!(
            SharedState::new("SZ000001", "1d", "hfq").adjust.get(),
            "hfq"
        );
        assert_eq!(
            SharedState::new("SZ000001", "1d", "none").adjust.get(),
            "none"
        );
    }

    /// Adversarial: the seed must arrive via the param, not via a hard-coded
    /// "qfq" constant that ignores the caller's configured default.
    #[test]
    fn shared_state_new_adjust_reflects_caller_default_not_hardcoded() {
        let state = SharedState::new("SH600519", "1M", "hfq");
        assert_eq!(state.adjust.get(), "hfq");
        assert_eq!(state.timeframe.get(), "1M");
        assert_eq!(state.symbol.get(), "SH600519");
    }
}
