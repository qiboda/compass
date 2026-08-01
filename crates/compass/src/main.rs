use std::sync::Arc;
use std::time::Duration;

use egui_citizen::{CitizenId, Dispatcher};
use egui_dock::{DockArea, DockState, Style};
use egui_file_dialog::FileDialog;
use tracing::{debug, info};

use compass_core::data::parquet::ParquetReader;
use compass_core::model::AppConfig;

mod backend;
mod citizens;
mod dispatcher;
mod messages;
mod state;
mod tabs;
mod theme;
mod widgets;

use citizens::chart::ChartCitizen;
use citizens::logger::LoggerPanel;
use citizens::screener::ScreenerPanel;
use tabs::{CHART_ID, LOGGER_ID, SCREENER_ID, Tab, TabKind, TabViewer};
use theme::CompassTheme;
use widgets::modal::Modal;
use widgets::searchable_dropdown::StockPicker;
use widgets::toast::{ToastLevel, ToastManager};

fn setup_cjk_fonts(ctx: &egui::Context) {
    let font_path = "/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf";
    let font_bytes = match std::fs::read(font_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(path = font_path, error = %e, "CJK font not found, Chinese may display as tofu");
            return;
        }
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "SourceHanSansCN".into(),
        std::sync::Arc::new(egui::FontData::from_owned(font_bytes)),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "SourceHanSansCN".into());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.push("SourceHanSansCN".into());
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> eframe::Result {
    let _file_guard = init_tracing();
    let config = load_config();

    // Create reactive shared state
    let shared_state = Arc::new(state::SharedState::new(
        &config.app.default_symbol,
        &config.app.default_timeframe,
    ));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Compass — Stock Chart",
        options,
        Box::new(move |cc| {
            let egui_ctx = cc.egui_ctx.clone();

            setup_cjk_fonts(&egui_ctx);

            // Load stock list from parquet at startup
            let stock_list = load_stock_list(&config);

            // Wire Level 3 backend (signal/slot + AsyncDispatcher)
            let (work_signal, run_screener_signal, _backend_handle) =
                backend::wire_backend(config.clone(), shared_state.clone(), egui_ctx);

            // Register citizens
            let mut dispatcher = Dispatcher::new();
            let registered = dispatcher::register_citizens(&mut dispatcher);

            // Create citizen panels
            let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
            let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
            let screener = ScreenerPanel::new(CitizenId::new(SCREENER_ID), registered.screener);

            // Derive distinct industry/board lists for the screener conditions.
            let mut industries: Vec<String> = stock_list
                .iter()
                .filter_map(|s| s.industry.clone())
                .collect();
            industries.sort();
            industries.dedup();
            let mut boards: Vec<String> =
                stock_list.iter().filter_map(|s| s.board.clone()).collect();
            boards.sort();
            boards.dedup();

            // Create initial dock state: Chart (root), Logger + Screener below.
            let mut dock_state = DockState::new(vec![Tab::new(TabKind::Chart)]);
            if let Some(surface) = dock_state.get_surface_mut(egui_dock::SurfaceIndex::main())
                && let Some(tree) = surface.node_tree_mut()
            {
                let _ = tree.split_below(
                    egui_dock::NodeIndex::root(),
                    0.75,
                    vec![Tab::new(TabKind::Logger)],
                );
                let _ = tree.split_below(
                    egui_dock::NodeIndex::root(),
                    0.5,
                    vec![Tab::new(TabKind::Screener)],
                );
            }

            let stock_picker = StockPicker::new(&config.app.default_symbol, &stock_list);

            let theme = CompassTheme::from_config(&config.theme);
            let dock_style = Style::from_egui(&cc.egui_ctx.style_of(cc.egui_ctx.theme()));

            let startup_symbol = shared_state.symbol.get();

            Ok(Box::new(CompassApp {
                dock_state,
                dispatcher,
                chart,
                logger,
                screener,
                run_screener_signal,
                screener_industries: industries,
                screener_boards: boards,
                shared_state,
                work_signal,
                stock_list,
                stock_picker,
                timeframe_index: 0usize,
                theme,
                dock_style,
                _backend_handle,
                toast: ToastManager::new(),
                modal: Modal::new(),
                file_dialog: FileDialog::new(),
                last_error: None,
                last_loading: false,
                last_screener_error: None,
                last_screener_synced_symbol: startup_symbol,
            }))
        }),
    )
}

// ---------------------------------------------------------------------------
// Tracing initialization
// ---------------------------------------------------------------------------

/// Initializes tracing with stderr + daily rolling file output.
/// Returns a `WorkerGuard` that must be kept alive for the lifetime of the
/// application.
fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    // Ensure logs/ directory exists
    let _ = std::fs::create_dir_all("logs");

    let file_appender = tracing_appender::rolling::daily("logs", "compass.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_ansi(false).with_writer(non_blocking));

    #[cfg(feature = "tracy")]
    let registry = registry.with(tracing_tracy::TracyLayer::default());

    registry.init();

    info!("Logging initialized");

    guard
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Reads `~/.config/compass/config.toml`. Falls back to `AppConfig::default()`
/// if the file is missing or malformed.
fn load_config() -> AppConfig {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(cfg) => {
                tracing::info!(path = %config_path.display(), "config loaded");
                cfg
            }
            Err(e) => {
                tracing::warn!(path = %config_path.display(), error = %e, "failed to parse config, using defaults");
                AppConfig::default()
            }
        },
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "config file not found, using defaults");
            AppConfig::default()
        }
    }
}

fn load_stock_list(config: &AppConfig) -> Vec<compass_core::model::StockBasic> {
    match ParquetReader::new(&config.parquet.dir) {
        Ok(reader) => match reader.load_all_stock_basics() {
            Ok(stocks) => {
                info!(count = stocks.len(), "stock list loaded from parquet");
                stocks
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to load stock list from parquet");
                Vec::new()
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "failed to create parquet reader");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// CompassApp — citizen-based application
// ---------------------------------------------------------------------------

struct CompassApp {
    dock_state: DockState<Tab>,
    dispatcher: Dispatcher,
    chart: ChartCitizen,
    logger: LoggerPanel,
    screener: ScreenerPanel,
    run_screener_signal: egui_mobius::signals::Signal<messages::RunScreenerRequest>,
    screener_industries: Vec<String>,
    screener_boards: Vec<String>,
    shared_state: Arc<state::SharedState>,
    work_signal: egui_mobius::signals::Signal<messages::FetchRequest>,
    stock_list: Vec<compass_core::model::StockBasic>,
    stock_picker: StockPicker,
    timeframe_index: usize,
    theme: CompassTheme,
    dock_style: egui_dock::Style,
    _backend_handle: backend::BackendHandle,
    toast: ToastManager,
    modal: Modal,
    file_dialog: FileDialog,
    last_error: Option<String>,
    last_loading: bool,
    // Consumed by the screener reverse-sync + toast logic (Todo 6).
    #[allow(dead_code)]
    last_screener_error: Option<String>,
    #[allow(dead_code)]
    last_screener_synced_symbol: String,
}

impl eframe::App for CompassApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.theme.apply_theme(ui.ctx());

        // Toolbar with full-width background.
        let toolbar_bg = {
            let panel = ui.visuals().panel_fill;
            let (r, g, b) = (panel.r(), panel.g(), panel.b());
            if ui.visuals().dark_mode {
                egui::Color32::from_rgb(
                    r.saturating_sub(15),
                    g.saturating_sub(15),
                    b.saturating_sub(15),
                )
            } else {
                egui::Color32::from_rgb(
                    r.saturating_add(15),
                    g.saturating_add(15),
                    b.saturating_add(15),
                )
            }
        };
        egui::Frame::default().fill(toolbar_bg).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                self.render_toolbar(ui);
            });
        });

        // Dock area with explicit background matching the theme.
        let dock_bg = ui.visuals().panel_fill;
        egui::Frame::default().fill(dock_bg).show(ui, |ui| {
            DockArea::new(&mut self.dock_state)
                .style(self.dock_style.clone())
                .show_inside(
                    ui,
                    &mut TabViewer {
                        dispatcher: &mut self.dispatcher,
                        chart: &mut self.chart,
                        logger: &mut self.logger,
                        screener: &mut self.screener,
                        run_screener_signal: &self.run_screener_signal,
                        work_signal: &self.work_signal,
                        screener_industries: &self.screener_industries,
                        screener_boards: &self.screener_boards,
                        shared_state: &self.shared_state,
                        theme: &self.theme,
                    },
                );

            self.toast.render(ui.ctx());
            self.modal.show(ui.ctx());
            self.file_dialog.update(ui.ctx());

            dispatcher::drain_citizen(&mut self.dispatcher, &self.shared_state);

            ui.ctx().request_repaint_after(Duration::from_millis(200));
        });
    }
}

impl CompassApp {
    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} Symbol:",
                egui_phosphor::regular::MAGNIFYING_GLASS
            ));
            self.stock_picker.show(ui, &self.stock_list);

            ui.separator();

            ui.label(format!("{} TF:", egui_phosphor::regular::CLOCK));
            let mut tf = self.timeframe_index;
            egui::ComboBox::from_id_salt("timeframe_combo")
                .selected_text(timeframe_label(tf))
                .show_ui(ui, |ui| {
                    for i in 0..=2 {
                        let val = timeframe_label(i);
                        if ui.selectable_value(&mut tf, i, val).clicked() {
                            debug!(timeframe = val, "timeframe changed");
                        }
                    }
                });
            self.timeframe_index = tf;

            if ui
                .button(format!("{} Fetch", egui_phosphor::regular::DOWNLOAD_SIMPLE))
                .clicked()
            {
                let symbol = if self.stock_picker.selected_exchange.is_empty() {
                    self.stock_picker.selected_symbol.clone()
                } else {
                    format!(
                        "{}.{}",
                        self.stock_picker.selected_exchange.to_lowercase(),
                        self.stock_picker.selected_symbol
                    )
                };
                let timeframe = timeframe_value(self.timeframe_index);
                info!(
                    symbol = %symbol,
                    timeframe = %timeframe,
                    "fetch requested"
                );
                self.shared_state.symbol.set(symbol);
                self.shared_state.timeframe.set(timeframe.clone());
                dispatcher::handle(
                    messages::AppMessage::FetchBars,
                    &self.shared_state,
                    &self.work_signal,
                    timeframe,
                );
            }

            if self.shared_state.loading.get() {
                ui.spinner();
            }
            // Push error toast only on None→Some transition (not every frame)
            let current_err = self.shared_state.error.get();
            if current_err != self.last_error {
                if let Some(ref err) = current_err {
                    self.toast.push(ToastLevel::Error, err.clone());
                }
                self.last_error = current_err;
            }

            // Push success toast when loading transitions true→false with no error
            let current_loading = self.shared_state.loading.get();
            if self.last_loading && !current_loading && self.shared_state.error.get().is_none() {
                self.toast
                    .push(ToastLevel::Success, "Data fetched successfully");
            }
            self.last_loading = current_loading;

            ui.separator();

            ui.label(egui_phosphor::regular::PALETTE.to_string());
            let themes = CompassTheme::all_names();
            let mut current = self.theme.name().to_string();
            egui::ComboBox::from_id_salt("theme_combo")
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for &name in themes {
                        if ui
                            .selectable_value(&mut current, name.to_string(), name)
                            .clicked()
                        {
                            self.theme = CompassTheme::from_config(name);
                            self.theme.apply_theme(ui.ctx());
                            self.dock_style =
                                egui_dock::Style::from_egui(&ui.ctx().style_of(ui.ctx().theme()));
                        }
                    }
                });
        });
    }
}

fn timeframe_label(idx: usize) -> &'static str {
    match idx {
        0 => "1d",
        1 => "1w",
        2 => "1M",
        _ => "1d",
    }
}

fn timeframe_value(idx: usize) -> String {
    timeframe_label(idx).to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use compass_core::model::{Exchange, StockBasic};

    use crate::citizens::chart::ChartCitizen;
    use crate::citizens::logger::LoggerPanel;
    use crate::citizens::screener::ScreenerPanel;
    use crate::state::SharedState;
    use crate::tabs::{CHART_ID, LOGGER_ID, SCREENER_ID};
    use crate::timeframe_label;
    use crate::timeframe_value;
    use egui_citizen::{CitizenId, Dispatcher};

    #[test]
    #[cfg(feature = "tracy")]
    fn tracy_layer_constructs() {
        let _layer = tracing_tracy::TracyLayer::default();
    }

    #[test]
    fn shared_state_initializes_with_defaults() {
        let state = SharedState::new("000001", "1d");
        assert_eq!(state.symbol.get(), "000001");
        assert_eq!(state.bars.get().len(), 0);
        assert!(!state.loading.get());
        assert_eq!(state.error.get(), None);
        assert_eq!(state.log.get().log_count(), 0);
    }

    #[test]
    fn citizens_register_and_activate() {
        let mut dispatcher = Dispatcher::new();
        let registered = crate::dispatcher::register_citizens(&mut dispatcher);

        let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);

        assert_eq!(chart.citizen_id.0, CHART_ID);
        assert_eq!(logger.citizen_id.0, LOGGER_ID);
    }

    fn make_stock_basic(symbol: &str, name: &str, exchange: &str) -> StockBasic {
        StockBasic {
            symbol: symbol.into(),
            name: name.into(),
            area: None,
            industry: None,
            market: None,
            board: None,
            full_name: None,
            total_share: None,
            exchange: Some(exchange.into()),
            list_date: None,
            delist_date: None,
        }
    }

    fn build_stock_list() -> Vec<StockBasic> {
        vec![
            make_stock_basic("000001", "平安银行", "SZ"),
            make_stock_basic("000002", "万科A", "SZ"),
            make_stock_basic("300750", "宁德时代", "SZ"),
            make_stock_basic("600519", "贵州茅台", "SH"),
            make_stock_basic("600036", "招商银行", "SH"),
            make_stock_basic("688001", "华兴源创", "SH"),
            make_stock_basic("830799", "艾融软件", "BJ"),
        ]
    }

    #[test]
    fn filter_stocks_code_prefix_match() {
        let stocks = build_stock_list();
        let result =
            crate::widgets::searchable_dropdown::filter_stocks(&stocks, "600", &Exchange::All);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol, "600036");
        assert_eq!(result[1].symbol, "600519");
    }

    #[test]
    fn filter_stocks_name_substring_match() {
        let stocks = build_stock_list();
        let result =
            crate::widgets::searchable_dropdown::filter_stocks(&stocks, "银行", &Exchange::All);
        assert_eq!(result.len(), 2);
        let found: Vec<_> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(found.contains(&"000001"));
        assert!(found.contains(&"600036"));
    }

    #[test]
    fn filter_stocks_exchange_filter_sh() {
        let stocks = build_stock_list();
        let result = crate::widgets::searchable_dropdown::filter_stocks(&stocks, "", &Exchange::SH);
        assert_eq!(result.len(), 3);
        let symbols: Vec<_> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(symbols.contains(&"600519"));
        assert!(symbols.contains(&"688001"));
    }

    #[test]
    fn filter_stocks_exchange_filter_sz() {
        let stocks = build_stock_list();
        let result = crate::widgets::searchable_dropdown::filter_stocks(&stocks, "", &Exchange::SZ);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn filter_stocks_empty_query_returns_all_in_exchange() {
        let stocks = build_stock_list();
        let result =
            crate::widgets::searchable_dropdown::filter_stocks(&stocks, "", &Exchange::All);
        assert_eq!(result.len(), 7);
    }

    #[test]
    fn timeframe_label_returns_correct_values() {
        assert_eq!(timeframe_label(0), "1d");
        assert_eq!(timeframe_label(1), "1w");
        assert_eq!(timeframe_label(2), "1M");
        assert_eq!(timeframe_label(99), "1d");
    }

    #[test]
    fn timeframe_value_returns_correct_strings() {
        assert_eq!(timeframe_value(0), "1d");
        assert_eq!(timeframe_value(1), "1w");
        assert_eq!(timeframe_value(2), "1M");
    }

    // ======================================================================
    // #79 coverage gate: kittest integration + non-UI function tests
    // ======================================================================

    use crate::CompassApp;
    use crate::tabs::{Tab, TabKind};
    use crate::theme::CompassTheme;
    use crate::widgets::modal::Modal;
    use crate::widgets::searchable_dropdown::StockPicker;
    use crate::widgets::toast::ToastManager;
    use compass_core::model::AppConfig;
    use egui_dock::DockState;
    use egui_kittest::kittest::Queryable;
    use std::sync::Arc;

    fn build_compass_app(egui_ctx: egui::Context) -> CompassApp {
        let config = AppConfig::default();
        let shared_state = Arc::new(SharedState::new("000001", "1d"));

        let (work_signal, run_screener_signal, _backend_handle) =
            crate::backend::wire_backend(config, shared_state.clone(), egui_ctx);

        let mut dispatcher = Dispatcher::new();
        let registered = crate::dispatcher::register_citizens(&mut dispatcher);

        let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let screener = ScreenerPanel::new(CitizenId::new(SCREENER_ID), registered.screener);

        let stock_list: Vec<StockBasic> = Vec::new();
        let stock_picker = StockPicker::new("000001", &stock_list);
        let theme = CompassTheme::compass_dark();
        let dock_style = egui_dock::Style::default();

        let mut dock_state = DockState::new(vec![Tab::new(TabKind::Chart)]);
        if let Some(surface) = dock_state.get_surface_mut(egui_dock::SurfaceIndex::main())
            && let Some(tree) = surface.node_tree_mut()
        {
            let _ = tree.split_below(
                egui_dock::NodeIndex::root(),
                0.75,
                vec![Tab::new(TabKind::Logger)],
            );
            let _ = tree.split_below(
                egui_dock::NodeIndex::root(),
                0.5,
                vec![Tab::new(TabKind::Screener)],
            );
        }

        let startup_symbol = shared_state.symbol.get();

        CompassApp {
            dock_state,
            dispatcher,
            chart,
            logger,
            screener,
            run_screener_signal,
            screener_industries: Vec::new(),
            screener_boards: Vec::new(),
            shared_state,
            work_signal,
            stock_list,
            stock_picker,
            timeframe_index: 0,
            theme,
            dock_style,
            _backend_handle,
            toast: ToastManager::new(),
            modal: Modal::new(),
            file_dialog: egui_file_dialog::FileDialog::new(),
            last_error: None,
            last_loading: false,
            last_screener_error: None,
            last_screener_synced_symbol: startup_symbol,
        }
    }

    // --- Non-UI function tests ---

    /// Serializes tests that mutate the global `HOME` env var. Rust runs tests
    /// in parallel within a binary; concurrent `set_var("HOME", ...)` makes
    /// these tests racy (flaky `load_config` failures under `cargo test`).
    static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn load_config_missing_file_returns_default() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let config = crate::load_config();

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert_eq!(
            config.app.default_symbol,
            AppConfig::default().app.default_symbol
        );
    }

    #[test]
    fn load_config_valid_toml_returns_parsed() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[parquet]
dir = "/custom/parquet/dir"

[app]
default_symbol = "600519"
default_timeframe = "1w"
"#,
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let config = crate::load_config();

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert_eq!(config.app.default_symbol, "600519");
        assert_eq!(config.app.default_timeframe, "1w");
        assert_eq!(config.parquet.dir, "/custom/parquet/dir");
    }

    #[test]
    fn load_config_invalid_toml_falls_back_to_default() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "{{{ not valid toml").unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let config = crate::load_config();

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert_eq!(
            config.app.default_symbol,
            AppConfig::default().app.default_symbol
        );
    }

    #[test]
    fn load_stock_list_empty_dir_returns_empty_vec() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.parquet.dir = tmp.path().to_string_lossy().to_string();

        let stocks = crate::load_stock_list(&config);
        assert!(stocks.is_empty());
    }

    #[test]
    fn setup_cjk_fonts_missing_file_no_panic() {
        let ctx = egui::Context::default();
        crate::setup_cjk_fonts(&ctx);
    }

    #[test]
    fn init_tracing_no_panic() {
        let _guard = crate::init_tracing();
    }

    // --- render_toolbar kittest tests ---

    #[test]
    fn render_toolbar_renders_combo_and_button() {
        let mut app = build_compass_app(egui::Context::default());
        let harness = egui_kittest::Harness::new_ui(|ui| {
            app.render_toolbar(ui);
        });

        let _ = harness.get_by_value("1d");
        let _ = harness.get_by_value("compass_dark");
    }

    #[test]
    fn render_toolbar_timeframe_switch_changes_index() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.get_by_value("1d").click();
            harness.step();
            harness.get_by_label("1w").click();
            harness.step();
        }

        assert_eq!(app.timeframe_index, 1);
    }

    #[test]
    fn render_toolbar_theme_switch_changes_theme() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.get_by_value("compass_dark").click();
            harness.step();
            harness.get_by_label("compass_light").click();
            harness.step();
        }

        assert_eq!(app.theme.name(), "compass_light");
    }

    #[test]
    fn render_toolbar_fetch_sets_loading() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });

            let fetch_label = format!("{} Fetch", egui_phosphor::regular::DOWNLOAD_SIMPLE);
            harness.get_by_label(&fetch_label).click();
            harness.step();
        }

        assert!(
            app.shared_state.loading.get(),
            "loading must be true after fetch click"
        );
    }

    #[test]
    fn render_toolbar_error_transition_pushes_toast() {
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.error.set(Some("test error".to_string()));

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.step();
        }

        let count_first = app.toast.len();
        assert!(
            count_first > 0,
            "error toast should be pushed on None→Some transition"
        );

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.step();
        }

        assert_eq!(
            app.toast.len(),
            count_first,
            "no duplicate toasts on subsequent frames"
        );
    }

    #[test]
    fn render_toolbar_success_transition_pushes_toast() {
        let mut app = build_compass_app(egui::Context::default());
        app.last_loading = true;
        app.shared_state.loading.set(false);
        app.shared_state.error.set(None);

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.step();
        }

        assert!(
            !app.toast.is_empty(),
            "success toast on loading true→false with no error"
        );
    }

    #[test]
    fn compass_app_ui_renders_no_panic() {
        let mut harness =
            egui_kittest::Harness::new_eframe(|cc| build_compass_app(cc.egui_ctx.clone()));
        harness.step();
    }

    #[test]
    fn compass_app_ui_multiple_frames_no_panic() {
        let mut harness =
            egui_kittest::Harness::new_eframe(|cc| build_compass_app(cc.egui_ctx.clone()));

        harness.step();
        harness.step();
        harness.step();
    }
}
