//! Sidebar composite: grouped watchlist list with search, add and per-row
//! delete (design doc §5.2 `Sidebar`).
//!
//! Pure UI — items and search text come from the caller; interactions are
//! returned as [`SidebarEvent`]s. Rows are 28 px tall with a hover fill,
//! a 2 px accent selection bar (animated via `animate_value_with_time`) and a
//! hover-only delete icon button.

use egui::{Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::tokens::ThemeTokens;

use super::empty_state::EmptyState;
use super::icon_button::IconButton;
use super::input::Input;
use super::section_title::SectionTitle;
use super::tag::{Tag, TagVariant};

/// Phosphor icons used by the sidebar.
const ICON_SEARCH: &str = "\u{E30C}";
const ICON_PLUS: &str = "\u{E3D4}";
const ICON_X: &str = "\u{E4F6}";
const ICON_STAR: &str = "\u{E46A}";
/// Row height (design doc §5.2: 28 px).
const ROW_HEIGHT: f32 = 28.0;

/// One watchlist row.
pub struct SidebarItem {
    /// Bare 6-digit symbol, e.g. `600519`.
    pub symbol: String,
    /// Display name, e.g. `贵州茅台`.
    pub name: String,
    /// Exchange code, e.g. `SH` / `SZ` / `BJ` (drives the `Tag` color).
    pub exchange: String,
    /// Whether this row is the currently selected one.
    pub selected: bool,
}

/// A titled group of sidebar rows.
pub struct SidebarGroup {
    /// Group title.
    pub title: String,
    /// Rows in display order.
    pub items: Vec<SidebarItem>,
}

/// User interaction produced by the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarEvent {
    /// A row was clicked.
    Select {
        /// The symbol of the clicked row.
        symbol: String,
    },
    /// The delete (×) button of a row was clicked.
    DeleteRequest {
        /// The symbol of the row whose delete button was clicked.
        symbol: String,
    },
    /// The search input text changed.
    Search(String),
    /// The add (＋) button was clicked.
    Add,
}

/// Grouped watchlist sidebar.
pub struct Sidebar<'a> {
    tokens: &'a ThemeTokens,
}

impl<'a> Sidebar<'a> {
    /// Create a sidebar for the given theme.
    pub fn new(tokens: &'a ThemeTokens) -> Self {
        Self { tokens }
    }

    /// Show the sidebar; returns the events produced this frame.
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        groups: &[SidebarGroup],
        search: &mut String,
    ) -> Vec<SidebarEvent> {
        let tokens = self.tokens;
        let mut events = Vec::new();

        ui.set_min_width(tokens.spacing.sidebar_w);

        // Search row: input + add button.
        ui.horizontal(|ui| {
            let search_resp = Input::new(tokens, search)
                .placeholder("搜索自选")
                .prefix_icon(ICON_SEARCH)
                .width(tokens.spacing.sidebar_w - 40.0)
                .show(ui);
            if search_resp.changed() {
                events.push(SidebarEvent::Search(search.clone()));
            }
            if IconButton::new(tokens, ICON_PLUS)
                .tooltip("添加")
                .small()
                .show(ui)
            {
                events.push(SidebarEvent::Add);
            }
        });
        ui.add_space(tokens.spacing.sm);

        let total: usize = groups.iter().map(|g| g.items.len()).sum();
        if total == 0 {
            EmptyState::new(tokens, ICON_STAR, "自选股为空")
                .description("点击 + 添加关注的股票")
                .show(ui);
            return events;
        }

        for group in groups {
            SectionTitle::new(tokens, &group.title)
                .count(group.items.len())
                .show(ui);
            ui.add_space(tokens.spacing.xs);
            for item in &group.items {
                events.extend(self.row(ui, item));
            }
            ui.add_space(tokens.spacing.md);
        }

        events
    }

    /// Render one 28 px row; returns the events it produced.
    fn row(&self, ui: &mut egui::Ui, item: &SidebarItem) -> Vec<SidebarEvent> {
        let tokens = self.tokens;
        let c = &tokens.color;
        let mut events = Vec::new();

        let id = ui.id().with(&item.symbol);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
        let hovering = response.hovered() || ui.ctx().rect_contains_pointer(ui.layer_id(), rect);

        // Hover fill (100 ms) and selection bar (120 ms) via the animation
        // manager; both are painted over the row rect.
        let hover = ui.ctx().animate_value_with_time(
            id.with("hover"),
            if hovering { 1.0 } else { 0.0 },
            tokens.motion.fast.as_secs_f32(),
        );
        let selected = ui.ctx().animate_value_with_time(
            id.with("selected"),
            if item.selected { 1.0 } else { 0.0 },
            0.12,
        );
        if hover > 0.0 {
            fill_rect(ui, rect, c.bg_hover, hover);
        }
        if selected > 0.0 {
            fill_rect(ui, rect, c.bg_hover, selected);
            ui.painter().vline(
                rect.left() + 1.0,
                rect.y_range(),
                Stroke::new(2.0, c.accent),
            );
        }

        let row_clicked = response.clicked();

        // Row content: name, monospace code, exchange tag, hover delete ×.
        // The name and symbol are made click-sensing themselves so a click on
        // the text registers even though the text widget sits above the row
        // interaction.
        let content_rect = Rect::from_min_max(
            Pos2::new(rect.left() + tokens.spacing.sm, rect.top()),
            Pos2::new(rect.right() - tokens.spacing.sm, rect.bottom()),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                let name_resp = ui
                    .label(
                        RichText::new(&item.name)
                            .size(tokens.typography.body)
                            .color(c.text_primary),
                    )
                    .interact(Sense::click());
                let symbol_resp = ui
                    .label(
                        RichText::new(&item.symbol)
                            .monospace()
                            .size(tokens.typography.caption)
                            .color(c.text_weak),
                    )
                    .interact(Sense::click());
                Tag::new(tokens, &item.exchange)
                    .variant(TagVariant::Exchange)
                    .show(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if (hovering || item.selected)
                        && IconButton::new(tokens, ICON_X)
                            .tooltip("删除")
                            .small()
                            .show(ui)
                    {
                        events.push(SidebarEvent::DeleteRequest {
                            symbol: item.symbol.clone(),
                        });
                    }
                });
                if row_clicked || name_resp.clicked() || symbol_resp.clicked() {
                    events.push(SidebarEvent::Select {
                        symbol: item.symbol.clone(),
                    });
                }
            },
        );

        events
    }
}

/// Fill a rect with a color scaled by `alpha` (0..=1).
fn fill_rect(ui: &mut egui::Ui, rect: Rect, color: Color32, alpha: f32) {
    let fill =
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (255.0 * alpha) as u8);
    ui.painter().rect_filled(rect, 0.0, fill);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    fn groups() -> Vec<SidebarGroup> {
        vec![
            SidebarGroup {
                title: "自选".to_string(),
                items: vec![
                    SidebarItem {
                        symbol: "600519".into(),
                        name: "贵州茅台".into(),
                        exchange: "SH".into(),
                        selected: false,
                    },
                    SidebarItem {
                        symbol: "000001".into(),
                        name: "平安银行".into(),
                        exchange: "SZ".into(),
                        selected: false,
                    },
                ],
            },
            SidebarGroup {
                title: "最近".to_string(),
                items: vec![SidebarItem {
                    symbol: "000002".into(),
                    name: "万科A".into(),
                    exchange: "SZ".into(),
                    selected: true,
                }],
            },
        ]
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    #[test]
    fn renders_groups_names_codes_and_tags() {
        let tokens = ThemeTokens::dark();
        let sidebar = Sidebar::new(&tokens);
        let mut search = String::new();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            sidebar.show(ui, &groups(), &mut search);
        });
        harness.run();
        let _ = harness.get_by_label("自选");
        let _ = harness.get_by_label("贵州茅台");
        let _ = harness.get_by_label_contains("600519");
        let _ = harness.get_by_label("SH");
    }

    #[test]
    fn empty_groups_show_empty_state() {
        let tokens = ThemeTokens::dark();
        let sidebar = Sidebar::new(&tokens);
        let mut search = String::new();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            sidebar.show(ui, &[], &mut search);
        });
        harness.run();
        let _ = harness.get_by_label("自选股为空");
    }

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    #[test]
    fn clicking_row_emits_select_event() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let sidebar = Sidebar::new(&tokens);
        let mut search = String::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let e = events.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            e.borrow_mut()
                .extend(sidebar.show(ui, &groups(), &mut search));
        });
        harness.fit_contents();
        harness.step();
        harness.get_by_label("贵州茅台").click();
        harness.step();
        assert_eq!(
            events.borrow().as_slice(),
            &[SidebarEvent::Select {
                symbol: "600519".into()
            }]
        );
    }

    #[test]
    fn hovering_row_reveals_delete_button_and_click_emits_delete_request() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let sidebar = Sidebar::new(&tokens);
        let mut search = String::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let e = events.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            e.borrow_mut()
                .extend(sidebar.show(ui, &groups(), &mut search));
        });
        harness.fit_contents();
        harness.step();
        harness.get_by_label("平安银行").hover();
        harness.step();
        // Both the hovered row and the selected row show a delete button; the
        // first in tree order belongs to the hovered row of the first group.
        let mut delete_buttons: Vec<_> = harness.query_all_by_label(ICON_X).collect();
        assert!(
            !delete_buttons.is_empty(),
            "hover must reveal the delete button"
        );
        delete_buttons.remove(0).click();
        harness.step();
        assert_eq!(
            events.borrow().as_slice(),
            &[SidebarEvent::DeleteRequest {
                symbol: "000001".into()
            }]
        );
    }

    #[test]
    fn typing_in_search_emits_search_event() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let sidebar = Sidebar::new(&tokens);
        let mut search = String::new();
        let events = Rc::new(RefCell::new(Vec::new()));
        let e = events.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            e.borrow_mut()
                .extend(sidebar.show(ui, &groups(), &mut search));
        });
        harness.fit_contents();
        harness.step();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .click();
        harness.step();
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .type_text("茅");
        harness.step();
        assert_eq!(
            events.borrow().as_slice(),
            &[SidebarEvent::Search("茅".into())]
        );
    }
}
