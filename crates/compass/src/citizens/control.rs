use crate::messages::AppMessage;
use crate::state::SharedState;
use egui_citizen::{Citizen, CitizenId, CitizenState};

/// Control panel citizen — symbol/timeframe input and fetch button.
///
/// Uses the outbox pattern: UI interactions push to `outbox` (a
/// `Vec<AppMessage>`), and the main loop drains it via
/// `std::mem::take`. No direct backend calls, no `Dynamic<T>` writes.
pub struct ControlCitizen {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    pub symbol_input: String,
    pub timeframe_input: String,
    pub outbox: Vec<AppMessage>,
}

impl Citizen for ControlCitizen {
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

impl ControlCitizen {
    /// Creates a new `ControlCitizen` primed with the given defaults.
    pub fn new(
        citizen_id: CitizenId,
        citizen_state: CitizenState,
        default_symbol: &str,
        default_timeframe: &str,
    ) -> Self {
        Self {
            citizen_id,
            citizen_state,
            symbol_input: default_symbol.to_string(),
            timeframe_input: default_timeframe.to_string(),
            outbox: Vec::new(),
        }
    }

    /// Renders the control panel UI.
    ///
    /// Reads `loading` and `error` from shared state reactively.
    /// Pushes `AppMessage::FetchBars` to the outbox when the user
    /// clicks "Fetch".
    pub fn show(&mut self, ui: &mut egui::Ui, state: &SharedState) {
        let loading = state.loading.get();
        let error = state.error.get();

        ui.horizontal(|ui| {
            ui.label("Symbol:");
            ui.text_edit_singleline(&mut self.symbol_input);

            ui.label("Timeframe:");
            egui::ComboBox::from_id_salt("timeframe_combo")
                .selected_text(&self.timeframe_input)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.timeframe_input, "1d".into(), "1d");
                });

            if ui.button("Fetch").clicked() {
                state.symbol.set(self.symbol_input.clone());
                state.timeframe.set(self.timeframe_input.clone());
                self.outbox.push(AppMessage::FetchBars);
            }
        });

        if loading {
            ui.label("Loading...");
        }

        if let Some(ref err) = error {
            ui.colored_label(egui::Color32::RED, err);
        }
    }
}
