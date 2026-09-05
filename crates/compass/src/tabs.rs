//! egui_dock TabViewer bridge for the citizen pattern.
//!
//! Each tab is a [`Tab`] wrapping an [`EditorKind`]. When a tab button is
//! clicked, the `on_tab_button` hook calls
//! [`Dispatcher::activate`] with the tab's [`CitizenId`] (skipped for
//! non-citizen kinds like Watchlist), enabling one-hot panel activation
//! across the dock layout. Rendering delegates to
//! [`EditorInstances::get_mut`] + [`EditorFrame`] (design §4.2/§4.5).

use egui_citizen::{CitizenId, Registry};
use serde::{Deserialize, Serialize};

use crate::editor::{
    ChartHeaderAction, EDITOR_REGISTRY, EditorCtx, EditorFrame, EditorInstances, EditorKind,
    EditorSignals, WatchlistAction,
};
use crate::state::SharedState;
use compass_core::model::{IndexBasic, StockBasic};
use compass_i18n::t;
use compass_ui::widgets::toast::ToastManager;

// ---------------------------------------------------------------------------
// Citizen ID constants
// ---------------------------------------------------------------------------

pub const CHART_ID: &str = "chart";
pub const LOGGER_ID: &str = "logger";
pub const SCREENER_ID: &str = "screener";
pub const SEPA_ID: &str = "sepa";
pub const MARKET_ID: &str = "market";

// ---------------------------------------------------------------------------
// EditorKind — citizen mapping (F6 hard coupling)
// ---------------------------------------------------------------------------

impl EditorKind {
    /// The [`CitizenId`] this editor kind maps to in the dispatcher
    /// (one-hot activation; the 1:1 link replaced the `TabKind`-to-citizen
    /// hard coupling, friction F6, removed in phase 3).
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
/// The payload switched from `TabKind` to `EditorKind` (plan A3 — phase 3
/// deleted the transition enum) so dock trees can carry
/// `EditorKind::Watchlist` leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tab {
    kind: EditorKind,
}

impl Tab {
    /// Create a tab of the given kind.
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
/// and delegates rendering to each editor via [`EditorInstances::get_mut`].
///
/// Created inline each frame — the short-lived borrows satisfy egui_dock's
/// borrowing requirements. Phase 3 converged the per-editor field list
/// (chart/logger/screener/… 6 fields + 5 signals + 4 context lists) onto
/// `editors` + the [`EditorCtx`] bundle (plan §5.1, design §4.2): the
/// out-params (`logger_export_clicked` / `chart_action` / `watchlist_action`)
/// stay as explicit fields per plan §5.2's preferred fallback — they are
/// owner channels written during render and consumed after `show_inside`.
pub struct TabViewer<'a> {
    pub dispatcher: &'a mut Registry,
    /// All editor instances — the single dispatch container (design §4.5).
    pub editors: &'a mut EditorInstances,
    pub shared_state: &'a SharedState,
    pub theme: &'a CompassTheme,
    /// Signal bundle passed through to every editor (design §4.2).
    pub signals: &'a EditorSignals<'a>,
    /// Index list backing the chart header's 前复权 hide guard (plan §4.1).
    pub index_list: &'a [IndexBasic],
    /// Screener condition-builder context (plan §4.2).
    pub screener_industries: &'a [String],
    pub screener_boards: &'a [String],
    /// Stock metadata list backing watchlist row names/exchange tags.
    pub stock_list: &'a [StockBasic],
    /// Toast sink handed to editors (plan §4.6 — watchlist add/remove).
    pub toasts: &'a mut ToastManager,
    /// Out-param: set to `true` when the logger export button was clicked.
    pub logger_export_clicked: &'a mut bool,
    /// Out-param: chart header action (timeframe/adjust/fetch) consumed by
    /// the owner after `show_inside` returns.
    pub chart_action: &'a mut Option<ChartHeaderAction>,
    /// Out-param: watchlist editor action (fetch/add/delete-request modal)
    /// consumed by the owner after `show_inside` returns.
    pub watchlist_action: &'a mut Option<WatchlistAction>,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        // egui_dock 0.21 requires a unique per-tree tab id; kind is unique per
        // tree (one instance per EditorKind per workspace) and Tab is Hash.
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.kind.icon(), t!(tab.title())).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        let kind = tab.kind;
        let desc = EDITOR_REGISTRY
            .iter()
            .find(|d| d.kind == kind)
            .expect("descriptor must exist in EDITOR_REGISTRY");
        // Sidebar visibility derives from the registered layout (arbitration
        // Q4: Chart/Screener default-visible; Sepa/Market/Logger/Watchlist
        // register None so this is false for them).
        let sidebar_visible = desc
            .layout
            .sidebar
            .as_ref()
            .is_some_and(|s| s.default_visible);
        let mut ctx = EditorCtx {
            state: self.shared_state,
            theme: self.theme,
            signals: self.signals,
            index_list: self.index_list,
            chart_action: self.chart_action,
            screener_industries: self.screener_industries,
            screener_boards: self.screener_boards,
            logger_export_clicked: self.logger_export_clicked,
            toasts: self.toasts,
            stock_list: self.stock_list,
            watchlist_action: self.watchlist_action,
        };
        let mut frame = EditorFrame { sidebar_visible };
        frame.show(ui, desc, self.editors.get_mut(kind), &mut ctx);
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        // Skips activate for non-citizen kinds (Watchlist, plan §4.6) — an
        // unregistered id would silently deactivate every citizen.
        if response.clicked()
            && let Some(cid) = tab.citizen_id()
        {
            self.dispatcher.activate(cid);
        }
    }
}

// ===========================================================================
// Tests — ref #79 (pure-logic Tab + EditorKind, no TabViewer rendering)
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
    // Tab::title via EditorKind
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // #222 i18n (T5): `EditorKind::title_key()` returns KEY CONSTANTS
    // ("editor.chart" etc.), not display text — the rendering consumer
    // (TabViewer::title) resolves them via t!() so a live locale switch
    // updates the dock tabs.
    // ------------------------------------------------------------------

    #[test]
    fn tab_title_delegates_to_key_constant() {
        let tab = Tab::new(EditorKind::Chart);
        // Tab payload is EditorKind — the title key is the `editor.*` tree
        // (display value unchanged: tab.chart == editor.chart == 图表).
        assert_eq!(tab.title(), "editor.chart");
    }

    // ------------------------------------------------------------------
    // EditorKind::citizen_id
    // ------------------------------------------------------------------

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

    // ------------------------------------------------------------------
    // Tab::new / Tab::title / Tab::citizen_id
    // ------------------------------------------------------------------

    #[test]
    fn tab_new_chart_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(EditorKind::Chart);
        assert_eq!(tr(tab.title()), "图表");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(CHART_ID)));
    }

    #[test]
    fn tab_new_logger_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(EditorKind::Logger);
        assert_eq!(tr(tab.title()), "日志");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(LOGGER_ID)));
    }

    #[test]
    fn tab_new_screener_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(EditorKind::Screener);
        assert_eq!(tr(tab.title()), "选股器");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(SCREENER_ID)));
    }

    #[test]
    fn tab_new_sepa_delegates_to_tab_kind() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let tab = Tab::new(EditorKind::Sepa);
        assert_eq!(tr(tab.title()), "东方SEPA");
        assert_eq!(tab.citizen_id(), Some(CitizenId::new(SEPA_ID)));
    }

    #[test]
    fn tab_same_kind_are_equal() {
        assert_eq!(Tab::new(EditorKind::Chart), Tab::new(EditorKind::Chart));
        assert_eq!(Tab::new(EditorKind::Logger), Tab::new(EditorKind::Logger));
        assert_eq!(
            Tab::new(EditorKind::Screener),
            Tab::new(EditorKind::Screener)
        );
        assert_eq!(Tab::new(EditorKind::Sepa), Tab::new(EditorKind::Sepa));
        assert_ne!(Tab::new(EditorKind::Chart), Tab::new(EditorKind::Logger));
        assert_ne!(Tab::new(EditorKind::Chart), Tab::new(EditorKind::Screener));
        assert_ne!(Tab::new(EditorKind::Chart), Tab::new(EditorKind::Sepa));
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
        use crate::citizens::market::MarketPanel;
        use crate::citizens::screener::ScreenerPanel;
        use crate::citizens::sepa::SepaPanel;
        use crate::dispatcher::register_citizens;
        use crate::editor::{EditorInstances, EditorSignals, WatchlistEditor};
        use crate::messages::{
            FetchRequest, RunIndexSnapshotRequest, RunLlmRequest, RunScreenerRequest,
            RunSepaRequest,
        };
        use crate::state::SharedState;
        use crate::theme::CompassTheme;
        use egui_dock::TabViewer as _;
        use egui_mobius::factory;

        let mut dispatcher = Registry::new();
        let registered = register_citizens(&mut dispatcher);
        let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let screener = ScreenerPanel::new(
            CitizenId::new(SCREENER_ID),
            registered.screener,
            None,
            Box::new(|_| {}),
            &compass_ui::tokens::ThemeTokens::dark(),
            false,
        );
        let sepa = SepaPanel::new(
            CitizenId::new(SEPA_ID),
            registered.sepa,
            &compass_ui::tokens::ThemeTokens::dark(),
        );
        let market = MarketPanel::new(
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

        let mut editors = EditorInstances {
            chart,
            logger,
            screener,
            sepa,
            market,
            watchlist: WatchlistEditor::new(),
        };
        let signals = EditorSignals {
            work: &work_signal,
            screener: &run_signal,
            sepa: &sepa_signal,
            index: &index_signal,
            llm: &llm_signal,
        };
        let mut logger_export_clicked = false;
        let mut chart_action = None;
        let mut toasts = ToastManager::new(*theme.tokens());
        let mut watchlist_action = None;
        let mut viewer = TabViewer {
            dispatcher: &mut dispatcher,
            editors: &mut editors,
            shared_state: &shared,
            theme: &theme,
            signals: &signals,
            index_list: &[],
            screener_industries: &[],
            screener_boards: &[],
            stock_list: &[],
            toasts: &mut toasts,
            logger_export_clicked: &mut logger_export_clicked,
            chart_action: &mut chart_action,
            watchlist_action: &mut watchlist_action,
        };

        for (kind, title) in [
            (EditorKind::Chart, tr("editor.chart")),
            (EditorKind::Logger, tr("editor.logger")),
            (EditorKind::Screener, tr("editor.screener")),
            (EditorKind::Sepa, tr("editor.sepa")),
            (EditorKind::Market, tr("editor.market")),
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
