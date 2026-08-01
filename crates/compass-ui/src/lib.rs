//! compass-ui: general-purpose GUI component library and design token system for Compass.
//!
//! Pure UI layer with **no** dependency on business crates (`compass-core` /
//! `compass-types`); the binary crate (`compass`) depends on this crate, never
//! the other way around. It provides the design token system ([`tokens`]),
//! theme mapping, font registration and the base/composite widget library
//! (the latter added by later sub-issues of epic #119).

#![warn(missing_docs)]

/// Design tokens: colors, spacing, typography, radii, shadows and motion durations.
///
/// See design doc `.omo/designs/gui-upgrade.md` §4 for the full value spec.
pub mod tokens;
