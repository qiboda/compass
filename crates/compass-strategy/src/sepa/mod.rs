//! SEPA (Stage Analysis + VCP) scoring engine building blocks (epic #139).
//!
//! Pure technical-indicator library over
//! [`CrossSectionBar`](compass_core::model::CrossSectionBar) cross-sections,
//! local concept-board daily aggregation ([`aggregation`]) and the
//! whole-market thermometer ([`temperature`]); the five-module scoring engine
//! arrives in a later sub-issue. All functions are pure, return `None`/0 on
//! insufficient windows and never panic or produce `NaN`.

/// Main engine look-back window (calendar days) shared by the thermometer and
/// the five-module scoring engine: `fetch_cross_section(now - 550, now)`.
pub const SEPA_WINDOW_DAYS: i64 = 550;

pub mod aggregation;
pub mod indicators;
pub mod temperature;
