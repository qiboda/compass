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
use crate::citizens::screener::ScreenerPanel;
use crate::messages::{FetchRequest, RunScreenerRequest};
use crate::state::SharedState;

// ---------------------------------------------------------------------------
// Citizen ID constants
// ---------------------------------------------------------------------------

pub const CHART_ID: &str = "chart";
pub const LOGGER_ID: &str = "logger";
pub const SCREENER_ID: &str = "screener";

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
}

impl TabKind {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Chart => "图表",
            Self::Logger => "日志",
            Self::Screener => "选股器",
        }
    }

    /// Phosphor icon glyph shown next to the tab title (design doc §Q2).
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Chart => egui_phosphor::regular::CHART_LINE,
            Self::Logger => egui_phosphor::regular::TERMINAL,
            Self::Screener => egui_phosphor::regular::FUNNEL_SIMPLE,
        }
    }

    pub fn citizen_id(&self) -> CitizenId {
        match self {
            Self::Chart => CitizenId::new(CHART_ID),
            Self::Logger => CitizenId::new(LOGGER_ID),
            Self::Screener => CitizenId::new(SCREENER_ID),
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
    pub run_screener_signal: &'a Signal<RunScreenerRequest>,
    pub work_signal: &'a Signal<FetchRequest>,
    pub screener_industries: &'a [String],
    pub screener_boards: &'a [String],
    pub shared_state: &'a SharedState,
    pub theme: &'a CompassTheme,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.kind.icon(), tab.title()).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.kind {
            TabKind::Chart => self.chart.show(ui, self.shared_state, self.theme),
            TabKind::Logger => self.logger.show(ui, self.shared_state),
            TabKind::Screener => self.screener.show(
                ui,
                self.shared_state,
                self.run_screener_signal,
                self.work_signal,
                self.screener_industries,
                self.screener_boards,
            ),
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

    // ------------------------------------------------------------------
    // TabKind::title
    // ------------------------------------------------------------------

    #[test]
    fn tab_kind_chart_title() {
        assert_eq!(TabKind::Chart.title(), "图表");
    }

    #[test]
    fn tab_kind_logger_title() {
        assert_eq!(TabKind::Logger.title(), "日志");
    }

    #[test]
    fn tab_kind_screener_title() {
        assert_eq!(TabKind::Screener.title(), "选股器");
    }

    #[test]
    fn tab_kind_icons_are_phosphor_glyphs() {
        assert_eq!(TabKind::Chart.icon(), egui_phosphor::regular::CHART_LINE);
        assert_eq!(TabKind::Logger.icon(), egui_phosphor::regular::TERMINAL);
        assert_eq!(
            TabKind::Screener.icon(),
            egui_phosphor::regular::FUNNEL_SIMPLE
        );
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

    // ------------------------------------------------------------------
    // Tab::new / Tab::title / Tab::citizen_id
    // ------------------------------------------------------------------

    #[test]
    fn tab_new_chart_delegates_to_tab_kind() {
        let tab = Tab::new(TabKind::Chart);
        assert_eq!(tab.title(), "图表");
        assert_eq!(tab.citizen_id(), CitizenId::new(CHART_ID));
    }

    #[test]
    fn tab_new_logger_delegates_to_tab_kind() {
        let tab = Tab::new(TabKind::Logger);
        assert_eq!(tab.title(), "日志");
        assert_eq!(tab.citizen_id(), CitizenId::new(LOGGER_ID));
    }

    #[test]
    fn tab_new_screener_delegates_to_tab_kind() {
        let tab = Tab::new(TabKind::Screener);
        assert_eq!(tab.title(), "选股器");
        assert_eq!(tab.citizen_id(), CitizenId::new(SCREENER_ID));
    }

    #[test]
    fn tab_same_kind_are_equal() {
        assert_eq!(Tab::new(TabKind::Chart), Tab::new(TabKind::Chart));
        assert_eq!(Tab::new(TabKind::Logger), Tab::new(TabKind::Logger));
        assert_eq!(Tab::new(TabKind::Screener), Tab::new(TabKind::Screener));
        assert_ne!(Tab::new(TabKind::Chart), Tab::new(TabKind::Logger));
        assert_ne!(Tab::new(TabKind::Chart), Tab::new(TabKind::Screener));
    }
}
