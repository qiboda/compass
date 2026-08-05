//! SEPA (Stage Analysis + VCP) scoring engine building blocks (epic #139).
//!
//! Pure technical-indicator library over
//! [`CrossSectionBar`](compass_core::model::CrossSectionBar) cross-sections,
//! local concept-board daily aggregation ([`aggregation`]), the
//! whole-market thermometer ([`temperature`]) and the five-module scoring
//! engine ([`scoring`]). All functions are pure, return `None`/0 on
//! insufficient windows and never panic or produce `NaN`.

/// Main engine look-back window (calendar days) shared by the thermometer and
/// the five-module scoring engine: `fetch_cross_section(now - 550, now)`.
pub const SEPA_WINDOW_DAYS: i64 = 550;

pub mod aggregation;
pub mod backtest;
pub mod indicators;
pub mod scoring;
pub mod temperature;

pub use scoring::run_sepa;
pub(crate) use scoring::{dedup_bars, fetch_sepa_window, score_sepa};
