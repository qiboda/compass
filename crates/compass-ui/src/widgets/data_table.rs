//! Data table composite: sortable striped table with price coloring, empty
//! state and count label (abstracted from `crates/compass/src/citizens/screener.rs`
//! results area, sub-issue #128 / S6).
//!
//! The table renders `egui_extras::TableBuilder` rows from caller-provided
//! [`DataCell`]s. Sorting is owned by the component: the pure function
//! [`sort_rows`] migrates the screener semantics (text lexicographic / price
//! numeric / count numeric, ties broken by the first column). Row clicks
//! return the original row index.

use std::cmp::Ordering;

use crate::tokens::ThemeTokens;
use egui::{Align, Color32, Layout, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};

use super::empty_state::EmptyState;
use super::price_text::{self, PriceText};

/// Phosphor icon used for the empty state ("list").
const EMPTY_ICON: &str = "\u{E2F0}";
/// Header height (design doc §5.2: 22 px).
const HEADER_HEIGHT: f32 = 22.0;

/// One data cell of a table row.
///
/// The variant determines both the rendered widget and the sort semantics:
/// [`Self::Text`] sorts lexicographically, [`Self::Price`], [`Self::Count`],
/// [`Self::Score`] and [`Self::Rank`] numerically.
#[derive(Clone, Debug, PartialEq)]
pub enum DataCell {
    /// Plain text (left-aligned, lexicographic sort).
    Text(String),
    /// Price with an optional change percentage (rendered with [`PriceText`],
    /// red-up / green-down; numeric sort on the price value).
    Price {
        /// The price value used for numeric sorting.
        value: f32,
        /// Optional change percentage; `None` renders the price flat-colored.
        change: Option<f32>,
    },
    /// Count (numeric sort).
    Count(usize),
    /// Color-scale score value (SEPA panel): mono value colored via
    /// [`score_color`]; numeric sort on the value. With `inverted = true`
    /// (risk columns) the cell shows the signed deduction `-x.x` and the
    /// scale norm becomes `1 - |value|/max` — 0 deduction green, full
    /// deduction red.
    Score {
        /// The score value (also used for numeric sorting).
        value: f32,
        /// Maximum achievable value for the color-scale normalization.
        max: f32,
        /// Whether the color scale is inverted (risk semantics).
        inverted: bool,
    },
    /// Rank (SEPA panel): numeric sort; ranks 1–3 emphasized in warning.
    Rank(usize),
}

/// Column specification: header text + numeric alignment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Header label rendered in the 22 px header row.
    pub header: &'static str,
    /// Right-aligns cells and uses the monospace font for numeric columns.
    pub numeric: bool,
}

/// Sortable striped data table with row-hover, price coloring, an empty state
/// and a row count label.
///
/// Sorting state (`sort_column` / `sort_descending`) lives in the component
/// and is toggled by header clicks; callers only supply columns and rows.
/// The theme tokens are copied at construction (like [`super::multi_select::MultiSelect`])
/// so the table can outlive the frame that created it.
pub struct DataTable {
    tokens: ThemeTokens,
    columns: Vec<ColumnSpec>,
    rows: Vec<Vec<DataCell>>,
    sort_column: usize,
    sort_descending: bool,
    descending_defaults: std::collections::BTreeSet<usize>,
    /// Original row index highlighted with the selection color (details
    /// panel linkage); `None` highlights nothing.
    selected: Option<usize>,
}

impl DataTable {
    /// Create a table for the given theme with the given column specs.
    pub fn new(tokens: &ThemeTokens, columns: Vec<ColumnSpec>) -> Self {
        Self {
            tokens: *tokens,
            columns,
            rows: Vec::new(),
            sort_column: 0,
            sort_descending: false,
            descending_defaults: std::collections::BTreeSet::new(),
            selected: None,
        }
    }

    /// Replace the rows to display (call each frame with fresh data).
    pub fn set_rows(&mut self, rows: Vec<Vec<DataCell>>) {
        self.rows = rows;
    }

    /// Highlight the row with the given original index (details-panel
    /// linkage); `None` clears the highlight.
    pub fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    /// Update the theme tokens after a theme switch without resetting the
    /// current sort state or rows.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
    }

    /// Set the initial sort column and direction (e.g. a business default such as
    /// "market cap descending" for the screener).
    pub fn set_sort(&mut self, column: usize, descending: bool) {
        if column < self.columns.len() {
            self.sort_column = column;
            self.sort_descending = descending;
        }
    }

    /// Whether the current sort is descending.
    pub fn sort_descending(&self) -> bool {
        self.sort_descending
    }

    /// Columns that should default to descending when newly selected
    /// (business preference, e.g. market cap in the screener).
    pub fn set_descending_default(&mut self, column: usize, descending: bool) {
        if descending {
            self.descending_defaults.insert(column);
        } else {
            self.descending_defaults.remove(&column);
        }
    }

    /// Show the table; returns the original index of the row that was clicked.
    pub fn show(&mut self, ui: &mut Ui) -> Option<usize> {
        let tokens = self.tokens;
        let c = &tokens.color;
        let mut clicked_row = None;

        if self.rows.is_empty() {
            EmptyState::new(&tokens, EMPTY_ICON, "无符合条件").show(ui);
            return None;
        }

        // Row count label.
        ui.label(
            RichText::new(format!("共 {} 行", self.rows.len()))
                .size(tokens.typography.caption)
                .color(c.text_secondary),
        );

        let sorted = sort_rows(&self.rows, self.sort_column, self.sort_descending);
        let sort_column = self.sort_column;
        let sort_descending = self.sort_descending;
        let columns: Vec<ColumnSpec> = self.columns.clone();
        let n_columns = columns.len();

        // Scoped style: zebra stripes from the tokens, hover fill from bg_hover.
        let previous_style = ui.style().clone();
        let mut style = (*previous_style).clone();
        style.visuals.faint_bg_color = c.bg_panel_alt.gamma_multiply(0.5);
        style.visuals.widgets.hovered.bg_fill = c.bg_hover;
        ui.set_style(style);

        // The auto columns size to their content; wide rows (e.g. SEPA's
        // "行业 · 题材" cell) would widen the table's min_rect past its
        // container frame-to-frame and push neighboring panels off the pane
        // (SEPA detail panel: "右边内容一团乱"). A horizontal ScrollArea
        // absorbs the overflow so the table stays at its allocated width.
        let mut scroll = egui::ScrollArea::horizontal().auto_shrink([false, false]);
        scroll = scroll.id_salt(("data_table", n_columns));
        scroll.show(ui, |ui| {
            let mut table = TableBuilder::new(ui).striped(true).sense(Sense::click());
            for idx in 0..n_columns {
                let column = if idx + 1 == n_columns {
                    Column::remainder()
                } else {
                    Column::auto()
                };
                table = table.column(column);
            }
            table
                .header(HEADER_HEIGHT, |mut header| {
                    for (idx, col) in columns.iter().enumerate() {
                        let mut text = col.header.to_string();
                        if idx == sort_column {
                            text.push_str(if sort_descending { " ↓" } else { " ↑" });
                        }
                        header.col(|ui| {
                            let layout = if col.numeric {
                                Layout::right_to_left(Align::Center)
                            } else {
                                Layout::left_to_right(Align::Center)
                            };
                            ui.with_layout(layout, |ui| {
                                if ui
                                    .selectable_label(
                                        sort_column == idx,
                                        RichText::new(text.clone()).strong(),
                                    )
                                    .clicked()
                                {
                                    self.toggle_sort(idx);
                                }
                            });
                        });
                    }
                })
                .body(|mut body| {
                    for orig_index in sorted {
                        let is_selected = self.selected == Some(orig_index);
                        body.row(tokens.spacing.table_row_h, |mut row| {
                            let cells = &self.rows[orig_index];
                            for (idx, cell) in cells.iter().enumerate() {
                                let numeric = self.columns[idx].numeric;
                                row.col(|ui| {
                                    render_cell(ui, &tokens, cell, is_selected, numeric);
                                });
                            }
                            if row.response().clicked() {
                                clicked_row = Some(orig_index);
                            }
                        });
                    }
                });
        });

        ui.set_style(previous_style);

        clicked_row
    }

    /// Toggle sort state for a header column click.
    ///
    /// Clicking the active column flips direction; clicking a new column
    /// selects it ascending (generic default — the screener's market-cap
    /// special case is business logic, not component concern).
    fn toggle_sort(&mut self, column: usize) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = self.descending_defaults.contains(&column);
        }
    }
}

/// Sort row indices by the given column and direction, migrating the screener
/// semantics (`screener.rs:112-131`): text lexicographic, price/count numeric,
/// descending only reverses the primary ordering, ties are broken by the
/// first column (index 0) in ascending order. Stable for fully equal rows.
pub fn sort_rows(rows: &[Vec<DataCell>], column: usize, descending: bool) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..rows.len()).collect();
    indices.sort_by(|&a, &b| {
        let ord = compare_cells(&rows[a][column], &rows[b][column]);
        let ord = if descending { ord.reverse() } else { ord };
        ord.then_with(|| compare_cells(&rows[a][0], &rows[b][0]))
    });
    indices
}

/// Order two cells of the same column by the screener semantics.
fn compare_cells(a: &DataCell, b: &DataCell) -> Ordering {
    match (a, b) {
        (DataCell::Text(a), DataCell::Text(b)) => a.cmp(b),
        (DataCell::Price { value: a, .. }, DataCell::Price { value: b, .. }) => a.total_cmp(b),
        (DataCell::Count(a), DataCell::Count(b)) => a.cmp(b),
        (DataCell::Score { value: a, .. }, DataCell::Score { value: b, .. }) => a.total_cmp(b),
        (DataCell::Rank(a), DataCell::Rank(b)) => a.cmp(b),
        // Mixed types in one column are a caller error; keep the stable order.
        _ => Ordering::Equal,
    }
}

/// Render one cell with the widget matching its variant.
///
/// `selected` paints the cell background with the selection color (gapless,
/// matching the internal stripe technique of `egui_extras`) so the row keeps
/// its per-cell semantic colors (score scale, price up/down) under highlight.
/// `numeric` right-aligns the cell like the column header does, so body
/// values line up with their header (ref #221 verification).
fn render_cell(ui: &mut Ui, tokens: &ThemeTokens, cell: &DataCell, selected: bool, numeric: bool) {
    let c = &tokens.color;
    let render = |ui: &mut Ui| match cell {
        DataCell::Text(text) => {
            ui.label(
                RichText::new(text)
                    .size(tokens.typography.body)
                    .color(c.text_primary),
            );
        }
        DataCell::Price { value, change } => {
            let mut price = PriceText::new(tokens, *value);
            if let Some(change) = change {
                price = price.change(*change);
                // A change column (SEPA 涨跌幅 / screener 20日涨跌幅)
                // carries the percentage as BOTH the sort value and the
                // change: render a single signed percent form instead of
                // the duplicated "2.50 +2.50%".
                if *change == *value {
                    price = price.percent_only();
                }
            }
            price.show(ui);
        }
        DataCell::Count(count) => {
            ui.label(
                RichText::new(count.to_string())
                    .size(tokens.typography.body)
                    .color(c.text_primary),
            );
        }
        DataCell::Score {
            value,
            max,
            inverted,
        } => {
            let norm = if *inverted {
                1.0 - value.abs() / max.max(f32::EPSILON)
            } else {
                *value / max.max(f32::EPSILON)
            };
            ui.label(
                RichText::new(format!("{value:.1}"))
                    .monospace()
                    .size(tokens.typography.mono)
                    .color(score_color(tokens, norm)),
            );
        }
        DataCell::Rank(rank) => {
            let color = if *rank <= 3 {
                c.warning
            } else {
                c.text_primary
            };
            ui.label(
                RichText::new(rank.to_string())
                    .monospace()
                    .size(tokens.typography.mono)
                    .color(color),
            );
        }
    };
    // The selection fill must paint the full cell rect before the widget,
    // matching the header's alignment scope (both rendered inside the same
    // cell rect).
    if selected {
        let rect = ui.max_rect().expand2(0.5 * ui.spacing().item_spacing);
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, c.selection_bg);
    }
    if numeric {
        ui.with_layout(Layout::right_to_left(Align::Center), render);
    } else {
        ui.with_layout(Layout::left_to_right(Align::Center), render);
    }
}

/// The color of a color-scale score for the normalized value `norm` in 0..=1
/// (SEPA design §6.1): `norm ≥ 0.8` is success, `0.5–0.8` lerps
/// warning→success, `0.25–0.5` lerps error→warning and `< 0.25` is error.
/// The input is clamped into 0..=1 so callers may pass unnormalized data.
pub fn score_color(tokens: &ThemeTokens, norm: f32) -> Color32 {
    let c = &tokens.color;
    let norm = norm.clamp(0.0, 1.0);
    if norm >= 0.8 {
        c.success
    } else if norm >= 0.5 {
        c.warning.lerp_to_gamma(c.success, (norm - 0.5) / 0.3)
    } else if norm >= 0.25 {
        c.error.lerp_to_gamma(c.warning, (norm - 0.25) / 0.25)
    } else {
        c.error
    }
}

/// The color a [`DataCell::Price`] cell renders with (A-share red-up /
/// green-down, flat when unchanged). Exposed for callers that need to match
/// the table's price coloring.
pub fn price_cell_color(tokens: &ThemeTokens, change: Option<f32>) -> Color32 {
    match price_text::auto_tone(change) {
        price_text::Tone::Up => tokens.color.up,
        price_text::Tone::Down => tokens.color.down,
        price_text::Tone::Flat => tokens.color.flat,
        price_text::Tone::Auto => unreachable!("auto_tone never resolves to Auto"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::ThemeTokens;
    use egui_kittest::kittest::Queryable;

    fn text_rows() -> Vec<Vec<DataCell>> {
        vec![
            vec![
                DataCell::Text("600519".into()),
                DataCell::Text("贵州茅台".into()),
            ],
            vec![
                DataCell::Text("000001".into()),
                DataCell::Text("平安银行".into()),
            ],
            vec![
                DataCell::Text("000002".into()),
                DataCell::Text("万科A".into()),
            ],
        ]
    }

    fn price_rows() -> Vec<Vec<DataCell>> {
        vec![
            vec![
                DataCell::Text("a".into()),
                DataCell::Price {
                    value: 100.0,
                    change: Some(1.5),
                },
            ],
            vec![
                DataCell::Text("b".into()),
                DataCell::Price {
                    value: 50.0,
                    change: Some(-2.0),
                },
            ],
            vec![
                DataCell::Text("c".into()),
                DataCell::Price {
                    value: 200.0,
                    change: None,
                },
            ],
        ]
    }

    // ------------------------------------------------------------------
    // sort_rows pure logic (screener.rs:112-131 + 819-848 semantics)
    // ------------------------------------------------------------------

    #[test]
    fn text_column_sorts_lexicographically_ascending() {
        let rows = text_rows();
        let idx = sort_rows(&rows, 0, false);
        let codes: Vec<&str> = idx
            .iter()
            .map(|&i| match &rows[i][0] {
                DataCell::Text(t) => t.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(codes, ["000001", "000002", "600519"]);
    }

    #[test]
    fn text_column_sorts_descending() {
        let rows = text_rows();
        let idx = sort_rows(&rows, 0, true);
        let codes: Vec<&str> = idx
            .iter()
            .map(|&i| match &rows[i][0] {
                DataCell::Text(t) => t.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(codes, ["600519", "000002", "000001"]);
    }

    #[test]
    fn price_column_sorts_numerically() {
        let rows = price_rows();
        let idx = sort_rows(&rows, 1, false);
        let values: Vec<f32> = idx
            .iter()
            .map(|&i| match &rows[i][1] {
                DataCell::Price { value, .. } => *value,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(values, [50.0, 100.0, 200.0]);
    }

    #[test]
    fn price_column_sorts_numerically_descending() {
        let rows = price_rows();
        let idx = sort_rows(&rows, 1, true);
        let values: Vec<f32> = idx
            .iter()
            .map(|&i| match &rows[i][1] {
                DataCell::Price { value, .. } => *value,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(values, [200.0, 100.0, 50.0]);
    }

    #[test]
    fn count_column_sorts_numerically() {
        let rows = vec![
            vec![DataCell::Text("x".into()), DataCell::Count(3)],
            vec![DataCell::Text("y".into()), DataCell::Count(1)],
            vec![DataCell::Text("z".into()), DataCell::Count(2)],
        ];
        let idx = sort_rows(&rows, 1, false);
        let counts: Vec<usize> = idx
            .iter()
            .map(|&i| match &rows[i][1] {
                DataCell::Count(n) => *n,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(counts, [1, 2, 3]);
    }

    #[test]
    fn ties_are_broken_by_first_column_ascending() {
        // Same price (100.0) for all rows: the symbol column breaks the tie
        // ascending, even though the price column sorts descending.
        let rows = vec![
            vec![
                DataCell::Text("b".into()),
                DataCell::Price {
                    value: 100.0,
                    change: None,
                },
            ],
            vec![
                DataCell::Text("a".into()),
                DataCell::Price {
                    value: 100.0,
                    change: None,
                },
            ],
            vec![
                DataCell::Text("c".into()),
                DataCell::Price {
                    value: 100.0,
                    change: None,
                },
            ],
        ];
        let idx = sort_rows(&rows, 1, true);
        let symbols: Vec<&str> = idx
            .iter()
            .map(|&i| match &rows[i][0] {
                DataCell::Text(t) => t.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(symbols, ["a", "b", "c"]);
    }

    #[test]
    fn fully_equal_rows_keep_original_order() {
        let rows = vec![
            vec![DataCell::Text("same".into()), DataCell::Count(7)],
            vec![DataCell::Text("same".into()), DataCell::Count(7)],
            vec![DataCell::Text("same".into()), DataCell::Count(7)],
        ];
        let idx = sort_rows(&rows, 1, true);
        assert_eq!(idx, [0, 1, 2], "stable sort keeps original order");
    }

    // ------------------------------------------------------------------
    // Price color (A-share red-up / green-down)
    // ------------------------------------------------------------------

    #[test]
    fn price_cell_colors_follow_up_down_convention() {
        let tokens = ThemeTokens::dark();
        assert_eq!(price_cell_color(&tokens, Some(1.5)), tokens.color.up);
        assert_eq!(price_cell_color(&tokens, Some(-1.5)), tokens.color.down);
        assert_eq!(price_cell_color(&tokens, Some(0.0)), tokens.color.flat);
        assert_eq!(price_cell_color(&tokens, None), tokens.color.flat);
    }

    // ------------------------------------------------------------------
    // Sort state
    // ------------------------------------------------------------------

    #[test]
    fn toggle_sort_flips_direction_on_active_column() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(
            &tokens,
            vec![
                ColumnSpec {
                    header: "代码",
                    numeric: false,
                },
                ColumnSpec {
                    header: "最新价",
                    numeric: true,
                },
            ],
        );
        assert!(!table.sort_descending());
        table.toggle_sort(0);
        assert!(
            table.sort_descending(),
            "active column click flips direction"
        );
        table.toggle_sort(0);
        assert!(!table.sort_descending(), "second click flips back");
    }

    #[test]
    fn toggle_sort_new_column_defaults_ascending() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(
            &tokens,
            vec![
                ColumnSpec {
                    header: "代码",
                    numeric: false,
                },
                ColumnSpec {
                    header: "最新价",
                    numeric: true,
                },
            ],
        );
        table.toggle_sort(0); // descending on column 0
        table.toggle_sort(1); // new column -> ascending
        assert!(!table.sort_descending(), "new column defaults ascending");
    }

    #[test]
    fn set_sort_applies_initial_sort() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(
            &tokens,
            vec![
                ColumnSpec {
                    header: "代码",
                    numeric: false,
                },
                ColumnSpec {
                    header: "市值(亿)",
                    numeric: true,
                },
            ],
        );
        table.set_sort(1, true);
        assert!(table.sort_descending(), "set_sort descending applies");
        table.set_sort(0, false);
        assert!(!table.sort_descending(), "set_sort ascending applies");
    }

    #[test]
    fn set_descending_default_keeps_business_preference_on_new_column() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(
            &tokens,
            vec![
                ColumnSpec {
                    header: "代码",
                    numeric: false,
                },
                ColumnSpec {
                    header: "市值(亿)",
                    numeric: true,
                },
            ],
        );
        // Market cap (column 1) prefers descending, mirroring the screener.
        table.set_descending_default(1, true);
        table.toggle_sort(0); // select code column
        table.toggle_sort(1); // switch to market cap -> descending default
        assert!(
            table.sort_descending(),
            "new column with descending default sorts descending"
        );
        table.toggle_sort(0); // switch back to code -> ascending default
        assert!(
            !table.sort_descending(),
            "new column without default sorts ascending"
        );
    }

    // ------------------------------------------------------------------
    // Rendering
    // ------------------------------------------------------------------

    fn columns() -> Vec<ColumnSpec> {
        vec![
            ColumnSpec {
                header: "代码",
                numeric: false,
            },
            ColumnSpec {
                header: "最新价",
                numeric: true,
            },
        ]
    }

    #[test]
    fn empty_table_shows_empty_state() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(&tokens, columns());
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            table.show(ui);
        });
        harness.run();
        let _ = harness.get_by_label("无符合条件");
    }

    #[test]
    fn header_renders_sort_arrow_for_sort_state() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let tokens = ThemeTokens::dark();
        let table = Rc::new(RefCell::new(DataTable::new(&tokens, columns())));
        table.borrow_mut().set_rows(text_rows());
        let t = table.clone();
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            t.borrow_mut().show(ui);
        });
        harness.fit_contents();
        harness.step();
        // Ascending (default): the active header shows the up arrow.
        let _ = harness.get_by_label("代码 ↑");
        // Flip the direction through the handler `toggle_sort` (kittest
        // cannot simulate clicks inside TableBuilder; the click wiring is
        // covered by the toggle_sort unit tests).
        table.borrow_mut().toggle_sort(0);
        harness.step();
        let _ = harness.get_by_label("代码 ↓");
    }

    #[test]
    fn renders_rows_with_price_cells_without_panic() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(&tokens, columns());
        table.set_rows(price_rows());
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            table.show(ui);
        });
        harness.fit_contents();
        harness.step();
        let _ = harness.get_by_label_contains("共 3 行");
    }

    /// A percent column (value == change, e.g. SEPA 涨跌幅 / screener
    /// 20日涨跌幅) must render a single signed percent form, not the
    /// duplicated "2.50 +2.50%".
    #[test]
    fn percent_column_renders_single_signed_form() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(
            &tokens,
            vec![
                ColumnSpec {
                    header: "代码",
                    numeric: false,
                },
                ColumnSpec {
                    header: "涨跌幅",
                    numeric: true,
                },
            ],
        );
        table.set_rows(vec![
            vec![
                DataCell::Text("a".into()),
                DataCell::Price {
                    value: 2.5,
                    change: Some(2.5),
                },
            ],
            vec![
                DataCell::Text("b".into()),
                DataCell::Price {
                    value: -1.23,
                    change: Some(-1.23),
                },
            ],
        ]);
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            table.show(ui);
        });
        harness.fit_contents();
        harness.step();
        // The single percent form exists and the duplicated form does not.
        let _ = harness.get_by_label("+2.50%");
        let _ = harness.get_by_label("-1.23%");
        assert!(
            harness.query_all_by_label_contains("2.50 +2.50%").count() == 0,
            "percent column must not render the duplicated '2.50 +2.50%'"
        );
    }

    #[test]
    fn set_tokens_updates_theme_after_switch() {
        let dark = ThemeTokens::dark();
        let light = ThemeTokens::light();
        let mut table = DataTable::new(&dark, columns());
        table.set_tokens(light);

        assert_eq!(
            table.tokens, light,
            "after set_tokens the table must use the light palette"
        );
        assert_ne!(
            table.tokens, dark,
            "the table must no longer use the dark palette"
        );
    }

    // ------------------------------------------------------------------
    // SEPA Score / Rank cells (sub-issue #152)
    // ------------------------------------------------------------------

    fn score_rows() -> Vec<Vec<DataCell>> {
        vec![
            vec![
                DataCell::Text("a".into()),
                DataCell::Score {
                    value: 88.5,
                    max: 100.0,
                    inverted: false,
                },
            ],
            vec![
                DataCell::Text("b".into()),
                DataCell::Score {
                    value: 72.0,
                    max: 100.0,
                    inverted: false,
                },
            ],
            vec![
                DataCell::Text("c".into()),
                DataCell::Score {
                    value: 95.0,
                    max: 100.0,
                    inverted: false,
                },
            ],
        ]
    }

    #[test]
    fn score_column_sorts_numerically() {
        let rows = score_rows();
        let idx = sort_rows(&rows, 1, false);
        let values: Vec<f32> = idx
            .iter()
            .map(|&i| match &rows[i][1] {
                DataCell::Score { value, .. } => *value,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(values, [72.0, 88.5, 95.0]);
    }

    #[test]
    fn score_column_sorts_numerically_descending() {
        let rows = score_rows();
        let idx = sort_rows(&rows, 1, true);
        let values: Vec<f32> = idx
            .iter()
            .map(|&i| match &rows[i][1] {
                DataCell::Score { value, .. } => *value,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(values, [95.0, 88.5, 72.0]);
    }

    #[test]
    fn rank_column_sorts_numerically() {
        let rows = vec![
            vec![DataCell::Text("x".into()), DataCell::Rank(3)],
            vec![DataCell::Text("y".into()), DataCell::Rank(1)],
            vec![DataCell::Text("z".into()), DataCell::Rank(2)],
        ];
        let idx = sort_rows(&rows, 1, false);
        let ranks: Vec<usize> = idx
            .iter()
            .map(|&i| match &rows[i][1] {
                DataCell::Rank(r) => *r,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(ranks, [1, 2, 3]);
    }

    #[test]
    fn score_cells_render_one_decimal_and_signed_inverted() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(
            &tokens,
            vec![
                ColumnSpec {
                    header: "排名",
                    numeric: true,
                },
                ColumnSpec {
                    header: "总分",
                    numeric: true,
                },
                ColumnSpec {
                    header: "风险",
                    numeric: true,
                },
            ],
        );
        table.set_rows(vec![vec![
            DataCell::Rank(1),
            DataCell::Score {
                value: 88.5,
                max: 100.0,
                inverted: false,
            },
            DataCell::Score {
                value: -3.2,
                max: 3.75,
                inverted: true,
            },
        ]]);
        let mut harness = egui_kittest::Harness::new_ui(move |ui| {
            table.show(ui);
        });
        harness.fit_contents();
        harness.step();
        let _ = harness.get_by_label("88.5");
        let _ = harness.get_by_label("-3.2");
        let _ = harness.get_by_label("1");
    }

    // ------------------------------------------------------------------
    // score_color scale (SEPA design §6.1)
    // ------------------------------------------------------------------

    #[test]
    fn score_color_endpoints_follow_design() {
        let tokens = ThemeTokens::dark();
        let c = &tokens.color;
        assert_eq!(score_color(&tokens, 1.0), c.success);
        assert_eq!(score_color(&tokens, 0.8), c.success, "0.8 is success");
        assert_eq!(
            score_color(&tokens, 0.79),
            c.warning.lerp_to_gamma(c.success, 0.29 / 0.3)
        );
        assert_eq!(
            score_color(&tokens, 0.5),
            c.warning,
            "0.5 is the warning end"
        );
        assert_eq!(score_color(&tokens, 0.25), c.error, "0.25 is the error end");
        assert_eq!(score_color(&tokens, 0.0), c.error);
        assert_eq!(
            score_color(&tokens, -1.0),
            c.error,
            "below 0 clamps to error"
        );
        assert_eq!(
            score_color(&tokens, 2.0),
            c.success,
            "above 1 clamps to success"
        );
    }

    #[test]
    fn score_color_midpoints_lerp_between_buckets() {
        let tokens = ThemeTokens::dark();
        let c = &tokens.color;
        // 0.65 is the exact midpoint of the 0.5–0.8 warning→success band.
        assert_eq!(
            score_color(&tokens, 0.65),
            c.warning.lerp_to_gamma(c.success, 0.5)
        );
        // 0.375 is the exact midpoint of the 0.25–0.5 error→warning band.
        assert_eq!(
            score_color(&tokens, 0.375),
            c.error.lerp_to_gamma(c.warning, 0.5)
        );
    }

    #[test]
    fn score_color_is_monotonically_increasing() {
        let tokens = ThemeTokens::dark();
        let mut prev = score_color(&tokens, 0.0);
        for i in 1..=100 {
            let norm = i as f32 / 100.0;
            let cur = score_color(&tokens, norm);
            assert!(
                cur.r() >= prev.r() || cur.g() >= prev.g(),
                "color must not regress at norm={norm}"
            );
            prev = cur;
        }
    }

    // ------------------------------------------------------------------
    // Row selection highlight (details-panel linkage)
    // ------------------------------------------------------------------

    #[test]
    fn set_selected_highlights_row_with_selection_color() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(&tokens, columns());
        table.set_rows(price_rows());
        table.set_selected(Some(1));
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(300.0, 120.0))
            .build_ui(move |ui| {
                table.show(ui);
            });
        harness.run();

        let selection_bg = tokens.color.selection_bg;
        let highlighted = harness
            .output()
            .shapes
            .iter()
            .any(|clipped| shapes_contain_fill(&clipped.shape, selection_bg));
        assert!(highlighted, "selected row must paint the selection_bg fill");
    }

    #[test]
    fn set_selected_none_paints_no_selection() {
        let tokens = ThemeTokens::dark();
        let mut table = DataTable::new(&tokens, columns());
        table.set_rows(price_rows());
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(300.0, 120.0))
            .build_ui(move |ui| {
                table.show(ui);
            });
        harness.run();

        let selection_bg = tokens.color.selection_bg;
        let highlighted = harness
            .output()
            .shapes
            .iter()
            .any(|clipped| shapes_contain_fill(&clipped.shape, selection_bg));
        assert!(!highlighted, "no selection must not paint selection_bg");
    }

    // ------------------------------------------------------------------
    // Column alignment: numeric cells must share the header's right
    // alignment (ref #221 verification: columns must line up with their
    // header).
    // ------------------------------------------------------------------

    #[test]
    fn numeric_cell_renders_right_aligned() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            ui.set_width(200.0);
            render_cell(
                ui,
                &tokens,
                &DataCell::Price {
                    value: 12.34,
                    change: None,
                },
                false,
                true,
            );
        });
        harness.run();
        let label = harness.get_by_label("12.34");
        assert!(
            label.rect().min.x > 150.0,
            "numeric cell must right-align like its header, got min.x={:.1} in 200px-wide cell",
            label.rect().min.x
        );
    }

    #[test]
    fn text_cell_renders_left_aligned() {
        let tokens = ThemeTokens::dark();
        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            ui.set_width(200.0);
            render_cell(ui, &tokens, &DataCell::Text("代码".into()), false, false);
        });
        harness.run();
        let label = harness.get_by_label("代码");
        assert!(
            label.rect().min.x < 50.0,
            "text cell must stay left-aligned, got min.x={:.1} in 200px-wide cell",
            label.rect().min.x
        );
    }

    /// Recursively scan emitted shapes for a rect filled with `color`.
    fn shapes_contain_fill(shape: &egui::Shape, color: egui::Color32) -> bool {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().any(|s| shapes_contain_fill(s, color)),
            egui::Shape::Rect(rect) => rect.fill == color,
            _ => false,
        }
    }
}
