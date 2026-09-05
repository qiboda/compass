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
use serde::{Deserialize, Serialize};

use crate::citizens::chart::ChartCitizen;
use crate::citizens::logger::LoggerPanel;
use crate::citizens::market::MarketPanel;
use crate::citizens::screener::ScreenerPanel;
use crate::citizens::sepa::SepaPanel;
use crate::editor::{
    ChartHeaderAction, EDITOR_REGISTRY, EditorCtx, EditorFrame, EditorKind, EditorSignals,
};
use crate::messages::{
    FetchRequest, RunIndexSnapshotRequest, RunLlmRequest, RunScreenerRequest, RunSepaRequest,
};
use crate::state::SharedState;
use compass_core::model::IndexBasic;
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
/// Transition enum (plan A3): the `Tab` payload switches to [`EditorKind`]
/// in phase 1; `TabKind` survives until phase 2 close (F6 hard-coupling
/// removal), kept for the legacy `TabKind → EditorKind` mapping and the
/// pre-migration unit tests. It does NOT carry a `Watchlist` variant —
/// watchlist exists only as `EditorKind::Watchlist`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabKind {
    Chart,
    Logger,
    Screener,
    Sepa,
    Market,
}

#[allow(dead_code)] // transition enum: title/icon/citizen_id kept for legacy tests until phase 2
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

/// Phase-1 mapping: every legacy `TabKind` maps to its `EditorKind`
/// (plan §3.1 / A3). `Watchlist` has no `TabKind` — it is constructed
/// directly as `EditorKind::Watchlist`.
impl From<TabKind> for EditorKind {
    fn from(value: TabKind) -> Self {
        match value {
            TabKind::Chart => Self::Chart,
            TabKind::Logger => Self::Logger,
            TabKind::Screener => Self::Screener,
            TabKind::Sepa => Self::Sepa,
            TabKind::Market => Self::Market,
        }
    }
}

impl EditorKind {
    /// The [`CitizenId`] this editor kind maps to in the dispatcher
    /// (one-hot activation; 1:1 link kept until the citizen layer lands —
    /// friction F6, removed in phase 2).
    ///
    /// `None` for kinds that are NOT 1:1 citizens — plan §4.6: the watchlist
    /// has no registered citizen (it is not a one-hot member); `on_tab_button`
    /// skips activate for it. Never fabricate an id for an unregistered
    /// citizen: `Dispatcher::activate` silently deactivates everything when
    /// the id matches nothing (egui_citizen dispatcher.rs:96-108).
    pub fn citizen_id(&self) -> Option<CitizenId> {
        match self {
            Self::Chart => Some(CitizenId::new(CHART_ID)),
            Self::Logger => Some(CitizenId::new(LOGGER_ID)),
            Self::Screener => Some(CitizenId::new(SCREENER_ID)),
            Self::Sepa => Some(CitizenId::new(SEPA_ID)),
            Self::Market => Some(CitizenId::new(MARKET_ID)),
            Self::Watchlist => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tab — wraps EditorKind for egui_dock
// ---------------------------------------------------------------------------

/// A dockable tab carrying its [`EditorKind`].
///
/// Used as `DockState<Tab>` and `TabViewer::Tab = Tab` in egui_dock.
/// The payload switched from `TabKind` in phase 1 (plan A3) so dock trees
/// can carry `EditorKind::Watchlist` leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    kind: EditorKind,
}

impl Tab {
    /// Create a tab of the given kind. Accepts both `EditorKind` directly
    /// and the transition `TabKind` (via `From<TabKind> for EditorKind`),
    /// so legacy call sites compile unchanged until phase 2.
    pub fn new(kind: impl Into<EditorKind>) -> Self {
        Self { kind: kind.into() }
    }

    /// The [`EditorKind`] this tab carries.
    #[allow(dead_code)] // consumed by editor/unit tests today; phase 2 dispatch
    pub fn kind(&self) -> EditorKind {
        self.kind
    }

    /// i18n key of the tab's display title (`editor.*` key tree).
    pub fn title(&self) -> &'static str {
        self.kind.title_key()
    }

    /// Phosphor icon glyph shown next to the tab title.
    #[allow(dead_code)] // kept for parity with title(); phase 2 chrome uses it
    pub fn icon(&self) -> &'static str {
        self.kind.icon()
    }

    /// The [`CitizenId`] this tab maps to in the dispatcher, if it is a
    /// 1:1 citizen (`None` for non-citizen kinds like Watchlist, plan §4.6).
    pub fn citizen_id(&self) -> Option<CitizenId> {
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
    /// Index list backing the chart header's 前复权 hide guard (plan §4.1 —
    /// the control moved out of the toolbar; `is_index_or_board` moved too).
    pub index_list: &'a [IndexBasic],
    /// Out-param: chart header action (timeframe/adjust/fetch) consumed by
    /// the owner after `show_inside` returns.
    pub chart_action: &'a mut Option<ChartHeaderAction>,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.kind.icon(), t!(tab.title())).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.kind {
            EditorKind::Chart => {
                let desc = EDITOR_REGISTRY
                    .iter()
                    .find(|d| d.kind == EditorKind::Chart)
                    .expect("chart descriptor must exist in EDITOR_REGISTRY");
                let sidebar_visible = desc
                    .layout
                    .sidebar
                    .as_ref()
                    .map(|s| s.default_visible)
                    .unwrap_or(false);
                let mut ctx = EditorCtx {
                    state: self.shared_state,
                    theme: self.theme,
                    signals: &EditorSignals {
                        work: self.work_signal,
                        screener: self.run_screener_signal,
                        sepa: self.sepa_signal,
                        index: self.index_signal,
                        llm: self.llm_signal,
                    },
                    index_list: self.index_list,
                    chart_action: self.chart_action,
                    screener_industries: self.screener_industries,
                    screener_boards: self.screener_boards,
                };
                let mut frame = EditorFrame { sidebar_visible };
                frame.show(ui, desc, self.chart, &mut ctx);
            }
            EditorKind::Logger => {
                *self.logger_export_clicked =
                    self.logger.show(ui, self.shared_state, self.theme.tokens());
            }
            EditorKind::Screener => {
                let desc = EDITOR_REGISTRY
                    .iter()
                    .find(|d| d.kind == EditorKind::Screener)
                    .expect("screener descriptor must exist in EDITOR_REGISTRY");
                let sidebar_visible = desc
                    .layout
                    .sidebar
                    .as_ref()
                    .map(|s| s.default_visible)
                    .unwrap_or(false);
                let mut ctx = EditorCtx {
                    state: self.shared_state,
                    theme: self.theme,
                    signals: &EditorSignals {
                        work: self.work_signal,
                        screener: self.run_screener_signal,
                        sepa: self.sepa_signal,
                        index: self.index_signal,
                        llm: self.llm_signal,
                    },
                    index_list: self.index_list,
                    chart_action: self.chart_action,
                    screener_industries: self.screener_industries,
                    screener_boards: self.screener_boards,
                };
                let mut frame = EditorFrame { sidebar_visible };
                frame.show(ui, desc, self.screener, &mut ctx);
            }
            EditorKind::Sepa => {
                let desc = EDITOR_REGISTRY
                    .iter()
                    .find(|d| d.kind == EditorKind::Sepa)
                    .expect("sepa descriptor must exist in EDITOR_REGISTRY");
                let sidebar_visible = false; // SEPA registers no sidebar (design §6).
                let mut ctx = EditorCtx {
                    state: self.shared_state,
                    theme: self.theme,
                    signals: &EditorSignals {
                        work: self.work_signal,
                        screener: self.run_screener_signal,
                        sepa: self.sepa_signal,
                        index: self.index_signal,
                        llm: self.llm_signal,
                    },
                    index_list: self.index_list,
                    chart_action: self.chart_action,
                    screener_industries: self.screener_industries,
                    screener_boards: self.screener_boards,
                };
                let mut frame = EditorFrame { sidebar_visible };
                frame.show(ui, desc, self.sepa, &mut ctx);
            }
            EditorKind::Market => {
                self.market
                    .show(ui, self.shared_state, self.index_signal, self.work_signal);
            }
            // Phase-1 placeholder: the Outliner-style watchlist editor lands
            // in phase 2f (plan §4.6); its leaf only becomes reachable once
            // the workspaces tie in (phase 3).
            EditorKind::Watchlist => {
                ui.label(t!("editor.watchlist"));
            }
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        // Skips activate for non-citizen kinds (Watchlist, plan §4.6) — an
        // unregistered id would silently deactivate every citizen.
        if response.clicked()
            && let Some(cid) = tab.citizen_id()
        {
            self.dispatcher.activate(&cid);
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
        // Phase 1: Tab payload is EditorKind — title key is the `editor.*`
        // tree (display value unchanged: tab.chart == editor.chart == 图表).
        assert_eq!(tab.title(), "editor.chart");
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
    // EditorKind::citizen_id — plan §4.0/§4.6 contract: Option<CitizenId>
    // (5 one-hot kinds → Some, Watchlist → None; tab activation skips None)
    // ------------------------------------------------------------------

    #[test]
    fn editor_kind_citizen_id_contract_five_one_hot_kinds_some() {
        // Each one-hot kind must return Some (registered citizen).
        let cases = [
            (EditorKind::Chart, CitizenId::new(CHART_ID)),
            (EditorKind::Logger, CitizenId::new(LOGGER_ID)),
            (EditorKind::Screener, CitizenId::new(SCREENER_ID)),
            (EditorKind::Sepa, CitizenId::new(SEPA_ID)),
            (EditorKind::Market, CitizenId::new(MARKET_ID)),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.citizen_id(), Some(expected), "{kind:?} must be Some");
        }
    }

    #[test]
    fn editor_kind_watchlist_citizen_id_is_none() {
        // plan §4.6: watchlist is NOT a 1:1 citizen. Fabricating an id
        // would make Dispatcher::activate silently deactivate every
        // registered citizen (egui_citizen dispatcher.rs:96-108).
        assert_eq!(EditorKind::Watchlist.citizen_id(), None);
    }

    #[test]
    fn editor_kind_citizen_id_matches_tabkind_identity() {
        // Migration guard: EditorKind must keep the exact TabKind citizen
        // mapping (a drift here flips tab activation semantics).
        for (editor, legacy) in [
            (EditorKind::Chart, TabKind::Chart),
            (EditorKind::Logger, TabKind::Logger),
            (EditorKind::Screener, TabKind::Screener),
            (EditorKind::Sepa, TabKind::Sepa),
            (EditorKind::Market, TabKind::Market),
        ] {
            assert_eq!(editor.citizen_id(), Some(legacy.citizen_id()));
        }
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
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(CHART_ID)));
    }

    #[test]
    fn tab_new_logger_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Logger);
        assert_eq!(tr(tab.title()), "日志");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(LOGGER_ID)));
    }

    #[test]
    fn tab_new_screener_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Screener);
        assert_eq!(tr(tab.title()), "选股器");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(SCREENER_ID)));
    }

    #[test]
    fn tab_new_sepa_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(TabKind::Sepa);
        assert_eq!(tr(tab.title()), "东方SEPA");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(SEPA_ID)));
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
        let mut chart_action = None;
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
            index_list: &[],
            chart_action: &mut chart_action,
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
