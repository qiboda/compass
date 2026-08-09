use compass_i18n::t;
use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::icon_button::IconButton;
use compass_ui::widgets::section_title::SectionTitle;
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

    /// Show the panel: a title row (heading + entry count + export button)
    /// above the egui_lens log viewer. Returns `true` when the export
    /// button was clicked (the caller opens the save-file dialog).
    pub fn show(&mut self, ui: &mut egui::Ui, state: &SharedState, tokens: &ThemeTokens) -> bool {
        let count = state.log.get().log_count();
        let export_tooltip = t!("logger.export_tooltip");
        let title = t!("logger.title");
        let export =
            IconButton::new(tokens, egui_phosphor::regular::EXPORT).tooltip(&export_tooltip);
        let export_clicked = SectionTitle::new(tokens, &title)
            .count(count)
            .action(export)
            .show(ui)
            .unwrap_or(false);
        ui.add_space(tokens.spacing.sm);
        let logger = ReactiveEventLogger::new(&state.log);
        logger.show(ui);
        export_clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_i18n::t;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    /// Key-resolution test helper (plan T4): resolves a key through the
    /// shared compass-i18n dictionary.
    fn tr(key: &str) -> String {
        t!(key).to_string()
    }

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
        let tokens = ThemeTokens::dark();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &tokens);
        });
        harness.run();
    }

    #[test]
    fn show_renders_title_row_with_count_and_export_button() {
        let id = CitizenId::new("logger");
        let state = CitizenState::new();
        let mut panel = LoggerPanel::new(id, state);

        let shared = SharedState::new("000001", "1d");
        let tokens = ThemeTokens::dark();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &tokens);
        });
        harness.run();
        let _ = harness.get_by_label(&tr("logger.title"));
        let _ = harness.get_by_label("0");
        let _ = harness.get_by_label(egui_phosphor::regular::EXPORT);
    }
}
