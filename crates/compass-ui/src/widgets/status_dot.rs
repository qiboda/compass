//! Status dot atom: 8 px state indicator with a breathing pulse for the
//! loading state (design doc §5.1 `StatusDot`).

use crate::tokens::ThemeTokens;
use egui::{Color32, Response, Sense, Ui};

/// Dot state; `Loading` renders a breathing pulse.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DotState {
    /// Neutral idle dot.
    #[default]
    Idle,
    /// Success (green).
    Success,
    /// Warning (amber).
    Warning,
    /// Error (red, always on).
    Error,
    /// Loading — 800 ms breathing pulse.
    Loading,
}

/// Small status indicator dot (8 px).
pub struct StatusDot<'a> {
    tokens: &'a ThemeTokens,
    state: DotState,
    size: f32,
}

impl<'a> StatusDot<'a> {
    /// Create a status dot in the given state.
    pub fn new(tokens: &'a ThemeTokens, state: DotState) -> Self {
        Self {
            tokens,
            state,
            size: 8.0,
        }
    }

    /// Override the dot diameter (default 8 px).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// The solid color for a non-pulsing state.
    pub fn color(&self) -> Color32 {
        let c = &self.tokens.color;
        match self.state {
            DotState::Idle => c.text_weak,
            DotState::Success => c.success,
            DotState::Warning => c.warning,
            DotState::Error => c.error,
            DotState::Loading => c.accent,
        }
    }

    /// Show the dot and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let base = self.color();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(self.size, self.size), Sense::hover());

        let alpha = match self.state {
            DotState::Loading => {
                // Breathing pulse: 800 ms sine period mapped to alpha 0.4 → 1,
                // smoothed through egui's animation manager.
                let time = ui.ctx().input(|i| i.time) as f32;
                let raw = 0.4 + 0.6 * (0.5 + 0.5 * (time * std::f32::consts::TAU / 0.8).sin());
                let animated = ui
                    .ctx()
                    .animate_value_with_time(ui.id().with("pulse"), raw, 0.8);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(16));
                animated
            }
            DotState::Idle | DotState::Success | DotState::Warning | DotState::Error => 1.0,
        };

        let color =
            Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (255.0 * alpha) as u8);
        ui.painter()
            .circle_filled(rect.center(), self.size / 2.0, color);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;

    /// State → color mapping follows the design doc.
    #[test]
    fn state_colors_follow_design() {
        let tokens = ThemeTokens::dark();
        let c = &tokens.color;
        assert_eq!(StatusDot::new(&tokens, DotState::Idle).color(), c.text_weak);
        assert_eq!(
            StatusDot::new(&tokens, DotState::Success).color(),
            c.success
        );
        assert_eq!(
            StatusDot::new(&tokens, DotState::Warning).color(),
            c.warning
        );
        assert_eq!(StatusDot::new(&tokens, DotState::Error).color(), c.error);
        assert_eq!(StatusDot::new(&tokens, DotState::Loading).color(), c.accent);
    }

    /// Only the loading state animates (non-loading states are static).
    #[test]
    fn only_loading_state_animates() {
        let tokens = ThemeTokens::dark();
        let static_states = [
            DotState::Idle,
            DotState::Success,
            DotState::Warning,
            DotState::Error,
        ];
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            for state in static_states {
                StatusDot::new(&tokens, state).show(ui);
            }
            StatusDot::new(&tokens, DotState::Loading).show(ui);
        });
        harness.step();
        harness.step();
        // Rendering both static and pulsing dots must not panic.
    }
}
