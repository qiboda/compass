//! Color palette tokens (design doc `.omo/designs/gui-upgrade.md` §4.1).

use egui::Color32;

/// Chart-specific color tokens, aligned with the egui-charts
/// `ChartSemanticTokens` mapping (design doc §4.1 `chart.*`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartTokens {
    /// Minor grid lines.
    pub grid_line: Color32,
    /// Major grid lines.
    pub grid_line_major: Color32,
    /// Crosshair line color.
    pub crosshair: Color32,
    /// Bullish (up) candle — A-share convention: red.
    pub candle_up: Color32,
    /// Bearish (down) candle — A-share convention: green.
    pub candle_down: Color32,
    /// Volume bars on up days ([`Self::candle_up`] at 60% alpha).
    pub volume_up: Color32,
    /// Volume bars on down days ([`Self::candle_down`] at 60% alpha).
    pub volume_down: Color32,
}

impl ChartTokens {
    /// Chart tokens for the dark palette.
    pub const fn dark() -> Self {
        Self {
            grid_line: Color32::from_rgb(0x2D, 0x32, 0x3C),
            grid_line_major: Color32::from_rgb(0x36, 0x3A, 0x45),
            crosshair: Color32::from_rgb(0x64, 0xA0, 0xA0),
            candle_up: Color32::from_rgb(0xEF, 0x53, 0x50),
            candle_down: Color32::from_rgb(0x26, 0xA6, 0x9A),
            volume_up: Color32::from_rgba_unmultiplied_const(0xEF, 0x53, 0x50, 153),
            volume_down: Color32::from_rgba_unmultiplied_const(0x26, 0xA6, 0x9A, 153),
        }
    }

    /// Chart tokens for the light palette.
    pub const fn light() -> Self {
        Self {
            grid_line: Color32::from_rgb(0xE4, 0xE7, 0xEC),
            grid_line_major: Color32::from_rgb(0xD6, 0xDA, 0xE0),
            crosshair: Color32::from_rgb(0x3D, 0x7A, 0x7A),
            candle_up: Color32::from_rgb(0xD9, 0x30, 0x25),
            candle_down: Color32::from_rgb(0x0E, 0x8F, 0x6E),
            volume_up: Color32::from_rgba_unmultiplied_const(0xD9, 0x30, 0x25, 153),
            volume_down: Color32::from_rgba_unmultiplied_const(0x0E, 0x8F, 0x6E, 153),
        }
    }
}

/// Full color palette for one theme (design doc §4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTokens {
    /// Application background (TradingView classic).
    pub bg_app: Color32,
    /// Panel / card / popover background.
    pub bg_panel: Color32,
    /// Secondary panel / toolbar / StatusBar background.
    pub bg_panel_alt: Color32,
    /// Row / control hover background.
    pub bg_hover: Color32,
    /// Selected / pressed state background.
    pub bg_active: Color32,
    /// Thin border.
    pub border: Color32,
    /// Separator / emphasized border.
    pub border_strong: Color32,
    /// Primary text.
    pub text_primary: Color32,
    /// Secondary text.
    pub text_secondary: Color32,
    /// Weak text / placeholders.
    pub text_weak: Color32,
    /// Disabled text.
    pub text_disabled: Color32,
    /// Accent — primary action / selection / links.
    pub accent: Color32,
    /// Accent hover state.
    pub accent_hover: Color32,
    /// Accent pressed state.
    pub accent_pressed: Color32,
    /// Up (A-share: red).
    pub up: Color32,
    /// Down (A-share: green).
    pub down: Color32,
    /// Flat / unchanged.
    pub flat: Color32,
    /// Success.
    pub success: Color32,
    /// Warning.
    pub warning: Color32,
    /// Error / danger.
    pub error: Color32,
    /// Informational.
    pub info: Color32,
    /// Selected row / text selection background ([`Self::accent`] at 20% alpha).
    pub selection_bg: Color32,
    /// Chart rendering colors.
    pub chart: ChartTokens,
}

impl ColorTokens {
    /// Dark palette (default theme, TradingView-style).
    pub const fn dark() -> Self {
        Self {
            bg_app: Color32::from_rgb(0x13, 0x17, 0x22),
            bg_panel: Color32::from_rgb(0x1E, 0x22, 0x2D),
            bg_panel_alt: Color32::from_rgb(0x2A, 0x2E, 0x39),
            bg_hover: Color32::from_rgb(0x2A, 0x2E, 0x39),
            bg_active: Color32::from_rgb(0x36, 0x3A, 0x45),
            border: Color32::from_rgb(0x2A, 0x2E, 0x39),
            border_strong: Color32::from_rgb(0x36, 0x3A, 0x45),
            text_primary: Color32::from_rgb(0xD1, 0xD4, 0xDC),
            text_secondary: Color32::from_rgb(0x78, 0x7B, 0x86),
            text_weak: Color32::from_rgb(0x5D, 0x60, 0x6B),
            text_disabled: Color32::from_rgb(0x46, 0x4A, 0x55),
            accent: Color32::from_rgb(0x29, 0x62, 0xFF),
            accent_hover: Color32::from_rgb(0x4D, 0x7F, 0xFF),
            accent_pressed: Color32::from_rgb(0x1E, 0x4F, 0xD6),
            up: Color32::from_rgb(0xEF, 0x53, 0x50),
            down: Color32::from_rgb(0x26, 0xA6, 0x9A),
            flat: Color32::from_rgb(0xD1, 0xD4, 0xDC),
            success: Color32::from_rgb(0x34, 0xC7, 0x7B),
            warning: Color32::from_rgb(0xF5, 0xA6, 0x23),
            error: Color32::from_rgb(0xEF, 0x53, 0x50),
            info: Color32::from_rgb(0x29, 0x62, 0xFF),
            selection_bg: Color32::from_rgba_unmultiplied_const(0x29, 0x62, 0xFF, 51),
            chart: ChartTokens::dark(),
        }
    }

    /// Light palette.
    pub const fn light() -> Self {
        Self {
            bg_app: Color32::from_rgb(0xF5, 0xF7, 0xFA),
            bg_panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            bg_panel_alt: Color32::from_rgb(0xED, 0xEF, 0xF2),
            bg_hover: Color32::from_rgb(0xE8, 0xEB, 0xEF),
            bg_active: Color32::from_rgb(0xDD, 0xE1, 0xE6),
            border: Color32::from_rgb(0xD6, 0xDA, 0xE0),
            border_strong: Color32::from_rgb(0xB8, 0xBE, 0xC7),
            text_primary: Color32::from_rgb(0x1B, 0x24, 0x30),
            text_secondary: Color32::from_rgb(0x5A, 0x64, 0x72),
            text_weak: Color32::from_rgb(0x8A, 0x93, 0xA0),
            text_disabled: Color32::from_rgb(0xB8, 0xBE, 0xC7),
            accent: Color32::from_rgb(0x29, 0x62, 0xFF),
            accent_hover: Color32::from_rgb(0x4D, 0x7F, 0xFF),
            accent_pressed: Color32::from_rgb(0x1E, 0x4F, 0xD6),
            up: Color32::from_rgb(0xD9, 0x30, 0x25),
            down: Color32::from_rgb(0x0E, 0x8F, 0x6E),
            flat: Color32::from_rgb(0x5A, 0x64, 0x72),
            success: Color32::from_rgb(0x18, 0x8A, 0x51),
            warning: Color32::from_rgb(0xB5, 0x7A, 0x00),
            error: Color32::from_rgb(0xD9, 0x30, 0x25),
            info: Color32::from_rgb(0x29, 0x62, 0xFF),
            selection_bg: Color32::from_rgba_unmultiplied_const(0x29, 0x62, 0xFF, 51),
            chart: ChartTokens::light(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dark-palette field asserted against the design doc §4.1 table.
    #[test]
    fn dark_palette_matches_design_spec() {
        let t = ColorTokens::dark();
        assert_eq!(t.bg_app, Color32::from_rgb(0x13, 0x17, 0x22));
        assert_eq!(t.bg_panel, Color32::from_rgb(0x1E, 0x22, 0x2D));
        assert_eq!(t.bg_panel_alt, Color32::from_rgb(0x2A, 0x2E, 0x39));
        assert_eq!(t.bg_hover, Color32::from_rgb(0x2A, 0x2E, 0x39));
        assert_eq!(t.bg_active, Color32::from_rgb(0x36, 0x3A, 0x45));
        assert_eq!(t.border, Color32::from_rgb(0x2A, 0x2E, 0x39));
        assert_eq!(t.border_strong, Color32::from_rgb(0x36, 0x3A, 0x45));
        assert_eq!(t.text_primary, Color32::from_rgb(0xD1, 0xD4, 0xDC));
        assert_eq!(t.text_secondary, Color32::from_rgb(0x78, 0x7B, 0x86));
        assert_eq!(t.text_weak, Color32::from_rgb(0x5D, 0x60, 0x6B));
        assert_eq!(t.text_disabled, Color32::from_rgb(0x46, 0x4A, 0x55));
        assert_eq!(t.accent, Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(t.accent_hover, Color32::from_rgb(0x4D, 0x7F, 0xFF));
        assert_eq!(t.accent_pressed, Color32::from_rgb(0x1E, 0x4F, 0xD6));
        assert_eq!(t.up, Color32::from_rgb(0xEF, 0x53, 0x50));
        assert_eq!(t.down, Color32::from_rgb(0x26, 0xA6, 0x9A));
        assert_eq!(t.flat, Color32::from_rgb(0xD1, 0xD4, 0xDC));
        assert_eq!(t.success, Color32::from_rgb(0x34, 0xC7, 0x7B));
        assert_eq!(t.warning, Color32::from_rgb(0xF5, 0xA6, 0x23));
        assert_eq!(t.error, Color32::from_rgb(0xEF, 0x53, 0x50));
        assert_eq!(t.info, Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(
            t.selection_bg,
            Color32::from_rgba_unmultiplied_const(0x29, 0x62, 0xFF, 51)
        );
    }

    /// Every light-palette field asserted against the design doc §4.1 table.
    #[test]
    fn light_palette_matches_design_spec() {
        let t = ColorTokens::light();
        assert_eq!(t.bg_app, Color32::from_rgb(0xF5, 0xF7, 0xFA));
        assert_eq!(t.bg_panel, Color32::from_rgb(0xFF, 0xFF, 0xFF));
        assert_eq!(t.bg_panel_alt, Color32::from_rgb(0xED, 0xEF, 0xF2));
        assert_eq!(t.bg_hover, Color32::from_rgb(0xE8, 0xEB, 0xEF));
        assert_eq!(t.bg_active, Color32::from_rgb(0xDD, 0xE1, 0xE6));
        assert_eq!(t.border, Color32::from_rgb(0xD6, 0xDA, 0xE0));
        assert_eq!(t.border_strong, Color32::from_rgb(0xB8, 0xBE, 0xC7));
        assert_eq!(t.text_primary, Color32::from_rgb(0x1B, 0x24, 0x30));
        assert_eq!(t.text_secondary, Color32::from_rgb(0x5A, 0x64, 0x72));
        assert_eq!(t.text_weak, Color32::from_rgb(0x8A, 0x93, 0xA0));
        assert_eq!(t.text_disabled, Color32::from_rgb(0xB8, 0xBE, 0xC7));
        assert_eq!(t.accent, Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(t.accent_hover, Color32::from_rgb(0x4D, 0x7F, 0xFF));
        assert_eq!(t.accent_pressed, Color32::from_rgb(0x1E, 0x4F, 0xD6));
        assert_eq!(t.up, Color32::from_rgb(0xD9, 0x30, 0x25));
        assert_eq!(t.down, Color32::from_rgb(0x0E, 0x8F, 0x6E));
        assert_eq!(t.flat, Color32::from_rgb(0x5A, 0x64, 0x72));
        assert_eq!(t.success, Color32::from_rgb(0x18, 0x8A, 0x51));
        assert_eq!(t.warning, Color32::from_rgb(0xB5, 0x7A, 0x00));
        assert_eq!(t.error, Color32::from_rgb(0xD9, 0x30, 0x25));
        assert_eq!(t.info, Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(
            t.selection_bg,
            Color32::from_rgba_unmultiplied_const(0x29, 0x62, 0xFF, 51)
        );
    }

    /// Chart tokens (dark) aligned with the egui-charts `ChartSemanticTokens` mapping.
    #[test]
    fn dark_chart_tokens_match_design_spec() {
        let c = ColorTokens::dark().chart;
        assert_eq!(c.grid_line, Color32::from_rgb(0x2D, 0x32, 0x3C));
        assert_eq!(c.grid_line_major, Color32::from_rgb(0x36, 0x3A, 0x45));
        assert_eq!(c.crosshair, Color32::from_rgb(0x64, 0xA0, 0xA0));
        assert_eq!(c.candle_up, Color32::from_rgb(0xEF, 0x53, 0x50));
        assert_eq!(c.candle_down, Color32::from_rgb(0x26, 0xA6, 0x9A));
        assert_eq!(
            c.volume_up,
            Color32::from_rgba_unmultiplied_const(0xEF, 0x53, 0x50, 153)
        );
        assert_eq!(
            c.volume_down,
            Color32::from_rgba_unmultiplied_const(0x26, 0xA6, 0x9A, 153)
        );
    }

    /// Chart tokens (light) aligned with the egui-charts `ChartSemanticTokens` mapping.
    #[test]
    fn light_chart_tokens_match_design_spec() {
        let c = ColorTokens::light().chart;
        assert_eq!(c.grid_line, Color32::from_rgb(0xE4, 0xE7, 0xEC));
        assert_eq!(c.grid_line_major, Color32::from_rgb(0xD6, 0xDA, 0xE0));
        assert_eq!(c.crosshair, Color32::from_rgb(0x3D, 0x7A, 0x7A));
        assert_eq!(c.candle_up, Color32::from_rgb(0xD9, 0x30, 0x25));
        assert_eq!(c.candle_down, Color32::from_rgb(0x0E, 0x8F, 0x6E));
        assert_eq!(
            c.volume_up,
            Color32::from_rgba_unmultiplied_const(0xD9, 0x30, 0x25, 153)
        );
        assert_eq!(
            c.volume_down,
            Color32::from_rgba_unmultiplied_const(0x0E, 0x8F, 0x6E, 153)
        );
    }

    /// A-share red-up/green-down consistency: candles equal the up/down text colors.
    #[test]
    fn candle_colors_follow_up_down_convention() {
        for palette in [ColorTokens::dark(), ColorTokens::light()] {
            assert_eq!(palette.chart.candle_up, palette.up);
            assert_eq!(palette.chart.candle_down, palette.down);
        }
    }

    /// Volume bars are the up/down colors at 60% alpha.
    #[test]
    fn volume_bars_are_semantic_colors_at_60_percent_alpha() {
        for palette in [ColorTokens::dark(), ColorTokens::light()] {
            assert_eq!(palette.chart.volume_up.a(), 153);
            assert_eq!(palette.chart.volume_down.a(), 153);
        }
    }

    /// Selection background is accent at 20% alpha in both palettes.
    #[test]
    fn selection_bg_is_accent_at_20_percent_alpha() {
        for palette in [ColorTokens::dark(), ColorTokens::light()] {
            assert_eq!(palette.selection_bg.a(), 51);
        }
    }

    /// Design aliases: error == up (A-share red), info == accent, flat == text_primary (dark).
    #[test]
    fn semantic_color_aliases_hold() {
        let dark = ColorTokens::dark();
        assert_eq!(dark.error, dark.up);
        assert_eq!(dark.info, dark.accent);
        assert_eq!(dark.flat, dark.text_primary);
        let light = ColorTokens::light();
        assert_eq!(light.error, light.up);
        assert_eq!(light.info, light.accent);
    }
}
