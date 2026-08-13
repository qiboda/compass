pub mod chart;
pub mod indicators;
pub mod logger;
pub mod screener;
pub mod screener_builder;
pub mod sepa;

// Epic #217 requirement-acceptance tests. The app-construction helpers in
// `ui_fixes_218` are the canonical test builders — `main.rs`'s own test
// module delegates to them (`use crate::citizens::ui_fixes_218::{...}`) so
// the `timeframe_index` derivation stays in one place.
#[cfg(test)]
pub(crate) mod ui_fixes_218;
#[cfg(test)]
pub(crate) mod ui_fixes_221;
