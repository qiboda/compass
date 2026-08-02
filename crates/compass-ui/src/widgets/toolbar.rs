//! Toolbar composite: group toolbar container with strong dividers between
//! groups (design doc §5.2 `Toolbar`).
//!
//! A `Frame` with the panel-alt fill and a bottom border, `toolbar_h` tall.
//! The first group is rendered without a separator; every following group is
//! preceded by a strong vertical divider with `spacing_lg` on both sides.

use egui::{Align, Layout, Stroke, Ui};

use crate::tokens::ThemeTokens;

use super::divider::Divider;

/// Group toolbar container.
#[derive(Clone)]
pub struct Toolbar<'a> {
    tokens: &'a ThemeTokens,
    group_count: usize,
}

impl<'a> Toolbar<'a> {
    /// Create a toolbar for the given theme.
    pub fn new(tokens: &'a ThemeTokens) -> Self {
        Self {
            tokens,
            group_count: 0,
        }
    }

    /// Show the toolbar, calling `add` with the toolbar and the toolbar row
    /// `Ui`; returns the closure's result.
    pub fn show<R>(mut self, ui: &mut Ui, add: impl FnOnce(&mut Self, &mut Ui) -> R) -> R {
        let tokens = self.tokens;
        let c = &tokens.color;

        let frame = egui::Frame::new().fill(c.bg_panel_alt);
        let frame_response = frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(tokens.spacing.toolbar_h);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = tokens.spacing.sm;
                add(&mut self, ui)
            })
            .inner
        });

        // Bottom border line.
        let rect = frame_response.response.rect;
        ui.painter()
            .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, c.border));

        frame_response.inner
    }

    /// Add one group; the first group gets no separator, later groups get a
    /// strong vertical divider with `spacing_lg` on both sides.
    pub fn group<R>(&mut self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        let tokens = self.tokens;
        if self.group_count > 0 {
            ui.add_space(tokens.spacing.lg);
            Divider::new(tokens).vertical(true).strong(true).show(ui);
            ui.add_space(tokens.spacing.lg);
        }
        self.group_count += 1;
        add(ui)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn group_count_increments_per_group() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let toolbar = Toolbar::new(&tokens);
        let counts = Rc::new(RefCell::new(Vec::new()));
        let c = counts.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            toolbar.clone().show(ui, |tb, ui| {
                tb.group(ui, |ui| {
                    ui.label("G1");
                });
                c.borrow_mut().push(tb.group_count);
                tb.group(ui, |ui| {
                    ui.label("G2");
                });
                c.borrow_mut().push(tb.group_count);
            });
        });
        harness.step();
        // The counter is 1 after the first group and 2 after the second, so
        // the separator is inserted before every group but the first. The
        // harness may run several frames per step; the counter resets each
        // frame because the toolbar is cloned per frame.
        let counts = counts.borrow();
        assert_eq!(&counts[..2], &[1, 2]);
        let _ = harness.get_by_label("G1");
        let _ = harness.get_by_label("G2");
    }

    #[test]
    fn show_renders_all_groups() {
        let tokens = ThemeTokens::dark();
        let toolbar = Toolbar::new(&tokens);
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            toolbar.clone().show(ui, |tb, ui| {
                tb.group(ui, |ui| {
                    let _ = ui.button("A");
                });
                tb.group(ui, |ui| {
                    let _ = ui.button("B");
                });
            });
        });
        harness.fit_contents();
        harness.run();
        let _ = harness.get_by_label("A");
        let _ = harness.get_by_label("B");
    }
}
