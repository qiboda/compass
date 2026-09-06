//! MA/BOLL overlay indicator for the candlestick chart.
//!
//! [`MaBollIndicator`] renders MA(5/10/60/120/250) moving averages plus
//! BOLL(20, 2.0) bands as a single eight-line overlay on the main price
//! chart. The math lives in `compass-core` pure functions; this type is a
//! thin adapter onto the vendored `egui_charts::studies::Indicator` trait.

use compass_core::indicators::{bollinger, ma};
use egui::Color32;
use egui_charts::model::Bar;
use egui_charts::studies::{Indicator, IndicatorValue};

/// Eight-line MA(5/10/60/120/250) + BOLL(20, 2.0) overlay.
///
/// One [`IndicatorValue::Multiple`] per input bar: `[ma5, ma10, ma60,
/// ma120, ma250, bb_upper, bb_middle, bb_lower]`. Bars inside a line's
/// warmup window carry `f64::NAN` placeholders — the vendored renderer
/// skips NaN points, so each line warms up independently (MA5 from bar 5,
/// MA250 from bar 250).
#[derive(Clone)]
pub struct MaBollIndicator {
    /// Computed per-bar values (one entry per input bar).
    values: Vec<IndicatorValue>,
    /// One color per line, applied every frame from the theme tokens.
    colors: Vec<Color32>,
    /// Whether the overlay is currently rendered.
    visible: bool,
    /// MA periods, one per overlay line (index 0..4). Held as instance state
    /// so the chart sidebar can edit them live (plan §4.1); `calculate`
    /// reads these instead of the historical hardcoded 5/10/60/120/250.
    ma_periods: [usize; 5],
    /// BOLL band period and standard-deviation multiplier (sidebar-editable).
    boll_period: usize,
    boll_std: f64,
}

impl MaBollIndicator {
    /// Creates a new indicator with the design-mandated dark palette
    /// defaults (theme tokens override them each frame) and the canonical
    /// MA(5/10/60/120/250) + BOLL(20, 2.0) parameter set.
    pub fn new() -> Self {
        Self::with_periods(DEFAULT_MA_PERIODS, DEFAULT_BOLL_PERIOD, DEFAULT_BOLL_STD)
    }

    /// Creates a new indicator over an explicit parameter set (plan §4.1:
    /// the chart sidebar edits MA periods and BOLL parameters live).
    ///
    /// Caller is responsible for valid parameters (no guard here): periods
    /// must be non-zero, `boll_std` must be finite and > 0. The `set_*`
    /// setters enforce these rules for runtime edits (plan §4.1).
    pub fn with_periods(ma_periods: [usize; 5], boll_period: usize, boll_std: f64) -> Self {
        Self {
            values: Vec::new(),
            colors: vec![
                Color32::from_rgb(0xD1, 0xD4, 0xDC), // MA5
                Color32::from_rgb(0xF5, 0xA6, 0x23), // MA10
                Color32::from_rgb(0xBA, 0x68, 0xC8), // MA60
                Color32::from_rgb(0x00, 0xBC, 0xD4), // MA120
                Color32::from_rgb(0xA1, 0x88, 0x7F), // MA250
                Color32::from_rgb(0x90, 0xA4, 0xAE), // BOLL upper
                Color32::from_rgb(0x90, 0xA4, 0xAE), // BOLL middle
                Color32::from_rgb(0x90, 0xA4, 0xAE), // BOLL lower
            ],
            visible: true,
            ma_periods,
            boll_period,
            boll_std,
        }
    }

    /// The five MA periods currently in effect.
    pub fn ma_periods(&self) -> &[usize; 5] {
        &self.ma_periods
    }

    /// Replace one MA period (index 0..5). Out-of-range indices and zero
    /// periods are ignored so a corrupt UI value cannot poison the overlay.
    pub fn set_ma_period(&mut self, index: usize, period: usize) {
        if index < self.ma_periods.len() && period > 0 {
            self.ma_periods[index] = period;
        }
    }

    /// The BOLL band period in effect.
    pub fn boll_period(&self) -> usize {
        self.boll_period
    }

    /// The BOLL standard-deviation multiplier in effect.
    pub fn boll_std(&self) -> f64 {
        self.boll_std
    }

    /// Replace the BOLL band period; zero is ignored (division-by-zero guard
    /// at the `compass-core` helper stays meaningful).
    pub fn set_boll_period(&mut self, period: usize) {
        if period > 0 {
            self.boll_period = period;
        }
    }

    /// Replace the BOLL standard-deviation multiplier; non-finite or
    /// zero values are ignored.
    pub fn set_boll_std(&mut self, std: f64) {
        if std.is_finite() && std > 0.0 {
            self.boll_std = std;
        }
    }
}

/// Canonical MA periods (design/plan §4.1 defaults).
pub const DEFAULT_MA_PERIODS: [usize; 5] = [5, 10, 60, 120, 250];
/// Canonical BOLL period.
pub const DEFAULT_BOLL_PERIOD: usize = 20;
/// Canonical BOLL standard-deviation multiplier.
pub const DEFAULT_BOLL_STD: f64 = 2.0;

impl Default for MaBollIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Indicator for MaBollIndicator {
    fn name(&self) -> &str {
        "MA/BOLL"
    }

    fn desc(&self) -> &str {
        "MA(5/10/60/120/250) moving averages plus BOLL(20, 2.0) bands"
    }

    fn calculate(&mut self, data: &[Bar]) {
        let closes: Vec<f64> = data.iter().map(|bar| bar.close).collect();
        let [p0, p1, p2, p3, p4] = self.ma_periods;
        let ma0 = ma(&closes, p0);
        let ma1 = ma(&closes, p1);
        let ma2 = ma(&closes, p2);
        let ma3 = ma(&closes, p3);
        let ma4 = ma(&closes, p4);
        let bands = bollinger(&closes, self.boll_period, self.boll_std);

        let nan = f64::NAN;
        self.values = closes
            .iter()
            .enumerate()
            .map(|(i, _)| {
                IndicatorValue::Multiple(vec![
                    ma0[i].unwrap_or(nan),
                    ma1[i].unwrap_or(nan),
                    ma2[i].unwrap_or(nan),
                    ma3[i].unwrap_or(nan),
                    ma4[i].unwrap_or(nan),
                    bands[i].0.unwrap_or(nan),
                    bands[i].1.unwrap_or(nan),
                    bands[i].2.unwrap_or(nan),
                ])
            })
            .collect();
    }

    fn values(&self) -> &[IndicatorValue] {
        &self.values
    }

    fn colors(&self) -> Vec<Color32> {
        self.colors.clone()
    }

    fn set_colors(&mut self, colors: Vec<Color32>) {
        if colors.len() == self.colors.len() {
            self.colors = colors;
        }
    }

    fn line_cnt(&self) -> usize {
        8
    }

    fn line_names(&self) -> Vec<String> {
        [
            format!("MA{}", self.ma_periods[0]),
            format!("MA{}", self.ma_periods[1]),
            format!("MA{}", self.ma_periods[2]),
            format!("MA{}", self.ma_periods[3]),
            format!("MA{}", self.ma_periods[4]),
            "BOLL-U".to_string(),
            "BOLL-M".to_string(),
            "BOLL-L".to_string(),
        ]
        .to_vec()
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn clone_box(&self) -> Box<dyn Indicator> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default constructor must carry the canonical parameter set
    /// (plan §4.1: MA 5/10/60/120/250 + BOLL 20/2.0).
    #[test]
    fn defaults_carry_canonical_parameters() {
        let ind = MaBollIndicator::new();
        assert_eq!(ind.ma_periods(), &[5, 10, 60, 120, 250]);
        assert_eq!(ind.boll_period(), 20);
        assert_eq!(ind.boll_std(), 2.0);
    }

    /// with_periods seeds the instance fields verbatim.
    #[test]
    fn with_periods_seeds_explicit_parameters() {
        let ind = MaBollIndicator::with_periods([3, 7, 14, 21, 30], 25, 1.5);
        assert_eq!(ind.ma_periods(), &[3, 7, 14, 21, 30]);
        assert_eq!(ind.boll_period(), 25);
        assert_eq!(ind.boll_std(), 1.5);
    }

    /// Setters apply per-index edits; out-of-range and zero inputs are
    /// ignored (cannot poison the overlay).
    #[test]
    fn setters_guard_invalid_inputs() {
        let mut ind = MaBollIndicator::new();
        ind.set_ma_period(2, 55);
        assert_eq!(ind.ma_periods()[2], 55);
        ind.set_ma_period(9, 99); // out of range → ignored
        assert_eq!(ind.ma_periods()[4], 250, "out-of-range set must be a no-op");
        ind.set_ma_period(0, 0); // zero period → ignored
        assert_eq!(ind.ma_periods()[0], 5);
        ind.set_boll_period(30);
        assert_eq!(ind.boll_period(), 30);
        ind.set_boll_period(0); // ignored
        assert_eq!(ind.boll_period(), 30);
        ind.set_boll_std(1.8);
        assert_eq!(ind.boll_std(), 1.8);
        ind.set_boll_std(0.0); // ignored
        assert_eq!(ind.boll_std(), 1.8);
        ind.set_boll_std(f64::NAN); // non-finite → ignored
        assert_eq!(ind.boll_std(), 1.8);
    }

    /// Line names reflect the live periods (the chart legend and the
    /// vendored label row must not keep stale "MA5" names after an edit).
    #[test]
    fn line_names_reflect_edited_periods() {
        let mut ind = MaBollIndicator::new();
        assert_eq!(
            ind.line_names(),
            [
                "MA5", "MA10", "MA60", "MA120", "MA250", "BOLL-U", "BOLL-M", "BOLL-L"
            ]
        );
        ind.set_ma_period(0, 8);
        assert_eq!(ind.line_names()[0], "MA8");
    }

    /// calculate must read the instance parameters: a period-2 MA warms up
    /// at bar index 1, a period-5 MA at index 4 (earlier values are NaN).
    #[test]
    fn calculate_uses_instance_periods() {
        let bars: Vec<Bar> = (0..10)
            .map(|i| Bar {
                time: chrono::TimeZone::timestamp_opt(&chrono::Utc, i, 0).unwrap(),
                open: 10.0,
                high: 12.0,
                low: 9.0,
                close: 10.0 + i as f64,
                volume: 100.0,
            })
            .collect();
        let mut ind = MaBollIndicator::with_periods([2, 3, 4, 5, 6], 5, 2.0);
        ind.calculate(&bars);
        let IndicatorValue::Multiple(values) = &ind.values()[1] else {
            panic!("expected Multiple values");
        };
        assert!(
            values[0].is_finite(),
            "MA{period} must be finite at bar index 1 (warmup = period - 1)",
            period = 2,
        );
        let IndicatorValue::Multiple(values5) = &ind.values()[3] else {
            panic!("expected Multiple values");
        };
        assert!(
            values5[3].is_nan(),
            "MA5 (line index 3, periods[3]=5) must still warp up at bar index 3 (warmup = period - 1)"
        );
        let IndicatorValue::Multiple(values5) = &ind.values()[4] else {
            panic!("expected Multiple values");
        };
        assert!(
            values5[3].is_finite(),
            "MA5 (line index 3, periods[3]=5) must be finite at bar index 4 (warmup = period - 1)"
        );
    }
}
