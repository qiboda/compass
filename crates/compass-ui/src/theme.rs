//! Theme: `CompassTheme` maps design tokens ([`crate::tokens::ThemeTokens`]) to egui
//! `Visuals`/`Style` directly, plus a thin wrapper mapping chart tokens onto
//! the egui-charts `ChartSemanticTokens` (sub-issue #126, S3).
//!
//! The UI layer is fully self-owned: `apply_theme` constructs
//! [`egui::Visuals`] and [`egui::Style`] from [`crate::tokens::ThemeTokens`] directly —
//! the chart library's global egui theming helper is never invoked. Only
//! chart rendering keeps a thin adapter (`apply_to_chart`), because the
//! chart widget's internal rendering is deeply bound to its own theme.
//!
//! Interface compatibility with the pre-upgrade `crates/compass/src/theme.rs`:
//! `compass_dark()` / `compass_light()` / `from_config()` / `all_names()` /
//! `name()` are preserved so the binary crate only needs a re-export.

use std::collections::BTreeMap;

use crate::tokens::ThemeTokens;

/// A named color theme for the Compass GUI, owning its design tokens.
///
/// Themes define token palettes for the whole application. Built-in themes
/// are `compass_dark` (default, TradingView-style) and `compass_light`.
#[derive(Debug, Clone, PartialEq)]
pub struct CompassTheme {
    /// Human-readable theme name shown in settings UI.
    name: &'static str,
    /// Design tokens this theme maps to egui.
    tokens: ThemeTokens,
}

impl CompassTheme {
    /// Returns the default dark theme ("compass_dark").
    pub fn compass_dark() -> Self {
        Self {
            name: "compass_dark",
            tokens: ThemeTokens::dark(),
        }
    }

    /// Returns the light theme ("compass_light").
    pub fn compass_light() -> Self {
        Self {
            name: "compass_light",
            tokens: ThemeTokens::light(),
        }
    }

    /// Resolve a theme from its config name string.
    ///
    /// Unknown names fall back to `compass_dark`.
    pub fn from_config(name: &str) -> Self {
        match name {
            "compass_light" => Self::compass_light(),
            _ => Self::compass_dark(),
        }
    }

    /// Returns all built-in themes; the default (`compass_dark`) is first.
    pub fn all() -> Vec<Self> {
        vec![Self::compass_dark(), Self::compass_light()]
    }

    /// Returns the ordered list of available theme names as a static slice.
    ///
    /// The default theme ("compass_dark") is always first. Used by the toolbar
    /// theme switcher to populate the dropdown without allocating a `Vec`.
    pub fn all_names() -> &'static [&'static str] {
        &["compass_dark", "compass_light"]
    }

    /// Returns the human-readable name of this theme instance.
    pub fn name(&self) -> &str {
        self.name
    }

    /// Returns the design tokens backing this theme.
    pub fn tokens(&self) -> &ThemeTokens {
        &self.tokens
    }

    /// Apply this theme to egui's visual system.
    ///
    /// Should be called at the start of each frame (in the app's `update()`
    /// function). Constructs [`egui::Visuals`] + [`egui::Style`] directly from
    /// the design tokens and installs them via
    /// [`egui::Context::set_style_of`] — the UI theme no longer depends on the
    /// chart library.
    pub fn apply_theme(&self, ctx: &egui::Context) {
        let theme = if Self::bg_is_dark(self.tokens.color.bg_app) {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.set_theme(theme);
        ctx.set_style_of(theme, self.to_style());
    }

    /// Apply this theme's colors to a chart widget.
    ///
    /// Maps the chart color tokens onto the egui-charts `ChartSemanticTokens`,
    /// applies them through `Theme::apply_to_config`, and sets the crosshair
    /// line colors (which live on `ChartOptions`, not `ChartConfig`).
    pub fn apply_to_chart(&self, chart: &mut egui_charts::Chart) {
        let preset = if Self::bg_is_dark(self.tokens.color.bg_app) {
            egui_charts::theme::ThemePreset::Dark
        } else {
            egui_charts::theme::ThemePreset::Light
        };
        let mut theme = egui_charts::theme::Theme::from_preset(preset);
        let c = &self.tokens.color;
        theme.semantic.chart = egui_charts::theme::ChartSemanticTokens {
            bg: c.bg_app,
            bg_axis: c.bg_panel,
            bg_tooltip: c.bg_panel,
            bg_legend: c.bg_panel,
            bg_selection: c.selection_bg,
            grid_line: c.chart.grid_line,
            grid_line_major: c.chart.grid_line_major,
            axis_text: c.text_secondary,
            axis_text_secondary: c.text_weak,
            crosshair_line: c.chart.crosshair,
            crosshair_label_bg: c.bg_panel_alt,
            crosshair_label_text: c.text_primary,
            candle_up: c.chart.candle_up,
            candle_up_border: c.chart.candle_up,
            candle_up_wick: c.chart.candle_up,
            candle_down: c.chart.candle_down,
            candle_down_border: c.chart.candle_down,
            candle_down_wick: c.chart.candle_down,
            volume_up: c.chart.volume_up,
            volume_down: c.chart.volume_down,
            price_line: c.accent,
            price_label_bg: c.bg_panel_alt,
            price_label_text: c.text_primary,
            watermark: c.text_weak,
        };
        let config = chart.config.clone();
        chart.config = theme.apply_to_config(config);

        // Crosshair colors live on ChartOptions, not ChartConfig.
        chart.chart_options.crosshair.vert_line_color = c.chart.crosshair;
        chart.chart_options.crosshair.horz_line_color = c.chart.crosshair;
    }

    /// Build the full [`egui::Style`] this theme maps to.
    fn to_style(&self) -> egui::Style {
        let t = &self.tokens;
        egui::Style {
            visuals: Self::to_visuals(t),
            spacing: egui::Spacing {
                item_spacing: egui::vec2(t.spacing.sm, t.spacing.xs),
                button_padding: egui::vec2(t.spacing.md, t.spacing.xs),
                interact_size: egui::vec2(40.0, t.spacing.control_md),
                indent: t.spacing.lg,
                ..Default::default()
            },
            text_styles: BTreeMap::from([
                (
                    egui::TextStyle::Heading,
                    egui::FontId::new(t.typography.heading, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Body,
                    egui::FontId::new(t.typography.body, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Button,
                    egui::FontId::new(t.typography.body, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Small,
                    egui::FontId::new(t.typography.caption, egui::FontFamily::Proportional),
                ),
                (
                    egui::TextStyle::Monospace,
                    egui::FontId::new(t.typography.mono, egui::FontFamily::Monospace),
                ),
            ]),
            ..Default::default()
        }
    }

    /// Map design tokens onto an [`egui::Visuals`] instance.
    ///
    /// Field mapping (see design doc §3.2 / §4):
    /// - surfaces: `panel_fill`/`window_fill` ← `bg_panel`, `extreme_bg_color`
    ///   ← `bg_app`, `faint_bg_color` ← `bg_panel_alt`
    /// - widget states: `hovered.weak_bg_fill` ← `bg_hover`,
    ///   `active.bg_fill` ← `bg_active`, `inactive.bg_fill` ← `bg_panel`
    /// - text hierarchy: default text (`inactive`/`hovered`/`active`/`open`
    ///   `fg_stroke`) ← `text_primary`, disabled (`noninteractive.fg_stroke`)
    ///   ← `text_disabled`, weak (`weak_text_color`) ← `text_weak`.
    ///   `text_secondary` has no dedicated egui `Visuals` slot; it is consumed
    ///   by the component/dock layers (e.g. `dock_style` inactive tab labels).
    /// - selection ← `selection_bg` + `accent` stroke, links ← `accent`
    /// - status colors ← `warning`/`error`, shadows ← `shadow.popup`
    fn to_visuals(t: &ThemeTokens) -> egui::Visuals {
        let c = &t.color;
        let dark = Self::bg_is_dark(c.bg_app);
        let base = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        egui::Visuals {
            dark_mode: dark,
            // Surfaces.
            panel_fill: c.bg_panel,
            window_fill: c.bg_panel,
            extreme_bg_color: c.bg_app,
            faint_bg_color: c.bg_panel_alt,
            code_bg_color: c.bg_panel_alt,
            // Widget states.
            widgets: egui::style::Widgets {
                noninteractive: egui::style::WidgetVisuals {
                    fg_stroke: egui::Stroke::new(1.0, c.text_disabled),
                    ..base.widgets.noninteractive
                },
                inactive: egui::style::WidgetVisuals {
                    bg_fill: c.bg_panel,
                    fg_stroke: egui::Stroke::new(1.0, c.text_primary),
                    ..base.widgets.inactive
                },
                hovered: egui::style::WidgetVisuals {
                    weak_bg_fill: c.bg_hover,
                    fg_stroke: egui::Stroke::new(1.0, c.text_primary),
                    ..base.widgets.hovered
                },
                active: egui::style::WidgetVisuals {
                    bg_fill: c.bg_active,
                    weak_bg_fill: c.bg_active,
                    fg_stroke: egui::Stroke::new(1.0, c.text_primary),
                    ..base.widgets.active
                },
                open: egui::style::WidgetVisuals {
                    bg_fill: c.bg_panel_alt,
                    fg_stroke: egui::Stroke::new(1.0, c.text_primary),
                    ..base.widgets.open
                },
            },
            // Text color hierarchy.
            weak_text_color: Some(c.text_weak),
            // Selection & links.
            selection: egui::style::Selection {
                bg_fill: c.selection_bg,
                stroke: egui::Stroke::new(1.0, c.accent),
            },
            hyperlink_color: c.accent,
            // Status colors.
            warn_fg_color: c.warning,
            error_fg_color: c.error,
            // Shadows.
            window_shadow: t.shadow.popup,
            popup_shadow: t.shadow.popup,
            ..base
        }
    }

    /// Whether a background color is dark, by ITU-R BT.601 luma (< 128).
    fn bg_is_dark(bg: egui::Color32) -> bool {
        let [r, g, b, _] = bg.to_array();
        let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
        luma < 128.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{ColorTokens, ShadowTokens, SpacingTokens, TypeTokens};

    #[test]
    fn compass_dark_has_name_and_tokens() {
        let theme = CompassTheme::compass_dark();
        assert_eq!(theme.name(), "compass_dark");
        assert_eq!(theme.tokens().color, ColorTokens::dark());
    }

    #[test]
    fn compass_light_has_name_and_tokens() {
        let theme = CompassTheme::compass_light();
        assert_eq!(theme.name(), "compass_light");
        assert_eq!(theme.tokens().color, ColorTokens::light());
    }

    #[test]
    fn all_names_returns_both_themes_dark_first() {
        assert_eq!(
            CompassTheme::all_names(),
            &["compass_dark", "compass_light"]
        );
        assert_eq!(CompassTheme::all_names().len(), 2);
    }

    #[test]
    fn all_returns_both_themes_dark_first() {
        let all = CompassTheme::all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name(), "compass_dark");
        assert_eq!(all[1].name(), "compass_light");
    }

    #[test]
    fn from_config_known_names_return_correct_themes() {
        assert_eq!(
            CompassTheme::from_config("compass_dark").name(),
            "compass_dark"
        );
        assert_eq!(
            CompassTheme::from_config("compass_light").name(),
            "compass_light"
        );
    }

    #[test]
    fn from_config_unknown_name_falls_back_to_dark() {
        assert_eq!(
            CompassTheme::from_config("nonexistent_xyz").name(),
            "compass_dark"
        );
        assert_eq!(CompassTheme::from_config("").name(), "compass_dark");
    }

    #[test]
    fn apply_theme_dark_sets_dark_mode_and_panel_fill() {
        let ctx = egui::Context::default();
        CompassTheme::compass_dark().apply_theme(&ctx);
        let visuals = &ctx.style_of(egui::Theme::Dark).visuals;
        assert!(visuals.dark_mode, "dark theme must enable dark mode");
        assert_eq!(visuals.panel_fill, ColorTokens::dark().bg_panel);
    }

    #[test]
    fn apply_theme_light_sets_light_mode_and_panel_fill() {
        let ctx = egui::Context::default();
        CompassTheme::compass_light().apply_theme(&ctx);
        let visuals = &ctx.style_of(egui::Theme::Light).visuals;
        assert!(!visuals.dark_mode, "light theme must disable dark mode");
        assert_eq!(visuals.panel_fill, ColorTokens::light().bg_panel);
    }

    #[test]
    fn apply_theme_maps_surfaces_widgets_selection_and_shadow() {
        let ctx = egui::Context::default();
        CompassTheme::compass_dark().apply_theme(&ctx);
        let visuals = &ctx.style_of(egui::Theme::Dark).visuals;
        let c = ColorTokens::dark();
        assert_eq!(visuals.window_fill, c.bg_panel);
        assert_eq!(visuals.extreme_bg_color, c.bg_app);
        assert_eq!(visuals.widgets.hovered.weak_bg_fill, c.bg_hover);
        assert_eq!(visuals.widgets.active.bg_fill, c.bg_active);
        assert_eq!(visuals.selection.bg_fill, c.selection_bg);
        assert_eq!(visuals.hyperlink_color, c.accent);
        assert_eq!(visuals.weak_text_color, Some(c.text_weak));
        assert_eq!(
            visuals.widgets.noninteractive.fg_stroke.color,
            c.text_disabled
        );
        assert_eq!(visuals.window_shadow, ShadowTokens::dark().popup);
    }

    #[test]
    fn apply_theme_sets_text_style_sizes_from_type_tokens() {
        let ctx = egui::Context::default();
        CompassTheme::compass_dark().apply_theme(&ctx);
        let text_styles = &ctx.style_of(egui::Theme::Dark).text_styles;
        let t = TypeTokens::default();
        assert_eq!(text_styles[&egui::TextStyle::Heading].size, t.heading);
        assert_eq!(text_styles[&egui::TextStyle::Body].size, t.body);
        assert_eq!(text_styles[&egui::TextStyle::Small].size, t.caption);
        assert_eq!(text_styles[&egui::TextStyle::Monospace].size, t.mono);
    }

    #[test]
    fn apply_theme_sets_spacing_from_spacing_tokens() {
        let ctx = egui::Context::default();
        CompassTheme::compass_dark().apply_theme(&ctx);
        let s = SpacingTokens::default();
        let style = ctx.style_of(egui::Theme::Dark);
        assert_eq!(style.spacing.item_spacing, egui::vec2(s.sm, s.xs));
        assert_eq!(style.spacing.interact_size.y, s.control_md);
    }

    #[test]
    fn apply_to_chart_maps_candle_grid_and_background_colors() {
        let theme = CompassTheme::compass_dark();
        let mut chart = egui_charts::Chart::new(egui_charts::model::BarData::new());
        theme.apply_to_chart(&mut chart);
        let c = ColorTokens::dark();
        assert_eq!(
            chart.config.bullish_color, c.chart.candle_up,
            "bullish must be A-share red"
        );
        assert_eq!(
            chart.config.bearish_color, c.chart.candle_down,
            "bearish must be A-share green"
        );
        assert_eq!(chart.config.grid_color, c.chart.grid_line);
        assert_eq!(chart.config.background_color, c.bg_app);
    }

    #[test]
    fn apply_to_chart_sets_crosshair_from_chart_tokens() {
        let theme = CompassTheme::compass_dark();
        let mut chart = egui_charts::Chart::new(egui_charts::model::BarData::new());
        theme.apply_to_chart(&mut chart);
        let expected = ColorTokens::dark().chart.crosshair;
        assert_eq!(chart.chart_options.crosshair.vert_line_color, expected);
        assert_eq!(chart.chart_options.crosshair.horz_line_color, expected);
    }
}
