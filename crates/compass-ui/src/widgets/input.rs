//! Text input atom with unified appearance: prefix/suffix icons and an
//! optional monospace font for codes/prices (design doc §5.1 `Input`).

use crate::tokens::ThemeTokens;
use egui::{
    Color32, CornerRadius, FontId, FontSelection, Response, RichText, Stroke, StrokeKind, TextEdit,
    Ui, Vec2,
};

/// Text input with unified compass styling.
pub struct Input<'a> {
    tokens: &'a ThemeTokens,
    value: &'a mut String,
    placeholder: Option<&'a str>,
    prefix_icon: Option<&'a str>,
    suffix_icon: Option<&'a str>,
    monospace: bool,
    width: f32,
}

impl<'a> Input<'a> {
    /// Create an input bound to the given value buffer.
    pub fn new(tokens: &'a ThemeTokens, value: &'a mut String) -> Self {
        Self {
            tokens,
            value,
            placeholder: None,
            prefix_icon: None,
            suffix_icon: None,
            monospace: false,
            width: 220.0,
        }
    }

    /// Set the placeholder (hint) text.
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = Some(placeholder);
        self
    }

    /// Set a leading phosphor icon character.
    pub fn prefix_icon(mut self, icon: &'a str) -> Self {
        self.prefix_icon = Some(icon);
        self
    }

    /// Set a trailing phosphor icon character.
    pub fn suffix_icon(mut self, icon: &'a str) -> Self {
        self.suffix_icon = Some(icon);
        self
    }

    /// Use the monospace font (codes / prices / times).
    pub fn monospace(mut self, monospace: bool) -> Self {
        self.monospace = monospace;
        self
    }

    /// Set the desired width in points (default 220).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Show the input and return its response.
    pub fn show(self, ui: &mut Ui) -> Response {
        let tokens = self.tokens;
        let c = &tokens.color;
        let height = tokens.spacing.control_md;

        let placeholder = self.placeholder.unwrap_or("");
        let hint = RichText::new(placeholder).color(c.text_weak);

        // Reserve horizontal space for the prefix/suffix icon slots only when
        // icons are actually present; icon-less inputs (e.g. the Dropdown
        // popup search box) otherwise render ~48 px narrower than `width`.
        let icon_budget = if self.prefix_icon.is_some() || self.suffix_icon.is_some() {
            56.0
        } else {
            8.0 // frame inner margin only (4 px each side)
        };
        let field_width = self.width - icon_budget;

        // Draw prefix/suffix icons beside the field inside the same border.
        let mut text_edit = TextEdit::singleline(self.value)
            .id_salt("compass_input")
            .hint_text(hint)
            .background_color(Color32::TRANSPARENT)
            .text_color(c.text_primary)
            .margin(egui::Margin::symmetric(8, 4))
            .desired_width(field_width)
            .min_size(Vec2::new(field_width, height - 8.0));
        if self.monospace {
            text_edit = text_edit.font(FontSelection::FontId(FontId::monospace(
                tokens.typography.mono,
            )));
        }

        let frame = egui::Frame::new()
            .fill(c.bg_panel_alt)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(tokens.radius.sm)
            .inner_margin(egui::Margin::symmetric(4, 2));

        let response = frame
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(icon) = self.prefix_icon {
                        ui.label(RichText::new(icon).size(14.0).color(c.text_secondary));
                    }
                    let resp = ui.add(text_edit);
                    if let Some(icon) = self.suffix_icon {
                        ui.label(RichText::new(icon).size(14.0).color(c.text_secondary));
                    }
                    resp
                })
                .inner
            })
            .inner;

        // Focus border: accent 1.5px stroke around the whole field.
        if response.has_focus() {
            ui.painter().rect_stroke(
                response.rect,
                CornerRadius::from(tokens.radius.sm),
                Stroke::new(1.5, c.accent),
                StrokeKind::Inside,
            );
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Placeholder is rendered as the input's hint (accesskit placeholder).
    #[test]
    fn placeholder_is_rendered() {
        let tokens = ThemeTokens::dark();
        let mut value = String::new();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Input::new(&tokens, &mut value)
                .placeholder("Search symbol")
                .show(ui);
        });
        harness.run();
        let _ = harness.get_by(|node| node.placeholder() == Some("Search symbol"));
    }

    /// Typing into the input updates the bound value.
    #[test]
    fn typing_updates_bound_value() {
        let tokens = ThemeTokens::dark();
        let value = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let v = value.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Input::new(&tokens, &mut v.borrow_mut()).show(ui);
        });
        harness.run();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .click();
        harness.run();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .type_text("a");
        harness.run();
        assert_eq!(value.borrow().as_str(), "a");
    }

    /// Icon-less inputs must not reserve the 56 px icon budget: the field
    /// renders at `width − 8` (frame inner margin only), not `width − 56`
    /// (issue #228 regression — the Dropdown popup search box was ~48 px
    /// narrower than the options below it).
    #[test]
    fn iconless_input_does_not_reserve_icon_budget() {
        let tokens = ThemeTokens::dark();
        let mut value = String::new();
        let rect = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::ZERO));
        let r = rect.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            r.set(Input::new(&tokens, &mut value).width(200.0).show(ui).rect);
        });
        harness.run();
        let width = rect.get().width();
        assert!(
            (width - 192.0).abs() <= 1.0,
            "icon-less Input width(200.0) must render the field at ≈192 px \
             (200 − 8 frame margin), got {width}"
        );
    }

    /// Inputs with an icon keep the reserved icon budget (prefix icon still
    /// occupies its slot beside the field).
    #[test]
    fn prefixed_input_reserves_icon_budget() {
        let tokens = ThemeTokens::dark();
        let mut value = String::new();
        let rect = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::ZERO));
        let r = rect.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            r.set(
                Input::new(&tokens, &mut value)
                    .prefix_icon("\u{E200}")
                    .width(200.0)
                    .show(ui)
                    .rect,
            );
        });
        harness.run();
        let width = rect.get().width();
        assert!(
            (width - 144.0).abs() <= 1.0,
            "prefixed Input width(200.0) must keep the 56 px icon budget \
             (field ≈144 px), got {width}"
        );
    }
}
