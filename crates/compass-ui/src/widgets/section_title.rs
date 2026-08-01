//! Section title atom: heading + optional count + right-aligned action
//! (design doc §5.1 `SectionTitle`).

use crate::tokens::ThemeTokens;
use egui::{Align, Layout, RichText, Ui};

use super::icon_button::IconButton;

/// Panel heading row with an optional count and action icon button.
pub struct SectionTitle<'a> {
    tokens: &'a ThemeTokens,
    text: &'a str,
    count: Option<usize>,
    action: Option<IconButton<'a>>,
}

impl<'a> SectionTitle<'a> {
    /// Create a section title with the given heading text.
    pub fn new(tokens: &'a ThemeTokens, text: &'a str) -> Self {
        Self {
            tokens,
            text,
            count: None,
            action: None,
        }
    }

    /// Show a secondary-text count next to the heading.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Attach an icon button, right-aligned on the same row.
    pub fn action(mut self, action: IconButton<'a>) -> Self {
        self.action = Some(action);
        self
    }

    /// Show the section title row; returns `Some(true)` when the action was clicked.
    pub fn show(self, ui: &mut Ui) -> Option<bool> {
        let tokens = self.tokens;
        let c = &tokens.color;
        let mut action_clicked: Option<bool> = None;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.text)
                    .size(tokens.typography.heading)
                    .strong()
                    .color(c.text_primary),
            );
            if let Some(count) = self.count {
                ui.label(
                    RichText::new(count.to_string())
                        .size(tokens.typography.caption)
                        .color(c.text_secondary),
                );
            }
            // Right-align the action on the same row.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(action) = self.action {
                    action_clicked = Some(action.show(ui));
                }
            });
        });

        action_clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Heading and count render as queryable labels.
    #[test]
    fn heading_and_count_render() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            SectionTitle::new(&tokens, "Watchlist").count(3).show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("Watchlist");
        let _ = harness.get_by_label("3");
    }

    /// The action icon button is clickable.
    #[test]
    fn action_is_clickable() {
        use std::cell::Cell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let action = IconButton::new(&tokens, "\u{F1150}").tooltip("Add");
            if let Some(true) = SectionTitle::new(&tokens, "Watchlist")
                .action(action)
                .show(ui)
            {
                c.set(true);
            }
        });
        harness.run();
        harness.get_by_label("\u{F1150}").click();
        harness.run();
        assert!(clicked.get(), "section-title action must be clickable");
    }
}
