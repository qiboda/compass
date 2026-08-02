//! Pure technical indicators for the SEPA engine.
//!
//! All functions take a time-ordered cross-section slice
//! (`&[&CrossSectionBar]`, oldest first) and return `None` (or 0 for
//! [`rs_score`]) when the window is insufficient. They never panic and never
//! produce `NaN`: non-finite inputs yield `None` instead of propagating.

use compass_core::model::CrossSectionBar;

/// Simple moving average of the last `n` adjusted closes.
///
/// `None` when `n == 0`, when `series.len() < n`, or when any of the last `n`
/// adjusted closes is non-finite.
pub fn ma(series: &[&CrossSectionBar], n: usize) -> Option<f64> {
    if n == 0 || series.len() < n {
        return None;
    }
    let start = series.len() - n;
    if !all_finite(&series[start..], |b| b.adjclose) {
        return None;
    }
    Some(series[start..].iter().map(|b| b.adjclose).sum::<f64>() / n as f64)
}

/// 20-bar average true range: TR = max(high-low, |high-prev_close|,
/// |low-prev_close|), averaged over the last 20 bars.
///
/// `None` when fewer than 21 bars are available (the first TR needs the
/// preceding close) or when the relevant high/low/close are non-finite.
pub fn atr20(series: &[&CrossSectionBar]) -> Option<f64> {
    const N: usize = 20;
    if series.len() < N + 1 {
        return None;
    }
    let start = series.len() - N - 1;
    let win = &series[start..];
    if !all_finite(win, |b| b.high) || !all_finite(win, |b| b.low) || !all_finite(win, |b| b.close)
    {
        return None;
    }
    let sum: f64 = win[1..]
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let prev_close = win[k].close;
            (b.high - b.low)
                .max((b.high - prev_close).abs())
                .max((b.low - prev_close).abs())
        })
        .sum();
    Some(sum / N as f64)
}

/// Return percent over the last `days` bars:
/// `(latest - base) / base * 100.0` where `base` is the adjusted close
/// `days + 1` bars back.
///
/// `None` when fewer than `days + 1` bars are available, when `base == 0.0`,
/// or when base/latest are non-finite.
pub fn momentum_return(series: &[&CrossSectionBar], days: usize) -> Option<f64> {
    if series.len() < days + 1 {
        return None;
    }
    let base = series[series.len() - days - 1].adjclose;
    let latest = series[series.len() - 1].adjclose;
    if !base.is_finite() || !latest.is_finite() || base == 0.0 {
        return None;
    }
    Some((latest - base) / base * 100.0)
}

/// Volume ratio (量比): average volume of the last `days` bars divided by
/// the average volume of the `days` bars before them.
///
/// `None` when fewer than `2 × days` bars are available, when the baseline
/// average is zero, or when the windowed volumes are non-finite.
pub fn volume_ratio(series: &[&CrossSectionBar], days: usize) -> Option<f64> {
    if days == 0 || series.len() < 2 * days {
        return None;
    }
    let start = series.len() - 2 * days;
    let split = series.len() - days;
    if !all_finite(&series[start..], |b| b.volume) {
        return None;
    }
    let recent: f64 = series[split..].iter().map(|b| b.volume).sum::<f64>() / days as f64;
    let prior: f64 = series[start..split].iter().map(|b| b.volume).sum::<f64>() / days as f64;
    if prior == 0.0 {
        return None;
    }
    Some(recent / prior)
}

/// Relative-strength percentile: own weighted momentum (60-day × 0.7 +
/// 20-day × 0.3) ranked 0-1 within `peers_momentum` (sector-component
/// momentums passed by the caller).
///
/// `0.0` when `peers_momentum` is empty (the caller falls back to a
/// whole-market ranking for sectors with fewer than 5 members); a percentile
/// is still computed from the passed peers when their count is between 1 and
/// 4 — the caller owns that fallback decision. Below 61 bars the own momentum
/// degrades to 20-day only; below 21 bars it is `0.0`. Never panics and never
/// yields `NaN`.
pub fn rs_score(series: &[&CrossSectionBar], peers_momentum: &[f64]) -> f64 {
    if peers_momentum.is_empty() {
        return 0.0;
    }
    let own = if series.len() >= 61 {
        let m60 = momentum_return(series, 60).unwrap_or(0.0);
        let m20 = momentum_return(series, 20).unwrap_or(0.0);
        m60 * 0.7 + m20 * 0.3
    } else {
        momentum_return(series, 20).unwrap_or(0.0)
    };
    let below = peers_momentum.iter().filter(|p| **p < own).count();
    below as f64 / peers_momentum.len() as f64
}

/// VCP (Volatility Contraction Pattern) shape score in 0..1.
///
/// Identifies up to 3 most recent "peak → pullback" cycles within the last
/// 120 bars, scores how closely the pullback depths converge toward the
/// classic 20% → 10% → 5% sequence, then adds a bonus for ATR20 contraction
/// (current ATR20 below the ATR20 of 60 bars ago) and a bonus for shrinking
/// volume over the latest consolidation. Non-converging (noisy) sequences
/// score low. Windows below 120 bars are prorated by `len/120`.
///
/// `None` when fewer than 30 bars are available or the windowed prices are
/// non-finite.
pub fn vcp_score(series: &[&CrossSectionBar]) -> Option<f64> {
    const MIN_BARS: usize = 30;
    const LOOKBACK: usize = 120;
    const IDEAL_DEPTHS: [f64; 3] = [0.20, 0.10, 0.05];
    if series.len() < MIN_BARS {
        return None;
    }
    let window = series.len().min(LOOKBACK);
    let bars = &series[series.len() - window..];
    if !all_finite(bars, |b| b.high) || !all_finite(bars, |b| b.low) {
        return None;
    }
    let depths = pullback_depths(bars);
    let k = depths.len().min(3);
    let recent = &depths[depths.len() - k..];
    let mut score = 0.0;
    let mut converged = true;
    let mut previous = f64::INFINITY;
    for (i, d) in recent.iter().enumerate() {
        if *d >= previous {
            converged = false;
        }
        let closeness = (1.0 - ((d - IDEAL_DEPTHS[i]).abs() / IDEAL_DEPTHS[i]).min(1.0)).max(0.0);
        score += closeness;
        previous = *d;
    }
    if !converged {
        score *= 0.2;
    }
    score /= 3.0;
    // ATR20 contraction bonus: current ATR20 meaningfully below (≥10%) the
    // ATR20 of 60 bars ago. The relative threshold keeps float noise (e.g.
    // identical stationary windows differing in the last ULP) from firing.
    if series.len() >= 81
        && let (Some(now), Some(past)) = (
            atr20(series),
            atr20(&series[series.len() - 81..series.len() - 60]),
        )
        && now < past * 0.9
    {
        score += 0.1;
    }
    // Consolidation volume shrink bonus: last 10 bars quieter than the 10 before.
    if series.len() >= 20 {
        let recent_vol: f64 = series[series.len() - 10..]
            .iter()
            .map(|b| b.volume)
            .sum::<f64>();
        let prior_vol: f64 = series[series.len() - 20..series.len() - 10]
            .iter()
            .map(|b| b.volume)
            .sum::<f64>();
        if recent_vol < prior_vol {
            score += 0.05;
        }
    }
    score *= window as f64 / LOOKBACK as f64;
    Some(score.min(1.0))
}

/// Percent drawdown from the highest adjusted close of the last `days` bars,
/// as a positive percentage: `(max - latest) / max * 100.0`.
///
/// `None` when `days == 0`, when `series.len() < days`, when `max == 0.0`, or
/// when the windowed adjusted closes are non-finite.
pub fn drawdown_from_high(series: &[&CrossSectionBar], days: usize) -> Option<f64> {
    if days == 0 || series.len() < days {
        return None;
    }
    let start = series.len() - days;
    if !all_finite(&series[start..], |b| b.adjclose) {
        return None;
    }
    let max = series[start..]
        .iter()
        .map(|b| b.adjclose)
        .fold(f64::NEG_INFINITY, f64::max);
    let latest = series[series.len() - 1].adjclose;
    if max == 0.0 {
        return None;
    }
    Some((max - latest) / max * 100.0)
}

/// True when every bar in `slice` has a finite value for the field selected
/// by `pick`.
fn all_finite(slice: &[&CrossSectionBar], pick: fn(&CrossSectionBar) -> f64) -> bool {
    slice.iter().all(|b| pick(b).is_finite())
}

/// Depths (0..1) of the "peak → pullback" cycles in `bars`, oldest first.
///
/// A cycle starts at a local peak (higher high than both neighbors); its
/// pullback is the deepest trough until the next strictly higher peak. A
/// later, strictly higher peak is required to keep the uptrend structure a
/// VCP presupposes — stationary oscillations therefore yield a single cycle.
fn pullback_depths(bars: &[&CrossSectionBar]) -> Vec<f64> {
    let mut depths = Vec::new();
    let mut i = 0;
    while i < bars.len() {
        while i < bars.len() && !is_local_peak(bars, i) {
            i += 1;
        }
        if i >= bars.len() || i == bars.len() - 1 {
            break;
        }
        let peak = bars[i].high;
        let mut trough = f64::MAX;
        let mut j = i + 1;
        let mut next_peak: Option<usize> = None;
        while j < bars.len() {
            trough = trough.min(bars[j].low);
            if is_local_peak(bars, j) && bars[j].high > peak {
                next_peak = Some(j);
                break;
            }
            j += 1;
        }
        if trough < peak {
            depths.push((peak - trough) / peak);
        }
        match next_peak {
            Some(j) => i = j,
            None => break,
        }
    }
    depths
}

/// True when bar `i` is a local high: `i > 0`, `i + 1 < len`, its high is at
/// least the previous high and strictly above the next high.
fn is_local_peak(bars: &[&CrossSectionBar], i: usize) -> bool {
    i > 0
        && i + 1 < bars.len()
        && bars[i].high >= bars[i - 1].high
        && bars[i].high > bars[i + 1].high
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Duration, NaiveDate, Weekday};

    /// One daily bar's values for in-memory fixtures; `adjclose == close`.
    #[derive(Clone)]
    struct TestBar {
        date: String,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    }

    /// Weekday-only daily bars ending at `end` (inclusive), closes from
    /// `closes`; high/low spread `(up, down)` around each close.
    fn daily_series(end: &str, closes: &[f64], up: f64, down: f64, volume: f64) -> Vec<TestBar> {
        let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
        let mut out = Vec::new();
        for close in closes.iter().rev() {
            while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                day -= Duration::days(1);
            }
            out.push(TestBar {
                date: day.format("%Y-%m-%d").to_string(),
                high: close + up,
                low: close - down,
                close: *close,
                volume,
            });
            day -= Duration::days(1);
        }
        out.reverse();
        out
    }

    /// Linear 10→20 rise over 40 bars (mirrors `tests/screener.rs`).
    fn rising_series(volume: f64) -> Vec<TestBar> {
        let closes: Vec<f64> = (0..40).map(|i| 10.0 + i as f64 * 10.0 / 39.0).collect();
        daily_series("2026-07-31", &closes, 1.0, 0.5, volume)
    }

    /// Linear 20→10 fall over 40 bars.
    fn falling_series(volume: f64) -> Vec<TestBar> {
        let closes: Vec<f64> = (0..40).map(|i| 20.0 - i as f64 * 10.0 / 39.0).collect();
        daily_series("2026-07-31", &closes, 1.0, 0.5, volume)
    }

    /// 40 flat bars at `price`.
    fn flat_series(price: f64, volume: f64) -> Vec<TestBar> {
        daily_series("2026-07-31", &vec![price; 40], 1.0, 0.5, volume)
    }

    /// Convert fixture bars to owned [`CrossSectionBar`]s (open = close - 1.0,
    /// amount = 0.0; fields unused by these indicators). Call sites build the
    /// `&CrossSectionBar` slice from the owned vec.
    fn to_cross_section(bars: &[TestBar]) -> Vec<CrossSectionBar> {
        bars.iter()
            .map(|b| CrossSectionBar {
                symbol: "000001".to_string(),
                trade_date: NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").expect("parse date"),
                open: b.close - 1.0,
                high: b.high,
                low: b.low,
                adjclose: b.close,
                close: b.close,
                volume: b.volume,
                amount: 0.0,
            })
            .collect()
    }

    /// Typical VCP: three converging pullbacks ≈ 20% → 10% → 5% over 110
    /// bars, with ATR contraction (wide early ranges, narrow late ranges) and
    /// shrinking volume on the final consolidation.
    ///
    /// Iterates closes newest-first (like [`daily_series`]) then reverses so
    /// the returned series runs oldest (phase A) to newest (phase F).
    fn vcp_converging_series() -> Vec<TestBar> {
        let mut closes: Vec<f64> = Vec::new();
        // Phase A: rise 20→100 (35 bars), B: pullback →80 (20), C: rise →105 (15),
        // D: pullback →94.5 (15), E: rise →108 (10), F: pullback →102.6 (15).
        closes.extend((0..35).map(|i| 20.0 + i as f64 * 80.0 / 34.0));
        closes.extend((0..20).map(|i| 100.0 - i as f64 * 20.0 / 19.0));
        closes.extend((0..15).map(|i| 80.0 + i as f64 * 25.0 / 14.0));
        closes.extend((0..15).map(|i| 105.0 - i as f64 * 10.5 / 14.0));
        closes.extend((0..10).map(|i| 94.5 + i as f64 * 13.5 / 9.0));
        closes.extend((0..15).map(|i| 108.0 - i as f64 * 5.4 / 14.0));
        let mut bars = Vec::new();
        let mut day = NaiveDate::parse_from_str("2026-07-31", "%Y-%m-%d").expect("parse end");
        // Phases E+F are the last 25 closes (indices 85..109 after reversal).
        for (days_elapsed, close) in closes.iter().rev().enumerate() {
            let (up, down, volume) = if days_elapsed < 25 {
                (0.4, 0.2, 1.0e6)
            } else {
                (2.0, 1.0, 5.0e6)
            };
            while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                day -= Duration::days(1);
            }
            bars.push(TestBar {
                date: day.format("%Y-%m-%d").to_string(),
                high: close + up,
                low: close - down,
                close: *close,
                volume,
            });
            day -= Duration::days(1);
        }
        bars.reverse();
        bars
    }

    /// Noisy stationary oscillation: sine around 100 with amplitude 5 and a
    /// constant range/volume — repeated equal-depth pullbacks, no convergence.
    fn vcp_noise_series() -> Vec<TestBar> {
        let closes: Vec<f64> = (0..110)
            .map(|i| 100.0 + 5.0 * (2.0 * std::f64::consts::PI * i as f64 / 12.0).sin())
            .collect();
        daily_series("2026-07-31", &closes, 0.5, 0.5, 5.0e6)
    }

    #[test]
    fn ma_of_rising_series_is_positive_mean() {
        let bars = rising_series(1.0e6);
        let s_owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        let v = ma(&s, 5).expect("enough bars");
        // last 5 closes of the 10→20 linear series: mean = 10 + 37×10/39.
        let expected = 10.0 + 37.0 * 10.0 / 39.0;
        assert!((v - expected).abs() < 1e-9);
        assert!(v > 0.0);
    }

    #[test]
    fn momentum_rising_positive_falling_negative_flat_zero() {
        let rising_owned = to_cross_section(&rising_series(1.0e6));
        let rising: Vec<&CrossSectionBar> = rising_owned.iter().collect();
        let v = momentum_return(&rising, 20).expect("enough bars");
        assert!(v > 30.0 && v < 40.0, "rising momentum {v}");

        let falling_owned = to_cross_section(&falling_series(1.0e6));
        let falling: Vec<&CrossSectionBar> = falling_owned.iter().collect();
        let v = momentum_return(&falling, 20).expect("enough bars");
        assert!(v < -30.0 && v > -40.0, "falling momentum {v}");

        let flat_owned = to_cross_section(&flat_series(12.5, 1.0e6));
        let flat: Vec<&CrossSectionBar> = flat_owned.iter().collect();
        let v = momentum_return(&flat, 20).expect("enough bars");
        assert_eq!(v, 0.0);
    }

    #[test]
    fn volume_ratio_doubles_when_recent_volume_doubles() {
        let mut bars = rising_series(1.0e6);
        for b in bars.iter_mut().skip(20) {
            b.volume = 2.0e6;
        }
        let s_owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        let v = volume_ratio(&s, 20).expect("enough bars");
        assert!((v - 2.0).abs() < 1e-9);
    }

    #[test]
    fn atr20_of_constant_range_series_matches_range() {
        // high = close + 1.0, low = close - 0.5 → TR = 1.5 on every bar.
        let s_owned = to_cross_section(&rising_series(1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        let v = atr20(&s).expect("enough bars");
        assert!((v - 1.5).abs() < 1e-9);
    }

    #[test]
    fn drawdown_from_high_reports_peak_distance() {
        let mut closes: Vec<f64> = (0..30).map(|i| 10.0 + i as f64 * 15.0 / 29.0).collect();
        closes.extend((0..10).map(|i| 25.0 - i as f64 * 5.0 / 9.0));
        let bars = daily_series("2026-07-31", &closes, 1.0, 0.5, 1.0e6);
        let s_owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        let v = drawdown_from_high(&s, 30).expect("enough bars");
        // window max = 25 (bar 29), latest = 25 - 9×5/9 = 20 → (25-20)/25×100.
        assert!((v - 20.0).abs() < 1e-9, "expected 20, got {v}");
    }

    #[test]
    fn rs_score_ranks_own_momentum_within_peers() {
        // 40-bar series → 20-day-only momentum ≈ 34.5% (below 61 bars).
        let s_owned = to_cross_section(&rising_series(1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(rs_score(&s, &[10.0, 20.0, 30.0]), 1.0);
        assert_eq!(rs_score(&s, &[35.0, 40.0]), 0.0);
        assert_eq!(rs_score(&s, &[]), 0.0);
    }

    #[test]
    fn rs_score_uses_dual_window_with_enough_bars() {
        let s_owned = to_cross_section(&vcp_converging_series());
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        // 60d momentum ≈ 20.33, 20d ≈ 2.09 → own = 20.33×0.7 + 2.09×0.3 ≈ 14.86.
        let v = rs_score(&s, &[0.0, 5.0, 10.0, 15.0, 20.0]);
        assert!((v - 0.6).abs() < 1e-6, "expected 0.6, got {v}");
    }

    #[test]
    fn vcp_converging_series_scores_high() {
        let s_owned = to_cross_section(&vcp_converging_series());
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        let v = vcp_score(&s).expect("enough bars");
        assert!(v >= 0.7, "converging VCP should score high, got {v}");
    }

    #[test]
    fn vcp_noise_series_scores_low() {
        let s_owned = to_cross_section(&vcp_noise_series());
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        let v = vcp_score(&s).expect("enough bars");
        assert!(v < 0.3, "noise should score low, got {v}");
    }

    #[test]
    fn vcp_discriminates_converging_from_noise() {
        let s_con_owned = to_cross_section(&vcp_converging_series());
        let s_con: Vec<&CrossSectionBar> = s_con_owned.iter().collect();
        let s_noise_owned = to_cross_section(&vcp_noise_series());
        let s_noise: Vec<&CrossSectionBar> = s_noise_owned.iter().collect();
        let con = vcp_score(&s_con).expect("enough bars");
        let noise = vcp_score(&s_noise).expect("enough bars");
        assert!(
            con - noise > 0.4,
            "gap too small: converging {con}, noise {noise}"
        );
    }

    #[test]
    fn ma_empty_and_short_series_return_none() {
        let empty: Vec<&CrossSectionBar> = Vec::new();
        assert_eq!(ma(&empty, 5), None);
        let s_owned = to_cross_section(&flat_series(10.0, 1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(ma(&s[..3], 5), None);
        assert_eq!(ma(&s, 0), None);
    }

    #[test]
    fn atr20_insufficient_bars_returns_none() {
        let s_owned = to_cross_section(&flat_series(10.0, 1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(atr20(&s[..20]), None, "exactly 20 bars lack the prev close");
        assert!(atr20(&s[..21]).is_some());
    }

    #[test]
    fn momentum_insufficient_and_zero_base_return_none() {
        let s_owned = to_cross_section(&flat_series(10.0, 1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(momentum_return(&s[..20], 20), None);
        let zero_owned = to_cross_section(&flat_series(0.0, 1.0e6));
        let zero: Vec<&CrossSectionBar> = zero_owned.iter().collect();
        assert_eq!(momentum_return(&zero, 5), None, "base == 0.0 must be None");
    }

    #[test]
    fn volume_ratio_insufficient_and_zero_baseline_return_none() {
        let s_owned = to_cross_section(&flat_series(10.0, 1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(volume_ratio(&s[..39], 20), None);
        let zero_vol_owned = to_cross_section(&flat_series(10.0, 0.0));
        let zero_vol: Vec<&CrossSectionBar> = zero_vol_owned.iter().collect();
        assert_eq!(
            volume_ratio(&zero_vol, 5),
            None,
            "zero baseline must be None"
        );
    }

    #[test]
    fn vcp_too_short_returns_none() {
        let s_owned = to_cross_section(&flat_series(10.0, 1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(vcp_score(&s[..29]), None);
    }

    #[test]
    fn drawdown_insufficient_and_zero_max_return_none() {
        let s_owned = to_cross_section(&flat_series(10.0, 1.0e6));
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(drawdown_from_high(&s[..20], 30), None);
        let zero_owned = to_cross_section(&flat_series(0.0, 1.0e6));
        let zero: Vec<&CrossSectionBar> = zero_owned.iter().collect();
        assert_eq!(
            drawdown_from_high(&zero, 5),
            None,
            "max == 0.0 must be None"
        );
        assert_eq!(drawdown_from_high(&s, 0), None);
    }

    #[test]
    fn nan_inputs_do_not_propagate() {
        // NaN inside the ma window → None.
        let mut bars = rising_series(1.0e6);
        bars[38].close = f64::NAN;
        let s_owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(ma(&s, 5), None, "NaN in window must yield None");
        // NaN as the momentum latest close → None.
        let mut bars = rising_series(1.0e6);
        bars[39].close = f64::NAN;
        let s_owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert_eq!(
            momentum_return(&s, 5),
            None,
            "NaN base/latest must yield None"
        );
        // NaN outside the window is irrelevant.
        let mut bars = rising_series(1.0e6);
        bars[0].close = f64::NAN;
        let s_owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = s_owned.iter().collect();
        assert!(ma(&s, 5).is_some());
    }
}
