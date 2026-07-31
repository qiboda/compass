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

use crate::citizens::chart::ChartCitizen;
use crate::citizens::logger::LoggerPanel;
use crate::state::SharedState;

// ---------------------------------------------------------------------------
// Citizen ID constants
// ---------------------------------------------------------------------------

pub const CHART_ID: &str = "chart";
pub const LOGGER_ID: &str = "logger";

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
}

impl TabKind {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Chart => "Chart",
            Self::Logger => "Logger",
        }
    }

    pub fn citizen_id(&self) -> CitizenId {
        match self {
            Self::Chart => CitizenId::new(CHART_ID),
            Self::Logger => CitizenId::new(LOGGER_ID),
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
    pub shared_state: &'a SharedState,
    pub theme: &'a CompassTheme,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.kind {
            TabKind::Chart => self.chart.show(ui, self.shared_state, self.theme),
            TabKind::Logger => self.logger.show(ui, self.shared_state),
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
        assert_eq!(TabKind::Chart.title(), "Chart");
    }

    #[test]
    fn tab_kind_logger_title() {
        assert_eq!(TabKind::Logger.title(), "Logger");
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

    // ------------------------------------------------------------------
    // Tab::new / Tab::title / Tab::citizen_id
    // ------------------------------------------------------------------

    #[test]
    fn tab_new_chart_delegates_to_tab_kind() {
        let tab = Tab::new(TabKind::Chart);
        assert_eq!(tab.title(), "Chart");
        assert_eq!(tab.citizen_id(), CitizenId::new(CHART_ID));
    }

    #[test]
    fn tab_new_logger_delegates_to_tab_kind() {
        let tab = Tab::new(TabKind::Logger);
        assert_eq!(tab.title(), "Logger");
        assert_eq!(tab.citizen_id(), CitizenId::new(LOGGER_ID));
    }

    #[test]
    fn tab_same_kind_are_equal() {
        assert_eq!(Tab::new(TabKind::Chart), Tab::new(TabKind::Chart));
        assert_eq!(Tab::new(TabKind::Logger), Tab::new(TabKind::Logger));
        assert_ne!(Tab::new(TabKind::Chart), Tab::new(TabKind::Logger));
    }
}
