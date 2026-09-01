use crate::citizens::indicators::MaBollIndicator;
use crate::state::SharedState;
use crate::theme::CompassTheme;
use compass_i18n::t;
use compass_ui::widgets::empty_state::EmptyState;
use egui::Color32;
use egui_charts::ChartType;
use egui_charts::model::BarData;
use egui_charts::studies::{IndicatorRegistry, IndicatorValue};
use egui_charts::widget::Chart;
use egui_citizen::{Citizen, CitizenId, CitizenState};

/// Chart panel citizen — renders an interactive OHLCV candlestick chart.
///
/// Reads `bars` from `SharedState` reactively and updates the chart
/// widget whenever data is available. The active theme's chart colors
/// (candles, grid, crosshair) are applied each frame via `app_theme`.
/// When no bars are loaded yet, an [`EmptyState`] guide is shown instead
/// of the chart (design §6.7).
pub struct ChartCitizen {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    chart: Chart,
    /// MA/BOLL overlay indicator registry, computed from the current bars.
    registry: IndicatorRegistry,
    /// Fingerprint of the bars the registry was last computed for:
    /// (symbol, bar count, first bar time, last bar time, last close bits).
    /// The last-close guard catches price revisions inside an unchanged
    /// date window — e.g. a 前复权 re-adjustment after a dividend rewrites
    /// every price while keeping the count and first/last timestamps the
    /// same — so a re-fetch cannot serve stale overlay values.
    cache_key: Option<(String, usize, i64, i64, u64)>,
}

impl Citizen for ChartCitizen {
    fn id(&self) -> &CitizenId {
        &self.citizen_id
    }

    fn citizen_state(&self) -> &CitizenState {
        &self.citizen_state
    }

    fn citizen_state_mut(&mut self) -> &mut CitizenState {
        &mut self.citizen_state
    }
}

impl ChartCitizen {
    /// Creates a new `ChartCitizen` with an empty candlestick chart.
    ///
    /// The chart is pre-configured with 100 visible bars and a "1d" timeframe
    /// label — the real symbol, data and theme colors are applied reactively
    /// on each frame in `show`.
    pub fn new(citizen_id: CitizenId, citizen_state: CitizenState) -> Self {
        let bars: Vec<egui_charts::model::Bar> = Vec::new();
        let data = BarData::from_bars(bars);
        let mut chart = Chart::new(data);
        chart.set_chart_type(ChartType::Candles);
        chart.set_visible_bars(100);
        chart.set_timeframe_label("1d");
        let mut registry = IndicatorRegistry::new();
        registry.add(Box::new(MaBollIndicator::new()));
        Self {
            citizen_id,
            citizen_state,
            chart,
            registry,
            cache_key: None,
        }
    }

    /// Renders the chart panel with the given theme.
    ///
    /// Applies `app_theme` chart colors (candles, grid, crosshair) each
    /// frame, reads `bars` from shared state, and delegates rendering to
    /// the egui-charts widget — or an empty-state guide when no bars exist.
    ///
    /// The MA/BOLL overlay is recomputed only when the bar series fingerprint
    /// (symbol, bar count, first/last bar timestamps, last close) changes,
    /// and its colors are re-applied from the theme every frame. A custom
    /// second legend row is painted over the chart's top-left corner.
    pub fn show(&mut self, ui: &mut egui::Ui, state: &SharedState, app_theme: &CompassTheme) {
        app_theme.apply_to_chart(&mut self.chart);
        self.chart.set_symbol(&state.symbol.get());

        let bars = state.bars.get();
        if bars.is_empty() {
            let empty_title = t!("chart.empty_title");
            let empty_desc = t!("chart.empty_desc");
            EmptyState::new(
                app_theme.tokens(),
                egui_phosphor::regular::CHART_LINE,
                &empty_title,
            )
            .description(&empty_desc)
            .show(ui);
            return;
        }

        // Recompute indicators only when the series actually changed; the
        // symbol is part of the fingerprint so switching to a symbol with
        // identical bar shapes cannot serve stale values.
        let fingerprint = Some((
            state.symbol.get(),
            bars.len(),
            bars[0].time.timestamp(),
            bars[bars.len() - 1].time.timestamp(),
            bars[bars.len() - 1].close.to_bits(),
        ));
        if fingerprint != self.cache_key {
            self.registry.calculate_all(&bars);
            self.cache_key = fingerprint;
        }

        self.chart.update_data(BarData::from_bars(bars));
        self.chart.set_timeframe_label(&state.timeframe.get());

        // Theme indicator colors are applied every frame so a theme switch
        // recolors the overlay immediately (tokens are the single source).
        let indicator = &app_theme.tokens().color.indicator;
        if let Some(first) = self.registry.indicators_mut().first_mut() {
            first.set_colors(vec![
                indicator.ma5,
                indicator.ma10,
                indicator.ma60,
                indicator.ma120,
                indicator.ma250,
                indicator.bb_upper,
                indicator.bb_middle,
                indicator.bb_lower,
            ]);
        }

        let response = self
            .chart
            .show_with_indicators(ui, None, Some(&self.registry));
        self.draw_indicator_legend(ui, response, app_theme.tokens());
    }

    /// Paints the static MA/BOLL legend row below the vendored OHLC legend.
    ///
    /// Reads the indicator values on the last visible bar and draws a
    /// translucent chip (`bg_panel_alt` at 85% alpha, 1px `border_strong`
    /// stroke, `radius.sm`) with one mono 12px value per MA line plus a
    /// joined three-value BOLL item, separated from the MA group by a 1px
    /// `border_strong` divider. Warmup values show "—". Purely decorative:
    /// it does not consume input.
    fn draw_indicator_legend(
        &self,
        ui: &egui::Ui,
        response: egui::Response,
        tokens: &compass_ui::tokens::ThemeTokens,
    ) {
        let (_, end) = self.chart.state.visible_range();
        let Some(first) = self.registry.indicators().first() else {
            return;
        };
        let Some(IndicatorValue::Multiple(values)) = first.values().get(end.saturating_sub(1))
        else {
            return;
        };

        /// One legend atom: a text run or the MA↔BOLL group divider.
        enum Segment {
            Text(egui::FontId, Color32, String),
            Divider,
        }

        let line_colors = first.colors();
        let label_font = egui::FontId::proportional(tokens.typography.caption);
        let mono_font = egui::FontId::monospace(tokens.typography.mono);
        let painter = ui.painter_at(response.rect);

        // Vendored format_price rule (labels.rs): ≥100 → 2 decimals, ≥1 → 4,
        // <1 → 6, keeping the legend aligned with the OHLC legend precision.
        let format_price = |price: f64| -> String {
            if price >= 100.0 {
                format!("{price:.2}")
            } else if price >= 1.0 {
                format!("{price:.4}")
            } else {
                format!("{price:.6}")
            }
        };
        let format_value = |v: &f64| -> String {
            if v.is_nan() {
                "—".to_owned()
            } else {
                format_price(*v)
            }
        };

        // Segment list with the gap to advance after each one: MA items are
        // caption labels (text_secondary) + mono values in the line color,
        // spaced by spacing.sm; the BOLL group is one "BOLL" label + the
        // three slate values joined by " / "; a 1px divider with spacing.md
        // on each side sits between the groups.
        let names = first.line_names();
        let ma_names = &names[..5];
        let mut segments: Vec<Segment> = Vec::with_capacity(ma_names.len() * 2 + 3);
        let mut gaps: Vec<f32> = Vec::with_capacity(segments.capacity());
        let space = painter
            .layout_no_wrap(" ".to_owned(), label_font.clone(), Color32::WHITE)
            .size()
            .x;
        for (i, name) in ma_names.iter().enumerate() {
            segments.push(Segment::Text(
                label_font.clone(),
                tokens.color.text_secondary,
                name.clone(),
            ));
            gaps.push(space);
            segments.push(Segment::Text(
                mono_font.clone(),
                line_colors[i],
                format_value(&values[i]),
            ));
            gaps.push(if i == ma_names.len() - 1 {
                tokens.spacing.md
            } else {
                tokens.spacing.sm
            });
        }
        segments.push(Segment::Divider);
        gaps.push(tokens.spacing.md);
        segments.push(Segment::Text(
            label_font.clone(),
            tokens.color.text_secondary,
            "BOLL".to_owned(),
        ));
        gaps.push(space);
        segments.push(Segment::Text(
            mono_font.clone(),
            line_colors[5],
            [&values[5], &values[6], &values[7]]
                .iter()
                .map(|v| format_value(v))
                .collect::<Vec<_>>()
                .join(" / "),
        ));
        gaps.push(0.0);

        const DIVIDER_WIDTH: f32 = 1.0;
        let mut total_width = 0.0f32;
        let mut widths = Vec::with_capacity(segments.len());
        for segment in &segments {
            let width = match segment {
                Segment::Text(font, color, text) => {
                    painter
                        .layout_no_wrap(text.clone(), font.clone(), *color)
                        .size()
                        .x
                }
                Segment::Divider => DIVIDER_WIDTH,
            };
            widths.push(width);
            total_width += width;
        }
        total_width += gaps.iter().sum::<f32>();

        let row_height = painter
            .layout_no_wrap("Ag".to_owned(), mono_font.clone(), Color32::WHITE)
            .size()
            .y;
        let padding = egui::vec2(6.0, 4.0);
        let chip = egui::Rect::from_min_size(
            response.rect.min + egui::vec2(40.0, 30.0),
            egui::vec2(total_width + padding.x * 2.0, row_height + padding.y * 2.0),
        );

        let bg = Color32::from_rgba_unmultiplied(
            tokens.color.bg_panel_alt.r(),
            tokens.color.bg_panel_alt.g(),
            tokens.color.bg_panel_alt.b(),
            218,
        );
        painter.rect_filled(chip, tokens.radius.sm, bg);
        painter.rect_stroke(
            chip,
            tokens.radius.sm,
            egui::Stroke::new(1.0, tokens.color.border_strong),
            egui::StrokeKind::Inside,
        );

        let mut x = chip.min.x + padding.x;
        let y = chip.min.y + padding.y;
        for ((segment, gap), width) in segments.iter().zip(&gaps).zip(widths) {
            match segment {
                Segment::Text(font, color, text) => {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::LEFT_TOP,
                        text.clone(),
                        font.clone(),
                        *color,
                    );
                }
                Segment::Divider => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(x, chip.min.y + padding.y),
                            egui::vec2(DIVIDER_WIDTH, row_height),
                        ),
                        egui::CornerRadius::ZERO,
                        tokens.color.border_strong,
                    );
                }
            }
            x += width + gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use chrono::Utc;
    use compass_i18n::t;
    use egui_charts::model::Bar;
    use egui_charts::studies::IndicatorValue;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    /// Key-resolution test helper (plan T4): resolves a key through the
    /// shared compass-i18n dictionary.
    fn tr(key: &str) -> String {
        t!(key).to_string()
    }

    fn make_bar(
        time: chrono::DateTime<Utc>,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> Bar {
        Bar::new(time, open, high, low, close, volume)
    }

    #[test]
    fn new_creates_citizen_with_correct_id() {
        let id = CitizenId::new("test_chart");
        let state = CitizenState::new();
        let citizen = ChartCitizen::new(id.clone(), state.clone());

        assert_eq!(citizen.citizen_id, id);
        assert_eq!(citizen.id(), &id);
    }

    #[test]
    fn show_empty_bars_renders_empty_state() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("chart");
        let state = CitizenState::new();
        let mut citizen = ChartCitizen::new(id, state);

        let shared = SharedState::new("000001", "1d", "qfq");
        let theme = CompassTheme::compass_dark();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            citizen.show(ui, &shared, &theme);
        });
        harness.run();
        let _ = harness.get_by_label(&tr("chart.empty_title"));
        let _ = harness.get_by_label_contains(&tr("chart.empty_desc"));
    }

    #[test]
    fn show_empty_bars_no_panic() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("chart");
        let state = CitizenState::new();
        let mut citizen = ChartCitizen::new(id, state);

        let shared = SharedState::new("000001", "1d", "qfq");
        let theme = CompassTheme::compass_dark();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            citizen.show(ui, &shared, &theme);
        });
        harness.run();
    }

    #[test]
    fn show_with_bars_renders_chart_not_empty_state() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("chart");
        let state = CitizenState::new();
        let mut citizen = ChartCitizen::new(id, state);

        let shared = SharedState::new("000001", "1d", "qfq");
        shared.bars.set(vec![
            make_bar(Utc::now(), 100.0, 105.0, 98.0, 103.0, 1000.0),
            make_bar(
                Utc::now() + chrono::Duration::days(1),
                103.0,
                108.0,
                101.0,
                106.0,
                1200.0,
            ),
        ]);

        let theme = CompassTheme::compass_dark();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            citizen.show(ui, &shared, &theme);
        });
        harness.run();
        assert!(
            harness.query_by_label(&tr("chart.empty_title")).is_none(),
            "chart must render instead of the empty state when bars exist"
        );
    }

    /// Helper: bars with ascending closes starting at `base` over `count`
    /// consecutive days anchored at `start`.
    fn series(start: chrono::DateTime<Utc>, base: f64, count: usize) -> Vec<Bar> {
        (0..count)
            .map(|i| {
                let c = base + i as f64;
                make_bar(
                    start + chrono::Duration::days(i as i64),
                    c,
                    c + 2.0,
                    c - 2.0,
                    c,
                    1000.0,
                )
            })
            .collect()
    }

    /// With bars loaded, the registry carries hand-checkable MA5/MA10/BOLL
    /// values on the last visible bar (MA60+ still in warmup → NaN placeholders
    /// so the vendored renderer warms each line up independently).
    #[test]
    fn show_with_bars_computes_ma_and_boll_values() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("chart");
        let state = CitizenState::new();
        let mut citizen = ChartCitizen::new(id, state);

        // 20 bars, closes 1..=20.
        let start = Utc::now();
        let bars = series(start, 1.0, 20);

        let shared = SharedState::new("000001", "1d", "qfq");
        shared.bars.set(bars);
        let theme = CompassTheme::compass_dark();

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                citizen.show(ui, &shared, &theme);
            });
            harness.run();
        }

        let (_, end) = citizen.chart.state.visible_range();
        assert_eq!(end, 20, "all 20 bars are visible");
        let values = citizen.registry.indicators()[0].values();
        let IndicatorValue::Multiple(v) = &values[end - 1] else {
            panic!("expected Multiple values on the last bar");
        };
        assert_eq!(v.len(), 8, "5 MA lines + 3 BOLL lines");
        // MA5(19) = mean(16..=20) = 18.0; MA10(19) = mean(11..=20) = 15.5.
        assert!((v[0] - 18.0).abs() < 1e-9, "MA5 = 18.0, got {}", v[0]);
        assert!((v[1] - 15.5).abs() < 1e-9, "MA10 = 15.5, got {}", v[1]);
        for &(i, label) in &[(2, "MA60"), (3, "MA120"), (4, "MA250")] {
            assert!(
                v[i].is_nan(),
                "{label} must still warm up with only 20 bars (NaN placeholder)"
            );
        }
        // BOLL(20, 2.0) at the last bar: window 1..=20 → mid 10.5, population
        // stddev sqrt(33.25), bands = mid ± 2·std.
        let std = (33.25f64).sqrt();
        assert!((v[5] - (10.5 + 2.0 * std)).abs() < 1e-9, "BOLL upper");
        assert!((v[6] - 10.5).abs() < 1e-9, "BOLL middle");
        assert!((v[7] - (10.5 - 2.0 * std)).abs() < 1e-9, "BOLL lower");
    }

    /// Switching symbols must recompute the indicator even when the new bars
    /// share the exact cache fingerprint (same len, same first/last times) —
    /// the symbol is part of the cache key, guarding against stale values.
    #[test]
    fn show_recomputes_indicator_values_on_symbol_change_same_fingerprint() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = CitizenId::new("chart");
        let state = CitizenState::new();
        let mut citizen = ChartCitizen::new(id, state);

        let start = Utc::now();
        let bars_a = series(start, 1.0, 10);
        let bars_b = series(start, 101.0, 10);
        let theme = CompassTheme::compass_dark();

        let shared_a = SharedState::new("600001", "1d", "qfq");
        shared_a.bars.set(bars_a);
        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                citizen.show(ui, &shared_a, &theme);
            });
            harness.run();
        }

        let shared_b = SharedState::new("600002", "1d", "qfq");
        shared_b.bars.set(bars_b);
        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                citizen.show(ui, &shared_b, &theme);
            });
            harness.run();
        }

        let (_, end) = citizen.chart.state.visible_range();
        let values = citizen.registry.indicators()[0].values();
        let IndicatorValue::Multiple(v) = &values[end - 1] else {
            panic!("expected Multiple values on the last bar");
        };
        // closes 101..=110 → MA5 of last bar = mean(106..=110) = 108.0.
        // A cache key without the symbol would reuse the stale 8.0 from bars_a.
        assert!(
            (v[0] - 108.0).abs() < 1e-9,
            "MA5 must be recomputed for the new symbol, got {}",
            v[0]
        );
    }
}
