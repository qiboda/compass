use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_lens::ReactiveEventLogger;

use crate::state::SharedState;

/// Logger panel citizen — powered by egui_lens.
///
/// Wraps `ReactiveEventLogger` which provides a terminal-like log viewer
/// with column toggles (timestamps, log levels, messages), filtering,
/// color-coded entries, and export-to-file.
pub struct LoggerPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
}

impl Citizen for LoggerPanel {
    fn id(&self) -> &CitizenId {
        &self.citizen_id
    }

    fn citizen_state(&self) -> &CitizenState {
        &self.citizen_state
    }

    fn citizen_state_mut(&mut self) -> &mut CitizenState {
        &mut self.citizen_state
    }
}

impl LoggerPanel {
    pub fn new(citizen_id: CitizenId, citizen_state: CitizenState) -> Self {
        Self {
            citizen_id,
            citizen_state,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, state: &SharedState) {
        let logger = ReactiveEventLogger::new(&state.log);
        logger.show(ui);
    }
}
