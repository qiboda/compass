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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_citizen::CitizenState;

    #[test]
    fn new_creates_panel_with_correct_id() {
        let id = CitizenId::new("test_logger");
        let state = CitizenState::new();
        let panel = LoggerPanel::new(id.clone(), state.clone());

        assert_eq!(panel.citizen_id, id);
        assert_eq!(panel.id(), &id);
    }

    #[test]
    fn new_creates_panel() {
        let id = CitizenId::new("logger");
        let state = CitizenState::new();
        let panel = LoggerPanel::new(id, state);

        assert_eq!(*panel.id(), CitizenId::new("logger"));
    }

    #[test]
    fn show_no_panic() {
        let id = CitizenId::new("logger");
        let state = CitizenState::new();
        let mut panel = LoggerPanel::new(id, state);

        let shared = SharedState::new("000001", "1d");

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared);
        });
        harness.run();
    }
}
