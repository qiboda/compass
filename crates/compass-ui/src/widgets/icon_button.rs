//! Icon button atom: a square icon-only button with optional tooltip
//! (design doc §5.1 `IconButton`).

use crate::tokens::ThemeTokens;
use egui::{Color32, CornerRadius, RichText, Stroke, Ui, Vec2};

/// Square icon button: `control_md` × `control_md` by default,
/// `control_sm` × `control_sm` when small.
pub struct IconButton<'a> {
    tokens: &'a ThemeTokens,
    icon: &'a str,
    tooltip: Option<&'a str>,
    side: f32,
}

impl<'a> IconButton<'a> {
    /// Create an icon button for the given theme with a phosphor icon character.
    pub fn new(tokens: &'a ThemeTokens, icon: &'a str) -> Self {
        Self {
            tokens,
            icon,
            tooltip: None,
            side: tokens.spacing.control_md,
        }
    }

    /// Attach a hover tooltip text.
    pub fn tooltip(mut self, tooltip: &'a str) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// Shrink to the small size (24×24).
    pub fn small(mut self) -> Self {
        self.side = self.tokens.spacing.control_sm;
        self
    }

    /// Override the button side length in points.
    pub fn size(mut self, side: f32) -> Self {
        self.side = side;
        self
    }

    /// Show the icon button and return whether it was clicked.
    pub fn show(self, ui: &mut Ui) -> bool {
        let tokens = self.tokens;
        let c = &tokens.color;

        // Scoped style: hover fill comes from the theme, icon is secondary text.
        let previous_style = ui.style().clone();
        let mut style = (*previous_style).clone();
        let w = &mut style.visuals.widgets;
        for state in [
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.noninteractive,
        ] {
            state.weak_bg_fill = Color32::TRANSPARENT;
            state.bg_stroke = Stroke::NONE;
            state.corner_radius = CornerRadius::from(tokens.radius.sm);
        }
        w.hovered.weak_bg_fill = c.bg_hover;
        w.active.weak_bg_fill = c.bg_active;

        let button = egui::Button::new(RichText::new(self.icon).size(16.0).color(c.text_secondary))
            .min_size(Vec2::splat(self.side));

        ui.set_style(style);
        let response = ui.add(button);
        ui.set_style(previous_style);

        let clicked = response.clicked();
        if let Some(tooltip) = self.tooltip {
            response.on_hover_text(tooltip);
        }
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;
    use std::cell::Cell;
    use std::rc::Rc;

    /// The icon character is rendered and clickable.
    #[test]
    fn icon_button_click_fires() {
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(0u32));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if IconButton::new(&tokens, "\u{E20C}").show(ui) {
                c.set(c.get() + 1);
            }
        });
        harness.run();
        harness.get_by_label("\u{E20C}").click();
        harness.run();
        assert_eq!(clicked.get(), 1);
    }

    /// The small size uses the control-sm token.
    #[test]
    fn small_size_uses_control_sm_token() {
        let tokens = ThemeTokens::dark();
        let big = IconButton::new(&tokens, "\u{E20C}");
        assert_eq!(big.side, 32.0);
        assert_eq!(big.small().side, tokens.spacing.control_sm);
    }

    /// Default side reads the `control_md` token, not a hardcoded literal
    /// (issue #226). Token change must propagate to the default size.
    #[test]
    fn default_side_follows_control_md_token() {
        let mut tokens = ThemeTokens::dark();
        tokens.spacing.control_md = 40.0;
        let big = IconButton::new(&tokens, "\u{E20C}");
        assert_eq!(
            big.side, tokens.spacing.control_md,
            "default side must follow the control_md token, not a hardcoded 32.0"
        );
        assert_eq!(big.small().side, tokens.spacing.control_sm);
    }

    /// An explicit `.size()` override wins over the default (issue #226).
    #[test]
    fn size_override_wins_over_default() {
        let tokens = ThemeTokens::dark();
        assert_eq!(IconButton::new(&tokens, "\u{E20C}").size(40.0).side, 40.0);
        assert_eq!(
            IconButton::new(&tokens, "\u{E20C}").small().size(40.0).side,
            40.0
        );
    }

    /// A tooltip attaches without affecting clicks.
    #[test]
    fn tooltip_does_not_block_clicks() {
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if IconButton::new(&tokens, "\u{E20C}")
                .tooltip("Open")
                .show(ui)
            {
                c.set(true);
            }
        });
        harness.run();
        harness.get_by_label("\u{E20C}").click();
        harness.run();
        assert!(clicked.get());
    }
}
