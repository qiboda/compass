//! Adversarial tests — epic #217 sub-issue #219 (Chinese date formats in the
//! egui-charts fork).
//!
//! These assertions exercise the *public* formatting API of the
//! `egui-charts` fork (git dependency at the commit pinned in Cargo.lock,
//! currently a1531ac). They double as a contract guard: if the pinned fork
//! commit regresses to English (`%b` / `%b %d`) or zero-padded forms, or the
//! dependency pin is rolled back, these tests turn RED again.
//!
//! Rationale for living here instead of the fork: the fork's verification
//! command is `cargo test --lib` (in-source `#[cfg(test)]` only), and this
//! sandbox denies writes to the fork's `src/**`. A `tests/` integration test
//! in the fork would never be compiled by `cargo test --lib`. A test in the
//! *consumer* crate is compiled and run by `cargo test -p compass` (proven by
//! `probe_bin_tests.rs`), reaches the fork through the public API, and
//! doubles as a contract guard against format regressions in the pinned
//! dependency.
//!
//! Format contract (plan-locked, Metis B1): `%-m`/`%-d` de-padded — "6月",
//! "6月15日"; zero-padded "06月"/"6月01日" must fail.

use chrono::{TimeZone, Utc};
use egui_charts::config::TimezoneMode;
use egui_charts::scales::{
    DefaultTimeFormatter, TickMarkType, TimeFormatter, TimeFormatterBuilder,
};

/// Month labels must be Chinese and de-padded for single-digit months.
/// Guard: a regression to English ("Jan"/"Jun"/"Oct"/"Dec") fails here.
#[test]
fn adversarial_219_month_labels_chinese_depadded() {
    egui_charts::set_locale("zh");
    let f = DefaultTimeFormatter::default();
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap(),
            TickMarkType::Month
        ),
        "1月",
        "January must render '1月', not '01月' (%-m de-padding)"
    );
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap(),
            TickMarkType::Month
        ),
        "6月"
    );
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 10, 15, 10, 0, 0).unwrap(),
            TickMarkType::Month
        ),
        "10月",
        "two-digit month must keep both digits"
    );
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 12, 31, 10, 0, 0).unwrap(),
            TickMarkType::Month
        ),
        "12月",
        "December/December 31 boundary"
    );
}

/// Day-of-month labels must be Chinese and de-padded for single-digit days.
/// Guard: a regression to English ("Jun 15" / "Jun 01" / "Dec 31") fails here.
#[test]
fn adversarial_219_day_of_month_labels_chinese_depadded() {
    egui_charts::set_locale("zh");
    let f = DefaultTimeFormatter::default();
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap(),
            TickMarkType::DayOfMonth
        ),
        "6月15日"
    );
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 6, 1, 10, 0, 0).unwrap(),
            TickMarkType::DayOfMonth
        ),
        "6月1日",
        "day 1 must not zero-pad to '6月01日'"
    );
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 6, 10, 10, 0, 0).unwrap(),
            TickMarkType::DayOfMonth
        ),
        "6月10日"
    );
    assert_eq!(
        f.format(
            Utc.with_ymd_and_hms(2024, 12, 31, 10, 0, 0).unwrap(),
            TickMarkType::DayOfMonth
        ),
        "12月31日",
        "year-end boundary"
    );
}

/// Zero-padding regression guard: the plan locks `%-m`/`%-d` de-padding;
/// a zero-padded "06月" output must never pass. (Assertions above already
/// fail on English output; this guard catches a half-done localization that
/// switched to Chinese but kept zero-padding.)
#[test]
fn adversarial_219_zero_padded_forms_are_forbidden() {
    egui_charts::set_locale("zh");
    let f = DefaultTimeFormatter::default();
    let jun = Utc.with_ymd_and_hms(2024, 6, 15, 10, 0, 0).unwrap();
    assert_ne!(
        f.format(jun, TickMarkType::Month),
        "06月",
        "zero-padded month is a plan violation"
    );
    assert_ne!(
        f.format(
            Utc.with_ymd_and_hms(2024, 6, 1, 10, 0, 0).unwrap(),
            TickMarkType::DayOfMonth
        ),
        "6月01日",
        "zero-padded day is a plan violation"
    );
}

/// Timezone conversion must keep the Chinese date after crossing a day
/// boundary. 2024-06-15 20:00 UTC == 2024-06-16 05:00 JST.
/// Guard: a regression to English ("Jun 16") after timezone conversion fails here.
#[test]
fn adversarial_219_timezone_cross_day_keeps_chinese() {
    egui_charts::set_locale("zh");
    let f = TimeFormatterBuilder::new()
        .with_24_hour(true)
        .with_seconds(false)
        .with_timezone(TimezoneMode::jse())
        .build();
    let t = Utc.with_ymd_and_hms(2024, 6, 15, 20, 0, 0).unwrap();
    assert_eq!(
        f.format(t, TickMarkType::DayOfMonth),
        "6月16日",
        "Tokyo conversion crosses midnight to June 16"
    );
    // Same-instant UTC formatter must stay on June 15 — proving the
    // conversion, not the input, produced the 16th.
    let utc = DefaultTimeFormatter::default();
    assert_eq!(utc.format(t, TickMarkType::DayOfMonth), "6月15日");
}
