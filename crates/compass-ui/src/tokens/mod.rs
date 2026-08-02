//! Design token system for the Compass GUI (design doc `.omo/designs/gui-upgrade.md` §4).
//!
//! Six token categories are defined here — color ([`crate::tokens::ColorTokens`]),
//! spacing ([`crate::tokens::SpacingTokens`]), typography ([`crate::tokens::TypeTokens`]),
//! radius ([`crate::tokens::RadiusTokens`]), shadow ([`crate::tokens::ShadowTokens`])
//! and motion ([`crate::tokens::MotionTokens`]) — plus the [`ThemeTokens`]
//! aggregate providing complete dark/light palettes.
//!
//! [`ThemeTokens`]: crate::tokens::ThemeTokens

mod color;
mod motion;
mod radius;
mod shadow;
mod spacing;
mod typography;

pub use color::{ChartTokens, ColorTokens};
pub use motion::MotionTokens;
pub use radius::RadiusTokens;
pub use shadow::ShadowTokens;
pub use spacing::SpacingTokens;
pub use typography::TypeTokens;

/// Aggregate of all six token categories for one theme (dark or light).
///
/// Theme-independent scales ([`crate::tokens::SpacingTokens`], [`crate::tokens::TypeTokens`],
/// [`crate::tokens::RadiusTokens`], [`crate::tokens::MotionTokens`]) are shared;
/// only color and shadow differ between palettes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemeTokens {
    /// Color palette.
    pub color: ColorTokens,
    /// Spacing / dimension scale.
    pub spacing: SpacingTokens,
    /// Font size scale.
    pub typography: TypeTokens,
    /// Corner radius scale.
    pub radius: RadiusTokens,
    /// Popup / modal shadows.
    pub shadow: ShadowTokens,
    /// Motion durations.
    pub motion: MotionTokens,
}

impl ThemeTokens {
    /// Dark theme (TradingView-style, the default).
    pub fn dark() -> Self {
        Self {
            color: ColorTokens::dark(),
            spacing: SpacingTokens::default(),
            typography: TypeTokens::default(),
            radius: RadiusTokens::default(),
            shadow: ShadowTokens::dark(),
            motion: MotionTokens::default(),
        }
    }

    /// Light theme.
    pub fn light() -> Self {
        Self {
            color: ColorTokens::light(),
            spacing: SpacingTokens::default(),
            typography: TypeTokens::default(),
            radius: RadiusTokens::default(),
            shadow: ShadowTokens::light(),
            motion: MotionTokens::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both palettes must contain every field defined in design doc §4.
    #[test]
    fn dark_and_light_themes_aggregate_all_token_categories() {
        for theme in [ThemeTokens::dark(), ThemeTokens::light()] {
            let _ = (
                theme.color,
                theme.spacing,
                theme.typography,
                theme.radius,
                theme.shadow,
                theme.motion,
            );
        }
    }

    #[test]
    fn dark_theme_matches_dark_token_sets() {
        let dark = ThemeTokens::dark();
        assert_eq!(dark.color, ColorTokens::dark());
        assert_eq!(dark.spacing, SpacingTokens::default());
        assert_eq!(dark.typography, TypeTokens::default());
        assert_eq!(dark.radius, RadiusTokens::default());
        assert_eq!(dark.shadow, ShadowTokens::dark());
        assert_eq!(dark.motion, MotionTokens::default());
    }

    #[test]
    fn light_theme_matches_light_token_sets() {
        let light = ThemeTokens::light();
        assert_eq!(light.color, ColorTokens::light());
        assert_eq!(light.shadow, ShadowTokens::light());
    }

    /// The two palettes must actually differ (colors are the discriminating axis).
    #[test]
    fn dark_and_light_palettes_differ() {
        assert_ne!(ThemeTokens::dark().color, ThemeTokens::light().color);
        assert_ne!(ThemeTokens::dark().shadow, ThemeTokens::light().shadow);
    }
}
