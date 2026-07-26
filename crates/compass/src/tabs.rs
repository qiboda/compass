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
//!     Tab::new(TabKind::Control),
//!     Tab::new(TabKind::Chart),
//!     Tab::new(TabKind::Logger),
//! ]);
//! let mut tab_viewer = TabViewer {
//!     dispatcher: &mut dispatcher,
//!     control: &mut (),
//!     chart: &mut (),
//!     logger: &mut (),
//! };
//! egui_dock::DockArea::new(&mut dock_state).show_inside(ui, &mut tab_viewer);
//! for msg in tab_viewer.dispatcher.drain_messages() { /* ... */ }
//! ```

use egui_citizen::{CitizenId, Dispatcher};

use crate::citizens::chart::ChartCitizen;
use crate::citizens::control::ControlCitizen;
use crate::citizens::logger::LoggerPanel;
use crate::state::SharedState;

// ---------------------------------------------------------------------------
// Citizen ID constants
// ---------------------------------------------------------------------------

pub const CONTROL_ID: &str = "control";
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
    Control,
    Chart,
    Logger,
}

impl TabKind {
    /// Human-readable label shown in the tab bar.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Control => "Controls",
            Self::Chart => "Chart",
            Self::Logger => "Logger",
        }
    }

    /// The corresponding [`CitizenId`] for dispatcher activation.
    pub fn citizen_id(&self) -> CitizenId {
        match self {
            Self::Control => CitizenId::new(CONTROL_ID),
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

/// egui_dock [`TabViewer`] that bridges tab clicks to citizen activation
/// and delegates rendering to each citizen's `show` method.
///
/// Created inline each frame — the short-lived borrows satisfy egui_dock's
/// borrowing requirements.
pub struct TabViewer<'a> {
    /// Central citizen dispatcher for one-hot activation.
    pub dispatcher: &'a mut Dispatcher,

    /// Control panel citizen (symbol / timeframe input + fetch button).
    pub control: &'a mut ControlCitizen,

    /// Chart panel citizen (OHLCV candlestick chart).
    pub chart: &'a mut ChartCitizen,

    /// Logger panel citizen (scrollable log entries).
    pub logger: &'a mut LoggerPanel,

    /// Reactive shared state, passed to each citizen's `show()`.
    pub shared_state: &'a SharedState,
}

impl egui_dock::TabViewer for TabViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab.kind {
            TabKind::Control => self.control.show(ui, self.shared_state),
            TabKind::Chart => self.chart.show(ui, self.shared_state),
            TabKind::Logger => self.logger.show(ui, self.shared_state),
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if response.clicked() {
            self.dispatcher.activate(&tab.citizen_id());
        }
    }
}
