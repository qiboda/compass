//! Empty state atom: centered icon + title + description (+ optional action)
//! for panels without data (design doc §5.1 `EmptyState`).

use crate::tokens::ThemeTokens;
use egui::{Align, Layout, Response, RichText, Ui};

use super::button::Button;

/// Centered empty-state placeholder.
pub struct EmptyState<'a> {
    tokens: &'a ThemeTokens,
    icon: &'a str,
    title: &'a str,
    description: &'a str,
    action: Option<Button<'a>>,
}

impl<'a> EmptyState<'a> {
    /// Create an empty state with an icon and title.
    pub fn new(tokens: &'a ThemeTokens, icon: &'a str, title: &'a str) -> Self {
        Self {
            tokens,
            icon,
            title,
            description: "",
            action: None,
        }
    }

    /// Set the description text (caption, secondary color).
    pub fn description(mut self, description: &'a str) -> Self {
        self.description = description;
        self
    }

    /// Attach an optional action button rendered below the description.
    pub fn action(mut self, action: Button<'a>) -> Self {
        self.action = Some(action);
        self
    }

    /// Show the empty state; returns the action button response if any.
    pub fn show(self, ui: &mut Ui) -> Option<Response> {
        let tokens = self.tokens;
        let c = &tokens.color;
        let mut action_response = None;

        ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
            ui.add_space(tokens.spacing.xl);
            // 48 px icon, weak text.
            ui.label(RichText::new(self.icon).size(48.0).color(c.text_weak));
            ui.add_space(tokens.spacing.md);
            // Heading title.
            ui.label(
                RichText::new(self.title)
                    .size(tokens.typography.heading)
                    .strong()
                    .color(c.text_primary),
            );
            if !self.description.is_empty() {
                ui.add_space(tokens.spacing.sm);
                // Caption description, wrapped at a sensible width.
                ui.set_max_width(320.0);
                ui.label(
                    RichText::new(self.description)
                        .size(tokens.typography.caption)
                        .color(c.text_secondary),
                );
            }
            if let Some(button) = self.action {
                ui.add_space(tokens.spacing.lg);
                action_response = Some(button.show(ui));
            }
            ui.add_space(tokens.spacing.xl);
        });

        action_response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Title and description text are queryable.
    #[test]
    fn title_and_description_are_queryable() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            EmptyState::new(&tokens, "\u{F1B2B}", "No data")
                .description("Fetch a symbol to see the chart")
                .show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("No data");
        let _ = harness.get_by_label_contains("Fetch a symbol");
    }

    /// The action button renders and fires clicks.
    #[test]
    fn action_button_is_clickable() {
        use std::cell::Cell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            let action =
                Button::new(&tokens, "Retry").variant(super::super::button::ButtonVariant::Primary);
            if let Some(resp) = EmptyState::new(&tokens, "\u{F1B2B}", "No data")
                .action(action)
                .show(ui)
                && resp.clicked()
            {
                c.set(true);
            }
        });
        harness.run();
        harness.get_by_label("Retry").click();
        harness.run();
        assert!(clicked.get(), "empty-state action must be clickable");
    }
}
