//! Card atom: panel container with optional title row and collapsible body
//! (design doc §5.1 `Card`).

use crate::tokens::ThemeTokens;
use egui::{Margin, Response, RichText, Stroke, Ui};

/// Card content padding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CardPadding {
    /// Medium padding (12 px).
    Md,
    /// Large padding (16 px) — the design default.
    #[default]
    Lg,
}

/// Panel container with optional heading row and collapsible body.
pub struct Card<'a> {
    tokens: &'a ThemeTokens,
    title: Option<&'a str>,
    padding: CardPadding,
    bordered: bool,
    collapsible: bool,
}

impl<'a> Card<'a> {
    /// Create a card for the given theme.
    pub fn new(tokens: &'a ThemeTokens) -> Self {
        Self {
            tokens,
            title: None,
            padding: CardPadding::Lg,
            bordered: true,
            collapsible: false,
        }
    }

    /// Set the heading row title.
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Set the content padding (defaults to [`CardPadding::Lg`]).
    pub fn padding(mut self, padding: CardPadding) -> Self {
        self.padding = padding;
        self
    }

    /// Show or hide the 1 px border (default true).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Make the title row toggle the body visibility.
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.collapsible = collapsible;
        self
    }

    /// Show the card and run the body contents; returns the combined response.
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> Response {
        let tokens = self.tokens;
        let c = &tokens.color;

        let padding = match self.padding {
            CardPadding::Md => Margin::symmetric(12, 12),
            CardPadding::Lg => Margin::symmetric(16, 16),
        };

        let mut frame = egui::Frame::new()
            .fill(c.bg_panel)
            .corner_radius(tokens.radius.md)
            .inner_margin(padding);
        if self.bordered {
            frame = frame.stroke(Stroke::new(1.0, c.border));
        }

        let collapsible = self.collapsible;
        let title = self.title;
        frame
            .show(ui, |ui| {
                let mut open = true;
                if collapsible {
                    open = ui
                        .ctx()
                        .data(|d| d.get_temp::<bool>(ui.id().with("open")).unwrap_or(true));
                }
                if let Some(title) = title {
                    if collapsible {
                        // Toggle row.
                        let button = egui::Button::new(
                            RichText::new(format!(
                                "{title} {}",
                                if open {
                                    egui_phosphor::regular::CARET_UP
                                } else {
                                    egui_phosphor::regular::CARET_DOWN
                                }
                            ))
                            .size(tokens.typography.heading)
                            .strong()
                            .color(c.text_primary),
                        )
                        .fill(c.bg_panel_alt)
                        .stroke(Stroke::NONE)
                        .corner_radius(tokens.radius.sm);
                        if ui.add(button).clicked() {
                            open = !open;
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(ui.id().with("open"), open));
                        }
                    } else {
                        ui.label(
                            RichText::new(title)
                                .size(tokens.typography.heading)
                                .strong()
                                .color(c.text_primary),
                        );
                        ui.add_space(tokens.spacing.sm);
                    }
                }
                if open {
                    add_contents(ui);
                }
            })
            .response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// The card title renders and the body is shown by default.
    #[test]
    fn title_and_body_render() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Card::new(&tokens).title("Settings").show(ui, |ui| {
                ui.label("Content here");
            });
        });
        harness.run();
        let _ = harness.get_by_label("Settings");
        let _ = harness.get_by_label("Content here");
    }

    /// A collapsible card hides its body when the title row is clicked.
    #[test]
    fn collapsible_card_toggles_body() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Card::new(&tokens)
                .title("Filters")
                .collapsible(true)
                .show(ui, |ui| {
                    ui.label("Hidden body");
                });
        });
        harness.run();
        let _ = harness.get_by_label_contains("Filters");
        harness.get_by_label_contains("Filters").click();
        harness.run();
        // Body must be gone after collapsing.
        assert!(harness.query_by_label("Hidden body").is_none());
    }
}
