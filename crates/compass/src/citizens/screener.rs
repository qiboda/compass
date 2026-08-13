//! Screener panel citizen — Metabase-style condition card builder + results table.
//!
//! The builder (epic #243 Batch 2, issue #245) replaces the fixed
//! `ConditionForm` with an AND/OR card group that operates directly on the
//! Batch 1 `Filter` AST. The view model lives in [`screener_builder`]; this
//! module owns the widget state (card items + MultiSelect popup instances)
//! and renders the card group tree.

use std::collections::HashMap;

use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use compass_types::{
    BreakoutCondition, Filter, MetaCond, MomentumCondition, ScreenerQuery, VolumeCondition,
};
use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::badge::Badge;
use compass_ui::widgets::button::{Button, ButtonSize, ButtonVariant};
use compass_ui::widgets::card::Card;
use compass_ui::widgets::checkbox::Checkbox;
use compass_ui::widgets::data_table::{ColumnSpec, DataCell, DataTable};
use compass_ui::widgets::dropdown::Dropdown;
use compass_ui::widgets::empty_state::EmptyState;
use compass_ui::widgets::icon_button::IconButton;
use compass_ui::widgets::multi_select::MultiSelect;
use compass_ui::widgets::segmented::Segmented;

use crate::citizens::screener_builder::{
    BoolOp, CondGroup, CondItem, CondLeaf, LeafKind, LeafParams, MaKind, filter_to_items,
    group_to_filter,
};
use crate::messages::{FetchRequest, RunScreenerRequest};
use crate::state::SharedState;

/// Results table column specs (design §6.6). Headers hold **i18n keys**
/// (design `.omo/designs/gui-i18n.md` §1); `DataTable::show` resolves them
/// via `compass_i18n::t!()` every frame.
const COLUMNS: [ColumnSpec; 6] = [
    ColumnSpec {
        header: "screener.table.code",
        numeric: false,
    },
    ColumnSpec {
        header: "screener.table.name",
        numeric: false,
    },
    ColumnSpec {
        header: "screener.table.latest",
        numeric: true,
    },
    ColumnSpec {
        header: "screener.table.change_20d",
        numeric: true,
    },
    ColumnSpec {
        header: "screener.table.market_cap",
        numeric: true,
    },
    ColumnSpec {
        header: "screener.table.industry",
        numeric: false,
    },
];

/// Index of the market-cap column — the screener's default sort target
/// (descending, biggest first), matching the pre-componentization behavior.
const MARKET_CAP_COLUMN: usize = 4;

/// Card kinds offered by the type dropdown / add menu (the add menu appends
/// the 「子分组」 sentinel after these). [`LeafKind::Unknown`] is read-only
/// and never selectable.
const SELECTABLE_KINDS: [LeafKind; 11] = [
    LeafKind::Industry,
    LeafKind::Exchange,
    LeafKind::Board,
    LeafKind::ListYears,
    LeafKind::MarketCap,
    LeafKind::Delisted,
    LeafKind::Ma,
    LeafKind::Breakout,
    LeafKind::Momentum,
    LeafKind::VolumeSurge,
    LeafKind::UpDays,
];

/// Screener panel citizen.
///
/// Renders the condition card builder and the results table. The heavy
/// lifting runs on the backend via `run_screener_signal`.
pub struct ScreenerPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    /// Theme tokens copied at construction (component styling).
    tokens: ThemeTokens,
    /// Condition card builder root group items.
    builder_root: Vec<CondItem>,
    /// Root group boolean operator (AND default).
    builder_root_operator: BoolOp,
    /// MultiSelect instances keyed by card path (stateful — open/filter live
    /// in the instance; selections are mirrored to/from `CondLeaf.params`
    /// each frame).
    builder_multi_selects: HashMap<String, MultiSelect>,
    /// Results table — owns its sort state across frames.
    table: DataTable,
    /// Persists the current query whenever a filter run is triggered.
    on_save: Box<dyn Fn(&ScreenerQuery) + Send + Sync>,
}

impl Citizen for ScreenerPanel {
    fn id(&self) -> &CitizenId {
        &self.citizen_id
    }

    fn citizen_state(&self) -> &CitizenState {
        &self.citizen_state
    }

    fn citizen_state_mut(&mut self) -> &mut CitizenState {
        &mut self.citizen_state
    }
}

impl ScreenerPanel {
    /// Create a screener panel with the given citizen identity/state.
    ///
    /// `restore` optionally provides saved conditions from config; `on_save`
    /// is invoked with the current query whenever the filter runs.
    pub fn new(
        citizen_id: CitizenId,
        citizen_state: CitizenState,
        restore: Option<&ScreenerQuery>,
        on_save: Box<dyn Fn(&ScreenerQuery) + Send + Sync>,
        tokens: &ThemeTokens,
    ) -> Self {
        // Restore of the default empty shape (bare `Delisted(false)` node or
        // empty `And` — the `From<ScreenerQuery>` outputs of an empty query)
        // seeds the standard 6 base cards, matching the pre-builder default
        // behavior (exclude-delisted checked, everything else unbounded).
        let (builder_root, builder_multi_selects) = match restore {
            None => (default_root_cards(), HashMap::new()),
            Some(query) => {
                let filter = Filter::from(query.clone());
                match &filter {
                    Filter::Meta(MetaCond::Delisted(false)) => {
                        (default_root_cards(), HashMap::new())
                    }
                    Filter::And(v) if v.is_empty() => (default_root_cards(), HashMap::new()),
                    _ => (filter_to_items(&filter), HashMap::new()),
                }
            }
        };
        let mut table = DataTable::new(tokens, COLUMNS.to_vec());
        table.set_sort(MARKET_CAP_COLUMN, true);
        table.set_descending_default(MARKET_CAP_COLUMN, true);
        Self {
            citizen_id,
            citizen_state,
            tokens: *tokens,
            builder_root,
            builder_root_operator: BoolOp::And,
            builder_multi_selects,
            table,
            on_save,
        }
    }

    /// Compile the builder cards into the `Filter` AST (the run contract).
    fn build_filter(&self) -> Filter {
        group_to_filter(&CondGroup {
            operator: self.builder_root_operator,
            items: self.builder_root.clone(),
        })
    }

    /// Render the panel: condition builder + results area.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        run_screener_signal: &Signal<RunScreenerRequest>,
        work_signal: &Signal<FetchRequest>,
        industries: &[String],
        boards: &[String],
    ) {
        ui.vertical(|ui| {
            self.condition_builder(ui, industries, boards);

            ui.add_space(self.form_tokens().spacing.sm);
            if Button::new(&self.form_tokens(), compass_i18n::t!("screener.filter"))
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Md)
                .show(ui)
                .clicked()
            {
                let filter = self.build_filter();
                shared_state.screener_loading.set(true);
                // Clear the previous run error before compressing: the legacy
                // save hint below must survive the whole run (the toast layer
                // in main.rs pushes it on the None→Some transition).
                shared_state.screener_error.set(None);
                // Legacy save: reuse the engine's restricted reverse-compile
                // as the compressibility oracle — the same accept-grammar the
                // run path uses, so a query that compresses here is exactly a
                // query the engine can run. Inexpressible combinations
                // (Or/Not/UpDays/duplicate fields) surface the unsaved-state
                // hint instead of writing a lossy config.
                match compass_strategy::filter_to_query(&filter) {
                    Ok(query) => (self.on_save)(&query),
                    Err(_) => shared_state.screener_error.set(Some(
                        compass_i18n::t!("screener.builder.unsupported_save").into_owned(),
                    )),
                }
                if let Err(e) = run_screener_signal.send(RunScreenerRequest { filter }) {
                    shared_state.screener_loading.set(false);
                    shared_state.screener_error.set(Some(
                        compass_i18n::t!("error.screener_run", e = e.to_string()).into_owned(),
                    ));
                }
            }

            ui.add_space(self.form_tokens().spacing.md);

            self.results_area(ui, shared_state, work_signal);
        });
    }

    /// Results table with sortable headers and row-click chart linkage.
    fn results_area(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        work_signal: &Signal<FetchRequest>,
    ) {
        let rows = shared_state.screener_result.get();

        if shared_state.screener_loading.get() {
            ui.spinner();
            ui.label(compass_i18n::t!("screener.filtering"));
        } else if let Some(err) = shared_state.screener_error.get() {
            // Engine's Display prefix for the restricted accept-grammar
            // rejection (compass-strategy ScreenerError::UnsupportedFilter).
            // Batch 2 builder shapes outside it (Or/Not/UpDays/sub-groups)
            // are supported in a later engine batch — add the friendly note.
            let label = if err.starts_with("unsupported filter shape") {
                compass_i18n::t!("screener.builder.unsupported_run", e = err).into_owned()
            } else {
                err
            };
            ui.colored_label(ui.visuals().error_fg_color, label);
        } else {
            self.table
                .set_rows(rows.iter().map(Self::row_cells).collect());
            if let Some(idx) = self.table.show(ui) {
                dispatch_row_fetch(shared_state, work_signal, &rows, idx);
            }
        }
    }

    /// Map one `ScreenerRow` into the table's cell model (design §6.6):
    /// code/name text, latest price + 20-day change as price cells (red-up /
    /// green-down), market cap as a count, industry text.
    fn row_cells(row: &compass_types::ScreenerRow) -> Vec<DataCell> {
        vec![
            DataCell::Text(row.symbol.clone()),
            DataCell::Text(row.name.clone()),
            DataCell::Price {
                value: row.latest_price as f32,
                change: None,
            },
            DataCell::Price {
                value: row.change_20d as f32,
                // value == change marks a percent column: the value drives
                // sorting while render_cell renders a single signed percent
                // form (e.g. "+2.50%"), not the duplicated "2.50 +2.50%".
                change: Some(row.change_20d as f32),
            },
            DataCell::Count(row.market_cap.round() as usize),
            DataCell::Text(row.industry.clone()),
        ]
    }

    /// Condition card builder (design `.omo/designs/llm-screener-ui.md` §3-5):
    /// one root `Card` whose header carries the AND/OR segmented + condition
    /// count + clear button, followed by the card list (leaf rows /
    /// recursively nested group frames) and the bottom add menu.
    fn condition_builder(&mut self, ui: &mut egui::Ui, industries: &[String], boards: &[String]) {
        let tokens = self.form_tokens();
        ui.vertical(|ui| {
            Card::new(&tokens)
                .title(&compass_i18n::t!("screener.builder.card_title"))
                .padding(compass_ui::widgets::card::CardPadding::Md)
                .show(ui, |ui| {
                    if render_root_header(
                        ui,
                        &tokens,
                        &mut self.builder_root_operator,
                        self.builder_root.len(),
                    ) {
                        self.builder_root.clear();
                        self.builder_multi_selects.clear();
                    }
                    ui.add_space(tokens.spacing.sm);
                    if self.builder_root.is_empty() {
                        EmptyState::new(
                            &tokens,
                            egui_phosphor::regular::FUNNEL,
                            &compass_i18n::t!("screener.builder.empty_title"),
                        )
                        .description(&compass_i18n::t!("screener.builder.empty_desc"))
                        .show(ui);
                    } else {
                        render_group_items(
                            ui,
                            &tokens,
                            "cond_root",
                            &mut self.builder_root,
                            &mut self.builder_multi_selects,
                            industries,
                            boards,
                        );
                    }
                    ui.add_space(tokens.spacing.sm);
                    render_add_menu(ui, &tokens, "cond_root", &mut self.builder_root);
                });
        });
    }

    /// The panel's theme tokens (copied at construction).
    fn form_tokens(&self) -> ThemeTokens {
        self.tokens
    }

    /// Update the theme tokens after a theme switch so the condition cards
    /// and results table restyle without losing the builder state.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
        for ms in self.builder_multi_selects.values_mut() {
            ms.set_tokens(tokens);
        }
        self.table.set_tokens(tokens);
    }
}

/// Root group header: AND/OR segmented + condition-count badge + clear
/// button. Returns whether the clear button was clicked.
fn render_root_header(
    ui: &mut egui::Ui,
    tokens: &ThemeTokens,
    operator: &mut BoolOp,
    count: usize,
) -> bool {
    let mut cleared = false;
    ui.horizontal(|ui| {
        let selected = match *operator {
            BoolOp::And => 0,
            BoolOp::Or => 1,
        };
        if let Some(idx) = Segmented::new(
            tokens,
            [
                compass_i18n::t!("screener.builder.group_and"),
                compass_i18n::t!("screener.builder.group_or"),
            ],
        )
        .selected(selected)
        .show(ui)
        {
            *operator = if idx == 1 { BoolOp::Or } else { BoolOp::And };
        }
        Badge::new(tokens, count).show(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if IconButton::new(tokens, egui_phosphor::regular::ERASER)
                .tooltip(&compass_i18n::t!("screener.builder.clear_tooltip"))
                .small()
                .show(ui)
            {
                cleared = true;
            }
        });
    });
    cleared
}

/// Render a group's items: leaf cards flow in an atomic-group wrapped row
/// pattern (ref #220 — label+control never split across rows), nested group
/// frames occupy full-width rows and recurse. Removals are collected and
/// applied after the loop (borrow-safe with the `iter_mut` walk).
fn render_group_items(
    ui: &mut egui::Ui,
    tokens: &ThemeTokens,
    path: &str,
    items: &mut Vec<CondItem>,
    ms_map: &mut HashMap<String, MultiSelect>,
    industries: &[String],
    boards: &[String],
) {
    let mut to_remove: Vec<usize> = Vec::new();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.y = tokens.spacing.sm;
        for (index, item) in items.iter_mut().enumerate() {
            let item_path = format!("{path}_{index}");
            let remove = match item {
                CondItem::Leaf(leaf) => {
                    render_leaf_row(ui, tokens, &item_path, leaf, ms_map, industries, boards)
                }
                CondItem::Group(group) => {
                    // Full-width nested container: start on a fresh row so the
                    // frame does not begin mid-row after preceding cards.
                    if ui.available_size_before_wrap().x < 320.0 {
                        ui.end_row();
                    }
                    let start = ui.cursor().min;
                    let row_w = ui.available_size_before_wrap().x;
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                            start,
                            egui::pos2(start.x + row_w, start.y + f32::INFINITY),
                        )),
                        |ui| {
                            render_sub_group(
                                ui, tokens, &item_path, group, ms_map, industries, boards,
                            )
                        },
                    )
                    .inner
                }
            };
            if remove {
                to_remove.push(index);
            }
            ui.add_space(tokens.spacing.md);
        }
    });
    for index in to_remove.iter().rev() {
        items.remove(*index);
    }
}

/// One leaf card row: type dropdown + kind parameters + negate + delete as a
/// single atomic group (ref #220). Returns whether the card was deleted.
fn render_leaf_row(
    ui: &mut egui::Ui,
    tokens: &ThemeTokens,
    path: &str,
    leaf: &mut CondLeaf,
    ms_map: &mut HashMap<String, MultiSelect>,
    industries: &[String],
    boards: &[String],
) -> bool {
    if leaf.kind == LeafKind::Unknown {
        return render_unknown_row(ui, tokens, leaf);
    }
    if ui.available_size_before_wrap().x < leaf_row_min_width(leaf.kind) {
        ui.end_row();
    }
    let start = ui.cursor().min;
    let row_w = ui.available_size_before_wrap().x;
    let mut remove = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                start,
                egui::pos2(start.x + row_w, start.y + tokens.spacing.control_md),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            // Type dropdown. Switching kind resets the params to defaults and
            // rebuilds the path's MultiSelect instance (prune stale keys).
            let kind_idx = SELECTABLE_KINDS
                .iter()
                .position(|k| *k == leaf.kind)
                .unwrap_or(0);
            if let Some(idx) = Dropdown::new(tokens, kind_options())
                .selected(kind_idx)
                .width(160.0)
                .id_salt(&format!("{path}_kind"))
                .show(ui)
            {
                leaf.kind = SELECTABLE_KINDS[idx];
                leaf.params = default_params(leaf.kind);
                leaf.negated = false;
                prune_ms_prefix(ms_map, &format!("{path}_"));
            }
            remove |= render_leaf_params(ui, tokens, path, leaf, ms_map, industries, boards);
            if IconButton::new(tokens, egui_phosphor::regular::EXCLUDE)
                .tooltip(&compass_i18n::t!("screener.builder.negate_tooltip"))
                .small()
                .show(ui)
            {
                leaf.negated = !leaf.negated;
            }
            if IconButton::new(tokens, egui_phosphor::regular::X)
                .tooltip(&compass_i18n::t!("screener.builder.delete_tooltip"))
                .small()
                .show(ui)
            {
                remove = true;
                prune_ms_prefix(ms_map, &format!("{path}_"));
            }
        },
    );
    remove
}

/// Read-only summary row for an unrecognized AST shape (mono, weak) plus a
/// delete button. Returns whether the card was deleted.
fn render_unknown_row(ui: &mut egui::Ui, tokens: &ThemeTokens, leaf: &mut CondLeaf) -> bool {
    if ui.available_size_before_wrap().x < 240.0 {
        ui.end_row();
    }
    let summary = match &leaf.params {
        LeafParams::Unknown(json) => json.chars().take(24).collect::<String>(),
        _ => String::new(),
    };
    let mut remove = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                ui.cursor().min,
                egui::pos2(
                    ui.cursor().min.x + 400.0,
                    ui.cursor().min.y + tokens.spacing.control_md,
                ),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.label(
                egui::RichText::new(compass_i18n::t!("screener.builder.unknown_shape"))
                    .monospace()
                    .weak(),
            );
            ui.label(egui::RichText::new(summary).monospace().weak());
            if IconButton::new(tokens, egui_phosphor::regular::X)
                .tooltip(&compass_i18n::t!("screener.builder.delete_tooltip"))
                .small()
                .show(ui)
            {
                remove = true;
            }
        },
    );
    remove
}

/// Kind parameter controls (design §3-5). Returns whether the card was
/// deleted (only the Delisted checkbox can remove its card, by unchecking).
fn render_leaf_params(
    ui: &mut egui::Ui,
    tokens: &ThemeTokens,
    path: &str,
    leaf: &mut CondLeaf,
    ms_map: &mut HashMap<String, MultiSelect>,
    industries: &[String],
    boards: &[String],
) -> bool {
    let mut remove = false;
    match &mut leaf.params {
        LeafParams::MultiSelect(v) => match leaf.kind {
            LeafKind::Industry => {
                render_multi_select(ui, tokens, path, "industry", v, ms_map, industries)
            }
            LeafKind::Exchange => {
                let exchanges = ["SH", "SZ", "BJ"].map(String::from);
                render_multi_select(ui, tokens, path, "exchange", v, ms_map, &exchanges)
            }
            LeafKind::Board => render_multi_select(ui, tokens, path, "board", v, ms_map, boards),
            _ => false,
        },
        LeafParams::ListYears(years) => {
            let options = [
                compass_i18n::t!("screener.any"),
                compass_i18n::t!("screener.years_1"),
                compass_i18n::t!("screener.years_3"),
                compass_i18n::t!("screener.years_5"),
            ];
            let values: [Option<u32>; 4] = [None, Some(1), Some(3), Some(5)];
            let current = options
                .iter()
                .zip(values.iter())
                .position(|(_, v)| *v == *years)
                .unwrap_or(0);
            if let Some(idx) = Dropdown::new(tokens, options)
                .selected(current)
                .width(100.0)
                .id_salt(&format!("{path}_years"))
                .show(ui)
            {
                *years = values[idx];
            }
            false
        }
        LeafParams::MarketCap { min, max } => {
            let mut min_v = min.unwrap_or(0.0);
            if ui
                .add(
                    egui::DragValue::new(&mut min_v)
                        .speed(1.0)
                        .prefix(compass_i18n::t!("screener.min_pct")),
                )
                .changed()
            {
                *min = (min_v > 0.0).then_some(min_v);
            }
            let mut max_v = max.unwrap_or(0.0);
            if ui
                .add(
                    egui::DragValue::new(&mut max_v)
                        .speed(1.0)
                        .prefix(compass_i18n::t!("screener.max_pct")),
                )
                .changed()
            {
                *max = (max_v > 0.0).then_some(max_v);
            }
            false
        }
        LeafParams::Delisted(exclude) => {
            // Checked = card exists (exclusion on, `Delisted(false)` AST).
            // Unchecking removes the card entirely.
            let mut checked = !*exclude;
            let resp = Checkbox::new(
                tokens,
                &mut checked,
                compass_i18n::t!("screener.exclude_delisted"),
            )
            .show(ui);
            if resp.changed() {
                if checked {
                    *exclude = false;
                } else {
                    remove = true;
                }
            }
            remove
        }
        LeafParams::Ma(kind) => {
            let options = [
                compass_i18n::t!("screener.ma_above20"),
                compass_i18n::t!("screener.ma_above60"),
                compass_i18n::t!("screener.ma_bullish"),
            ];
            let current = match kind {
                MaKind::AboveMa20 => 0,
                MaKind::AboveMa60 => 1,
                MaKind::BullishAlign => 2,
            };
            if let Some(idx) = Dropdown::new(tokens, options)
                .selected(current)
                .width(210.0)
                .id_salt(&format!("{path}_ma_kind"))
                .show(ui)
            {
                *kind = match idx {
                    1 => MaKind::AboveMa60,
                    2 => MaKind::BullishAlign,
                    _ => MaKind::AboveMa20,
                };
            }
            false
        }
        LeafParams::Breakout(days) => {
            ui.label(compass_i18n::t!("screener.n_label"));
            ui.add(egui::DragValue::new(days).range(1..=250));
            false
        }
        LeafParams::Momentum {
            days,
            min_pct,
            max_pct,
        } => {
            ui.label(compass_i18n::t!("screener.n_label"));
            ui.add(egui::DragValue::new(days).range(1..=250));
            ui.label(compass_i18n::t!("screener.min_pct"));
            ui.add(egui::DragValue::new(min_pct).speed(1.0));
            ui.label(compass_i18n::t!("screener.max_pct"));
            ui.add(egui::DragValue::new(max_pct).speed(1.0));
            false
        }
        LeafParams::VolumeSurge { days, times } => {
            ui.label(compass_i18n::t!("screener.n_label"));
            ui.add(egui::DragValue::new(days).range(1..=80));
            ui.label(compass_i18n::t!("screener.times"));
            ui.add(egui::DragValue::new(times).speed(0.1));
            false
        }
        LeafParams::UpDays { n, min_pct } => {
            ui.label(compass_i18n::t!("screener.n_label"));
            ui.add(egui::DragValue::new(n).range(1..=250));
            ui.label(compass_i18n::t!("screener.min_pct"));
            ui.add(egui::DragValue::new(min_pct).speed(1.0));
            false
        }
        LeafParams::Unknown(_) | LeafParams::None => false,
    }
}

/// One multi-select parameter. The instance is cached in `ms_map` keyed by
/// the card path (id_salt = same path, ref #220/#222); `CondLeaf.params` is
/// the single source of truth — mirrored into the instance before rendering
/// and written back after interaction.
fn render_multi_select(
    ui: &mut egui::Ui,
    tokens: &ThemeTokens,
    path: &str,
    slug: &str,
    values: &mut Vec<String>,
    ms_map: &mut HashMap<String, MultiSelect>,
    options: &[String],
) -> bool {
    let key = format!("{path}_{slug}");
    let entry = ms_map
        .entry(key.clone())
        .or_insert_with(|| MultiSelect::new(tokens, std::iter::empty::<&str>()).id_salt(&key));
    entry.options = options.to_vec();
    entry.selected = values.clone();
    let changed = entry.show(ui);
    if changed {
        *values = entry.selected.clone();
    }
    false
}

/// Nested AND/OR group container (design §3): lightweight `Frame` (never a
/// Card-in-Card) with a header row (segmented + delete) and a recursive item
/// list. Returns whether the group was deleted.
fn render_sub_group(
    ui: &mut egui::Ui,
    tokens: &ThemeTokens,
    path: &str,
    group: &mut CondGroup,
    ms_map: &mut HashMap<String, MultiSelect>,
    industries: &[String],
    boards: &[String],
) -> bool {
    let c = &tokens.color;
    let mut remove = false;
    let frame = egui::Frame::new()
        .fill(c.bg_panel_alt)
        .stroke(egui::Stroke::new(1.0, c.border_strong))
        .corner_radius(tokens.radius.sm)
        .inner_margin(egui::Margin::symmetric(10, 8));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let selected = match group.operator {
                BoolOp::And => 0,
                BoolOp::Or => 1,
            };
            if let Some(idx) = Segmented::new(
                tokens,
                [
                    compass_i18n::t!("screener.builder.group_and"),
                    compass_i18n::t!("screener.builder.group_or"),
                ],
            )
            .selected(selected)
            .show(ui)
            {
                group.operator = if idx == 1 { BoolOp::Or } else { BoolOp::And };
            }
            if IconButton::new(tokens, egui_phosphor::regular::X)
                .tooltip(&compass_i18n::t!("screener.builder.delete_tooltip"))
                .small()
                .show(ui)
            {
                remove = true;
                prune_ms_prefix(ms_map, &format!("{path}_"));
            }
        });
        if group.items.is_empty() {
            ui.label(egui::RichText::new(compass_i18n::t!("screener.builder.empty_group")).weak());
        } else {
            ui.add_space(tokens.spacing.xs);
            render_group_items(
                ui,
                tokens,
                path,
                &mut group.items,
                ms_map,
                industries,
                boards,
            );
        }
        ui.add_space(tokens.spacing.xs);
        render_add_menu(ui, tokens, path, &mut group.items);
    });
    remove
}

/// Bottom add menu: a Dropdown whose trigger always shows the add-condition
/// sentinel (`.selected(0)` re-applied every frame), with the 11 selectable
/// kinds plus the 「子分组」 sentinel. Selecting a kind appends a default card;
/// selecting the group sentinel appends an empty AND group.
fn render_add_menu(ui: &mut egui::Ui, tokens: &ThemeTokens, path: &str, items: &mut Vec<CondItem>) {
    let add_group_idx = SELECTABLE_KINDS.len() + 1;
    let mut options: Vec<String> =
        vec![compass_i18n::t!("screener.builder.add_condition").into_owned()];
    options.extend(kind_options());
    options.push(compass_i18n::t!("screener.builder.add_group").into_owned());
    if let Some(idx) = Dropdown::new(tokens, options)
        .selected(0)
        .width(150.0)
        .id_salt(&format!("{path}_add"))
        .show(ui)
    {
        if idx == add_group_idx {
            items.push(CondItem::Group(CondGroup::default()));
        } else if idx > 0 {
            let kind = SELECTABLE_KINDS[idx - 1];
            items.push(CondItem::Leaf(CondLeaf {
                kind,
                params: default_params(kind),
                negated: false,
            }));
        }
    }
}

/// The default 6 base cards (design §4, decision: default = 现状 behavior):
/// industry / exchange / board / listing years / market cap / delisted
/// (exclusion checked).
fn default_root_cards() -> Vec<CondItem> {
    vec![
        CondItem::Leaf(CondLeaf {
            kind: LeafKind::Industry,
            params: LeafParams::MultiSelect(Vec::new()),
            negated: false,
        }),
        CondItem::Leaf(CondLeaf {
            kind: LeafKind::Exchange,
            params: LeafParams::MultiSelect(Vec::new()),
            negated: false,
        }),
        CondItem::Leaf(CondLeaf {
            kind: LeafKind::Board,
            params: LeafParams::MultiSelect(Vec::new()),
            negated: false,
        }),
        CondItem::Leaf(CondLeaf {
            kind: LeafKind::ListYears,
            params: LeafParams::ListYears(None),
            negated: false,
        }),
        CondItem::Leaf(CondLeaf {
            kind: LeafKind::MarketCap,
            params: LeafParams::MarketCap {
                min: None,
                max: None,
            },
            negated: false,
        }),
        CondItem::Leaf(CondLeaf {
            kind: LeafKind::Delisted,
            params: LeafParams::Delisted(false),
            negated: false,
        }),
    ]
}

/// Default parameters for a freshly added / type-switched card.
fn default_params(kind: LeafKind) -> LeafParams {
    match kind {
        LeafKind::Industry | LeafKind::Exchange | LeafKind::Board => {
            LeafParams::MultiSelect(Vec::new())
        }
        LeafKind::ListYears => LeafParams::ListYears(None),
        LeafKind::MarketCap => LeafParams::MarketCap {
            min: None,
            max: None,
        },
        LeafKind::Delisted => LeafParams::Delisted(false),
        LeafKind::Ma => LeafParams::Ma(MaKind::AboveMa20),
        LeafKind::Breakout => LeafParams::Breakout(BreakoutCondition::default().days),
        LeafKind::Momentum => LeafParams::Momentum {
            days: MomentumCondition::default().days,
            min_pct: MomentumCondition::default().min_pct,
            max_pct: MomentumCondition::default().max_pct,
        },
        LeafKind::VolumeSurge => LeafParams::VolumeSurge {
            days: VolumeCondition::default().days,
            times: VolumeCondition::default().times,
        },
        LeafKind::UpDays => LeafParams::UpDays { n: 3, min_pct: 0.0 },
        LeafKind::Unknown => LeafParams::Unknown(String::new()),
    }
}

/// I18n key for a leaf kind's display label.
fn leaf_kind_label(kind: LeafKind) -> &'static str {
    match kind {
        LeafKind::Industry => "screener.industry",
        LeafKind::Exchange => "screener.exchange",
        LeafKind::Board => "screener.board",
        LeafKind::ListYears => "screener.list_years",
        LeafKind::MarketCap => "screener.market_cap",
        LeafKind::Delisted => "screener.exclude_delisted",
        LeafKind::Ma => "screener.ma",
        LeafKind::Breakout => "screener.breakout",
        LeafKind::Momentum => "screener.momentum",
        LeafKind::VolumeSurge => "screener.volume",
        LeafKind::UpDays => "screener.builder.cond_up_days",
        LeafKind::Unknown => "screener.builder.unknown_shape",
    }
}

/// Translated kind labels for the type dropdown / add menu.
fn kind_options() -> Vec<String> {
    SELECTABLE_KINDS
        .iter()
        .map(|k| compass_i18n::t!(leaf_kind_label(*k)).into_owned())
        .collect()
}

/// Width estimate of a leaf card row, used by the wrap check (ref #220):
/// the whole card moves to a fresh row when it would not fit.
fn leaf_row_min_width(kind: LeafKind) -> f32 {
    match kind {
        LeafKind::Industry | LeafKind::Exchange | LeafKind::Board => 330.0,
        LeafKind::ListYears => 340.0,
        LeafKind::MarketCap => 390.0,
        LeafKind::Delisted => 360.0,
        LeafKind::Ma => 460.0,
        LeafKind::Breakout => 340.0,
        LeafKind::Momentum => 470.0,
        LeafKind::VolumeSurge => 360.0,
        LeafKind::UpDays => 390.0,
        LeafKind::Unknown => 300.0,
    }
}

/// Drop stale MultiSelect instances under a card path (deleted cards, type
/// switches) so shifted paths never resurrect old selections.
fn prune_ms_prefix(ms_map: &mut HashMap<String, MultiSelect>, prefix: &str) {
    ms_map.retain(|key, _| !key.starts_with(prefix));
}

/// Row-click linkage: fetch bars for the clicked result row (design §6.6).
/// Thin wrapper over the shared [`crate::dispatcher::dispatch_symbol_fetch`].
fn dispatch_row_fetch(
    shared_state: &SharedState,
    work_signal: &Signal<FetchRequest>,
    rows: &[compass_types::ScreenerRow],
    idx: usize,
) {
    if let Some(row) = rows.get(idx) {
        crate::dispatcher::dispatch_symbol_fetch(shared_state, work_signal, &row.symbol);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use compass_types::{CmpOp, FactorRef, MaCondition, SeriesCond, SeriesFactor};
    use compass_ui::tokens::ThemeTokens;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    fn panel_with_form() -> (ScreenerPanel, SharedState) {
        rust_i18n::set_locale("zh");
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let panel = ScreenerPanel::new(id, state, None, Box::new(|_| {}), &tokens);
        (panel, SharedState::new("SZ000001", "1d"))
    }

    #[test]
    fn new_creates_panel_with_correct_id() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (panel, _) = panel_with_form();
        assert_eq!(panel.id(), &CitizenId::new("screener"));
    }

    #[test]
    fn new_builder_seeds_default_six_cards_matching_query_contract() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (panel, _) = panel_with_form();
        assert_eq!(
            panel.builder_root.len(),
            6,
            "default root group seeds 6 base cards"
        );
        match panel.build_filter() {
            Filter::And(nodes) => assert!(
                nodes.contains(&Filter::Meta(MetaCond::Delisted(false))),
                "exclude-delisted defaults checked → Meta(Delisted(false)) present"
            ),
            other => panic!("default 6 cards must compile to an And root, got {other:?}"),
        }
    }

    #[test]
    fn build_filter_reflects_builder_state() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, _) = panel_with_form();
        match &mut panel.builder_root[0] {
            CondItem::Leaf(leaf) => {
                leaf.params = LeafParams::MultiSelect(vec!["白酒".to_string()]);
            }
            _ => panic!("card 0 must be a leaf"),
        }
        panel.builder_root.push(CondItem::Leaf(CondLeaf {
            kind: LeafKind::Ma,
            params: LeafParams::Ma(MaKind::BullishAlign),
            negated: false,
        }));
        panel.builder_root.push(CondItem::Leaf(CondLeaf {
            kind: LeafKind::Breakout,
            params: LeafParams::Breakout(120),
            negated: false,
        }));
        match panel.build_filter() {
            Filter::And(nodes) => {
                assert!(
                    nodes.contains(&Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])))
                );
                assert!(nodes.contains(&Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::Close,
                    op: CmpOp::Gt,
                    value: FactorRef::Factor(SeriesFactor::NDayHigh(120)),
                })));
                assert!(nodes.contains(&Filter::And(vec![
                    Filter::Series(SeriesCond::Cmp {
                        factor: SeriesFactor::Sma(5),
                        op: CmpOp::Gt,
                        value: FactorRef::Factor(SeriesFactor::Sma(20)),
                    }),
                    Filter::Series(SeriesCond::Cmp {
                        factor: SeriesFactor::Sma(20),
                        op: CmpOp::Gt,
                        value: FactorRef::Factor(SeriesFactor::Sma(60)),
                    }),
                ])));
            }
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn restore_seeds_builder_cards_from_query() {
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let query = ScreenerQuery {
            industries: vec!["银行".to_string()],
            ma: Some(MaCondition::BullishAlign),
            ..ScreenerQuery::default()
        };
        let panel = ScreenerPanel::new(id, state, Some(&query), Box::new(|_| {}), &tokens);

        // From(query) = And[Industry(银行), bullish pair, Delisted(false)] —
        // a non-empty shape restores as a single nested root group.
        assert_eq!(
            panel.builder_root.len(),
            1,
            "multi-member restore seeds a root group"
        );
        match &panel.builder_root[0] {
            CondItem::Group(group) => {
                assert_eq!(group.items.len(), 3);
                let industry = group
                    .items
                    .iter()
                    .find_map(|item| match item {
                        CondItem::Leaf(leaf) if leaf.kind == LeafKind::Industry => Some(leaf),
                        _ => None,
                    })
                    .expect("industry card");
                assert_eq!(
                    industry.params,
                    LeafParams::MultiSelect(vec!["银行".to_string()])
                );
                let ma = group
                    .items
                    .iter()
                    .find_map(|item| match item {
                        CondItem::Leaf(leaf) if leaf.kind == LeafKind::Ma => Some(leaf),
                        _ => None,
                    })
                    .expect("ma card");
                assert_eq!(ma.params, LeafParams::Ma(MaKind::BullishAlign));
            }
            _ => panic!(
                "expected a nested root group, got {:?}",
                panel.builder_root[0]
            ),
        }
    }

    #[test]
    fn builder_multi_select_cards_are_independent() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, _) = panel_with_form();
        match &mut panel.builder_root[0] {
            CondItem::Leaf(leaf) => {
                leaf.params = LeafParams::MultiSelect(vec!["银行".to_string()]);
            }
            _ => panic!("card 0 must be a leaf"),
        }
        match &mut panel.builder_root[1] {
            CondItem::Leaf(leaf) => {
                leaf.params = LeafParams::MultiSelect(vec!["SH".to_string()]);
            }
            _ => panic!("card 1 must be a leaf"),
        }
        match panel.build_filter() {
            Filter::And(nodes) => {
                assert!(
                    nodes.contains(&Filter::Meta(MetaCond::Industry(vec!["银行".to_string()])))
                );
                assert!(nodes.contains(&Filter::Meta(MetaCond::Exchange(vec!["SH".to_string()]))));
                assert!(
                    nodes.contains(&Filter::Meta(MetaCond::Board(vec![]))),
                    "board card stays untouched"
                );
            }
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn show_renders_condition_builder_no_panic() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, shared) = panel_with_form();
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries = vec!["银行".to_string(), "白酒".to_string()];
        let boards = vec!["主板".to_string(), "创业板".to_string()];

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.run();
        let _ = harness.get_by_label_contains("筛选条件");
        // "排除退市" appears twice: the card's type dropdown and its checkbox.
        let _ = harness.query_all_by_label_contains("排除退市").count();
        // "全部" appears three times: the industry/exchange/board multi-select
        // triggers of the six preset cards.
        let _ = harness.query_all_by_label_contains("全部").count();
    }

    #[test]
    fn filter_button_click_sets_loading() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, shared) = panel_with_form();
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.fit_contents();
        let btn = harness.get_by_label("筛选");
        btn.click();
        harness.step();

        assert!(
            shared.screener_loading.get(),
            "screener_loading should be set after filter click"
        );
    }

    #[test]
    fn filter_click_compresses_to_legacy_query_on_save() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let saved: std::sync::Arc<std::sync::Mutex<Option<ScreenerQuery>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let saved_clone = saved.clone();
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let mut panel = ScreenerPanel::new(
            id,
            state,
            None,
            Box::new(move |q: &ScreenerQuery| {
                *saved_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(q.clone());
            }),
            &tokens,
        );
        // Industry selection in the first preset card compresses to the legacy
        // query without loss (engine accept-grammar covers Meta nodes).
        if let CondItem::Leaf(l) = &mut panel.builder_root[0] {
            l.params = LeafParams::MultiSelect(vec!["白酒".to_string()]);
        }
        let shared = SharedState::new("SZ000001", "1d");
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.fit_contents();
        harness.get_by_label("筛选").click();
        harness.step();

        let q = saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("compressible builder state must invoke on_save");
        assert_eq!(q.industries, vec!["白酒".to_string()]);
        assert!(
            q.exclude_delisted,
            "preset Delisted card compresses to exclude"
        );
    }

    #[test]
    fn filter_click_uncompressible_state_shows_hint_without_save() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let saved = std::sync::Arc::new(std::sync::Mutex::new(false));
        let saved_clone = saved.clone();
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let mut panel = ScreenerPanel::new(
            id,
            state,
            None,
            Box::new(move |_: &ScreenerQuery| {
                *saved_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
            }),
            &tokens,
        );
        // UpDays is outside the engine accept-grammar (Batch 3): the run
        // compiles, but the legacy save must refuse instead of dropping it.
        panel.builder_root.push(CondItem::Leaf(CondLeaf {
            kind: LeafKind::UpDays,
            params: LeafParams::UpDays { n: 3, min_pct: 0.0 },
            negated: false,
        }));
        let shared = SharedState::new("SZ000001", "1d");
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.fit_contents();
        harness.get_by_label("筛选").click();
        harness.step();

        assert!(
            !*saved
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            "uncompressible builder state must NOT invoke on_save"
        );
        assert!(
            shared
                .screener_error
                .get()
                .is_some_and(|e| e.contains("无法保存")),
            "uncompressible save must surface the unsupported_save hint"
        );
    }

    // ------------------------------------------------------------------
    // Results table (S8 DataTable migration)
    // ------------------------------------------------------------------

    fn sample_row(symbol: &str, name: &str, cap: f64) -> compass_types::ScreenerRow {
        compass_types::ScreenerRow {
            symbol: symbol.to_string(),
            name: name.to_string(),
            latest_price: 10.0,
            change_20d: 5.0,
            market_cap: cap,
            industry: "银行".to_string(),
        }
    }

    fn signals() -> (
        egui_mobius::signals::Signal<RunScreenerRequest>,
        egui_mobius::signals::Signal<FetchRequest>,
    ) {
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        (run_signal, work_signal)
    }

    #[test]
    fn results_table_renders_rows_and_count() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(3);
        shared.screener_result.set(vec![
            sample_row("SZ000001", "平安银行", 100.0),
            sample_row("SH600519", "贵州茅台", 200.0),
            sample_row("SZ000002", "万科A", 50.0),
        ]);
        let (run_signal, work_signal) = signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.fit_contents();
        harness.step();
        let _ = harness.get_by_label_contains("共 3 行");
    }

    #[test]
    fn results_table_shows_empty_state_without_rows() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal) = signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.run();
        let _ = harness.get_by_label("无符合条件");
    }

    #[test]
    fn results_table_zero_market_cap_renders_without_panic() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(1);
        shared
            .screener_result
            .set(vec![sample_row("SZ000001", "平安银行", 0.0)]);
        let (run_signal, work_signal) = signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.step();
    }

    #[test]
    fn row_cells_map_screener_row_to_data_cells() {
        let cells = ScreenerPanel::row_cells(&sample_row("SH600519", "贵州茅台", 200.0));
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0], DataCell::Text("SH600519".to_string()));
        assert_eq!(
            cells[2],
            DataCell::Price {
                value: 10.0,
                change: None
            }
        );
        assert_eq!(
            cells[3],
            DataCell::Price {
                value: 5.0,
                change: Some(5.0)
            },
            "20-day change renders as a signed price cell (red-up/green-down)"
        );
        assert_eq!(cells[4], DataCell::Count(200));
    }

    #[test]
    fn dispatch_row_fetch_sets_symbol_and_triggers_fetch() {
        let shared = SharedState::new("SZ000001", "1d");
        // The work slot must stay alive so the signal send succeeds.
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let rows = vec![sample_row("SH600519", "贵州茅台", 200.0)];

        dispatch_row_fetch(&shared, &work_signal, &rows, 0);

        assert_eq!(shared.symbol.get(), "SH600519");
        assert!(
            shared.loading.get(),
            "row click must dispatch a FetchBars request"
        );
    }

    #[test]
    fn dispatch_row_fetch_ignores_out_of_range_index() {
        let shared = SharedState::new("SZ000001", "1d");
        let (_, work_signal) = signals();
        let rows = vec![sample_row("SH600519", "贵州茅台", 200.0)];

        dispatch_row_fetch(&shared, &work_signal, &rows, 5);

        assert_eq!(
            shared.symbol.get(),
            "SZ000001",
            "out-of-range index is a no-op"
        );
    }

    // ------------------------------------------------------------------
    // #220 atomic condition groups: each card's label+control stays on one
    // row (migrated from the fixed-form GROUP_ALIGNMENT series)
    // ------------------------------------------------------------------

    /// Widths swept by the alignment test. 500 px is a stress case below the
    /// design's supported minimum (>600 px): it must still keep each card's
    /// type dropdown and parameter control on the same row at ANY width.
    const GROUP_ALIGNMENT_WIDTHS: [f32; 5] = [500.0, 600.0, 800.0, 1000.0, 1200.0];

    fn assert_same_row_contains(
        harness: &egui_kittest::Harness<'_, ()>,
        label_fragment: &str,
        control: &egui_kittest::Node<'_>,
        width: f32,
    ) {
        // Take the first matching node in document order (the card's type
        // dropdown renders before its parameter controls, so "MA" resolves to
        // the type dropdown even though "Above MA20" also contains "MA").
        let label_node = harness
            .query_all_by_label_contains(label_fragment)
            .next()
            .unwrap_or_else(|| panic!("label fragment {label_fragment:?} not found"));
        let dy = (label_node.rect().center().y - control.rect().center().y).abs();
        assert!(
            dy <= 1.0,
            "label {label_fragment:?} and its control must share a row at width {width}px, dy={dy}"
        );
    }

    fn builder_harness(panel: &mut ScreenerPanel, width: f32) -> egui_kittest::Harness<'_, ()> {
        egui_kittest::Harness::builder()
            .with_size([width, 600.0])
            .build_ui(|ui| panel.condition_builder(ui, &[], &[]))
    }

    #[test]
    fn builder_card_rows_keep_label_and_control_aligned_across_widths() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for width in GROUP_ALIGNMENT_WIDTHS {
            let (mut panel, _shared) = panel_with_form();
            let mut harness = builder_harness(&mut panel, width);
            harness.run();

            let selects = harness
                .query_all_by_label_contains("全部")
                .collect::<Vec<_>>();
            assert_eq!(
                selects.len(),
                3,
                "three multi-select triggers rendered at width {width}px"
            );
            assert_same_row_contains(&harness, "行业", &selects[0], width);
            assert_same_row_contains(&harness, "交易所", &selects[1], width);
            assert_same_row_contains(&harness, "板块", &selects[2], width);

            let years = harness
                .query_by_label_contains("不限")
                .expect("上市时长 dropdown rendered");
            assert_same_row_contains(&harness, "上市时长", &years, width);
        }
    }

    #[test]
    fn builder_technical_cards_keep_label_and_control_aligned_across_widths() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for width in GROUP_ALIGNMENT_WIDTHS {
            let (mut panel, _shared) = panel_with_form();
            panel.builder_root = vec![
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Ma,
                    params: LeafParams::Ma(MaKind::AboveMa20),
                    negated: false,
                }),
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Breakout,
                    params: LeafParams::Breakout(60),
                    negated: false,
                }),
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Momentum,
                    params: LeafParams::Momentum {
                        days: 30,
                        min_pct: -5.0,
                        max_pct: 50.0,
                    },
                    negated: false,
                }),
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::VolumeSurge,
                    params: LeafParams::VolumeSurge {
                        days: 10,
                        times: 1.5,
                    },
                    negated: false,
                }),
            ];
            let mut harness = builder_harness(&mut panel, width);
            harness.run();

            let ma_dropdown = harness
                .query_by_label_contains("站上 MA20")
                .expect("MA kind dropdown rendered");
            assert_same_row_contains(&harness, "均线", &ma_dropdown, width);

            let n_labels = harness
                .query_all_by_label_contains("N:")
                .collect::<Vec<_>>();
            assert_eq!(
                n_labels.len(),
                3,
                "three N: parameter labels rendered at width {width}px"
            );
            assert_same_row_contains(&harness, "突破新高", &n_labels[0], width);
            assert_same_row_contains(&harness, "动量", &n_labels[1], width);
            assert_same_row_contains(&harness, "量能", &n_labels[2], width);
        }
    }

    #[test]
    fn builder_cards_wrap_to_own_rows_on_narrow_width() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut panel, _shared) = panel_with_form();
        let mut harness = egui_kittest::Harness::builder()
            .with_size([500.0, 600.0])
            .build_ui(|ui| panel.condition_builder(ui, &[], &[]));
        harness.run();

        let selects = harness
            .query_all_by_label_contains("全部")
            .collect::<Vec<_>>();
        let years = harness
            .query_by_label_contains("不限")
            .expect("上市时长 dropdown rendered");

        let dy_between = (selects[0].rect().center().y - years.rect().center().y).abs();
        assert!(
            dy_between > 1.0,
            "industry and 上市时长 cards must wrap to different rows at 500px, dy={dy_between}"
        );
    }

    // ------------------------------------------------------------------
    // #222 i18n (T15): the same alignment sweeps must hold in English —
    // wider en labels (Industry/Exchange/…) must not push the control off
    // the row. Each test holds LANG_LOCK so it is serialized against the
    // zh sweeps and the en-locale tests in other modules.
    // ------------------------------------------------------------------

    #[test]
    fn en_builder_card_rows_keep_label_and_control_aligned_across_widths() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for width in GROUP_ALIGNMENT_WIDTHS {
            let (mut panel, _shared) = panel_with_form();
            compass_i18n::set_locale("en");
            let mut harness = builder_harness(&mut panel, width);
            harness.run();

            let selects = harness
                .query_all_by_label_contains("All")
                .collect::<Vec<_>>();
            assert_eq!(
                selects.len(),
                3,
                "three multi-select triggers rendered at width {width}px"
            );
            assert_same_row_contains(&harness, "Industry", &selects[0], width);
            assert_same_row_contains(&harness, "Exchange", &selects[1], width);
            assert_same_row_contains(&harness, "Board", &selects[2], width);

            let years = harness
                .query_by_label_contains("Any")
                .expect("上市时长 dropdown rendered in en");
            assert_same_row_contains(&harness, "Listed ≥", &years, width);
        }
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn en_builder_technical_cards_keep_label_and_control_aligned_across_widths() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for width in GROUP_ALIGNMENT_WIDTHS {
            let (mut panel, _shared) = panel_with_form();
            compass_i18n::set_locale("en");
            panel.builder_root = vec![
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Ma,
                    params: LeafParams::Ma(MaKind::AboveMa20),
                    negated: false,
                }),
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Breakout,
                    params: LeafParams::Breakout(60),
                    negated: false,
                }),
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Momentum,
                    params: LeafParams::Momentum {
                        days: 30,
                        min_pct: -5.0,
                        max_pct: 50.0,
                    },
                    negated: false,
                }),
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::VolumeSurge,
                    params: LeafParams::VolumeSurge {
                        days: 10,
                        times: 1.5,
                    },
                    negated: false,
                }),
            ];
            let mut harness = builder_harness(&mut panel, width);
            harness.run();

            let ma_dropdown = harness
                .query_by_label_contains("Above MA20")
                .expect("MA kind dropdown rendered in en");
            assert_same_row_contains(&harness, "MA", &ma_dropdown, width);

            let n_labels = harness
                .query_all_by_label_contains("N:")
                .collect::<Vec<_>>();
            assert_eq!(
                n_labels.len(),
                3,
                "three N: parameter labels rendered at width {width}px"
            );
            assert_same_row_contains(&harness, "New High", &n_labels[0], width);
            assert_same_row_contains(&harness, "Momentum", &n_labels[1], width);
            assert_same_row_contains(&harness, "Volume", &n_labels[2], width);
        }
        compass_i18n::set_locale("zh");
    }
}
