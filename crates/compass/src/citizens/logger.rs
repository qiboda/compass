use compass_i18n::t;
use compass_ui::widgets::icon_button::IconButton;
use compass_ui::widgets::section_title::SectionTitle;
use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_lens::ReactiveEventLogger;

use crate::editor::{EditorCtx, EditorKind, EditorView};

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

    #[cfg(test)]
    /// Test-only stand-in for the old combined render (plan §4.5): the
    /// production path now renders header + body through `EditorFrame`;
    /// this keeps the old signature so existing kittest harnesses stay
    /// untouched (same pattern as 2b/2c/2d stand-ins). The `tokens`
    /// argument is ignored on purpose — `EditorCtx.theme` is built from
    /// `CompassTheme::compass_dark()`, whose token values are the dark
    /// variant the tests render with; queries only assert label presence.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        state: &crate::state::SharedState,
        _tokens: &compass_ui::tokens::ThemeTokens,
    ) -> bool {
        let theme = crate::theme::CompassTheme::compass_dark();
        let (work_signal, _work_slot) =
            egui_mobius::factory::create_signal_slot::<crate::messages::FetchRequest>();
        let (screener_signal, _screener_slot) =
            egui_mobius::factory::create_signal_slot::<crate::messages::RunScreenerRequest>();
        let (sepa_signal, _sepa_slot) =
            egui_mobius::factory::create_signal_slot::<crate::messages::RunSepaRequest>();
        let (index_signal, _index_slot) =
            egui_mobius::factory::create_signal_slot::<crate::messages::RunIndexSnapshotRequest>();
        let (llm_signal, _llm_slot) =
            egui_mobius::factory::create_signal_slot::<crate::messages::RunLlmRequest>();
        let desc = crate::editor::EDITOR_REGISTRY
            .iter()
            .find(|d| d.kind == EditorKind::Logger)
            .expect("logger descriptor must exist in EDITOR_REGISTRY");
        let mut logger_export_clicked = false;
        let mut chart_action = None;
        let mut toasts = compass_ui::widgets::toast::ToastManager::new(*theme.tokens());
        let mut watchlist_action = None;
        let mut sidebar_toggle_requested = false;
        let mut ctx = EditorCtx {
            state,
            theme: &theme,
            signals: &crate::editor::EditorSignals {
                work: &work_signal,
                screener: &screener_signal,
                sepa: &sepa_signal,
                index: &index_signal,
                llm: &llm_signal,
            },
            index_list: &[],
            chart_action: &mut chart_action,
            screener_industries: &[],
            screener_boards: &[],
            logger_export_clicked: &mut logger_export_clicked,
            toasts: &mut toasts,
            stock_list: &[],
            watchlist_action: &mut watchlist_action,
            sidebar_toggle_requested: &mut sidebar_toggle_requested,
        };
        let mut frame = crate::editor::EditorFrame {
            sidebar_visible: false,
        };
        frame.show(ui, desc, self, &mut ctx);
        logger_export_clicked
    }
}

impl EditorView for LoggerPanel {
    fn kind(&self) -> EditorKind {
        EditorKind::Logger
    }

    /// Header (plan §4.5 / design §6 Logger 行): the existing
    /// `SectionTitle` row — heading + entry count left, export icon button
    /// right. The click is reported through the `logger_export_clicked`
    /// out-param; the App opens the save-file dialog after the frame pass
    /// (main.rs unchanged flow).
    fn header(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {
        let tokens = *ctx.theme.tokens();
        let count = ctx.state.log.get().log_count();
        let export_tooltip = t!("logger.export_tooltip");
        let title = t!("logger.title");
        let export =
            IconButton::new(&tokens, egui_phosphor::regular::EXPORT).tooltip(&export_tooltip);
        *ctx.logger_export_clicked = SectionTitle::new(&tokens, &title)
            .count(count)
            .action(export)
            .show(ui)
            .unwrap_or(false);
    }

    /// Body: the egui_lens terminal-like log viewer.
    fn body(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {
        let logger = ReactiveEventLogger::new(&ctx.state.log);
        logger.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use crate::state::SharedState;
    use compass_i18n::t;
    use compass_ui::tokens::ThemeTokens;
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("logger");
        let state = CitizenState::new();
        let mut panel = LoggerPanel::new(id, state);

        let shared = SharedState::new("000001", "1d", "qfq");
        let tokens = ThemeTokens::dark();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &tokens);
        });
        harness.run();
    }

    #[test]
    fn show_renders_title_row_with_count_and_export_button() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("logger");
        let state = CitizenState::new();
        let mut panel = LoggerPanel::new(id, state);

        let shared = SharedState::new("000001", "1d", "qfq");
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
