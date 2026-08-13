//! General recursive evaluator for the screener [`Filter`] AST (Batch 3,
//! issue #246).
//!
//! [`evaluate`] walks the whole [`Filter`] tree — metadata constraints
//! (`MetaCond`) against [`StockBasic`], series conditions (`SeriesCond`)
//! against the daily bar slice, and boolean combinators (`And`/`Or`/`Not`)
//! — returning `true` when the symbol passes the filter.
//!
//! Semantics contract (mirrors the legacy hard-coded engine in `lib.rs`):
//!
//! - `Meta(Industry/Exchange/Board)` use OR-within-field semantics; empty
//!   vectors mean "no constraint".
//! - `Meta(ListYears)` requires a listing date and sufficient age.
//! - `Meta(Delisted(false))` matches non-delisted symbols; `Delisted(true)`
//!   matches delisted-only (full AST support — the compile layer never emits
//!   `true`, but the evaluator handles it).
//! - `Meta(MarketCap)` uses `total_share × latest.close / 1e8` (亿元). A
//!   missing `total_share` with an active bound excludes the symbol; with
//!   both bounds `None` it is treated as `0.0` (sorts to the bottom) and
//!   matches — the GUI default card is `MarketCap{None,None}`.
//! - `Series(UpDays)` / `Series(VolumeSurge)` delegate to the pure functions
//!   in [`crate::screener_series`]; window-insufficient or non-finite input
//!   yields "no match" (`false`), never an error.
//! - `Series(Cmp)` evaluates both sides via `factor_at`; any
//!   window-insufficient or non-finite factor yields `false`.
//! - `Series(Count)` evaluates the comparison per bar over the last `window`
//!   bars ("as of" each bar index) and requires `at_least` matches; bars
//!   whose factor window is insufficient do not count.
//! - `NDayHigh(n)` is the max adjusted close of the **previous** `n` bars
//!   (excluding the current bar) — the engine's breakout reference
//!   (`matches_breakout`, lib.rs:527-535).
//!
//! The evaluator never panics and never returns `NaN`-contaminated results.

use chrono::{Duration, NaiveDate};
use compass_core::data::symbol::exchange_of_symbol;
use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::{CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor};

use crate::screener_series;

/// Recursively evaluate `filter` for one symbol.
///
/// `series` is the symbol's daily bars, oldest first (as fetched by
/// `run_screener`); `basic` its stock metadata; `now` the evaluation date.
/// Returns `true` when the symbol passes every constraint in the filter.
pub fn evaluate(
    filter: &Filter,
    basic: &StockBasic,
    series: &[&CrossSectionBar],
    now: NaiveDate,
) -> bool {
    match filter {
        Filter::Meta(meta) => evaluate_meta(meta, basic, series, now),
        Filter::Series(cond) => evaluate_series(cond, series),
        Filter::And(children) => children.iter().all(|f| evaluate(f, basic, series, now)),
        Filter::Or(children) => children.iter().any(|f| evaluate(f, basic, series, now)),
        Filter::Not(child) => !evaluate(child, basic, series, now),
    }
}

/// Evaluate a metadata constraint against stock metadata (+ the latest bar
/// for market cap).
fn evaluate_meta(
    meta: &MetaCond,
    basic: &StockBasic,
    series: &[&CrossSectionBar],
    now: NaiveDate,
) -> bool {
    match meta {
        MetaCond::Industry(v) => {
            v.is_empty()
                || basic
                    .industry
                    .as_deref()
                    .is_some_and(|i| v.iter().any(|q| q == i))
        }
        MetaCond::Exchange(v) => {
            v.is_empty() || v.iter().any(|e| e == exchange_of_symbol(&basic.symbol))
        }
        MetaCond::Board(v) => {
            v.is_empty()
                || basic
                    .board
                    .as_deref()
                    .is_some_and(|b| v.iter().any(|q| q == b))
        }
        MetaCond::ListYears(n) => basic
            .list_date
            .is_some_and(|d| now - d >= Duration::days(*n as i64 * 365)),
        MetaCond::Delisted(only_delisted) => {
            if *only_delisted {
                basic.delist_date.is_some()
            } else {
                basic.delist_date.is_none()
            }
        }
        MetaCond::MarketCap { min, max } => {
            let Some(latest) = series.last() else {
                return false;
            };
            let market_cap = match basic.total_share {
                Some(share) => share * latest.close / 1e8,
                // Missing total_share: excluded when a bound is active
                // (mirrors lib.rs:435-444); otherwise treated as 0.0 which
                // passes both None bounds.
                None => {
                    if min.is_some() || max.is_some() {
                        return false;
                    }
                    0.0
                }
            };
            let min_ok = min.is_none_or(|m| market_cap >= m);
            let max_ok = max.is_none_or(|m| market_cap <= m);
            min_ok && max_ok
        }
    }
}

/// Evaluate a series condition against the daily bar slice.
fn evaluate_series(cond: &SeriesCond, series: &[&CrossSectionBar]) -> bool {
    match cond {
        SeriesCond::Cmp { factor, op, value } => {
            let Some(lhs) = factor_value(series, *factor) else {
                return false;
            };
            let Some(rhs) = reference_value(series, *value) else {
                return false;
            };
            compare(lhs, *op, rhs)
        }
        SeriesCond::UpDays { n, min_pct } => {
            screener_series::up_days(series, *n, *min_pct).unwrap_or(false)
        }
        SeriesCond::Count {
            factor,
            op,
            value,
            window,
            at_least,
        } => count_matches(series, *factor, *op, *value, *window, *at_least),
        SeriesCond::VolumeSurge { days, times } => {
            screener_series::volume_surge(series, *days, *times).unwrap_or(false)
        }
    }
}

/// Count how many of the last `window` bars satisfy the comparison, and
/// require at least `at_least` of them.
fn count_matches(
    series: &[&CrossSectionBar],
    factor: SeriesFactor,
    op: CmpOp,
    value: FactorRef,
    window: u32,
    at_least: u32,
) -> bool {
    let window = window as usize;
    if series.len() < window {
        return false;
    }
    let start = series.len() - window;
    let count = (start..series.len())
        .filter(|&i| day_matches(series, i, factor, op, value))
        .count();
    count >= at_least as usize
}

/// Does the comparison hold "as of" bar index `end` (inclusive)?
///
/// `end` is the latest bar of the lookback: each factor is computed on the
/// slice ending at `end`. A bar whose factor window is insufficient (e.g.
/// `Sma(60)` at `end < 59`) does not count toward the total.
fn day_matches(
    series: &[&CrossSectionBar],
    end: usize,
    factor: SeriesFactor,
    op: CmpOp,
    value: FactorRef,
) -> bool {
    let Some(lhs) = factor_at(series, end, factor) else {
        return false;
    };
    let Some(rhs) = reference_at(series, end, value) else {
        return false;
    };
    compare(lhs, op, rhs)
}

/// Value of a series factor on the full slice (window ending at the latest
/// bar).
fn factor_value(series: &[&CrossSectionBar], factor: SeriesFactor) -> Option<f64> {
    if series.is_empty() {
        return None;
    }
    factor_at(series, series.len() - 1, factor)
}

/// Value of a [`FactorRef`] on the full slice.
fn reference_value(series: &[&CrossSectionBar], value: FactorRef) -> Option<f64> {
    match value {
        FactorRef::Const(c) => Some(c),
        FactorRef::Factor(f) => factor_value(series, f),
    }
}

/// Value of a [`FactorRef`] "as of" bar index `end`.
fn reference_at(series: &[&CrossSectionBar], end: usize, value: FactorRef) -> Option<f64> {
    match value {
        FactorRef::Const(c) => Some(c),
        FactorRef::Factor(f) => factor_at(series, end, f),
    }
}

/// Compute a series factor on the slice ending at bar index `end` (inclusive).
///
/// Returns `None` when the window is insufficient or the result is not
/// finite. `NDayHigh(n)` is the max adjusted close of the `n` bars
/// **before** `end` (engine breakout semantics).
fn factor_at(series: &[&CrossSectionBar], end: usize, factor: SeriesFactor) -> Option<f64> {
    let finite = |v: f64| v.is_finite().then_some(v);
    match factor {
        SeriesFactor::Close => series.get(end).and_then(|b| finite(b.adjclose)),
        SeriesFactor::Sma(n) => {
            let n = n as usize;
            if n == 0 || end + 1 < n {
                return None;
            }
            let sum: f64 = series[end + 1 - n..=end].iter().map(|b| b.adjclose).sum();
            finite(sum / n as f64)
        }
        SeriesFactor::ChangePct(n) => {
            let n = n as usize;
            if end < n {
                return None;
            }
            let base = series[end - n].adjclose;
            if base == 0.0 {
                return None;
            }
            let latest = series[end].adjclose;
            finite((latest - base) / base * 100.0)
        }
        SeriesFactor::DayPct => {
            if end < 1 {
                return None;
            }
            let prev = series[end - 1].adjclose;
            if prev == 0.0 {
                return None;
            }
            let latest = series[end].adjclose;
            finite((latest - prev) / prev * 100.0)
        }
        SeriesFactor::AvgVolume(n) => {
            let n = n as usize;
            if n == 0 || end + 1 < n {
                return None;
            }
            let sum: f64 = series[end + 1 - n..=end].iter().map(|b| b.volume).sum();
            finite(sum / n as f64)
        }
        SeriesFactor::NDayHigh(n) => {
            let n = n as usize;
            if n == 0 || end < n {
                return None;
            }
            let max = series[end - n..end]
                .iter()
                .map(|b| b.adjclose)
                .fold(f64::NEG_INFINITY, f64::max);
            finite(max)
        }
    }
}

/// Compare two values with a comparison operator.
fn compare(lhs: f64, op: CmpOp, rhs: f64) -> bool {
    match op {
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Ge => lhs >= rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Le => lhs <= rhs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Weekday};

    /// Weekday-only daily bars ending 2026-07-31 (inclusive) with the given
    /// closes (adjclose == close) and a constant volume.
    fn bars(closes: &[f64], volume: f64) -> Vec<CrossSectionBar> {
        let mut day = NaiveDate::from_ymd_opt(2026, 7, 31).expect("valid date");
        let mut out = Vec::new();
        for close in closes.iter().rev() {
            while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                day -= Duration::days(1);
            }
            out.push(CrossSectionBar {
                symbol: "SZ000001".to_string(),
                trade_date: day,
                open: close - 1.0,
                high: close + 1.0,
                low: close - 0.5,
                adjclose: *close,
                close: *close,
                volume,
                amount: 0.0,
            });
            day -= Duration::days(1);
        }
        out.reverse();
        out
    }

    fn refs(owned: &[CrossSectionBar]) -> Vec<&CrossSectionBar> {
        owned.iter().collect()
    }

    fn basic() -> StockBasic {
        StockBasic {
            symbol: "SZ000001".to_string(),
            name: "平安银行".to_string(),
            area: None,
            industry: Some("银行".to_string()),
            market: Some("主板".to_string()),
            board: Some("主板".to_string()),
            full_name: None,
            total_share: Some(1.0e10),
            list_date: Some(NaiveDate::from_ymd_opt(1991, 4, 3).expect("date")),
            delist_date: None,
        }
    }

    fn now() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 31).expect("date")
    }

    fn industry(name: &str) -> Filter {
        Filter::Meta(MetaCond::Industry(vec![name.to_string()]))
    }

    // --- Meta semantics ----------------------------------------------------

    #[test]
    fn industry_matches_and_empty_is_no_constraint() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        assert!(evaluate(&industry("银行"), &b, &s, now()));
        assert!(!evaluate(&industry("白酒"), &b, &s, now()));
        assert!(evaluate(
            &Filter::Meta(MetaCond::Industry(vec![])),
            &b,
            &s,
            now()
        ));
    }

    #[test]
    fn exchange_uses_exchange_of_symbol() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Meta(MetaCond::Exchange(vec!["SZ".to_string()]));
        assert!(evaluate(&f, &b, &s, now()));
        let f = Filter::Meta(MetaCond::Exchange(vec!["SH".to_string()]));
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn list_years_requires_sufficient_age() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic(); // listed 1991 — plenty old
        let f = Filter::Meta(MetaCond::ListYears(3));
        assert!(evaluate(&f, &b, &s, now()));
        let f = Filter::Meta(MetaCond::ListYears(100));
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn list_years_missing_list_date_is_no_match() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let mut b = basic();
        b.list_date = None;
        let f = Filter::Meta(MetaCond::ListYears(3));
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn delisted_bool_semantics() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let active = basic();
        let mut delisted = basic();
        delisted.delist_date = Some(NaiveDate::from_ymd_opt(2026, 7, 14).expect("date"));
        let exclude = Filter::Meta(MetaCond::Delisted(false));
        let only_delisted = Filter::Meta(MetaCond::Delisted(true));
        assert!(evaluate(&exclude, &active, &s, now()));
        assert!(!evaluate(&exclude, &delisted, &s, now()));
        assert!(!evaluate(&only_delisted, &active, &s, now()));
        assert!(evaluate(&only_delisted, &delisted, &s, now()));
    }

    #[test]
    fn market_cap_bounds_and_missing_share() {
        let owned = bars(&[100.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic(); // 1e10 × 100 / 1e8 = 10000 亿
        let window = Filter::Meta(MetaCond::MarketCap {
            min: Some(5000.0),
            max: Some(15000.0),
        });
        assert!(evaluate(&window, &b, &s, now()));
        let too_high = Filter::Meta(MetaCond::MarketCap {
            min: Some(5000.0),
            max: Some(9000.0),
        });
        assert!(!evaluate(&too_high, &b, &s, now()));

        // Missing total_share + active bound → no match.
        let mut b2 = basic();
        b2.total_share = None;
        assert!(!evaluate(&window, &b2, &s, now()));
        // Missing total_share + both bounds None → match (GUI default card).
        let none_none = Filter::Meta(MetaCond::MarketCap {
            min: None,
            max: None,
        });
        assert!(evaluate(&none_none, &b2, &s, now()));
    }

    // --- Boolean combinators ----------------------------------------------

    #[test]
    fn and_requires_all() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::And(vec![industry("银行"), industry("白酒")]);
        assert!(!evaluate(&f, &b, &s, now()));
        let f = Filter::And(vec![industry("银行"), industry("银行")]);
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn or_requires_any() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Or(vec![industry("银行"), industry("白酒")]);
        assert!(evaluate(&f, &b, &s, now()));
        let f = Filter::Or(vec![industry("白酒"), industry("医药")]);
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn not_negates() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Not(Box::new(industry("银行")));
        assert!(!evaluate(&f, &b, &s, now()));
        let f = Filter::Not(Box::new(industry("白酒")));
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        // Not⁶(And([Or([银行, 白酒]), UpDays{n:1, min_pct:0.5}]))
        let owned = bars(&[100.0, 101.0, 102.03, 104.04], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let inner = Filter::And(vec![
            Filter::Or(vec![industry("银行"), industry("白酒")]),
            Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 0.5 }),
        ]);
        let filter = (0..6).fold(inner, |f, _| Filter::Not(Box::new(f)));
        assert!(evaluate(&filter, &b, &s, now()));
    }

    // --- Series Cmp ---------------------------------------------------------

    #[test]
    fn close_gt_sma20_matches_rising_series() {
        let owned = bars(&[100.0, 101.0, 102.0, 103.0, 104.0, 105.0], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(5)),
        });
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn cmp_window_insufficient_is_no_match() {
        let owned = bars(&[10.0; 3], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        });
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn nday_high_is_previous_n_bars_excluding_latest() {
        // Latest 12, previous 3 = [10, 11, 12] → max 12; 12 > 12 false.
        let owned = bars(&[10.0, 11.0, 12.0, 12.0], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(3)),
        });
        assert!(!evaluate(&f, &b, &s, now()));
        // Latest 13 > max(10,11,12)=12 → true.
        let owned = bars(&[10.0, 11.0, 12.0, 13.0], 1.0e6);
        let s = refs(&owned);
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn factor_ref_on_both_sides_compares_factors() {
        // Sma(5) > Sma(20) on a rising 30-bar series.
        let closes: Vec<f64> = (0..30).map(|i| 10.0 + i as f64 * 3.0 / 29.0).collect();
        let owned = bars(&closes, 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Sma(5),
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        });
        assert!(evaluate(&f, &b, &s, now()));
        // Falling series → Sma5 < Sma20 → no match.
        let closes: Vec<f64> = (0..30).map(|i| 13.0 - i as f64 * 3.0 / 29.0).collect();
        let owned = bars(&closes, 1.0e6);
        let s = refs(&owned);
        assert!(!evaluate(&f, &b, &s, now()));
    }

    // --- Count ---------------------------------------------------------------

    #[test]
    fn count_at_least_zero_matches_with_no_qualifying_day() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 5,
            at_least: 0,
        });
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn count_zero_window_with_zero_at_least_matches() {
        // Plan formula: len < window false (0), empty index range → count 0
        // ≥ at_least 0 → true.
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 0,
            at_least: 0,
        });
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn count_insufficient_total_window_is_no_match() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 10,
            at_least: 1,
        });
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn count_at_least_exceeding_window_never_matches() {
        let owned = bars(&[100.0, 101.0, 102.0, 103.0, 104.0], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 5,
            at_least: 6,
        });
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn count_sma_insufficient_days_inside_loop_are_not_counted() {
        // 10 bars, Sma(5) computable only from index 4: max count 6.
        let owned = bars(&[10.0; 10], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::Sma(5),
            op: CmpOp::Gt,
            value: FactorRef::Const(1.0),
            window: 10,
            at_least: 7,
        });
        assert!(!evaluate(&f, &b, &s, now()));
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::Sma(5),
            op: CmpOp::Gt,
            value: FactorRef::Const(1.0),
            window: 10,
            at_least: 6,
        });
        assert!(evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn count_factor_vs_factor_evaluated_per_day() {
        // Rising 10 bars ≈ +1%/day: ChangePct(2) ≈ 2.01% > DayPct ≈ 1.00%
        // for every computable day (i ≥ 2) → count 8 ≥ 8.
        let closes = [
            100.0, 101.0, 102.01, 103.03, 104.06, 105.10, 106.15, 107.21, 108.29, 109.37,
        ];
        let owned = bars(&closes, 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::ChangePct(2),
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::DayPct),
            window: 10,
            at_least: 8,
        });
        assert!(evaluate(&f, &b, &s, now()));
    }

    // --- Series delegations --------------------------------------------------

    #[test]
    fn up_days_delegates_to_series_fn() {
        let owned = bars(&[100.0, 101.0, 102.03, 104.04], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 0.5 });
        assert!(evaluate(&f, &b, &s, now()));
        let f = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.5 });
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn up_days_zero_base_and_nan_threshold_no_match() {
        let owned = bars(&[5.0, 0.0, 2.0], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::UpDays { n: 2, min_pct: 0.5 });
        assert!(!evaluate(&f, &b, &s, now()));
        let owned = bars(&[100.0, 101.0, 102.0], 1.0e6);
        let s = refs(&owned);
        let f = Filter::Series(SeriesCond::UpDays {
            n: 2,
            min_pct: f64::NAN,
        });
        assert!(!evaluate(&f, &b, &s, now()));
    }

    #[test]
    fn volume_surge_delegates_to_series_fn() {
        let mut owned = bars(&[10.0; 30], 1.0e6);
        for b in owned.iter_mut().skip(20) {
            b.volume = 2.0e6;
        }
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::VolumeSurge {
            days: 10,
            times: 1.5,
        });
        assert!(evaluate(&f, &b, &s, now()));
        let f = Filter::Series(SeriesCond::VolumeSurge {
            days: 10,
            times: 1.51,
        });
        assert!(!evaluate(&f, &b, &s, now()));
    }

    // --- Empty / degenerate series ------------------------------------------

    #[test]
    fn empty_series_is_no_match_not_panic() {
        let empty: Vec<&CrossSectionBar> = Vec::new();
        let b = basic();
        assert!(evaluate(&industry("银行"), &b, &empty, now()));
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Const(5.0),
        });
        assert!(!evaluate(&f, &b, &empty, now()));
    }

    #[test]
    fn single_bar_series_window_insufficient_is_no_match() {
        let owned = bars(&[10.0], 1.0e6);
        let s = refs(&owned);
        let b = basic();
        let f = Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 0.1 });
        assert!(!evaluate(&f, &b, &s, now()));
    }
}
