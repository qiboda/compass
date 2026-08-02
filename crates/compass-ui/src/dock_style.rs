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

    // Tab interaction states (design §6.1). All seven egui_dock states are
    // covered so keyboard-focus variants never fall back to the default
    // black/white palette that clashes with the dark theme.
    //
    // In this app every leaf holds a single tab, so EVERY tab is "active"
    // in its own leaf (`leaf.active == tab_index` is always true). Only the
    // focused leaf's tab (the one last interacted with) is visually raised
    // with the accent ring; active-but-not-focused tabs stay quiet so the
    // focused panel is unambiguous.
    style.tab.active.bg_fill = c.bg_panel;
    style.tab.active.text_color = c.text_primary;
    style.tab.active.outline_color = egui::Color32::TRANSPARENT;
    style.tab.inactive.bg_fill = c.bg_panel;
    style.tab.inactive.text_color = c.text_secondary;
    style.tab.inactive.outline_color = egui::Color32::TRANSPARENT;
    style.tab.focused.text_color = c.accent;
    style.tab.focused.bg_fill = c.bg_panel_alt;
    style.tab.focused.outline_color = c.accent;
    style.tab.hovered.bg_fill = c.bg_hover;
    style.tab.hovered.text_color = c.text_primary;
    style.tab.hovered.outline_color = egui::Color32::TRANSPARENT;
    style.tab.active_with_kb_focus.bg_fill = c.bg_panel;
    style.tab.active_with_kb_focus.text_color = c.text_primary;
    style.tab.active_with_kb_focus.outline_color = egui::Color32::TRANSPARENT;
    style.tab.inactive_with_kb_focus.bg_fill = c.bg_panel;
    style.tab.inactive_with_kb_focus.text_color = c.text_secondary;
    style.tab.inactive_with_kb_focus.outline_color = egui::Color32::TRANSPARENT;
    style.tab.focused_with_kb_focus.text_color = c.accent;
    style.tab.focused_with_kb_focus.bg_fill = c.bg_panel_alt;
    style.tab.focused_with_kb_focus.outline_color = c.accent;
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
        assert_eq!(style.tab.active.bg_fill, c.bg_panel);
        assert_eq!(style.tab.active.text_color, c.text_primary);
        assert_eq!(style.tab.active.outline_color, egui::Color32::TRANSPARENT);
        assert_eq!(style.tab.inactive.bg_fill, c.bg_panel);
        assert_eq!(style.tab.inactive.text_color, c.text_secondary);
        assert_eq!(style.tab.inactive.outline_color, egui::Color32::TRANSPARENT);
        assert_eq!(style.tab.focused.bg_fill, c.bg_panel_alt);
        assert_eq!(style.tab.focused.text_color, c.accent);
        assert_eq!(style.tab.focused.outline_color, c.accent);
        assert_eq!(style.tab.hovered.bg_fill, c.bg_hover);
        assert_eq!(style.tab.hovered.text_color, c.text_primary);
        assert_eq!(style.tab.hovered.outline_color, egui::Color32::TRANSPARENT);
        assert_eq!(style.tab.active_with_kb_focus.bg_fill, c.bg_panel);
        assert_eq!(style.tab.active_with_kb_focus.text_color, c.text_primary);
        assert_eq!(
            style.tab.active_with_kb_focus.outline_color,
            egui::Color32::TRANSPARENT
        );
        assert_eq!(style.tab.inactive_with_kb_focus.bg_fill, c.bg_panel);
        assert_eq!(
            style.tab.inactive_with_kb_focus.text_color,
            c.text_secondary
        );
        assert_eq!(
            style.tab.inactive_with_kb_focus.outline_color,
            egui::Color32::TRANSPARENT
        );
        assert_eq!(style.tab.focused_with_kb_focus.bg_fill, c.bg_panel_alt);
        assert_eq!(style.tab.focused_with_kb_focus.text_color, c.accent);
        assert_eq!(style.tab.focused_with_kb_focus.outline_color, c.accent);
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

    // ------------------------------------------------------------------
    // Rendering-chain verification: the dock_style must actually reach the
    // shapes egui emits for the active tab (accent outline ring + accent
    // title). This is objective evidence that the active state is
    // distinguishable — it does not rely on eyeballing a screenshot.
    // ------------------------------------------------------------------

    /// Minimal tab for the rendering-chain test.
    #[derive(Clone, Debug)]
    struct TestTab(&'static str);

    /// Minimal viewer for the rendering-chain test.
    struct TestViewer;

    impl egui_dock::TabViewer for TestViewer {
        type Tab = TestTab;

        fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
            tab.0.into()
        }

        fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
            ui.label(tab.0);
        }
    }

    /// Recursively scan emitted shapes for text whose fallback color is `color`.
    fn shapes_contain_text_color(shapes: &[egui::Shape], color: egui::Color32) -> bool {
        shapes.iter().any(|shape| match shape {
            egui::Shape::Vec(inner) => shapes_contain_text_color(inner, color),
            egui::Shape::Text(text) => text.fallback_color == color,
            _ => false,
        })
    }

    /// Recursively scan emitted shapes for a rect stroke of `color` (outline ring).
    fn shapes_contain_stroke_color(shapes: &[egui::Shape], color: egui::Color32) -> bool {
        shapes.iter().any(|shape| match shape {
            egui::Shape::Vec(inner) => shapes_contain_stroke_color(inner, color),
            egui::Shape::Rect(rect) => rect.stroke.color == color,
            egui::Shape::Path(path) => path.stroke.color == egui::epaint::ColorMode::Solid(color),
            egui::Shape::LineSegment { stroke, .. } => stroke.color == color,
            _ => false,
        })
    }

    #[test]
    fn focused_tab_renders_accent_ring_while_others_stay_quiet() {
        use egui_dock::DockArea;

        // Mirror the real app layout: one tab per leaf (Chart root, Logger
        // and Screener split below). Every tab is "active" in its own leaf,
        // so only the focused leaf's tab may carry the accent ring.
        fn render_dock(
            dock_state: &mut egui_dock::DockState<TestTab>,
            viewer: &mut TestViewer,
        ) -> Vec<egui::Shape> {
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(600.0, 400.0))
                .build_ui(|ui| {
                    let style = dock_style(&ThemeTokens::dark());
                    DockArea::new(dock_state)
                        .style(style)
                        .show_inside(ui, viewer);
                });
            harness.run();
            harness
                .output()
                .shapes
                .iter()
                .map(|clipped| clipped.shape.clone())
                .collect()
        }

        let mut dock_state = egui_dock::DockState::new(vec![TestTab("Chart")]);
        if let Some(tree) = dock_state
            .get_surface_mut(egui_dock::SurfaceIndex::main())
            .and_then(|s| s.node_tree_mut())
        {
            let _ = tree.split_below(egui_dock::NodeIndex::root(), 0.5, vec![TestTab("Logger")]);
            let _ = tree.split_below(egui_dock::NodeIndex::root(), 0.5, vec![TestTab("Screener")]);
        }
        let mut viewer = TestViewer;

        let accent = ColorTokens::dark().accent;
        let quiet_colors: Vec<egui::Color32> = [
            ColorTokens::dark().text_primary,
            ColorTokens::dark().text_secondary,
        ]
        .to_vec();

        // Before any interaction no leaf is focused: no accent anywhere.
        let rendered_shapes = render_dock(&mut dock_state, &mut viewer);
        assert!(
            !shapes_contain_stroke_color(&rendered_shapes, accent)
                && !shapes_contain_text_color(&rendered_shapes, accent),
            "no focused leaf yet: accent must not appear, got colors: {}",
            unique_colors(&rendered_shapes)
        );

        // Click the Chart tab (top-left of the first tab bar) — the same
        // interaction path a user performs, which makes egui_dock focus the
        // root leaf and raise its tab with the accent ring.
        let (focused_after_click, rendered_shapes) = {
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(600.0, 400.0))
                .build_ui(|ui| {
                    let style = dock_style(&ThemeTokens::dark());
                    DockArea::new(&mut dock_state)
                        .style(style)
                        .show_inside(ui, &mut viewer);
                });
            harness.run();
            harness.event(egui::Event::PointerMoved(egui::pos2(40.0, 22.0)));
            harness.step();
            harness.event(egui::Event::PointerButton {
                pos: egui::pos2(40.0, 22.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
            harness.event(egui::Event::PointerButton {
                pos: egui::pos2(40.0, 22.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
            let shapes: Vec<egui::Shape> = harness
                .output()
                .shapes
                .iter()
                .map(|clipped| clipped.shape.clone())
                .collect();
            drop(harness);
            (dock_state.focused_leaf(), shapes)
        };
        assert!(
            focused_after_click.is_some(),
            "clicking the Chart tab must focus its leaf"
        );

        assert!(
            shapes_contain_stroke_color(&rendered_shapes, accent),
            "focused tab must emit an accent outline ring, got colors: {}",
            unique_colors(&rendered_shapes)
        );
        assert!(
            shapes_contain_text_color(&rendered_shapes, accent),
            "focused tab title must render in accent, got colors: {}",
            unique_colors(&rendered_shapes)
        );
        assert!(
            quiet_colors
                .iter()
                .any(|c| shapes_contain_text_color(&rendered_shapes, *c)),
            "non-focused tabs must stay quiet (text_primary/secondary titles), got colors: {}",
            unique_colors(&rendered_shapes)
        );
    }

    /// Debug helper: list every distinct color emitted by the dock rendering.
    fn unique_colors(shapes: &[egui::Shape]) -> String {
        let mut colors = std::collections::BTreeSet::new();
        fn walk(shapes: &[egui::Shape], colors: &mut std::collections::BTreeSet<String>) {
            for shape in shapes {
                match shape {
                    egui::Shape::Vec(inner) => walk(inner, colors),
                    egui::Shape::Rect(rect) => {
                        colors.insert(format!("{:?}", rect.fill));
                        colors.insert(format!("{:?}", rect.stroke.color));
                    }
                    egui::Shape::Path(path) => {
                        colors.insert(format!("{:?}", path.stroke.color));
                    }
                    egui::Shape::Text(text) => {
                        colors.insert(format!("{:?}", text.fallback_color));
                    }
                    egui::Shape::LineSegment { stroke, .. } => {
                        colors.insert(format!("{:?}", stroke.color));
                    }
                    _ => {}
                }
            }
        }
        walk(shapes, &mut colors);
        colors.into_iter().collect::<Vec<_>>().join(", ")
    }
}
