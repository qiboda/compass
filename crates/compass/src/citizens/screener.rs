//! Screener panel citizen — condition input + results table (Waves 3-5).

use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use crate::messages::{FetchRequest, RunScreenerRequest};
use crate::state::SharedState;

/// Screener panel citizen.
///
/// Holds the reactive lifecycle state required by the citizen pattern and
/// renders the condition form and results table. The heavy lifting (filter
/// evaluation) runs on the backend via `run_screener_signal`.
pub struct ScreenerPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
}

impl Citizen for ScreenerPanel {
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

impl ScreenerPanel {
    /// Create a screener panel with the given citizen identity/state.
    pub fn new(citizen_id: CitizenId, citizen_state: CitizenState) -> Self {
        Self {
            citizen_id,
            citizen_state,
        }
    }

    /// Render the panel: condition form (Todo 5) and results table (Todo 6).
    ///
    /// The signal handles and derived industry/board lists are provided by
    /// the app so the panel stays decoupled from the backend.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        _shared_state: &SharedState,
        _run_screener_signal: &Signal<RunScreenerRequest>,
        _work_signal: &Signal<FetchRequest>,
        _industries: &[String],
        _boards: &[String],
    ) {
        ui.label("Screener (under construction)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_citizen::CitizenState;

    #[test]
    fn new_creates_panel_with_correct_id() {
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let panel = ScreenerPanel::new(id.clone(), state.clone());

        assert_eq!(panel.citizen_id, id);
        assert_eq!(panel.id(), &id);
    }

    #[test]
    fn show_no_panic() {
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let mut panel = ScreenerPanel::new(id, state);

        let shared = SharedState::new("000001", "1d");
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries = vec!["银行".to_string()];
        let boards = vec!["主板".to_string()];

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.run();
    }
}
