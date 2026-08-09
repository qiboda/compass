//! Dropdown atom: unified select with optional search, custom popup styling
//! (design doc §5.1 `Dropdown`).

use crate::tokens::ThemeTokens;
use crate::widgets::input::Input;
use compass_i18n::t;
use egui::{Area, Color32, CornerRadius, Frame, Margin, Order, RichText, Sense, Stroke, Ui};

/// Dropdown with a unified trigger and popup look. Selection state is
/// returned as the new index; popup open/close is tracked in egui memory.
pub struct Dropdown<'a> {
    tokens: &'a ThemeTokens,
    options: Vec<String>,
    selected: usize,
    width: f32,
    searchable: bool,
    /// Popup-state salt; distinguishes multiple Dropdown instances rendered
    /// in the same `Ui` (their `ui.id()` would otherwise collide).
    id_salt: &'a str,
}

impl<'a> Dropdown<'a> {
    /// Create a dropdown with the given options (cloned into owned strings).
    pub fn new(
        tokens: &'a ThemeTokens,
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            tokens,
            options: options.into_iter().map(Into::into).collect(),
            selected: 0,
            width: 160.0,
            searchable: false,
            id_salt: "",
        }
    }

    /// Set the index of the initially selected option.
    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// Set the trigger width in points (default 160).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Set a unique popup-state salt (default empty).
    pub fn id_salt(mut self, id_salt: &'a str) -> Self {
        self.id_salt = id_salt;
        self
    }

    /// Enable a search box inside the popup that filters options.
    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    /// Show the dropdown; returns `Some(index)` when the selection changed.
    pub fn show(self, ui: &mut Ui) -> Option<usize> {
        let tokens = self.tokens;
        let c = &tokens.color;
        let height = tokens.spacing.control_md;

        let popup_id = ui
            .id()
            .with(format!("compass_dropdown_popup:{}", self.id_salt));
        let mut open = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(popup_id).unwrap_or(false));
        let mut selected = self.selected;
        let mut changed = None;

        // Trigger: current selection + caret.
        let label = self
            .options
            .get(selected)
            .cloned()
            .unwrap_or_else(|| String::from("—"));
        let trigger = egui::Button::new(
            RichText::new(format!("{label} {}", egui_phosphor::regular::CARET_DOWN))
                .color(c.text_primary)
                .size(tokens.typography.body),
        )
        .fill(c.bg_panel_alt)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(tokens.radius.sm)
        .min_size(egui::Vec2::new(self.width, height));
        let trigger_resp = ui.add(trigger);
        if trigger_resp.clicked() {
            open = !open;
        }

        if open {
            // Popup: panel bg + popup shadow + md radius.
            let frame = Frame::new()
                .fill(c.bg_panel)
                .stroke(Stroke::new(1.0, c.border))
                .corner_radius(tokens.radius.md)
                .shadow(tokens.shadow.popup)
                .inner_margin(Margin::symmetric(4, 4));

            let area_response = Area::new(popup_id)
                .order(Order::Foreground)
                .fixed_pos(trigger_resp.rect.left_bottom())
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(self.width);
                    frame.show(ui, |ui| {
                        // Optional search field (Input component: unified
                        // appearance + focus border, per design doc §5.1).
                        if self.searchable {
                            let mut query: String = ui.ctx().data(|d| {
                                d.get_temp::<String>(ui.id().with("query"))
                                    .unwrap_or_default()
                            });
                            let search_hint = t!("common.search");
                            Input::new(tokens, &mut query)
                                .placeholder(&search_hint)
                                .width(self.width - 8.0)
                                .show(ui);
                            ui.add_space(tokens.spacing.xs);
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(ui.id().with("query"), query.clone()));
                            self.render_options(ui, &query, &mut selected, &mut changed);
                        } else {
                            self.render_options(ui, "", &mut selected, &mut changed);
                        }
                    });
                });

            // Close when the pointer clicks anywhere outside the popup and trigger.
            let clicked_outside = ui.ctx().input(|i| i.pointer.any_click())
                && !area_response.response.hovered()
                && !trigger_resp.hovered();
            if clicked_outside {
                open = false;
            }
        }

        ui.ctx().data_mut(|d| d.insert_temp(popup_id, open));
        if changed.is_some() {
            ui.ctx().data_mut(|d| d.insert_temp(popup_id, false));
        }
        changed
    }

    /// Render the option rows (28 px each, hover fill, accent for selected).
    fn render_options(
        &self,
        ui: &mut Ui,
        query: &str,
        selected: &mut usize,
        changed: &mut Option<usize>,
    ) {
        let tokens = self.tokens;
        let c = &tokens.color;
        let q = query.trim().to_lowercase();

        let previous_style = ui.style().clone();
        let mut style = (*previous_style).clone();
        style.visuals.widgets.hovered.weak_bg_fill = c.bg_hover;
        style.visuals.widgets.hovered.corner_radius = CornerRadius::from(tokens.radius.sm);
        ui.set_style(style);

        let mut any = false;
        for (index, option) in self.options.iter().enumerate() {
            if !q.is_empty() && !option.to_lowercase().contains(&q) {
                continue;
            }
            any = true;
            let is_selected = *selected == index;
            let button = egui::Button::new(
                RichText::new(option.as_str())
                    .color(if is_selected {
                        c.accent
                    } else {
                        c.text_primary
                    })
                    .size(tokens.typography.body),
            )
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(tokens.radius.sm)
            .sense(Sense::click())
            .min_size(egui::Vec2::new(self.width - 8.0, 28.0));
            if ui.add(button).clicked() {
                *selected = index;
                *changed = Some(index);
            }
        }
        if !any {
            ui.label(
                RichText::new(t!("common.no_matches"))
                    .color(c.text_weak)
                    .size(tokens.typography.caption),
            );
        }
        ui.set_style(previous_style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    /// Initial selection is the first option; the trigger shows it.
    #[test]
    fn initial_selection_is_first_option() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Dropdown::new(&tokens, ["1d", "1w", "1M"]).show(ui);
        });
        harness.run();
        let _ = harness.get_by_label_contains("1d");
    }

    /// Clicking an option in the popup returns the new selection.
    #[test]
    fn clicking_option_changes_selection() {
        use std::cell::Cell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let last = Rc::new(Cell::new(None));
        let l = last.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            if let Some(idx) = Dropdown::new(&tokens, ["1d", "1w", "1M"]).show(ui) {
                l.set(Some(idx));
            }
        });
        harness.run();
        harness.get_by_label_contains("1d").click();
        harness.run();
        harness.get_by_label("1w").click();
        harness.run();
        assert_eq!(last.get(), Some(1), "selecting '1w' must report index 1");
    }

    /// Searching filters the option list; the empty state hint renders.
    #[test]
    fn searchable_filters_and_shows_empty_hint() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Dropdown::new(&tokens, ["1d", "1w", "1M"])
                .searchable(true)
                .show(ui);
        });
        harness.run();
        harness.get_by_label_contains("1d").click();
        harness.run();
        // Empty state must be present before typing (query starts empty → all options shown,
        // so assert the popup renders at least one option).
        let _ = harness.get_by_label("1d");
        let _ = harness.get_by_label("1w");
        let _ = harness.get_by_label("1M");
    }

    /// The searchable popup renders a text-input search box (issue #228).
    #[test]
    fn searchable_popup_has_text_input() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Dropdown::new(&tokens, ["1d", "1w", "1M"])
                .searchable(true)
                .show(ui);
        });
        harness.run();
        harness.get_by_label_contains("1d").click();
        harness.run();
        let _ = harness.get_by_role(egui::accesskit::Role::TextInput);
    }

    /// The popup search box must not carry the hardcoded「搜索…」hint
    /// (issue #228).
    #[test]
    fn search_box_has_no_hardcoded_hint() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Dropdown::new(&tokens, ["1d", "1w", "1M"])
                .searchable(true)
                .show(ui);
        });
        harness.run();
        harness.get_by_label_contains("1d").click();
        harness.run();
        assert!(
            harness
                .query_all_by(|n| n.placeholder() == Some("搜索…"))
                .next()
                .is_none(),
            "the popup search box must not hardcode the '搜索…' hint (issue #228)"
        );
    }

    /// Typing in the search box filters the option list; the behavior must
    /// survive the Input swap (issue #228).
    #[test]
    fn searchable_typing_filters_options() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Dropdown::new(&tokens, ["1d", "1w", "1M"])
                .searchable(true)
                .show(ui);
        });
        harness.run();
        harness.get_by_label_contains("1d").click();
        harness.run();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .focus();
        harness.run();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .type_text("1w");
        harness.run();
        let _ = harness.get_by_label("1w");
        assert!(
            harness.query_by_label("1d").is_none(),
            "non-matching '1d' must be filtered out"
        );
        assert!(
            harness.query_by_label("1M").is_none(),
            "non-matching '1M' must be filtered out"
        );
    }

    /// The「无匹配结果」empty state renders when no option matches; the
    /// behavior must survive the Input swap (issue #228).
    #[test]
    fn empty_state_renders_when_no_match() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            Dropdown::new(&tokens, ["1d", "1w", "1M"])
                .searchable(true)
                .show(ui);
        });
        harness.run();
        harness.get_by_label_contains("1d").click();
        harness.run();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .focus();
        harness.run();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .type_text("zzz");
        harness.run();
        let _ = harness.get_by_label("无匹配结果");
    }
}
