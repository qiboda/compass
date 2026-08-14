//! compass-ui: general-purpose GUI component library and design token system for Compass.
//!
//! Pure UI layer with **no** dependency on business crates (`compass-core` /
//! `compass-types`); the binary crate (`compass`) depends on this crate, never
//! the other way around. It provides the design token system ([`tokens`]),
//! theme mapping, font registration and the base/composite widget library
//! (the latter added by later sub-issues of epic #119).
//!
//! i18n (issue #222): the crate declares its locale data via the shared
//! `compass-i18n` crate (the single `locales/` directory). `compass-i18n` is
//! UI infrastructure (a dictionary + rust-i18n re-exports), not a business
//! crate, so the zero-business-dependency contract is preserved.

#![warn(missing_docs)]

rust_i18n::i18n!("../compass-i18n/locales", fallback = "zh");

/// Design tokens: colors, spacing, typography, radii, shadows and motion durations.
///
/// See design doc `.dsh/designs/gui-upgrade.md` §4 for the full value spec.
pub mod tokens;

/// Font registration: SourceHanSansCN (Chinese) + JetBrains Mono (numeric).
pub mod fonts;

/// Theme: CompassTheme maps [`tokens::ThemeTokens`] to `egui::Visuals` + chart config,
/// plus the egui_dock `Style` builder.
pub mod theme;

/// egui_dock `Style` builder: maps [`tokens::ThemeTokens`] onto dock chrome
/// (tab bar, tabs, separators, borders, drag overlay).
pub mod dock_style;

/// Widget library: base atoms + composite molecules.
pub mod widgets;
