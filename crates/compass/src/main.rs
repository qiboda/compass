use std::sync::Arc;
use std::time::Duration;

use egui_citizen::{CitizenId, Dispatcher};
use egui_dock::{DockArea, DockState};
use egui_file_dialog::FileDialog;
use serde::Deserialize;
use tracing::{debug, info};

use compass_core::data::parquet::ParquetReader;
use compass_core::data::symbol::parse_explicit_prefix;
use compass_core::model::{AppConfig, WatchlistConfig};
use compass_types::ScreenerQuery;
use compass_ui::widgets::button::{Button, ButtonSize, ButtonVariant};
use compass_ui::widgets::dropdown::Dropdown;
use compass_ui::widgets::icon_button::IconButton;
use compass_ui::widgets::modal::Modal;
use compass_ui::widgets::searchable_dropdown::{StockPicker, StockProjection};
use compass_ui::widgets::segmented::Segmented;
use compass_ui::widgets::sidebar::{Sidebar, SidebarEvent, SidebarGroup, SidebarItem};
use compass_ui::widgets::status_bar::{StatusBar, StatusBarData, StatusKind, StockSummary};
use compass_ui::widgets::tag::{Tag, TagVariant};
use compass_ui::widgets::toast::{ToastLevel, ToastManager};
use compass_ui::widgets::toolbar::Toolbar;

mod backend;
mod citizens;
mod dispatcher;
mod messages;
mod state;
mod tabs;
mod theme;

use citizens::chart::ChartCitizen;
use citizens::logger::LoggerPanel;
use citizens::screener::ScreenerPanel;
use citizens::sepa::SepaPanel;
use tabs::{CHART_ID, LOGGER_ID, SCREENER_ID, SEPA_ID, Tab, TabKind, TabViewer};
use theme::CompassTheme;

/// Default inner window size (design doc §Q8: 1440×900).
const WINDOW_INNER_SIZE: egui::Vec2 = egui::vec2(1440.0, 900.0);

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> eframe::Result {
    let _file_guard = init_tracing();
    let config = load_config();

    // Create reactive shared state
    let shared_state = Arc::new(state::SharedState::new(
        &config.app.app.default_symbol,
        &config.app.app.default_timeframe,
    ));
    shared_state.watchlist.set(config.watchlist.symbols.clone());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(WINDOW_INNER_SIZE),
        ..Default::default()
    };

    eframe::run_native(
        "Compass — Stock Chart",
        options,
        Box::new(move |cc| {
            let egui_ctx = cc.egui_ctx.clone();

            compass_ui::fonts::setup_fonts(&egui_ctx);

            // Load stock list from parquet at startup
            let stock_list = load_stock_list(&config.app);

            // Wire Level 3 backend (signal/slot + AsyncDispatcher)
            let (work_signal, run_screener_signal, sepa_signal, _backend_handle) =
                backend::wire_backend(config.app.clone(), shared_state.clone(), egui_ctx);

            // Register citizens
            let mut dispatcher = Dispatcher::new();
            let registered = dispatcher::register_citizens(&mut dispatcher);

            // The theme drives the citizen panel styling (screener components
            // copy the tokens at construction, like StockPicker/Modal).
            let theme = CompassTheme::from_config(&config.app.theme);

            // Create citizen panels
            let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
            let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
            let screener = ScreenerPanel::new(
                CitizenId::new(SCREENER_ID),
                registered.screener,
                Some(&config.screener),
                Box::new(|q| {
                    if let Err(e) = save_screener_config(q) {
                        tracing::warn!(error = %e, "failed to save screener config");
                    }
                }),
                theme.tokens(),
            );
            let sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, theme.tokens());

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

            // Create initial dock state: Chart + 东方SEPA share the top leaf
            // (SEPA's 12-column table + detail panel need the full width),
            // Logger + Screener below.
            let mut dock_state =
                DockState::new(vec![Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa)]);
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

            let theme_tokens = *theme.tokens();
            let stock_picker = StockPicker::new(
                theme_tokens,
                &config.app.app.default_symbol,
                stock_projection(),
            );
            let dock_style = compass_ui::dock_style::dock_style(theme.tokens());

            let startup_symbol = shared_state.symbol.get();

            Ok(Box::new(CompassApp {
                dock_state,
                dispatcher,
                chart,
                logger,
                screener,
                sepa,
                run_screener_signal,
                sepa_signal,
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
                toast: ToastManager::new(theme_tokens),
                modal: Modal::new(theme_tokens),
                file_dialog: FileDialog::new(),
                last_error: None,
                last_loading: false,
                last_screener_error: None,
                last_sepa_error: None,
                last_sepa_loading: false,
                last_screener_synced_symbol: startup_symbol,
                sidebar_visible: true,
                sidebar_search: String::new(),
                status_clock: String::new(),
                symbol_input_id: None,
                pending_delete: None,
                delete_confirmed: std::rc::Rc::new(std::cell::RefCell::new(false)),
                startup_modal_shown: false,
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

/// Top-level config file contents: the legacy `AppConfig` sections plus the
/// screener condition section and the watchlist section. Flattening keeps
/// existing TOML keys mapping to `AppConfig` while `[screener]` parses into
/// `ScreenerQuery` and `[watchlist]` into `WatchlistConfig`.
#[derive(Deserialize)]
struct FullConfig {
    #[serde(flatten)]
    app: AppConfig,
    #[serde(default)]
    screener: ScreenerQuery,
    #[serde(default)]
    watchlist: WatchlistConfig,
}

/// Reads `~/.config/compass/config.toml`. Falls back to `AppConfig::default()`
/// if the file is missing or malformed.
fn load_config() -> FullConfig {
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
                FullConfig {
                    app: AppConfig::default(),
                    screener: ScreenerQuery::default(),
                    watchlist: WatchlistConfig::default(),
                }
            }
        },
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "config file not found, using defaults");
            FullConfig {
                app: AppConfig::default(),
                screener: ScreenerQuery::default(),
                watchlist: WatchlistConfig::default(),
            }
        }
    }
}

/// Persist the screener conditions to the `[screener]` section of
/// `~/.config/compass/config.toml`.
///
/// Reads the existing file as a `toml::Value`, replaces the `screener`
/// table, and writes it back. Creates the file (with only `[screener]`)
/// when it does not exist. Comments and unknown sections are lost on
/// rewrite — accepted trade-off.
fn save_screener_config(query: &ScreenerQuery) -> Result<(), String> {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    let mut doc = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents
            .parse::<toml::Value>()
            .map_err(|e| format!("failed to parse config.toml: {e}"))?,
        Err(_) => toml::Value::Table(Default::default()),
    };

    let screener_value = toml::Value::try_from(query)
        .map_err(|e| format!("failed to serialize screener config: {e}"))?;
    doc.as_table_mut()
        .expect("value is a table")
        .insert("screener".to_string(), screener_value);

    let serialized =
        toml::to_string(&doc).map_err(|e| format!("failed to serialize config.toml: {e}"))?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    }
    std::fs::write(&config_path, serialized)
        .map_err(|e| format!("failed to write config.toml: {e}"))
}

/// Persist the watchlist symbols to the `[watchlist]` section of
/// `~/.config/compass/config.toml`.
///
/// Mirrors [`save_screener_config`]: reads the existing file as a
/// `toml::Value`, replaces the `watchlist` table, and writes it back.
/// Creates the file (with only `[watchlist]`) when it does not exist.
fn save_watchlist_config(symbols: &[String]) -> Result<(), String> {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    let mut doc = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents
            .parse::<toml::Value>()
            .map_err(|e| format!("failed to parse config.toml: {e}"))?,
        Err(_) => toml::Value::Table(Default::default()),
    };

    let watchlist_value = toml::Value::try_from(WatchlistConfig {
        symbols: symbols.to_vec(),
    })
    .map_err(|e| format!("failed to serialize watchlist config: {e}"))?;
    doc.as_table_mut()
        .expect("value is a table")
        .insert("watchlist".to_string(), watchlist_value);

    let serialized =
        toml::to_string(&doc).map_err(|e| format!("failed to serialize config.toml: {e}"))?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    }
    std::fs::write(&config_path, serialized)
        .map_err(|e| format!("failed to write config.toml: {e}"))
}

/// Exchange code from a symbol's explicit prefix, falling back to the
/// legacy bare-code shape heuristic for pre-migration data.
fn exchange_of_symbol(symbol: &str) -> &str {
    let (exchange, _) = parse_explicit_prefix(symbol);
    if !exchange.is_empty() {
        return exchange;
    }
    if symbol.starts_with('6') {
        "SH"
    } else if symbol.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

/// Projection mapping `StockBasic` rows into the searchable dropdown's fields.
///
/// The UI crate stays free of business-crate dependencies; the binary adapts
/// its own row type through projection functions.
fn stock_projection() -> StockProjection<compass_core::model::StockBasic> {
    StockProjection::new(
        |s: &compass_core::model::StockBasic| &s.symbol,
        |s: &compass_core::model::StockBasic| &s.name,
        |s: &compass_core::model::StockBasic| Some(exchange_of_symbol(&s.symbol)),
    )
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

/// Write the shared log entries to `path` in a plain-text export format
/// (header + `[timestamp] [level] message` lines, oldest first).
fn export_logs(state: &state::SharedState, path: &std::path::Path) -> Result<(), String> {
    let logger_state = state.log.get();
    let mut out = String::new();
    out.push_str("--- Logger Export ---\n");
    out.push_str(&format!(
        "Exported: {}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    for entry in &logger_state.logs {
        if !entry.timestamp.value.value.is_empty() {
            out.push_str(&format!("[{}] ", entry.timestamp.value.value));
        }
        let level = if !entry.log_level.info.value.is_empty() {
            entry.log_level.info.value.as_str()
        } else if !entry.log_level.debug.value.is_empty() {
            entry.log_level.debug.value.as_str()
        } else if !entry.log_level.warning.value.is_empty() {
            entry.log_level.warning.value.as_str()
        } else if !entry.log_level.error.value.is_empty() {
            entry.log_level.error.value.as_str()
        } else {
            ""
        };
        if !level.is_empty() {
            out.push_str(&format!("[{level}] "));
        }
        out.push_str(&entry.log_message.content.value);
        out.push('\n');
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
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
    sepa: SepaPanel,
    run_screener_signal: egui_mobius::signals::Signal<messages::RunScreenerRequest>,
    sepa_signal: egui_mobius::signals::Signal<messages::RunSepaRequest>,
    screener_industries: Vec<String>,
    screener_boards: Vec<String>,
    shared_state: Arc<state::SharedState>,
    work_signal: egui_mobius::signals::Signal<messages::FetchRequest>,
    stock_list: Vec<compass_core::model::StockBasic>,
    stock_picker: StockPicker<compass_core::model::StockBasic>,
    timeframe_index: usize,
    theme: CompassTheme,
    dock_style: egui_dock::Style,
    _backend_handle: backend::BackendHandle,
    toast: ToastManager,
    modal: Modal,
    file_dialog: FileDialog,
    last_error: Option<String>,
    last_loading: bool,
    last_screener_error: Option<String>,
    last_sepa_error: Option<String>,
    last_sepa_loading: bool,
    last_screener_synced_symbol: String,
    /// Whether the left watchlist sidebar is visible.
    sidebar_visible: bool,
    /// Sidebar search filter text.
    sidebar_search: String,
    /// Clock string refreshed every frame (`%H:%M:%S`, local time).
    status_clock: String,
    /// Widget id of the toolbar symbol input (for the `/` shortcut).
    symbol_input_id: Option<egui::Id>,
    /// Symbol whose watchlist removal is awaiting modal confirmation.
    pending_delete: Option<String>,
    /// Shared flag flipped by the delete-confirm modal callback; read back
    /// after [`Modal::show`] each frame to complete the removal.
    delete_confirmed: std::rc::Rc<std::cell::RefCell<bool>>,
    /// Whether the startup data-missing modal has been offered this session.
    startup_modal_shown: bool,
}

impl eframe::App for CompassApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.theme.apply_theme(ui.ctx());

        self.handle_shortcuts(ui);

        // Startup guide: when the stock list is empty, offer the data-missing
        // modal once per session (design §6.5 scenario 1).
        if !self.startup_modal_shown {
            self.startup_modal_shown = true;
            if self.stock_list.is_empty() {
                self.modal.set_title("数据未就绪");
                self.modal.set_body(
                    "未在本地数据目录中找到股票列表（stock_basic.parquet）。\n请先用数据管线导入数据：cargo run --bin compass-data -- import-compass --table stock_basic",
                );
                self.modal.set_danger(false);
                self.modal.set_confirm_text("知道了");
                self.modal.set_cancel_text("取消");
                self.modal.set_on_confirm(|| {});
                self.modal.open(ui.ctx().input(|i| i.time));
            }
        }

        // Top: group toolbar (40 px, design §6.1).
        egui::Panel::top("toolbar").show(ui, |ui| {
            self.render_toolbar(ui);
        });

        // Left: watchlist sidebar (240 px, resizable 200–320, design §6.2).
        if self.sidebar_visible {
            egui::Panel::left("sidebar")
                .default_size(240.0)
                .size_range(200.0..=320.0)
                .resizable(true)
                .show(ui, |ui| {
                    self.render_sidebar(ui);
                });
        }

        // Bottom: status bar (26 px, design §6.3).
        egui::Panel::bottom("statusbar").show(ui, |ui| {
            self.render_status_bar(ui);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let mut logger_export_clicked = false;
            DockArea::new(&mut self.dock_state)
                .style(self.dock_style.clone())
                .show_inside(
                    ui,
                    &mut TabViewer {
                        dispatcher: &mut self.dispatcher,
                        chart: &mut self.chart,
                        logger: &mut self.logger,
                        screener: &mut self.screener,
                        sepa: &mut self.sepa,
                        run_screener_signal: &self.run_screener_signal,
                        sepa_signal: &self.sepa_signal,
                        work_signal: &self.work_signal,
                        screener_industries: &self.screener_industries,
                        screener_boards: &self.screener_boards,
                        shared_state: &self.shared_state,
                        theme: &self.theme,
                        logger_export_clicked: &mut logger_export_clicked,
                    },
                );

            self.toast.render(ui.ctx());
            self.modal.show(ui.ctx());
            self.file_dialog.update(ui.ctx());

            // Logger export (design §6.5 scenario 2): the export button opens
            // the save-file dialog; a picked path writes the log entries.
            if logger_export_clicked {
                self.file_dialog.save_file();
            }
            if let Some(path) = self.file_dialog.take_picked() {
                self.handle_log_export_pick(path);
            }

            // Complete a pending watchlist removal once the confirm modal
            // fires (the callback only flips the shared flag above).
            if *self.delete_confirmed.borrow()
                && let Some(symbol) = self.pending_delete.take()
            {
                self.remove_from_watchlist(&symbol);
                *self.delete_confirmed.borrow_mut() = false;
            }
            // A stale pending delete (modal dismissed via Cancel/Esc) is
            // cleared once the closing fade completes.
            if !self.modal.is_open()
                && self.pending_delete.is_some()
                && !*self.delete_confirmed.borrow()
            {
                self.pending_delete = None;
            }

            // Push screener error toast only on None→Some transition.
            let current_screener_err = self.shared_state.screener_error.get();
            if current_screener_err != self.last_screener_error {
                if let Some(ref err) = current_screener_err {
                    self.toast.push(ToastLevel::Error, err.clone());
                }
                self.last_screener_error = current_screener_err;
            }

            // SEPA error toast on None→Some transition (same pattern as the
            // screener error above).
            let current_sepa_err = self.shared_state.sepa_error.get();
            if current_sepa_err != self.last_sepa_error {
                if let Some(ref err) = current_sepa_err {
                    self.toast.push(ToastLevel::Error, err.clone());
                }
                self.last_sepa_error = current_sepa_err;
            }

            // SEPA success toast on loading true→false with no error; the
            // stale selection index points at pre-refresh data, so it is
            // dropped along with the toast (design §7).
            let current_sepa_loading = self.shared_state.sepa_loading.get();
            if self.last_sepa_loading && !current_sepa_loading {
                self.sepa.reset_selection();
                if self.shared_state.sepa_error.get().is_none() {
                    let count = self
                        .shared_state
                        .sepa_data
                        .get()
                        .map(|d| d.rows.len())
                        .unwrap_or(0);
                    self.toast
                        .push(ToastLevel::Success, format!("SEPA 评分已更新 · {count} 只"));
                }
            }
            self.last_sepa_loading = current_sepa_loading;

            // Reverse-sync: when the symbol changed (e.g. a screener row
            // click), reflect it in the StockPicker — but only for bare
            // 6-digit codes; prefixed toolbar symbols must not clobber the
            // picker's exchange state.
            self.sync_picker_from_symbol();

            dispatcher::drain_citizen(&mut self.dispatcher, &self.shared_state);

            ui.ctx().request_repaint_after(Duration::from_millis(200));
        });
    }
}

impl CompassApp {
    /// Global keyboard shortcuts (design §6.4 / §7).
    ///
    /// `/` focuses the toolbar symbol input, `Ctrl+Enter` fetches the current
    /// selection, `Ctrl+K` focuses the sidebar search and `1/2/3` switch the
    /// timeframe.
    fn handle_shortcuts(&mut self, ui: &egui::Ui) {
        // Guard: plain keys (digits, `/`) must not fire while a text widget has
        // focus — typing a symbol like "601318" would otherwise flip the
        // timeframe under the user's fingers. Ctrl-combos stay active.
        let editing_text = ui.ctx().memory(|m| m.focused().is_some());
        let (slash, ctrl_enter, ctrl_k, num1, num2, num3) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::Slash) && !editing_text,
                i.key_pressed(egui::Key::Enter) && i.modifiers.command,
                i.key_pressed(egui::Key::K) && i.modifiers.command,
                i.key_pressed(egui::Key::Num1) && !editing_text,
                i.key_pressed(egui::Key::Num2) && !editing_text,
                i.key_pressed(egui::Key::Num3) && !editing_text,
            )
        });
        if slash && let Some(id) = self.symbol_input_id {
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        if ctrl_enter {
            self.fetch_bars();
        }
        if ctrl_k {
            ui.ctx()
                .memory_mut(|m| m.request_focus(Self::sidebar_search_input_id()));
        }
        if num1 {
            self.set_timeframe(0);
        }
        if num2 {
            self.set_timeframe(1);
        }
        if num3 {
            self.set_timeframe(2);
        }
    }

    fn set_timeframe(&mut self, idx: usize) {
        if idx != self.timeframe_index {
            debug!(timeframe = timeframe_label(idx), "timeframe changed");
            self.timeframe_index = idx;
        }
    }

    /// Widget id of the search input rendered inside [`Sidebar::show`].
    ///
    /// The sidebar body runs under an explicit `sidebar_body` Ui id so the
    /// chain is fully derivable: each child Ui layer (`Sidebar`'s horizontal,
    /// the `Input` frame and its inner horizontal) uses the default `"child"`
    /// salt on the parent's stable id, and the `TextEdit` adds its
    /// `"compass_input"` salt. The salts must be applied as [`egui::IdSalt`]
    /// (hash of the salt value), matching egui's `Ui::new_child` derivation —
    /// `Id::with(&str)` hashes the string instead and yields a different id.
    fn sidebar_search_input_id() -> egui::Id {
        let body = egui::Id::new("sidebar_body");
        let child = egui::IdSalt::new("child");
        let input = egui::IdSalt::new("compass_input");
        body.with(child).with(child).with(child).with(input)
    }

    /// Fetch bars for a symbol through the dispatcher.
    fn fetch_symbol(&mut self, symbol: &str) {
        self.shared_state.symbol.set(symbol.to_string());
        let timeframe = timeframe_value(self.timeframe_index);
        self.shared_state.timeframe.set(timeframe.clone());
        dispatcher::handle(
            messages::AppMessage::FetchBars,
            &self.shared_state,
            &self.work_signal,
            timeframe,
        );
    }

    /// Fetch the current toolbar selection.
    ///
    /// Parquet/DuckDB store bare 6-digit codes; the picker's exchange field
    /// is display-only, so the prefixed form ("sz.000001") must NOT be sent
    /// or the lookup finds no data.
    fn fetch_bars(&mut self) {
        let symbol = self.stock_picker.selected_symbol.clone();
        info!(symbol = %symbol, timeframe = %timeframe_value(self.timeframe_index), "fetch requested");
        self.fetch_symbol(&symbol);
    }

    /// Reflect `shared_state.symbol` changes back into the StockPicker.
    ///
    /// Only bare 6-digit codes are synced — prefixed symbols ("sz.000001")
    /// come from external sources (watchlist config) and copying those back
    /// would corrupt the picker. The marker field tracks the last seen
    /// symbol so per-frame checks fire only on actual changes.
    fn sync_picker_from_symbol(&mut self) {
        let symbol = self.shared_state.symbol.get();
        if symbol == self.last_screener_synced_symbol {
            return;
        }
        self.last_screener_synced_symbol = symbol.clone();
        let is_bare_code = symbol.len() == 6 && symbol.chars().all(|c| c.is_ascii_digit());
        if !is_bare_code {
            return;
        }
        let name = self
            .stock_list
            .iter()
            .find(|s| s.symbol == symbol)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        self.stock_picker.selected_symbol = symbol;
        self.stock_picker.selected_name = name;
        self.stock_picker.selected_exchange.clear();
    }

    /// Left watchlist sidebar: search row + the "自选" group backed by
    /// `SharedState.watchlist` (design §6.2). Add inserts the current symbol;
    /// delete requests open a danger confirm modal before removal.
    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        let tokens = *self.theme.tokens();
        let sidebar = Sidebar::new(&tokens);
        let current_symbol = self.shared_state.symbol.get();
        let watchlist = self.shared_state.watchlist.get();
        let query = self.sidebar_search.trim().to_lowercase();

        let mut items = Vec::new();
        for symbol in &watchlist {
            let stock = self.stock_list.iter().find(|s| &s.symbol == symbol);
            let name = stock
                .map(|s| s.name.clone())
                .unwrap_or_else(|| symbol.clone());
            let exchange = stock
                .map(|s| exchange_of_symbol(&s.symbol).to_string())
                .unwrap_or_default();
            let matches = query.is_empty()
                || symbol.to_lowercase().contains(&query)
                || name.to_lowercase().contains(&query);
            if matches {
                items.push(SidebarItem {
                    symbol: symbol.clone(),
                    name,
                    exchange,
                    selected: symbol == &current_symbol,
                });
            }
        }
        let groups = [SidebarGroup {
            title: "自选".to_string(),
            items,
        }];

        let events = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("sidebar_body")),
                |ui| sidebar.show(ui, &groups, &mut self.sidebar_search),
            )
            .inner;

        for event in events {
            match event {
                SidebarEvent::Select { symbol } => self.fetch_symbol(&symbol),
                SidebarEvent::Search(_) => {}
                SidebarEvent::Add => self.add_to_watchlist(&current_symbol),
                SidebarEvent::DeleteRequest { symbol } => {
                    self.request_watchlist_removal(ui.ctx().input(|i| i.time), &symbol)
                }
            }
        }
    }

    /// Add `symbol` to the watchlist (dedup + sort) and persist it.
    fn add_to_watchlist(&mut self, symbol: &str) {
        let mut watchlist = self.shared_state.watchlist.get();
        if watchlist.iter().any(|s| s == symbol) {
            return;
        }
        watchlist.push(symbol.to_string());
        watchlist.sort();
        self.shared_state.watchlist.set(watchlist.clone());
        if let Err(e) = save_watchlist_config(&watchlist) {
            tracing::warn!(error = %e, "failed to save watchlist config");
        }
        self.toast
            .push(ToastLevel::Success, format!("已添加 {symbol} 到自选"));
    }

    /// Remove `symbol` from the watchlist and persist it.
    fn remove_from_watchlist(&mut self, symbol: &str) {
        let mut watchlist = self.shared_state.watchlist.get();
        let before = watchlist.len();
        watchlist.retain(|s| s != symbol);
        if watchlist.len() == before {
            return;
        }
        self.shared_state.watchlist.set(watchlist.clone());
        if let Err(e) = save_watchlist_config(&watchlist) {
            tracing::warn!(error = %e, "failed to save watchlist config");
        }
        self.toast
            .push(ToastLevel::Success, format!("已从自选移除 {symbol}"));
    }

    /// Open the danger confirm modal for removing `symbol` from the watchlist
    /// (design §6.5 scenario 3). The confirm callback only flips a shared
    /// flag; the actual removal runs after [`Modal::show`] in `ui()`.
    ///
    /// `now` is the current egui virtual time in seconds
    /// (`ctx.input(|i| i.time)`), stamped as the modal's entry-animation start.
    fn request_watchlist_removal(&mut self, now: f64, symbol: &str) {
        if self.pending_delete.as_deref() == Some(symbol) && self.modal.is_open() {
            return;
        }
        self.pending_delete = Some(symbol.to_string());
        *self.delete_confirmed.borrow_mut() = false;
        self.modal.set_title("移除自选");
        self.modal
            .set_body(format!("确定要从自选中移除 {symbol} 吗？"));
        self.modal.set_danger(true);
        self.modal.set_confirm_text("移除");
        self.modal.set_cancel_text("保留");
        let confirmed = self.delete_confirmed.clone();
        self.modal.set_on_confirm(move || {
            *confirmed.borrow_mut() = true;
        });
        self.modal.open(now);
    }

    /// Handle a log-export save path: write the shared log entries and toast
    /// the outcome (design §6.5 scenario 2).
    fn handle_log_export_pick(&mut self, path: std::path::PathBuf) {
        match export_logs(&self.shared_state, &path) {
            Ok(()) => {
                self.toast.push(
                    ToastLevel::Success,
                    format!("日志已导出: {}", path.display()),
                );
            }
            Err(e) => {
                self.toast
                    .push(ToastLevel::Error, format!("日志导出失败: {e}"));
            }
        }
    }

    /// Bottom status bar: stock summary, loading/error state, data source
    /// count and the wall-clock time (design §6.3).
    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let tokens = *self.theme.tokens();
        self.status_clock = chrono::Local::now().format("%H:%M:%S").to_string();

        let symbol = self.shared_state.symbol.get();
        let name = self
            .stock_list
            .iter()
            .find(|s| s.symbol == symbol)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let loading = self.shared_state.loading.get();
        let error = self.shared_state.error.get();
        let (status, status_text) = if loading {
            (StatusKind::Loading, "加载中…".to_string())
        } else if let Some(err) = error {
            (StatusKind::Error, err)
        } else {
            (StatusKind::Idle, String::new())
        };

        // Latest close as the price; the change vs. the previous close when
        // both are available (None otherwise — design §6.3 keeps the slot
        // optional).
        let (price, change) = latest_quote(&self.shared_state.bars.get());

        StatusBar::new(&tokens).show(
            ui,
            &StatusBarData {
                summary: Some(StockSummary {
                    symbol,
                    name,
                    price,
                    change,
                }),
                status,
                status_text,
                source: format!("本地数据源 · {} 只", self.stock_list.len()),
                clock: self.status_clock.clone(),
            },
        );
    }

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        let tokens = *self.theme.tokens();
        let loading = self.shared_state.loading.get();

        Toolbar::new(&tokens).show(ui, |tb, ui| {
            // Group A — 标的: symbol picker.
            tb.group(ui, |ui| {
                let response = self.stock_picker.show(ui, &self.stock_list);
                self.symbol_input_id = Some(response.id);
            });

            // Group B — 周期: segmented 1d/1w/1M + 前复权 tag.
            tb.group(ui, |ui| {
                if let Some(idx) = Segmented::new(&tokens, ["1d", "1w", "1M"])
                    .selected(self.timeframe_index)
                    .show(ui)
                {
                    self.set_timeframe(idx);
                }
                Tag::new(&tokens, "前复权")
                    .variant(TagVariant::Custom)
                    .color(tokens.color.info)
                    .show(ui);
            });

            // Group C — 操作: primary Fetch button with loading state.
            tb.group(ui, |ui| {
                let fetch_clicked = Button::new(&tokens, if loading { "加载中…" } else { "Fetch" })
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Lg)
                    .icon(egui_phosphor::regular::DOWNLOAD_SIMPLE)
                    .loading(loading)
                    .show(ui)
                    .clicked();
                if fetch_clicked && !loading {
                    self.fetch_bars();
                }
            });

            // Group D — 显示: sidebar toggle + theme dropdown.
            tb.group(ui, |ui| {
                if IconButton::new(&tokens, egui_phosphor::regular::SIDEBAR_SIMPLE)
                    .tooltip("切换侧边栏")
                    .show(ui)
                {
                    self.sidebar_visible = !self.sidebar_visible;
                }
                let theme_idx = CompassTheme::all_names()
                    .iter()
                    .position(|n| *n == self.theme.name())
                    .unwrap_or(0);
                if let Some(idx) = Dropdown::new(&tokens, CompassTheme::all_names().to_vec())
                    .selected(theme_idx)
                    .width(140.0)
                    .show(ui)
                {
                    let name = CompassTheme::all_names()[idx];
                    if name != self.theme.name() {
                        self.theme = CompassTheme::from_config(name);
                        let tokens = *self.theme.tokens();
                        self.dock_style = compass_ui::dock_style::dock_style(&tokens);
                        // Stored stateful widgets copy tokens at construction;
                        // refresh them so the theme switch applies everywhere.
                        self.stock_picker.set_tokens(tokens);
                        self.toast.set_tokens(tokens);
                        self.modal.set_tokens(tokens);
                        self.screener.set_tokens(tokens);
                        self.sepa.set_tokens(tokens);
                        self.toast.push(ToastLevel::Info, "主题已切换");
                    }
                }
            });
        });

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

/// Latest close as the status-bar price plus the change vs. the previous
/// close (percent). `(None, None)` when there is no data; a single bar
/// yields a price with no change.
fn latest_quote(bars: &[egui_charts::model::Bar]) -> (Option<f32>, Option<f32>) {
    match bars {
        [last] => (Some(last.close as f32), None),
        [.., prev, last] => {
            let change = (prev.close != 0.0)
                .then_some((((last.close - prev.close) / prev.close) * 100.0) as f32);
            (Some(last.close as f32), change)
        }
        _ => (None, None),
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
    use crate::FullConfig;
    use compass_core::model::StockBasic;
    use compass_types::ScreenerQuery;

    use crate::citizens::chart::ChartCitizen;
    use crate::citizens::logger::LoggerPanel;
    use crate::citizens::screener::ScreenerPanel;
    use crate::citizens::sepa::SepaPanel;
    use crate::latest_quote;
    use crate::state::SharedState;
    use crate::stock_projection;
    use crate::tabs::{CHART_ID, LOGGER_ID, SCREENER_ID, SEPA_ID};
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
    use crate::WINDOW_INNER_SIZE;
    use crate::tabs::{Tab, TabKind};
    use crate::theme::CompassTheme;
    use compass_core::model::AppConfig;
    use compass_ui::tokens::ColorTokens;
    use compass_ui::widgets::modal::Modal;
    use compass_ui::widgets::searchable_dropdown::StockPicker;
    use compass_ui::widgets::toast::{ToastLevel, ToastManager};
    use egui_dock::DockState;
    use egui_kittest::kittest::Queryable;
    use std::sync::Arc;

    fn build_compass_app_with_stocks(
        egui_ctx: egui::Context,
        stocks: Vec<StockBasic>,
    ) -> CompassApp {
        let config = AppConfig::default();
        let shared_state = Arc::new(SharedState::new("000001", "1d"));

        let (work_signal, run_screener_signal, sepa_signal, _backend_handle) =
            crate::backend::wire_backend(config, shared_state.clone(), egui_ctx);

        let mut dispatcher = Dispatcher::new();
        let registered = crate::dispatcher::register_citizens(&mut dispatcher);

        let theme = CompassTheme::compass_dark();
        let theme_tokens = *theme.tokens();
        let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let screener = ScreenerPanel::new(
            CitizenId::new(SCREENER_ID),
            registered.screener,
            None,
            Box::new(|_| {}),
            &theme_tokens,
        );
        let sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, &theme_tokens);
        let stock_picker = StockPicker::new(theme_tokens, "000001", stock_projection());
        let dock_style = egui_dock::Style::default();

        let mut dock_state =
            DockState::new(vec![Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa)]);
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
            sepa,
            run_screener_signal,
            sepa_signal,
            screener_industries: Vec::new(),
            screener_boards: Vec::new(),
            shared_state,
            work_signal,
            stock_list: stocks,
            stock_picker,
            timeframe_index: 0,
            theme,
            dock_style,
            _backend_handle,
            toast: ToastManager::new(theme_tokens),
            modal: Modal::new(theme_tokens),
            file_dialog: egui_file_dialog::FileDialog::new(),
            last_error: None,
            last_loading: false,
            last_screener_error: None,
            last_sepa_error: None,
            last_sepa_loading: false,
            last_screener_synced_symbol: startup_symbol,
            sidebar_visible: true,
            sidebar_search: String::new(),
            status_clock: String::new(),
            symbol_input_id: None,
            pending_delete: None,
            delete_confirmed: std::rc::Rc::new(std::cell::RefCell::new(false)),
            startup_modal_shown: false,
        }
    }

    fn build_compass_app(egui_ctx: egui::Context) -> CompassApp {
        build_compass_app_with_stocks(egui_ctx, Vec::new())
    }

    fn sized_harness(app: CompassApp) -> egui_kittest::Harness<'static, CompassApp> {
        egui_kittest::Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|_| app)
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
            config.app.app.default_symbol,
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

        assert_eq!(config.app.app.default_symbol, "600519");
        assert_eq!(config.app.app.default_timeframe, "1w");
        assert_eq!(config.app.parquet.dir, "/custom/parquet/dir");
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
            config.app.app.default_symbol,
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
    fn setup_fonts_installs_embedded_fonts_no_panic() {
        let ctx = egui::Context::default();
        compass_ui::fonts::setup_fonts(&ctx);
    }

    #[test]
    fn init_tracing_no_panic() {
        let _guard = crate::init_tracing();
    }

    // --- render_toolbar kittest tests ---

    #[test]
    fn render_toolbar_renders_segmented_and_theme_dropdown() {
        let mut app = build_compass_app(egui::Context::default());
        let harness = egui_kittest::Harness::new_ui(|ui| {
            app.render_toolbar(ui);
        });

        let _ = harness.get_by_label("1d");
        let _ = harness.get_by_label_contains("compass_dark");
    }

    #[test]
    fn render_toolbar_renders_adjusted_price_tag() {
        let mut app = build_compass_app(egui::Context::default());
        let harness = egui_kittest::Harness::new_ui(|ui| {
            app.render_toolbar(ui);
        });

        let _ = harness.get_by_label("前复权");
    }

    #[test]
    fn render_toolbar_timeframe_switch_changes_index() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.run();
            harness.get_by_label("1w").click();
            harness.step();
        }

        assert_eq!(app.timeframe_index, 1);
    }

    #[test]
    fn render_toolbar_theme_switch_changes_theme_and_rebuilds_dock_style() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.run();
            harness.get_by_label_contains("compass_dark").click();
            harness.run();
            harness.get_by_label("compass_light").click();
            harness.run();
        }

        assert_eq!(app.theme.name(), "compass_light");
        assert_eq!(
            app.dock_style.tab_bar.bg_fill,
            ColorTokens::light().bg_panel,
            "dock style must be rebuilt from the new theme tokens"
        );
    }

    #[test]
    fn render_toolbar_fetch_sets_loading() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.run();
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
    fn render_toolbar_sidebar_toggle_flips_visibility() {
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.run();
            harness
                .get_by_label(egui_phosphor::regular::SIDEBAR_SIMPLE)
                .click();
            harness.step();
        }

        assert!(!app.sidebar_visible, "toggle must hide the sidebar");
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

    // ======================================================================
    // SEPA double-tab leaf (design M4 — egui_dock per-tab state objective
    // verification): with Chart + 东方SEPA sharing one leaf, the active tab
    // title renders text_primary while the inactive one renders text_secondary;
    // clicking the inactive tab raises it with the accent ring. This is the
    // shape-level evidence that dock_style needs no change for double tabs.
    // ======================================================================

    /// Collect the fallback colors of text shapes inside the tab bar band
    /// (y < 30 — `Style::tab_bar.height` is 28) of the rendered dock.
    fn tab_band_text_colors(shapes: &[egui::Shape]) -> Vec<egui::Color32> {
        let mut colors = Vec::new();
        fn walk(shapes: &[egui::Shape], colors: &mut Vec<egui::Color32>) {
            for shape in shapes {
                match shape {
                    egui::Shape::Vec(inner) => walk(inner, colors),
                    egui::Shape::Text(text) if text.pos.y < 30.0 => {
                        colors.push(text.fallback_color);
                    }
                    _ => {}
                }
            }
        }
        walk(shapes, &mut colors);
        colors
    }

    /// True when a stroke of `color` intersects the tab bar band (y < 40).
    fn tab_band_has_stroke(shapes: &[egui::Shape], color: egui::Color32) -> bool {
        fn walk(shapes: &[egui::Shape], color: egui::Color32) -> bool {
            shapes.iter().any(|shape| match shape {
                egui::Shape::Vec(inner) => walk(inner, color),
                egui::Shape::Rect(rect) => rect.stroke.color == color && rect.rect.min.y < 40.0,
                egui::Shape::Path(path) => {
                    path.stroke.color == egui::epaint::ColorMode::Solid(color)
                        && path.points.iter().any(|p| p.y < 40.0)
                }
                _ => false,
            })
        }
        walk(shapes, color)
    }

    /// Position of the first text shape whose galley contains `needle`
    /// (used to locate a tab title for the click interaction).
    fn text_pos_containing(shapes: &[egui::Shape], needle: &str) -> Option<egui::Pos2> {
        fn walk(shapes: &[egui::Shape], needle: &str) -> Option<egui::Pos2> {
            for shape in shapes {
                match shape {
                    egui::Shape::Vec(inner) => {
                        if let Some(pos) = walk(inner, needle) {
                            return Some(pos);
                        }
                    }
                    egui::Shape::Text(text) if text.galley.text().contains(needle) => {
                        return Some(text.pos);
                    }
                    _ => {}
                }
            }
            None
        }
        walk(shapes, needle)
    }

    #[test]
    fn double_tab_leaf_renders_active_and_inactive_styles() {
        use crate::citizens::chart::ChartCitizen;
        use crate::citizens::logger::LoggerPanel;
        use crate::citizens::screener::ScreenerPanel;
        use crate::citizens::sepa::SepaPanel;
        use crate::dispatcher::register_citizens;
        use crate::messages::{FetchRequest, RunScreenerRequest, RunSepaRequest};
        use crate::state::SharedState;
        use crate::tabs::TabViewer;
        use egui_dock::{DockArea, DockState};
        use egui_mobius::factory;

        let tokens = compass_ui::tokens::ThemeTokens::dark();
        let mut dispatcher = Dispatcher::new();
        let registered = register_citizens(&mut dispatcher);
        let mut chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let mut logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let mut screener = ScreenerPanel::new(
            CitizenId::new(SCREENER_ID),
            registered.screener,
            None,
            Box::new(|_| {}),
            &tokens,
        );
        let mut sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, &tokens);
        let (run_signal, _run_slot) = factory::create_signal_slot::<RunScreenerRequest>();
        let (sepa_signal, _sepa_slot) = factory::create_signal_slot::<RunSepaRequest>();
        let (work_signal, _work_slot) = factory::create_signal_slot::<FetchRequest>();
        let shared = SharedState::new("000001", "1d");
        let theme = CompassTheme::compass_dark();

        let mut dock_state =
            DockState::new(vec![Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa)]);
        let mut logger_export_clicked = false;
        let mut viewer = TabViewer {
            dispatcher: &mut dispatcher,
            chart: &mut chart,
            logger: &mut logger,
            screener: &mut screener,
            sepa: &mut sepa,
            run_screener_signal: &run_signal,
            sepa_signal: &sepa_signal,
            work_signal: &work_signal,
            screener_industries: &[],
            screener_boards: &[],
            shared_state: &shared,
            theme: &theme,
            logger_export_clicked: &mut logger_export_clicked,
        };

        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(800.0, 400.0))
            .build_ui(|ui| {
                let style = compass_ui::dock_style::dock_style(theme.tokens());
                DockArea::new(&mut dock_state)
                    .style(style)
                    .show_inside(ui, &mut viewer);
            });
        harness.run();

        let c = theme.tokens().color;
        let shapes: Vec<egui::Shape> = harness
            .output()
            .shapes
            .iter()
            .map(|clipped| clipped.shape.clone())
            .collect();
        let band_colors = tab_band_text_colors(&shapes);
        assert!(
            band_colors.contains(&c.text_primary),
            "active tab title must render text_primary, got {band_colors:?}"
        );
        assert!(
            band_colors.contains(&c.text_secondary),
            "inactive tab title must render text_secondary, got {band_colors:?}"
        );
        assert!(
            !tab_band_has_stroke(&shapes, c.accent),
            "no leaf focused yet: no accent ring in the tab band"
        );

        // Click the 东方SEPA tab (located via its rendered title) — the same
        // interaction path a user performs; it focuses the leaf and raises
        // the tab with the accent ring while Chart turns inactive.
        let sepa_title = text_pos_containing(&shapes, "东方SEPA").expect("sepa tab title rendered");
        harness.event(egui::Event::PointerMoved(
            sepa_title + egui::vec2(10.0, 10.0),
        ));
        harness.step();
        harness.event(egui::Event::PointerButton {
            pos: sepa_title + egui::vec2(10.0, 10.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
        harness.event(egui::Event::PointerButton {
            pos: sepa_title + egui::vec2(10.0, 10.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();

        let focused_shapes: Vec<egui::Shape> = harness
            .output()
            .shapes
            .iter()
            .map(|clipped| clipped.shape.clone())
            .collect();
        assert!(
            tab_band_has_stroke(&focused_shapes, c.accent),
            "clicked tab must raise with the accent outline ring"
        );
        assert!(
            tab_band_text_colors(&focused_shapes).contains(&c.accent),
            "focused tab title must render in accent"
        );
        assert!(
            tab_band_text_colors(&focused_shapes).contains(&c.text_secondary),
            "the previously active Chart tab must turn inactive (text_secondary)"
        );
    }

    // ======================================================================
    // S7 three-column layout (design §6.1)
    // ======================================================================

    #[test]
    fn window_inner_size_is_1440x900() {
        assert_eq!(WINDOW_INNER_SIZE, egui::vec2(1440.0, 900.0));
    }

    #[test]
    fn layout_renders_three_columns() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let fetch_label = format!("{} Fetch", egui_phosphor::regular::DOWNLOAD_SIMPLE);
        let _ = harness.get_by_label(&fetch_label);
        let _ = harness.get_by(|n| n.placeholder() == Some("搜索自选"));
        let _ = harness.get_by_label("本地数据源 · 0 只");
        // Dock area renders: the logger citizen's "Logs: n/1000" counter is
        // visible (egui_dock paints tab buttons without accesskit labels, so
        // tab titles are asserted at the TabViewer::title unit level).
        let _ = harness.get_by_label_contains("Logs:");
    }

    #[test]
    fn sidebar_panel_is_left_anchored_at_240px() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let search = harness.get_by(|n| n.placeholder() == Some("搜索自选"));
        assert!(
            search.rect().min.x < 60.0,
            "sidebar must hug the left edge, got min.x={}",
            search.rect().min.x
        );
        let add_button = harness.get_by_label("\u{e3d4}");
        assert!(
            add_button.rect().max.x > 220.0,
            "sidebar must be ~240px wide (add button right edge), got max.x={}",
            add_button.rect().max.x
        );
    }

    #[test]
    fn status_bar_is_bottom_anchored_with_source_and_clock() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let source = harness.get_by_label("本地数据源 · 0 只");
        let rect = source.rect();
        assert!(
            rect.max.y > 870.0,
            "status bar must sit at the window bottom (900px), got max.y={}",
            rect.max.y
        );
        let _ = harness.get_by(|n| {
            n.role() == egui::accesskit::Role::Label
                && matches!(n.value(), Some(v) if v.len() == 8 && v.as_bytes()[2] == b':' && v.as_bytes()[5] == b':')
        });
    }

    #[test]
    fn sidebar_toggle_hides_and_reshows_sidebar() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        // Dismiss the startup data-missing modal so its backdrop stops
        // blocking clicks (the 100 ms fade completes within one 0.25 s step
        // of egui virtual time).
        harness.get_by_label("知道了").click();
        harness.step();
        harness.step();
        assert!(!harness.state().modal.is_open());

        harness
            .get_by_label(egui_phosphor::regular::SIDEBAR_SIMPLE)
            .click();
        harness.step();
        assert!(
            harness
                .query_all_by(|n| n.placeholder() == Some("搜索自选"))
                .next()
                .is_none(),
            "sidebar must be hidden after toggle click"
        );

        harness
            .get_by_label(egui_phosphor::regular::SIDEBAR_SIMPLE)
            .click();
        harness.step();
        let _ = harness.get_by(|n| n.placeholder() == Some("搜索自选"));
    }

    #[test]
    fn sidebar_empty_state_shows_when_no_stock_list() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        let _ = harness.get_by_label("自选股为空");
    }

    #[test]
    fn sidebar_row_click_fetches_selected_symbol() {
        let stocks = vec![StockBasic {
            symbol: "600519".to_string(),
            name: "贵州茅台".to_string(),
            area: None,
            industry: None,
            market: None,
            board: None,
            full_name: None,
            total_share: None,
            list_date: None,
            delist_date: None,
        }];
        let app = build_compass_app_with_stocks(egui::Context::default(), stocks);
        app.shared_state.symbol.set("600519".to_string());
        app.shared_state.watchlist.set(vec!["600519".to_string()]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.get_by_label("贵州茅台").click();
        harness.step();

        assert_eq!(harness.state().shared_state.symbol.get(), "600519");
        assert!(
            harness.state().shared_state.loading.get(),
            "sidebar select must trigger a fetch"
        );
        assert_eq!(
            harness.state().stock_picker.selected_symbol,
            "600519",
            "sidebar select must sync the picker"
        );
    }

    // ------------------------------------------------------------------
    // S8 watchlist wiring (design §6.2 / §6.5 scenario 3)
    // ------------------------------------------------------------------

    fn stock_basic(symbol: &str, name: &str) -> StockBasic {
        StockBasic {
            symbol: symbol.to_string(),
            name: name.to_string(),
            area: None,
            industry: None,
            market: None,
            board: None,
            full_name: None,
            total_share: None,
            list_date: None,
            delist_date: None,
        }
    }

    #[test]
    fn sidebar_add_button_adds_current_symbol_to_watchlist_and_persists() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("000001", "平安银行")],
        );
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.get_by_label("\u{e3d4}").click(); // ＋ add button
        harness.step();

        assert_eq!(
            harness.state().shared_state.watchlist.get(),
            vec!["000001".to_string()],
            "add must insert the current symbol"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config written");
        assert!(
            contents.contains("[watchlist]"),
            "watchlist section must be persisted, got: {contents}"
        );
        assert!(contents.contains("\"000001\""));

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn add_to_watchlist_deduplicates_and_sorts() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.watchlist.set(vec!["600519".to_string()]);

        app.add_to_watchlist("600519"); // already present → no-op
        assert_eq!(
            app.shared_state.watchlist.get(),
            vec!["600519".to_string()],
            "duplicate add must be rejected"
        );

        app.add_to_watchlist("000001");
        assert_eq!(
            app.shared_state.watchlist.get(),
            vec!["000001".to_string(), "600519".to_string()],
            "watchlist must stay sorted after insert"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config written");
        assert!(contents.contains("000001"), "insert must be persisted");

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn sidebar_delete_opens_danger_modal_and_removes_on_confirm() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[watchlist]\nsymbols = [\"600519\"]\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("600519", "贵州茅台")],
        );
        app.shared_state.symbol.set("600519".to_string());
        app.shared_state.watchlist.set(vec!["600519".to_string()]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        // The selected row reveals its × button without hovering.
        let mut delete_buttons: Vec<_> = harness.query_all_by_label("\u{e4f6}").collect();
        assert!(
            !delete_buttons.is_empty(),
            "selected row must show the delete button"
        );
        delete_buttons.remove(0).click();
        harness.step();

        // Danger confirm modal (design §6.5 scenario 3). One 0.25 s step
        // completes the 120/150 ms entry animation (egui virtual time) so
        // hit-testing is restored.
        harness.step();
        let _ = harness.get_by_label("移除自选");
        assert!(harness.state().modal.is_open());
        harness.get_by_label("移除").click();
        harness.step();
        harness.step();

        assert!(
            harness.state().shared_state.watchlist.get().is_empty(),
            "confirm must remove the symbol from the watchlist"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config written");
        assert!(
            !contents.contains("600519"),
            "persisted watchlist must drop the removed symbol, got: {contents}"
        );

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn sidebar_delete_modal_cancel_keeps_watchlist() {
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("600519", "贵州茅台")],
        );
        app.shared_state.symbol.set("600519".to_string());
        app.shared_state.watchlist.set(vec!["600519".to_string()]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let mut delete_buttons: Vec<_> = harness.query_all_by_label("\u{e4f6}").collect();
        delete_buttons.remove(0).click();
        harness.step();
        // One 0.25 s step completes the entry animation so the Cancel button
        // is clickable.
        harness.step();
        harness.get_by_label("保留").click();
        harness.step();

        assert_eq!(
            harness.state().shared_state.watchlist.get(),
            vec!["600519".to_string()],
            "cancel must keep the watchlist intact"
        );
        assert!(
            harness.state().modal.closing,
            "cancel must start the modal closing animation"
        );
        // Complete the fade: the pending delete is cleared without confirm
        // (one 0.25 s step > the 100 ms close fade).
        harness.step();
        assert!(!harness.state().modal.is_open());
        assert!(
            harness.state().pending_delete.is_none(),
            "dismissed modal must clear the pending delete"
        );
    }

    // ------------------------------------------------------------------
    // S8 Modal scenarios (design §6.5): startup guide + logger export
    // ------------------------------------------------------------------

    #[test]
    fn startup_modal_opens_when_stock_list_empty() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let _ = harness.get_by_label("数据未就绪");
        let _ = harness.get_by_label("知道了");
        assert!(
            harness.state().modal.is_open(),
            "data-missing modal must open on first frame"
        );
    }

    #[test]
    fn startup_modal_skipped_when_stock_list_present() {
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("600519", "贵州茅台")],
        );
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        assert!(
            harness.query_by_label("数据未就绪").is_none(),
            "no startup modal when stock data is present"
        );
    }

    #[test]
    fn startup_modal_dismisses_with_confirm() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.get_by_label("知道了").click();
        harness.step();
        assert!(
            harness.state().modal.closing,
            "知道了 must start the modal closing animation"
        );
    }

    #[test]
    fn export_logs_writes_entries_to_file() {
        let state = SharedState::new("000001", "1d");
        {
            let logger = egui_lens::ReactiveEventLogger::new(&state.log);
            logger.log_info("hello world");
            logger.log_warning("watch out");
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("logs.txt");

        crate::export_logs(&state, &path).expect("export succeeds");

        let contents = std::fs::read_to_string(&path).expect("file written");
        assert!(contents.contains("--- Logger Export ---"));
        assert!(contents.contains("hello world"));
        assert!(contents.contains("watch out"));
        assert!(
            contents.contains("[INFO]"),
            "level prefix present: {contents}"
        );
        assert!(
            contents.contains("[WARNING]"),
            "level prefix present: {contents}"
        );
    }

    #[test]
    fn export_logs_empty_state_writes_header_only() {
        let state = SharedState::new("000001", "1d");
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.txt");

        crate::export_logs(&state, &path).expect("export succeeds");
        let contents = std::fs::read_to_string(&path).expect("file written");
        assert!(contents.contains("--- Logger Export ---"));
    }

    #[test]
    fn handle_log_export_pick_pushes_success_toast_and_writes_file() {
        let mut app = build_compass_app(egui::Context::default());
        {
            let logger = egui_lens::ReactiveEventLogger::new(&app.shared_state.log);
            logger.log_info("fetch ok");
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.txt");

        app.handle_log_export_pick(path.clone());

        let toast = app.toast.pop().expect("success toast pushed");
        assert_eq!(toast.level, ToastLevel::Success);
        assert!(
            toast.message.contains("日志已导出"),
            "message: {}",
            toast.message
        );
        let contents = std::fs::read_to_string(&path).expect("file written");
        assert!(contents.contains("fetch ok"));
    }

    #[test]
    fn handle_log_export_pick_pushes_error_toast_on_failure() {
        let mut app = build_compass_app(egui::Context::default());
        // Writing into a path whose parent is a file must fail.
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let bad_path = blocker.join("out.txt");

        app.handle_log_export_pick(bad_path);

        let toast = app.toast.pop().expect("error toast pushed");
        assert_eq!(toast.level, ToastLevel::Error);
        assert!(
            toast.message.contains("日志导出失败"),
            "message: {}",
            toast.message
        );
    }

    #[test]
    fn logger_export_button_triggers_save_dialog() {
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("600519", "贵州茅台")],
        );
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        // The export icon button lives in the logger panel's title row.
        harness.get_by_label(egui_phosphor::regular::EXPORT).click();
        harness.step();
        harness.run_steps(2);
        // The save-file dialog window appears.
        let _ = harness.get_by_label_contains("Save File");
    }

    #[test]
    fn sidebar_watchlist_restores_from_config() {
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![
                stock_basic("000001", "平安银行"),
                stock_basic("600519", "贵州茅台"),
            ],
        );
        app.shared_state
            .watchlist
            .set(vec!["000001".to_string(), "600519".to_string()]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let _ = harness.get_by_label("平安银行");
        let _ = harness.get_by_label("贵州茅台");
        assert_eq!(
            harness.state().shared_state.watchlist.get().len(),
            2,
            "both watchlist symbols render as sidebar rows"
        );
    }

    // ------------------------------------------------------------------
    // S8 status bar market data (design §6.3)
    // ------------------------------------------------------------------

    fn make_bar(close: f64) -> egui_charts::model::Bar {
        egui_charts::model::Bar::new(
            chrono::Utc::now(),
            close - 1.0,
            close + 1.0,
            close - 2.0,
            close,
            1000.0,
        )
    }

    #[test]
    fn latest_quote_computes_price_and_change() {
        assert_eq!(latest_quote(&[]), (None, None));
        assert_eq!(
            latest_quote(&[make_bar(10.0)]),
            (Some(10.0), None),
            "single bar has no change"
        );
        let (price, change) = latest_quote(&[make_bar(10.0), make_bar(10.5)]);
        assert_eq!(price, Some(10.5));
        assert!(
            (change.expect("change present") - 5.0).abs() < 0.001,
            "change must be +5% vs previous close, got {change:?}"
        );
        let (_, change) = latest_quote(&[make_bar(0.0), make_bar(10.5)]);
        assert_eq!(change, None, "zero previous close must not divide");
    }

    #[test]
    fn status_bar_renders_price_and_change_from_bars() {
        let app = build_compass_app(egui::Context::default());
        app.shared_state
            .bars
            .set(vec![make_bar(10.0), make_bar(10.5)]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let _ = harness.get_by_label("10.50 +5.00%");
    }

    #[test]
    fn status_bar_omits_price_when_no_bars() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        assert!(
            harness.query_by_label_contains("+0.00%").is_none(),
            "no price text when no bars"
        );
    }

    // ======================================================================
    // S7 keyboard shortcuts (design §6.4 / §7)
    // ======================================================================

    #[test]
    fn slash_focuses_symbol_input() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.key_press(egui::Key::Slash);
        harness.run_steps(3);

        let input = harness.get_by(|n| {
            n.role() == egui::accesskit::Role::TextInput && n.value() == Some("000001".to_string())
        });
        assert!(
            input.is_focused(),
            "slash must focus the toolbar symbol input"
        );
    }

    #[test]
    fn ctrl_k_focuses_sidebar_search_input() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
        harness.run_steps(3);

        let search = harness.get_by(|n| n.placeholder() == Some("搜索自选"));
        assert!(
            search.is_focused(),
            "Ctrl+K must focus the sidebar search input"
        );
    }

    #[test]
    fn ctrl_enter_triggers_fetch() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Enter);
        harness.step();

        assert!(
            harness.state().shared_state.loading.get(),
            "Ctrl+Enter must trigger a fetch"
        );
    }

    #[test]
    fn digit_keys_switch_timeframe() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.key_press(egui::Key::Num2);
        harness.step();
        assert_eq!(harness.state().timeframe_index, 1);

        harness.key_press(egui::Key::Num3);
        harness.step();
        assert_eq!(harness.state().timeframe_index, 2);

        harness.key_press(egui::Key::Num1);
        harness.step();
        assert_eq!(harness.state().timeframe_index, 0);
    }

    #[test]
    fn digit_keys_do_not_switch_timeframe_while_typing_in_input() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        // Focus the symbol input via the `/` shortcut, then type a digit:
        // the timeframe must NOT flip while the input has focus.
        harness.key_press(egui::Key::Slash);
        harness.step();

        harness.key_press(egui::Key::Num2);
        harness.step();
        assert_eq!(
            harness.state().timeframe_index,
            0,
            "typing a digit in a focused input must not switch timeframe"
        );
    }

    // ------------------------------------------------------------------
    // Screener reverse-sync (Todo 6)
    // ------------------------------------------------------------------

    #[test]
    fn sync_picker_from_symbol_syncs_bare_code_and_clears_exchange() {
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.symbol.set("600519".to_string());
        app.last_screener_synced_symbol = "000001".to_string();

        app.sync_picker_from_symbol();

        assert_eq!(app.stock_picker.selected_symbol, "600519");
        assert!(
            app.stock_picker.selected_exchange.is_empty(),
            "bare code sync must clear stale exchange"
        );
        assert_eq!(app.last_screener_synced_symbol, "600519");
    }

    #[test]
    fn sync_picker_from_symbol_ignores_prefixed_symbol() {
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.symbol.set("sz.000001".to_string());
        app.last_screener_synced_symbol = "000001".to_string();

        app.sync_picker_from_symbol();

        assert_eq!(
            app.stock_picker.selected_symbol, "000001",
            "prefixed symbol must not clobber picker selection"
        );
        assert_eq!(
            app.last_screener_synced_symbol, "sz.000001",
            "marker still advances"
        );
    }

    #[test]
    fn sync_picker_from_symbol_noop_when_symbol_unchanged() {
        let mut app = build_compass_app(egui::Context::default());
        // marker == symbol at startup → no-op, picker untouched.
        app.sync_picker_from_symbol();
        assert_eq!(app.stock_picker.selected_symbol, "000001");
        assert_eq!(app.last_screener_synced_symbol, "000001");
    }

    // ------------------------------------------------------------------
    // Toolbar fetch (regression: prefixed symbol breaks the parquet lookup)
    // ------------------------------------------------------------------

    #[test]
    fn fetch_bars_sends_bare_symbol_even_when_exchange_selected() {
        let mut app = build_compass_app(egui::Context::default());
        app.stock_picker.selected_symbol = "000001".to_string();
        app.stock_picker.selected_exchange = "SZ".to_string();

        app.fetch_bars();

        assert_eq!(
            app.shared_state.symbol.get(),
            "000001",
            "parquet stores bare codes; prefixed 'sz.000001' would find no data"
        );
    }

    #[test]
    fn fetch_bars_sends_bare_symbol_when_no_exchange_selected() {
        let mut app = build_compass_app(egui::Context::default());
        app.stock_picker.selected_symbol = "600519".to_string();
        app.stock_picker.selected_exchange.clear();

        app.fetch_bars();

        assert_eq!(app.shared_state.symbol.get(), "600519");
    }

    // ------------------------------------------------------------------
    // Screener config persistence (Todo 7)
    // ------------------------------------------------------------------

    #[test]
    fn save_screener_config_roundtrips() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"600519\"\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let query = ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ma: Some(compass_types::MaCondition::BullishAlign),
            ..ScreenerQuery::default()
        };
        let save_result = crate::save_screener_config(&query);
        let loaded: FullConfig =
            toml::from_str(&std::fs::read_to_string(config_dir.join("config.toml")).unwrap())
                .unwrap();

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(save_result.is_ok(), "save should succeed");
        assert_eq!(
            loaded.app.app.default_symbol, "600519",
            "existing sections preserved"
        );
        assert_eq!(loaded.screener.industries, vec!["白酒".to_string()]);
        assert_eq!(
            loaded.screener.ma,
            Some(compass_types::MaCondition::BullishAlign)
        );
        assert!(
            loaded.screener.exclude_delisted,
            "default true survives roundtrip"
        );
    }

    #[test]
    fn save_screener_config_creates_file_when_missing() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let query = ScreenerQuery::default();
        let result = crate::save_screener_config(&query);
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("file created");

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(result.is_ok());
        assert!(
            contents.contains("[screener]"),
            "created file has [screener] section"
        );
    }

    #[test]
    fn load_config_parses_screener_section() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[screener]\nindustries = [\"银行\"]\nbreakout = { days = 120 }\n",
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

        assert_eq!(config.screener.industries, vec!["银行".to_string()]);
        assert_eq!(
            config.screener.breakout,
            Some(compass_types::BreakoutCondition::new(120))
        );
        assert!(
            config.screener.exclude_delisted,
            "missing key defaults true"
        );
    }

    #[test]
    fn load_config_missing_screener_section_uses_default() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"000001\"\n",
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

        assert_eq!(config.screener, ScreenerQuery::default());
        assert_eq!(config.app.app.default_symbol, "000001");
    }

    // ------------------------------------------------------------------
    // Watchlist persistence (S8)
    // ------------------------------------------------------------------

    #[test]
    fn save_watchlist_config_roundtrips() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"600519\"\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let symbols = vec!["000001".to_string(), "600519".to_string()];
        let save_result = crate::save_watchlist_config(&symbols);
        let loaded: FullConfig =
            toml::from_str(&std::fs::read_to_string(config_dir.join("config.toml")).unwrap())
                .unwrap();

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(save_result.is_ok(), "save should succeed");
        assert_eq!(
            loaded.app.app.default_symbol, "600519",
            "existing sections preserved"
        );
        assert_eq!(loaded.watchlist.symbols, symbols);
    }

    #[test]
    fn save_watchlist_config_creates_file_when_missing() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let result = crate::save_watchlist_config(&["600519".to_string()]);
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("file created");

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(result.is_ok());
        assert!(
            contents.contains("[watchlist]"),
            "created file has [watchlist] section"
        );
    }

    #[test]
    fn load_config_parses_watchlist_section() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[watchlist]\nsymbols = [\"600519\", \"000001\"]\n",
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

        assert_eq!(
            config.watchlist.symbols,
            vec!["600519".to_string(), "000001".to_string()]
        );
    }

    #[test]
    fn load_config_missing_watchlist_section_uses_default() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"000001\"\n",
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

        assert!(config.watchlist.symbols.is_empty());
        assert_eq!(config.app.app.default_symbol, "000001");
    }
}
