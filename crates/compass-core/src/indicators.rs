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

/// Forward-adjust a raw OHLCV series into chart bars.
///
/// Adjustment factor per bar: `factor = adjclose_i / close_i` (forward-adjusted
/// close over unadjusted close). The latest bar has `adjclose == close` so its
/// factor is 1.0 (前复权锚点). Every OHLC price is scaled by its bar's factor;
/// volume is passed through unchanged.
///
/// Bars with `close <= 0` or non-finite `adjclose` fall back to `factor = 1.0`
/// (no scaling) instead of producing NaN/Inf. Output bars are in ascending
/// time order, matching the input.
///
/// # Panics
///
/// Never panics; `adjclose.len()` must equal `raw.len()` (debug-asserted).
pub fn adjust_ohlc(raw: &[RawBar], adjclose: &[f64]) -> Vec<Bar> {
    debug_assert_eq!(
        adjclose.len(),
        raw.len(),
        "adjclose must provide exactly one factor per raw bar"
    );
    raw.iter()
        .zip(adjclose)
        .map(|(r, &adj)| {
            let factor = if r.close > 0.0 && adj.is_finite() && adj > 0.0 {
                adj / r.close
            } else {
                1.0
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

    /// Latest bar has adjclose == close (前复权锚点) → factor 1.0, prices
    /// unchanged; historical bars scaled so that scaled_close == adjclose.
    #[test]
    fn adjust_ohlc_scales_ohlc_by_adjclose_over_close() {
        // Two bars: latest close 20 / adjclose 20 (anchor), older close 10 /
        // adjclose 8 (factor 0.8).
        let raw = raw_bars(&[10.0, 20.0]);
        let adj = vec![8.0, 20.0];
        let bars = adjust_ohlc(&raw, &adj);

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
        let bars = adjust_ohlc(&raw, &[8.0, 20.0]);
        assert_eq!(bars[0].time.date_naive(), raw[0].date);
        assert_eq!(bars[1].time.date_naive(), raw[1].date);
    }

    /// close == 0 must not produce NaN/Inf — factor falls back to 1.0.
    #[test]
    fn adjust_ohlc_zero_close_falls_back_to_factor_one() {
        let raw = raw_bars(&[0.0, 20.0]);
        let bars = adjust_ohlc(&raw, &[0.0, 20.0]);
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
        let bars = adjust_ohlc(&raw, &[f64::NAN, 20.0]);
        assert!(bars[0].close.is_finite());
        assert_eq!(bars[0].close, 10.0); // factor 1.0 → unchanged
    }

    /// Empty input → empty output, no panic.
    #[test]
    fn adjust_ohlc_empty_input_returns_empty() {
        assert!(adjust_ohlc(&[], &[]).is_empty());
    }
}
