//! Pure technical indicators for the Compass charting app.
//!
//! All functions are pure: they take numeric slices and return computed
//! values without touching storage, providers, or the UI. Values are computed
//! in real time from the bars fetched by [`crate::data::provider::DataProvider`]
//! — indicators are never persisted.
//!
//! # Conventions (mirrors `compass-strategy::sepa::indicators`)
//!
//! - Input series are oldest-first (ascending time).
//! - A `None` entry means "no value for this position" (insufficient window
//!   or non-finite input); functions never panic and never emit NaN/Inf.
//! - Window sizes are inclusive of the current bar (MA5 uses the last 5 bars).

use chrono::{DateTime, Utc};
use egui_charts::model::Bar;

/// Simple moving average of `values` over the trailing `n`-bar window.
///
/// Returns one entry per input value (aligned with the input series):
/// `None` while fewer than `n` bars are available, or when any of the
/// trailing `n` values is non-finite.
///
/// # Panics
///
/// Never panics.
pub fn ma(values: &[f64], n: usize) -> Vec<Option<f64>> {
    if n == 0 {
        return values.iter().map(|_| None).collect();
    }
    values
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < n {
                return None;
            }
            let start = i + 1 - n;
            let window = &values[start..=i];
            if !window.iter().all(|v| v.is_finite()) {
                return None;
            }
            Some(window.iter().sum::<f64>() / n as f64)
        })
        .collect()
}

/// Bollinger Bands over the trailing `period`-bar window with `k` std-devs.
///
/// Returns one `(upper, middle, lower)` tuple per input value (aligned with
/// the input series): `None` while fewer than `period` bars are available, or
/// when any of the trailing `period` values is non-finite. The middle band is
/// the SMA over the same window; bands use the population standard deviation.
///
/// # Panics
///
/// Never panics.
pub fn bollinger(
    values: &[f64],
    period: usize,
    k: f64,
) -> Vec<(Option<f64>, Option<f64>, Option<f64>)> {
    if period == 0 {
        return values.iter().map(|_| (None, None, None)).collect();
    }
    values
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < period {
                return (None, None, None);
            }
            let start = i + 1 - period;
            let window = &values[start..=i];
            if !window.iter().all(|v| v.is_finite()) {
                return (None, None, None);
            }
            let mid = window.iter().sum::<f64>() / period as f64;
            let variance = window
                .iter()
                .map(|&v| {
                    let diff = v - mid;
                    diff * diff
                })
                .sum::<f64>()
                / period as f64;
            let std = variance.sqrt();
            let upper = mid + k * std;
            let lower = mid - k * std;
            (Some(upper), Some(mid), Some(lower))
        })
        .collect()
}

/// Raw OHLCV row (unadjusted prices) plus its date, in ascending time order.
pub struct RawBar {
    /// Trade date (ascending with the rest of the series).
    pub date: chrono::NaiveDate,
    /// Unadjusted open.
    pub open: f64,
    /// Unadjusted high.
    pub high: f64,
    /// Unadjusted low.
    pub low: f64,
    /// Unadjusted close.
    pub close: f64,
    /// Volume.
    pub volume: f64,
}

/// Price adjustment mode (复权方式) for chart bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustMode {
    /// Forward-adjusted (前复权): the latest bar equals the current market
    /// price; historical bars are scaled by `ratio_i / r_anchor` where
    /// `r_anchor` is the ratio of the last valid bar.
    Forward,
    /// Backward-adjusted (后复权): raw prices scaled by
    /// `ratio_i = adjclose_i / close_i` (the stored adjclose itself).
    Backward,
    /// Unadjusted (不复权): raw prices as-is.
    None,
}

impl std::str::FromStr for AdjustMode {
    type Err = std::convert::Infallible;

    /// Parses a canonical mode string: `"qfq"` (forward), `"hfq"`
    /// (backward), `"none"` (unadjusted); unknown values fall back to
    /// [`AdjustMode::Forward`] — the app/UI default.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "hfq" => Self::Backward,
            "none" => Self::None,
            _ => Self::Forward,
        })
    }
}

/// Adjust a raw OHLCV series into chart bars for a given [`AdjustMode`].
///
/// Per-bar adjustment factor (every OHLC price is scaled; volume is passed
/// through unchanged):
///
/// - [`AdjustMode::None`]: `factor = 1.0` for every bar.
/// - [`AdjustMode::Backward`]: `factor = ratio`, where
///   `ratio = adjclose_i / close_i` — the stored adjclose is itself the
///   backward-adjusted close, so prices scale to the adjusted series.
/// - [`AdjustMode::Forward`]: `factor = ratio / r_anchor`, where `r_anchor`
///   is the ratio of the **last valid bar** in the series; the anchor bar
///   keeps `factor = 1.0` (前复权锚点, latest bar = current price).
///
/// A bar's ratio is invalid (falls back to `factor = 1.0`) when `close <= 0`
/// or `adjclose` is `None`, non-finite, or `<= 0`. When `Forward` and no valid
/// ratio exists, every factor is 1.0. Output bars are in ascending time
/// order, matching the input.
///
/// # Panics
///
/// Never panics; `adjclose.len()` must equal `raw.len()` (debug-asserted).
pub fn adjust_ohlc(raw: &[RawBar], adjclose: &[Option<f64>], mode: AdjustMode) -> Vec<Bar> {
    debug_assert_eq!(
        adjclose.len(),
        raw.len(),
        "adjclose must provide exactly one factor per raw bar"
    );
    let ratios: Vec<Option<f64>> = raw
        .iter()
        .zip(adjclose)
        .map(|(r, &adj)| {
            let ratio = match adj {
                Some(a) if a.is_finite() && a > 0.0 => a / r.close,
                _ => 1.0,
            };
            (r.close > 0.0 && adj.is_some_and(|a| a.is_finite() && a > 0.0)).then_some(ratio)
        })
        .collect();
    // Forward anchor: ratio of the last valid bar (ascending series). When a
    // bar's ratio is valid, the anchor is Some by definition (this bar or a
    // later one); unwrap_or(1.0) only guards the impossible all-invalid case.
    let anchor = match mode {
        AdjustMode::Forward => ratios.iter().rev().find_map(|r| *r),
        _ => None,
    };
    raw.iter()
        .zip(&ratios)
        .map(|(r, ratio)| {
            let factor = match (mode, ratio) {
                (AdjustMode::None, _) => 1.0,
                (AdjustMode::Backward, Some(ratio)) => *ratio,
                (AdjustMode::Forward, Some(ratio)) => ratio / anchor.unwrap_or(1.0),
                (_, None) => 1.0,
            };
            let time = DateTime::<Utc>::from_naive_utc_and_offset(
                r.date
                    .and_hms_opt(0, 0, 0)
                    .expect("midnight is always valid"),
                Utc,
            );
            Bar::new(
                time,
                r.open * factor,
                r.high * factor,
                r.low * factor,
                r.close * factor,
                r.volume,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    // -----------------------------------------------------------------------
    // ma
    // -----------------------------------------------------------------------

    /// MA of a flat series equals the constant value everywhere (once the
    /// window is full).
    #[test]
    fn ma_flat_series_is_constant() {
        let values: Vec<f64> = vec![10.0; 10];
        let out = ma(&values, 5);
        assert_eq!(out.len(), 10);
        // First 4 bars have insufficient window.
        assert!(out[..4].iter().all(Option::is_none));
        // From bar 5 onward the SMA of 5 flat values is exactly 10.0.
        for v in out[4..].iter() {
            assert_eq!(*v, Some(10.0));
        }
    }

    /// MA of a rising linear series is the mean of the trailing window.
    #[test]
    fn ma_rising_series_matches_hand_computed_mean() {
        // 1..=6, window 3: mean(1,2,3)=2, mean(2,3,4)=3, mean(3,4,5)=4, ...
        let values: Vec<f64> = (1..=6).map(|x| x as f64).collect();
        let out = ma(&values, 3);
        assert!(out[..2].iter().all(Option::is_none));
        assert_eq!(out[2], Some(2.0));
        assert_eq!(out[3], Some(3.0));
        assert_eq!(out[4], Some(4.0));
        assert_eq!(out[5], Some(5.0));
    }

    /// Empty input returns an empty output (no panic).
    #[test]
    fn ma_empty_input_returns_empty() {
        assert!(ma(&[], 5).is_empty());
    }

    /// Zero window: nothing meaningful can be computed — all entries None.
    #[test]
    fn ma_zero_window_all_none() {
        let out = ma(&[1.0, 2.0, 3.0], 0);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(Option::is_none));
    }

    /// Insufficient window (len < n) yields all None, never panics.
    #[test]
    fn ma_insufficient_window_all_none() {
        let out = ma(&[1.0, 2.0, 3.0], 5);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(Option::is_none));
    }

    /// Non-finite values inside the window make that position None; positions
    /// whose window contains no NaN still compute.
    #[test]
    fn ma_nan_in_window_yields_none_for_that_position() {
        let values = vec![1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0];
        let out = ma(&values, 3);
        assert_eq!(out[0], None); // window [1,2,NaN]
        assert_eq!(out[1], None); // window [2,NaN,4]
        assert_eq!(out[2], None); // window [NaN,4,5]
        assert_eq!(out[5], Some(5.0)); // window [4,5,6]
    }

    /// NaN strictly outside the window (older than `n` bars back) does not
    /// poison later positions.
    #[test]
    fn ma_nan_outside_window_does_not_poison() {
        let values = vec![f64::NAN, 2.0, 3.0, 4.0, 5.0];
        let out = ma(&values, 3);
        // Window for index 3 is [2,3,4] — NaN at index 0 is outside it.
        assert_eq!(out[3], Some(3.0));
        assert_eq!(out[4], Some(4.0));
    }

    // -----------------------------------------------------------------------
    // bollinger
    // -----------------------------------------------------------------------

    /// Flat series: middle == value, bands == value ± k×0 = value (stddev 0).
    #[test]
    fn bollinger_flat_series_bands_equal_middle() {
        let values: Vec<f64> = vec![10.0; 8];
        let out = bollinger(&values, 3, 2.0);
        assert_eq!(out.len(), 8);
        assert!(
            out[..2]
                .iter()
                .all(|t| t.0.is_none() && t.1.is_none() && t.2.is_none())
        );
        for t in out[2..].iter() {
            let (u, m, l) = t;
            assert_eq!((u.unwrap(), m.unwrap(), l.unwrap()), (10.0, 10.0, 10.0));
        }
    }

    /// Window of [1,2,3]: mean 2, population stddev sqrt(2/3) ≈ 0.8165, so
    /// bands at k=1 are 2 ± 0.8165. k=0 collapses to the mean.
    #[test]
    fn bollinger_hand_computed_population_stddev() {
        let values = vec![1.0, 2.0, 3.0];
        let out = bollinger(&values, 3, 1.0);
        let (u, m, l) = out[2];
        assert!((m.unwrap() - 2.0).abs() < 1e-9);
        let std = (2.0f64 / 3.0f64).sqrt();
        assert!((u.unwrap() - (2.0 + std)).abs() < 1e-9);
        assert!((l.unwrap() - (2.0 - std)).abs() < 1e-9);

        let out0 = bollinger(&values, 3, 0.0);
        let (u0, m0, l0) = out0[2];
        assert!(
            (u0.unwrap() - 2.0).abs() < 1e-9
                && (m0.unwrap() - 2.0).abs() < 1e-9
                && (l0.unwrap() - 2.0).abs() < 1e-9
        );
    }

    /// Insufficient window → all None, never panics.
    #[test]
    fn bollinger_insufficient_window_all_none() {
        let out = bollinger(&[1.0, 2.0], 5, 2.0);
        assert_eq!(out.len(), 2);
        assert!(
            out.iter()
                .all(|t| t.0.is_none() && t.1.is_none() && t.2.is_none())
        );
    }

    /// Zero period: nothing meaningful can be computed — all entries None,
    /// never panics.
    #[test]
    fn bollinger_zero_period_all_none() {
        let out = bollinger(&[1.0, 2.0, 3.0], 0, 2.0);
        assert_eq!(out.len(), 3);
        assert!(
            out.iter()
                .all(|t| t.0.is_none() && t.1.is_none() && t.2.is_none())
        );
    }

    /// NaN anywhere in the window poisons that position.
    #[test]
    fn bollinger_nan_in_window_yields_none() {
        let values = vec![1.0, f64::NAN, 3.0, 4.0];
        let out = bollinger(&values, 3, 2.0);
        assert!(out[0].0.is_none());
        assert!(out[1].0.is_none());
        assert!(out[2].0.is_none());
        assert!(out[3].0.is_none());
    }

    // -----------------------------------------------------------------------
    // adjust_ohlc
    // -----------------------------------------------------------------------

    fn raw_bars(closes: &[f64]) -> Vec<RawBar> {
        let mut date = NaiveDate::from_ymd_opt(2026, 8, 1).expect("valid date");
        closes
            .iter()
            .map(|&c| {
                let b = RawBar {
                    date,
                    open: c - 1.0,
                    high: c + 2.0,
                    low: c - 2.0,
                    close: c,
                    volume: 1000.0,
                };
                date += chrono::Duration::days(1);
                b
            })
            .collect()
    }

    fn adj(values: &[f64]) -> Vec<Option<f64>> {
        values.iter().map(|&v| Some(v)).collect()
    }

    /// Latest bar has adjclose == close (前复权锚点) → factor 1.0, prices
    /// unchanged; historical bars scaled so that scaled_close == adjclose.
    #[test]
    fn adjust_ohlc_scales_ohlc_by_adjclose_over_close() {
        // Two bars: latest close 20 / adjclose 20 (anchor), older close 10 /
        // adjclose 8 (factor 0.8).
        let raw = raw_bars(&[10.0, 20.0]);
        let adj = adj(&[8.0, 20.0]);
        let bars = adjust_ohlc(&raw, &adj, AdjustMode::Forward);

        assert_eq!(bars.len(), 2);

        // Latest: factor = 20/20 = 1.0 → unchanged.
        let latest = &bars[1];
        assert_eq!(latest.open, 19.0);
        assert_eq!(latest.high, 22.0);
        assert_eq!(latest.low, 18.0);
        assert_eq!(latest.close, 20.0);
        assert_eq!(latest.volume, 1000.0);

        // Older: factor = 8/10 = 0.8 → open 9×0.8=7.2, high 12×0.8=9.6,
        // low 8×0.8=6.4, close 10×0.8=8.0.
        let older = &bars[0];
        assert!((older.open - 7.2).abs() < 1e-9);
        assert!((older.high - 9.6).abs() < 1e-9);
        assert!((older.low - 6.4).abs() < 1e-9);
        assert!((older.close - 8.0).abs() < 1e-9);
        assert_eq!(older.volume, 1000.0);
    }

    /// Output dates are ascending and match input dates.
    #[test]
    fn adjust_ohlc_preserves_dates() {
        let raw = raw_bars(&[10.0, 20.0]);
        let bars = adjust_ohlc(&raw, &adj(&[8.0, 20.0]), AdjustMode::Forward);
        assert_eq!(bars[0].time.date_naive(), raw[0].date);
        assert_eq!(bars[1].time.date_naive(), raw[1].date);
    }

    /// close == 0 must not produce NaN/Inf — factor falls back to 1.0.
    #[test]
    fn adjust_ohlc_zero_close_falls_back_to_factor_one() {
        let raw = raw_bars(&[0.0, 20.0]);
        let bars = adjust_ohlc(&raw, &adj(&[0.0, 20.0]), AdjustMode::Forward);
        assert!(bars[0].open.is_finite());
        assert!(bars[0].high.is_finite());
        assert!(bars[0].low.is_finite());
        assert!(bars[0].close.is_finite());
        // factor = 1.0 → close stays 0.0 (guarded, not scaled to NaN).
        assert_eq!(bars[0].close, 0.0);
    }

    /// Non-finite adjclose falls back to factor 1.0, never NaN.
    #[test]
    fn adjust_ohlc_nan_adjclose_falls_back_to_factor_one() {
        let raw = raw_bars(&[10.0, 20.0]);
        let bars = adjust_ohlc(&raw, &[Some(f64::NAN), Some(20.0)], AdjustMode::Forward);
        assert!(bars[0].close.is_finite());
        assert_eq!(bars[0].close, 10.0); // factor 1.0 → unchanged
    }

    /// Empty input → empty output, no panic.
    #[test]
    fn adjust_ohlc_empty_input_returns_empty() {
        assert!(adjust_ohlc(&[], &[], AdjustMode::Forward).is_empty());
    }

    /// Backward mode scales by raw ratio: latest ratio is not normalized to 1.
    #[test]
    fn adjust_ohlc_backward_uses_raw_ratio() {
        let raw = raw_bars(&[10.0, 20.0]);
        let bars = adjust_ohlc(&raw, &adj(&[8.0, 25.0]), AdjustMode::Backward);
        assert!((bars[0].close - 8.0).abs() < 1e-9); // 10 × 8/10
        assert!((bars[1].close - 25.0).abs() < 1e-9); // 20 × 25/20
    }

    /// None mode leaves every price untouched.
    #[test]
    fn adjust_ohlc_none_leaves_prices_untouched() {
        let raw = raw_bars(&[10.0, 20.0]);
        let bars = adjust_ohlc(&raw, &adj(&[8.0, 25.0]), AdjustMode::None);
        assert_eq!(bars[0].close, 10.0);
        assert_eq!(bars[1].close, 20.0);
        assert_eq!(bars[0].volume, 1000.0);
    }

    /// Forward anchor is the last **valid** ratio: a trailing invalid bar does
    /// not capture the anchor (it keeps factor 1.0) and earlier bars are
    /// normalized against the last valid one.
    #[test]
    fn adjust_ohlc_forward_anchor_is_last_valid_ratio() {
        // Ratios: 8/10 = 0.8, 18/20 = 0.9, NULL (invalid).
        let raw = raw_bars(&[10.0, 20.0, 14.0]);
        let adj: Vec<Option<f64>> = vec![Some(8.0), Some(18.0), None];
        let bars = adjust_ohlc(&raw, &adj, AdjustMode::Forward);
        let anchor = 0.9;
        assert!((bars[0].close - 10.0 * 0.8 / anchor).abs() < 1e-9);
        assert!((bars[1].close - 20.0 * 0.9 / anchor).abs() < 1e-9); // anchor → factor 1.0
        assert!((bars[2].close - 14.0).abs() < 1e-9); // invalid → factor 1.0
    }

    /// No valid ratio anywhere → every factor is 1.0 (no division by nothing).
    #[test]
    fn adjust_ohlc_forward_no_valid_ratio_keeps_prices() {
        let raw = raw_bars(&[10.0, 20.0]);
        let adj: Vec<Option<f64>> = vec![None, Some(f64::NAN)];
        let bars = adjust_ohlc(&raw, &adj, AdjustMode::Forward);
        assert_eq!(bars[0].close, 10.0);
        assert_eq!(bars[1].close, 20.0);
    }

    /// Unknown strings parse to Forward (the app default).
    #[test]
    fn adjust_mode_from_str_unknown_falls_back_to_forward() {
        assert_eq!("qfq".parse::<AdjustMode>().unwrap(), AdjustMode::Forward);
        assert_eq!("hfq".parse::<AdjustMode>().unwrap(), AdjustMode::Backward);
        assert_eq!("none".parse::<AdjustMode>().unwrap(), AdjustMode::None);
        assert_eq!("bogus".parse::<AdjustMode>().unwrap(), AdjustMode::Forward);
        assert_eq!("".parse::<AdjustMode>().unwrap(), AdjustMode::Forward);
    }
}
