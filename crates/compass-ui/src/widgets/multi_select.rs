//! Multi-select composite: searchable checkbox dropdown with a summary
//! trigger and a confirm button (migrated from
//! `crates/compass/src/citizens/screener.rs:508-574`, sub-issue #128 / S6).
//!
//! The component owns its selection state; [`MultiSelect::show`] returns
//! whether the selection changed this frame.

use egui::{Align, Area, Layout, Margin, Order, RichText, ScrollArea, Stroke, Ui};

use crate::tokens::ThemeTokens;

use super::button::{Button, ButtonSize, ButtonVariant};
use super::checkbox::Checkbox;
use super::input::Input;

/// Phosphor caret-down icon used on the summary trigger.
const CARET_DOWN: &str = egui_phosphor::regular::CARET_DOWN;
/// Popup minimum width (design doc §5.2).
const POPUP_MIN_WIDTH: f32 = 220.0;
/// Popup option list max height.
const POPUP_MAX_HEIGHT: f32 = 180.0;

/// Multi-select dropdown: summary trigger + searchable checkbox popup.
///
/// State lives in the struct so callers can persist it across frames:
/// `options` / `selected` hold the data, `open` / `filter` the transient
/// popup state.
pub struct MultiSelect {
    /// All selectable options, in display order.
    pub options: Vec<String>,
    /// Currently selected options.
    pub selected: Vec<String>,
    /// Whether the popup is open.
    pub open: bool,
    /// Live search filter text inside the popup.
    pub filter: String,
    /// Optional id salt so several instances can live in one `Ui`.
    id_salt: String,
    tokens: ThemeTokens,
}

impl MultiSelect {
    /// Create a multi-select for the given theme with the given options.
    pub fn new(tokens: &ThemeTokens, options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            selected: Vec::new(),
            open: false,
            filter: String::new(),
            id_salt: String::new(),
            tokens: *tokens,
        }
    }

    /// Preselect the given options.
    pub fn selected(mut self, selected: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.selected = selected.into_iter().map(Into::into).collect();
        self
    }

    /// Update the theme tokens after a theme switch without losing the
    /// current selection.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
    }

    /// Set an id salt so multiple instances can coexist in one `Ui`.
    pub fn id_salt(mut self, id_salt: impl Into<String>) -> Self {
        self.id_salt = id_salt.into();
        self
    }

    /// The summary text of the trigger: `全部` when nothing is selected,
    /// `已选 N 个` otherwise.
    pub fn summary(&self) -> String {
        if self.selected.is_empty() {
            "全部".to_string()
        } else {
            format!("已选 {} 个", self.selected.len())
        }
    }

    /// Toggle an option in the selection; returns whether it changed.
    pub fn toggle(&mut self, option: &str) -> bool {
        if let Some(pos) = self.selected.iter().position(|s| s == option) {
            self.selected.remove(pos);
        } else {
            self.selected.push(option.to_string());
        }
        true
    }

    /// Show the multi-select; returns whether the selection changed.
    pub fn show(&mut self, ui: &mut Ui) -> bool {
        let tokens = &self.tokens;
        let c = &tokens.color;
        let popup_id = ui.id().with("multi_select_popup").with(&self.id_salt);
        let mut changed = false;

        // Trigger: summary + caret.
        let trigger = egui::Button::new(
            RichText::new(format!("{} {}", self.summary(), CARET_DOWN))
                .color(c.text_primary)
                .size(tokens.typography.body),
        )
        .fill(c.bg_panel_alt)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(tokens.radius.sm)
        .min_size(egui::Vec2::new(0.0, tokens.spacing.control_md));
        let trigger_resp = ui.add(trigger);
        if trigger_resp.clicked() {
            self.open = !self.open;
        }

        if self.open {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.open = false;
            } else {
                let frame = egui::Frame::new()
                    .fill(c.bg_panel)
                    .stroke(Stroke::new(1.0, c.border))
                    .corner_radius(tokens.radius.md)
                    .shadow(tokens.shadow.popup)
                    .inner_margin(Margin::symmetric(8, 8));

                Area::new(popup_id)
                    .order(Order::Foreground)
                    .fixed_pos(trigger_resp.rect.left_bottom())
                    .constrain(true)
                    .show(ui.ctx(), |ui| {
                        ui.set_min_width(POPUP_MIN_WIDTH);
                        frame.show(ui, |ui| {
                            ScrollArea::vertical()
                                .max_height(POPUP_MAX_HEIGHT)
                                .show(ui, |ui| {
                                    changed |= self.popup_content(ui);
                                });
                        });
                    });
            }
        }

        changed
    }

    /// Render the popup body (search field, filtered checkbox rows, confirm
    /// button) into any `Ui`. Extracted from the floating [`Area`] so the
    /// interactions stay unit-testable (egui_kittest cannot simulate clicks
    /// inside `Area` / `ScrollArea` layers).
    fn popup_content(&mut self, ui: &mut Ui) -> bool {
        let tokens = &self.tokens;
        let mut changed = false;

        Input::new(tokens, &mut self.filter)
            .placeholder("搜索…")
            .width(POPUP_MIN_WIDTH - 16.0)
            .show(ui);
        ui.add_space(tokens.spacing.xs);
        let lower = self.filter.to_lowercase();

        for opt in self.options.iter() {
            if !lower.is_empty() && !opt.to_lowercase().contains(&lower) {
                continue;
            }
            let mut is_selected = self.selected.contains(opt);
            let resp = Checkbox::new(tokens, &mut is_selected, opt.clone()).show(ui);
            if resp.changed() {
                if is_selected {
                    self.selected.push(opt.clone());
                } else {
                    self.selected.retain(|s| s != opt);
                }
                changed = true;
            }
        }

        ui.add_space(tokens.spacing.sm);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if Button::new(tokens, "完成")
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Sm)
                .show(ui)
                .clicked()
            {
                self.open = false;
            }
        });

        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui::Key;
    use egui_kittest::kittest::Queryable;

    fn tokens() -> ThemeTokens {
        ThemeTokens::dark()
    }

    // ------------------------------------------------------------------
    // Pure logic
    // ------------------------------------------------------------------

    #[test]
    fn summary_shows_all_when_empty() {
        let ms = MultiSelect::new(&tokens(), ["a", "b", "c"]);
        assert_eq!(ms.summary(), "全部");
    }

    #[test]
    fn summary_counts_selected() {
        let ms = MultiSelect::new(&tokens(), ["a", "b", "c"]).selected(["a", "b"]);
        assert_eq!(ms.summary(), "已选 2 个");
    }

    #[test]
    fn toggle_adds_and_removes_options() {
        let mut ms = MultiSelect::new(&tokens(), ["a", "b"]);
        assert!(ms.toggle("a"), "first toggle adds");
        assert_eq!(ms.selected, ["a"]);
        assert!(ms.toggle("b"));
        assert_eq!(ms.selected, ["a", "b"]);
        assert!(ms.toggle("a"), "second toggle removes");
        assert_eq!(ms.selected, ["b"]);
    }

    // ------------------------------------------------------------------
    // Interaction (kittest)
    // ------------------------------------------------------------------

    #[test]
    fn clicking_rows_accumulates_selection_and_confirm_closes() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let ms = Rc::new(RefCell::new(MultiSelect::new(
            &tokens,
            ["银行", "白酒", "医药"],
        )));
        ms.borrow_mut().open = true; // popup_content assumes the popup is open
        let changed_seen = Rc::new(std::cell::Cell::new(false));
        let ms_c = ms.clone();
        let cs = changed_seen.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            cs.set(cs.get() | ms_c.borrow_mut().popup_content(ui));
        });
        harness.fit_contents();
        harness.step();

        // Check two options; the selection accumulates without closing.
        harness.get_by_label("银行").click();
        harness.step();
        harness.get_by_label("白酒").click();
        harness.step();
        assert!(
            ms.borrow().open,
            "popup content must not close on option clicks"
        );
        assert_eq!(ms.borrow().selected, ["银行", "白酒"]);
        assert!(changed_seen.get(), "selection change must be reported");

        // The confirm button closes the popup.
        harness.get_by_label("完成").click();
        harness.step();
        assert!(!ms.borrow().open, "confirm must close the popup");
    }

    #[test]
    fn trigger_opens_and_escape_closes_the_popup() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let ms = Rc::new(RefCell::new(MultiSelect::new(&tokens, ["银行", "白酒"])));
        let ms_c = ms.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            ms_c.borrow_mut().show(ui);
        });
        harness.fit_contents();
        harness.step();
        assert!(!ms.borrow().open);

        harness.get_by_label_contains("全部").click();
        harness.step();
        assert!(ms.borrow().open, "trigger click must open the popup");

        harness.key_down(Key::Escape);
        harness.key_up(Key::Escape);
        harness.step();
        assert!(!ms.borrow().open, "Escape must close the popup");
    }

    #[test]
    fn search_filter_hides_non_matching_options() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let ms = Rc::new(RefCell::new(MultiSelect::new(
            &tokens,
            ["银行", "白酒", "医药"],
        )));
        let ms_c = ms.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            ms_c.borrow_mut().popup_content(ui);
        });
        harness.fit_contents();
        harness.step();

        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .click();
        harness.step();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .type_text("银");
        harness.step();

        assert_eq!(ms.borrow().filter, "银");
        // The matching option remains reachable; the others are filtered out.
        let _ = harness.get_by_label("银行");
        assert!(
            harness.query_by_label("白酒").is_none(),
            "non-matching options must be hidden"
        );
    }

    #[test]
    fn distinct_id_salts_allow_two_popups_open_simultaneously() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let a = Rc::new(RefCell::new(
            MultiSelect::new(&tokens, ["银行"]).id_salt("a"),
        ));
        let b = Rc::new(RefCell::new(
            MultiSelect::new(&tokens, ["白酒"]).id_salt("b"),
        ));
        a.borrow_mut().open = true;
        b.borrow_mut().open = true;
        let ac = a.clone();
        let bc = b.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            ui.horizontal(|ui| {
                ac.borrow_mut().show(ui);
                bc.borrow_mut().show(ui);
            });
        });
        harness.fit_contents();
        harness.step();

        // Both popups are open with distinct Area ids — neither may shadow
        // the other. The screener bug: three MultiSelects shared the default
        // empty id_salt, so their popup Areas collided (only one rendered).
        // With distinct salts both option labels must be reachable.
        assert!(a.borrow().open, "first popup stays open");
        assert!(b.borrow().open, "second popup stays open");
        let _ = harness.get_by_label("银行");
        let _ = harness.get_by_label("白酒");
    }

    #[test]
    fn set_tokens_updates_theme_after_switch() {
        let dark = ThemeTokens::dark();
        let light = ThemeTokens::light();
        let mut ms = MultiSelect::new(&dark, ["SH", "SZ"]);
        ms.set_tokens(light);

        assert_eq!(
            ms.tokens, light,
            "after set_tokens the multi-select must use the light palette"
        );
    }
}
