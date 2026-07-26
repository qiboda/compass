use egui_citizen::{Citizen, CitizenId, CitizenState};

use crate::state::SharedState;

/// Logger panel citizen — displays accumulated log entries.
///
/// Reads from `SharedState::log` (a reactive `Dynamic<ReactiveEventLoggerState>`) and
/// renders entries in a scrollable area. When empty, shows a placeholder.
///
/// Note: egui_lens 0.5.0 has a panic bug in `ReactiveEventLogger::show()` on empty
/// state (logger.rs:271 — removal index out of bounds). The `ReactiveEventLogger` API
/// is still used for writing (log_info, log_error, log_custom), but rendering falls
/// back to a simple ScrollArea + label loop.
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
        let logger_state = state.log.get();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if logger_state.logs.is_empty() {
                ui.label("No log entries yet.");
            } else {
                for entry in &logger_state.logs {
                    ui.label(format!("{:?}", entry.log_message));
                }
            }
        });
    }
}
