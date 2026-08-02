//! Checkbox atom with unified compass styling: accent check mark, disabled
//! state (design doc §5.1 `Checkbox`).

use crate::tokens::ThemeTokens;
use egui::{Response, RichText, Stroke, Ui};

/// Checkbox with unified appearance; the hit area follows egui's
/// interaction radius around the box.
pub struct Checkbox<'a> {
    tokens: &'a ThemeTokens,
    checked: &'a mut bool,
    label: String,
    disabled: bool,
}

impl<'a> Checkbox<'a> {
    /// Create a checkbox bound to the given boolean.
    pub fn new(tokens: &'a ThemeTokens, checked: &'a mut bool, label: impl Into<String>) -> Self {
        Self {
            tokens,
            checked,
            label: label.into(),
            disabled: false,
        }
    }

    /// Disable the checkbox (no clicks, disabled colors).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Show the checkbox and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let c = &self.tokens.color;

        // Scoped style: the check mark stroke is the accent when checked.
        let previous_style = ui.style().clone();
        let mut style = (*previous_style).clone();
        let w = &mut style.visuals.widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            state.fg_stroke = if *self.checked {
                Stroke::new(2.0, c.accent)
            } else {
                Stroke::new(1.0, c.text_weak)
            };
        }
        w.hovered.fg_stroke = if *self.checked {
            Stroke::new(2.0, c.accent_hover)
        } else {
            Stroke::new(1.0, c.text_secondary)
        };
        ui.set_style(style);

        let text = RichText::new(self.label.as_str()).color(if self.disabled {
            c.text_disabled
        } else {
            c.text_primary
        });

        let response = if self.disabled {
            ui.add_enabled(false, egui::Checkbox::new(self.checked, text))
        } else {
            ui.add(egui::Checkbox::new(self.checked, text))
        };
        ui.set_style(previous_style);
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Clicking the checkbox toggles the bound boolean.
    #[test]
    fn click_toggles_checked() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let checked = Rc::new(RefCell::new(false));
        let c = checked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Checkbox::new(&tokens, &mut c.borrow_mut(), "Enable").show(ui);
        });
        harness.run();
        harness.get_by_label("Enable").click();
        harness.run();
        assert!(*checked.borrow(), "click must check the box");
        harness.get_by_label("Enable").click();
        harness.run();
        assert!(!*checked.borrow(), "second click must uncheck the box");
    }

    /// A disabled checkbox does not toggle.
    #[test]
    fn disabled_checkbox_does_not_toggle() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let checked = Rc::new(RefCell::new(false));
        let c = checked.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Checkbox::new(&tokens, &mut c.borrow_mut(), "Locked")
                .disabled(true)
                .show(ui);
        });
        harness.run();
        harness.get_by_label("Locked").click();
        harness.run();
        assert!(!*checked.borrow(), "disabled checkbox must not toggle");
    }
}
