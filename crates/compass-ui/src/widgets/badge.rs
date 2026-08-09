//! Badge atom: numeric count pill (design doc §5.1 `Badge`).

use crate::tokens::ThemeTokens;
use egui::{Color32, Margin, Response, RichText, Ui};

/// Badge tone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeTone {
    /// Neutral panel-alt pill with secondary text.
    #[default]
    Neutral,
    /// Accent-filled pill with white text.
    Accent,
    /// Error-filled pill with white text.
    Error,
}

/// Numeric count badge: 16 px tall pill, min width 16 px.
pub struct Badge<'a> {
    tokens: &'a ThemeTokens,
    count: usize,
    tone: BadgeTone,
}

impl<'a> Badge<'a> {
    /// Create a badge showing `count`.
    pub fn new(tokens: &'a ThemeTokens, count: usize) -> Self {
        Self {
            tokens,
            count,
            tone: BadgeTone::Neutral,
        }
    }

    /// Set the tone (defaults to [`BadgeTone::Neutral`]).
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }

    /// (background, text) colors for this tone.
    pub fn colors(&self) -> (Color32, Color32) {
        let c = &self.tokens.color;
        match self.tone {
            BadgeTone::Neutral => (c.bg_panel_alt, c.text_secondary),
            BadgeTone::Accent => (c.accent, Color32::WHITE),
            BadgeTone::Error => (c.error, Color32::WHITE),
        }
    }

    /// Show the badge pill and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let tokens = self.tokens;
        let (bg, fg) = self.colors();
        let text = self.count.to_string();
        let frame = egui::Frame::new()
            .fill(bg)
            .corner_radius(tokens.radius.pill)
            .inner_margin(Margin::symmetric(4, 2));
        frame
            .show(ui, |ui| {
                // Min-width 16 px: single-digit counts would otherwise render
                // narrower than the design spec (issue #227).
                ui.set_min_width(16.0);
                ui.label(
                    RichText::new(text)
                        .size(tokens.typography.caption)
                        .color(fg),
                )
            })
            .response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Tone colors follow the design doc (error = red fill, white text).
    #[test]
    fn tone_colors_follow_design() {
        let tokens = ThemeTokens::dark();
        let neutral = Badge::new(&tokens, 3);
        assert_eq!(neutral.colors().0, tokens.color.bg_panel_alt);
        assert_eq!(neutral.colors().1, tokens.color.text_secondary);

        let accent = Badge::new(&tokens, 3).tone(BadgeTone::Accent);
        assert_eq!(accent.colors().0, tokens.color.accent);
        assert_eq!(accent.colors().1, Color32::WHITE);

        let error = Badge::new(&tokens, 3).tone(BadgeTone::Error);
        assert_eq!(error.colors().0, tokens.color.error);
        assert_eq!(error.colors().1, Color32::WHITE);
    }

    /// The count text renders and is queryable.
    #[test]
    fn count_is_queryable() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Badge::new(&tokens, 42).tone(BadgeTone::Error).show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("42");
    }

    /// Single-digit counts still meet the 16px min-width spec
    /// (issue #227).
    #[test]
    fn single_digit_badge_meets_min_width_spec() {
        let tokens = ThemeTokens::dark();
        let rect = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::ZERO));
        let rect_c = rect.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            rect_c.set(Badge::new(&tokens, 1).show(ui).rect);
        });
        harness.run();
        let width = rect.get().width();
        assert!(
            width >= 16.0,
            "badge pill must be at least 16px wide, got {width}"
        );
    }

    /// Badge is pure display: the rendered node stays a plain Label with no
    /// click/button semantics (issue #227).
    #[test]
    fn badge_is_pure_display_without_click_semantics() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Badge::new(&tokens, 42).show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("42");
        let _ = harness.get_by(|n| {
            n.role() == egui::accesskit::Role::Label && n.value() == Some("42".to_string())
        });
    }
}
