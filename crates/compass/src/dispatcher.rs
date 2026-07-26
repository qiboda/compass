//! Dispatcher wiring: register citizens at startup, drain lifecycle
//! messages each frame, route AppMessage events to the work signal.

use chrono::Utc;
use egui_citizen::{CitizenId, CitizenState, Dispatcher};
use egui_lens::ReactiveEventLogger;
use egui_mobius::signals::Signal;

use crate::messages::{AppMessage, FetchRequest};
use crate::state::SharedState;
use crate::tabs::{CHART_ID, CONTROL_ID, LOGGER_ID};

/// Holds the `CitizenState` handles returned during registration.
///
/// Each handle can be cloned and handed to the corresponding citizen
/// panel so they share a single reactive lifecycle state.
pub struct RegisteredCitizens {
    pub control: CitizenState,
    pub chart: CitizenState,
    pub logger: CitizenState,
}

/// Register the three core citizens with the dispatcher and activate the
/// chart panel (one-hot: exactly one citizen is active at a time).
///
/// Returns the `CitizenState` handles so callers can construct the citizen
/// panel structs with the same reactive state that the dispatcher manages.
pub fn register_citizens(dispatcher: &mut Dispatcher) -> RegisteredCitizens {
    let control = dispatcher.register(CitizenId::new(CONTROL_ID));
    let chart = dispatcher.register(CitizenId::new(CHART_ID));
    let logger = dispatcher.register(CitizenId::new(LOGGER_ID));

    dispatcher.activate(&CitizenId::new(CHART_ID));

    RegisteredCitizens {
        control,
        chart,
        logger,
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
pub fn handle(msg: AppMessage, state: &SharedState, work_signal: &Signal<FetchRequest>) {
    match msg {
        AppMessage::FetchBars => {
            let symbol = state.symbol.get();
            let timeframe = state.timeframe.get();

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
