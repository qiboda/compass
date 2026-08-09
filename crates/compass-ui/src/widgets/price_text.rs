//! Price text atom: monospace price with A-share up/down coloring and a
//! signed percentage change (design doc §5.1 `PriceText`).

use crate::tokens::ThemeTokens;
use egui::{Color32, Response, RichText, Ui};

/// Price tone; `Auto` derives the color from the sign of the change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Derive color from the change sign.
    #[default]
    Auto,
    /// Up (A-share red).
    Up,
    /// Down (A-share green).
    Down,
    /// Flat / unchanged.
    Flat,
}

/// Monospace price with change percentage, colored by tone.
pub struct PriceText<'a> {
    tokens: &'a ThemeTokens,
    price: f32,
    change: Option<f32>,
    tone: Tone,
    /// When set, the price value itself is a percentage (e.g. a change
    /// column): render only the signed percent form.
    percent_only: bool,
}

impl<'a> PriceText<'a> {
    /// Create a price text for the given theme.
    pub fn new(tokens: &'a ThemeTokens, price: f32) -> Self {
        Self {
            tokens,
            price,
            change: None,
            tone: Tone::Auto,
            percent_only: false,
        }
    }

    /// Set the change percentage (rendered as `+1.23%`).
    pub fn change(mut self, change: f32) -> Self {
        self.change = Some(change);
        self
    }

    /// Render only the signed percent form (the price value IS the change
    /// percentage, e.g. a 涨跌幅 column) instead of "price + change".
    pub fn percent_only(mut self) -> Self {
        self.percent_only = true;
        self
    }

    /// Set the tone explicitly (defaults to [`Tone::Auto`]).
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The color for the current tone / change.
    pub fn color(&self) -> Color32 {
        let c = &self.tokens.color;
        match self.tone {
            Tone::Auto => auto_tone(self.change).color(c),
            Tone::Up => c.up,
            Tone::Down => c.down,
            Tone::Flat => c.flat,
        }
    }

    /// The full rendered text: `12.34` plus ` +1.23%` when a change is set.
    /// In [`Self::percent_only`] mode the price itself is the percentage, so
    /// only the signed percent form renders (`+1.23%`).
    pub fn text(&self) -> String {
        match self.change {
            Some(change) if self.percent_only => format_change(change),
            Some(change) => format!("{:.2} {}", self.price, format_change(change)),
            None => format!("{:.2}", self.price),
        }
    }

    /// Show the price and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        ui.label(
            RichText::new(self.text())
                .monospace()
                .size(self.tokens.typography.mono)
                .color(self.color()),
        )
    }
}

/// The tone implied by a change value: positive → up, negative → down, else flat.
pub fn auto_tone(change: Option<f32>) -> Tone {
    match change {
        Some(c) if c > 0.0 => Tone::Up,
        Some(c) if c < 0.0 => Tone::Down,
        _ => Tone::Flat,
    }
}

impl Tone {
    fn color(&self, c: &crate::tokens::ColorTokens) -> Color32 {
        match self {
            Tone::Auto => unreachable!("Auto resolves to a concrete tone first"),
            Tone::Up => c.up,
            Tone::Down => c.down,
            Tone::Flat => c.flat,
        }
    }
}

/// Format a change percentage as `+1.23%` / `-0.45%` (unsigned `0.00%` for zero).
pub fn format_change(change: f32) -> String {
    if change.abs() < 0.005 {
        return "0.00%".to_string();
    }
    format!("{:+.2}%", change)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Percentage formatting: signed, two decimals, `%` suffix.
    #[test]
    fn format_change_matches_design() {
        assert_eq!(format_change(1.234), "+1.23%");
        assert_eq!(format_change(-0.4567), "-0.46%");
        assert_eq!(format_change(0.0), "0.00%");
        assert_eq!(format_change(-0.001), "0.00%");
    }

    /// Auto tone follows the A-share convention: positive up, negative down.
    #[test]
    fn auto_tone_follows_sign() {
        assert_eq!(auto_tone(Some(1.5)), Tone::Up);
        assert_eq!(auto_tone(Some(-1.5)), Tone::Down);
        assert_eq!(auto_tone(Some(0.0)), Tone::Flat);
        assert_eq!(auto_tone(None), Tone::Flat);
    }

    /// Auto coloring uses the up/down tokens.
    #[test]
    fn auto_colors_use_up_down_tokens() {
        let tokens = ThemeTokens::dark();
        let up = PriceText::new(&tokens, 10.0).change(1.5);
        assert_eq!(up.color(), tokens.color.up);
        let down = PriceText::new(&tokens, 10.0).change(-1.5);
        assert_eq!(down.color(), tokens.color.down);
        let flat = PriceText::new(&tokens, 10.0).change(0.0);
        assert_eq!(flat.color(), tokens.color.flat);
    }

    /// The rendered text includes the price and signed change.
    #[test]
    fn rendered_text_contains_price_and_change() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            PriceText::new(&tokens, 12.34).change(1.23).show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("12.34 +1.23%");
    }

    /// Percent-only mode renders a single signed percent form, not the
    /// duplicated "price + change" (ref #221 fix: 涨跌幅 column showed
    /// "2.50 +2.50%" for one value).
    #[test]
    fn percent_only_renders_single_signed_percent() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            PriceText::new(&tokens, 2.50)
                .change(2.50)
                .percent_only()
                .show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("+2.50%");
    }

    /// Percent-only keeps the up/down coloring from the change sign.
    #[test]
    fn percent_only_colors_by_change_sign() {
        let tokens = ThemeTokens::dark();
        let up = PriceText::new(&tokens, 2.50).change(2.50).percent_only();
        assert_eq!(up.color(), tokens.color.up);
        let down = PriceText::new(&tokens, -1.23).change(-1.23).percent_only();
        assert_eq!(down.color(), tokens.color.down);
        let flat = PriceText::new(&tokens, 0.0).change(0.0).percent_only();
        assert_eq!(flat.color(), tokens.color.flat);
    }
}
