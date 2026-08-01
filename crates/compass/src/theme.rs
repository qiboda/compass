//! GUI color theme system.
//!
//! Since epic #119 (S3), the theme implementation lives in `compass-ui`:
//! [`CompassTheme`] maps the design tokens to `egui::Visuals`/`Style`
//! directly (no longer wrapping `egui_charts::theme::Theme`). This module is
//! kept as a thin re-export so binary-crate call sites
//! (`crate::theme::CompassTheme`) stay unchanged.

pub use compass_ui::theme::CompassTheme;
