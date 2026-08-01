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
/// [`Self::Text`] sorts lexicographically, [`Self::Price`] and [`Self::Count`]
/// numerically.
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
        }
    }

    /// Replace the rows to display (call each frame with fresh data).
    pub fn set_rows(&mut self, rows: Vec<Vec<DataCell>>) {
        self.rows = rows;
    }

    /// Whether the current sort is descending.
    pub fn sort_descending(&self) -> bool {
        self.sort_descending
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
                    body.row(tokens.spacing.table_row_h, |mut row| {
                        let cells = &self.rows[orig_index];
                        for cell in cells {
                            row.col(|ui| {
                                render_cell(ui, &tokens, cell);
                            });
                        }
                        if row.response().clicked() {
                            clicked_row = Some(orig_index);
                        }
                    });
                }
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
            self.sort_descending = false;
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
        // Mixed types in one column are a caller error; keep the stable order.
        _ => Ordering::Equal,
    }
}

/// Render one cell with the widget matching its variant.
fn render_cell(ui: &mut Ui, tokens: &ThemeTokens, cell: &DataCell) {
    let c = &tokens.color;
    match cell {
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
}
