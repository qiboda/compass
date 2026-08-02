//! Divider atom: horizontal / vertical 1 px separator (design doc §5.1
//! `Divider`).

use crate::tokens::ThemeTokens;
use egui::{Response, Sense, Stroke, Ui};

/// 1 px divider line, horizontal by default.
pub struct Divider<'a> {
    tokens: &'a ThemeTokens,
    vertical: bool,
    strong: bool,
}

impl<'a> Divider<'a> {
    /// Create a horizontal divider for the given theme.
    pub fn new(tokens: &'a ThemeTokens) -> Self {
        Self {
            tokens,
            vertical: false,
            strong: false,
        }
    }

    /// Render a vertical divider (fits a horizontal parent layout).
    pub fn vertical(mut self, vertical: bool) -> Self {
        self.vertical = vertical;
        self
    }

    /// Use the stronger border color.
    pub fn strong(mut self, strong: bool) -> Self {
        self.strong = strong;
        self
    }

    /// The stroke used for this divider.
    pub fn stroke(&self) -> Stroke {
        Stroke::new(
            1.0,
            if self.strong {
                self.tokens.color.border_strong
            } else {
                self.tokens.color.border
            },
        )
    }

    /// Show the divider and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let stroke = self.stroke();
        if self.vertical {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(1.0, ui.available_height()), Sense::hover());
            ui.painter().vline(rect.center().x, rect.y_range(), stroke);
            response
        } else {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().hline(rect.x_range(), rect.center().y, stroke);
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;

    /// Regular dividers use the border color; strong ones the strong border.
    #[test]
    fn stroke_follows_strength() {
        let tokens = ThemeTokens::dark();
        assert_eq!(Divider::new(&tokens).stroke().color, tokens.color.border);
        assert_eq!(
            Divider::new(&tokens).strong(true).stroke().color,
            tokens.color.border_strong
        );
    }

    /// Both orientations render without panic.
    #[test]
    fn both_orientations_render() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            ui.horizontal(|ui| {
                Divider::new(&tokens).vertical(true).strong(true).show(ui);
            });
            Divider::new(&tokens).show(ui);
        });
        harness.run();
    }
}
