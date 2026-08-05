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
}

impl MaBollIndicator {
    /// Creates a new indicator with the design-mandated dark palette
    /// defaults (theme tokens override them each frame).
    pub fn new() -> Self {
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
        }
    }
}

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
        let ma5 = ma(&closes, 5);
        let ma10 = ma(&closes, 10);
        let ma60 = ma(&closes, 60);
        let ma120 = ma(&closes, 120);
        let ma250 = ma(&closes, 250);
        let bands = bollinger(&closes, 20, 2.0);

        let nan = f64::NAN;
        self.values = closes
            .iter()
            .enumerate()
            .map(|(i, _)| {
                IndicatorValue::Multiple(vec![
                    ma5[i].unwrap_or(nan),
                    ma10[i].unwrap_or(nan),
                    ma60[i].unwrap_or(nan),
                    ma120[i].unwrap_or(nan),
                    ma250[i].unwrap_or(nan),
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
            "MA5", "MA10", "MA60", "MA120", "MA250", "BOLL-U", "BOLL-M", "BOLL-L",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
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
