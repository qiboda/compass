//! egui_dock TabViewer bridge for the citizen pattern.
//!
//! Each tab is a [`Tab`] wrapping a [`TabKind`] variant. When a tab button is
//! clicked, the `on_tab_button` hook calls
//! [`Dispatcher::activate`] with the tab's [`CitizenId`], enabling one-hot
//! panel activation across the dock layout.
//!
//! ## Usage
//!
//! ```text
//! let mut dock_state = egui_dock::DockState::new(vec![
//!     Tab::new(TabKind::Chart),
//!     Tab::new(TabKind::Logger),
//! ]);
//! let mut tab_viewer = TabViewer {
//!     dispatcher: &mut dispatcher,
//!     chart: &mut (),
//!     logger: &mut (),
//! };
//! egui_dock::DockArea::new(&mut dock_state).show_inside(ui, &mut tab_viewer);
//! for msg in tab_viewer.dispatcher.drain_messages() { /* ... */ }
//! ```

use egui_citizen::{CitizenId, Dispatcher};
use egui_mobius::signals::Signal;

use crate::citizens::chart::ChartCitizen;
use crate::citizens::logger::LoggerPanel;
use crate::citizens::market::MarketPanel;
use crate::citizens::screener::ScreenerPanel;
use crate::citizens::sepa::SepaPanel;
use crate::messages::{
    FetchRequest, RunIndexSnapshotRequest, RunLlmRequest, RunScreenerRequest, RunSepaRequest,
};
use crate::state::SharedState;
use compass_i18n::t;

// ---------------------------------------------------------------------------
// Citizen ID constants
// ---------------------------------------------------------------------------

pub const CHART_ID: &str = "chart";
pub const LOGGER_ID: &str = "logger";
pub const SCREENER_ID: &str = "screener";
pub const SEPA_ID: &str = "sepa";
pub const MARKET_ID: &str = "market";

// ---------------------------------------------------------------------------
// TabKind — enum of dockable panel types
// ---------------------------------------------------------------------------

/// Identifies which kind of panel a tab represents.
///
/// Maps 1:1 to citizen IDs — each variant has a fixed [`CitizenId`] that
/// the dispatcher uses for one-hot activation tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Chart,
    Logger,
    Screener,
    Sepa,
    Market,
}

impl TabKind {
    /// i18n key of the tab's display title. The rendering consumer
    /// ([`Tab::title`] via the egui_dock `TabViewer`) resolves it via `t!()`
    /// so a live locale switch updates the dock tabs (issue #222, plan T5
    /// Metis M2).
    pub fn title(&self) -> &'static str {
        match self {
            Self::Chart => "tab.chart",
            Self::Logger => "tab.logger",
            Self::Screener => "tab.screener",
            Self::Sepa => "tab.sepa",
            Self::Market => "tab.market",
        }
    }

    /// Phosphor icon glyph shown next to the tab title (design doc §Q2).
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Chart => egui_phosphor::regular::CHART_LINE,
            Self::Logger => egui_phosphor::regular::TERMINAL,
            Self::Screener => egui_phosphor::regular::FUNNEL_SIMPLE,
            Self::Sepa => egui_phosphor::regular::GAUGE,
            Self::Market => egui_phosphor::regular::TREND_UP,
        }
    }

    pub fn citizen_id(&self) -> CitizenId {
        match self {
            Self::Chart => CitizenId::new(CHART_ID),
            Self::Logger => CitizenId::new(LOGGER_ID),
            Self::Screener => CitizenId::new(SCREENER_ID),
            Self::Sepa => CitizenId::new(SEPA_ID),
            Self::Market => CitizenId::new(MARKET_ID),
        }
    }
}

// ---------------------------------------------------------------------------
// Tab — wraps TabKind for egui_dock
// ---------------------------------------------------------------------------

/// A dockable tab carrying its [`TabKind`].
///
/// Used as `DockState<Tab>` and `TabViewer::Tab = Tab` in egui_dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    kind: TabKind,
}

impl Tab {
    /// Create a tab of the given kind.
    pub fn new(kind: TabKind) -> Self {
        Self { kind }
    }

    /// Human-readable title for the tab bar.
    pub fn title(&self) -> &'static str {
        self.kind.title()
    }

    /// The [`CitizenId`] this tab maps to in the dispatcher.
    pub fn citizen_id(&self) -> CitizenId {
        self.kind.citizen_id()
    }
}

// ---------------------------------------------------------------------------
// TabViewer — egui_dock bridge
// ---------------------------------------------------------------------------

use crate::theme::CompassTheme;

/// egui_dock [`TabViewer`] that bridges tab clicks to citizen activation
/// and delegates rendering to each citizen's `show` method.
///
/// Created inline each frame — the short-lived borrows satisfy egui_dock's
/// borrowing requirements.
pub struct TabViewer<'a> {
    pub dispatcher: &'a mut Dispatcher,
    pub chart: &'a mut ChartCitizen,
    pub logger: &'a mut LoggerPanel,
    pub screener: &'a mut ScreenerPanel,
    pub sepa: &'a mut SepaPanel,
    pub market: &'a mut MarketPanel,
    pub run_screener_signal: &'a Signal<RunScreenerRequest>,
    pub sepa_signal: &'a Signal<RunSepaRequest>,
    pub index_signal: &'a Signal<RunIndexSnapshotRequest>,
    pub llm_signal: &'a Signal<RunLlmRequest>,
    pub work_signal: &'a Signal<FetchRequest>,
    pub screener_industries: &'a [String],
    pub screener_boards: &'a [String],
    pub shared_state: &'a SharedState,
    pub theme: &'a CompassTheme,
    /// Out-param: set to `true` when the logger export button was clicked.
    pub logger_export_clicked: &'a mut bool,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.kind.icon(), t!(tab.title())).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.kind {
            TabKind::Chart => self.chart.show(ui, self.shared_state, self.theme),
            TabKind::Logger => {
                *self.logger_export_clicked =
                    self.logger.show(ui, self.shared_state, self.theme.tokens());
            }
            TabKind::Screener => self.screener.show(
                ui,
                self.shared_state,
                self.run_screener_signal,
                self.work_signal,
                self.screener_industries,
                self.screener_boards,
                self.llm_signal,
            ),
            TabKind::Sepa => {
                self.sepa
                    .show(ui, self.shared_state, self.sepa_signal, self.work_signal);
            }
            TabKind::Market => {
                self.market
                    .show(ui, self.shared_state, self.index_signal, self.work_signal);
            }
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if response.clicked() {
            self.dispatcher.activate(&tab.citizen_id());
        }
    }
}

// ===========================================================================
// Tests — ref #79 (pure-logic TabKind + Tab, no TabViewer rendering)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use compass_i18n::t;

    /// Key-resolution test helper (plan T4): resolves a key through the
    /// shared compass-i18n dictionary.
    fn tr(key: &str) -> String {
        t!(key).to_string()
    }

    // ------------------------------------------------------------------
    // TabKind::title
    // ------------------------------------------------------------------

    #[test]
    fn tab_kind_chart_title() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(tr(TabKind::Chart.title()), "图表");
    }

    #[test]
    fn tab_kind_logger_title() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(tr(TabKind::Logger.title()), "日志");
    }

    #[test]
    fn tab_kind_screener_title() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(tr(TabKind::Screener.title()), "选股器");
    }

    #[test]
    fn tab_kind_sepa_title() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(tr(TabKind::Sepa.title()), "东方SEPA");
    }

    // ------------------------------------------------------------------
    // #222 i18n (T5): `TabKind::title()` returns KEY CONSTANTS ("tab.chart"
    // etc.), not display text — the rendering consumer (TabViewer::title)
    // resolves them via t!() so a live locale switch updates the dock tabs.
    // RED now: title() still returns the zh literal.
    // ------------------------------------------------------------------

    #[test]
    fn tab_kind_titles_are_key_constants() {
        assert_eq!(TabKind::Chart.title(), "tab.chart");
        assert_eq!(TabKind::Logger.title(), "tab.logger");
        assert_eq!(TabKind::Screener.title(), "tab.screener");
        assert_eq!(TabKind::Sepa.title(), "tab.sepa");
    }

    #[test]
    fn tab_title_delegates_to_key_constant() {
        let tab = Tab::new(TabKind::Chart);
        assert_eq!(tab.title(), "tab.chart");
    }

    #[test]
    fn tab_kind_icons_are_phosphor_glyphs() {
        assert_eq!(TabKind::Chart.icon(), egui_phosphor::regular::CHART_LINE);
        assert_eq!(TabKind::Logger.icon(), egui_phosphor::regular::TERMINAL);
        assert_eq!(
            TabKind::Screener.icon(),
            egui_phosphor::regular::FUNNEL_SIMPLE
        );
        assert_eq!(TabKind::Sepa.icon(), egui_phosphor::regular::GAUGE);
    }

    // ------------------------------------------------------------------
    // TabKind::citizen_id
    // ------------------------------------------------------------------

    #[test]
    fn tab_kind_chart_citizen_id() {
        assert_eq!(TabKind::Chart.citizen_id(), CitizenId::new(CHART_ID));
    }

    #[test]
    fn tab_kind_logger_citizen_id() {
        assert_eq!(TabKind::Logger.citizen_id(), CitizenId::new(LOGGER_ID));
    }

    #[test]
    fn tab_kind_screener_citizen_id() {
        assert_eq!(TabKind::Screener.citizen_id(), CitizenId::new(SCREENER_ID));
    }

    #[test]
    fn tab_kind_sepa_citizen_id() {
        assert_eq!(TabKind::Sepa.citizen_id(), CitizenId::new(SEPA_ID));
    }

    // ------------------------------------------------------------------
    // Tab::new / Tab::title / Tab::citizen_id
    // ------------------------------------------------------------------

    #[test]
    fn tab_new_chart_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Chart);
        assert_eq!(tr(tab.title()), "图表");
        assert_eq!(tab.citizen_id(), CitizenId::new(CHART_ID));
    }

    #[test]
    fn tab_new_logger_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Logger);
        assert_eq!(tr(tab.title()), "日志");
        assert_eq!(tab.citizen_id(), CitizenId::new(LOGGER_ID));
    }

    #[test]
    fn tab_new_screener_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Screener);
        assert_eq!(tr(tab.title()), "选股器");
        assert_eq!(tab.citizen_id(), CitizenId::new(SCREENER_ID));
    }

    #[test]
    fn tab_new_sepa_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Sepa);
        assert_eq!(tr(tab.title()), "东方SEPA");
        assert_eq!(tab.citizen_id(), CitizenId::new(SEPA_ID));
    }

    #[test]
    fn tab_same_kind_are_equal() {
        assert_eq!(Tab::new(TabKind::Chart), Tab::new(TabKind::Chart));
        assert_eq!(Tab::new(TabKind::Logger), Tab::new(TabKind::Logger));
        assert_eq!(Tab::new(TabKind::Screener), Tab::new(TabKind::Screener));
        assert_eq!(Tab::new(TabKind::Sepa), Tab::new(TabKind::Sepa));
        assert_ne!(Tab::new(TabKind::Chart), Tab::new(TabKind::Logger));
        assert_ne!(Tab::new(TabKind::Chart), Tab::new(TabKind::Screener));
        assert_ne!(Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa));
    }

    // ------------------------------------------------------------------
    // TabViewer::title — icon + Chinese title (design §Q2)
    // ------------------------------------------------------------------
    //
    // egui_dock 0.20 paints tab buttons with `ui.interact` + painter, so the
    // labels are invisible to the accesskit tree — the rendered title is
    // asserted at this unit level instead of via kittest queries.

    #[test]
    fn tab_viewer_title_combines_icon_and_chinese_title() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        use crate::citizens::chart::ChartCitizen;
        use crate::citizens::logger::LoggerPanel;
        use crate::citizens::screener::ScreenerPanel;
        use crate::dispatcher::register_citizens;
        use crate::messages::{FetchRequest, RunScreenerRequest};
        use crate::state::SharedState;
        use crate::theme::CompassTheme;
        use egui_dock::TabViewer as _;
        use egui_mobius::factory;

        let mut dispatcher = Dispatcher::new();
        let registered = register_citizens(&mut dispatcher);
        let mut chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let mut logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let mut screener = ScreenerPanel::new(
            CitizenId::new(SCREENER_ID),
            registered.screener,
            None,
            Box::new(|_| {}),
            &compass_ui::tokens::ThemeTokens::dark(),
            false,
        );
        let mut sepa = SepaPanel::new(
            CitizenId::new(SEPA_ID),
            registered.sepa,
            &compass_ui::tokens::ThemeTokens::dark(),
        );
        let mut market = MarketPanel::new(
            CitizenId::new(MARKET_ID),
            registered.market,
            &compass_ui::tokens::ThemeTokens::dark(),
        );
        let (run_signal, _run_slot) = factory::create_signal_slot::<RunScreenerRequest>();
        let (sepa_signal, _sepa_slot) = factory::create_signal_slot::<RunSepaRequest>();
        let (index_signal, _index_slot) = factory::create_signal_slot::<RunIndexSnapshotRequest>();
        let (work_signal, _work_slot) = factory::create_signal_slot::<FetchRequest>();
        let (llm_signal, _llm_slot) = factory::create_signal_slot::<RunLlmRequest>();
        let shared = SharedState::new("000001", "1d", "qfq");
        let theme = CompassTheme::compass_dark();

        let mut logger_export_clicked = false;
        let mut viewer = TabViewer {
            dispatcher: &mut dispatcher,
            chart: &mut chart,
            logger: &mut logger,
            screener: &mut screener,
            sepa: &mut sepa,
            market: &mut market,
            run_screener_signal: &run_signal,
            sepa_signal: &sepa_signal,
            index_signal: &index_signal,
            llm_signal: &llm_signal,
            work_signal: &work_signal,
            screener_industries: &[],
            screener_boards: &[],
            shared_state: &shared,
            theme: &theme,
            logger_export_clicked: &mut logger_export_clicked,
        };

        for (kind, title) in [
            (TabKind::Chart, tr("tab.chart")),
            (TabKind::Logger, tr("tab.logger")),
            (TabKind::Screener, tr("tab.screener")),
            (TabKind::Sepa, tr("tab.sepa")),
            (TabKind::Market, tr("tab.market")),
        ] {
            let mut tab = Tab::new(kind);
            let text = viewer.title(&mut tab).text().to_string();
            assert!(
                text.contains(title.as_str()),
                "tab title must contain {title}, got {text}"
            );
            assert!(
                text.contains(kind.icon()),
                "tab title must carry the icon glyph, got {text}"
            );
        }
    }
}
