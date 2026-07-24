use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use egui_charts::ChartType;
use egui_charts::model::BarData;
use egui_charts::theme::Theme;
use egui_charts::widget::Chart;
use tracing::info;

use compass_core::data::{
    CachedProvider, duckdb::DuckDbProvider, eastmoney::EastMoneyProvider, provider::DataProvider,
};
use compass_core::model::{AppConfig, Cmd, CompassState};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> eframe::Result {
    // 1. Initialize tracing
    let _file_guard = init_tracing();

    // 2. Load config
    let config = load_config();

    // 3. Create shared state
    let state = Arc::new(Mutex::new(CompassState::new(
        &config.app.default_symbol,
        &config.app.default_timeframe,
    )));

    // 4. Create command channel
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();

    // 5. Shared egui context holder (set after eframe app is created)
    let ctx_holder: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));

    // 6. Spawn worker thread
    start_worker_thread(cmd_rx, state.clone(), ctx_holder.clone(), config);

    // 7. Run the eframe application
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Compass — Stock Chart",
        options,
        Box::new(move |cc| {
            *ctx_holder.lock().unwrap() = Some(cc.egui_ctx.clone());
            Ok(Box::new(CompassApp::new(
                cc.egui_ctx.clone(),
                state.clone(),
                cmd_tx.clone(),
            )))
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

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

/// Spawns a background worker that listens for `Cmd` messages, dispatches to
/// the `CachedProvider`, and updates `CompassState`.
fn start_worker_thread(
    cmd_rx: mpsc::Receiver<Cmd>,
    state: Arc<Mutex<CompassState>>,
    ctx_holder: Arc<Mutex<Option<egui::Context>>>,
    config: AppConfig,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

        rt.block_on(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(config.api.timeout_secs))
                .build()
                .expect("failed to create HTTP client");

            let reader = EastMoneyProvider::new(
                client,
                config.api.base_url,
                "https://push2delay.eastmoney.com".to_string(),
            );

            let cache = match DuckDbProvider::new(&config.database.path) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(path = %config.database.path, error = %e, "failed to open duckdb cache, worker cannot start");
                    return;
                }
            };

            let provider = CachedProvider::new(reader, cache);

            loop {
                let cmd = match cmd_rx.recv() {
                    Ok(cmd) => cmd,
                    Err(_) => {
                        tracing::info!("command channel closed, worker exiting");
                        break;
                    }
                };

                match cmd {
                    Cmd::FetchBars {
                        symbol,
                        timeframe,
                        range_start,
                        range_end,
                    } => {
                        tracing::info!(%symbol, %timeframe, "fetching bars");

                        {
                            let mut s = state.lock().unwrap();
                            s.loading = true;
                            s.error = None;
                        }

                        match provider
                            .fetch_bars(&symbol, &timeframe, range_start, range_end)
                            .await
                        {
                            Ok(bars) => {
                                let count = bars.len();
                                let mut s = state.lock().unwrap();
                                s.set_bars(&symbol, &timeframe, bars);
                                s.current_symbol = symbol;
                                s.current_timeframe = timeframe;
                                s.loading = false;
                                tracing::info!(count, "bars loaded");
                            }
                            Err(e) => {
                                let mut s = state.lock().unwrap();
                                s.loading = false;
                                s.error = Some(format!("{e}"));
                                tracing::error!(error = %e, "fetch failed");
                            }
                        }
                    }
                    Cmd::SearchSymbols { query } => {
                        tracing::info!(%query, "searching symbols");

                        match provider.search_symbols(&query).await {
                            Ok(results) => {
                                let mut s = state.lock().unwrap();
                                s.search_results = results;
                                tracing::info!("search complete");
                            }
                            Err(e) => {
                                let mut s = state.lock().unwrap();
                                s.error = Some(format!("{e}"));
                                tracing::error!(error = %e, "search failed");
                            }
                        }
                    }
                }

                if let Some(ctx) = ctx_holder.lock().unwrap().as_ref() {
                    ctx.request_repaint();
                }
            }
        });
    })
}

// ---------------------------------------------------------------------------
// CompassApp — interactive chart application
// ---------------------------------------------------------------------------

struct CompassApp {
    chart: Chart,
    state: Arc<Mutex<CompassState>>,
    cmd_tx: mpsc::Sender<Cmd>,
    bars_version: u64,
    symbol_input: String,
    timeframe_input: String,
}

impl CompassApp {
    fn new(
        _egui_ctx: egui::Context,
        state: Arc<Mutex<CompassState>>,
        cmd_tx: mpsc::Sender<Cmd>,
    ) -> Self {
        let (symbol_input, timeframe_input) = {
            let s = state.lock().unwrap();
            (s.current_symbol.clone(), s.current_timeframe.clone())
        };

        let bars: Vec<egui_charts::model::Bar> = Vec::new();
        let data = BarData::from_bars(bars);
        let mut chart = Chart::new(data);
        chart.set_chart_type(ChartType::Candles);
        chart.set_visible_bars(100);
        chart.set_symbol("COMPASS");
        chart.set_timeframe_label("1d");

        Self {
            chart,
            state,
            cmd_tx,
            bars_version: 0,
            symbol_input,
            timeframe_input,
        }
    }
}

impl eframe::App for CompassApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = custom_dark_theme();
        egui_charts::theme::apply_to_egui(ctx, &theme);

        // Check whether the worker updated bars → rebuild chart data
        {
            let state = self.state.lock().unwrap();
            if state.bars_version != self.bars_version {
                let key = (
                    state.current_symbol.clone(),
                    state.current_timeframe.clone(),
                );
                if let Some(bars) = state.bars.get(&key) {
                    self.chart.update_data(BarData::from_bars(bars.clone()));
                }
                self.bars_version = state.bars_version;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // --- Top controls row ---
            let (loading, error) = {
                let s = self.state.lock().unwrap();
                (s.loading, s.error.clone())
            };

            ui.horizontal(|ui| {
                ui.label("Symbol:");
                ui.text_edit_singleline(&mut self.symbol_input);

                ui.label("Timeframe:");
                egui::ComboBox::from_id_salt("timeframe_combo")
                    .selected_text(&self.timeframe_input)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.timeframe_input, "1d".into(), "1d");
                    });

                if ui.button("Fetch").clicked() {
                    let cmd = Cmd::FetchBars {
                        symbol: self.symbol_input.clone(),
                        timeframe: self.timeframe_input.clone(),
                        range_start: Utc::now() - chrono::Duration::days(365),
                        range_end: Utc::now(),
                    };
                    let _ = self.cmd_tx.send(cmd);
                }
            });

            // --- Status indicators ---
            if loading {
                ui.label("Loading...");
            }

            if let Some(ref err) = error {
                ui.colored_label(egui::Color32::RED, err);
            }

            // --- Chart area ---
            self.chart.show(ui);
        });

        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

// ---------------------------------------------------------------------------
// Dark theme
// ---------------------------------------------------------------------------

fn custom_dark_theme() -> Theme {
    Theme::dark()
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_core::model::CompassState;

    fn make_state() -> Arc<Mutex<CompassState>> {
        Arc::new(Mutex::new(CompassState::new("000001", "1d")))
    }

    #[test]
    fn compass_app_new_reads_initial_state() {
        let state = make_state();
        let (tx, _rx) = mpsc::channel::<Cmd>();
        let ctx = egui::Context::default();
        let app = CompassApp::new(ctx, state.clone(), tx);
        assert_eq!(app.symbol_input, "000001");
        assert_eq!(app.timeframe_input, "1d");
        assert_eq!(app.bars_version, 0);
    }

    #[test]
    #[cfg(feature = "tracy")]
    fn tracy_layer_constructs() {
        let _layer = tracing_tracy::TracyLayer::default();
    }
}
