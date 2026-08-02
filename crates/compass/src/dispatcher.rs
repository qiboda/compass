//! Dispatcher wiring: register citizens at startup, drain lifecycle
//! messages each frame, route AppMessage events to the work signal.

use chrono::Utc;
use egui_citizen::{CitizenId, CitizenState, Dispatcher};
use egui_lens::ReactiveEventLogger;
use egui_mobius::signals::Signal;

use crate::messages::{AppMessage, FetchRequest};
use crate::state::SharedState;
use crate::tabs::{CHART_ID, LOGGER_ID, SCREENER_ID, SEPA_ID};

/// Holds the `CitizenState` handles returned during registration.
///
/// Each handle can be cloned and handed to the corresponding citizen
/// panel so they share a single reactive lifecycle state.
pub struct RegisteredCitizens {
    pub chart: CitizenState,
    pub logger: CitizenState,
    pub screener: CitizenState,
    pub sepa: CitizenState,
}

/// Register the core citizens with the dispatcher and activate the
/// chart panel (one-hot: exactly one citizen is active at a time).
///
/// Returns the `CitizenState` handles so callers can construct the citizen
/// panel structs with the same reactive state that the dispatcher manages.
pub fn register_citizens(dispatcher: &mut Dispatcher) -> RegisteredCitizens {
    let chart = dispatcher.register(CitizenId::new(CHART_ID));
    let logger = dispatcher.register(CitizenId::new(LOGGER_ID));
    let screener = dispatcher.register(CitizenId::new(SCREENER_ID));
    let sepa = dispatcher.register(CitizenId::new(SEPA_ID));

    dispatcher.activate(&CitizenId::new(CHART_ID));

    RegisteredCitizens {
        chart,
        logger,
        screener,
        sepa,
    }
}

/// Drain citizen lifecycle messages from the dispatcher and append them
/// to the shared log.
///
/// Call once per frame after `DockArea::show()` returns. Messages are
/// consumed — calling again returns an empty vec until new messages are
/// produced by tab clicks or other lifecycle events.
pub fn drain_citizen(dispatcher: &mut Dispatcher, state: &SharedState) {
    let logger = ReactiveEventLogger::new(&state.log);
    for msg in dispatcher.drain_messages() {
        logger.log_custom("citizen", &format!("{msg:?}"));
    }
}

/// Route an application-level message.
///
/// `FetchBars` snapshots the current symbol and timeframe from shared
/// state, constructs a `FetchRequest` with a 365-day range, pushes it onto
/// the work signal bus, and sets the loading indicator.
pub fn handle(
    msg: AppMessage,
    state: &SharedState,
    work_signal: &Signal<FetchRequest>,
    timeframe: String,
) {
    match msg {
        AppMessage::FetchBars => {
            let symbol = state.symbol.get();

            let request = FetchRequest {
                symbol,
                timeframe,
                range_start: Utc::now() - chrono::Duration::days(365),
                range_end: Utc::now(),
            };

            state.loading.set(true);
            state.error.set(None);

            if let Err(e) = work_signal.send(request) {
                let logger = ReactiveEventLogger::new(&state.log);
                logger.log_error(&format!("work_signal.send failed: {e}"));
                state.loading.set(false);
            }
        }
    }
}

/// Core row-click linkage shared by the screener and SEPA panels: set the
/// shared symbol, then dispatch a `FetchBars` request with the current
/// timeframe so the chart switches to the clicked stock.
pub fn dispatch_symbol_fetch(
    shared_state: &SharedState,
    work_signal: &Signal<FetchRequest>,
    symbol: &str,
) {
    shared_state.symbol.set(symbol.to_string());
    let timeframe = shared_state.timeframe.get();
    handle(AppMessage::FetchBars, shared_state, work_signal, timeframe);
}

// ===========================================================================
// Tests — ref #79
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{AppMessage, FetchRequest};
    use crate::state::SharedState;
    use crate::tabs::{CHART_ID, LOGGER_ID};
    use egui_citizen::{CitizenId, Dispatcher};
    use egui_mobius::factory;

    // ------------------------------------------------------------------
    // register_citizens
    // ------------------------------------------------------------------

    #[test]
    fn register_citizens_registers_chart_and_logger_and_activates_chart() {
        let mut dispatcher = Dispatcher::new();
        let registered = register_citizens(&mut dispatcher);

        let chart_state = dispatcher
            .get(&CitizenId::new(CHART_ID))
            .expect("chart citizen should be registered");
        let logger_state = dispatcher
            .get(&CitizenId::new(LOGGER_ID))
            .expect("logger citizen should be registered");
        let screener_state = dispatcher
            .get(&CitizenId::new(SCREENER_ID))
            .expect("screener citizen should be registered");
        let sepa_state = dispatcher
            .get(&CitizenId::new(SEPA_ID))
            .expect("sepa citizen should be registered");

        // Chart is active (one-hot), logger is inactive.
        assert!(chart_state.active.get(), "chart should be active");
        assert!(
            !logger_state.active.get(),
            "logger should be inactive after register_citizens"
        );
        assert!(
            !screener_state.active.get(),
            "screener should be inactive after register_citizens"
        );
        assert!(
            !sepa_state.active.get(),
            "sepa should be inactive after register_citizens"
        );

        // The returned handles share the same reactive state.
        assert_eq!(registered.chart.active.get(), chart_state.active.get());
        assert_eq!(registered.logger.active.get(), logger_state.active.get());
        assert_eq!(
            registered.screener.active.get(),
            screener_state.active.get()
        );
        assert_eq!(registered.sepa.active.get(), sepa_state.active.get());
    }

    // ------------------------------------------------------------------
    // drain_citizen
    // ------------------------------------------------------------------

    #[test]
    fn drain_citizen_appends_lifecycle_messages_to_log() {
        let mut dispatcher = Dispatcher::new();
        let _registered = register_citizens(&mut dispatcher);
        let state = SharedState::new("000001", "1d");

        assert_eq!(state.log.get().log_count(), 0, "log should start empty");

        // register_citizens activates "chart", which queues an Activated message.
        drain_citizen(&mut dispatcher, &state);

        let count = state.log.get().log_count();
        assert!(
            count > 0,
            "drain_citizen should flush lifecycle messages into the log, got {count}"
        );
    }

    // ------------------------------------------------------------------
    // handle — happy path
    // ------------------------------------------------------------------

    #[test]
    fn handle_fetch_bars_sends_request_and_sets_loading() {
        let state = SharedState::new("000001", "1d");
        let (signal, slot) = factory::create_signal_slot::<FetchRequest>();

        assert!(!state.loading.get(), "loading should start false");

        handle(AppMessage::FetchBars, &state, &signal, "1w".to_string());

        assert!(
            state.loading.get(),
            "loading should be true after fetch dispatch"
        );
        assert_eq!(
            state.error.get(),
            None,
            "error should be cleared on new fetch"
        );

        // The request is on the slot's receiver — read it back.
        let request = slot
            .receiver
            .lock()
            .unwrap()
            .recv()
            .expect("slot should receive the FetchRequest");

        assert_eq!(request.symbol, "000001");
        assert_eq!(request.timeframe, "1w");
    }

    // ------------------------------------------------------------------
    // handle — failure path (slot dropped → send fails)
    // ------------------------------------------------------------------

    #[test]
    fn handle_fetch_bars_resets_loading_on_send_failure() {
        let state = SharedState::new("600519", "1d");
        let (signal, slot) = factory::create_signal_slot::<FetchRequest>();

        // Drop the slot so the receiver is gone — send will fail.
        drop(slot);

        state.loading.set(true);
        assert_eq!(state.log.get().log_count(), 0, "log should start empty");

        handle(AppMessage::FetchBars, &state, &signal, "1M".to_string());

        // On failure handle resets loading and logs the error.
        assert!(
            !state.loading.get(),
            "loading should be reset to false when send fails"
        );

        let log_count = state.log.get().log_count();
        assert!(
            log_count > 0,
            "log should contain the send-failure error message (got {log_count})"
        );
    }

    // ------------------------------------------------------------------
    // dispatch_symbol_fetch — shared row-click linkage (SEPA + screener)
    // ------------------------------------------------------------------

    #[test]
    fn dispatch_symbol_fetch_sets_symbol_and_triggers_fetch() {
        let state = SharedState::new("000001", "1d");
        let (work_signal, _work_slot) = factory::create_signal_slot::<FetchRequest>();

        dispatch_symbol_fetch(&state, &work_signal, "600519");

        assert_eq!(state.symbol.get(), "600519");
        assert!(
            state.loading.get(),
            "symbol fetch must dispatch a FetchBars request"
        );
    }
}
