//! Screener panel citizen — condition input + results table.

use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use compass_types::{
    BreakoutCondition, MaCondition, MomentumCondition, ScreenerQuery, VolumeCondition,
};
use compass_ui::tokens::ThemeTokens;
use compass_ui::widgets::button::{Button, ButtonSize, ButtonVariant};
use compass_ui::widgets::card::Card;
use compass_ui::widgets::checkbox::Checkbox;
use compass_ui::widgets::data_table::{ColumnSpec, DataCell, DataTable};
use compass_ui::widgets::dropdown::Dropdown;
use compass_ui::widgets::multi_select::MultiSelect;
use compass_ui::widgets::section_title::SectionTitle;

use crate::messages::{FetchRequest, RunScreenerRequest};
use crate::state::SharedState;

/// Mutable UI state for the condition form.
#[derive(Default)]
struct ConditionForm {
    list_years: Option<u32>,
    market_cap_min: Option<f64>,
    market_cap_max: Option<f64>,
    exclude_delisted: bool,
    ma_enabled: bool,
    ma_kind: MaKind,
    breakout_enabled: bool,
    breakout_days: u32,
    momentum_enabled: bool,
    momentum_days: u32,
    momentum_min_pct: f64,
    momentum_max_pct: f64,
    volume_enabled: bool,
    volume_days: u32,
    volume_times: f64,
}

/// MA condition selector options.
#[derive(Default, Clone, Copy, PartialEq)]
enum MaKind {
    #[default]
    AboveMa20,
    AboveMa60,
    BullishAlign,
}

impl MaKind {
    fn label(self) -> &'static str {
        match self {
            Self::AboveMa20 => "站上 MA20",
            Self::AboveMa60 => "站上 MA60",
            Self::BullishAlign => "多头排列 MA5>MA20>MA60",
        }
    }
}

/// Results table column specs (design §6.6).
const COLUMNS: [ColumnSpec; 6] = [
    ColumnSpec {
        header: "代码",
        numeric: false,
    },
    ColumnSpec {
        header: "名称",
        numeric: false,
    },
    ColumnSpec {
        header: "最新价",
        numeric: true,
    },
    ColumnSpec {
        header: "20日涨跌幅",
        numeric: true,
    },
    ColumnSpec {
        header: "市值(亿)",
        numeric: true,
    },
    ColumnSpec {
        header: "行业",
        numeric: false,
    },
];

/// Index of the market-cap column — the screener's default sort target
/// (descending, biggest first), matching the pre-componentization behavior.
const MARKET_CAP_COLUMN: usize = 4;

/// Screener panel citizen.
///
/// Renders the condition form (two card sections) and the results table.
/// The heavy lifting runs on the backend via `run_screener_signal`.
pub struct ScreenerPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    form: ConditionForm,
    /// Theme tokens copied at construction (component styling).
    tokens: ThemeTokens,
    /// Industry multi-select (options refreshed each frame).
    ms_industry: MultiSelect,
    /// Exchange multi-select (fixed SH/SZ/BJ).
    ms_exchange: MultiSelect,
    /// Board multi-select (options refreshed each frame).
    ms_board: MultiSelect,
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
        let mut form = ConditionForm {
            exclude_delisted: true,
            breakout_days: BreakoutCondition::default().days,
            momentum_days: MomentumCondition::default().days,
            momentum_min_pct: MomentumCondition::default().min_pct,
            momentum_max_pct: MomentumCondition::default().max_pct,
            volume_days: VolumeCondition::default().days,
            volume_times: VolumeCondition::default().times,
            ..ConditionForm::default()
        };
        let mut ms_industry = MultiSelect::new(tokens, std::iter::empty::<&str>());
        let mut ms_exchange = MultiSelect::new(tokens, ["SH", "SZ", "BJ"]);
        let mut ms_board = MultiSelect::new(tokens, std::iter::empty::<&str>());
        if let Some(q) = restore {
            form.list_years = q.list_years;
            form.market_cap_min = q.market_cap_min;
            form.market_cap_max = q.market_cap_max;
            form.exclude_delisted = q.exclude_delisted;
            form.ma_enabled = q.ma.is_some();
            form.ma_kind = match q.ma {
                Some(MaCondition::AboveMa60) => MaKind::AboveMa60,
                Some(MaCondition::BullishAlign) => MaKind::BullishAlign,
                _ => MaKind::AboveMa20,
            };
            form.breakout_enabled = q.breakout.is_some();
            if let Some(b) = q.breakout {
                form.breakout_days = b.days;
            }
            form.momentum_enabled = q.momentum.is_some();
            if let Some(m) = q.momentum {
                form.momentum_days = m.days;
                form.momentum_min_pct = m.min_pct;
                form.momentum_max_pct = m.max_pct;
            }
            form.volume_enabled = q.volume.is_some();
            if let Some(v) = q.volume {
                form.volume_days = v.days;
                form.volume_times = v.times;
            }
            ms_industry = ms_industry.selected(q.industries.iter().cloned());
            ms_exchange = ms_exchange.selected(q.exchanges.iter().cloned());
            ms_board = ms_board.selected(q.boards.iter().cloned());
        }
        let mut table = DataTable::new(tokens, COLUMNS.to_vec());
        table.set_sort(MARKET_CAP_COLUMN, true);
        table.set_descending_default(MARKET_CAP_COLUMN, true);
        Self {
            citizen_id,
            citizen_state,
            form,
            tokens: *tokens,
            ms_industry,
            ms_exchange,
            ms_board,
            table,
            on_save,
        }
    }

    /// Build the query from the form state plus the multi-select selections.
    fn build_query(&self) -> ScreenerQuery {
        ScreenerQuery {
            industries: self.ms_industry.selected.clone(),
            exchanges: self.ms_exchange.selected.clone(),
            boards: self.ms_board.selected.clone(),
            list_years: self.form.list_years,
            market_cap_min: self.form.market_cap_min,
            market_cap_max: self.form.market_cap_max,
            exclude_delisted: self.form.exclude_delisted,
            ma: self.form.ma_enabled.then_some(match self.form.ma_kind {
                MaKind::AboveMa20 => MaCondition::AboveMa20,
                MaKind::AboveMa60 => MaCondition::AboveMa60,
                MaKind::BullishAlign => MaCondition::BullishAlign,
            }),
            breakout: self
                .form
                .breakout_enabled
                .then(|| BreakoutCondition::new(self.form.breakout_days)),
            momentum: self.form.momentum_enabled.then(|| {
                MomentumCondition::new(
                    self.form.momentum_days,
                    self.form.momentum_min_pct,
                    self.form.momentum_max_pct,
                )
            }),
            volume: self
                .form
                .volume_enabled
                .then(|| VolumeCondition::new(self.form.volume_days, self.form.volume_times)),
        }
    }

    /// Render the panel: condition form + results area.
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
            self.condition_form(ui, industries, boards);

            ui.add_space(self.form_tokens().spacing.sm);
            if Button::new(&self.form_tokens(), "筛选")
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Md)
                .show(ui)
                .clicked()
            {
                let query = self.build_query();
                (self.on_save)(&query);
                shared_state.screener_loading.set(true);
                shared_state.screener_error.set(None);
                if let Err(e) = run_screener_signal.send(RunScreenerRequest { query }) {
                    shared_state.screener_loading.set(false);
                    shared_state
                        .screener_error
                        .set(Some(format!("failed to run screener: {e}")));
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
            ui.label("筛选进行中…");
        } else if let Some(err) = shared_state.screener_error.get() {
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
                change: Some(row.change_20d as f32),
            },
            DataCell::Count(row.market_cap.round() as usize),
            DataCell::Text(row.industry.clone()),
        ]
    }

    /// Condition form split into two card sections (design §6.6): 基础条件
    /// (filters) and 技术面条件 (indicator toggles).
    fn condition_form(&mut self, ui: &mut egui::Ui, industries: &[String], boards: &[String]) {
        self.ms_industry.options = industries.to_vec();
        self.ms_board.options = boards.to_vec();
        let tokens = self.form_tokens();

        ui.vertical(|ui| {
            Card::new(&tokens)
                .title("基础条件")
                .padding(compass_ui::widgets::card::CardPadding::Md)
                .show(ui, |ui| {
                    self.basic_conditions(ui);
                });
            ui.add_space(tokens.spacing.sm);
            Card::new(&tokens)
                .title("技术面条件")
                .padding(compass_ui::widgets::card::CardPadding::Md)
                .show(ui, |ui| {
                    self.technical_conditions(ui);
                });
        });
    }

    /// 基础条件 card: industry/exchange/board multi-selects, listing years,
    /// market-cap range and the delisted-exclusion checkbox.
    fn basic_conditions(&mut self, ui: &mut egui::Ui) {
        let tokens = self.form_tokens();

        ui.horizontal_wrapped(|ui| {
            SectionTitle::new(&tokens, "行业").show(ui);
            self.ms_industry.show(ui);
            ui.add_space(tokens.spacing.md);

            SectionTitle::new(&tokens, "交易所").show(ui);
            self.ms_exchange.show(ui);
            ui.add_space(tokens.spacing.md);

            SectionTitle::new(&tokens, "板块").show(ui);
            self.ms_board.show(ui);
            ui.add_space(tokens.spacing.md);

            SectionTitle::new(&tokens, "上市时长").show(ui);
            let options = ["不限", "≥1年", "≥3年", "≥5年"];
            let values: [Option<u32>; 4] = [None, Some(1), Some(3), Some(5)];
            let current = options
                .iter()
                .zip(values.iter())
                .position(|(_, v)| *v == self.form.list_years)
                .unwrap_or(0);
            if let Some(idx) = Dropdown::new(&tokens, options)
                .selected(current)
                .width(100.0)
                .show(ui)
            {
                self.form.list_years = values[idx];
            }
            ui.add_space(tokens.spacing.md);

            SectionTitle::new(&tokens, "市值(亿)").show(ui);
            let mut min = self.form.market_cap_min.unwrap_or(0.0);
            if ui
                .add(egui::DragValue::new(&mut min).speed(1.0).prefix("min "))
                .changed()
            {
                self.form.market_cap_min = (min > 0.0).then_some(min);
            }
            let mut max = self.form.market_cap_max.unwrap_or(0.0);
            if ui
                .add(egui::DragValue::new(&mut max).speed(1.0).prefix("max "))
                .changed()
            {
                self.form.market_cap_max = (max > 0.0).then_some(max);
            }
            ui.add_space(tokens.spacing.md);

            Checkbox::new(&tokens, &mut self.form.exclude_delisted, "排除退市").show(ui);
        });
    }

    /// 技术面条件 card: MA / breakout / momentum / volume toggles.
    fn technical_conditions(&mut self, ui: &mut egui::Ui) {
        let tokens = self.form_tokens();

        ui.horizontal_wrapped(|ui| {
            Checkbox::new(&tokens, &mut self.form.ma_enabled, "均线").show(ui);
            if self.form.ma_enabled {
                let current = match self.form.ma_kind {
                    MaKind::AboveMa20 => 0,
                    MaKind::AboveMa60 => 1,
                    MaKind::BullishAlign => 2,
                };
                if let Some(idx) = Dropdown::new(
                    &tokens,
                    [
                        MaKind::AboveMa20.label(),
                        MaKind::AboveMa60.label(),
                        MaKind::BullishAlign.label(),
                    ],
                )
                .selected(current)
                .width(210.0)
                .show(ui)
                {
                    self.form.ma_kind = match idx {
                        1 => MaKind::AboveMa60,
                        2 => MaKind::BullishAlign,
                        _ => MaKind::AboveMa20,
                    };
                }
            }
            ui.add_space(tokens.spacing.md);

            Checkbox::new(&tokens, &mut self.form.breakout_enabled, "突破新高").show(ui);
            if self.form.breakout_enabled {
                ui.label("N:");
                ui.add(egui::DragValue::new(&mut self.form.breakout_days).range(1..=250));
            }
            ui.add_space(tokens.spacing.md);

            Checkbox::new(&tokens, &mut self.form.momentum_enabled, "动量").show(ui);
            if self.form.momentum_enabled {
                ui.label("N:");
                ui.add(egui::DragValue::new(&mut self.form.momentum_days).range(1..=250));
                ui.label("min%:");
                ui.add(egui::DragValue::new(&mut self.form.momentum_min_pct).speed(1.0));
                ui.label("max%:");
                ui.add(egui::DragValue::new(&mut self.form.momentum_max_pct).speed(1.0));
            }
            ui.add_space(tokens.spacing.md);

            Checkbox::new(&tokens, &mut self.form.volume_enabled, "量能").show(ui);
            if self.form.volume_enabled {
                ui.label("N:");
                ui.add(egui::DragValue::new(&mut self.form.volume_days).range(1..=80));
                ui.label("倍数:");
                ui.add(egui::DragValue::new(&mut self.form.volume_times).speed(0.1));
            }
        });
    }

    /// The panel's theme tokens (copied at construction).
    fn form_tokens(&self) -> ThemeTokens {
        self.tokens
    }

    /// Update the theme tokens after a theme switch so the condition cards
    /// and results table restyle without losing the query state.
    pub fn set_tokens(&mut self, tokens: ThemeTokens) {
        self.tokens = tokens;
        self.ms_industry.set_tokens(tokens);
        self.ms_exchange.set_tokens(tokens);
        self.ms_board.set_tokens(tokens);
        self.table.set_tokens(tokens);
    }
}

/// Row-click linkage: fetch bars for the clicked result row (design §6.6).
fn dispatch_row_fetch(
    shared_state: &SharedState,
    work_signal: &Signal<FetchRequest>,
    rows: &[compass_types::ScreenerRow],
    idx: usize,
) {
    if let Some(row) = rows.get(idx) {
        shared_state.symbol.set(row.symbol.clone());
        let timeframe = shared_state.timeframe.get();
        crate::dispatcher::handle(
            crate::messages::AppMessage::FetchBars,
            shared_state,
            work_signal,
            timeframe,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_ui::tokens::ThemeTokens;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    fn panel_with_form() -> (ScreenerPanel, SharedState) {
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let panel = ScreenerPanel::new(id, state, None, Box::new(|_| {}), &tokens);
        (panel, SharedState::new("000001", "1d"))
    }

    #[test]
    fn new_creates_panel_with_correct_id() {
        let (panel, _) = panel_with_form();
        assert_eq!(panel.id(), &CitizenId::new("screener"));
    }

    #[test]
    fn new_form_defaults_match_query_contract() {
        let (panel, _) = panel_with_form();
        let q = panel.build_query();
        assert!(q.exclude_delisted, "exclude_delisted defaults true");
        assert_eq!(q.breakout, None);
        assert_eq!(q.momentum, None);
        assert_eq!(q.volume, None);
        assert_eq!(q.ma, None);
        assert!(q.industries.is_empty());
    }

    #[test]
    fn build_query_reflects_conditions() {
        let (mut panel, _) = panel_with_form();
        panel.form.ma_enabled = true;
        panel.form.ma_kind = MaKind::BullishAlign;
        panel.form.breakout_enabled = true;
        panel.form.breakout_days = 120;
        panel.form.momentum_enabled = true;
        panel.form.volume_enabled = true;
        panel.form.market_cap_min = Some(100.0);
        panel.ms_industry.toggle("白酒");

        let q = panel.build_query();
        assert_eq!(q.ma, Some(MaCondition::BullishAlign));
        assert_eq!(q.breakout, Some(BreakoutCondition::new(120)));
        assert_eq!(q.momentum, Some(MomentumCondition::default()));
        assert_eq!(q.volume, Some(VolumeCondition::default()));
        assert_eq!(q.market_cap_min, Some(100.0));
        assert_eq!(q.industries, vec!["白酒".to_string()]);
    }

    #[test]
    fn restore_seeds_form_and_multi_selects() {
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let tokens = ThemeTokens::dark();
        let query = ScreenerQuery {
            industries: vec!["银行".to_string()],
            ma: Some(MaCondition::BullishAlign),
            ..ScreenerQuery::default()
        };
        let panel = ScreenerPanel::new(id, state, Some(&query), Box::new(|_| {}), &tokens);

        let q = panel.build_query();
        assert_eq!(q.industries, vec!["银行".to_string()]);
        assert_eq!(q.ma, Some(MaCondition::BullishAlign));
    }

    #[test]
    fn multi_selects_are_independent() {
        let (mut panel, _) = panel_with_form();
        panel.ms_industry.toggle("银行");
        panel.ms_exchange.toggle("SH");

        assert!(
            panel.ms_board.selected.is_empty(),
            "board selection must stay untouched"
        );
        let q = panel.build_query();
        assert_eq!(q.industries, vec!["银行".to_string()]);
        assert_eq!(q.exchanges, vec!["SH".to_string()]);
        assert!(q.boards.is_empty());
    }

    #[test]
    fn show_renders_condition_form_no_panic() {
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
        let _ = harness.get_by_label("基础条件");
        let _ = harness.get_by_label("技术面条件");
        let _ = harness.get_by_label("排除退市");
    }

    #[test]
    fn filter_button_click_sets_loading() {
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
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(3);
        shared.screener_result.set(vec![
            sample_row("000001", "平安银行", 100.0),
            sample_row("600519", "贵州茅台", 200.0),
            sample_row("000002", "万科A", 50.0),
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
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(1);
        shared
            .screener_result
            .set(vec![sample_row("000001", "平安银行", 0.0)]);
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
        let cells = ScreenerPanel::row_cells(&sample_row("600519", "贵州茅台", 200.0));
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[0], DataCell::Text("600519".to_string()));
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
        let shared = SharedState::new("000001", "1d");
        // The work slot must stay alive so the signal send succeeds.
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let rows = vec![sample_row("600519", "贵州茅台", 200.0)];

        dispatch_row_fetch(&shared, &work_signal, &rows, 0);

        assert_eq!(shared.symbol.get(), "600519");
        assert!(
            shared.loading.get(),
            "row click must dispatch a FetchBars request"
        );
    }

    #[test]
    fn dispatch_row_fetch_ignores_out_of_range_index() {
        let shared = SharedState::new("000001", "1d");
        let (_, work_signal) = signals();
        let rows = vec![sample_row("600519", "贵州茅台", 200.0)];

        dispatch_row_fetch(&shared, &work_signal, &rows, 5);

        assert_eq!(
            shared.symbol.get(),
            "000001",
            "out-of-range index is a no-op"
        );
    }
}
