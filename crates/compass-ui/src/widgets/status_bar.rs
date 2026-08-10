//! Status bar composite: three-segment status strip with a stock summary,
//! status indicator and source/clock (design doc §5.2 `StatusBar`).
//!
//! Pure UI — all data arrives via [`StatusBarData`]; the clock string is
//! formatted by the caller so this crate stays chrono-free.

use egui::{Align, Layout, RichText};

use crate::tokens::ThemeTokens;

use super::price_text::PriceText;
use super::status_dot::{DotState, StatusDot};

/// Left-segment stock summary.
pub struct StockSummary {
    /// Symbol, e.g. `600519`.
    pub symbol: String,
    /// Display name, e.g. `贵州茅台`.
    pub name: String,
    /// Latest price; `None` hides the price text.
    pub price: Option<f32>,
    /// Change percentage; `None` renders the price flat-colored.
    pub change: Option<f32>,
}

/// Overall status of the application shown in the middle segment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusKind {
    /// Idle / no pending work.
    #[default]
    Idle,
    /// Work in progress (breathing dot).
    Loading,
    /// An error occurred (error dot).
    Error,
    /// The last operation succeeded (success dot).
    Success,
}

/// All data the status bar renders in one frame.
pub struct StatusBarData {
    /// Stock summary shown on the left; `None` hides the left segment.
    pub summary: Option<StockSummary>,
    /// Status of the middle segment.
    pub status: StatusKind,
    /// Status text next to the dot.
    pub status_text: String,
    /// Data source label on the right (e.g. `本地数据源`).
    pub source: String,
    /// Clock string formatted by the caller (e.g. `2026-08-02 10:30:00`).
    pub clock: String,
}

/// Three-segment status strip (26 px tall).
pub struct StatusBar<'a> {
    tokens: &'a ThemeTokens,
}

impl<'a> StatusBar<'a> {
    /// Create a status bar for the given theme.
    pub fn new(tokens: &'a ThemeTokens) -> Self {
        Self { tokens }
    }

    /// The dot state implied by a [`StatusKind`].
    pub fn dot_state(&self, kind: StatusKind) -> DotState {
        match kind {
            StatusKind::Idle => DotState::Idle,
            StatusKind::Loading => DotState::Loading,
            StatusKind::Error => DotState::Error,
            StatusKind::Success => DotState::Success,
        }
    }

    /// Show the status bar.
    pub fn show(&self, ui: &mut egui::Ui, data: &StatusBarData) {
        let tokens = self.tokens;
        let c = &tokens.color;

        ui.set_min_height(tokens.spacing.statusbar_h);

        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());

            // Left: stock summary (symbol + name + mono price).
            if let Some(summary) = &data.summary {
                ui.label(
                    RichText::new(format!("{} {}", summary.symbol, summary.name))
                        .size(tokens.typography.body)
                        .color(c.text_secondary),
                );
                if let Some(price) = summary.price {
                    let mut price_text = PriceText::new(tokens, price);
                    if let Some(change) = summary.change {
                        price_text = price_text.change(change);
                    }
                    price_text.show(ui);
                }
                ui.add_space(tokens.spacing.md);
            }

            // Middle: status dot + text.
            if !data.status_text.is_empty() {
                StatusDot::new(tokens, self.dot_state(data.status)).show(ui);
                ui.label(
                    RichText::new(&data.status_text)
                        .size(tokens.typography.caption)
                        .color(c.text_secondary),
                );
                ui.add_space(tokens.spacing.md);
            }

            // Right: source + clock, mono.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(&data.clock)
                        .monospace()
                        .size(tokens.typography.mono)
                        .color(c.text_secondary),
                );
                ui.add_space(tokens.spacing.sm);
                if !data.source.is_empty() {
                    ui.label(
                        RichText::new(&data.source)
                            .size(tokens.typography.caption)
                            .color(c.text_weak),
                    );
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    fn data() -> StatusBarData {
        StatusBarData {
            summary: Some(StockSummary {
                symbol: "600519".into(),
                name: "贵州茅台".into(),
                price: Some(1500.25),
                change: Some(1.23),
            }),
            status: StatusKind::Loading,
            status_text: "加载中…".into(),
            source: "本地数据源".into(),
            clock: "2026-08-02 10:30:00".into(),
        }
    }

    #[test]
    fn dot_state_maps_kinds() {
        let tokens = ThemeTokens::dark();
        let bar = StatusBar::new(&tokens);
        assert_eq!(bar.dot_state(StatusKind::Idle), DotState::Idle);
        assert_eq!(bar.dot_state(StatusKind::Loading), DotState::Loading);
        assert_eq!(bar.dot_state(StatusKind::Error), DotState::Error);
        assert_eq!(bar.dot_state(StatusKind::Success), DotState::Success);
    }

    #[test]
    fn renders_three_segments() {
        rust_i18n::set_locale("zh");
        let tokens = ThemeTokens::dark();
        let bar = StatusBar::new(&tokens);
        let data = data();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            bar.show(ui, &data);
        });
        harness.step();
        let _ = harness.get_by_label_contains("600519");
        let _ = harness.get_by_label_contains("1500.25");
        let _ = harness.get_by_label_contains("加载中");
        let _ = harness.get_by_label("本地数据源");
        let _ = harness.get_by_label("2026-08-02 10:30:00");
    }

    #[test]
    fn renders_without_summary() {
        rust_i18n::set_locale("zh");
        let tokens = ThemeTokens::dark();
        let bar = StatusBar::new(&tokens);
        let mut data = data();
        data.summary = None;
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            bar.show(ui, &data);
        });
        harness.step();
        let _ = harness.get_by_label_contains("加载中");
    }
}
