//! GUI color theme system.
//!
//! Themes define color palettes for the chart application UI. Built-in themes
//! include `compass_dark` (default, TradingView-style) and `compass_light`.
//!
//! Each `CompassTheme` wraps an [`egui_charts::theme::Theme`] internally and
//! provides convenience methods for applying the theme to egui's visual system
//! and to chart widgets.

use egui_charts::theme::ThemePreset;

/// A named color theme for the Compass GUI.
///
/// Wraps an [`egui_charts::theme::Theme`] internally and adds a
/// human-readable name shown in settings UI.
#[derive(Debug, Clone)]
pub struct CompassTheme {
    /// Human-readable theme name shown in settings UI.
    pub name: String,
    /// The underlying egui-charts theme providing all semantic color tokens.
    inner: egui_charts::theme::Theme,
}

impl CompassTheme {
    /// Returns the default dark theme ("compass_dark").
    ///
    /// Based on [`ThemePreset::Dark`] with compass brand overrides applied
    /// to semantic chart tokens.
    pub fn compass_dark() -> Self {
        let mut theme = egui_charts::theme::Theme::from_preset(ThemePreset::Dark);
        // Compass brand overrides: use a slightly warmer candle-down color
        // and a muted teal grid to distinguish from the base Dark preset.
        theme.semantic.chart.grid_line = egui::Color32::from_rgb(45, 50, 60);
        theme.semantic.chart.crosshair_line = egui::Color32::from_rgb(100, 160, 160);

        Self {
            name: "compass_dark".to_string(),
            inner: theme,
        }
    }

    /// Returns the light theme ("compass_light").
    ///
    /// Based on [`ThemePreset::Light`] for a bright reading experience.
    pub fn compass_light() -> Self {
        let theme = egui_charts::theme::Theme::from_preset(ThemePreset::Light);

        Self {
            name: "compass_light".to_string(),
            inner: theme,
        }
    }

    /// Returns the blue-tinted theme ("compass_blue").
    /// Apply this theme to egui's visual system.
    ///
    /// Should be called at the start of each frame (in the app's `update()`
    /// function). Delegates to [`egui_charts::theme::apply_to_egui`].
    pub fn apply_theme(&self, ctx: &egui::Context) {
        egui_charts::theme::apply_to_egui(ctx, &self.inner);
    }

    /// Apply this theme's colors to a chart widget.
    ///
    /// Maps semantic chart tokens onto the chart's [`ChartConfig`] and sets
    /// crosshair line colors from the theme's token palette.
    pub fn apply_to_chart(&self, chart: &mut egui_charts::Chart) {
        let config = chart.config.clone();
        chart.config = self.inner.apply_to_config(config);

        // Crosshair colors live on ChartOptions, not ChartConfig.
        chart.chart_options.crosshair.vert_line_color =
            self.inner.semantic.chart.crosshair_line;
        chart.chart_options.crosshair.horz_line_color =
            self.inner.semantic.chart.crosshair_line;
    }

    /// Resolve a theme from its config name string.
    ///
    /// Maps known theme names to their constructors:
    /// - `"compass_dark"` → [`compass_dark`](Self::compass_dark)
    /// - `"compass_light"` → [`compass_light`](Self::compass_light)
    /// - `"compass_blue"` → [`compass_blue`](Self::compass_blue)
    ///
    /// Unknown names fall back to `compass_dark`.
    pub fn from_config(name: &str) -> Self {
        match name {
            "compass_light" => Self::compass_light(),
            _ => Self::compass_dark(),
        }
    }

    /// Returns all built-in themes as a vector.
    ///
    /// Useful for populating theme-switcher dropdowns in settings UI.
    /// The default theme (`compass_dark`) is always first.
    #[allow(dead_code)]
    pub fn all() -> Vec<Self> {
        vec![Self::compass_dark(), Self::compass_light()]
    }

    /// Returns the ordered list of available theme names as a static slice.
    ///
    /// The default theme ("compass_dark") is always first. Used by the toolbar
    /// theme switcher to populate the dropdown without allocating a Vec.
    pub fn all_names() -> &'static [&'static str] {
        &["compass_dark", "compass_light"]
    }

    /// Returns the human-readable name of this theme instance.
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compass_dark_returns_valid_theme() {
        let theme = CompassTheme::compass_dark();
        assert_eq!(theme.name, "compass_dark");
    }

    #[test]
    fn theme_names_includes_compass_dark_first() {
        let names = CompassTheme::all_names();
        assert!(!names.is_empty(), "theme list must not be empty");
        assert_eq!(names[0], "compass_dark");
    }
}
