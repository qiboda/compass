//! Market panel citizen — 大盘 overview tab (epic #255 C4, plan T6).
//!
//! Report-type panel: a core-index card (6-index whitelist), a toolbar
//! (count label + industry/concept/official Segmented + manual refresh) and
//! a sortable ranking table fed by the fourth `AsyncDispatcher` channel
//! (`RunIndexSnapshotRequest` → `IndexSnapshot`). Segment switching filters
//! a local copy of the snapshot in memory — never re-fetches (SEPA TOP-N
//! truncation precedent). Row clicks link to the chart via
//! `dispatch_symbol_fetch` without switching tabs.

use egui::RichText;
use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use compass_types::{IndexRow, IndexSnapshot};
use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::button::{Button, ButtonSize, ButtonVariant};
use compass_ui::widgets::card::{Card, CardPadding};
use compass_ui::widgets::data_table::{ColumnSpec, DataCell, DataTable};
use compass_ui::widgets::empty_state::EmptyState;
use compass_ui::widgets::segmented::Segmented;

use crate::messages::{FetchRequest, RunIndexSnapshotRequest};
use crate::state::SharedState;

/// Core-index whitelist (plan T6): the six official indexes pinned in the
/// overview card. Names are data — the snapshot's own `name`/`name_en` from
/// index_basic.parquet win when present; the triple `(symbol, zh, en)`
/// fallback covers a missing row (epic #266 B3: English fallback joins the
/// card so the name follows the locale).
const CORE_INDEX_WHITELIST: [(&str, &str, &str); 6] = [
    ("SH000001", "上证指数", "SSE Composite"),
    ("SZ399001", "深证成指", "SZSE Component"),
    ("SZ399006", "创业板指", "ChiNext"),
    ("SH000300", "沪深300", "CSI 300"),
    ("SH000905", "中证500", "CSI 500"),
    ("SH000852", "中证1000", "CSI 1000"),
];

/// Ranking-table columns (design 大盘 tab §③): name / code / latest /
/// change / turnover. Headers are i18n key constants resolved at render
/// time (issue #222 contract).
const COLUMNS: [ColumnSpec; 5] = [
    ColumnSpec {
        header: "index.table.name",
        numeric: false,
    },
    ColumnSpec {
        header: "index.table.code",
        numeric: false,
    },
    ColumnSpec {
        header: "index.table.latest",
        numeric: true,
    },
    ColumnSpec {
        header: "index.table.change",
        numeric: true,
    },
    ColumnSpec {
        header: "index.table.amount",
        numeric: true,
    },
];

/// Change-percent column index — the business default sort (板块轮动:
/// strongest mover first).
const CHANGE_COLUMN: usize = 3;

/// Segmented options map to the `index_type` filter values, in segment
/// order: 0 = industry (default), 1 = concept, 2 = official.
const SEGMENT_TYPES: [&str; 3] = ["industry", "concept", "official"];

/// Format a price per the vendored `format_price` rule (labels.rs): ≥100 →
/// 2 decimals (index points are 3000+), ≥1 → 4, <1 → 6.
fn format_price(price: f64) -> String {
    if price >= 100.0 {
        format!("{price:.2}")
    } else if price >= 1.0 {
        format!("{price:.4}")
    } else {
        format!("{price:.6}")
    }
}

/// Market panel citizen.
pub struct MarketPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    /// Theme tokens copied at construction (component styling).
    tokens: ThemeTokens,
    /// Ranking table — owns its sort state across frames.
    table: DataTable,
    /// Selected Segmented index: 0 = industry, 1 = concept, 2 = official.
    segment: usize,
}

impl Citizen for MarketPanel {
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

impl MarketPanel {
    /// Create a market panel with the given citizen identity/state.
    pub fn new(citizen_id: CitizenId, citizen_state: CitizenState, tokens: &ThemeTokens) -> Self {
        let mut table = DataTable::new(tokens, COLUMNS.to_vec());
        // Business default: change percent descending (板块轮动视角, design
        // §四-③); header clicks keep the descending default for this column.
        table.set_sort(CHANGE_COLUMN, true);
        table.set_descending_default(CHANGE_COLUMN, true);
        Self {
            citizen_id,
            citizen_state,
            tokens: *tokens,
            table,
            segment: 0,
        }
    }

    /// Render the panel: core-index card + toolbar + ranking table.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        index_signal: &Signal<RunIndexSnapshotRequest>,
        work_signal: &Signal<FetchRequest>,
    ) {
        let snapshot = shared_state.index_snapshot.get();
        ui.vertical(|ui| {
            self.core_index_card(ui, shared_state, snapshot.as_ref(), work_signal);

            ui.add_space(self.tokens.spacing.sm);
            self.toolbar(ui, shared_state, index_signal);

            ui.add_space(self.tokens.spacing.md);
            self.results_area(
                ui,
                shared_state,
                index_signal,
                work_signal,
                snapshot.as_ref(),
            );
        });
    }

    /// Core-index card (design ①): the six whitelist indexes, each rendered
    /// as an interactive block — name caption + mono point + colored change.
    /// Click links to the chart (不切 tab, SEPA row-click precedent).
    fn core_index_card(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        snapshot: Option<&IndexSnapshot>,
        work_signal: &Signal<FetchRequest>,
    ) {
        let tokens = self.tokens;
        let c = &tokens.color;
        let current = shared_state.symbol.get();
        Card::new(&tokens).padding(CardPadding::Md).show(ui, |ui| {
            ui.label(
                RichText::new(compass_i18n::t!("index.card_title"))
                    .size(tokens.typography.caption)
                    .color(c.text_secondary),
            );
            ui.add_space(tokens.spacing.sm);
            ui.horizontal_wrapped(|ui| {
                let locale = &*compass_i18n::locale();
                for (symbol, zh, en) in CORE_INDEX_WHITELIST {
                    let row = snapshot.and_then(|s| s.rows.iter().find(|r| r.symbol == symbol));
                    // Precedence (epic #266 B3): present row → its locale name
                    // (snapshot beats the hardcoded fallback); absent row →
                    // the triple's zh/en fallback per locale.
                    let name = match row {
                        Some(r) => {
                            crate::i18n_name::display_name(locale, &r.name, r.name_en.as_deref())
                        }
                        None => crate::i18n_name::display_name(locale, zh, Some(en)),
                    };
                    let point = row
                        .map(|r| format_price(r.latest))
                        .unwrap_or_else(|| "--".to_string());
                    let style = (*ui.style()).clone();
                    let mut job = egui::text::LayoutJob::default();
                    RichText::new(name)
                        .size(tokens.typography.caption)
                        .color(c.text_secondary)
                        .append_to(
                            &mut job,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Min,
                        );
                    RichText::new(format!("  {point}"))
                        .monospace()
                        .size(tokens.typography.mono)
                        .color(c.text_primary)
                        .append_to(
                            &mut job,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Min,
                        );
                    if let Some(r) = row {
                        let pct_color = if r.change_pct >= 0.0 { c.up } else { c.down };
                        RichText::new(format!(
                            "  {}",
                            compass_ui::widgets::price_text::format_change(r.change_pct as f32)
                        ))
                        .size(tokens.typography.caption)
                        .color(pct_color)
                        .append_to(
                            &mut job,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Min,
                        );
                    }
                    let response = ui.selectable_label(current == *symbol, job);
                    if response.clicked()
                        && let Some(r) = row
                    {
                        crate::dispatcher::dispatch_symbol_fetch(
                            shared_state,
                            work_signal,
                            &r.symbol,
                        );
                    }
                }
            });
        });
    }

    /// Toolbar (design ②): count label + Segmented filter + refresh button.
    /// Refresh is purely manual — no auto-refresh (SEPA precedent).
    fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        index_signal: &Signal<RunIndexSnapshotRequest>,
    ) {
        let tokens = self.tokens;
        let c = &tokens.color;
        let loading = shared_state.index_snapshot_loading.get();
        let count_text = match shared_state.index_snapshot.get() {
            Some(snap) if !snap.rows.is_empty() => {
                compass_i18n::t!("index.count", count = snap.rows.len(), date = snap.date)
                    .into_owned()
            }
            _ => compass_i18n::t!("index.no_data").into_owned(),
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
                        compass_i18n::t!("index.computing")
                    } else {
                        compass_i18n::t!("index.refresh")
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
                    self.trigger_refresh(shared_state, index_signal);
                }
                ui.add_space(tokens.spacing.md);
                if let Some(idx) = Segmented::new(
                    &tokens,
                    [
                        compass_i18n::t!("index.segment.industry"),
                        compass_i18n::t!("index.segment.concept"),
                        compass_i18n::t!("index.segment.official"),
                    ],
                )
                .selected(self.segment)
                .show(ui)
                {
                    self.segment = idx;
                }
            });
        });
    }

    /// Set loading, clear the error and dispatch a `RunIndexSnapshotRequest`;
    /// on a failed send reset the loading flag and surface the error.
    fn trigger_refresh(
        &self,
        shared_state: &SharedState,
        index_signal: &Signal<RunIndexSnapshotRequest>,
    ) {
        shared_state.index_snapshot_loading.set(true);
        shared_state.index_snapshot_error.set(None);
        if let Err(e) = index_signal.send(RunIndexSnapshotRequest {}) {
            shared_state.index_snapshot_loading.set(false);
            shared_state.index_snapshot_error.set(Some(
                compass_i18n::t!("error.index_run", e = e.to_string()).into_owned(),
            ));
        }
    }

    /// Loading / error / data / empty-state branches (design §四), then the
    /// ranking table filtered by the selected segment.
    fn results_area(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        index_signal: &Signal<RunIndexSnapshotRequest>,
        work_signal: &Signal<FetchRequest>,
        snapshot: Option<&IndexSnapshot>,
    ) {
        if shared_state.index_snapshot_loading.get() {
            ui.spinner();
            ui.label(compass_i18n::t!("index.computing"));
        } else if let Some(err) = shared_state.index_snapshot_error.get() {
            ui.colored_label(ui.visuals().error_fg_color, err);
        } else if let Some(snapshot) = snapshot {
            // Segment filtering + default ordering apply to a local render
            // copy only — shared state is never written back.
            let rows = Self::filter_rows(&snapshot.rows, self.segment);
            self.table
                .set_rows(rows.iter().map(Self::row_cells).collect());
            if let Some(idx) = self.table.show(ui)
                && let Some(row) = rows.get(idx)
            {
                crate::dispatcher::dispatch_symbol_fetch(shared_state, work_signal, &row.symbol);
            }
        } else {
            let tokens = self.tokens;
            let clicked = EmptyState::new(
                &tokens,
                egui_phosphor::regular::TREND_UP,
                &compass_i18n::t!("index.empty_title"),
            )
            .description(&compass_i18n::t!("index.empty_desc"))
            .action(
                Button::new(&tokens, compass_i18n::t!("index.refresh"))
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Md)
                    .icon(egui_phosphor::regular::ARROW_CLOCKWISE),
            )
            .show(ui);
            if clicked.is_some_and(|r| r.clicked()) {
                self.trigger_refresh(shared_state, index_signal);
            }
        }
    }

    /// Filter snapshot rows to the selected segment and order them by change
    /// percent descending (板块轮动视角 — the business default; the table
    /// header can re-sort afterwards).
    fn filter_rows(rows: &[IndexRow], segment: usize) -> Vec<IndexRow> {
        let want = SEGMENT_TYPES[segment.min(SEGMENT_TYPES.len() - 1)];
        let mut filtered: Vec<IndexRow> = rows
            .iter()
            .filter(|r| r.index_type == want)
            .cloned()
            .collect();
        // Default order: change percent descending.
        filtered.sort_by(|a, b| b.change_pct.total_cmp(&a.change_pct));
        filtered
    }

    /// Map one `IndexRow` into the table's cell model (design §③): name /
    /// code / latest / change / turnover (亿元, integer). The name cell
    /// resolves the locale display name (epic #266 B3d).
    fn row_cells(row: &IndexRow) -> Vec<DataCell> {
        let locale = &*compass_i18n::locale();
        vec![
            DataCell::Text(crate::i18n_name::display_name(
                locale,
                &row.name,
                row.name_en.as_deref(),
            )),
            DataCell::Text(row.symbol.clone()),
            DataCell::Price {
                value: row.latest as f32,
                change: None,
            },
            DataCell::Price {
                value: row.change_pct as f32,
                // value == change marks a percent column (SEPA precedent):
                // renders a single signed percent form.
                change: Some(row.change_pct as f32),
            },
            DataCell::Count((row.amount / 1e8).round() as usize),
        ]
    }

    /// Update the theme tokens after a theme switch so the table restyles
    /// without losing sort/selection state.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
        self.table.set_tokens(tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    /// Key-resolution test helper.
    fn tr(key: &str) -> String {
        compass_i18n::t!(key).to_string()
    }

    fn panel() -> (MarketPanel, SharedState) {
        let id = CitizenId::new("market");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let panel = MarketPanel::new(id, state, &tokens);
        (panel, SharedState::new("SZ000001", "1d"))
    }

    fn signals() -> (
        egui_mobius::signals::Signal<RunIndexSnapshotRequest>,
        egui_mobius::signals::Signal<FetchRequest>,
    ) {
        let (index_signal, _index_slot) =
            egui_mobius::factory::create_signal_slot::<RunIndexSnapshotRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        (index_signal, work_signal)
    }

    fn sample_row(symbol: &str, name: &str, index_type: &str, change: f64) -> IndexRow {
        IndexRow {
            symbol: symbol.to_string(),
            name: name.to_string(),
            name_en: None,
            index_type: index_type.to_string(),
            latest: 3000.0,
            change_pct: change,
            amount: 123_456_789.0,
        }
    }

    /// `sample_row` with an explicit `name_en` (epic #266 B3d — the
    /// Rank-table name column must honour the locale-aware helper).
    fn row_with_name_en(symbol: &str, name: &str, name_en: Option<&str>) -> IndexRow {
        IndexRow {
            symbol: symbol.to_string(),
            name: name.to_string(),
            name_en: name_en.map(str::to_string),
            index_type: "official".to_string(),
            latest: 3000.0,
            change_pct: 1.0,
            amount: 123_456_789.0,
        }
    }

    fn sample_snapshot() -> IndexSnapshot {
        IndexSnapshot {
            rows: vec![
                sample_row("SH000001", "上证指数", "official", 0.82),
                sample_row("BK0475", "半导体", "industry", -1.25),
                sample_row("BK1169", "AI概念", "concept", 3.5),
            ],
            date: "2026-08-13".to_string(),
        }
    }

    #[test]
    fn new_creates_panel_with_correct_id_and_defaults() {
        let (panel, _) = panel();
        assert_eq!(panel.id(), &CitizenId::new("market"));
        assert_eq!(panel.segment, 0, "default segment is industry");
        assert!(panel.table.sort_descending(), "change column defaults desc");
    }

    #[test]
    fn show_renders_empty_state_without_data() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();
        let _ = harness.get_by_label(&tr("index.empty_title"));
        let _ = harness.get_by_label_contains(&tr("index.empty_desc"));
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn refresh_button_click_sets_loading() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        let (index_signal, _index_slot) =
            egui_mobius::factory::create_signal_slot::<RunIndexSnapshotRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        let refresh_label = tr("index.refresh");
        let btn = harness
            .query_all_by_label_contains(&refresh_label)
            .next()
            .expect("refresh button rendered");
        btn.click();
        harness.step();

        assert!(
            shared.index_snapshot_loading.get(),
            "index_snapshot_loading should be set after refresh click"
        );
    }

    #[test]
    fn results_renders_card_table_and_count() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        shared.index_snapshot.set(Some(sample_snapshot()));
        shared.index_snapshot_loading.set(false);
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        let _ = harness.get_by_label(&tr("index.card_title"));
        let _ = harness.get_by_label_contains(&compass_i18n::t!(
            "index.count",
            count = 3,
            date = "2026-08-13"
        ));
        // Industry segment is the default: only BK0475 survives the filter.
        let _ = harness.get_by_label_contains("半导体");
    }

    #[test]
    fn segment_switch_filters_locally() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        shared.index_snapshot.set(Some(sample_snapshot()));
        shared.index_snapshot_loading.set(false);
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        harness.get_by_label(&tr("index.segment.official")).click();
        harness.step();
        // The official segment shows SH000001 (code appears only in the
        // table row — the card renders name/point/change, no code).
        let _ = harness.get_by_label_contains("SH000001");
    }

    #[test]
    fn filter_rows_orders_by_change_descending() {
        let mut rows = vec![
            sample_row("BK1", "a", "industry", 1.0),
            sample_row("BK2", "b", "industry", 5.0),
            sample_row("BK3", "c", "concept", 2.0),
            sample_row("BK4", "d", "industry", -3.0),
        ];
        rows.sort_by(|a, b| b.change_pct.total_cmp(&a.change_pct));
        let filtered = MarketPanel::filter_rows(&rows, 0);
        assert_eq!(filtered.len(), 3, "industry segment has 3 rows");
        assert_eq!(filtered[0].symbol, "BK2", "highest change first");
        assert_eq!(filtered[2].symbol, "BK4", "negative change last");
    }

    #[test]
    fn row_cells_map_index_row_to_five_cells() {
        let row = sample_row("SH000001", "上证指数", "official", 0.82);
        let cells = MarketPanel::row_cells(&row);
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[0], DataCell::Text("上证指数".to_string()));
        assert_eq!(cells[4], DataCell::Count(1), "1.23e8 yuan = 1 亿元");
    }

    #[test]
    fn whitelist_embeds_six_core_indexes() {
        assert_eq!(CORE_INDEX_WHITELIST.len(), 6);
        // epic #266 decision #6: the whitelist expands to (symbol, zh, en)
        // triples — the core-index card falls back to the locale-correct
        // name when the snapshot has no row for the symbol.
        // COMPILE-RED while the production CORE_INDEX_WHITELIST is still a
        // (symbol, zh) pair; it becomes compile-clean once B3d promotes it to
        // a triple.
        for (symbol, zh, en) in CORE_INDEX_WHITELIST {
            assert!(symbol.starts_with("SH") || symbol.starts_with("SZ"));
            assert!(!zh.is_empty(), "zh fallback must never be empty");
            assert!(!en.is_empty(), "en fallback must never be empty");
        }
    }

    // ------------------------------------------------------------------
    // epic #266 B3d — rank-table name column locale-aware (name_en).
    // English locale shows the English name when a row carries `name_en`;
    // a missing `name_en` falls back to the Chinese `name`; the Chinese
    // locale always renders the Chinese name (name_en never leaks).
    // ------------------------------------------------------------------

    #[test]
    fn row_cells_name_uses_name_en_in_english_locale() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let row = row_with_name_en("SH000001", "上证指数", Some("SSE Composite"));
        let cells = MarketPanel::row_cells(&row);
        assert_eq!(
            cells[0],
            DataCell::Text("SSE Composite".to_string()),
            "en locale + name_en=Some must render the English name"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn row_cells_name_falls_back_to_chinese_without_name_en() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let row = row_with_name_en("SH000001", "上证指数", None);
        let cells = MarketPanel::row_cells(&row);
        assert_eq!(
            cells[0],
            DataCell::Text("上证指数".to_string()),
            "en locale + name_en=None must fall back to the Chinese name"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn row_cells_name_stays_chinese_in_zh_locale_even_with_name_en() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let row = row_with_name_en("SH000001", "上证指数", Some("SSE Composite"));
        let cells = MarketPanel::row_cells(&row);
        assert_eq!(
            cells[0],
            DataCell::Text("上证指数".to_string()),
            "zh locale must always render the Chinese name, name_en must not leak"
        );
    }

    #[test]
    fn row_cells_name_ignores_empty_string_name_en() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let row = row_with_name_en("SH000001", "上证指数", Some(""));
        let cells = MarketPanel::row_cells(&row);
        assert_eq!(
            cells[0],
            DataCell::Text("上证指数".to_string()),
            "an empty-string name_en must be treated as unmapped and fall back to Chinese"
        );
        compass_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // epic #266 B3d — core-index card triplet fallback by locale: when the
    // snapshot has no row for a whitelist symbol, the card must render the
    // locale-correct fallback name (zh → 上证指数, en → SSE Composite).
    // Rendered text is asserted at the shape level (testing.md ref #217:
    // selectable_label with a LayoutJob has no AccessKit label).
    // ------------------------------------------------------------------

    /// True when any text shape in the rendered output contains `needle`.
    /// `harness.output().shapes` is a `Vec<ClippedShape>`; each clip carries
    /// a `Shape` in its `.shape` field (main.rs tessellate-decompose pattern).
    fn text_drawn(shapes: &[egui::epaint::ClippedShape], needle: &str) -> bool {
        fn walk(shapes: &[egui::Shape], needle: &str) -> bool {
            shapes.iter().any(|shape| match shape {
                egui::Shape::Vec(inner) => walk(inner, needle),
                egui::Shape::Text(text) => text.galley.text().contains(needle),
                _ => false,
            })
        }
        shapes
            .iter()
            .any(|clipped| walk(std::slice::from_ref(&clipped.shape), needle))
    }

    #[test]
    fn core_index_card_fallback_renders_chinese_in_zh_locale() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let (mut panel, shared) = panel();
        // Snapshot without any whitelist symbol → all six core cards fall back.
        shared.index_snapshot.set(Some(IndexSnapshot {
            rows: vec![sample_row("BK0475", "半导体", "industry", -1.25)],
            date: "2026-08-13".to_string(),
        }));
        shared.index_snapshot_loading.set(false);
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        assert!(
            text_drawn(&harness.output().shapes, "上证指数"),
            "zh locale core-card fallback must render the Chinese name"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn core_index_card_fallback_renders_english_in_en_locale() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let (mut panel, shared) = panel();
        // No whitelist symbol in the snapshot → the card uses the en triplet
        // fallback (SSE Composite for SH000001).
        shared.index_snapshot.set(Some(IndexSnapshot {
            rows: vec![sample_row("BK0475", "半导体", "industry", -1.25)],
            date: "2026-08-13".to_string(),
        }));
        shared.index_snapshot_loading.set(false);
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        assert!(
            text_drawn(&harness.output().shapes, "SSE Composite"),
            "en locale core-card fallback must render the English name"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn row_click_dispatches_symbol_fetch() {
        let (panel, shared) = panel();
        let snapshot = sample_snapshot();
        shared.index_snapshot.set(Some(snapshot.clone()));
        let (_index_signal, _index_slot) =
            egui_mobius::factory::create_signal_slot::<RunIndexSnapshotRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();

        // TableBuilder clicks cannot be simulated by kittest (SEPA
        // limitation); exercise the dispatch handler directly.
        let rows = MarketPanel::filter_rows(&snapshot.rows, panel.segment);
        if let Some(row) = rows.first() {
            crate::dispatcher::dispatch_symbol_fetch(&shared, &work_signal, &row.symbol);
        }
        assert_eq!(shared.symbol.get(), "BK0475");
        assert!(
            shared.loading.get(),
            "row click must dispatch a FetchBars request"
        );
    }

    #[test]
    fn set_tokens_updates_table_theme() {
        let (mut panel, _) = panel();
        let light = ThemeTokens::light();
        panel.set_tokens(light);
        assert_eq!(panel.tokens, light);
    }

    // ------------------------------------------------------------------
    // epic #266 B3d — adversarial (subagent): the locale-aware name must not
    // half-apply. M1 locale round-trip catches a helper that reads a cached
    // locale or a `row_cells` that ignores locale; M5 arms (a)/(b) pin the
    // core-card precedence (row present wins over triple fallback); M6
    // guards a whitelist-lookup regression on arbitrary table rows; P1
    // guards O(n·k) per-row churn on large snapshots.
    // ------------------------------------------------------------------

    #[test]
    fn adversarial_270_market_locale_round_trip_flips_cell() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let row = row_with_name_en("SH000001", "上证指数", Some("SSE Composite"));
        // zh → en → zh → en: each flip must change the rendered cell and the
        // final cell must match the last locale, not a cached earlier one.
        compass_i18n::set_locale("zh");
        assert_eq!(
            MarketPanel::row_cells(&row)[0],
            DataCell::Text("上证指数".to_string())
        );
        compass_i18n::set_locale("en");
        assert_eq!(
            MarketPanel::row_cells(&row)[0],
            DataCell::Text("SSE Composite".to_string()),
            "M1: en flip must render English"
        );
        compass_i18n::set_locale("zh");
        assert_eq!(
            MarketPanel::row_cells(&row)[0],
            DataCell::Text("上证指数".to_string()),
            "M1: back to zh must render Chinese again"
        );
        compass_i18n::set_locale("en");
        assert_eq!(
            MarketPanel::row_cells(&row)[0],
            DataCell::Text("SSE Composite".to_string()),
            "M1: second en flip must render English, not a stale zh"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn adversarial_270_market_card_row_present_with_en_wins_in_en_locale() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let (mut panel, shared) = panel();
        // SH000001 is in the whitelist AND carries name_en → the card must
        // render the row's English name (SSE Composite), never the zh triple.
        shared.index_snapshot.set(Some(IndexSnapshot {
            rows: vec![row_with_name_en(
                "SH000001",
                "上证指数",
                Some("SSE Composite"),
            )],
            date: "2026-08-13".to_string(),
        }));
        shared.index_snapshot_loading.set(false);
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        assert!(
            text_drawn(&harness.output().shapes, "SSE Composite"),
            "M5a: card must render the row's English name when a whitelist row is present with name_en"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn adversarial_270_market_card_row_present_without_en_prefers_row_name_over_triple() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        let (mut panel, shared) = panel();
        // SH000001 present but name_en=None. Precedence: snapshot wins → the
        // card must render the row's Chinese name, NOT the triple's "SSE
        // Composite" fallback. The ranking table (industry segment default)
        // filters the official row out, so only the card draws these names.
        shared.index_snapshot.set(Some(IndexSnapshot {
            rows: vec![row_with_name_en("SH000001", "上证指数", None)],
            date: "2026-08-13".to_string(),
        }));
        shared.index_snapshot_loading.set(false);
        let (index_signal, work_signal) = signals();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &index_signal, &work_signal);
        });
        harness.fit_contents();
        harness.step();

        let shapes = harness.output().shapes.clone();
        assert!(
            text_drawn(&shapes, "上证指数"),
            "M5b: card must render the present row's Chinese name in en locale"
        );
        assert!(
            !text_drawn(&shapes, "SSE Composite"),
            "M5b: the triple en fallback must NOT override a present row's name"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn adversarial_270_market_arbitrary_non_whitelist_row_falls_back_to_own_name() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        // An arbitrary symbol outside the 6-index whitelist, with no name_en,
        // must render its own Chinese name — never a failure, never a blank.
        let row = row_with_name_en("BK9999", "自定义指数", None);
        assert_eq!(
            MarketPanel::row_cells(&row)[0],
            DataCell::Text("自定义指数".to_string()),
            "M6: non-whitelist row must fall back to its own name (not a whitelist en name, not blank)"
        );
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn adversarial_270_market_large_snapshot_no_churn() {
        let _guard = crate::citizens::ui_fixes_218::LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        // 10k rows — half industry half concept — must filter + map in O(n):
        // one cell vector per row, no per-row whitelist re-scan blow-up, and
        // the name cell follows the locale (en rows render English).
        let types = ["industry", "concept"];
        let rows: Vec<IndexRow> = (0..10_000)
            .map(|i| {
                let mut r = sample_row(
                    &format!("BK{i:05}"),
                    &format!("指数{i}"),
                    types[i % 2],
                    (i as f64 - 5000.0) / 100.0,
                );
                if i % 3 == 0 {
                    r.name_en = Some(format!("Index {i}"));
                }
                r
            })
            .collect();

        let filtered = MarketPanel::filter_rows(&rows, 0);
        assert_eq!(
            filtered.len(),
            5000,
            "P1: industry segment keeps exactly half of 10k rows"
        );
        // Spot-check a couple of mapped rows rather than all 5000 (fast), and
        // verify rows carrying name_en render English while others stay zh.
        let mut en_rendered = 0;
        let mut zh_fallback = 0;
        for r in filtered.iter().take(2000) {
            let cells = MarketPanel::row_cells(r);
            assert_eq!(cells.len(), 5, "each row maps to exactly 5 cells");
            match cells[0] {
                DataCell::Text(ref name) => {
                    if r.name_en.is_some() && name.starts_with("Index ") {
                        en_rendered += 1;
                    } else if name == &r.name {
                        zh_fallback += 1;
                    }
                }
                _ => panic!("P1: name cell must be Text"),
            }
        }
        assert!(
            en_rendered > 0 && zh_fallback > 0,
            "P1: en-locale mapping must render both English (name_en rows) and zh fallback rows"
        );
        // The price cell must stay stable at 3000.0 across the whole pass.
        let all_price_cells: Vec<DataCell> =
            filtered.iter().flat_map(MarketPanel::row_cells).collect();
        assert!(
            all_price_cells
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 5 == 2)
                .all(|(_, c)| matches!(c, DataCell::Price { value, change: None } if value.round() as i64 == 3000)),
            "P1: latest column must be a price cell of 3000.0 for every row"
        );
        compass_i18n::set_locale("zh");
    }
}
