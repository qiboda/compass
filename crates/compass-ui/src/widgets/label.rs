//! Label atom: text with level (color) and size (typography) tokens
//! (design doc §5.1 `Label`).

use crate::tokens::ThemeTokens;
use egui::{Color32, Response, RichText, Ui};

/// Text level — maps to the color tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelLevel {
    /// Primary text.
    #[default]
    Primary,
    /// Secondary text.
    Secondary,
    /// Weak / placeholder text.
    Weak,
    /// Disabled text.
    Disabled,
}

/// Text size — maps to the typography tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelSize {
    /// Body (12.5 px).
    #[default]
    Body,
    /// Caption (11 px).
    Caption,
    /// Heading (14 px).
    Heading,
}

/// Styled text label driven entirely by the token system.
pub struct Label<'a> {
    tokens: &'a ThemeTokens,
    text: String,
    level: LabelLevel,
    size: LabelSize,
}

impl<'a> Label<'a> {
    /// Create a label for the given theme with the given text.
    pub fn new(tokens: &'a ThemeTokens, text: impl Into<String>) -> Self {
        Self {
            tokens,
            text: text.into(),
            level: LabelLevel::Primary,
            size: LabelSize::Body,
        }
    }

    /// Set the text level (color).
    pub fn level(mut self, level: LabelLevel) -> Self {
        self.level = level;
        self
    }

    /// Set the text size.
    pub fn size(mut self, size: LabelSize) -> Self {
        self.size = size;
        self
    }

    /// The color for this level.
    pub fn color(&self) -> Color32 {
        let c = &self.tokens.color;
        match self.level {
            LabelLevel::Primary => c.text_primary,
            LabelLevel::Secondary => c.text_secondary,
            LabelLevel::Weak => c.text_weak,
            LabelLevel::Disabled => c.text_disabled,
        }
    }

    /// The font size in points for this size.
    pub fn font_size(&self) -> f32 {
        let t = &self.tokens.typography;
        match self.size {
            LabelSize::Body => t.body,
            LabelSize::Caption => t.caption,
            LabelSize::Heading => t.heading,
        }
    }

    /// Show the label and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        ui.label(
            RichText::new(self.text.as_str())
                .size(self.font_size())
                .color(self.color()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Levels map to the color tokens.
    #[test]
    fn levels_map_to_color_tokens() {
        let tokens = ThemeTokens::dark();
        let c = &tokens.color;
        assert_eq!(Label::new(&tokens, "x").color(), c.text_primary);
        assert_eq!(
            Label::new(&tokens, "x").level(LabelLevel::Primary).color(),
            c.text_primary
        );
        assert_eq!(
            Label::new(&tokens, "x")
                .level(LabelLevel::Secondary)
                .color(),
            c.text_secondary
        );
        assert_eq!(
            Label::new(&tokens, "x").level(LabelLevel::Weak).color(),
            c.text_weak
        );
        assert_eq!(
            Label::new(&tokens, "x").level(LabelLevel::Disabled).color(),
            c.text_disabled
        );
    }

    /// Sizes map to the typography tokens.
    #[test]
    fn sizes_map_to_typography_tokens() {
        let tokens = ThemeTokens::dark();
        let t = &tokens.typography;
        assert_eq!(Label::new(&tokens, "x").font_size(), t.body);
        assert_eq!(
            Label::new(&tokens, "x")
                .size(LabelSize::Caption)
                .font_size(),
            t.caption
        );
        assert_eq!(
            Label::new(&tokens, "x")
                .size(LabelSize::Heading)
                .font_size(),
            t.heading
        );
    }

    /// The label text renders and is queryable.
    #[test]
    fn label_text_is_queryable() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Label::new(&tokens, "Hello")
                .size(LabelSize::Heading)
                .show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("Hello");
    }
}
