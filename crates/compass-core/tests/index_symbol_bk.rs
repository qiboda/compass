//! Adversarial tests: `BK` board-code namespace in the symbol system
//! (epic #255, plan T2 / C3).
//!
//! Plan contract under attack:
//! - `parse_explicit_prefix("BK0475") == ("BK", "0475")`
//! - `exchange_of_symbol` returns `BK` as-is for the BK prefix
//! - case-insensitive `bk` prefix must normalize to uppercase `BK`
//!
//! These are RED against the current code: the BK branch does not exist yet,
//! so `parse_explicit_prefix("BK0475")` falls through to `("", "BK0475")` and
//! `exchange_of_symbol` falls back to the bare-code heuristic (`SZ`).
//!
//! Why `tests/` instead of the in-source `#[cfg(test)]` module: the sandbox
//! denies writes to `src/**` (write/edit only allowed under `**/tests/**`),
//! mirroring the precedent in `crates/compass-data/tests/data_quality_adversarial.rs`.
//! All functions under test are `pub`, so integration tests reach them.

use compass_core::data::symbol::{exchange_of_symbol, parse_explicit_prefix};

// ---------------------------------------------------------------------------
// parse_explicit_prefix
// ---------------------------------------------------------------------------

#[test]
fn parse_explicit_prefix_bk_prefix() {
    // Plan acceptance: parse_explicit_prefix("BK0475") == ("BK", "0475").
    assert_eq!(parse_explicit_prefix("BK0475"), ("BK", "0475"));
}

#[test]
fn parse_explicit_prefix_bk_boundaries() {
    // 4-digit code extremes — both must parse, not just "nice" mid-range codes.
    assert_eq!(parse_explicit_prefix("BK0000"), ("BK", "0000"));
    assert_eq!(parse_explicit_prefix("BK9999"), ("BK", "9999"));
}

#[test]
fn parse_explicit_prefix_bk_case_insensitive() {
    assert_eq!(parse_explicit_prefix("bk0475"), ("BK", "0475"));
    assert_eq!(parse_explicit_prefix("bK0475"), ("BK", "0475"));
}

#[test]
fn parse_explicit_prefix_bk_does_not_break_existing_prefixes() {
    // Guard: adding the BK branch must not regress SH/SZ/BJ parsing.
    assert_eq!(parse_explicit_prefix("SZ000001"), ("SZ", "000001"));
    assert_eq!(parse_explicit_prefix("SH600519"), ("SH", "600519"));
    assert_eq!(parse_explicit_prefix("bj830799"), ("BJ", "830799"));
}

// ---------------------------------------------------------------------------
// exchange_of_symbol
// ---------------------------------------------------------------------------

#[test]
fn exchange_of_symbol_bk_passthrough() {
    // Plan: exchange_of_symbol 对 BK 前缀原样返回.
    // RED: currently falls back to the bare-code heuristic → "SZ".
    assert_eq!(exchange_of_symbol("BK0475"), "BK");
    assert_eq!(exchange_of_symbol("BK0000"), "BK");
}

#[test]
fn exchange_of_symbol_bk_lowercase() {
    assert_eq!(exchange_of_symbol("bk0475"), "BK");
}

#[test]
fn exchange_of_symbol_bk_does_not_break_existing_exchanges() {
    assert_eq!(exchange_of_symbol("SZ000001"), "SZ");
    assert_eq!(exchange_of_symbol("SH600519"), "SH");
    assert_eq!(exchange_of_symbol("BJ830799"), "BJ");
}
