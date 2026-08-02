//! Segmented selector atom: equal-width segments in a panel-alt track
//! (design doc §5.1 `Segmented`).

use crate::tokens::ThemeTokens;
use egui::{Color32, CornerRadius, Margin, RichText, Stroke, StrokeKind, Ui, Vec2};

/// Segmented selector; returns the index of the clicked segment.
pub struct Segmented<'a> {
    tokens: &'a ThemeTokens,
    options: Vec<String>,
    selected: usize,
    height: f32,
}

impl<'a> Segmented<'a> {
    /// Create a segmented control with the given options.
    pub fn new(
        tokens: &'a ThemeTokens,
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            tokens,
            options: options.into_iter().map(Into::into).collect(),
            selected: 0,
            height: tokens.spacing.control_md,
        }
    }

    /// Set the initially selected index.
    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// Override the control height (default control-md).
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Show the segmented control; returns `Some(index)` of the clicked segment.
    pub fn show(self, ui: &mut Ui) -> Option<usize> {
        let tokens = self.tokens;
        let c = &tokens.color;
        let mut clicked = None;

        let previous_style = ui.style().clone();
        let mut style = (*previous_style).clone();
        style.visuals.widgets.hovered.weak_bg_fill = c.bg_hover;
        style.visuals.widgets.hovered.corner_radius = CornerRadius::from(tokens.radius.sm);
        style.visuals.widgets.active.weak_bg_fill = c.bg_active;
        ui.set_style(style);

        let frame = egui::Frame::new()
            .fill(c.bg_panel_alt)
            .corner_radius(tokens.radius.sm)
            .inner_margin(Margin::symmetric(2, 2));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                for (index, option) in self.options.iter().enumerate() {
                    let is_selected = self.selected == index;
                    let button = egui::Button::new(
                        RichText::new(option.as_str())
                            .color(if is_selected {
                                c.accent
                            } else {
                                c.text_secondary
                            })
                            .size(tokens.typography.body),
                    )
                    .fill(if is_selected {
                        c.bg_panel
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(if is_selected {
                        Stroke::new(1.0, c.border)
                    } else {
                        Stroke::NONE
                    })
                    .corner_radius(tokens.radius.sm)
                    .min_size(Vec2::new(0.0, self.height - 4.0));
                    if ui.add(button).clicked() {
                        clicked = Some(index);
                    }
                }
            });
        });
        ui.set_style(previous_style);

        let _ = StrokeKind::Inside; // keep stroke-kind import stable for future use
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Clicking a segment reports its index.
    #[test]
    fn clicking_segment_reports_index() {
        use std::cell::Cell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let last = Rc::new(Cell::new(None));
        let l = last.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if let Some(idx) = Segmented::new(&tokens, ["1d", "1w", "1M"])
                .selected(1)
                .show(ui)
            {
                l.set(Some(idx));
            }
        });
        harness.run();
        harness.get_by_label("1M").click();
        harness.run();
        assert_eq!(last.get(), Some(2), "clicking '1M' must report index 2");
    }

    /// All options render as queryable segments.
    #[test]
    fn all_options_render() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Segmented::new(&tokens, ["1d", "1w", "1M"]).show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("1d");
        let _ = harness.get_by_label("1w");
        let _ = harness.get_by_label("1M");
    }
}
