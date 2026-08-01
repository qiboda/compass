//! SearchableDropdown composite widget (design doc `.omo/designs/gui-upgrade.md` §5.2).
//!
//! Migrated from the binary crate's `StockPicker` (epic #119 S5) and enhanced
//! with ↑↓/Enter keyboard navigation and the「无匹配结果」empty state. The
//! widget is fully generic over the row type via a [`StockProjection`] — the
//! UI crate keeps zero business-crate dependencies (`compass-core` /
//! `compass-types` are never imported here; the binary adapts its own row
//! type through projection functions).

use crate::tokens::ThemeTokens;
use crate::widgets::input::Input;

/// Field projection for an arbitrary stock-list row type.
///
/// `SearchableDropdown` is generic over `T` and reads the three searchable /
/// displayable fields through these function pointers, so any business row
/// type (e.g. `compass_core::model::StockBasic`) can be shown without the UI
/// crate depending on the data crate.
#[derive(Clone, Copy)]
pub struct StockProjection<T> {
    symbol: for<'a> fn(&'a T) -> &'a str,
    name: for<'a> fn(&'a T) -> &'a str,
    exchange: for<'a> fn(&'a T) -> Option<&'a str>,
}

impl<T> StockProjection<T> {
    /// Create a projection from three accessor functions.
    pub fn new(
        symbol: for<'a> fn(&'a T) -> &'a str,
        name: for<'a> fn(&'a T) -> &'a str,
        exchange: for<'a> fn(&'a T) -> Option<&'a str>,
    ) -> Self {
        Self {
            symbol,
            name,
            exchange,
        }
    }

    /// Read the stock's symbol.
    pub fn symbol_of<'a>(&self, stock: &'a T) -> &'a str {
        (self.symbol)(stock)
    }

    /// Read the stock's display name.
    pub fn name_of<'a>(&self, stock: &'a T) -> &'a str {
        (self.name)(stock)
    }

    /// Read the stock's exchange code (`"SH"` / `"SZ"` / `"BJ"`).
    pub fn exchange_of<'a>(&self, stock: &'a T) -> Option<&'a str> {
        (self.exchange)(stock)
    }
}

/// Pure filter: symbol-prefix or name-substring match, optional exchange
/// filter, results sorted by symbol (design doc §5.2, migrated from the
/// binary crate's `filter_stocks`).
///
/// An empty query matches every stock in the (optionally exchange-filtered)
/// list. The exchange filter takes a two-letter code such as `"SH"`; `None`
/// matches all exchanges.
pub fn filter_stocks<'a, T>(
    stocks: &'a [T],
    query: &str,
    exchange: Option<&str>,
    projection: &StockProjection<T>,
) -> Vec<&'a T> {
    let lower = query.trim().to_lowercase();
    let mut result: Vec<&T> = stocks
        .iter()
        .filter(|s| {
            exchange
                .map(|ex| projection.exchange_of(s) == Some(ex))
                .unwrap_or(true)
        })
        .filter(|s| {
            if lower.is_empty() {
                return true;
            }
            projection.symbol_of(s).to_lowercase().starts_with(&lower)
                || projection.name_of(s).to_lowercase().contains(&lower)
        })
        .collect();
    result.sort_by(|a, b| projection.symbol_of(a).cmp(projection.symbol_of(b)));
    result
}

/// Searchable input with a filtered option popup and ↑↓/Enter keyboard
/// navigation (design doc §5.2).
///
/// Keeps the `StockPicker` data contract: `selected_symbol` / `selected_name`
/// / `selected_exchange` / `filter_text` / `popup_open` are public state
/// fields, and `show` renders an [`crate::widgets::input::Input`] with a
/// `bg_panel` popup (22 px rows, accent bar for the selected row, `bg_hover`
/// fill for hover/highlight,「无匹配结果」empty state). `Esc` closes the
/// popup.
pub struct SearchableDropdown<T> {
    tokens: ThemeTokens,
    projection: StockProjection<T>,
    /// Current filter query while the popup is open.
    pub filter_text: String,
    /// Currently selected symbol (e.g. `"000001"`).
    pub selected_symbol: String,
    /// Display name of the selected symbol.
    pub selected_name: String,
    /// Exchange code of the selected symbol (`"SH"` / `"SZ"` / `"BJ"` / empty).
    pub selected_exchange: String,
    /// Whether the option popup is open.
    pub popup_open: bool,
    /// Keyboard-highlighted row position within the filtered list (`None` = no highlight).
    pub highlighted: Option<usize>,
    cached_indices: Vec<usize>,
    last_filter_text: String,
}

/// Compatibility alias for the pre-migration name (`StockPicker`).
pub type StockPicker<T> = SearchableDropdown<T>;

impl<T> SearchableDropdown<T> {
    /// Create a dropdown with the given theme, initial symbol and field projection.
    ///
    /// The display name / exchange of `default_symbol` are resolved lazily on
    /// the first [`Self::show`] call against the supplied stock list.
    pub fn new(tokens: ThemeTokens, default_symbol: &str, projection: StockProjection<T>) -> Self {
        Self {
            tokens,
            projection,
            filter_text: String::new(),
            selected_symbol: default_symbol.to_string(),
            selected_name: String::new(),
            selected_exchange: String::new(),
            popup_open: false,
            highlighted: None,
            cached_indices: Vec::new(),
            last_filter_text: String::new(),
        }
    }

    /// Render the input and (when open) the option popup; returns the input response.
    pub fn show(&mut self, ui: &mut egui::Ui, stock_list: &[T]) -> egui::Response {
        let tokens = self.tokens;
        let c = &tokens.color;

        // Resolve the display name / exchange of the initial selection lazily
        // on the first render against the supplied stock list.
        if self.selected_name.is_empty()
            && let Some(stock) = stock_list
                .iter()
                .find(|s| self.projection.symbol_of(s) == self.selected_symbol)
        {
            self.selected_name = self.projection.name_of(stock).to_string();
            self.selected_exchange = self.projection.exchange_of(stock).unwrap_or("").to_string();
        }

        let display_text = format_display(
            &self.selected_exchange,
            &self.selected_symbol,
            &self.selected_name,
        );

        let response = if self.popup_open {
            Input::new(&tokens, &mut self.filter_text).show(ui)
        } else {
            let mut dummy = display_text.clone();
            let resp = Input::new(&tokens, &mut dummy).show(ui);
            if resp.clicked() {
                self.popup_open = true;
                self.filter_text.clone_from(&display_text);
                self.highlighted = None;
            }
            resp
        };

        if self.popup_open {
            // Esc closes the popup (existing behavior preserved).
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.popup_open = false;
                self.highlighted = None;
                return response;
            }

            let needs_refilter = self.filter_text != self.last_filter_text;
            if needs_refilter {
                self.refilter(stock_list);
            }
            let filtered_count = self.cached_indices.len();

            // ↑↓ keyboard navigation over the filtered list.
            if filtered_count > 0 {
                if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    self.highlighted = Some(match self.highlighted {
                        None => 0,
                        Some(h) => (h + 1) % filtered_count,
                    });
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.highlighted = Some(match self.highlighted {
                        None => filtered_count - 1,
                        Some(h) => (h + filtered_count - 1) % filtered_count,
                    });
                }
            }
            // Enter selects the highlighted row.
            let enter_pick = if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.highlighted
            } else {
                None
            };

            let max_rows = 12.min(filtered_count);
            let row_height = 22.0;
            let popup_height = 8.0 + max_rows as f32 * row_height;

            let mut clicked_idx: Option<usize> = None;

            egui::Area::new(ui.id().with("compass_searchable_dropdown_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(response.rect.left_bottom())
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    ui.set_min_width(320.0);
                    let frame = egui::Frame::new()
                        .fill(c.bg_panel)
                        .stroke(egui::Stroke::new(1.0, c.border))
                        .corner_radius(tokens.radius.md)
                        .shadow(tokens.shadow.popup)
                        .inner_margin(egui::Margin::symmetric(4, 4));
                    frame.show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(popup_height)
                            .show_rows(ui, row_height, filtered_count, |ui, range| {
                                // Row hover fill comes from the scoped widget style.
                                let previous_style = ui.style().clone();
                                let mut style = (*previous_style).clone();
                                style.visuals.widgets.hovered.weak_bg_fill = c.bg_hover;
                                style.visuals.widgets.hovered.corner_radius =
                                    egui::CornerRadius::from(tokens.radius.sm);
                                ui.set_style(style);

                                for (pos, &idx) in
                                    range.clone().zip(&self.cached_indices[range.clone()])
                                {
                                    let stock = &stock_list[idx];
                                    let symbol = self.projection.symbol_of(stock);
                                    let is_selected = symbol == self.selected_symbol;
                                    let is_highlighted = self.highlighted == Some(pos);
                                    let text = format!(
                                        "{} | {} | {}",
                                        self.projection.exchange_of(stock).unwrap_or(""),
                                        symbol,
                                        self.projection.name_of(stock)
                                    );
                                    let row = egui::Button::new(
                                        egui::RichText::new(&text)
                                            .color(if is_selected {
                                                c.accent
                                            } else {
                                                c.text_primary
                                            })
                                            .size(tokens.typography.body),
                                    )
                                    .fill(if is_highlighted {
                                        c.bg_hover
                                    } else if is_selected {
                                        c.selection_bg
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    })
                                    .stroke(egui::Stroke::NONE)
                                    .corner_radius(tokens.radius.sm)
                                    .min_size(egui::Vec2::new(312.0, row_height));
                                    let resp = ui.add(row);
                                    // Selected row: 2 px accent vertical bar.
                                    if is_selected {
                                        ui.painter().rect_filled(
                                            egui::Rect::from_min_size(
                                                resp.rect.min,
                                                egui::vec2(2.0, resp.rect.height()),
                                            ),
                                            0.0,
                                            c.accent,
                                        );
                                    }
                                    if resp.clicked() {
                                        clicked_idx = Some(idx);
                                    }
                                }
                                ui.set_style(previous_style);

                                // Empty filter result hint.
                                if filtered_count == 0 {
                                    ui.add_space(tokens.spacing.sm);
                                    ui.label(
                                        egui::RichText::new("无匹配结果")
                                            .color(c.text_weak)
                                            .size(tokens.typography.caption),
                                    );
                                }
                            });
                    });
                });

            // Apply the selection from a mouse click or Enter (outside the
            // popup borrow scope).
            if let Some(pos) = enter_pick
                && let Some(&idx) = self.cached_indices.get(pos)
            {
                clicked_idx = Some(idx);
            }
            if let Some(idx) = clicked_idx {
                self.select(idx, stock_list);
            }
        }
        response
    }

    /// Recompute `cached_indices` from the current filter text.
    fn refilter(&mut self, stock_list: &[T]) {
        let lower = self.filter_text.trim().to_lowercase();
        self.cached_indices = stock_list
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                if lower.is_empty() {
                    return true;
                }
                self.projection
                    .symbol_of(s)
                    .to_lowercase()
                    .starts_with(&lower)
                    || self.projection.name_of(s).to_lowercase().contains(&lower)
            })
            .map(|(i, _)| i)
            .collect();
        self.cached_indices.sort_by(|a, b| {
            self.projection
                .symbol_of(&stock_list[*a])
                .cmp(self.projection.symbol_of(&stock_list[*b]))
        });
        // A new filter resets the keyboard highlight.
        self.highlighted = None;
        self.last_filter_text.clone_from(&self.filter_text);
    }

    /// Apply a selection (by stock-list index) to the picker state.
    fn select(&mut self, stock_index: usize, stock_list: &[T]) {
        let stock = &stock_list[stock_index];
        self.selected_symbol = self.projection.symbol_of(stock).to_string();
        self.selected_name = self.projection.name_of(stock).to_string();
        self.selected_exchange = self.projection.exchange_of(stock).unwrap_or("").to_string();
        self.popup_open = false;
        self.filter_text.clear();
        self.highlighted = None;
    }
}

/// Format the closed-state display text: `exchange | symbol | name`, omitting
/// empty parts (migrated from the binary crate).
fn format_display(exchange: &str, symbol: &str, name: &str) -> String {
    if name.is_empty() {
        if exchange.is_empty() {
            symbol.to_string()
        } else {
            format!("{exchange} | {symbol}")
        }
    } else if exchange.is_empty() {
        format!("{symbol} | {name}")
    } else {
        format!("{exchange} | {symbol} | {name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use crate::widgets::input::Input;
    use egui_kittest::kittest::Queryable;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Minimal local row type — the UI crate must not depend on business crates.
    #[derive(Clone)]
    struct TestStock {
        symbol: String,
        name: String,
        exchange: Option<String>,
    }

    impl TestStock {
        fn new(symbol: &str, name: &str, exchange: &str) -> Self {
            Self {
                symbol: symbol.into(),
                name: name.into(),
                exchange: Some(exchange.into()),
            }
        }
    }

    fn test_projection() -> StockProjection<TestStock> {
        StockProjection::new(
            |s: &TestStock| &s.symbol,
            |s: &TestStock| &s.name,
            |s: &TestStock| s.exchange.as_deref(),
        )
    }

    fn make_stocks() -> Vec<TestStock> {
        vec![
            TestStock::new("000001", "平安银行", "SZ"),
            TestStock::new("000002", "万科A", "SZ"),
            TestStock::new("600519", "贵州茅台", "SH"),
            TestStock::new("600036", "招商银行", "SH"),
            TestStock::new("300750", "宁德时代", "SZ"),
        ]
    }

    fn harness_for_picker<'a>(
        picker: &Rc<RefCell<SearchableDropdown<TestStock>>>,
        stocks: &'a [TestStock],
    ) -> egui_kittest::Harness<'a> {
        let p = picker.clone();
        egui_kittest::Harness::new_ui(move |ui| {
            p.borrow_mut().show(ui, stocks);
        })
    }

    /// Helper: find the text input by its value and click it.
    fn click_text_input(harness: &egui_kittest::Harness<'_>, value: &str) {
        harness
            .get_all_by_value(value)
            .next()
            .expect("should find text input node")
            .click();
    }

    // --- format_display (migrated) ---

    #[test]
    fn format_display_full() {
        assert_eq!(
            format_display("SZ", "000001", "平安银行"),
            "SZ | 000001 | 平安银行"
        );
    }

    #[test]
    fn format_display_no_name() {
        assert_eq!(format_display("SZ", "000001", ""), "SZ | 000001");
    }

    #[test]
    fn format_display_no_exchange() {
        assert_eq!(
            format_display("", "000001", "平安银行"),
            "000001 | 平安银行"
        );
    }

    #[test]
    fn format_display_symbol_only() {
        assert_eq!(format_display("", "000001", ""), "000001");
    }

    // --- filter_stocks (migrated + exchange filter) ---

    #[test]
    fn filter_stocks_code_prefix_match() {
        let stocks = make_stocks();
        let result = filter_stocks(&stocks, "600", None, &test_projection());
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol, "600036");
        assert_eq!(result[1].symbol, "600519");
    }

    #[test]
    fn filter_stocks_name_substring_match() {
        let stocks = make_stocks();
        let result = filter_stocks(&stocks, "银行", None, &test_projection());
        assert_eq!(result.len(), 2);
        let found: Vec<_> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(found.contains(&"000001"));
        assert!(found.contains(&"600036"));
    }

    #[test]
    fn filter_stocks_exchange_filter_sh() {
        let stocks = make_stocks();
        let result = filter_stocks(&stocks, "", Some("SH"), &test_projection());
        assert_eq!(result.len(), 2);
        let symbols: Vec<_> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(symbols.contains(&"600519"));
        assert!(symbols.contains(&"600036"));
    }

    #[test]
    fn filter_stocks_exchange_filter_sz() {
        let stocks = make_stocks();
        let result = filter_stocks(&stocks, "", Some("SZ"), &test_projection());
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn filter_stocks_empty_query_returns_all_in_exchange() {
        let stocks = make_stocks();
        let result = filter_stocks(&stocks, "", None, &test_projection());
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn filter_stocks_unknown_exchange_returns_none() {
        let stocks = make_stocks();
        let result = filter_stocks(&stocks, "", Some("BJ"), &test_projection());
        assert!(result.is_empty());
    }

    // --- state (migrated) ---

    #[test]
    fn stock_picker_starts_with_empty_cache() {
        let picker = SearchableDropdown::new(ThemeTokens::dark(), "000001", test_projection());
        assert!(picker.cached_indices.is_empty());
    }

    #[test]
    fn stock_picker_detects_filter_change() {
        let mut picker = SearchableDropdown::new(ThemeTokens::dark(), "000001", test_projection());
        picker.filter_text = "平安".into();
        picker.popup_open = true;
        assert_ne!(picker.filter_text, picker.last_filter_text);
    }

    // --- kittest interaction (migrated) ---

    #[test]
    fn test_show_click_opens_popup() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        assert!(!picker.borrow().popup_open);
        // First render resolves the default symbol's name/exchange.
        assert_eq!(picker.borrow().selected_name, "平安银行");

        click_text_input(&harness, "SZ | 000001 | 平安银行");
        harness.run();

        assert!(
            picker.borrow().popup_open,
            "popup should open on text edit click"
        );
    }

    #[test]
    fn test_escape_closes_popup() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        click_text_input(&harness, "SZ | 000001 | 平安银行");
        harness.run();
        assert!(picker.borrow().popup_open);

        harness.key_press(egui::Key::Escape);
        harness.run();

        assert!(!picker.borrow().popup_open, "popup should close on Escape");
    }

    #[test]
    fn test_row_click_selects_stock() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "6".into();

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();
        assert!(picker.borrow().popup_open);

        harness.get_by_label("SH | 600519 | 贵州茅台").click();
        harness.run();

        assert_eq!(picker.borrow().selected_symbol, "600519");
        assert_eq!(picker.borrow().selected_name, "贵州茅台");
        assert_eq!(picker.borrow().selected_exchange, "SH");
        assert!(
            !picker.borrow().popup_open,
            "popup should close after selection"
        );
    }

    #[test]
    fn test_popup_repopulates_cached_indices() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "0".into();

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        assert!(
            !picker.borrow().cached_indices.is_empty(),
            "cached_indices should be populated when popup opens with filter"
        );
    }

    #[test]
    fn test_refilter_caching_last_filter_text_updated() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        assert!(picker.borrow().last_filter_text.is_empty());

        click_text_input(&harness, "SZ | 000001 | 平安银行");
        harness.run();
        harness.run();

        assert!(
            !picker.borrow().last_filter_text.is_empty(),
            "last_filter_text should be updated after popup open triggers refilter"
        );
    }

    // --- NEW: keyboard navigation (design §5.2) ---

    #[test]
    fn arrow_down_moves_highlight_wrapping() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "0".into(); // 2 matches: 000001, 000002

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        harness.key_press(egui::Key::ArrowDown);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(0));

        harness.key_press(egui::Key::ArrowDown);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(1));

        // Wraps back to the first row.
        harness.key_press(egui::Key::ArrowDown);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(0));
        harness.key_press(egui::Key::ArrowDown);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(1));
    }

    #[test]
    fn arrow_up_moves_highlight_backwards() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "0".into(); // 2 matches

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        // Up from no highlight lands on the last row.
        harness.key_press(egui::Key::ArrowUp);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(1));

        harness.key_press(egui::Key::ArrowUp);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(0));
    }

    #[test]
    fn enter_selects_highlighted_row_and_closes() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "6".into(); // sorted: 600036, 600519

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        harness.key_press(egui::Key::ArrowDown);
        harness.step();
        harness.key_press(egui::Key::ArrowDown);
        harness.step();
        assert_eq!(picker.borrow().highlighted, Some(1));

        harness.key_press(egui::Key::Enter);
        harness.step();

        assert_eq!(picker.borrow().selected_symbol, "600519");
        assert_eq!(picker.borrow().selected_exchange, "SH");
        assert!(
            !picker.borrow().popup_open,
            "Enter must close the popup after selecting"
        );
    }

    #[test]
    fn enter_without_highlight_does_nothing() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "6".into();

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        harness.key_press(egui::Key::Enter);
        harness.step();

        assert_eq!(picker.borrow().selected_symbol, "000001");
        assert!(
            picker.borrow().popup_open,
            "Enter without highlight is a no-op"
        );
    }

    // --- NEW: empty state (design §5.2) ---

    #[test]
    fn empty_filter_shows_no_match_hint() {
        let stocks = make_stocks();
        let picker = Rc::new(RefCell::new(SearchableDropdown::new(
            ThemeTokens::dark(),
            "000001",
            test_projection(),
        )));

        picker.borrow_mut().popup_open = true;
        picker.borrow_mut().filter_text = "zzzz".into();

        let mut harness = harness_for_picker(&picker, &stocks);
        harness.run();

        let _ = harness.get_by_label("无匹配结果");
    }
}
