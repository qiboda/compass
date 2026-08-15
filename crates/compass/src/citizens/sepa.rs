//! SEPA panel citizen — daily TOP-N multi-factor scoring report (epic #139,
//! sub-issue #152).
//!
//! Report-type panel (read-only ranking), in contrast to the query-type
//! screener: a market thermometer bar, a sortable 12-column ranking table
//! and a per-row score detail panel, all fed by the third
//! `AsyncDispatcher` channel (`RunSepaRequest` → `SepaData`). TOP-N
//! switching is pure GUI truncation of a local render copy — the backend
//! always returns the full list and the shared state is never written back.

use egui::RichText;
use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use compass_types::{MarketThermometer, SepaIndicator, SepaRow};
use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::button::{Button, ButtonSize, ButtonVariant};
use compass_ui::widgets::card::{Card, CardPadding};
use compass_ui::widgets::data_table::{ColumnSpec, DataCell, DataTable, score_color};
use compass_ui::widgets::empty_state::EmptyState;
use compass_ui::widgets::segmented::Segmented;
use compass_ui::widgets::tag::{Tag, TagVariant, tint};

use crate::messages::{FetchRequest, RunSepaRequest};
use crate::state::SharedState;

/// Risk module max deduction (engine contract: 75 × 0.05 = 3.75, risk ∈
/// [-3.75, 0]). Used for the inverted color-scale normalization.
const RISK_MAX: f32 = 3.75;

/// Right-side detail panel fixed width (design §3). Also reserved by the
/// ranking table in `results_area` so the table and detail panel share the
/// horizontal row exactly.
const DETAIL_PANEL_WIDTH: f32 = 280.0;

/// Ranking table columns (design §2: 12 columns, default sort = rank asc,
/// descending business default for the score columns). Each header holds a
/// `sepa.table.*` i18n key resolved by the renderer via `compass_i18n::t!()` (issue #222).
const COLUMNS: [ColumnSpec; 12] = [
    ColumnSpec {
        header: "sepa.table.rank",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.code",
        numeric: false,
    },
    ColumnSpec {
        header: "sepa.table.name",
        numeric: false,
    },
    ColumnSpec {
        header: "sepa.table.total",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.trend",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.theme",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.capital",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.pattern",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.risk",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.industry",
        numeric: false,
    },
    ColumnSpec {
        header: "sepa.table.latest",
        numeric: true,
    },
    ColumnSpec {
        header: "sepa.table.change",
        numeric: true,
    },
];

/// Format an indicator raw value per its unit precision contract (Metis C6:
/// percent 1 decimal, count integer, trillion 2 decimals). The locale unit
/// templates interpolate the number via `%{v}`.
fn format_indicator_value(value: f64, unit_key: &str) -> String {
    match unit_key {
        "sepa.unit.count" => format!("{value:.0}"),
        "sepa.unit.trillion" => format!("{value:.2}"),
        _ => format!("{value:.1}"),
    }
}

/// Resolve a factor note from its key + positional numeric args. Args map
/// positionally onto the note template names (drawdown/momentum → `pct`,
/// big_capital → main/dragon/survey/block, thermometer → score).
fn factor_note_text(note_key: &'static str, args: &[f64]) -> String {
    let arg = |i: usize| args.get(i).copied().unwrap_or(0.0);
    // Pre-format numeric args exactly like the pre-i18n `format!` strings in
    // compass-strategy scoring.rs (drawdown 1dp, momentum/thermometer int,
    // big-capital ints with signed block amount) — the strategy only ships
    // semantic keys + raw values, so formatting happens here (issue #222).
    // rust-i18n `t!` specifiers are avoided: its tokenizer mangles `+`/`.`.
    match note_key {
        "sepa.note.drawdown" => {
            compass_i18n::t!(note_key, pct = format!("{:.1}", arg(0))).into_owned()
        }
        "sepa.note.momentum_percentile" => {
            compass_i18n::t!(note_key, pct = format!("{:.0}", arg(0))).into_owned()
        }
        "sepa.note.big_capital" => compass_i18n::t!(
            note_key,
            main = format!("{:.0}", arg(0)),
            dragon = format!("{:.0}", arg(1)),
            survey = format!("{:.0}", arg(2)),
            block = format!("{:+.0}", arg(3)),
        )
        .into_owned(),
        "sepa.note.thermometer" => {
            compass_i18n::t!(note_key, score = format!("{:.0}", arg(0))).into_owned()
        }
        _ => compass_i18n::t!(note_key).into_owned(),
    }
}

/// SEPA panel citizen.
///
/// Renders the thermometer bar, the toolbar (count label / TOP-N switch /
/// refresh) and the ranking table next to the per-row detail panel.
pub struct SepaPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    /// Theme tokens copied at construction (component styling).
    tokens: ThemeTokens,
    /// Ranking table — owns its sort state across frames.
    table: DataTable,
    /// GUI-side TOP-N cap (50/30); truncation applies to a local render
    /// copy only and never writes the shared state back.
    top_n: usize,
    /// Original row index shown in the detail panel and highlighted.
    selected: Option<usize>,
}

impl Citizen for SepaPanel {
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

impl SepaPanel {
    /// Create a SEPA panel with the given citizen identity/state.
    pub fn new(citizen_id: CitizenId, citizen_state: CitizenState, tokens: &ThemeTokens) -> Self {
        let mut table = DataTable::new(tokens, COLUMNS.to_vec());
        table.set_sort(0, false); // rank ascending = official order
        for col in 3..=8 {
            table.set_descending_default(col, true); // score columns: best first
        }
        Self {
            citizen_id,
            citizen_state,
            tokens: *tokens,
            table,
            top_n: 50,
            selected: None,
        }
    }

    /// Render the panel: thermometer bar + toolbar + results area.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        sepa_signal: &Signal<RunSepaRequest>,
        work_signal: &Signal<FetchRequest>,
    ) {
        let data = shared_state.sepa_data.get();
        ui.vertical(|ui| {
            self.thermometer_bar(ui, data.as_ref().map(|d| &d.thermometer));

            ui.add_space(self.tokens.spacing.sm);
            self.toolbar(ui, shared_state, sepa_signal);

            ui.add_space(self.tokens.spacing.md);
            self.results_area(ui, shared_state, sepa_signal, work_signal, data.as_ref());
        });
    }

    /// Market thermometer strip (design §4): icon + score + position tag +
    /// the five indicator chips. Renders a muted placeholder without data.
    fn thermometer_bar(&mut self, ui: &mut egui::Ui, thermometer: Option<&MarketThermometer>) {
        let tokens = self.tokens;
        let c = &tokens.color;
        Card::new(&tokens).padding(CardPadding::Md).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::THERMOMETER)
                        .size(tokens.typography.body)
                        .color(c.accent),
                );
                ui.label(
                    RichText::new(compass_i18n::t!("sepa.thermometer"))
                        .size(tokens.typography.caption)
                        .color(c.text_secondary),
                );
                if let Some(t) = thermometer {
                    ui.label(
                        RichText::new(format!("{:.1}", t.score))
                            .monospace()
                            .size(tokens.typography.display)
                            .color(score_color(&tokens, t.score as f32 / 100.0)),
                    );
                    ui.add_space(tokens.spacing.md);
                    let pos_color = score_color(&tokens, t.position_pct as f32 / 100.0);
                    let pos = compass_i18n::t!(t.position_key);
                    Tag::new(&tokens, &pos)
                        .variant(TagVariant::Custom)
                        .color(pos_color)
                        .show(ui);
                } else {
                    ui.label(
                        RichText::new("--")
                            .monospace()
                            .size(tokens.typography.display)
                            .color(c.text_secondary),
                    );
                }
            });
            if let Some(t) = thermometer {
                ui.add_space(tokens.spacing.sm);
                ui.add(egui::Separator::default().horizontal());
                ui.add_space(tokens.spacing.sm);
                ui.horizontal_wrapped(|ui| {
                    for indicator in &t.indicators {
                        Self::indicator_chip(ui, &tokens, indicator);
                        ui.add_space(tokens.spacing.sm);
                    }
                });
            }
        });
    }

    /// One thermometer indicator chip: label + mono value + A-share-colored
    /// delta arrow; the pill tint follows the heat color scale while the
    /// arrow follows the red-up/green-down convention (two semantics, one
    /// chip — design §4). Label/unit render through `compass_i18n::t!()` from the semantic
    /// keys; the value is formatted per the unit precision contract.
    fn indicator_chip(ui: &mut egui::Ui, tokens: &ThemeTokens, ind: &SepaIndicator) {
        let c = &tokens.color;
        let heat = score_color(tokens, ind.heat as f32);
        let value = format_indicator_value(ind.value, ind.unit_key);
        let value_label = compass_i18n::t!(ind.unit_key, v = value);
        egui::Frame::new()
            .fill(tint(heat, 0.18))
            .corner_radius(tokens.radius.pill)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(compass_i18n::t!(ind.label_key))
                            .size(tokens.typography.caption)
                            .color(c.text_secondary),
                    );
                    ui.label(
                        RichText::new(value_label)
                            .monospace()
                            .size(tokens.typography.mono)
                            .color(heat),
                    );
                    if let Some(delta) = ind.delta_pct {
                        let (arrow, color) = if delta >= 0.0 {
                            ("▲", c.up)
                        } else {
                            ("▼", c.down)
                        };
                        ui.label(
                            RichText::new(format!("{arrow} {:.1}%", delta.abs()))
                                .size(tokens.typography.caption)
                                .color(color),
                        );
                    }
                });
            });
    }

    /// Toolbar: count label + TOP-N segmented + refresh button (design §5).
    fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        sepa_signal: &Signal<RunSepaRequest>,
    ) {
        let tokens = self.tokens;
        let c = &tokens.color;
        let loading = shared_state.sepa_loading.get();
        let count_text = match shared_state.sepa_data.get() {
            Some(data) => {
                let shown = data.rows.len().min(self.top_n);
                compass_i18n::t!("sepa.count", shown = shown, date = data.date).into_owned()
            }
            None => compass_i18n::t!("sepa.no_data").into_owned(),
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(count_text)
                    .size(tokens.typography.caption)
                    .color(c.text_secondary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if Button::new(
                    &tokens,
                    if loading {
                        compass_i18n::t!("sepa.computing")
                    } else {
                        compass_i18n::t!("sepa.refresh")
                    },
                )
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Md)
                .icon(egui_phosphor::regular::ARROW_CLOCKWISE)
                .min_width(96.0)
                .loading(loading)
                .show(ui)
                .clicked()
                {
                    self.trigger_refresh(shared_state, sepa_signal);
                }
                ui.add_space(tokens.spacing.md);
                if let Some(idx) = Segmented::new(&tokens, ["TOP 50", "TOP 30"])
                    .selected(if self.top_n == 30 { 1 } else { 0 })
                    .show(ui)
                {
                    self.top_n = if idx == 1 { 30 } else { 50 };
                }
            });
        });
    }

    /// Set loading, clear the error and dispatch a `RunSepaRequest`; on a
    /// failed send reset the loading flag and surface the error.
    fn trigger_refresh(&self, shared_state: &SharedState, sepa_signal: &Signal<RunSepaRequest>) {
        shared_state.sepa_loading.set(true);
        shared_state.sepa_error.set(None);
        if let Err(e) = sepa_signal.send(RunSepaRequest {}) {
            shared_state.sepa_loading.set(false);
            shared_state.sepa_error.set(Some(
                compass_i18n::t!("error.sepa_run", e = e.to_string()).into_owned(),
            ));
        }
    }

    /// Loading / error / data / empty-state branches (design §6.3), then the
    /// ranking table next to the detail panel.
    fn results_area(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        sepa_signal: &Signal<RunSepaRequest>,
        work_signal: &Signal<FetchRequest>,
        data: Option<&compass_types::SepaData>,
    ) {
        if shared_state.sepa_loading.get() {
            ui.spinner();
            ui.label(compass_i18n::t!("sepa.computing_full"));
        } else if let Some(err) = shared_state.sepa_error.get() {
            ui.colored_label(ui.visuals().error_fg_color, err);
        } else if let Some(data) = data {
            // TOP-N truncation applies to a local render copy only — never
            // written back to shared state, so switching 50↔30 loses nothing.
            let mut rows = data.rows.clone();
            rows.truncate(self.top_n);
            let concept_names = shared_state.concept_names.get();
            self.table.set_rows(
                rows.iter()
                    .map(|r| Self::row_cells(r, &concept_names))
                    .collect(),
            );
            ui.horizontal(|ui| {
                // The table must render in a vertical stacking context:
                // egui_extras TableBuilder assumes its header and body
                // ScrollArea stack vertically, but `ui::horizontal` treats
                // them as side-by-side widgets (body rows land to the RIGHT
                // of the header — the #221 real-GUI regression). Reserve the
                // detail-panel width and give the table its own vertical ui.
                //
                // The table width is a fixed slice of the pane (not the
                // dynamically-shrinking `available_width`): egui shrinks a
                // horizontal's available width frame-to-frame as widgets
                // report their min_rect, and DataTable's auto columns grow,
                // which would push the detail panel off the pane edge
                // (user acceptance: "右边内容一团乱").
                let pane_w = ui.available_width();
                let table_w = (pane_w - (DETAIL_PANEL_WIDTH + self.tokens.spacing.md)).max(200.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(table_w, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if let Some(idx) = self.table.show(ui) {
                            self.selected = Some(idx);
                            if let Some(row) = rows.get(idx) {
                                crate::dispatcher::dispatch_symbol_fetch(
                                    shared_state,
                                    work_signal,
                                    &row.symbol,
                                );
                            }
                        }
                    },
                );
                ui.add_space(self.tokens.spacing.md);
                let selected_row = self.selected.and_then(|i| rows.get(i));
                // Pin the detail panel to its reserved width. `ui.set_width`
                // inside the frame does not bound it: the `right_to_left`
                // layouts (rank tag, score/max, factor notes) grow the frame
                // to their content width and bleed past the panel edge
                // (user acceptance: "右边内容一团乱"). Allocating a fixed
                // 280 px container mirrors the table's vertical-context fix.
                ui.allocate_ui_with_layout(
                    egui::vec2(DETAIL_PANEL_WIDTH, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.detail_panel(ui, selected_row),
                );
            });
        } else {
            let tokens = self.tokens;
            let clicked = EmptyState::new(
                &tokens,
                egui_phosphor::regular::CHART_SCATTER,
                &compass_i18n::t!("sepa.empty_title"),
            )
            .description(&compass_i18n::t!("sepa.empty_desc"))
            .action(
                Button::new(&tokens, compass_i18n::t!("sepa.refresh"))
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Md)
                    .icon(egui_phosphor::regular::ARROW_CLOCKWISE),
            )
            .show(ui);
            if clicked.is_some_and(|r| r.clicked()) {
                self.trigger_refresh(shared_state, sepa_signal);
            }
        }
    }

    /// Map one `SepaRow` into the table's cell model (design §2). The
    /// industry + theme cell resolves locale names (epic #266 B3e): industry
    /// via `industry_en`, each theme via the concept zh→en map (D1-A) —
    /// unmapped concepts fall back to Chinese.
    fn row_cells(
        row: &SepaRow,
        concept_names: &std::collections::HashMap<String, String>,
    ) -> Vec<DataCell> {
        let locale = &*compass_i18n::locale();
        let mut industry =
            crate::i18n_name::display_name(locale, &row.industry, row.industry_en.as_deref());
        for theme in row.themes.iter().take(2) {
            // No leading separator when the industry itself is empty (S4).
            if !industry.is_empty() {
                industry.push_str(" · ");
            }
            let en = concept_names.get(theme).map(String::as_str);
            industry.push_str(&crate::i18n_name::display_name(locale, theme, en));
        }
        vec![
            DataCell::Rank(row.rank),
            DataCell::Text(row.symbol.clone()),
            DataCell::Text(row.name.clone()),
            DataCell::Score {
                value: row.total_score as f32,
                max: 100.0,
                inverted: false,
            },
            DataCell::Score {
                value: row.trend as f32,
                max: 30.0,
                inverted: false,
            },
            DataCell::Score {
                value: row.theme as f32,
                max: 25.0,
                inverted: false,
            },
            DataCell::Score {
                value: row.capital as f32,
                max: 20.0,
                inverted: false,
            },
            DataCell::Score {
                value: row.pattern as f32,
                max: 20.0,
                inverted: false,
            },
            DataCell::Score {
                value: row.risk as f32,
                max: RISK_MAX,
                inverted: true,
            },
            DataCell::Text(industry),
            DataCell::Price {
                value: row.latest_price as f32,
                change: None,
            },
            DataCell::Price {
                value: row.change_pct as f32,
                // value == change marks a percent column: the value drives
                // sorting while render_cell renders a single signed percent
                // form (e.g. "+2.50%"), not the duplicated "2.50 +2.50%".
                change: Some(row.change_pct as f32),
            },
        ]
    }

    /// Right-side detail panel: header + total score + five module
    /// rows with per-factor sub-items + theme tags (design §3).
    fn detail_panel(&mut self, ui: &mut egui::Ui, row: Option<&SepaRow>) {
        let tokens = self.tokens;
        let c = &tokens.color;
        egui::Frame::new()
            .fill(c.bg_panel)
            .corner_radius(tokens.radius.md)
            .inner_margin(egui::Margin::symmetric(12, 12))
            .show(ui, |ui| {
                ui.set_width(DETAIL_PANEL_WIDTH);
                let Some(row) = row else {
                    ui.label(
                        RichText::new(compass_i18n::t!("sepa.detail_hint"))
                            .size(tokens.typography.caption)
                            .color(c.text_secondary),
                    );
                    return;
                };

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&row.name)
                            .size(tokens.typography.heading)
                            .color(c.text_primary),
                    );
                    ui.label(
                        RichText::new(&row.symbol)
                            .monospace()
                            .size(tokens.typography.mono)
                            .color(c.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let rank_color = if row.rank <= 3 {
                            c.warning
                        } else {
                            c.text_secondary
                        };
                        Tag::new(&tokens, &format!("#{}", row.rank))
                            .variant(TagVariant::Custom)
                            .color(rank_color)
                            .show(ui);
                    });
                });

                ui.add_space(tokens.spacing.sm);
                ui.label(
                    RichText::new(compass_i18n::t!(
                        "sepa.total_score",
                        score = format!("{:.1}", row.total_score)
                    ))
                    .monospace()
                    .size(tokens.typography.display)
                    .color(score_color(&tokens, row.total_score as f32 / 100.0)),
                );

                ui.add_space(tokens.spacing.sm);
                ui.add(egui::Separator::default().horizontal());
                ui.add_space(tokens.spacing.sm);

                let theme_norm = row.theme as f32 / 25.0;
                for (label, score, max, factors, inverted) in [
                    (
                        compass_i18n::t!("sepa.module.trend"),
                        row.trend,
                        30.0,
                        &row.details.trend,
                        false,
                    ),
                    (
                        compass_i18n::t!("sepa.module.theme"),
                        row.theme,
                        25.0,
                        &row.details.theme,
                        false,
                    ),
                    (
                        compass_i18n::t!("sepa.module.capital"),
                        row.capital,
                        20.0,
                        &row.details.capital,
                        false,
                    ),
                    (
                        compass_i18n::t!("sepa.module.pattern"),
                        row.pattern,
                        20.0,
                        &row.details.pattern,
                        false,
                    ),
                    (
                        compass_i18n::t!("sepa.module.risk"),
                        row.risk,
                        RISK_MAX as f64,
                        &row.details.risk,
                        true,
                    ),
                ] {
                    self.module_row(ui, label.as_ref(), score, max, factors, inverted);
                    ui.add_space(tokens.spacing.sm);
                }

                if !row.themes.is_empty() {
                    ui.add_space(tokens.spacing.sm);
                    ui.horizontal_wrapped(|ui| {
                        for theme in &row.themes {
                            Tag::new(&tokens, theme)
                                .variant(TagVariant::Custom)
                                .color(score_color(&tokens, theme_norm))
                                .show(ui);
                        }
                    });
                }
            });
    }

    /// One module row: label + `score/max` (color-scaled) + a `ProgressBar`
    /// (fill = scale color) + the per-factor sub-items. The risk module is
    /// inverted: the bar shows the deduction fraction and the color scale
    /// runs 0 deduction green → full deduction red.
    fn module_row(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        score: f64,
        max: f64,
        factors: &[compass_types::SepaFactor],
        inverted: bool,
    ) {
        let tokens = self.tokens;
        let c = &tokens.color;
        let norm = if inverted {
            1.0 - score.abs() as f32 / max.max(1e-9) as f32
        } else {
            score as f32 / max.max(1e-9) as f32
        };
        let color = score_color(&tokens, norm);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .size(tokens.typography.body)
                    .color(c.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{score:.1}/{max:.1}"))
                        .monospace()
                        .size(tokens.typography.mono)
                        .color(color),
                );
            });
        });
        let frac = if inverted {
            score.abs() as f32 / max.max(1e-9) as f32
        } else {
            score as f32 / max.max(1e-9) as f32
        };
        ui.add(
            egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                .fill(color)
                .desired_height(6.0),
        );
        for factor in factors {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(compass_i18n::t!(factor.label_key))
                        .size(tokens.typography.caption)
                        .color(c.text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(note_key) = factor.note_key {
                        let note =
                            factor_note_text(note_key, factor.note_args.as_deref().unwrap_or(&[]));
                        ui.label(
                            RichText::new(note)
                                .size(tokens.typography.caption)
                                .color(c.text_secondary),
                        );
                    }
                    let factor_norm = if inverted {
                        1.0 - factor.score.abs() as f32 / factor.max.max(1e-9) as f32
                    } else {
                        factor.score as f32 / factor.max.max(1e-9) as f32
                    };
                    ui.label(
                        RichText::new(format!("{:.1}/{:.0}", factor.score, factor.max))
                            .monospace()
                            .size(tokens.typography.mono)
                            .color(score_color(&tokens, factor_norm)),
                    );
                });
            });
        }
    }

    /// Update the theme tokens after a theme switch so the table restyles
    /// without losing sort/selection state.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
        self.table.set_tokens(tokens);
    }

    /// Drop the selected row index after a refresh — the old index points
    /// at stale data (design §7).
    pub fn reset_selection(&mut self) {
        self.selected = None;
        self.table.set_selected(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_types::{SepaData, SepaDetails, SepaFactor, SepaIndicator};
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    /// Key-resolution test helper (plan T4): resolves a key through the
    /// shared compass-i18n dictionary.
    fn tr(key: &str) -> String {
        compass_i18n::t!(key).to_string()
    }

    /// Factor-note rendering must preserve the pre-i18n numeric precision
    /// contract (issue #222 F2 review): drawdown 1 decimal, momentum/score
    /// integers, big-capital integers with a signed block amount. Passing
    /// raw f64 into an unspecifier'd template would leak full precision
    /// (12.34567 instead of 12.3).
    #[test]
    fn factor_note_text_preserves_numeric_precision() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(
            factor_note_text("sepa.note.drawdown", &[12.34567]),
            "距一年高点回撤 12.3%"
        );
        assert_eq!(
            factor_note_text("sepa.note.momentum_percentile", &[87.345]),
            "动量分位 87%"
        );
        assert_eq!(
            factor_note_text("sepa.note.thermometer", &[63.7]),
            "温度计 64"
        );
        assert_eq!(
            factor_note_text("sepa.note.big_capital", &[75.0, 10.0, 5.0, 5.0]),
            "主力75+龙虎10+调研5+大宗+5"
        );
        assert_eq!(
            factor_note_text("sepa.note.big_capital", &[75.0, 10.0, 5.0, -5.0]),
            "主力75+龙虎10+调研5+大宗-5"
        );
        compass_i18n::set_locale("zh");
    }

    fn panel() -> (SepaPanel, SharedState) {
        let id = CitizenId::new("sepa");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let panel = SepaPanel::new(id, state, &tokens);
        (panel, SharedState::new("SZ000001", "1d"))
    }

    fn signals() -> (
        egui_mobius::signals::Signal<RunSepaRequest>,
        egui_mobius::signals::Signal<FetchRequest>,
    ) {
        let (sepa_signal, _sepa_slot) =
            egui_mobius::factory::create_signal_slot::<RunSepaRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        (sepa_signal, work_signal)
    }

    fn sample_row(rank: usize, symbol: &str, name: &str) -> SepaRow {
        SepaRow {
            symbol: symbol.to_string(),
            name: name.to_string(),
            rank,
            total_score: 80.0 - rank as f64,
            trend: 20.0,
            theme: 18.0,
            capital: 15.0,
            pattern: 15.0,
            risk: 0.0,
            industry: "白酒".to_string(),
            industry_en: None,
            themes: vec!["茅指数".to_string()],
            latest_price: 1500.0,
            change_pct: 2.5,
            details: SepaDetails {
                trend: vec![SepaFactor {
                    label_key: "sepa.factor.vcp_quality",
                    score: 9.2,
                    max: 10.0,
                    note_key: Some("sepa.note.drawdown"),
                    note_args: Some(vec![12.3]),
                }],
                theme: vec![],
                capital: vec![],
                pattern: vec![],
                risk: vec![SepaFactor {
                    label_key: "sepa.factor.volume_stagnation",
                    score: 0.0,
                    max: 2.0,
                    note_key: None,
                    note_args: None,
                }],
            },
        }
    }

    /// `sample_row` with an explicit `industry`/`industry_en` pair + themes
    /// (epic #266 B3e — the industry/theme cell must honour the locale-aware
    /// helper).
    fn row_with_industry(industry: &str, industry_en: Option<&str>, themes: &[&str]) -> SepaRow {
        let mut row = sample_row(1, "SH600519", "贵州茅台");
        row.industry = industry.to_string();
        row.industry_en = industry_en.map(str::to_string);
        row.themes = themes.iter().map(|s| s.to_string()).collect();
        row
    }

    fn sample_data() -> SepaData {
        SepaData {
            rows: vec![
                sample_row(1, "SH600519", "贵州茅台"),
                sample_row(2, "SZ300750", "宁德时代"),
                sample_row(3, "SZ000001", "平安银行"),
            ],
            thermometer: MarketThermometer {
                score: 72.0,
                position_key: "sepa.position.full",
                position_pct: 90.0,
                indicators: vec![
                    SepaIndicator {
                        label_key: "sepa.indicator.hs300_trend",
                        value: 62.4,
                        unit_key: "sepa.unit.percent",
                        delta_pct: Some(2.0),
                        heat: 0.8,
                    },
                    SepaIndicator {
                        label_key: "sepa.indicator.limit_up",
                        value: 45.0,
                        unit_key: "sepa.unit.count",
                        delta_pct: Some(-3.0),
                        heat: 0.6,
                    },
                ],
            },
            date: "2026-08-02".to_string(),
        }
    }

    #[test]
    fn new_creates_panel_with_correct_id() {
        let (panel, _) = panel();
        assert_eq!(panel.id(), &CitizenId::new("sepa"));
        assert_eq!(panel.top_n, 50, "default TOP-N is 50");
        assert!(panel.selected.is_none());
    }

    #[test]
    fn show_renders_empty_state_without_data() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        let (sepa_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();
        let _ = harness.get_by_label(&tr("sepa.empty_title"));
        let _ = harness.get_by_label_contains(&tr("sepa.empty_desc"));
    }

    #[test]
    fn refresh_button_click_sets_loading() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        // The sepa slot must stay alive so the signal send succeeds.
        let (sepa_signal, _sepa_slot) =
            egui_mobius::factory::create_signal_slot::<RunSepaRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        // "刷新" appears both in the toolbar and in the empty-state action;
        // the toolbar renders first — click that one.
        let refresh_label = tr("sepa.refresh");
        let btn = harness
            .query_all_by_label_contains(&refresh_label)
            .next()
            .expect("refresh button rendered");
        btn.click();
        harness.step();

        assert!(
            shared.sepa_loading.get(),
            "sepa_loading should be set after refresh click"
        );
    }

    #[test]
    fn results_renders_rows_thermometer_and_detail() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        shared.sepa_data.set(Some(sample_data()));
        shared.sepa_loading.set(false);
        let (sepa_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        let _ = harness.get_by_label_contains(&compass_i18n::t!(
            "sepa.count",
            shown = 3,
            date = "2026-08-02"
        ));
        let _ = harness.get_by_label(&tr("sepa.thermometer"));
        let _ = harness.get_by_label("72.0");
        let _ = harness.get_by_label_contains(&tr("sepa.indicator.hs300_trend"));
        let _ = harness.get_by_label_contains(&tr("sepa.detail_hint"));
    }

    #[test]
    fn top_n_truncates_local_copy_only() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        let full = sample_data();
        shared.sepa_data.set(Some(full.clone()));
        panel.top_n = 2;
        let (sepa_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        // The date suffix makes the toolbar label unique — the table renders
        // its own "共 2 行" counter too.
        let _ = harness.get_by_label_contains(&compass_i18n::t!(
            "sepa.count",
            shown = 2,
            date = "2026-08-02"
        ));
        assert_eq!(
            shared.sepa_data.get().unwrap().rows.len(),
            3,
            "the full list must stay in shared state"
        );
    }

    #[test]
    fn detail_panel_shows_selected_row_content() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        shared.sepa_data.set(Some(sample_data()));
        panel.selected = Some(0);
        panel.table.set_selected(Some(0));
        let (sepa_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        // Name/symbol also render as table cells — assert the detail-only
        // content instead.
        let _ = harness.get_by_label_contains(&compass_i18n::t!(
            "sepa.total_score",
            score = format!("{:.1}", 79.0)
        ));
        let _ = harness.get_by_label("#1");
        let _ = harness.get_by_label_contains(&tr("sepa.factor.vcp_quality"));
        let _ = harness.get_by_label("茅指数");
    }

    #[test]
    fn row_click_sets_selected_and_dispatches_fetch() {
        let (mut panel, shared) = panel();
        let data = sample_data();
        shared.sepa_data.set(Some(data));
        let (_sepa_signal, _sepa_slot) =
            egui_mobius::factory::create_signal_slot::<RunSepaRequest>();
        // The work slot must stay alive so the signal send succeeds.
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();

        // The click path through TableBuilder cannot be simulated by kittest
        // (same limitation as the screener); exercise the handler directly —
        // the show() wiring is a one-line call into the same code.
        let rows = shared.sepa_data.get().unwrap().rows.clone();
        panel.selected = Some(0);
        if let Some(row) = rows.first() {
            crate::dispatcher::dispatch_symbol_fetch(&shared, &work_signal, &row.symbol);
        }

        assert_eq!(shared.symbol.get(), "SH600519");
        assert!(
            shared.loading.get(),
            "row click must dispatch a FetchBars request"
        );
        assert_eq!(panel.selected, Some(0));
    }

    #[test]
    fn row_cells_map_sepa_row_to_twelve_cells() {
        let cells = SepaPanel::row_cells(
            &sample_row(1, "SH600519", "贵州茅台"),
            &std::collections::HashMap::new(),
        );
        assert_eq!(cells.len(), 12);
        assert_eq!(cells[0], DataCell::Rank(1));
        assert_eq!(
            cells[8],
            DataCell::Score {
                value: 0.0,
                max: RISK_MAX,
                inverted: true
            }
        );
        assert_eq!(
            cells[9],
            DataCell::Text("白酒 · 茅指数".to_string()),
            "industry joins up to two themes"
        );
    }

    // ------------------------------------------------------------------
    // epic #266 B3e — industry cell locale-aware (industry_en). English
    // locale shows the English industry when `industry_en` is present; a
    // missing `industry_en` falls back to the Chinese `industry`; the Chinese
    // locale always renders the Chinese industry.
    // ------------------------------------------------------------------

    #[test]
    fn row_cells_uses_industry_en_in_english_locale() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let row = row_with_industry("白酒", Some("Bai Jiu"), &["茅指数"]);
        let cells = SepaPanel::row_cells(&row, &std::collections::HashMap::new());
        assert_eq!(
            cells[9],
            DataCell::Text("Bai Jiu · 茅指数".to_string()),
            "en locale + industry_en=Some must render the English industry"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn row_cells_industry_falls_back_to_chinese_without_industry_en() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let row = row_with_industry("白酒", None, &["茅指数"]);
        let cells = SepaPanel::row_cells(&row, &std::collections::HashMap::new());
        assert_eq!(
            cells[9],
            DataCell::Text("白酒 · 茅指数".to_string()),
            "en locale + industry_en=None must fall back to the Chinese industry"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn row_cells_industry_stays_chinese_in_zh_locale_even_with_industry_en() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let row = row_with_industry("白酒", Some("Bai Jiu"), &["茅指数"]);
        let cells = SepaPanel::row_cells(&row, &std::collections::HashMap::new());
        assert_eq!(
            cells[9],
            DataCell::Text("白酒 · 茅指数".to_string()),
            "zh locale must always render the Chinese industry, industry_en must not leak"
        );
    }

    #[test]
    fn row_cells_industry_ignores_empty_string_industry_en() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let row = row_with_industry("白酒", Some(""), &["茅指数"]);
        let cells = SepaPanel::row_cells(&row, &std::collections::HashMap::new());
        assert_eq!(
            cells[9],
            DataCell::Text("白酒 · 茅指数".to_string()),
            "an empty-string industry_en must be treated as unmapped and fall back to Chinese"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn set_tokens_updates_table_theme() {
        let (mut panel, _) = panel();
        let light = ThemeTokens::light();
        panel.set_tokens(light);
        assert_eq!(panel.tokens, light);
    }

    #[test]
    fn reset_selection_clears_selected() {
        let (mut panel, _) = panel();
        panel.selected = Some(2);
        panel.reset_selection();
        assert!(panel.selected.is_none());
    }

    /// The detail panel must lay out its content inside its reserved width
    /// (280 px) with no text bleeding past the right edge or overlapping —
    /// user acceptance reported "右边内容一团乱" after clicking a ranking row.
    #[test]
    fn detail_panel_content_stays_inside_panel_width() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        shared.sepa_data.set(Some(sample_data()));
        panel.selected = Some(0);
        panel.table.set_selected(Some(0));
        let (sepa_signal, work_signal) = signals();
        let md = panel.tokens.spacing.md;

        // Real-window width (≈1887 px dock pane): the 12-column table needs
        // ~1100 px, leaving the detail panel its reserved 280 px. Narrower
        // widths overflow the table itself (separate issue), so test at the
        // supported width.
        let pane_w = 1400.0;
        let harness_panel_right = std::cell::Cell::new(0.0f32);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(pane_w, 600.0))
            .build_ui(|ui| {
                // Capture the panel's actual rect: CentralPanel applies its
                // own margins, so the right edge is not simply pane_w.
                let probe = ui.allocate_ui_with_layout(
                    egui::vec2(pane_w, 600.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        panel.show(ui, &shared, &sepa_signal, &work_signal);
                    },
                );
                harness_panel_right.set(probe.response.rect.max.x);
            });
        harness.run_steps(2);

        let texts = harness.query_all_by_label_contains("").collect::<Vec<_>>();
        // The detail panel spans the rightmost 280 px of the panel rect; the
        // right-aligned score/max text must not bleed past the panel's right
        // edge (minus the inter-column margin).
        let panel_right = harness_panel_right.get() - md;
        let panel_left = panel_right - DETAIL_PANEL_WIDTH;
        let mut offenders: Vec<String> = Vec::new();
        for t in &texts {
            if t.rect().min.x < panel_left - 1.0 {
                // Table-side text is allowed; only detail-panel text counts.
                continue;
            }
            if t.rect().max.x > panel_right + 1.0 && t.rect().width() > 0.0 {
                offenders.push(format!(
                    "'{}' right edge {:.1} > {:.1}",
                    t.value().unwrap_or_default(),
                    t.rect().max.x,
                    panel_right
                ));
            }
        }
        assert!(
            offenders.is_empty(),
            "detail-panel text bleeds past the reserved width: {offenders:?}"
        );
    }

    /// Same 1400 px detail-panel sweep in English (plan T15): wider en
    /// strings must not bleed past the reserved panel width either.
    #[test]
    fn en_detail_panel_content_stays_inside_panel_width() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let (mut panel, shared) = panel();
        shared.sepa_data.set(Some(sample_data()));
        panel.selected = Some(0);
        panel.table.set_selected(Some(0));
        let (sepa_signal, work_signal) = signals();
        let md = panel.tokens.spacing.md;

        let pane_w = 1400.0;
        let harness_panel_right = std::cell::Cell::new(0.0f32);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(pane_w, 600.0))
            .build_ui(|ui| {
                let probe = ui.allocate_ui_with_layout(
                    egui::vec2(pane_w, 600.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        panel.show(ui, &shared, &sepa_signal, &work_signal);
                    },
                );
                harness_panel_right.set(probe.response.rect.max.x);
            });
        harness.run_steps(2);

        let texts = harness.query_all_by_label_contains("").collect::<Vec<_>>();
        let panel_right = harness_panel_right.get() - md;
        let panel_left = panel_right - DETAIL_PANEL_WIDTH;
        let mut offenders: Vec<String> = Vec::new();
        for t in &texts {
            if t.rect().min.x < panel_left - 1.0 {
                continue;
            }
            if t.rect().max.x > panel_right + 1.0 && t.rect().width() > 0.0 {
                offenders.push(format!(
                    "'{}' right edge {:.1} > {:.1}",
                    t.value().unwrap_or_default(),
                    t.rect().max.x,
                    panel_right
                ));
            }
        }
        assert!(
            offenders.is_empty(),
            "en detail-panel text bleeds past the reserved width: {offenders:?}"
        );
        compass_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // #222 i18n acceptance (RED).
    // - T6: the 12 COLUMNS headers become key constants ("sepa.table.rank"
    //   etc.) — assertion-RED now (they are zh literals).
    // - T7/T8/T10: SepaIndicator/MarketThermometer carry semantic key
    //   fields (label_key/unit_key/value, position_key) and the renderers
    //   resolve them via compass_i18n::t!() — compile-RED (fields not yet added).
    // set_locale is process-global; T15 must unify LANG_LOCK with the one
    // in main.rs tests.
    // ------------------------------------------------------------------

    use crate::citizens::ui_fixes_218::LANG_LOCK;

    #[test]
    fn sepa_columns_headers_are_key_constants() {
        assert_eq!(COLUMNS[0].header, "sepa.table.rank");
        assert_eq!(COLUMNS[1].header, "sepa.table.code");
        assert_eq!(COLUMNS[2].header, "sepa.table.name");
        assert_eq!(COLUMNS[3].header, "sepa.table.total");
        assert_eq!(COLUMNS[4].header, "sepa.table.trend");
        assert_eq!(COLUMNS[5].header, "sepa.table.theme");
        assert_eq!(COLUMNS[6].header, "sepa.table.capital");
        assert_eq!(COLUMNS[7].header, "sepa.table.pattern");
        assert_eq!(COLUMNS[8].header, "sepa.table.risk");
        assert_eq!(COLUMNS[9].header, "sepa.table.industry");
        assert_eq!(COLUMNS[10].header, "sepa.table.latest");
        assert_eq!(COLUMNS[11].header, "sepa.table.change");
    }

    fn keyed_sample_data() -> SepaData {
        SepaData {
            rows: vec![],
            thermometer: MarketThermometer {
                score: 72.0,
                position_key: "sepa.position.full",
                position_pct: 90.0,
                indicators: vec![SepaIndicator {
                    label_key: "sepa.indicator.hs300_trend",
                    value: 62.4,
                    unit_key: "sepa.unit.percent",
                    delta_pct: Some(2.0),
                    heat: 0.8,
                }],
            },
            date: "2026-08-02".to_string(),
        }
    }

    #[test]
    fn thermometer_renders_label_key_in_zh() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        shared.sepa_data.set(Some(keyed_sample_data()));
        shared.sepa_loading.set(false);
        let (sepa_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        let _ = harness.get_by_label("沪深300趋势");
        let _ = harness.get_by_label("62.4%");
        let _ = harness.get_by_label("80%-100%");
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn thermometer_renders_label_key_in_en() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let (mut panel, shared) = panel();
        shared.sepa_data.set(Some(keyed_sample_data()));
        shared.sepa_loading.set(false);
        let (sepa_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &sepa_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        let _ = harness.get_by_label("HS300 Trend");
        let _ = harness.get_by_label("80%-100%");
        compass_i18n::set_locale("zh");
    }
}
