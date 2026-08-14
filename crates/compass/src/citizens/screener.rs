//! Screener panel citizen — Metabase-style condition card builder + results table.
//!
//! The builder (epic #243 Batch 2, issue #245) replaces the fixed
//! `ConditionForm` with an AND/OR card group that operates directly on the
//! Batch 1 `Filter` AST. The view model lives in `screener_builder`; this
//! module owns the widget state (card items + MultiSelect popup instances)
//! and renders the card group tree.

use std::collections::HashMap;

use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use compass_types::{BreakoutCondition, Filter, MetaCond, MomentumCondition, VolumeCondition};
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
use crate::messages::{FetchRequest, RunLlmRequest, RunScreenerRequest};
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
    on_save: Box<dyn Fn(&Filter) + Send + Sync>,
    /// Whether the LLM natural-language entry is rendered (API key present
    /// at startup — design §1, issue #247).
    llm_enabled: bool,
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
    /// is invoked with the current filter whenever the filter runs.
    pub fn new(
        citizen_id: CitizenId,
        citizen_state: CitizenState,
        restore: Option<&Filter>,
        on_save: Box<dyn Fn(&Filter) + Send + Sync>,
        tokens: &ThemeTokens,
        llm_enabled: bool,
    ) -> Self {
        // Restore of the default empty shape (bare `Delisted(false)` node or
        // empty `And` — the `From<ScreenerQuery>` outputs of an empty query)
        // seeds the standard 6 base cards, matching the pre-builder default
        // behavior (exclude-delisted checked, everything else unbounded).
        let (builder_root, builder_multi_selects) = match restore {
            None => (default_root_cards(), HashMap::new()),
            Some(filter) => match filter {
                Filter::Meta(MetaCond::Delisted(false)) => (default_root_cards(), HashMap::new()),
                Filter::And(v) if v.is_empty() => (default_root_cards(), HashMap::new()),
                _ => (filter_to_items(filter), HashMap::new()),
            },
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
            llm_enabled,
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
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        run_screener_signal: &Signal<RunScreenerRequest>,
        work_signal: &Signal<FetchRequest>,
        industries: &[String],
        boards: &[String],
        llm_signal: &Signal<RunLlmRequest>,
    ) {
        self.consume_llm_result(shared_state);
        ui.vertical(|ui| {
            self.condition_builder(ui, shared_state, llm_signal, industries, boards);

            ui.add_space(self.form_tokens().spacing.sm);
            if Button::new(&self.form_tokens(), compass_i18n::t!("screener.filter"))
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Md)
                .show(ui)
                .clicked()
            {
                let filter = self.build_filter();
                shared_state.screener_loading.set(true);
                // Clear the previous run error before saving: the save hint
                // below must survive the whole run (the toast layer in
                // main.rs pushes it on the None→Some transition).
                shared_state.screener_error.set(None);
                // Persist the Filter AST directly — the engine evaluates any
                // AST shape (issue #246), so no legacy compressibility oracle
                // is needed and no combination is unsaved.
                (self.on_save)(&filter);
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
            // A run/data error surfaces as the engine's Display text.
            ui.colored_label(ui.visuals().error_fg_color, err);
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
    fn condition_builder(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        llm_signal: &Signal<RunLlmRequest>,
        industries: &[String],
        boards: &[String],
    ) {
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
                    if self.llm_enabled {
                        self.render_llm_entry(ui, shared_state, llm_signal);
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

    /// Merge a pending LLM-generated filter into the builder root once the
    /// generation finished (design §3). Runs every frame before rendering.
    fn consume_llm_result(&mut self, shared_state: &SharedState) {
        if shared_state.llm_loading.get() {
            return;
        }
        if let Some(generated) = shared_state.llm_result.get() {
            llm_merge_into_root(
                &mut self.builder_root,
                self.builder_root_operator,
                generated,
            );
            shared_state.llm_result.set(None);
        }
    }

    /// The LLM natural-language entry (design §1): input + generate button,
    /// inline error line, Enter-to-submit and Esc-to-cancel while loading.
    fn render_llm_entry(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        llm_signal: &Signal<RunLlmRequest>,
    ) {
        let tokens = self.form_tokens();
        let loading = shared_state.llm_loading.get();
        ui.add_space(tokens.spacing.sm);

        let mut submit = false;
        ui.horizontal(|ui| {
            let mut input = shared_state.llm_input.get();
            let input_w = (ui.available_width() - 120.0 - tokens.spacing.md).max(200.0);
            let input_resp = ui
                .add_enabled_ui(!loading, |ui| {
                    compass_ui::widgets::input::Input::new(&tokens, &mut input)
                        .placeholder(&compass_i18n::t!("screener.llm.placeholder"))
                        .prefix_icon(egui_phosphor::regular::LIGHTNING)
                        .width(input_w)
                        .show(ui)
                })
                .inner;
            let enter_pressed =
                input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            shared_state.llm_input.set(input);

            let empty = shared_state.llm_input.get().trim().is_empty();
            let label = if loading {
                compass_i18n::t!("screener.llm.generating")
            } else {
                compass_i18n::t!("screener.llm.generate")
            };
            let clicked = ui
                .add_enabled_ui(!empty && !loading, |ui| {
                    Button::new(&tokens, label)
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Md)
                        .loading(loading)
                        .show(ui)
                })
                .inner
                .clicked();
            if !empty && !loading && (clicked || enter_pressed) {
                submit = true;
            }
        });

        if submit {
            let seq = shared_state.llm_seq.get() + 1;
            shared_state.llm_seq.set(seq);
            shared_state.llm_loading.set(true);
            shared_state.llm_error.set(None);
            let _ = llm_signal.send(RunLlmRequest {
                prompt: shared_state.llm_input.get(),
                seq,
            });
        }

        if loading && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            shared_state.llm_seq.set(shared_state.llm_seq.get() + 1);
            shared_state.llm_loading.set(false);
            shared_state.llm_error.set(None);
        }

        if let Some(err) = shared_state.llm_error.get() {
            ui.add_space(tokens.spacing.xs);
            ui.colored_label(ui.visuals().error_fg_color, err);
        }
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

/// Merge a generated filter into the root card group (design §2).
///
/// And-flattens when both the root operator and the generated shape are AND
/// (associativity: `And[And[a,b], ...]` == `And[a,b,...]`), keeping the common
/// "A 且 B 且 C" case as peer cards; Or/Not/nested shapes stay as subgroups.
/// Non-destructive: existing cards are never replaced or removed.
fn llm_merge_into_root(root_items: &mut Vec<CondItem>, root_op: BoolOp, generated: Filter) {
    let items = filter_to_items(&generated);
    if root_op == BoolOp::And
        && items.len() == 1
        && let CondItem::Group(g) = &items[0]
        && g.operator == BoolOp::And
    {
        root_items.extend(g.items.clone());
    } else {
        root_items.extend(items);
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
                    // frame does not begin mid-row after preceding cards. The
                    // scope's bottom is the available viewport bottom — a
                    // finite bound so wrap layout can center content (infinite
                    // height made every internal rect NaN and killed clicks).
                    if ui.available_size_before_wrap().x < 320.0 {
                        ui.end_row();
                    }
                    let start = ui.cursor().min;
                    let row_w = ui.available_size_before_wrap().x;
                    let bottom = ui.available_rect_before_wrap().bottom();
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                            start,
                            egui::pos2(start.x + row_w, bottom),
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
    use crate::messages::RunLlmRequest;
    use compass_types::{CmpOp, FactorRef, MaCondition, ScreenerQuery, SeriesCond, SeriesFactor};
    use compass_ui::tokens::ThemeTokens;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    fn panel_with_form() -> (ScreenerPanel, SharedState) {
        rust_i18n::set_locale("zh");
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let panel = ScreenerPanel::new(id, state, None, Box::new(|_| {}), &tokens, false);
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
        let panel = ScreenerPanel::new(
            id,
            state,
            Some(&Filter::from(query)),
            Box::new(|_| {}),
            &tokens,
            false,
        );

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
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        let industries = vec!["银行".to_string(), "白酒".to_string()];
        let boards = vec!["主板".to_string(), "创业板".to_string()];

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
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
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
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
    fn filter_click_saves_filter_ast() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let saved: std::sync::Arc<std::sync::Mutex<Option<Filter>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let saved_clone = saved.clone();
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let mut panel = ScreenerPanel::new(
            id,
            state,
            None,
            Box::new(move |f: &Filter| {
                *saved_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f.clone());
            }),
            &tokens,
            false,
        );
        // Industry selection in the first preset card is saved as the Filter
        // AST node (no legacy compression oracle).
        if let CondItem::Leaf(l) = &mut panel.builder_root[0] {
            l.params = LeafParams::MultiSelect(vec!["白酒".to_string()]);
        }
        let shared = SharedState::new("SZ000001", "1d");
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
        });
        harness.fit_contents();
        harness.get_by_label("筛选").click();
        harness.step();
        drop(harness);

        let f = saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("clicking 筛选 must invoke on_save with the filter");
        assert!(
            f == panel.build_filter(),
            "saved filter equals the builder-compiled AST"
        );
        match f {
            Filter::And(nodes) => {
                assert!(
                    nodes.contains(&Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()]))),
                    "industry selection is part of the saved AST"
                );
                assert!(
                    nodes.contains(&Filter::Meta(MetaCond::Delisted(false))),
                    "preset Delisted card is part of the saved AST"
                );
            }
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn filter_click_saves_up_days_combination() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let saved: std::sync::Arc<std::sync::Mutex<Option<Filter>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let saved_clone = saved.clone();
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let mut panel = ScreenerPanel::new(
            id,
            state,
            None,
            Box::new(move |f: &Filter| {
                *saved_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f.clone());
            }),
            &tokens,
            false,
        );
        // UpDays was previously unsaveable (outside the legacy accept-
        // grammar); the AST persistence path saves any combination (issue
        // #246).
        panel.builder_root.push(CondItem::Leaf(CondLeaf {
            kind: LeafKind::UpDays,
            params: LeafParams::UpDays { n: 3, min_pct: 0.0 },
            negated: false,
        }));
        let shared = SharedState::new("SZ000001", "1d");
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
        });
        harness.fit_contents();
        harness.get_by_label("筛选").click();
        harness.step();
        drop(harness);

        let f = saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("UpDays combination must save");
        assert!(
            f == panel.build_filter(),
            "saved filter equals the builder-compiled AST"
        );
        assert!(
            shared.screener_error.get().is_none(),
            "no unsaved-state hint for an UpDays combination"
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
        egui_mobius::signals::Signal<RunLlmRequest>,
    ) {
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        (run_signal, work_signal, llm_signal)
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
        let (run_signal, work_signal, llm_signal) = signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
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
        let (run_signal, work_signal, llm_signal) = signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
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
        let (run_signal, work_signal, llm_signal) = signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(
                ui,
                &shared,
                &run_signal,
                &work_signal,
                &industries,
                &boards,
                &llm_signal,
            );
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
        let (_, work_signal, _) = signals();
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

    fn builder_harness<'a>(
        panel: &'a mut ScreenerPanel,
        shared: &'a SharedState,
        llm_signal: &'a Signal<RunLlmRequest>,
        width: f32,
    ) -> egui_kittest::Harness<'a, ()> {
        egui_kittest::Harness::builder()
            .with_size([width, 600.0])
            .build_ui(|ui| panel.condition_builder(ui, shared, llm_signal, &[], &[]))
    }

    #[test]
    fn builder_card_rows_keep_label_and_control_aligned_across_widths() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for width in GROUP_ALIGNMENT_WIDTHS {
            let (mut panel, shared) = panel_with_form();
            let (llm_signal, _llm_slot) =
                egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
            let mut harness = builder_harness(&mut panel, &shared, &llm_signal, width);
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
            let (mut panel, shared) = panel_with_form();
            let (llm_signal, _llm_slot) =
                egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
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
            let mut harness = builder_harness(&mut panel, &shared, &llm_signal, width);
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
        let (mut panel, shared) = panel_with_form();
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        let mut harness = egui_kittest::Harness::builder()
            .with_size([500.0, 600.0])
            .build_ui(|ui| panel.condition_builder(ui, &shared, &llm_signal, &[], &[]));
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
            let (mut panel, shared) = panel_with_form();
            let (llm_signal, _llm_slot) =
                egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
            compass_i18n::set_locale("en");
            let mut harness = builder_harness(&mut panel, &shared, &llm_signal, width);
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
            let (mut panel, shared) = panel_with_form();
            let (llm_signal, _llm_slot) =
                egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
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
            let mut harness = builder_harness(&mut panel, &shared, &llm_signal, width);
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

    // ------------------------------------------------------------------
    // #245 Todo 5: interactive path tests — real clicks drive the builder
    // and the UI state / compiled Filter AST is asserted afterwards.
    // Dropdown popups follow the widget's own harness pattern
    // (compass-ui dropdown.rs): click trigger → step (popup opens) →
    // click option → step (selection applied, popup closes). Single
    // `step()`s are used instead of `run()` because the panel schedules
    // a delayed repaint while popups animate, which trips `run()`'s
    // max_steps guard.
    // ------------------------------------------------------------------

    /// Full-panel harness with a fixed tall window. `fit_contents` would
    /// under-size the window for nested-group layouts (the group frame's
    /// unbounded-height `scope_builder` skews the content-rect estimate),
    /// pushing bottom-anchored widgets (add-menu popups, the 筛选 button)
    /// outside the window where their clicks are silently dropped. The
    /// returned harness borrows `panel` mutably for its lifetime — drop it
    /// before asserting on panel state.
    fn panel_harness<'a>(
        panel: &'a mut ScreenerPanel,
        shared: &'a SharedState,
        run_signal: &'a egui_mobius::signals::Signal<RunScreenerRequest>,
        work_signal: &'a egui_mobius::signals::Signal<FetchRequest>,
        llm_signal: &'a egui_mobius::signals::Signal<RunLlmRequest>,
        industries: &'a [String],
        boards: &'a [String],
    ) -> egui_kittest::Harness<'a, ()> {
        let mut harness = egui_kittest::Harness::builder()
            .with_size([1000.0, 1400.0])
            .build_ui(|ui| {
                panel.show(
                    ui,
                    shared,
                    run_signal,
                    work_signal,
                    industries,
                    boards,
                    llm_signal,
                );
            });
        harness.step();
        harness
    }

    fn builder_signals() -> (
        egui_mobius::signals::Signal<RunScreenerRequest>,
        egui_mobius::signals::Signal<FetchRequest>,
        egui_mobius::signals::Signal<RunLlmRequest>,
    ) {
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        (run_signal, work_signal, llm_signal)
    }

    #[test]
    fn add_condition_via_root_menu_appends_card() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        // Root add menu trigger (unique: default 6 cards contain no sub-group).
        harness.get_by_label_contains("添加条件").click();
        harness.step();
        // Popup option「均线」— unique: no MA card exists yet (「行业」would
        // multi-match the first card's type dropdown and panic).
        harness.get_by_label("均线").click();
        harness.step();
        drop(harness);

        assert_eq!(
            panel.builder_root.len(),
            7,
            "add menu appends a card to the root group"
        );
        match &panel.builder_root[6] {
            CondItem::Leaf(l) => assert_eq!(
                l.kind,
                LeafKind::Ma,
                "appended card must be the selected MA kind"
            ),
            other => panic!("last item must be a leaf card, got {other:?}"),
        }
        match panel.build_filter() {
            Filter::And(nodes) => assert!(
                nodes.contains(&Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::Close,
                    op: CmpOp::Gt,
                    value: FactorRef::Factor(SeriesFactor::Sma(20)),
                })),
                "fresh MA card compiles to Close > Sma(20)"
            ),
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn delete_first_card_removes_card_and_ast_node() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        let del: Vec<_> = harness
            .query_all_by_label(egui_phosphor::regular::X)
            .collect();
        assert_eq!(del.len(), 6, "one delete button per default card");
        del[0].click();
        harness.step();
        drop(harness);

        assert_eq!(panel.builder_root.len(), 5, "first card removed");
        match panel.build_filter() {
            Filter::And(nodes) => assert!(
                !nodes.contains(&Filter::Meta(MetaCond::Industry(vec![]))),
                "deleted industry card must not appear in the AST"
            ),
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn root_operator_segmented_toggles_or_and_back() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        // Root group header segmented — unique with no sub-groups.
        harness.get_by_label_contains("或 (OR)").click();
        harness.step();
        drop(harness);
        assert_eq!(
            panel.builder_root_operator,
            BoolOp::Or,
            "segmented OR click must flip the root operator"
        );
        assert!(
            matches!(panel.build_filter(), Filter::Or(_)),
            "root operator must compile to an Or root"
        );

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        harness.get_by_label_contains("且 (AND)").click();
        harness.step();
        drop(harness);
        assert_eq!(
            panel.builder_root_operator,
            BoolOp::And,
            "segmented AND click must flip back"
        );
        assert!(
            matches!(panel.build_filter(), Filter::And(_)),
            "root operator must compile back to an And root"
        );
    }

    #[test]
    fn clear_button_empties_builder_and_shows_empty_state() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        harness.get_by_label(egui_phosphor::regular::ERASER).click();
        harness.step();
        assert!(
            harness.query_by_label_contains("暂无筛选条件").is_some(),
            "cleared builder must show the empty state title"
        );
        drop(harness);
        assert!(
            panel.builder_root.is_empty(),
            "clear button must empty the root group"
        );
    }

    #[test]
    fn add_sub_group_and_nested_cards_compile_nested_and() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        harness.get_by_label_contains("添加条件").click();
        harness.step();
        harness.get_by_label("子分组").click();
        harness.step();
        drop(harness);
        match &panel.builder_root[6] {
            CondItem::Group(g) => {
                assert_eq!(g.operator, BoolOp::And, "fresh sub-group defaults to AND");
                assert!(g.items.is_empty(), "fresh sub-group has no cards");
            }
            other => panic!("expected a nested group, got {other:?}"),
        }
        match panel.build_filter() {
            Filter::And(nodes) => assert_eq!(
                nodes[6],
                Filter::And(vec![]),
                "empty sub-group compiles to a nested And node"
            ),
            other => panic!("expected And root, got {other:?}"),
        }

        // kittest cannot reliably click a second popup inside the nested
        // sub-group scope (the first in-group popup works, the second click
        // is swallowed by the Area close-on-outside logic — see
        // kb/dev/toolchain.md). Add the two cards via the view model instead;
        // the in-group popup interaction itself is covered by the root add
        // menu in `add_condition_via_root_menu_appends_card`.
        if let CondItem::Group(g) = &mut panel.builder_root[6] {
            g.items.push(CondItem::Leaf(CondLeaf {
                kind: LeafKind::Ma,
                params: LeafParams::Ma(MaKind::AboveMa20),
                negated: false,
            }));
            g.items.push(CondItem::Leaf(CondLeaf {
                kind: LeafKind::Breakout,
                params: LeafParams::Breakout(60),
                negated: false,
            }));
        } else {
            panic!("expected a nested group");
        }
        match panel.build_filter() {
            Filter::And(nodes) => assert_eq!(
                nodes[6],
                Filter::And(vec![
                    Filter::Series(SeriesCond::Cmp {
                        factor: SeriesFactor::Close,
                        op: CmpOp::Gt,
                        value: FactorRef::Factor(SeriesFactor::Sma(20)),
                    }),
                    Filter::Series(SeriesCond::Cmp {
                        factor: SeriesFactor::Close,
                        op: CmpOp::Gt,
                        value: FactorRef::Factor(SeriesFactor::NDayHigh(60)),
                    }),
                ]),
                "sub-group with two cards compiles to a nested And"
            ),
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn restore_query_renders_cards_and_filter_saves_equivalent_filter() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let tokens = ThemeTokens::dark();

        // Phase A — the multi-member restore shape (task spec): Industry +
        // MA bullish + Delisted seed one nested root group of three cards.
        let query = ScreenerQuery {
            industries: vec!["银行".to_string()],
            ma: Some(MaCondition::BullishAlign),
            ..ScreenerQuery::default()
        };
        let panel = ScreenerPanel::new(
            CitizenId::new("screener"),
            CitizenState::new(),
            Some(&Filter::from(query)),
            Box::new(|_| {}),
            &tokens,
            false,
        );
        assert_eq!(
            panel.builder_root.len(),
            1,
            "multi-member restore seeds a root group"
        );
        match &panel.builder_root[0] {
            CondItem::Group(g) => assert_eq!(
                g.items.len(),
                3,
                "Industry + MA bullish + Delisted restore as three cards"
            ),
            other => panic!("expected a nested root group, got {other:?}"),
        }

        // Phase B — flat industries-only restore: clicking 筛选 must save
        // the equivalent Filter AST.
        let saved_b: std::sync::Arc<std::sync::Mutex<Option<Filter>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let saved_b_clone = saved_b.clone();
        let query_b = ScreenerQuery {
            industries: vec!["银行".to_string()],
            exclude_delisted: false,
            ..ScreenerQuery::default()
        };
        let mut panel_b = ScreenerPanel::new(
            CitizenId::new("screener"),
            CitizenState::new(),
            Some(&Filter::from(query_b)),
            Box::new(move |f: &Filter| {
                *saved_b_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f.clone());
            }),
            &tokens,
            false,
        );
        assert_eq!(
            panel_b.builder_root.len(),
            1,
            "single-member restore folds to a flat card"
        );
        let shared_b = SharedState::new("SZ000001", "1d");
        let (run_signal_b, work_signal_b, llm_signal_b) = builder_signals();
        let industries_b: Vec<String> = Vec::new();
        let boards_b: Vec<String> = Vec::new();
        let mut harness_b = panel_harness(
            &mut panel_b,
            &shared_b,
            &run_signal_b,
            &work_signal_b,
            &llm_signal_b,
            &industries_b,
            &boards_b,
        );
        harness_b.get_by_label("筛选").click();
        harness_b.step();
        drop(harness_b);
        let f_b = saved_b
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("flat restore must save the equivalent filter");
        assert_eq!(
            f_b,
            Filter::Meta(MetaCond::Industry(vec!["银行".to_string()])),
            "restored industry selection survives the save round trip"
        );

        // Phase C — flat MA-only restore: the bullish-alignment card must
        // survive the save round trip.
        let saved_c: std::sync::Arc<std::sync::Mutex<Option<Filter>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let saved_c_clone = saved_c.clone();
        let query_c = ScreenerQuery {
            ma: Some(MaCondition::BullishAlign),
            exclude_delisted: false,
            ..ScreenerQuery::default()
        };
        let mut panel_c = ScreenerPanel::new(
            CitizenId::new("screener"),
            CitizenState::new(),
            Some(&Filter::from(query_c)),
            Box::new(move |f: &Filter| {
                *saved_c_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f.clone());
            }),
            &tokens,
            false,
        );
        match &panel_c.builder_root[0] {
            CondItem::Leaf(l) => assert_eq!(
                l.params,
                LeafParams::Ma(MaKind::BullishAlign),
                "bullish pair folds to a single MA card"
            ),
            other => panic!("expected a flat MA card, got {other:?}"),
        }
        let shared_c = SharedState::new("SZ000001", "1d");
        let (run_signal_c, work_signal_c, llm_signal_c) = builder_signals();
        let industries_c: Vec<String> = Vec::new();
        let boards_c: Vec<String> = Vec::new();
        let mut harness_c = panel_harness(
            &mut panel_c,
            &shared_c,
            &run_signal_c,
            &work_signal_c,
            &llm_signal_c,
            &industries_c,
            &boards_c,
        );
        harness_c.get_by_label("筛选").click();
        harness_c.step();
        drop(harness_c);
        let f_c = saved_c
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .expect("flat MA restore must save the equivalent filter");
        assert_eq!(
            f_c,
            Filter::And(vec![
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
            ]),
            "restored bullish alignment survives the save round trip"
        );
    }

    #[test]
    fn negate_toggle_wraps_leaf_in_not() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        let neg: Vec<_> = harness
            .query_all_by_label(egui_phosphor::regular::EXCLUDE)
            .collect();
        assert_eq!(neg.len(), 6, "one negate button per default card");
        neg[0].click();
        harness.step();
        drop(harness);

        match &panel.builder_root[0] {
            CondItem::Leaf(l) => assert!(l.negated, "negate toggle must flip the leaf"),
            other => panic!("card 0 must be a leaf, got {other:?}"),
        }
        match panel.build_filter() {
            Filter::And(nodes) => assert_eq!(
                nodes[0],
                Filter::Not(Box::new(Filter::Meta(MetaCond::Industry(vec![])))),
                "negated industry card compiles to Not(Industry)"
            ),
            other => panic!("expected And root, got {other:?}"),
        }
    }

    #[test]
    fn unknown_shape_renders_readonly_summary_and_deletes() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = panel_with_form();
        // ScreenerQuery cannot express the Count shape — inject the unknown
        // AST directly through the reverse mapping (the production fallback).
        let unknown_filter = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 10,
            at_least: 5,
        });
        panel.builder_root = filter_to_items(&unknown_filter);
        assert_eq!(
            panel.builder_root.len(),
            1,
            "unrecognized shape falls back to a single Unknown card"
        );
        let (run_signal, work_signal, llm_signal) = builder_signals();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = panel_harness(
            &mut panel,
            &shared,
            &run_signal,
            &work_signal,
            &llm_signal,
            &industries,
            &boards,
        );
        assert!(
            harness.query_by_label_contains("高级条件").is_some(),
            "Unknown card renders its read-only summary title"
        );
        let del: Vec<_> = harness
            .query_all_by_label(egui_phosphor::regular::X)
            .collect();
        assert_eq!(del.len(), 1, "only the Unknown card has a delete button");
        del[0].click();
        harness.step();
        drop(harness);
        assert!(
            panel.builder_root.is_empty(),
            "deleting the Unknown card empties the builder"
        );
    }

    // --- LLM natural-language entry (epic #243 Batch 4, ref #247, Todo 6) ---
    //
    // RED phase: these tests compile only after the Todo 6 signatures land —
    // `ScreenerPanel::new(..., llm_enabled: bool)` and
    // `show(..., llm_signal: &Signal<RunLlmRequest>)` (design
    // .omo/designs/llm-screener-llm.md §8). i18n labels below follow the
    // design §6 zh values (screener.llm.placeholder / generate / generating).

    /// Panel with the LLM entry enabled/disabled per the constructor flag.
    fn llm_panel_with_form(llm_enabled: bool) -> (ScreenerPanel, SharedState) {
        rust_i18n::set_locale("zh");
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let panel = ScreenerPanel::new(id, state, None, Box::new(|_| {}), &tokens, llm_enabled);
        (panel, SharedState::new("SZ000001", "1d"))
    }

    /// Harness with the Todo 6 `show` signature (llm_signal added last).
    fn llm_panel_harness<'a>(
        panel: &'a mut ScreenerPanel,
        shared: &'a SharedState,
        llm_signal: &'a egui_mobius::signals::Signal<RunLlmRequest>,
    ) -> egui_kittest::Harness<'a, ()> {
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();
        let mut harness = egui_kittest::Harness::builder()
            .with_size([1000.0, 1400.0])
            .build_ui(move |ui| {
                panel.show(
                    ui,
                    shared,
                    &run_signal,
                    &work_signal,
                    &industries,
                    &boards,
                    llm_signal,
                );
            });
        harness.step();
        harness
    }

    #[test]
    fn llm_entry_visible_when_enabled() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = llm_panel_with_form(true);
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();

        let harness = llm_panel_harness(&mut panel, &shared, &llm_signal);

        let _ = harness.get_by_label("AI 生成");
        let _ = harness.get_by(|n| n.placeholder() == Some("用自然语言描述选股条件…"));
    }

    #[test]
    fn llm_entry_hidden_when_not_configured() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = llm_panel_with_form(false);
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();

        let harness = llm_panel_harness(&mut panel, &shared, &llm_signal);

        assert_eq!(
            harness.query_all_by_label("AI 生成").count(),
            0,
            "no generate button when LLM is not configured"
        );
        assert_eq!(
            harness
                .query_all_by(|n| n.placeholder() == Some("用自然语言描述选股条件…"))
                .count(),
            0,
            "no input row when LLM is not configured"
        );
    }

    #[test]
    fn llm_generate_click_sends_request_with_typed_prompt() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = llm_panel_with_form(true);
        let (llm_signal, mut llm_slot) =
            egui_mobius::factory::create_signal_slot::<RunLlmRequest>();
        let sent: std::sync::Arc<std::sync::Mutex<Option<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let sent_clone = sent.clone();
        llm_slot.start(move |req: RunLlmRequest| {
            *sent_clone
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(req.prompt);
        });

        let mut harness = llm_panel_harness(&mut panel, &shared, &llm_signal);
        let input = harness.get_by(|n| n.placeholder() == Some("用自然语言描述选股条件…"));
        input.focus();
        input.type_text("最近5天每天涨超3%");
        harness.step();
        harness.get_by_label("AI 生成").click();
        harness.step();
        drop(harness);

        let prompt = {
            let mut guard = sent
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for _ in 0..20 {
                if guard.is_some() {
                    break;
                }
                drop(guard);
                std::thread::sleep(std::time::Duration::from_millis(50));
                guard = sent
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            guard
                .clone()
                .expect("generate click must send RunLlmRequest")
        };
        assert_eq!(prompt, "最近5天每天涨超3%");
    }

    #[test]
    fn llm_loading_disables_generate_button() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = llm_panel_with_form(true);
        shared.llm_loading.set(true);
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();

        let harness = llm_panel_harness(&mut panel, &shared, &llm_signal);

        // Loading state must render a disabled generate control — the label
        // may switch to 生成中… (design §5) or stay AI 生成 with a spinner.
        let disabled_generate = harness
            .query_all_by(|n| {
                n.is_disabled()
                    && (n.label() == Some("AI 生成".to_string())
                        || n.label() == Some("生成中…".to_string()))
            })
            .count();
        assert_eq!(
            disabled_generate, 1,
            "generate button must be disabled while llm_loading is true"
        );
    }

    #[test]
    fn llm_error_renders_inline_message() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        rust_i18n::set_locale("zh");
        let (mut panel, shared) = llm_panel_with_form(true);
        shared.llm_error.set(Some("生成失败：测试错误".to_string()));
        let (llm_signal, _llm_slot) = egui_mobius::factory::create_signal_slot::<RunLlmRequest>();

        let harness = llm_panel_harness(&mut panel, &shared, &llm_signal);

        assert!(
            harness.query_all_by_label_contains("生成失败").count() >= 1,
            "llm_error must render as an inline message"
        );
    }
}
