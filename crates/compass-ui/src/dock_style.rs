//! egui_dock `Style` builder: maps design tokens onto the dock layout chrome
//! (tab bar, tabs, separators, borders, drag overlay) per design doc §6.1
//! (sub-issue #126, S3).

use egui::{CornerRadius, Stroke};
use egui_dock::Style;

use crate::tokens::ThemeTokens;

/// Build the fully-styled [`egui_dock::Style`] for the three-pane layout.
///
/// Starts from [`Style::default`] and overrides every field in the design
/// doc §6.1 table: tab bar height/background/rounding, the 7 interaction
/// states' chrome, separators, the main surface border and the drag overlay.
pub fn dock_style(tokens: &ThemeTokens) -> Style {
    let c = &tokens.color;
    let mut style = Style::default();

    // Tab bar (design §6.1: bg_panel, 28px, rounded top corners, border line).
    style.tab_bar.bg_fill = c.bg_panel;
    style.tab_bar.height = tokens.spacing.tabbar_h;
    style.tab_bar.corner_radius = CornerRadius {
        // radius.sm (4 px) top corners, square bottom corners.
        nw: 4,
        ne: 4,
        sw: 0,
        se: 0,
    };
    style.tab_bar.hline_color = c.border;

    // Tab interaction states (design §6.1).
    style.tab.active.bg_fill = c.bg_panel_alt;
    style.tab.active.text_color = c.accent;
    style.tab.inactive.text_color = c.text_secondary;
    style.tab.hovered.bg_fill = c.bg_hover;
    style.tab.hovered.text_color = c.text_primary;
    style.tab.spacing = 2.0;
    style.tab.hline_below_active_tab_name = true;

    // Tab body: content area unified with the chart background, no inner margin.
    style.tab.tab_body.bg_fill = c.bg_app;
    style.tab.tab_body.inner_margin = egui::Margin::ZERO;

    // Node separators.
    style.separator.width = 1.0;
    style.separator.color_idle = c.border;
    style.separator.color_hovered = c.border_strong;
    style.separator.color_dragged = c.accent;

    // Main surface outline.
    style.main_surface_border_stroke = Stroke::new(1.0, c.border);

    // Drag-drop overlay: accent at 50% alpha.
    style.overlay.selection_color =
        egui::Color32::from_rgba_unmultiplied(c.accent.r(), c.accent.g(), c.accent.b(), 128);

    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ColorTokens;

    #[test]
    fn tab_bar_matches_design_spec() {
        let style = dock_style(&ThemeTokens::dark());
        let c = ColorTokens::dark();
        assert_eq!(style.tab_bar.bg_fill, c.bg_panel);
        assert_eq!(style.tab_bar.height, 28.0);
        assert_eq!(
            style.tab_bar.corner_radius,
            CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            }
        );
        assert_eq!(style.tab_bar.hline_color, c.border);
    }

    #[test]
    fn tab_interaction_states_match_design_spec() {
        let style = dock_style(&ThemeTokens::dark());
        let c = ColorTokens::dark();
        assert_eq!(style.tab.active.bg_fill, c.bg_panel_alt);
        assert_eq!(style.tab.active.text_color, c.accent);
        assert_eq!(style.tab.inactive.text_color, c.text_secondary);
        assert_eq!(style.tab.hovered.bg_fill, c.bg_hover);
        assert_eq!(style.tab.hovered.text_color, c.text_primary);
        assert_eq!(style.tab.spacing, 2.0);
        assert!(style.tab.hline_below_active_tab_name);
    }

    #[test]
    fn tab_body_matches_design_spec() {
        let style = dock_style(&ThemeTokens::dark());
        let c = ColorTokens::dark();
        assert_eq!(style.tab.tab_body.bg_fill, c.bg_app);
        assert_eq!(style.tab.tab_body.inner_margin, egui::Margin::ZERO);
    }

    #[test]
    fn separator_colors_and_width_match_design_spec() {
        let style = dock_style(&ThemeTokens::dark());
        let c = ColorTokens::dark();
        assert_eq!(style.separator.width, 1.0);
        assert_eq!(style.separator.color_idle, c.border);
        assert_eq!(style.separator.color_hovered, c.border_strong);
        assert_eq!(style.separator.color_dragged, c.accent);
    }

    #[test]
    fn main_surface_border_and_overlay_match_design_spec() {
        let style = dock_style(&ThemeTokens::dark());
        let c = ColorTokens::dark();
        assert_eq!(style.main_surface_border_stroke, Stroke::new(1.0, c.border));
        assert_eq!(
            style.overlay.selection_color,
            egui::Color32::from_rgba_unmultiplied(c.accent.r(), c.accent.g(), c.accent.b(), 128)
        );
    }
}
