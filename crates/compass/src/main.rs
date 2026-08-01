use std::sync::Arc;
use std::time::Duration;

use egui_citizen::{CitizenId, Dispatcher};
use egui_dock::{DockArea, DockState};
use egui_file_dialog::FileDialog;
use serde::Deserialize;
use tracing::{debug, info};

use compass_core::data::parquet::ParquetReader;
use compass_core::model::AppConfig;
use compass_types::ScreenerQuery;
use compass_ui::widgets::button::{Button, ButtonSize, ButtonVariant};
use compass_ui::widgets::dropdown::Dropdown;
use compass_ui::widgets::icon_button::IconButton;
use compass_ui::widgets::modal::Modal;
use compass_ui::widgets::searchable_dropdown::{StockPicker, StockProjection};
use compass_ui::widgets::segmented::Segmented;
use compass_ui::widgets::sidebar::{Sidebar, SidebarEvent, SidebarGroup, SidebarItem};
use compass_ui::widgets::status_bar::{StatusBar, StatusBarData, StatusKind, StockSummary};
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
use tabs::{CHART_ID, LOGGER_ID, SCREENER_ID, Tab, TabKind, TabViewer};
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
            let (work_signal, run_screener_signal, _backend_handle) =
                backend::wire_backend(config.app.clone(), shared_state.clone(), egui_ctx);

            // Register citizens
            let mut dispatcher = Dispatcher::new();
            let registered = dispatcher::register_citizens(&mut dispatcher);

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
            );

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

            let theme = CompassTheme::from_config(&config.app.theme);
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
                toast: ToastManager::new(theme_tokens),
                modal: Modal::new(theme_tokens),
                file_dialog: FileDialog::new(),
                last_error: None,
                last_loading: false,
                last_screener_error: None,
                last_screener_synced_symbol: startup_symbol,
                sidebar_visible: true,
                sidebar_search: String::new(),
                status_clock: String::new(),
                symbol_input_id: None,
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
/// screener condition section. Flattening keeps existing TOML keys mapping to
/// `AppConfig` while `[screener]` parses into `ScreenerQuery`.
#[derive(Deserialize)]
struct FullConfig {
    #[serde(flatten)]
    app: AppConfig,
    #[serde(default)]
    screener: ScreenerQuery,
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
                }
            }
        },
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "config file not found, using defaults");
            FullConfig {
                app: AppConfig::default(),
                screener: ScreenerQuery::default(),
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

/// Projection mapping `StockBasic` rows into the searchable dropdown's fields.
///
/// The UI crate stays free of business-crate dependencies; the binary adapts
/// its own row type through projection functions.
fn stock_projection() -> StockProjection<compass_core::model::StockBasic> {
    StockProjection::new(
        |s: &compass_core::model::StockBasic| &s.symbol,
        |s: &compass_core::model::StockBasic| &s.name,
        |s: &compass_core::model::StockBasic| s.exchange.as_deref(),
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
    last_screener_synced_symbol: String,
    /// Whether the left watchlist sidebar is visible.
    sidebar_visible: bool,
    /// Sidebar search filter text.
    sidebar_search: String,
    /// Clock string refreshed every frame (`%H:%M:%S`, local time).
    status_clock: String,
    /// Widget id of the toolbar symbol input (for the `/` shortcut).
    symbol_input_id: Option<egui::Id>,
}

impl eframe::App for CompassApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.theme.apply_theme(ui.ctx());

        self.handle_shortcuts(ui);

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

            // Push screener error toast only on None→Some transition.
            let current_screener_err = self.shared_state.screener_error.get();
            if current_screener_err != self.last_screener_error {
                if let Some(ref err) = current_screener_err {
                    self.toast.push(ToastLevel::Error, err.clone());
                }
                self.last_screener_error = current_screener_err;
            }

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
        let (slash, ctrl_enter, ctrl_k, num1, num2, num3) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::Slash),
                i.key_pressed(egui::Key::Enter) && i.modifiers.command,
                i.key_pressed(egui::Key::K) && i.modifiers.command,
                i.key_pressed(egui::Key::Num1),
                i.key_pressed(egui::Key::Num2),
                i.key_pressed(egui::Key::Num3),
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

    /// Fetch the current toolbar selection (exchange-prefixed when an
    /// exchange is selected in the picker).
    fn fetch_bars(&mut self) {
        let symbol = if self.stock_picker.selected_exchange.is_empty() {
            self.stock_picker.selected_symbol.clone()
        } else {
            format!(
                "{}.{}",
                self.stock_picker.selected_exchange.to_lowercase(),
                self.stock_picker.selected_symbol
            )
        };
        info!(symbol = %symbol, timeframe = %timeframe_value(self.timeframe_index), "fetch requested");
        self.fetch_symbol(&symbol);
    }

    /// Reflect `shared_state.symbol` changes back into the StockPicker.
    ///
    /// Only bare 6-digit codes are synced — the toolbar writes prefixed
    /// symbols ("sz.000001") when an exchange is selected, and copying those
    /// back would corrupt the picker. The marker field tracks the last seen
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

    /// Left watchlist sidebar: search row + a "自选" group seeded with the
    /// current symbol (real watchlist groups arrive in S8).
    fn render_sidebar(&mut self, ui: &mut egui::Ui) {
        let tokens = *self.theme.tokens();
        let sidebar = Sidebar::new(&tokens);
        let symbol = self.shared_state.symbol.get();
        let query = self.sidebar_search.trim().to_lowercase();

        let mut items = Vec::new();
        for stock in &self.stock_list {
            if stock.symbol != symbol {
                continue;
            }
            let matches = query.is_empty()
                || stock.symbol.to_lowercase().contains(&query)
                || stock.name.to_lowercase().contains(&query);
            if matches {
                items.push(SidebarItem {
                    symbol: stock.symbol.clone(),
                    name: stock.name.clone(),
                    exchange: stock.exchange.clone().unwrap_or_default(),
                    selected: true,
                });
                break;
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
                SidebarEvent::Add | SidebarEvent::DeleteRequest { .. } => {}
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

        StatusBar::new(&tokens).show(
            ui,
            &StatusBarData {
                summary: Some(StockSummary {
                    symbol,
                    name,
                    price: None,
                    change: None,
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

            // Group B — 周期: segmented 1d/1w/1M.
            tb.group(ui, |ui| {
                if let Some(idx) = Segmented::new(&tokens, ["1d", "1w", "1M"])
                    .selected(self.timeframe_index)
                    .show(ui)
                {
                    self.set_timeframe(idx);
                }
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
                        self.dock_style = compass_ui::dock_style::dock_style(self.theme.tokens());
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
    use crate::state::SharedState;
    use crate::stock_projection;
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
    use compass_ui::widgets::toast::ToastManager;
    use egui_dock::DockState;
    use egui_kittest::kittest::Queryable;
    use std::sync::Arc;

    fn build_compass_app_with_stocks(
        egui_ctx: egui::Context,
        stocks: Vec<StockBasic>,
    ) -> CompassApp {
        let config = AppConfig::default();
        let shared_state = Arc::new(SharedState::new("000001", "1d"));

        let (work_signal, run_screener_signal, _backend_handle) =
            crate::backend::wire_backend(config, shared_state.clone(), egui_ctx);

        let mut dispatcher = Dispatcher::new();
        let registered = crate::dispatcher::register_citizens(&mut dispatcher);

        let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let screener = ScreenerPanel::new(
            CitizenId::new(SCREENER_ID),
            registered.screener,
            None,
            Box::new(|_| {}),
        );

        let theme = CompassTheme::compass_dark();
        let theme_tokens = *theme.tokens();
        let stock_picker = StockPicker::new(theme_tokens, "000001", stock_projection());
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
            last_screener_synced_symbol: startup_symbol,
            sidebar_visible: true,
            sidebar_search: String::new(),
            status_clock: String::new(),
            symbol_input_id: None,
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

        let _ = harness.get_by_label_contains("Fetch");
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
            exchange: Some("SH".to_string()),
            list_date: None,
            delist_date: None,
        }];
        let app = build_compass_app_with_stocks(egui::Context::default(), stocks);
        app.shared_state.symbol.set("600519".to_string());
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
}
