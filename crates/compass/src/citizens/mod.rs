pub mod chart;
pub mod indicators;
pub mod logger;
pub mod screener;
pub mod sepa;

// Epic #217 requirement-acceptance tests. Mounted here (rather than inside
// `main.rs`'s test module) because the test-agent sandbox locks the crate
// root `src/main.rs`; the helpers mirror the main.rs test utilities 1:1.
#[cfg(test)]
pub(crate) mod ui_fixes_218;
#[cfg(test)]
pub(crate) mod ui_fixes_221;
