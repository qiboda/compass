//! SEPA (Stage Analysis + VCP) scoring engine building blocks (epic #139).
//!
//! Pure technical-indicator library over
//! [`CrossSectionBar`](compass_core::model::CrossSectionBar) cross-sections,
//! plus (in later sub-issues) concept aggregation, the market thermometer and
//! the five-module scoring engine. All indicator functions are pure, return
//! `None`/0 on insufficient windows and never panic or produce `NaN`.

pub mod indicators;
