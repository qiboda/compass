use std::sync::Arc;
use std::time::Duration;

use egui_citizen::{CitizenId, Dispatcher};
use egui_dock::{DockArea, DockState};
use tracing::{debug, info};

use compass_core::data::parquet::ParquetReader;
use compass_core::model::AppConfig;

mod backend;
mod citizens;
mod dispatcher;
mod messages;
mod state;
mod tabs;
mod widgets;

use citizens::chart::ChartCitizen;
use citizens::logger::LoggerPanel;
use tabs::{CHART_ID, LOGGER_ID, Tab, TabKind, TabViewer};
use widgets::searchable_dropdown::StockPicker;

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
    fonts
        .font_data
        .insert("SourceHanSansCN".into(), std::sync::Arc::new(egui::FontData::from_owned(font_bytes)));
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "SourceHanSansCN".into());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.push("SourceHanSansCN".into());
    }
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
            let (work_signal, _backend_handle) =
                backend::wire_backend(config.clone(), shared_state.clone(), egui_ctx);

            // Register citizens
            let mut dispatcher = Dispatcher::new();
            let registered = dispatcher::register_citizens(&mut dispatcher);

            // Create citizen panels
            let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
            let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);

            // Create initial dock state with 2 tabs in vertical stack
            let mut dock_state = DockState::new(vec![Tab::new(TabKind::Chart)]);
            if let Some(surface) = dock_state.get_surface_mut(egui_dock::SurfaceIndex::main())
                && let Some(tree) = surface.node_tree_mut()
            {
                let _ = tree.split_below(
                    egui_dock::NodeIndex::root(),
                    0.75,
                    vec![Tab::new(TabKind::Logger)],
                );
            }

            let stock_picker = StockPicker::new(
                &config.app.default_symbol,
                &stock_list,
            );

            Ok(Box::new(CompassApp {
                dock_state,
                dispatcher,
                chart,
                logger,
                shared_state,
                work_signal,
                stock_list,
                stock_picker,
                timeframe_index: 0usize,
                _backend_handle,
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
    shared_state: Arc<state::SharedState>,
    work_signal: egui_mobius::signals::Signal<messages::FetchRequest>,
    stock_list: Vec<compass_core::model::StockBasic>,
    stock_picker: StockPicker,
    timeframe_index: usize,
    _backend_handle: backend::BackendHandle,
}

impl eframe::App for CompassApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().item_spacing.x);
            ui.vertical(|ui| {
                self.render_toolbar(ui);
            });
        });

        ui.separator();

        DockArea::new(&mut self.dock_state).show_inside(
            ui,
            &mut TabViewer {
                dispatcher: &mut self.dispatcher,
                chart: &mut self.chart,
                logger: &mut self.logger,
                shared_state: &self.shared_state,
            },
        );

        dispatcher::drain_citizen(&mut self.dispatcher, &self.shared_state);

        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }
}

impl CompassApp {
    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Symbol:");
            self.stock_picker.show(ui, &self.stock_list);

            ui.separator();

            ui.label("TF:");
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

            if ui.button("Fetch").clicked() {
                let symbol = self.stock_picker.selected_symbol.clone();
                let timeframe = timeframe_value(self.timeframe_index);
                info!(
                    symbol = %symbol,
                    timeframe = %timeframe,
                    "fetch requested"
                );
                self.shared_state.symbol.set(symbol);
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
            if let Some(ref err) = self.shared_state.error.get() {
                ui.colored_label(egui::Color32::RED, err);
            }
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
    use crate::state::SharedState;
    use crate::tabs::{CHART_ID, LOGGER_ID};
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
        let state = SharedState::new("000001");
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
        let result = crate::widgets::searchable_dropdown::filter_stocks(
            &stocks, "600", &Exchange::All,
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].symbol, "600036");
        assert_eq!(result[1].symbol, "600519");
    }

    #[test]
    fn filter_stocks_name_substring_match() {
        let stocks = build_stock_list();
        let result = crate::widgets::searchable_dropdown::filter_stocks(
            &stocks, "银行", &Exchange::All,
        );
        assert_eq!(result.len(), 2);
        let found: Vec<_> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(found.contains(&"000001"));
        assert!(found.contains(&"600036"));
    }

    #[test]
    fn filter_stocks_exchange_filter_sh() {
        let stocks = build_stock_list();
        let result = crate::widgets::searchable_dropdown::filter_stocks(
            &stocks, "", &Exchange::SH,
        );
        assert_eq!(result.len(), 3);
        let symbols: Vec<_> = result.iter().map(|s| s.symbol.as_str()).collect();
        assert!(symbols.contains(&"600519"));
        assert!(symbols.contains(&"688001"));
    }

    #[test]
    fn filter_stocks_exchange_filter_sz() {
        let stocks = build_stock_list();
        let result = crate::widgets::searchable_dropdown::filter_stocks(
            &stocks, "", &Exchange::SZ,
        );
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn filter_stocks_empty_query_returns_all_in_exchange() {
        let stocks = build_stock_list();
        let result = crate::widgets::searchable_dropdown::filter_stocks(
            &stocks, "", &Exchange::All,
        );
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
}
