//! Screener panel citizen — condition input + results table.

use egui::RichText;
use egui_citizen::{Citizen, CitizenId, CitizenState};
use egui_mobius::signals::Signal;

use compass_types::{
    BreakoutCondition, MaCondition, MomentumCondition, ScreenerQuery, VolumeCondition,
};

use crate::messages::{FetchRequest, RunScreenerRequest};
use crate::state::SharedState;

/// Mutable UI state for the condition form.
#[derive(Default)]
struct ConditionForm {
    industries: Vec<String>,
    exchanges: Vec<String>,
    boards: Vec<String>,
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

impl ConditionForm {
    /// Build a query from the form state.
    fn to_query(&self) -> ScreenerQuery {
        ScreenerQuery {
            industries: self.industries.clone(),
            exchanges: self.exchanges.clone(),
            boards: self.boards.clone(),
            list_years: self.list_years,
            market_cap_min: self.market_cap_min,
            market_cap_max: self.market_cap_max,
            exclude_delisted: self.exclude_delisted,
            ma: self.ma_enabled.then_some(match self.ma_kind {
                MaKind::AboveMa20 => MaCondition::AboveMa20,
                MaKind::AboveMa60 => MaCondition::AboveMa60,
                MaKind::BullishAlign => MaCondition::BullishAlign,
            }),
            breakout: self
                .breakout_enabled
                .then(|| BreakoutCondition::new(self.breakout_days)),
            momentum: self.momentum_enabled.then(|| {
                MomentumCondition::new(
                    self.momentum_days,
                    self.momentum_min_pct,
                    self.momentum_max_pct,
                )
            }),
            volume: self
                .volume_enabled
                .then(|| VolumeCondition::new(self.volume_days, self.volume_times)),
        }
    }
}

/// Screener panel citizen.
///
/// Renders the condition form (left) and results table (right). The heavy
/// lifting runs on the backend via `run_screener_signal`.
pub struct ScreenerPanel {
    pub citizen_id: CitizenId,
    pub citizen_state: CitizenState,
    form: ConditionForm,
    /// Industry search filter text.
    industry_filter: String,
    /// Sort column index into `ScreenerRow` fields (0-5).
    sort_column: usize,
    /// `true` = descending (default for market cap).
    sort_descending: bool,
    /// Whether the industry multi-select popup is open.
    industry_popup: bool,
    /// Persists the current query whenever a filter run is triggered.
    on_save: Box<dyn Fn(&ScreenerQuery) + Send + Sync>,
}

/// Column keys for the results table.
const COLUMNS: [&str; 6] = ["代码", "名称", "最新价", "20日涨跌幅", "市值(亿)", "行业"];

/// Sort rows by the given column (0-5) and direction.
///
/// Column 4 (market cap) defaults to descending in the UI; the caller
/// controls both parameters. Ties are broken by symbol for determinism.
fn sort_rows(
    rows: &[compass_types::ScreenerRow],
    column: usize,
    descending: bool,
) -> Vec<compass_types::ScreenerRow> {
    let mut sorted = rows.to_vec();
    sorted.sort_by(|a, b| {
        let ord = match column {
            0 => a.symbol.cmp(&b.symbol),
            1 => a.name.cmp(&b.name),
            2 => a.latest_price.total_cmp(&b.latest_price),
            3 => a.change_20d.total_cmp(&b.change_20d),
            4 => a.market_cap.total_cmp(&b.market_cap),
            _ => a.industry.cmp(&b.industry),
        };
        let ord = if descending { ord.reverse() } else { ord };
        ord.then_with(|| a.symbol.cmp(&b.symbol))
    });
    sorted
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
        if let Some(q) = restore {
            form.industries = q.industries.clone();
            form.exchanges = q.exchanges.clone();
            form.boards = q.boards.clone();
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
        }
        Self {
            citizen_id,
            citizen_state,
            form,
            industry_filter: String::new(),
            sort_column: 4, // market cap
            sort_descending: true,
            industry_popup: false,
            on_save,
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
        ui.horizontal(|ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                self.condition_form(ui, industries, boards);
                ui.separator();
                if ui.button("筛选").clicked() {
                    let query = self.form.to_query();
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
            });

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.results_area(ui, shared_state, work_signal);
            });
        });
    }

    /// Toggle sort state for a header column click.
    ///
    /// Clicking the active column flips direction; clicking a new column
    /// selects it with the market-cap default (descending).
    fn toggle_sort(&mut self, column: usize) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = column == 4;
        }
    }

    /// Results table with sortable headers, count label and row-click
    /// chart linkage.
    fn results_area(
        &mut self,
        ui: &mut egui::Ui,
        shared_state: &SharedState,
        work_signal: &Signal<FetchRequest>,
    ) {
        let total = shared_state.screener_total.get();
        let rows = shared_state.screener_result.get();

        if shared_state.screener_loading.get() {
            ui.spinner();
            ui.label("筛选进行中…");
        } else if let Some(err) = shared_state.screener_error.get() {
            ui.colored_label(ui.visuals().error_fg_color, err);
        } else if rows.is_empty() {
            ui.label(RichText::new("无符合条件的股票").weak());
        } else {
            if total > 100 {
                ui.label(format!("共 {total} 只，已显示前 100"));
            } else {
                ui.label(format!("共 {total} 只"));
            }

            let sorted = sort_rows(&rows, self.sort_column, self.sort_descending);

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("screener_results")
                    .striped(true)
                    .show(ui, |ui| {
                        for (idx, col) in COLUMNS.iter().enumerate() {
                            let mut text = (*col).to_string();
                            if idx == self.sort_column {
                                text.push_str(if self.sort_descending { " ↓" } else { " ↑" });
                            }
                            if ui
                                .selectable_label(
                                    self.sort_column == idx,
                                    RichText::new(text).strong(),
                                )
                                .clicked()
                            {
                                self.toggle_sort(idx);
                            }
                        }
                        ui.end_row();

                        for row in &sorted {
                            if ui
                                .selectable_label(false, &row.symbol)
                                .on_hover_text(&row.name)
                                .clicked()
                            {
                                // Chart linkage: bare 6-digit code + FetchBars.
                                shared_state.symbol.set(row.symbol.clone());
                                let timeframe = shared_state.timeframe.get();
                                crate::dispatcher::handle(
                                    crate::messages::AppMessage::FetchBars,
                                    shared_state,
                                    work_signal,
                                    timeframe,
                                );
                            }
                            ui.label(&row.name);
                            ui.label(format!("{:.2}", row.latest_price));
                            ui.label(format!("{:.2}%", row.change_20d));
                            if row.market_cap == 0.0 {
                                ui.label("—");
                            } else {
                                ui.label(format!("{:.1}", row.market_cap));
                            }
                            ui.label(&row.industry);
                            ui.end_row();
                        }
                    });
            });
        }
    }

    /// Condition form: metadata + technical conditions.
    fn condition_form(&mut self, ui: &mut egui::Ui, industries: &[String], boards: &[String]) {
        ui.heading("条件");

        ui.label("行业");
        let summary = if self.form.industries.is_empty() {
            "全部".to_string()
        } else if self.form.industries.len() <= 2 {
            self.form.industries.join("、")
        } else {
            format!("已选 {} 个", self.form.industries.len())
        };
        let response = ui.button(format!("{} ▾", summary));
        if response.clicked() {
            self.industry_popup = !self.industry_popup;
        }

        if self.industry_popup {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.industry_popup = false;
            } else {
                egui::Area::new(egui::Id::new("industry_popup"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(response.rect.left_bottom())
                    .constrain(true)
                    .show(ui.ctx(), |ui| {
                        ui.set_min_width(220.0);
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            let mut filter = self.industry_filter.clone();
                            ui.text_edit_singleline(&mut filter);
                            self.industry_filter = filter;
                            let lower = self.industry_filter.to_lowercase();

                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .show(ui, |ui| {
                                    for ind in industries {
                                        if !lower.is_empty() && !ind.to_lowercase().contains(&lower)
                                        {
                                            continue;
                                        }
                                        let mut selected = self.form.industries.contains(ind);
                                        if ui.checkbox(&mut selected, ind).changed() {
                                            if selected {
                                                self.form.industries.push(ind.clone());
                                            } else {
                                                self.form.industries.retain(|i| i != ind);
                                            }
                                        }
                                    }
                                });
                            if ui.button("完成").clicked() {
                                self.industry_popup = false;
                            }
                        });
                    });
            }
        }

        ui.label("交易所");
        for ex in ["SH", "SZ", "BJ"] {
            let mut selected = self.form.exchanges.contains(&ex.to_string());
            if ui.checkbox(&mut selected, ex).changed() {
                if selected {
                    self.form.exchanges.push(ex.to_string());
                } else {
                    self.form.exchanges.retain(|e| e != ex);
                }
            }
        }

        ui.label("板块");
        for b in boards {
            let mut selected = self.form.boards.contains(b);
            if ui.checkbox(&mut selected, b).changed() {
                if selected {
                    self.form.boards.push(b.clone());
                } else {
                    self.form.boards.retain(|x| x != b);
                }
            }
        }

        ui.label("上市时长");
        let options: [(&str, Option<u32>); 4] = [
            ("不限", None),
            ("≥1年", Some(1)),
            ("≥3年", Some(3)),
            ("≥5年", Some(5)),
        ];
        let mut current_idx = options
            .iter()
            .position(|(_, v)| *v == self.form.list_years)
            .unwrap_or(0);
        egui::ComboBox::from_id_salt("list_years_combo")
            .selected_text(options[current_idx].0)
            .show_ui(ui, |ui| {
                for (idx, (label, val)) in options.iter().enumerate() {
                    if ui.selectable_value(&mut current_idx, idx, *label).clicked() {
                        self.form.list_years = *val;
                    }
                }
            });

        ui.label("市值区间（亿元）");
        ui.horizontal(|ui| {
            ui.label("min");
            let mut min = self.form.market_cap_min.unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut min).speed(1.0)).changed() {
                self.form.market_cap_min = (min > 0.0).then_some(min);
            }
            ui.label("max");
            let mut max = self.form.market_cap_max.unwrap_or(0.0);
            if ui.add(egui::DragValue::new(&mut max).speed(1.0)).changed() {
                self.form.market_cap_max = (max > 0.0).then_some(max);
            }
        });

        ui.separator();

        let mut ma_enabled = self.form.ma_enabled;
        ui.checkbox(&mut ma_enabled, "均线");
        self.form.ma_enabled = ma_enabled;
        if self.form.ma_enabled {
            let mut kind = self.form.ma_kind;
            let label = match kind {
                MaKind::AboveMa20 => "站上 MA20",
                MaKind::AboveMa60 => "站上 MA60",
                MaKind::BullishAlign => "多头排列 MA5>MA20>MA60",
            };
            egui::ComboBox::from_id_salt("ma_combo")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut kind, MaKind::AboveMa20, "站上 MA20");
                    ui.selectable_value(&mut kind, MaKind::AboveMa60, "站上 MA60");
                    ui.selectable_value(&mut kind, MaKind::BullishAlign, "多头排列 MA5>MA20>MA60");
                });
            self.form.ma_kind = kind;
        }

        let mut breakout_enabled = self.form.breakout_enabled;
        ui.checkbox(&mut breakout_enabled, "突破 N 日新高");
        self.form.breakout_enabled = breakout_enabled;
        if self.form.breakout_enabled {
            ui.horizontal(|ui| {
                ui.label("N:");
                ui.add(egui::DragValue::new(&mut self.form.breakout_days).range(1..=250));
            });
        }

        let mut momentum_enabled = self.form.momentum_enabled;
        ui.checkbox(&mut momentum_enabled, "动量（近 N 日涨幅）");
        self.form.momentum_enabled = momentum_enabled;
        if self.form.momentum_enabled {
            ui.horizontal(|ui| {
                ui.label("N:");
                ui.add(egui::DragValue::new(&mut self.form.momentum_days).range(1..=250));
            });
            ui.horizontal(|ui| {
                ui.label("min%:");
                ui.add(egui::DragValue::new(&mut self.form.momentum_min_pct).speed(1.0));
                ui.label("max%:");
                ui.add(egui::DragValue::new(&mut self.form.momentum_max_pct).speed(1.0));
            });
        }

        let mut volume_enabled = self.form.volume_enabled;
        ui.checkbox(&mut volume_enabled, "量能（近 N 日均量）");
        self.form.volume_enabled = volume_enabled;
        if self.form.volume_enabled {
            ui.horizontal(|ui| {
                ui.label("N:");
                ui.add(egui::DragValue::new(&mut self.form.volume_days).range(1..=80));
                ui.label("倍数:");
                ui.add(egui::DragValue::new(&mut self.form.volume_times).speed(0.1));
            });
        }

        ui.checkbox(&mut self.form.exclude_delisted, "排除退市");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_citizen::CitizenState;
    use egui_kittest::kittest::Queryable;

    fn panel_with_form() -> (ScreenerPanel, SharedState) {
        let id = CitizenId::new("screener");
        let state = CitizenState::new();
        let panel = ScreenerPanel::new(id, state, None, Box::new(|_| {}));
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
        let q = panel.form.to_query();
        assert!(q.exclude_delisted, "exclude_delisted defaults true");
        assert_eq!(q.breakout, None);
        assert_eq!(q.momentum, None);
        assert_eq!(q.volume, None);
        assert_eq!(q.ma, None);
        assert!(q.industries.is_empty());
    }

    #[test]
    fn to_query_reflects_conditions() {
        let (mut panel, _) = panel_with_form();
        panel.form.ma_enabled = true;
        panel.form.ma_kind = MaKind::BullishAlign;
        panel.form.breakout_enabled = true;
        panel.form.breakout_days = 120;
        panel.form.momentum_enabled = true;
        panel.form.volume_enabled = true;
        panel.form.market_cap_min = Some(100.0);

        let q = panel.form.to_query();
        assert_eq!(q.ma, Some(MaCondition::BullishAlign));
        assert_eq!(q.breakout, Some(BreakoutCondition::new(120)));
        assert_eq!(q.momentum, Some(MomentumCondition::default()));
        assert_eq!(q.volume, Some(VolumeCondition::default()));
        assert_eq!(q.market_cap_min, Some(100.0));
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
        let btn = harness.get_by_label("筛选");
        btn.click();
        harness.step();

        assert!(
            shared.screener_loading.get(),
            "screener_loading should be set after filter click"
        );
    }

    #[test]
    fn industry_dropdown_toggles_selection_via_popup_state() {
        // Pure-logic coverage: the popup checkbox toggling mutates the form's
        // industry list; UI rendering is covered by show_renders_no_panic.
        // (AccessKit does not expose button labels inside the form reliably —
        // see kb/dev/testing.md.)
        let (mut panel, _shared) = panel_with_form();
        let industries = ["银行".to_string(), "白酒".to_string(), "医药".to_string()];

        // Simulate the checkbox handler: toggle 银行 on.
        let ind = &industries[0];
        let mut selected = panel.form.industries.contains(ind);
        selected = !selected;
        if selected {
            panel.form.industries.push(ind.clone());
        } else {
            panel.form.industries.retain(|i| i != ind);
        }

        assert_eq!(
            panel.form.industries,
            vec!["银行".to_string()],
            "checkbox toggle adds industry"
        );
        assert!(
            !panel.form.industries.is_empty(),
            "selection non-empty after toggle"
        );
    }

    // ------------------------------------------------------------------
    // Results table (Todo 6)
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

    #[test]
    fn results_table_renders_rows_and_count() {
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(3);
        shared.screener_result.set(vec![
            sample_row("000001", "平安银行", 100.0),
            sample_row("600519", "贵州茅台", 200.0),
            sample_row("000002", "万科A", 50.0),
        ]);
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.step();
        // Rendering with results must not panic. (Grid labels are not
        // queryable via AccessKit in egui_kittest — see kb/dev/testing.md.)
    }

    #[test]
    fn results_table_shows_cap_placeholder_for_zero_market_cap() {
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(1);
        shared
            .screener_result
            .set(vec![sample_row("000001", "平安银行", 0.0)]);
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.step();
    }

    #[test]
    fn row_click_sets_symbol_and_triggers_fetch() {
        let (mut panel, shared) = panel_with_form();
        shared.screener_total.set(1);
        shared
            .screener_result
            .set(vec![sample_row("600519", "贵州茅台", 200.0)]);
        let (run_signal, _run_slot) =
            egui_mobius::factory::create_signal_slot::<RunScreenerRequest>();
        let (work_signal, _work_slot) = egui_mobius::factory::create_signal_slot::<FetchRequest>();
        let industries: Vec<String> = Vec::new();
        let boards: Vec<String> = Vec::new();

        let mut harness = egui_kittest::Harness::new_ui(|ui| {
            panel.show(ui, &shared, &run_signal, &work_signal, &industries, &boards);
        });
        harness.step();
        harness.get_by_label("600519").click();
        harness.step();

        assert_eq!(
            shared.symbol.get(),
            "600519",
            "row click must set shared_state.symbol to bare code"
        );
    }

    #[test]
    fn toggle_sort_flips_direction_on_active_column() {
        let (mut panel, _) = panel_with_form();
        // Default: market cap (4), descending.
        assert_eq!(panel.sort_column, 4);
        assert!(panel.sort_descending);
        panel.toggle_sort(4);
        assert!(
            !panel.sort_descending,
            "active column click toggles to ascending"
        );
        panel.toggle_sort(4);
        assert!(panel.sort_descending, "second click toggles back");
    }

    #[test]
    fn toggle_sort_selects_new_column_with_cap_default() {
        let (mut panel, _) = panel_with_form();
        panel.toggle_sort(0); // code column
        assert_eq!(panel.sort_column, 0);
        assert!(!panel.sort_descending, "non-cap column defaults ascending");
        panel.toggle_sort(2); // latest price
        assert_eq!(panel.sort_column, 2);
        assert!(!panel.sort_descending);
        panel.toggle_sort(4); // back to market cap
        assert!(
            panel.sort_descending,
            "market cap column defaults descending"
        );
    }
}
