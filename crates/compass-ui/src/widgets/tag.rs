//! Tag atom: short labels for exchanges / boards / industries (design doc
//! §5.1 `Tag`).

use crate::tokens::ThemeTokens;
use egui::{Color32, Margin, Response, RichText, Ui};

/// Tag variant; the `Exchange` variant auto-colors by the exchange code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TagVariant {
    /// Exchange badge — auto-colored for `SH` / `SZ` / `BJ`.
    Exchange,
    /// Board tag (accent tint).
    Board,
    /// Industry tag (secondary tint).
    Industry,
    /// Custom color tag.
    #[default]
    Custom,
}

/// Short pill tag (20 px tall, 9–11 px text).
pub struct Tag<'a> {
    tokens: &'a ThemeTokens,
    text: &'a str,
    variant: TagVariant,
    color: Option<Color32>,
}

impl<'a> Tag<'a> {
    /// Create a tag for the given theme and text.
    pub fn new(tokens: &'a ThemeTokens, text: &'a str) -> Self {
        Self {
            tokens,
            text,
            variant: TagVariant::Custom,
            color: None,
        }
    }

    /// Set the tag variant.
    pub fn variant(mut self, variant: TagVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Override the color (used by `Custom` and as the tint base otherwise).
    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// (background, text color) for this tag per the design doc.
    pub fn colors(&self) -> (Color32, Color32) {
        let c = &self.tokens.color;
        match self.variant {
            TagVariant::Exchange => (exchange_color(self.text), Color32::WHITE),
            TagVariant::Board => {
                let base = self.color.unwrap_or(c.accent);
                (tint(base, 0.18), base)
            }
            TagVariant::Industry => {
                let base = self.color.unwrap_or(c.text_secondary);
                (tint(base, 0.18), base)
            }
            TagVariant::Custom => {
                let base = self.color.unwrap_or(c.accent);
                (tint(base, 0.18), base)
            }
        }
    }

    /// Show the tag pill and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let tokens = self.tokens;
        let (bg, fg) = self.colors();
        let frame = egui::Frame::new()
            .fill(bg)
            .corner_radius(tokens.radius.pill)
            .inner_margin(Margin::symmetric(6, 3));
        frame
            .show(ui, |ui| {
                ui.label(
                    RichText::new(self.text)
                        .size(tokens.typography.caption)
                        .color(fg),
                )
            })
            .response
    }
}

/// The design-mandated exchange badge colors: SH blue / SZ green / BJ purple.
pub fn exchange_color(text: &str) -> Color32 {
    match text.trim().to_uppercase().as_str() {
        "SH" => Color32::from_rgb(0x29, 0x62, 0xFF),
        "SZ" => Color32::from_rgb(0x0E, 0x9F, 0x6E),
        "BJ" => Color32::from_rgb(0x8B, 0x5C, 0xF6),
        _ => Color32::from_rgb(0x29, 0x62, 0xFF),
    }
}

/// Mix `base` over a transparent background at the given alpha (0..=1).
fn tint(base: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_premultiplied(
        (base.r() as f32 * alpha) as u8,
        (base.g() as f32 * alpha) as u8,
        (base.b() as f32 * alpha) as u8,
        (255.0 * alpha) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Exchange colors follow the design spec for SH / SZ / BJ.
    #[test]
    fn exchange_colors_follow_design() {
        assert_eq!(exchange_color("SH"), Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(exchange_color("SZ"), Color32::from_rgb(0x0E, 0x9F, 0x6E));
        assert_eq!(exchange_color("BJ"), Color32::from_rgb(0x8B, 0x5C, 0xF6));
        // Case-insensitive and unknown codes fall back to the default blue.
        assert_eq!(exchange_color("sh"), Color32::from_rgb(0x29, 0x62, 0xFF));
        assert_eq!(exchange_color("ZZ"), Color32::from_rgb(0x29, 0x62, 0xFF));
    }

    /// Exchange tags render white text on the exchange color.
    #[test]
    fn exchange_tag_uses_white_text() {
        let tokens = ThemeTokens::dark();
        let tag = Tag::new(&tokens, "SH").variant(TagVariant::Exchange);
        let (bg, fg) = tag.colors();
        assert_eq!(bg, exchange_color("SH"));
        assert_eq!(fg, Color32::WHITE);
    }

    /// Custom tags tint the base color at low alpha.
    #[test]
    fn custom_tag_tints_base_color() {
        let tokens = ThemeTokens::dark();
        let tag = Tag::new(&tokens, "X").color(Color32::from_rgb(0xFF, 0x00, 0x00));
        let (bg, fg) = tag.colors();
        assert_eq!(fg, Color32::from_rgb(0xFF, 0x00, 0x00));
        assert!(bg.a() < 128, "tint background must be translucent");
    }

    /// The tag text is rendered and queryable.
    #[test]
    fn tag_text_is_queryable() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Tag::new(&tokens, "SH")
                .variant(TagVariant::Exchange)
                .show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("SH");
    }
}
