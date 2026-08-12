//! Pure series functions for the screener AST (Batch 1 of epic #243).
//!
//! All functions take a time-ordered cross-section slice
//! (`&[&CrossSectionBar]`, oldest first) and return `None` when the window is
//! insufficient. They never panic and never produce `NaN`: non-finite inputs
//! yield `None` instead of propagating.

use compass_core::model::CrossSectionBar;

/// True when each of the last `n` daily returns is strictly above `min_pct`
/// percent (adjusted close, `(cur - prev) / prev * 100.0`).
///
/// Requires `n + 1` bars (one base bar plus `n` returns); fewer bars yield
/// `None`. `n == 0` is vacuously true (`Some(true)`) — no bars are needed.
/// Returns `None` when `min_pct` is non-finite, when any adjusted close in the
/// window is non-finite, or when a return's base price is `0.0` (division by
/// zero).
pub fn up_days(series: &[&CrossSectionBar], n: u32, min_pct: f64) -> Option<bool> {
    if !min_pct.is_finite() {
        return None;
    }
    if n == 0 {
        return Some(true);
    }
    let needed = n as usize + 1;
    if series.len() < needed {
        return None;
    }
    let start = series.len() - needed;
    if !all_finite(&series[start..], |b| b.adjclose) {
        return None;
    }
    // Every return divides by the previous bar's adjusted close: a zero base
    // anywhere in the window (all but the latest bar) makes the streak
    // undefined — check before evaluating so an early disqualifying return
    // cannot mask a later division by zero.
    if series[start..series.len() - 1]
        .iter()
        .any(|b| b.adjclose == 0.0)
    {
        return None;
    }
    for i in start + 1..series.len() {
        let prev = series[i - 1].adjclose;
        let cur = series[i].adjclose;
        if (cur - prev) / prev * 100.0 <= min_pct {
            return Some(false);
        }
    }
    Some(true)
}

/// Number of bars in the last `window` days satisfying `pred`.
///
/// `None` when `window == 0` or when fewer than `window` bars are available;
/// otherwise `Some(count)` in `0..=window`.
pub fn count_in_window(
    series: &[&CrossSectionBar],
    window: u32,
    pred: impl Fn(&CrossSectionBar) -> bool,
) -> Option<usize> {
    if window == 0 || series.len() < window as usize {
        return None;
    }
    let start = series.len() - window as usize;
    Some(series[start..].iter().filter(|b| pred(b)).count())
}

/// True when the recent `days`-bar average volume is at least `times` × the
/// last `3 × days`-bar average volume (nested baseline: the baseline window
/// includes the recent window — the engine's `matches_volume` semantics).
///
/// `None` when `days == 0`, when fewer than `3 × days` bars are available,
/// when `times` or any volume in the window is non-finite, or when the
/// baseline average is `0.0` (division by zero).
pub fn volume_surge(series: &[&CrossSectionBar], days: u32, times: f64) -> Option<bool> {
    if days == 0 || !times.is_finite() {
        return None;
    }
    let needed = days as usize * 3;
    if series.len() < needed {
        return None;
    }
    let start = series.len() - needed;
    if !all_finite(&series[start..], |b| b.volume) {
        return None;
    }
    let recent: f64 = series[series.len() - days as usize..]
        .iter()
        .map(|b| b.volume)
        .sum::<f64>()
        / days as f64;
    let baseline: f64 = series[start..].iter().map(|b| b.volume).sum::<f64>() / (3 * days) as f64;
    if baseline == 0.0 {
        return None;
    }
    Some(recent >= times * baseline)
}

/// True when every bar in `slice` has a finite value for the field selected
/// by `pick`.
fn all_finite(slice: &[&CrossSectionBar], pick: fn(&CrossSectionBar) -> f64) -> bool {
    slice.iter().all(|b| pick(b).is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Duration, NaiveDate, Weekday};
    use compass_core::model::CrossSectionBar;

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

    /// Borrow a slice of `&CrossSectionBar` from an owned vec.
    fn refs(owned: &[CrossSectionBar]) -> Vec<&CrossSectionBar> {
        owned.iter().collect()
    }

    #[test]
    fn up_days_all_rising_days_exceed_min_pct() {
        // Closes 100 → 101 → 102.03 → 104.04: returns ≈ +1.0%, +1.02%, +1.97%.
        let owned = bars(&[100.0, 101.0, 102.03, 104.04], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, 0.5), Some(true), "last 2 returns > 0.5%");
        assert_eq!(up_days(&s, 3, 0.5), Some(true), "all 3 returns > 0.5%");
    }

    #[test]
    fn up_days_any_day_below_threshold_is_false() {
        // +1.0%, -1.98%, +1.01% — a down day disqualifies the streak.
        let owned = bars(&[100.0, 101.0, 99.0, 100.0], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, 0.5), Some(false));
        // A rise smaller than min_pct also disqualifies: +1.02% vs min 1.5%.
        let owned = bars(&[100.0, 101.0, 102.03, 104.04], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, 1.5), Some(false), "1.02% < 1.5%");
    }

    #[test]
    fn up_days_zero_n_is_vacuous_true() {
        let empty: Vec<&CrossSectionBar> = Vec::new();
        assert_eq!(up_days(&empty, 0, 5.0), Some(true), "no days, no bars");
        let owned = bars(&[100.0, 101.0], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 0, 5.0), Some(true));
    }

    #[test]
    fn up_days_insufficient_window_returns_none() {
        let empty: Vec<&CrossSectionBar> = Vec::new();
        assert_eq!(up_days(&empty, 1, 0.5), None, "0 bars < n+1");
        let owned = bars(&[100.0, 101.0, 102.0], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 3, 0.5), None, "3 bars < 4 needed for n=3");
    }

    #[test]
    fn up_days_exactly_n_plus_one_bars_works() {
        // n=2 needs exactly 3 bars: returns +1.0% and +1.02%.
        let owned = bars(&[100.0, 101.0, 102.03], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, 0.5), Some(true));
        assert_eq!(up_days(&s, 2, 1.1), Some(false), "1.02% < 1.1%");
    }

    #[test]
    fn up_days_nan_close_in_window_returns_none() {
        // NaN inside the return window → None.
        let mut owned = bars(&[100.0, 101.0, 102.0, 103.0], 1.0e6);
        owned[2].adjclose = f64::NAN;
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, 0.5), None, "NaN in window must be None");
        // NaN outside the window is irrelevant.
        let mut owned = bars(&[100.0, 101.0, 102.0, 103.0], 1.0e6);
        owned[0].adjclose = f64::NAN;
        let s = refs(&owned);
        assert_eq!(
            up_days(&s, 2, 0.5),
            Some(true),
            "NaN outside window is fine"
        );
    }

    #[test]
    fn up_days_nan_or_infinite_min_pct_returns_none() {
        let owned = bars(&[100.0, 101.0, 102.0, 103.0], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, f64::NAN), None);
        assert_eq!(up_days(&s, 2, f64::INFINITY), None);
        assert_eq!(
            up_days(&s, 0, f64::NAN),
            None,
            "NaN threshold is never accepted"
        );
    }

    #[test]
    fn up_days_zero_base_price_returns_none() {
        // Second return divides by a zero base → None, no panic.
        let owned = bars(&[5.0, 0.0, 2.0], 1.0e6);
        let s = refs(&owned);
        assert_eq!(up_days(&s, 2, 0.5), None);
    }

    #[test]
    fn count_in_window_counts_matching_bars() {
        let owned = bars(&[10.0; 10], 1.0e6);
        let mut owned = owned;
        for (i, b) in owned.iter_mut().enumerate() {
            b.volume = (i + 1) as f64 * 100.0;
        }
        let s = refs(&owned);
        assert_eq!(
            count_in_window(&s, 5, |b| b.volume > 650.0),
            Some(4),
            "last 5 volumes 600..1000, >650 → 4"
        );
    }

    #[test]
    fn count_in_window_zero_window_returns_none() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        assert_eq!(count_in_window(&s, 0, |b| b.volume > 0.0), None);
    }

    #[test]
    fn count_in_window_insufficient_window_returns_none() {
        let empty: Vec<&CrossSectionBar> = Vec::new();
        assert_eq!(count_in_window(&empty, 1, |_| true), None);
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        assert_eq!(count_in_window(&s, 6, |_| true), None, "5 bars < window 6");
    }

    #[test]
    fn count_in_window_exactly_window_bars_works() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        assert_eq!(
            count_in_window(&s, 5, |b| b.volume > 0.0),
            Some(5),
            "boundary: window == series length"
        );
    }

    #[test]
    fn count_in_window_no_matches_is_zero() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        assert_eq!(count_in_window(&s, 3, |b| b.volume > 1.0e9), Some(0));
    }

    #[test]
    fn count_in_window_all_matches_is_window() {
        let owned = bars(&[10.0; 5], 1.0e6);
        let s = refs(&owned);
        assert_eq!(count_in_window(&s, 3, |_| true), Some(3));
    }

    #[test]
    fn volume_surge_recent_volume_surge_matches() {
        // 30 bars: first 20 at 1.0e6, last 10 at 2.0e6. days=10:
        // recent avg = 2.0e6, nested baseline avg (last 30) = 40e6/30.
        // 2.0e6 >= 1.5 × 40e6/30 = 2.0e6 → true.
        let mut owned = bars(&[10.0; 30], 1.0e6);
        for b in owned.iter_mut().skip(20) {
            b.volume = 2.0e6;
        }
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, 1.5), Some(true));
    }

    #[test]
    fn volume_surge_below_times_is_false() {
        let mut owned = bars(&[10.0; 30], 1.0e6);
        for b in owned.iter_mut().skip(20) {
            b.volume = 2.0e6;
        }
        let s = refs(&owned);
        assert_eq!(
            volume_surge(&s, 10, 1.51),
            Some(false),
            "2.0e6 < 1.51×40e6/30"
        );
    }

    #[test]
    fn volume_surge_zero_days_returns_none() {
        let owned = bars(&[10.0; 30], 1.0e6);
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 0, 1.5), None);
    }

    #[test]
    fn volume_surge_insufficient_window_returns_none() {
        let owned = bars(&[10.0; 29], 1.0e6);
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, 1.5), None, "29 bars < 3×10");
        let owned = bars(&[10.0; 30], 1.0e6);
        let s = refs(&owned);
        assert_eq!(
            volume_surge(&s, 10, 1.0),
            Some(true),
            "boundary: exactly 3×days bars"
        );
    }

    #[test]
    fn volume_surge_zero_baseline_returns_none() {
        let owned = bars(&[10.0; 30], 0.0);
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, 1.5), None, "baseline avg 0 → None");
    }

    #[test]
    fn volume_surge_nan_volume_or_times_returns_none() {
        // NaN inside the 3×days window → None.
        let mut owned = bars(&[10.0; 30], 1.0e6);
        owned[25].volume = f64::NAN;
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, 1.5), None);
        // NaN outside the window is irrelevant: 40 bars, window = last 30
        // (bars 10..40) — bar 0 is genuinely outside it.
        let mut owned = bars(&[10.0; 40], 1.0e6);
        owned[0].volume = f64::NAN;
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, 1.5), Some(false));
        // NaN times → None.
        let owned = bars(&[10.0; 30], 1.0e6);
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, f64::NAN), None);
        assert_eq!(volume_surge(&s, 10, f64::INFINITY), None);
    }

    #[test]
    fn volume_surge_uses_nested_baseline_including_recent() {
        // 30 bars: first 20 at 1.0e6, last 10 at 3.0e6. days=10, times=1.9:
        // nested baseline (last 30) avg = 50e6/30 ≈ 1.667e6 → 1.9×1.667e6
        // ≈ 3.167e6 > recent 3.0e6 → false. A disjoint baseline (1.0e6)
        // would yield 1.9e6 ≤ 3.0e6 → true; asserting false proves the
        // recent window is INCLUDED in the baseline (engine semantics).
        let mut owned = bars(&[10.0; 30], 1.0e6);
        for b in owned.iter_mut().skip(20) {
            b.volume = 3.0e6;
        }
        let s = refs(&owned);
        assert_eq!(volume_surge(&s, 10, 1.9), Some(false));
    }
}
