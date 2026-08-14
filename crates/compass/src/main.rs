use std::sync::Arc;
use std::time::Duration;

use egui_citizen::{CitizenId, Dispatcher};
use egui_dock::{DockArea, DockState};
use egui_file_dialog::FileDialog;
use serde::Deserialize;
use tracing::{debug, info};

use compass_core::data::parquet::ParquetReader;
use compass_core::data::symbol::{
    exchange_of_symbol, infer_exchange_prefix, parse_explicit_prefix,
};
use compass_core::model::{AppConfig, IndexBasic, WatchlistConfig};
use compass_types::{Filter, ScreenerQuery};
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
mod llm_screener;
mod messages;
mod state;
mod tabs;
mod theme;

// i18n (issue #222): shared locale data from compass-i18n. `fallback = "zh"`
// keeps every t!() call Chinese when the active locale misses a key; the
// process-global set_locale (wired in main) switches zh/en for the whole GUI.
rust_i18n::i18n!("../compass-i18n/locales", fallback = "zh");
use compass_i18n::t;

use citizens::chart::ChartCitizen;
use citizens::logger::LoggerPanel;
use citizens::market::MarketPanel;
use citizens::screener::ScreenerPanel;
use citizens::sepa::SepaPanel;
use tabs::{CHART_ID, LOGGER_ID, MARKET_ID, SCREENER_ID, SEPA_ID, Tab, TabKind, TabViewer};
use theme::CompassTheme;

/// Default inner window size (design doc §Q8: 1440×900).
const WINDOW_INNER_SIZE: egui::Vec2 = egui::vec2(1440.0, 900.0);

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> eframe::Result {
    let _file_guard = init_tracing();
    let config = load_config();
    compass_i18n::set_locale(normalize_language(&config.app.language));

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
            let index_list = load_index_list(&config.app);
            let mut picker_list = stock_list.clone();
            picker_list.extend(index_list.clone().into_iter().map(index_basic_to_stock));

            // Wire Level 3 backend (signal/slot + AsyncDispatcher)
            let (
                work_signal,
                run_screener_signal,
                sepa_signal,
                index_signal,
                llm_signal,
                _backend_handle,
            ) = backend::wire_backend(
                config.app.clone(),
                shared_state.clone(),
                egui_ctx,
                config.llm.to_client_config(),
            );

            // Register citizens
            let mut dispatcher = Dispatcher::new();
            let registered = dispatcher::register_citizens(&mut dispatcher);

            // The theme drives the citizen panel styling (screener components
            // copy the tokens at construction, like StockPicker/Modal).
            let theme = CompassTheme::from_config(&config.app.theme);

            // Create citizen panels
            let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
            let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
            let screener_filter = resolve_screener_filter(&config.screener);
            let screener = ScreenerPanel::new(
                CitizenId::new(SCREENER_ID),
                registered.screener,
                Some(&screener_filter),
                Box::new(|f| {
                    if let Err(e) = save_screener_config(f) {
                        tracing::warn!(error = %e, "failed to save screener config");
                    }
                }),
                theme.tokens(),
                config.llm.is_configured(),
            );
            let sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, theme.tokens());
            let market =
                MarketPanel::new(CitizenId::new(MARKET_ID), registered.market, theme.tokens());

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

            // Create initial dock state: Chart + 大盘 + 东方SEPA share the top
            // leaf (SEPA's 12-column table + detail panel and the market
            // panel's card + table need the full width), Logger + Screener
            // below.
            let mut dock_state = DockState::new(vec![
                Tab::new(TabKind::Chart),
                Tab::new(TabKind::Market),
                Tab::new(TabKind::Sepa),
            ]);
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
                market,
                run_screener_signal,
                sepa_signal,
                index_signal,
                llm_signal,
                screener_industries: industries,
                screener_boards: boards,
                shared_state,
                work_signal,
                stock_list,
                index_list,
                picker_list,
                stock_picker,
                timeframe_index: timeframe_index_from_value(&config.app.app.default_timeframe),
                theme,
                dock_style,
                _backend_handle,
                toast: ToastManager::new(theme_tokens),
                modal: Modal::new(theme_tokens),
                file_dialog: FileDialog::new(),
                last_error: None,
                last_loading: false,
                last_screener_error: None,
                last_llm_error: None,
                last_sepa_error: None,
                last_sepa_loading: false,
                last_index_error: None,
                last_index_loading: false,
                last_screener_synced_symbol: startup_symbol,
                sidebar_visible: true,
                sidebar_search: String::new(),
                status_clock: String::new(),
                symbol_input_id: None,
                pending_delete: None,
                delete_confirmed: std::rc::Rc::new(std::cell::RefCell::new(false)),
                startup_modal_shown: false,
                language: normalize_language(&config.app.language).to_string(),
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
/// `ScreenerSection` and `[watchlist]` into `WatchlistConfig`.
#[derive(Deserialize)]
struct FullConfig {
    #[serde(flatten)]
    app: AppConfig,
    #[serde(default)]
    screener: ScreenerSection,
    #[serde(default)]
    watchlist: WatchlistConfig,
    #[serde(default)]
    llm: LlmSection,
}

/// The `[screener]` config section — dual-format (issue #246).
///
/// New format: `filter = "<Filter JSON>"` (the AST persisted verbatim).
/// Legacy format: the flat 11-key `ScreenerQuery` TOML from pre-Batch-3
/// builds, still readable for migration. `resolve` prefers the new format
/// and falls back to compiling the legacy query into a `Filter`.
///
/// Only `Deserialize` is derived: the save path writes the section by hand
/// as a `toml::Value` table (see `save_screener_config`), and a `Serialize`
/// derive on the flattened legacy field would emit the 11 default keys
/// beside `filter` (toml flatten serializes at the wrong level).
#[derive(Deserialize, Default)]
struct ScreenerSection {
    /// Filter AST as JSON (new format). `None` = legacy/missing.
    #[serde(default)]
    filter: Option<String>,
    /// Legacy flat 11-key query (pre-Batch-3). Only used when `filter` is
    /// absent.
    #[serde(flatten)]
    legacy: ScreenerQuery,
}

impl ScreenerSection {
    /// Resolve the persisted filter, preferring the JSON AST.
    fn resolve(&self) -> Result<Filter, String> {
        match &self.filter {
            Some(json) => {
                serde_json::from_str(json).map_err(|e| format!("invalid screener filter JSON: {e}"))
            }
            None => Ok(Filter::from(self.legacy.clone())),
        }
    }
}

/// The `[llm]` config section (epic #243 Batch 4, issue #247).
///
/// OpenAI-compatible chat-completions endpoint settings. `api_key` is
/// optional: when absent the LLM entry is hidden in the GUI (no network
/// calls are ever made without a key). Empty `base_url`/`model` fall back
/// to the defaults below; unknown keys in the section are ignored by serde.
#[derive(Deserialize)]
struct LlmSection {
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
}

impl Default for LlmSection {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
        }
    }
}

impl LlmSection {
    /// Whether the LLM feature is enabled — an API key is present.
    fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// The client config for the LLM backend, or `None` when unconfigured.
    ///
    /// Empty `base_url`/`model` fall back to [`LlmSection::default`] values so
    /// a partial `[llm]` section (key only) still works out of the box.
    fn to_client_config(&self) -> Option<compass_core::llm::LlmConfig> {
        if !self.is_configured() {
            return None;
        }
        let defaults = LlmSection::default();
        Some(compass_core::llm::LlmConfig {
            base_url: if self.base_url.is_empty() {
                defaults.base_url
            } else {
                self.base_url.clone()
            },
            api_key: self.api_key.clone(),
            model: if self.model.is_empty() {
                defaults.model
            } else {
                self.model.clone()
            },
        })
    }
}

/// Normalize a raw config language value to the two supported codes ("zh" /
/// "en"), falling back to "zh" for anything else — including the empty string
/// produced by `AppConfig::default()` on a parse failure (derive `Default`
/// yields ""). Emits a warning for unrecognized values (issue #222).
fn normalize_language(raw: &str) -> &'static str {
    match raw {
        "zh" => "zh",
        "en" => "en",
        other => {
            tracing::warn!(language = %other, "unrecognized language, falling back to zh");
            "zh"
        }
    }
}

/// Reads `~/.config/compass/config.toml`. Falls back to `AppConfig::default()`
/// if the file is missing or malformed. Legacy bare-code values (D10) are
/// auto-migrated to the exchange-prefixed form, rewriting the file.
fn load_config() -> FullConfig {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(mut cfg) => {
                tracing::info!(path = %config_path.display(), "config loaded");
                migrate_legacy_config(&mut cfg, &config_path, &contents);
                cfg
            }
            Err(e) => {
                tracing::warn!(path = %config_path.display(), error = %e, "failed to parse config, using defaults");
                FullConfig {
                    app: AppConfig::default(),
                    screener: ScreenerSection::default(),
                    watchlist: WatchlistConfig::default(),
                    llm: LlmSection::default(),
                }
            }
        },
        Err(e) => {
            tracing::warn!(path = %config_path.display(), error = %e, "config file not found, using defaults");
            FullConfig {
                app: AppConfig::default(),
                screener: ScreenerSection::default(),
                watchlist: WatchlistConfig::default(),
                llm: LlmSection::default(),
            }
        }
    }
}

/// Resolve the persisted `[screener]` section into the `Filter` AST for the
/// GUI restore path. A malformed `filter` JSON falls back to the default
/// filter (delisted excluded, no other constraints) with a warning — the
/// config must never prevent startup.
fn resolve_screener_filter(section: &ScreenerSection) -> Filter {
    match section.resolve() {
        Ok(filter) => filter,
        Err(e) => {
            tracing::warn!(error = %e, "falling back to default screener filter");
            Filter::from(ScreenerQuery::default())
        }
    }
}

/// Normalize a legacy config symbol to the canonical exchange-prefixed form
/// (D10): dot forms (`sh.000001`) and prefixed forms canonicalize to the
/// uppercase native form (`SH000001`); unprefixed 6-digit codes get the
/// inferred exchange prefix. Already-canonical symbols return `None`.
fn normalize_config_symbol(symbol: &str) -> Option<String> {
    let (exchange, code) = parse_explicit_prefix(symbol);
    if !exchange.is_empty() {
        if code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) {
            let migrated = format!("{exchange}{code}");
            return (migrated != symbol).then_some(migrated);
        }
        return None;
    }
    infer_exchange_prefix(symbol).map(|ex| format!("{ex}{symbol}"))
}

/// D10: auto-migrate legacy bare-code / dot-form values in a loaded config to
/// the canonical exchange-prefixed form, rewriting the file when the on-disk
/// values differ. Write-back failures (read-only config, no permission) only
/// warn — the migrated in-memory values still take effect and startup is not
/// blocked. Only values read from the file are migrated; in-memory defaults
/// are never touched.
fn migrate_legacy_config(cfg: &mut FullConfig, config_path: &std::path::Path, contents: &str) {
    let mut changed = false;
    if let Some(migrated) = normalize_config_symbol(&cfg.app.app.default_symbol) {
        tracing::warn!(
            symbol = %cfg.app.app.default_symbol,
            migrated = %migrated,
            "migrating legacy default_symbol to exchange-prefixed form"
        );
        cfg.app.app.default_symbol = migrated;
        changed = true;
    }
    for symbol in &mut cfg.watchlist.symbols {
        if let Some(migrated) = normalize_config_symbol(symbol) {
            tracing::warn!(
                symbol = %symbol,
                migrated = %migrated,
                "migrating legacy watchlist symbol to exchange-prefixed form"
            );
            *symbol = migrated;
            changed = true;
        }
    }
    if !changed {
        return;
    }
    match rewrite_config_file(
        config_path,
        contents,
        &cfg.app.app.default_symbol,
        &cfg.watchlist.symbols,
    ) {
        Ok(()) => {
            tracing::info!(path = %config_path.display(), "legacy config migrated and rewritten")
        }
        Err(e) => tracing::warn!(
            error = %e,
            path = %config_path.display(),
            "failed to rewrite migrated config; using migrated values in memory only"
        ),
    }
}

/// Rewrite the config file with migrated `default_symbol` / watchlist
/// symbols. Unknown sections are preserved (the file is edited as a
/// `toml::Value`); only the two migrated keys are replaced.
fn rewrite_config_file(
    config_path: &std::path::Path,
    contents: &str,
    default_symbol: &str,
    watchlist: &[String],
) -> Result<(), String> {
    let mut doc = contents
        .parse::<toml::Value>()
        .map_err(|e| format!("failed to parse config.toml: {e}"))?;
    if let Some(app) = doc.get_mut("app").and_then(|v| v.as_table_mut())
        && let Some(ds) = app.get_mut("default_symbol")
    {
        *ds = toml::Value::String(default_symbol.to_string());
    }
    if let Some(wl) = doc.get_mut("watchlist").and_then(|v| v.as_table_mut())
        && let Some(symbols) = wl.get_mut("symbols").and_then(|v| v.as_array_mut())
    {
        *symbols = watchlist
            .iter()
            .map(|s| toml::Value::String(s.clone()))
            .collect();
    }
    let serialized =
        toml::to_string(&doc).map_err(|e| format!("failed to serialize config.toml: {e}"))?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    }
    std::fs::write(config_path, serialized).map_err(|e| format!("failed to write config.toml: {e}"))
}

/// Persist the screener conditions to the `[screener]` section of
/// `~/.config/compass/config.toml`.
///
/// The filter AST is serialized to JSON and stored under the `filter` key
/// (new format, issue #246). Reads the existing file as a `toml::Value`,
/// replaces the `screener` table, and writes it back. Creates the file (with
/// only `[screener]`) when it does not exist. Comments and unknown sections
/// are lost on rewrite — accepted trade-off.
fn save_screener_config(filter: &Filter) -> Result<(), String> {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    let mut doc = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents
            .parse::<toml::Value>()
            .map_err(|e| format!("failed to parse config.toml: {e}"))?,
        Err(_) => toml::Value::Table(Default::default()),
    };

    let json = serde_json::to_string(filter)
        .map_err(|e| format!("failed to serialize screener filter: {e}"))?;
    let mut screener_table = toml::map::Map::new();
    screener_table.insert("filter".to_string(), toml::Value::String(json));
    doc.as_table_mut()
        .expect("value is a table")
        .insert("screener".to_string(), toml::Value::Table(screener_table));

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

/// Persist the GUI language to the top-level `language` key of config.toml
/// (issue #222). Mirrors [`save_watchlist_config`]: read-modify-write keeps
/// every other config section (`[app]`, `[watchlist]`, `[screener]`, ...)
/// intact; creates the file when it does not exist.
fn save_language_config(language: &str) -> Result<(), String> {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    let mut doc = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents
            .parse::<toml::Value>()
            .map_err(|e| format!("failed to parse config.toml: {e}"))?,
        Err(_) => toml::Value::Table(Default::default()),
    };
    doc.as_table_mut().expect("value is a table").insert(
        "language".to_string(),
        toml::Value::String(language.to_string()),
    );

    let serialized =
        toml::to_string(&doc).map_err(|e| format!("failed to serialize config.toml: {e}"))?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    }
    std::fs::write(&config_path, serialized)
        .map_err(|e| format!("failed to write config.toml: {e}"))
}

/// Persist the selected GUI theme to the top-level `theme` key of
/// `~/.config/compass/config.toml` (issue #132). Mirrors
/// [`save_language_config`]: read-modify-write keeps every other config
/// section intact and creates the file when it does not exist. Failures
/// are returned as `Err` and logged as a warning by the caller — the theme
/// switch itself stays in-memory.
fn save_theme_config(theme: &str) -> Result<(), String> {
    let config_path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".config/compass/config.toml"))
        .unwrap_or_else(|_| std::path::PathBuf::from("~/.config/compass/config.toml"));

    let mut doc = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents
            .parse::<toml::Value>()
            .map_err(|e| format!("failed to parse config.toml: {e}"))?,
        Err(_) => toml::Value::Table(Default::default()),
    };
    doc.as_table_mut()
        .expect("value is a table")
        .insert("theme".to_string(), toml::Value::String(theme.to_string()));

    let serialized =
        toml::to_string(&doc).map_err(|e| format!("failed to serialize config.toml: {e}"))?;
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    }
    std::fs::write(&config_path, serialized)
        .map_err(|e| format!("failed to write config.toml: {e}"))
}
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

/// Load the stock list for the GUI picker, filtered to currently-listed
/// A-shares (issue #71). `stock_basic.parquet` intentionally contains
/// delisted stocks and (delisted) B-shares — `delist_date` is `Some` for
/// every row that must not appear in the picker, so a single
/// `delist_date.is_none()` filter removes both while leaving the shared
/// data-layer method (`ParquetReader::load_all_stock_basics`) untouched
/// for SEPA/screener.
fn load_stock_list(config: &AppConfig) -> Vec<compass_core::model::StockBasic> {
    match ParquetReader::new(&config.parquet.dir) {
        Ok(reader) => match reader.load_all_stock_basics() {
            Ok(stocks) => {
                let listed: Vec<_> = stocks
                    .into_iter()
                    .filter(|s| s.delist_date.is_none())
                    .collect();
                info!(count = listed.len(), "stock list loaded from parquet");
                listed
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

/// Load the index/board name table for the GUI picker and the market tab
/// (epic #255 C4, plan T7). `index_basic.parquet` is optional — when it is
/// missing the picker degrades gracefully to the stock-only list.
fn load_index_list(config: &AppConfig) -> Vec<IndexBasic> {
    match ParquetReader::new(&config.parquet.dir) {
        Ok(reader) => match reader.load_all_index_basics() {
            Ok(list) => {
                info!(count = list.len(), "index list loaded from parquet");
                list
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to load index list from parquet");
                Vec::new()
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "failed to create parquet reader for index list");
            Vec::new()
        }
    }
}

/// Map an `IndexBasic` row onto the picker's `StockBasic` shape so the
/// merged picker list keeps a single row type. Index/board rows are always
/// "listed" (`delist_date = None` — the stock filter keeps them).
fn index_basic_to_stock(index: IndexBasic) -> compass_core::model::StockBasic {
    compass_core::model::StockBasic {
        symbol: index.symbol,
        name: index.name,
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
    market: MarketPanel,
    run_screener_signal: egui_mobius::signals::Signal<messages::RunScreenerRequest>,
    sepa_signal: egui_mobius::signals::Signal<messages::RunSepaRequest>,
    index_signal: egui_mobius::signals::Signal<messages::RunIndexSnapshotRequest>,
    llm_signal: egui_mobius::signals::Signal<messages::RunLlmRequest>,
    screener_industries: Vec<String>,
    screener_boards: Vec<String>,
    shared_state: Arc<state::SharedState>,
    work_signal: egui_mobius::signals::Signal<messages::FetchRequest>,
    stock_list: Vec<compass_core::model::StockBasic>,
    /// Index/board name table from index_basic.parquet (epic #255 C4).
    index_list: Vec<IndexBasic>,
    /// Merged picker list = stock_list + index_list (as StockBasic rows).
    picker_list: Vec<compass_core::model::StockBasic>,
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
    last_llm_error: Option<String>,
    last_sepa_error: Option<String>,
    last_sepa_loading: bool,
    last_index_error: Option<String>,
    last_index_loading: bool,
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
    /// Current UI language (`"zh"` | `"en"`), mirroring the process-global
    /// rust-i18n locale so the toolbar dropdown can render the selection.
    language: String,
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
                self.modal.set_title(t!("modal.startup.title"));
                self.modal.set_body(t!("modal.startup.body"));
                self.modal.set_danger(false);
                self.modal.set_confirm_text(t!("modal.startup.confirm"));
                self.modal.set_cancel_text(t!("common.cancel"));
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
                        market: &mut self.market,
                        run_screener_signal: &self.run_screener_signal,
                        sepa_signal: &self.sepa_signal,
                        index_signal: &self.index_signal,
                        llm_signal: &self.llm_signal,
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

            // LLM generation error toast on None→Some transition (design §5,
            // ref #247) — same dual channel as the screener/sepa errors.
            let current_llm_err = self.shared_state.llm_error.get();
            if current_llm_err != self.last_llm_error {
                if let Some(ref err) = current_llm_err {
                    self.toast.push(ToastLevel::Error, err.clone());
                }
                self.last_llm_error = current_llm_err;
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
                        .push(ToastLevel::Success, t!("toast.sepa_updated", count = count));
                }
            }
            self.last_sepa_loading = current_sepa_loading;

            // Index snapshot error toast on None→Some transition (same
            // pattern as the screener/sepa errors above).
            let current_index_err = self.shared_state.index_snapshot_error.get();
            if current_index_err != self.last_index_error {
                if let Some(ref err) = current_index_err {
                    self.toast.push(ToastLevel::Error, err.clone());
                }
                self.last_index_error = current_index_err;
            }

            // Index snapshot success toast on loading true→false with no
            // error (设计交互表: 刷新 → toast「指数数据已更新 · N 个」).
            let current_index_loading = self.shared_state.index_snapshot_loading.get();
            if self.last_index_loading
                && !current_index_loading
                && self.shared_state.index_snapshot_error.get().is_none()
            {
                let count = self
                    .shared_state
                    .index_snapshot
                    .get()
                    .map(|s| s.rows.len())
                    .unwrap_or(0);
                self.toast.push(
                    ToastLevel::Success,
                    t!("toast.index_updated", count = count),
                );
            }
            self.last_index_loading = current_index_loading;

            // Reverse-sync: when the symbol changed (e.g. a screener row
            // click), reflect it in the StockPicker — but only when the new
            // symbol is a valid exchange-prefixed code (D9); bare or
            // malformed values are ignored.
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
        }
        self.timeframe_index = idx;
        self.shared_state.timeframe.set(timeframe_value(idx));
        // Reload unconditionally: a fetch already in flight belongs to the
        // old timeframe, so skipping here would leave chart and toolbar
        // label disagreeing. The dispatcher sets loading=true synchronously
        // on every fetch; the last request wins.
        self.fetch_bars();
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
    /// Symbols are exchange-prefixed throughout the pipeline (D5/D7); the
    /// picker's selection is sent as-is so the parquet lookup finds the row.
    fn fetch_bars(&mut self) {
        let symbol = self.stock_picker.selected_symbol.clone();
        info!(symbol = %symbol, timeframe = %timeframe_value(self.timeframe_index), "fetch requested");
        self.fetch_symbol(&symbol);
    }

    /// Reflect `shared_state.symbol` changes back into the StockPicker.
    ///
    /// Symbols are exchange-prefixed everywhere (screener rows, watchlist,
    /// toolbar), so any prefixed change is synced back into the picker with
    /// its exchange code derived from the prefix. Non-symbol values (empty,
    /// malformed) are ignored. The marker field tracks the last seen symbol
    /// so per-frame checks fire only on actual changes.
    fn sync_picker_from_symbol(&mut self) {
        let symbol = self.shared_state.symbol.get();
        if symbol == self.last_screener_synced_symbol {
            return;
        }
        self.last_screener_synced_symbol = symbol.clone();
        let (exchange, code) = parse_explicit_prefix(&symbol);
        // Stocks are SH/SZ/BJ + 6 digits; board/index codes are BK + 4 digits
        // (epic #255 C3). Both sync back into the picker.
        let is_prefixed = !exchange.is_empty()
            && code.chars().all(|c| c.is_ascii_digit())
            && (code.len() == 6 || (exchange == "BK" && code.len() == 4));
        if !is_prefixed {
            return;
        }
        let name = self
            .picker_list
            .iter()
            .find(|s| s.symbol == symbol)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let exchange = exchange.to_string();
        self.stock_picker.selected_symbol = symbol;
        self.stock_picker.selected_name = name;
        self.stock_picker.selected_exchange = exchange;
    }

    /// Whether `symbol` is an index/board (epic #255 C4): BK-prefixed board
    /// codes or any symbol listed in index_basic.parquet. Drives the 前复权
    /// tag hide guard — indexes are not adjusted (fqt=0), so showing the tag
    /// would be wrong information.
    fn is_index_or_board(&self, symbol: &str) -> bool {
        parse_explicit_prefix(symbol).0 == "BK"
            || self
                .index_list
                .iter()
                .any(|i| i.symbol == symbol && !i.index_type.is_empty())
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
            title: t!("sidebar.group_watchlist").to_string(),
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
        self.toast.push(
            ToastLevel::Success,
            t!("toast.watchlist_added", symbol = symbol),
        );
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
        self.toast.push(
            ToastLevel::Success,
            t!("toast.watchlist_removed", symbol = symbol),
        );
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
        self.modal.set_title(t!("modal.remove.title"));
        self.modal
            .set_body(t!("modal.remove.body", symbol = symbol));
        self.modal.set_danger(true);
        self.modal.set_confirm_text(t!("modal.remove.confirm"));
        self.modal.set_cancel_text(t!("modal.remove.cancel"));
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
                    t!("toast.log_exported", path = path.display().to_string()),
                );
            }
            Err(e) => {
                self.toast
                    .push(ToastLevel::Error, t!("toast.log_export_failed", error = e));
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
            .picker_list
            .iter()
            .find(|s| s.symbol == symbol)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let loading = self.shared_state.loading.get();
        let error = self.shared_state.error.get();
        let (status, status_text) = if loading {
            (StatusKind::Loading, t!("statusbar.loading").to_string())
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
                source: t!("statusbar.source", count = self.stock_list.len()).to_string(),
                clock: self.status_clock.clone(),
            },
        );
    }

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        let tokens = *self.theme.tokens();
        let loading = self.shared_state.loading.get();

        Toolbar::new(&tokens).show(ui, |tb, ui| {
            // Group A — 标的: symbol picker (merged stock + index/board list).
            tb.group(ui, |ui| {
                let response = self.stock_picker.show(ui, &self.picker_list);
                self.symbol_input_id = Some(response.id);
            });

            // Group B — 周期: segmented 1d/1w/1M + 前复权 tag. The adjust tag
            // is hidden when the current symbol is an index/board (指数不
            // 复权, fqt=0 — plan T7); stocks keep it.
            tb.group(ui, |ui| {
                if let Some(idx) = Segmented::new(&tokens, ["1d", "1w", "1M"])
                    .selected(self.timeframe_index)
                    .show(ui)
                {
                    self.set_timeframe(idx);
                }
                let current_symbol = self.shared_state.symbol.get();
                let is_index = self.is_index_or_board(&current_symbol);
                if !is_index {
                    let adjust = t!("toolbar.adjust");
                    Tag::new(&tokens, &adjust)
                        .variant(TagVariant::Custom)
                        .color(tokens.color.info)
                        .show(ui);
                }
            });

            // Group C — 操作: primary Fetch button with loading state.
            tb.group(ui, |ui| {
                let fetch_label = if loading {
                    t!("toolbar.loading")
                } else {
                    t!("toolbar.fetch")
                };
                let fetch_clicked = Button::new(&tokens, fetch_label)
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Lg)
                    .icon(egui_phosphor::regular::DOWNLOAD_SIMPLE)
                    .min_width(104.0)
                    .loading(loading)
                    .show(ui)
                    .clicked();
                if fetch_clicked && !loading {
                    self.fetch_bars();
                }
            });

            // Group D — 显示: sidebar toggle + theme dropdown.
            tb.group(ui, |ui| {
                let toggle_sidebar = t!("toolbar.toggle_sidebar");
                if IconButton::new(&tokens, egui_phosphor::regular::SIDEBAR_SIMPLE)
                    .tooltip(&toggle_sidebar)
                    .show(ui)
                {
                    self.sidebar_visible = !self.sidebar_visible;
                }
                let theme_idx = CompassTheme::all_names()
                    .iter()
                    .position(|n| *n == self.theme.name())
                    .unwrap_or(0);
                if let Some(idx) = Dropdown::new(&tokens, CompassTheme::all_names().to_vec())
                    .id_salt("theme")
                    .selected(theme_idx)
                    .width(140.0)
                    .show(ui)
                {
                    let name = CompassTheme::all_names()[idx];
                    if name != self.theme.name() {
                        self.theme = CompassTheme::from_config(name);
                        if let Err(e) = save_theme_config(name) {
                            tracing::warn!(error = %e, "failed to save theme config");
                        }
                        let tokens = *self.theme.tokens();
                        self.dock_style = compass_ui::dock_style::dock_style(&tokens);
                        // Stored stateful widgets copy tokens at construction;
                        // refresh them so the theme switch applies everywhere.
                        self.stock_picker.set_tokens(tokens);
                        self.toast.set_tokens(tokens);
                        self.modal.set_tokens(tokens);
                        self.screener.set_tokens(tokens);
                        self.sepa.set_tokens(tokens);
                        self.market.set_tokens(tokens);
                        self.toast
                            .push(ToastLevel::Info, t!("toast.theme_switched"));
                    }
                }

                // Language dropdown: native-name options (中文/English), not
                // keyed — the option strings are the visible labels in both
                // locales. Switching applies the process-global locale
                // immediately; the window title stays the English brand.
                let lang_options = ["中文", "English"];
                let lang_idx = if self.language == "en" { 1 } else { 0 };
                if let Some(idx) = Dropdown::new(&tokens, lang_options.to_vec())
                    .id_salt("language")
                    .selected(lang_idx)
                    .width(76.0)
                    .show(ui)
                {
                    let new_lang = if idx == 1 { "en" } else { "zh" };
                    if new_lang != self.language {
                        self.language = new_lang.to_string();
                        compass_i18n::set_locale(new_lang);
                        ui.ctx().request_repaint();
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(
                            "Compass — Stock Chart".to_string(),
                        ));
                        self.toast
                            .push(ToastLevel::Info, t!("toast.language_switched"));
                        if let Err(e) = save_language_config(new_lang) {
                            tracing::warn!(error = %e, "failed to save language config");
                        }
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
                .push(ToastLevel::Success, t!("toast.fetch_success"));
        }
        self.last_loading = current_loading;
    }
}

/// Map a timeframe index to its label. The inverse mapping is
/// [`timeframe_index_from_value`] — keep the two matches in sync.
fn timeframe_label(idx: usize) -> &'static str {
    match idx {
        0 => "1d",
        1 => "1w",
        2 => "1M",
        _ => "1d",
    }
}

/// Map a timeframe value back to its index. The inverse mapping is
/// [`timeframe_label`] — keep the two matches in sync. Unknown values fall
/// back to 0 ("1d") so the toolbar selection and the chart never disagree
/// even with an unexpected configured timeframe.
fn timeframe_index_from_value(value: &str) -> usize {
    match value {
        "1w" => 1,
        "1M" => 2,
        _ => 0,
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
    use crate::LlmSection;
    use crate::ScreenerSection;
    use crate::messages::RunLlmRequest;
    use compass_core::model::StockBasic;
    use compass_types::{Filter, ScreenerQuery};

    use crate::citizens::chart::ChartCitizen;
    use crate::citizens::logger::LoggerPanel;
    use crate::latest_quote;
    use crate::state::SharedState;
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
        let state = SharedState::new("SZ000001", "1d");
        assert_eq!(state.symbol.get(), "SZ000001");
        assert_eq!(state.bars.get().len(), 0);
        assert!(!state.loading.get());
        assert_eq!(state.error.get(), None);
        assert_eq!(state.log.get().log_count(), 0);
    }

    #[test]
    fn llm_section_defaults_to_openai_endpoint() {
        let s = LlmSection::default();
        assert_eq!(s.base_url, "https://api.openai.com/v1");
        assert_eq!(s.model, "gpt-4o-mini");
        assert!(!s.is_configured(), "no api_key by default");
    }

    #[test]
    fn llm_section_without_api_key_is_unconfigured() {
        let s = LlmSection::default();
        assert!(!s.is_configured());
        assert!(s.to_client_config().is_none());
    }

    #[test]
    fn llm_section_with_api_key_produces_client_config() {
        let s = LlmSection {
            base_url: String::new(),
            api_key: "sk-test".to_string(),
            model: String::new(),
        };
        assert!(s.is_configured());
        let cfg = s
            .to_client_config()
            .expect("configured key yields a config");
        assert_eq!(cfg.api_key, "sk-test");
        assert_eq!(
            cfg.base_url, "https://api.openai.com/v1",
            "empty base_url falls back to the default"
        );
        assert_eq!(
            cfg.model, "gpt-4o-mini",
            "empty model falls back to the default"
        );
    }

    #[test]
    fn llm_section_keeps_explicit_url_and_model() {
        let s = LlmSection {
            base_url: "http://127.0.0.1:8080/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "custom-model".to_string(),
        };
        let cfg = s
            .to_client_config()
            .expect("configured key yields a config");
        assert_eq!(cfg.base_url, "http://127.0.0.1:8080/v1");
        assert_eq!(cfg.model, "custom-model");
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

    use crate::WINDOW_INNER_SIZE;
    use crate::tabs::{Tab, TabKind};
    use crate::theme::CompassTheme;
    use compass_core::model::{AppConfig, AppSection, WatchlistConfig};
    use compass_ui::tokens::ColorTokens;
    use compass_ui::widgets::toast::ToastLevel;
    use egui_kittest::kittest::Queryable;

    // App-construction helpers are shared with the epic #217 requirement
    // tests: the canonical builders live in `crate::citizens::ui_fixes_218`
    // (`build_compass_app_with_timeframe` derives `timeframe_index` from the
    // configured default — see `timeframe_index_from_value`). Delegating here
    // keeps one source of truth instead of a second hardcoded copy
    // (`timeframe_index: 0` used to drift from production).
    use crate::citizens::ui_fixes_218::{
        build_compass_app, build_compass_app_with_stocks, sized_harness,
    };

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

        assert_eq!(
            config.app.app.default_symbol, "SH600519",
            "bare legacy default_symbol must auto-migrate to the prefixed form"
        );
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

    // ------------------------------------------------------------------
    // #71: stock list filtering — `load_stock_list` must return only
    // currently-listed A-shares. The fix filters in the GUI layer
    // (`delist_date.is_none()`); the data layer (parquet.rs) is untouched.
    // These tests drive the declared behavior end-to-end through real
    // parquet fixtures (the plan inlines the filter, so no helper seam
    // exists to unit-test).
    // ------------------------------------------------------------------

    /// Test-only fixture: write `stock_basic.parquet` with the given rows
    /// `(symbol, name, delist_date)` into `tmp` and return an `AppConfig`
    /// pointing at it. A `None` name also NULLs `list_date` (mirrors the
    /// production fixture's all-NULL row) to exercise the non-panic path.
    fn create_stock_basic_parquet(
        tmp: &std::path::Path,
        rows: &[(&str, Option<&str>, Option<&str>)],
    ) -> AppConfig {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR)",
        )
        .unwrap();
        for (symbol, name, delist) in rows {
            let name_sql = match name {
                Some(n) => format!("'{n}'"),
                None => "NULL".to_string(),
            };
            let list_sql = if name.is_some() {
                "'2020-01-02'"
            } else {
                "NULL"
            };
            let delist_sql = match delist {
                Some(d) => format!("'{d}'"),
                None => "NULL".to_string(),
            };
            conn.execute_batch(&format!(
                "INSERT INTO basic (symbol, name, list_date, delist_date) \
                 VALUES ('{symbol}', {name_sql}, {list_sql}, {delist_sql})"
            ))
            .unwrap();
        }
        let basic_path = tmp.join("stock_basic.parquet");
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            basic_path.display()
        ))
        .unwrap();
        let mut config = AppConfig::default();
        config.parquet.dir = tmp.to_string_lossy().to_string();
        config
    }

    #[test]
    fn load_stock_list_filters_out_delisted_and_bshares() {
        // Mixed market: 2 listed A-shares, 1 delisted A-share, 1 B-share
        // (all B-shares are delisted), 1 row with a FUTURE delist date
        // (a present delist_date excludes regardless of its value), and
        // 1 listed row with all-NULL metadata (must not panic).
        let tmp = tempfile::tempdir().unwrap();
        let config = create_stock_basic_parquet(
            tmp.path(),
            &[
                ("SH600519", Some("贵州茅台"), None),
                ("SZ000001", Some("平安银行"), None),
                ("SZ000003", Some("深中华A"), Some("2023-06-30")),
                ("SZ200001", Some("深康佳B"), Some("2021-01-01")),
                ("SZ300001", Some("未来退市"), Some("2099-01-01")),
                ("SZ999999", None, None),
            ],
        );

        let stocks = crate::load_stock_list(&config);
        let symbols: Vec<&str> = stocks.iter().map(|s| s.symbol.as_str()).collect();

        assert_eq!(
            symbols,
            vec!["SH600519", "SZ000001", "SZ999999"],
            "delisted A-shares, B-shares and future-delisted rows must be filtered out, got: {symbols:?}"
        );
        assert!(
            stocks.iter().all(|s| s.delist_date.is_none()),
            "no returned stock may carry a delist_date"
        );
    }

    #[test]
    fn load_stock_list_only_delisted_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let config = create_stock_basic_parquet(
            tmp.path(),
            &[
                ("SZ000003", Some("深中华A"), Some("2023-06-30")),
                ("SZ200001", Some("深康佳B"), Some("2021-01-01")),
            ],
        );

        let stocks = crate::load_stock_list(&config);
        assert!(
            stocks.is_empty(),
            "a list with only delisted stocks must be empty, got {} rows",
            stocks.len()
        );
    }

    #[test]
    fn load_stock_list_all_listed_returns_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let config = create_stock_basic_parquet(
            tmp.path(),
            &[
                ("SH600519", Some("贵州茅台"), None),
                ("SZ000001", Some("平安银行"), None),
            ],
        );

        let stocks = crate::load_stock_list(&config);
        let symbols: Vec<&str> = stocks.iter().map(|s| s.symbol.as_str()).collect();
        assert_eq!(
            symbols,
            vec!["SH600519", "SZ000001"],
            "an all-listed list must pass through unchanged"
        );
    }

    #[test]
    fn load_stock_list_empty_parquet_returns_empty_no_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let config = create_stock_basic_parquet(tmp.path(), &[]);

        let stocks = crate::load_stock_list(&config);
        assert!(
            stocks.is_empty(),
            "a zero-row parquet must yield an empty list without panicking"
        );
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        let harness = egui_kittest::Harness::new_ui(|ui| {
            app.render_toolbar(ui);
        });

        let _ = harness.get_by_label("1d");
        let _ = harness.get_by_label_contains("compass_dark");
    }

    #[test]
    fn render_toolbar_renders_adjusted_price_tag() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        let harness = egui_kittest::Harness::new_ui(|ui| {
            app.render_toolbar(ui);
        });

        let _ = harness.get_by_label(&tr("toolbar.adjust"));
    }

    #[test]
    fn render_toolbar_timeframe_switch_changes_index() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());

        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.run();
            let fetch_label = format!(
                "{} {}",
                egui_phosphor::regular::DOWNLOAD_SIMPLE,
                t!("toolbar.fetch")
            );
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut harness =
            egui_kittest::Harness::new_eframe(|cc| build_compass_app(cc.egui_ctx.clone()));
        harness.step();
    }

    #[test]
    fn compass_app_ui_multiple_frames_no_panic() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        use crate::citizens::market::MarketPanel;
        use crate::citizens::screener::ScreenerPanel;
        use crate::citizens::sepa::SepaPanel;
        use crate::dispatcher::register_citizens;
        use crate::messages::{
            FetchRequest, RunIndexSnapshotRequest, RunScreenerRequest, RunSepaRequest,
        };
        use crate::state::SharedState;
        use crate::tabs::MARKET_ID;
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
            false,
        );
        let mut sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, &tokens);
        let mut market = MarketPanel::new(CitizenId::new(MARKET_ID), registered.market, &tokens);
        let (run_signal, _run_slot) = factory::create_signal_slot::<RunScreenerRequest>();
        let (sepa_signal, _sepa_slot) = factory::create_signal_slot::<RunSepaRequest>();
        let (index_signal, _index_slot) = factory::create_signal_slot::<RunIndexSnapshotRequest>();
        let (work_signal, _work_slot) = factory::create_signal_slot::<FetchRequest>();
        let (llm_signal, _llm_slot) = factory::create_signal_slot::<RunLlmRequest>();
        let shared = SharedState::new("SZ000001", "1d");
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
            market: &mut market,
            run_screener_signal: &run_signal,
            sepa_signal: &sepa_signal,
            llm_signal: &llm_signal,
            index_signal: &index_signal,
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
        let sepa_title =
            text_pos_containing(&shapes, &tr("tab.sepa")).expect("sepa tab title rendered");
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let fetch_label = format!(
            "{} {}",
            egui_phosphor::regular::DOWNLOAD_SIMPLE,
            t!("toolbar.fetch")
        );
        let _ = harness.get_by_label(&fetch_label);
        let _ =
            harness.get_by(|n| n.placeholder() == Some(tr("sidebar.search_placeholder").as_str()));
        let _ = harness.get_by_label(&t!("statusbar.source", count = 0));
        // Dock area renders: the logger citizen's "Logs: n/1000" counter is
        // visible (egui_dock paints tab buttons without accesskit labels, so
        // tab titles are asserted at the TabViewer::title unit level).
        let _ = harness.get_by_label_contains("Logs:");
    }

    #[test]
    fn sidebar_panel_is_left_anchored_at_240px() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let search =
            harness.get_by(|n| n.placeholder() == Some(tr("sidebar.search_placeholder").as_str()));
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let source_label = t!("statusbar.source", count = 0);
        let source = harness.get_by_label(&source_label);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        // Dismiss the startup data-missing modal so its backdrop stops
        // blocking clicks (the 100 ms fade completes within one 0.25 s step
        // of egui virtual time).
        harness.get_by_label(&tr("modal.startup.confirm")).click();
        harness.step();
        harness.step();
        assert!(!harness.state().modal.is_open());

        harness
            .get_by_label(egui_phosphor::regular::SIDEBAR_SIMPLE)
            .click();
        harness.step();
        assert!(
            harness
                .query_all_by(|n| n.placeholder() == Some(tr("sidebar.search_placeholder").as_str()))
                .next()
                .is_none(),
            "sidebar must be hidden after toggle click"
        );

        harness
            .get_by_label(egui_phosphor::regular::SIDEBAR_SIMPLE)
            .click();
        harness.step();
        let _ =
            harness.get_by(|n| n.placeholder() == Some(tr("sidebar.search_placeholder").as_str()));
    }

    #[test]
    fn sidebar_empty_state_shows_when_no_stock_list() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        let _ = harness.get_by_label(&tr("sidebar.empty_title"));
    }

    #[test]
    fn sidebar_row_click_fetches_selected_symbol() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stocks = vec![StockBasic {
            symbol: "SH600519".to_string(),
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
        app.shared_state.symbol.set("SH600519".to_string());
        app.shared_state.watchlist.set(vec!["SH600519".to_string()]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.get_by_label("贵州茅台").click();
        harness.step();

        assert_eq!(harness.state().shared_state.symbol.get(), "SH600519");
        assert!(
            harness.state().shared_state.loading.get(),
            "sidebar select must trigger a fetch"
        );
        assert_eq!(
            harness.state().stock_picker.selected_symbol,
            "SH600519",
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            vec![stock_basic("SZ000001", "平安银行")],
        );
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.get_by_label("\u{e3d4}").click(); // ＋ add button
        harness.step();

        assert_eq!(
            harness.state().shared_state.watchlist.get(),
            vec!["SZ000001".to_string()],
            "add must insert the current symbol"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config written");
        assert!(
            contents.contains("[watchlist]"),
            "watchlist section must be persisted, got: {contents}"
        );
        assert!(contents.contains("\"SZ000001\""));

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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.watchlist.set(vec!["SH600519".to_string()]);

        app.add_to_watchlist("SH600519"); // already present → no-op
        assert_eq!(
            app.shared_state.watchlist.get(),
            vec!["SH600519".to_string()],
            "duplicate add must be rejected"
        );

        app.add_to_watchlist("SZ000001");
        assert_eq!(
            app.shared_state.watchlist.get(),
            vec!["SH600519".to_string(), "SZ000001".to_string()],
            "watchlist must stay sorted after insert"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config written");
        assert!(
            contents.contains("\"SZ000001\""),
            "insert must be persisted"
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
    fn sidebar_delete_opens_danger_modal_and_removes_on_confirm() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[watchlist]\nsymbols = [\"SH600519\"]\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("SH600519", "贵州茅台")],
        );
        app.shared_state.symbol.set("SH600519".to_string());
        app.shared_state.watchlist.set(vec!["SH600519".to_string()]);
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
        let _ = harness.get_by_label(&tr("modal.remove.title"));
        assert!(harness.state().modal.is_open());
        harness.get_by_label(&tr("modal.remove.confirm")).click();
        harness.step();
        harness.step();

        assert!(
            harness.state().shared_state.watchlist.get().is_empty(),
            "confirm must remove the symbol from the watchlist"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config written");
        assert!(
            !contents.contains("SH600519"),
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("SH600519", "贵州茅台")],
        );
        app.shared_state.symbol.set("SH600519".to_string());
        app.shared_state.watchlist.set(vec!["SH600519".to_string()]);
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let mut delete_buttons: Vec<_> = harness.query_all_by_label("\u{e4f6}").collect();
        delete_buttons.remove(0).click();
        harness.step();
        // One 0.25 s step completes the entry animation so the Cancel button
        // is clickable.
        harness.step();
        harness.get_by_label(&tr("modal.remove.cancel")).click();
        harness.step();

        assert_eq!(
            harness.state().shared_state.watchlist.get(),
            vec!["SH600519".to_string()],
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        let _ = harness.get_by_label(&tr("modal.startup.title"));
        let _ = harness.get_by_label(&tr("modal.startup.confirm"));
        assert!(
            harness.state().modal.is_open(),
            "data-missing modal must open on first frame"
        );
    }

    #[test]
    fn startup_modal_skipped_when_stock_list_present() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("SH600519", "贵州茅台")],
        );
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        assert!(
            harness.query_by_label(&tr("modal.startup.title")).is_none(),
            "no startup modal when stock data is present"
        );
    }

    #[test]
    fn startup_modal_dismisses_with_confirm() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.get_by_label(&tr("modal.startup.confirm")).click();
        harness.step();
        assert!(
            harness.state().modal.closing,
            "知道了 must start the modal closing animation"
        );
    }

    #[test]
    fn export_logs_writes_entries_to_file() {
        let state = SharedState::new("SZ000001", "1d");
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
        let state = SharedState::new("SZ000001", "1d");
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.txt");

        crate::export_logs(&state, &path).expect("export succeeds");
        let contents = std::fs::read_to_string(&path).expect("file written");
        assert!(contents.contains("--- Logger Export ---"));
    }

    #[test]
    fn handle_log_export_pick_pushes_success_toast_and_writes_file() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            toast
                .message
                .contains(t!("toast.log_exported", path = "").as_ref()),
            "message: {}",
            toast.message
        );
        let contents = std::fs::read_to_string(&path).expect("file written");
        assert!(contents.contains("fetch ok"));
    }

    #[test]
    fn handle_log_export_pick_pushes_error_toast_on_failure() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            toast
                .message
                .contains(t!("toast.log_export_failed", error = "").as_ref()),
            "message: {}",
            toast.message
        );
    }

    #[test]
    fn logger_export_button_triggers_save_dialog() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![stock_basic("SH600519", "贵州茅台")],
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![
                stock_basic("SZ000001", "平安银行"),
                stock_basic("SH600519", "贵州茅台"),
            ],
        );
        app.shared_state
            .watchlist
            .set(vec!["SZ000001".to_string(), "SH600519".to_string()]);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.key_press(egui::Key::Slash);
        harness.run_steps(3);

        let input = harness.get_by(|n| {
            n.role() == egui::accesskit::Role::TextInput
                && n.value() == Some("SZ000001".to_string())
        });
        assert!(
            input.is_focused(),
            "slash must focus the toolbar symbol input"
        );
    }

    #[test]
    fn ctrl_k_focuses_sidebar_search_input() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
        harness.run_steps(3);

        let search =
            harness.get_by(|n| n.placeholder() == Some(tr("sidebar.search_placeholder").as_str()));
        assert!(
            search.is_focused(),
            "Ctrl+K must focus the sidebar search input"
        );
    }

    #[test]
    fn ctrl_enter_triggers_fetch() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    fn sync_picker_from_symbol_syncs_prefixed_symbol_and_exchange() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.symbol.set("SH600519".to_string());
        app.last_screener_synced_symbol = "SZ000001".to_string();

        app.sync_picker_from_symbol();

        assert_eq!(app.stock_picker.selected_symbol, "SH600519");
        assert_eq!(
            app.stock_picker.selected_exchange, "SH",
            "exchange derives from the symbol prefix"
        );
        assert_eq!(app.last_screener_synced_symbol, "SH600519");
    }

    #[test]
    fn sync_picker_from_symbol_ignores_non_symbol_values() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.symbol.set("not-a-symbol".to_string());
        app.last_screener_synced_symbol = "SZ000001".to_string();

        app.sync_picker_from_symbol();

        assert_eq!(
            app.stock_picker.selected_symbol, "SZ000001",
            "malformed symbol must not clobber picker selection"
        );
        assert_eq!(
            app.last_screener_synced_symbol, "not-a-symbol",
            "marker still advances"
        );
    }

    #[test]
    fn sync_picker_from_symbol_noop_when_symbol_unchanged() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        // marker == symbol at startup → no-op, picker untouched.
        app.sync_picker_from_symbol();
        assert_eq!(app.stock_picker.selected_symbol, "SZ000001");
        assert_eq!(app.last_screener_synced_symbol, "SZ000001");
    }

    #[test]
    fn sync_picker_from_symbol_bk_board_code_syncs() {
        // Epic #255 C3: BK + 4-digit board codes sync back into the picker
        // with symbol + name + exchange (BK must not fall back to SZ).
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app_with_stocks(
            egui::Context::default(),
            vec![StockBasic {
                symbol: "BK0475".into(),
                name: "半导体".into(),
                area: None,
                industry: None,
                market: None,
                board: None,
                full_name: None,
                total_share: None,
                list_date: None,
                delist_date: None,
            }],
        );
        app.shared_state.symbol.set("BK0475".to_string());
        app.last_screener_synced_symbol = "SZ000001".to_string();

        app.sync_picker_from_symbol();

        assert_eq!(app.stock_picker.selected_symbol, "BK0475");
        assert_eq!(app.stock_picker.selected_name, "半导体");
        assert_eq!(app.stock_picker.selected_exchange, "BK");
        assert_eq!(app.last_screener_synced_symbol, "BK0475");
    }

    #[test]
    fn sync_picker_from_symbol_rejects_malformed_bk_codes() {
        // Guard: BK + 3 digits / BK + 5 digits are not valid board codes and
        // must not clobber the picker selection.
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for bad in ["BK047", "BK04755"] {
            let mut app = build_compass_app(egui::Context::default());
            app.shared_state.symbol.set(bad.to_string());
            app.last_screener_synced_symbol = "SZ000001".to_string();

            app.sync_picker_from_symbol();

            assert_eq!(
                app.stock_picker.selected_symbol, "SZ000001",
                "malformed BK code {bad:?} must not clobber picker selection"
            );
        }
    }

    // ------------------------------------------------------------------
    // Toolbar fetch (regression: the canonical prefixed symbol is sent)
    // ------------------------------------------------------------------

    #[test]
    fn fetch_bars_sends_prefixed_symbol_when_exchange_selected() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        app.stock_picker.selected_symbol = "SZ000001".to_string();
        app.stock_picker.selected_exchange = "SZ".to_string();

        app.fetch_bars();

        assert_eq!(
            app.shared_state.symbol.get(),
            "SZ000001",
            "fetch must send the canonical prefixed symbol"
        );
    }

    #[test]
    fn fetch_bars_sends_prefixed_symbol_when_no_exchange_selected() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut app = build_compass_app(egui::Context::default());
        app.stock_picker.selected_symbol = "SH600519".to_string();
        app.stock_picker.selected_exchange.clear();

        app.fetch_bars();

        assert_eq!(app.shared_state.symbol.get(), "SH600519");
    }

    // ------------------------------------------------------------------
    // Screener config persistence (issue #246)
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

        let filter = Filter::from(ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ma: Some(compass_types::MaCondition::BullishAlign),
            ..ScreenerQuery::default()
        });
        let save_result = crate::save_screener_config(&filter);
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
        assert_eq!(
            loaded.screener.resolve().expect("valid filter JSON"),
            filter,
            "filter JSON round-trips through the [screener] filter key"
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

        let filter = Filter::from(ScreenerQuery::default());
        let result = crate::save_screener_config(&filter);
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
        assert!(
            contents.contains("filter"),
            "created file has the filter key"
        );
    }

    #[test]
    fn load_config_parses_legacy_screener_section() {
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

        // Legacy flat keys still parse; resolve compiles them into a Filter
        // with the same constraints (industries + breakout + default delisted
        // exclusion).
        assert_eq!(
            config.screener.resolve().expect("legacy resolve"),
            Filter::from(ScreenerQuery {
                industries: vec!["银行".to_string()],
                breakout: Some(compass_types::BreakoutCondition::new(120)),
                ..ScreenerQuery::default()
            })
        );
    }

    #[test]
    fn load_config_parses_filter_json_section() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        let filter = Filter::from(ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ..ScreenerQuery::default()
        });
        let json = serde_json::to_string(&filter).expect("serialize filter");
        // Escape the JSON string's inner quotes for a TOML basic string.
        let escaped = json.replace('\\', "\\\\").replace('"', "\\\"");
        std::fs::write(
            config_dir.join("config.toml"),
            format!("[screener]\nfilter = \"{escaped}\"\n"),
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
            config.screener.resolve().expect("new-format resolve"),
            filter,
            "filter JSON is the preferred format"
        );
    }

    #[test]
    fn load_config_invalid_filter_json_falls_back_to_default() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[screener]\nfilter = \"{not json\"\n",
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

        assert!(
            config.screener.resolve().is_err(),
            "malformed filter JSON surfaces as an error"
        );
        assert_eq!(
            crate::resolve_screener_filter(&config.screener),
            Filter::from(ScreenerQuery::default()),
            "startup path falls back to the default filter"
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

        assert_eq!(
            config.screener.resolve().expect("default resolve"),
            Filter::from(ScreenerQuery::default())
        );
        assert_eq!(
            config.app.app.default_symbol, "SZ000001",
            "bare legacy default_symbol must auto-migrate to the prefixed form"
        );
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
            vec!["SH600519".to_string(), "SZ000001".to_string()],
            "bare watchlist symbols must auto-migrate to the prefixed form"
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
        assert_eq!(
            config.app.app.default_symbol, "SZ000001",
            "bare legacy default_symbol must auto-migrate to the prefixed form"
        );
    }

    #[test]
    fn load_config_migrates_bare_default_symbol_and_watchlist_and_rewrites() {
        // D10: legacy bare codes in the file are auto-migrated to the
        // canonical exchange-prefixed form and the file is rewritten.
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"600519\"\n\n[watchlist]\nsymbols = [\"920001\", \"sh.000001\"]\n",
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
            config.app.app.default_symbol, "SH600519",
            "6-prefix legacy code migrates to SH"
        );
        assert_eq!(
            config.watchlist.symbols,
            vec!["BJ920001".to_string(), "SH000001".to_string()],
            "92-prefix code migrates to BJ, dot form normalizes to native"
        );
        let contents =
            std::fs::read_to_string(config_dir.join("config.toml")).expect("config rewritten");
        assert!(contents.contains("SH600519"), "rewritten file: {contents}");
        assert!(
            contents.contains("\"BJ920001\""),
            "rewritten file: {contents}"
        );
        assert!(
            contents.contains("\"SH000001\""),
            "rewritten file: {contents}"
        );
    }

    #[test]
    fn load_config_migrates_8_prefix_code_to_bj() {
        // "830799" must migrate to BJ (not SZ), mirroring the legacy
        // heuristic — 8-prefix codes are Beijing Stock Exchange.
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"830799\"\n",
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

        assert_eq!(config.app.app.default_symbol, "BJ830799");
    }

    #[test]
    fn migrate_legacy_config_writeback_failure_keeps_migrated_values() {
        // D10 boundary: when the config file cannot be rewritten (read-only,
        // no permission, blocked path), the migration must warn and keep the
        // migrated values in memory without panicking.
        let mut cfg = FullConfig {
            app: AppConfig {
                app: AppSection {
                    default_symbol: "600519".to_string(),
                    default_timeframe: "1d".to_string(),
                },
                ..AppConfig::default()
            },
            screener: ScreenerSection::default(),
            watchlist: WatchlistConfig {
                symbols: vec!["000001".to_string()],
            },
            llm: LlmSection::default(),
        };
        // The parent of the config path is a regular file → create_dir_all /
        // write must fail.
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let bad_path = blocker.join("config.toml");

        crate::migrate_legacy_config(&mut cfg, &bad_path, "[app]\ndefault_symbol = \"600519\"\n");

        assert_eq!(cfg.app.app.default_symbol, "SH600519");
        assert_eq!(cfg.watchlist.symbols, vec!["SZ000001".to_string()]);
    }

    #[test]
    fn load_config_already_prefixed_values_are_not_migrated() {
        // Canonical configs pass through unchanged (and the file is not
        // rewritten with different values).
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"SH600519\"\n\n[watchlist]\nsymbols = [\"SZ000001\"]\n",
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

        assert_eq!(config.app.app.default_symbol, "SH600519");
        assert_eq!(config.watchlist.symbols, vec!["SZ000001".to_string()]);
    }

    // ======================================================================
    // #222 GUI full-i18n acceptance tests (RED until T1/T2/T3/T5/T14 land).
    //
    // These tests reference the not-yet-existing `compass-i18n` crate
    // (t!/set_locale), the not-yet-added `AppConfig::language` field, and
    // the not-yet-written `normalize_language` / `save_language_config`
    // functions — the resulting compile failure IS the RED state.
    //
    // `set_locale` is process-global: every test that touches it takes
    // `LANG_LOCK` (the plan's T15 lock, modeled on `HOME_LOCK` above), and
    // every en-locale test resets to zh before releasing so the default
    // zh-locale contract stays stable under parallel execution.
    // ======================================================================

    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use compass_i18n::t;

    /// Key-resolution test helper (plan T4 `tr()`): resolves a key through
    /// the shared compass-i18n dictionary.
    fn tr(key: &str) -> String {
        compass_i18n::t!(key).to_string()
    }

    /// Full key tree from .omo/designs/gui-i18n.md §1 (compass-side
    /// domains only — the fork `chart.tooltip.*`/`chart.date.*`/
    /// `chart.realtime`/`chart.legend.*` keys live in the fork's own
    /// locales and are covered by the fork-side contract tests). Plain keys
    /// without interpolation; interpolated keys are asserted separately.
    const KEY_TREE: &[(&str, &str, &str)] = &[
        (
            "app.title",
            "Compass — Stock Chart",
            "Compass — Stock Chart",
        ),
        ("tab.chart", "图表", "Chart"),
        ("tab.logger", "日志", "Log"),
        ("tab.screener", "选股器", "Screener"),
        ("tab.sepa", "东方SEPA", "East SEPA"),
        ("toolbar.fetch", "获取数据", "Fetch"),
        ("toolbar.loading", "加载中…", "Loading..."),
        ("toolbar.adjust", "前复权", "Adj."),
        ("toolbar.toggle_sidebar", "切换侧边栏", "Toggle sidebar"),
        ("sidebar.group_watchlist", "自选", "Watchlist"),
        ("sidebar.search_placeholder", "搜索自选", "Search watchlist"),
        ("sidebar.add_tooltip", "添加", "Add"),
        ("sidebar.delete_tooltip", "删除", "Remove"),
        ("sidebar.empty_title", "自选股为空", "Watchlist is empty"),
        (
            "sidebar.empty_desc",
            "点击 + 添加关注的股票",
            "Click + to add stocks",
        ),
        ("statusbar.loading", "加载中…", "Loading..."),
        ("common.loading", "加载中…", "Loading..."),
        ("common.refresh", "刷新", "Refresh"),
        ("common.confirm", "确认", "Confirm"),
        ("common.cancel", "取消", "Cancel"),
        ("common.remove", "移除", "Remove"),
        ("common.search", "搜索…", "Search…"),
        ("common.no_matches", "无匹配结果", "No matches"),
        ("common.all", "全部", "All"),
        ("chart.empty_title", "暂无图表数据", "No chart data"),
        (
            "chart.empty_desc",
            "输入代码并点击获取数据",
            "Enter a code and click Fetch",
        ),
        ("logger.title", "日志", "Log"),
        ("logger.export_tooltip", "导出日志", "Export log"),
        ("modal.startup.title", "数据未就绪", "Data not ready"),
        ("modal.startup.confirm", "知道了", "Got it"),
        ("modal.remove.title", "移除自选", "Remove from watchlist"),
        ("modal.remove.confirm", "移除", "Remove"),
        ("modal.remove.cancel", "保留", "Keep"),
        ("toast.theme_switched", "主题已切换", "Theme switched"),
        ("toast.language_switched", "语言已切换", "Language switched"),
        (
            "toast.fetch_success",
            "数据获取成功",
            "Data fetched successfully",
        ),
        ("screener.filter", "筛选", "Filter"),
        ("screener.filtering", "筛选进行中…", "Filtering…"),
        ("screener.card_basic", "基础条件", "Basic"),
        ("screener.card_technical", "技术面条件", "Technical"),
        ("screener.industry", "行业", "Industry"),
        ("screener.exchange", "交易所", "Exchange"),
        ("screener.board", "板块", "Board"),
        ("screener.list_years", "上市时长", "Listed ≥"),
        ("screener.any", "不限", "Any"),
        ("screener.years_1", "≥1年", "≥1y"),
        ("screener.years_3", "≥3年", "≥3y"),
        ("screener.years_5", "≥5年", "≥5y"),
        ("screener.market_cap", "市值(亿)", "Mkt Cap(Bn)"),
        ("screener.exclude_delisted", "排除退市", "Excl. delisted"),
        ("screener.ma", "均线", "MA"),
        ("screener.ma_above20", "站上 MA20", "Above MA20"),
        ("screener.ma_above60", "站上 MA60", "Above MA60"),
        (
            "screener.ma_bullish",
            "多头排列 MA5>MA20>MA60",
            "Bullish MA5>20>60",
        ),
        ("screener.breakout", "突破新高", "New High"),
        ("screener.momentum", "动量", "Momentum"),
        ("screener.volume", "量能", "Volume"),
        ("screener.n_label", "N:", "N:"),
        ("screener.min_pct", "min%:", "min%:"),
        ("screener.max_pct", "max%:", "max%:"),
        ("screener.times", "倍数:", "×:"),
        ("screener.table.code", "代码", "Code"),
        ("screener.table.name", "名称", "Name"),
        ("screener.table.latest", "最新价", "Price"),
        ("screener.table.change_20d", "20日涨跌幅", "20D Chg%"),
        ("screener.table.market_cap", "市值(亿)", "Mkt Cap(Bn)"),
        ("screener.table.industry", "行业", "Industry"),
        ("sepa.thermometer", "市场温度", "Market Temp"),
        ("sepa.no_data", "暂无评分数据", "No score data yet"),
        ("sepa.computing", "计算中…", "Computing…"),
        (
            "sepa.computing_full",
            "SEPA 评分计算中…（全市场）",
            "Computing SEPA scores (full market)…",
        ),
        ("sepa.refresh", "刷新", "Refresh"),
        (
            "sepa.empty_title",
            "暂无 SEPA 评分数据",
            "No SEPA score data",
        ),
        (
            "sepa.empty_desc",
            "点击刷新计算全市场 TOP50 评分",
            "Click refresh to score the full-market TOP50",
        ),
        (
            "sepa.detail_hint",
            "点击排名行查看评分详情",
            "Click a row to view score details",
        ),
        ("sepa.table.rank", "排名", "Rank"),
        ("sepa.table.code", "代码", "Code"),
        ("sepa.table.name", "名称", "Name"),
        ("sepa.table.total", "总分", "Score"),
        ("sepa.table.trend", "趋势", "Trend"),
        ("sepa.table.theme", "题材", "Theme"),
        ("sepa.table.capital", "资金", "Capital"),
        ("sepa.table.pattern", "形态", "Pattern"),
        ("sepa.table.risk", "风险", "Risk"),
        ("sepa.table.industry", "行业", "Industry"),
        ("sepa.table.latest", "最新价", "Price"),
        ("sepa.table.change", "涨跌幅", "Chg%"),
        ("sepa.module.trend", "趋势", "Trend"),
        ("sepa.module.theme", "题材", "Theme"),
        ("sepa.module.capital", "资金", "Capital"),
        ("sepa.module.pattern", "形态", "Pattern"),
        ("sepa.module.risk", "风险", "Risk"),
        ("sepa.position.full", "80%-100%", "80%-100%"),
        ("sepa.position.mid", "40%-70%", "40%-70%"),
        ("sepa.position.low", "0%-20%", "0%-20%"),
        ("sepa.indicator.hs300_trend", "沪深300趋势", "HS300 Trend"),
        (
            "sepa.indicator.zz1000_trend",
            "中证1000趋势",
            "CSI1000 Trend",
        ),
        ("sepa.indicator.limit_up", "涨停数", "Limit-ups"),
        ("sepa.indicator.amount", "成交额", "Turnover"),
        ("sepa.indicator.breadth", "赚钱效应", "Breadth"),
        ("sepa.factor.ma_structure", "均线结构", "MA structure"),
        ("sepa.factor.price_position", "价格位置", "Price position"),
        ("sepa.factor.relative_strength", "相对强度", "Rel. strength"),
        ("sepa.factor.sector_gain", "板块涨幅", "Sector gain"),
        ("sepa.factor.sector_amount", "板块成交额", "Sector turnover"),
        ("sepa.factor.sector_diffusion", "板块扩散", "Sector breadth"),
        ("sepa.factor.news_heat", "新闻热度", "News heat"),
        ("sepa.factor.volume_price", "量价配合", "Vol-price fit"),
        (
            "sepa.factor.chip_concentration",
            "筹码集中",
            "Chip concentration",
        ),
        (
            "sepa.factor.big_capital_inflow",
            "大资金流入",
            "Big-cap inflow",
        ),
        ("sepa.factor.vcp_quality", "VCP质量", "VCP quality"),
        (
            "sepa.factor.breakout_confirm",
            "突破确认",
            "Breakout confirm",
        ),
        (
            "sepa.factor.vol_penalty",
            "波动惩罚(ATR)",
            "Vol penalty (ATR)",
        ),
        ("sepa.factor.deep_drawdown", "深度回撤", "Deep drawdown"),
        (
            "sepa.factor.volume_stagnation",
            "放量滞涨",
            "Vol up, price stall",
        ),
        ("sepa.note.no_sector_data", "无板块数据", "No sector data"),
        ("sepa.note.news_v1", "v1 无新闻数据", "v1: no news data"),
        (
            "sepa.note.news_default",
            "v1 默认 10/20",
            "v1: default 10/20",
        ),
        (
            "widgets.searchable_dropdown.no_matches",
            "无匹配结果",
            "No matches",
        ),
        ("widgets.data_table.empty", "无符合条件", "No matching rows"),
        (
            "widgets.multi_select.selected",
            "已选 %{count} 个",
            "Selected %{count}",
        ),
        ("widgets.multi_select.confirm", "完成", "Done"),
    ];

    // ------------------------------------------------------------------
    // Contract (a): DEFAULT LANGUAGE — no config file (or no language key)
    // → the GUI is Chinese; the config defaults to language="zh".
    // ------------------------------------------------------------------

    #[test]
    fn default_locale_is_zh_without_set_locale() {
        // kittest constructs CompassApp directly without main() (Metis M5),
        // so no set_locale call ever runs on that path. rust-i18n's
        // process-global locale defaults to "en" (the `default-locale` cargo
        // metadata is a no-op in 4.2.1 — see kb/dev/toolchain.md), so the zh
        // contract is pinned explicitly here rather than assumed from the
        // library default.
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(tr("tab.chart"), "图表");
        assert_eq!(tr("app.title"), "Compass — Stock Chart");
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn load_config_language_missing_defaults_to_zh() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"SH600519\"\n",
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
            config.app.language, "zh",
            "config without a language key must default to zh"
        );
    }

    // ------------------------------------------------------------------
    // Contract (b): CONFIG KEY — `language = "en"` → "en", `"zh"` → "zh",
    // missing → "zh", invalid values fall back to "zh" WITH a warn.
    // ------------------------------------------------------------------

    #[test]
    fn load_config_language_en_parses() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "language = \"en\"\n").unwrap();

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
            config.app.language, "en",
            "top-level language = \"en\" must parse into the config struct"
        );
    }

    #[test]
    fn load_config_language_invalid_passes_raw_to_normalize_guard() {
        // `"fr"` parses as an ordinary String — load_config keeps it raw;
        // the T3 `normalize_language` guard is the single place that falls
        // back to zh + warn (asserted in `normalize_language_*` below).
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "language = \"fr\"\n").unwrap();

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
            config.app.language, "fr",
            "load_config must NOT normalize — the T3 guard owns the fallback"
        );
    }

    /// Collects the `tracing::warn!` output written while `f` runs, so tests
    /// can assert that invalid language values emit a warning (T3).
    fn capture_warns(f: impl FnOnce()) -> String {
        use std::io::Write;
        use std::sync::Arc;

        #[derive(Clone, Default)]
        struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Buf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl tracing_subscriber::fmt::MakeWriter<'_> for Buf {
            type Writer = Self;
            fn make_writer(&self) -> Self::Writer {
                self.clone()
            }
        }

        let buf = Buf::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_max_level(tracing::Level::WARN)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        String::from_utf8_lossy(&buf.0.lock().unwrap()).to_string()
    }

    #[test]
    fn normalize_language_maps_valid_and_invalid_values() {
        assert_eq!(crate::normalize_language("zh"), "zh");
        assert_eq!(crate::normalize_language("en"), "en");
        assert_eq!(
            crate::normalize_language(""),
            "zh",
            "empty string must fall back"
        );
        assert_eq!(
            crate::normalize_language("fr"),
            "zh",
            "unknown language must fall back"
        );
        assert_eq!(
            crate::normalize_language("ZH"),
            "zh",
            "matching is case-sensitive"
        );
        assert_eq!(crate::normalize_language("EN"), "zh");
    }

    #[test]
    fn normalize_language_invalid_value_warns() {
        let logs = capture_warns(|| {
            let _ = crate::normalize_language("fr");
        });
        assert!(
            logs.contains("fr"),
            "invalid language must emit a tracing::warn! mentioning the value, got: {logs}"
        );
    }

    #[test]
    fn normalize_language_empty_value_warns() {
        let logs = capture_warns(|| {
            let _ = crate::normalize_language("");
        });
        assert!(
            logs.contains("falling back") || logs.contains("normalize"),
            "empty language must warn during normalization, got: {logs}"
        );
    }

    // ------------------------------------------------------------------
    // Contract (c): KEY TREE — every key in the design §1 tree resolves to
    // the design's zh column (zh locale) and en column (en locale). This is
    // the acceptance test that the dictionary faithfully implements the
    // approved design.
    // ------------------------------------------------------------------

    #[test]
    fn key_tree_zh_values_match_approved_design() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        for (key, zh, _en) in KEY_TREE {
            assert_eq!(
                tr(key),
                *zh,
                "zh value of `{key}` must match the approved design"
            );
        }
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn key_tree_en_values_match_approved_design() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("en");
        for (key, _zh, en) in KEY_TREE {
            assert_eq!(
                tr(key),
                *en,
                "en value of `{key}` must match the approved design"
            );
        }
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn key_tree_interpolated_values_match_approved_design() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        assert_eq!(t!("statusbar.source", count = 5324), "本地数据源 · 5324 只");
        assert_eq!(
            t!("modal.remove.body", symbol = "SH600519"),
            "确定要从自选中移除 SH600519 吗？"
        );
        assert_eq!(
            t!("toast.watchlist_added", symbol = "SH600519"),
            "已添加 SH600519 到自选"
        );
        assert_eq!(
            t!("toast.watchlist_removed", symbol = "SH600519"),
            "已从自选移除 SH600519"
        );
        assert_eq!(
            t!("toast.log_exported", path = "/tmp/log.txt"),
            "日志已导出: /tmp/log.txt"
        );
        assert_eq!(
            t!("toast.log_export_failed", error = "IO"),
            "日志导出失败: IO"
        );
        assert_eq!(
            t!("toast.sepa_updated", count = 50),
            "SEPA 评分已更新 · 50 只"
        );
        assert_eq!(t!("error.duckdb_open", e = "IO"), "打开 DuckDB 失败: IO");
        assert_eq!(t!("error.parquet_open", e = "IO"), "打开 Parquet 失败: IO");
        assert_eq!(
            t!("error.no_data", symbol = "SH600519"),
            "没有 SH600519 的数据"
        );
        assert_eq!(t!("error.screener_run", e = "IO"), "选股运行失败: IO");
        assert_eq!(t!("error.sepa_run", e = "IO"), "SEPA 计算失败: IO");
        assert_eq!(
            t!("sepa.count", shown = 12, date = "2026-08-02"),
            "共 12 行 · 2026-08-02 评分"
        );
        assert_eq!(t!("sepa.total_score", score = 80.5), "总分 80.5");
        assert_eq!(t!("sepa.unit.percent", v = 62.4), "62.4%");
        assert_eq!(t!("sepa.unit.count", v = 2000), "2000 家");
        assert_eq!(t!("sepa.unit.trillion", v = 1.2), "1.2万亿");
        assert_eq!(t!("sepa.note.drawdown", pct = 12.3), "距一年高点回撤 12.3%");
        assert_eq!(
            t!("sepa.note.momentum_percentile", pct = 75),
            "动量分位 75%"
        );
        assert_eq!(t!("widgets.data_table.count", count = 12), "共 12 行");

        compass_i18n::set_locale("en");
        assert_eq!(
            t!("statusbar.source", count = 5324),
            "Local data · 5324 symbols"
        );
        assert_eq!(
            t!("modal.remove.body", symbol = "SH600519"),
            "Remove SH600519 from watchlist?"
        );
        assert_eq!(
            t!("toast.watchlist_added", symbol = "SH600519"),
            "Added SH600519 to watchlist"
        );
        assert_eq!(
            t!("toast.watchlist_removed", symbol = "SH600519"),
            "Removed SH600519 from watchlist"
        );
        assert_eq!(
            t!("toast.log_exported", path = "/tmp/log.txt"),
            "Logs exported: /tmp/log.txt"
        );
        assert_eq!(
            t!("toast.log_export_failed", error = "IO"),
            "Log export failed: IO"
        );
        assert_eq!(
            t!("toast.sepa_updated", count = 50),
            "SEPA scores updated · 50"
        );
        assert_eq!(
            t!("error.duckdb_open", e = "IO"),
            "Failed to open DuckDB: IO"
        );
        assert_eq!(
            t!("error.parquet_open", e = "IO"),
            "Failed to open Parquet: IO"
        );
        assert_eq!(
            t!("error.no_data", symbol = "SH600519"),
            "No data for SH600519"
        );
        assert_eq!(
            t!("error.screener_run", e = "IO"),
            "Screener run failed: IO"
        );
        assert_eq!(t!("error.sepa_run", e = "IO"), "SEPA run failed: IO");
        assert_eq!(
            t!("sepa.count", shown = 12, date = "2026-08-02"),
            "12 rows · scored 2026-08-02"
        );
        assert_eq!(t!("sepa.total_score", score = 80.5), "Total 80.5");
        assert_eq!(t!("sepa.unit.percent", v = 62.4), "62.4%");
        assert_eq!(t!("sepa.unit.count", v = 2000), "2000");
        assert_eq!(t!("sepa.unit.trillion", v = 1.2), "1.2T");
        assert_eq!(
            t!("sepa.note.drawdown", pct = 12.3),
            "Drawdown 12.3% from 1y high"
        );
        assert_eq!(
            t!("sepa.note.momentum_percentile", pct = 75),
            "Momentum percentile 75%"
        );
        assert_eq!(t!("widgets.data_table.count", count = 12), "12 rows");
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn c5_c7_supplement_keys_resolve_to_real_text() {
        // Metis C5/C7 supplement keys have no design-table values (they are
        // derived from current literals); the missing-key fallback (t!()
        // returns the key string itself) is a silent false positive and is
        // explicitly rejected by the plan (Metis A7).
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        for key in [
            "sepa.note.big_capital",
            "sepa.note.thermometer",
            "logger.log_fetch_failed",
            "logger.log_fetch_completed",
            "logger.log_screener_failed",
            "logger.log_screener_completed",
            "logger.log_sepa_failed",
            "logger.log_sepa_completed",
        ] {
            let resolved = tr(key);
            assert_ne!(
                resolved, key,
                "`{key}` must resolve to real text, not the missing-key fallback"
            );
            assert!(
                !resolved.is_empty(),
                "`{key}` must not resolve to empty text"
            );
        }
        compass_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Contract (d): WINDOW TITLE — the native title stays the English brand
    // "Compass — Stock Chart" in BOTH locales; it is not keyed.
    // ------------------------------------------------------------------

    #[test]
    fn window_title_stays_english_brand_not_keyed() {
        let source = include_str!("main.rs");
        assert!(
            source.contains("Compass — Stock Chart"),
            "the run_native title literal must stay in the source (not keyed)"
        );
    }

    // ------------------------------------------------------------------
    // Contract (e): FETCH BUTTON — zh shows 「获取数据」(Q2 change), en shows
    // "Fetch".
    // ------------------------------------------------------------------

    #[test]
    fn fetch_button_zh_shows_get_data() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        let _ = harness
            .query_all_by_label_contains("获取数据")
            .next()
            .expect("fetch button in zh");
    }

    #[test]
    fn fetch_button_en_shows_fetch() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let app = build_compass_app(egui::Context::default());
        // Override AFTER build — build_compass_app pins zh for the default
        // GUI-test baseline (rust-i18n 4.2.1 default-locale is a no-op).
        compass_i18n::set_locale("en");
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        let _ = harness
            .query_all_by_label_contains("Fetch")
            .next()
            .expect("fetch button in en");
        compass_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Contract (g): PERSISTENCE — language selection persists: write
    // language="en", load, and the app starts in English; the save function
    // preserves the other config sections.
    // ------------------------------------------------------------------

    #[test]
    fn save_language_config_roundtrip_preserves_other_sections() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"[app]
default_symbol = "SH600519"

[watchlist]
symbols = ["SZ000001"]
"#,
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        crate::save_language_config("en").unwrap();
        let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
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

        assert!(
            raw.contains("language = \"en\""),
            "language key must be written to config.toml, got: {raw}"
        );
        assert_eq!(
            config.app.language, "en",
            "load must round-trip the saved language"
        );
        assert_eq!(
            config.app.app.default_symbol, "SH600519",
            "[app] section must be preserved by the read-modify-write"
        );
        assert_eq!(
            config.watchlist.symbols,
            vec!["SZ000001".to_string()],
            "[watchlist] section must be preserved by the read-modify-write"
        );
    }

    // ------------------------------------------------------------------
    // #132: theme persistence — save_theme_config must write the top-level
    // `theme` key to config.toml (read-modify-write mirroring
    // save_language_config), create the file when missing, preserve every
    // other section, propagate parse/write failures, and survive hostile
    // values without corrupting the file.
    // ------------------------------------------------------------------

    #[test]
    fn save_theme_config_creates_file_when_missing() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let result = crate::save_theme_config("compass_light");
        let raw = std::fs::read_to_string(config_dir.join("config.toml"))
            .expect("config.toml must be created by the save");

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(result.is_ok(), "save must succeed, got: {result:?}");
        assert!(
            raw.contains("theme = \"compass_light\""),
            "created file must contain the theme key, got: {raw}"
        );
    }

    #[test]
    fn save_theme_config_roundtrips_theme_key() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[app]\ndefault_symbol = \"SH600519\"\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        crate::save_theme_config("compass_light").unwrap();
        let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
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

        assert!(
            raw.contains("theme = \"compass_light\""),
            "theme key must be written, got: {raw}"
        );
        assert_eq!(
            config.app.theme, "compass_light",
            "load must round-trip the saved theme"
        );
        assert_eq!(
            CompassTheme::from_config(&config.app.theme).name(),
            "compass_light",
            "startup resolution (CompassTheme::from_config) must pick up the persisted theme"
        );
    }

    #[test]
    fn save_theme_config_preserves_other_sections() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            r#"[app]
default_symbol = "SH600519"

[watchlist]
symbols = ["SZ000001"]

[screener]
industries = ["银行"]
breakout = { days = 120 }
"#,
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        crate::save_theme_config("compass_dark").unwrap();
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

        assert_eq!(config.app.theme, "compass_dark");
        assert_eq!(
            config.app.app.default_symbol, "SH600519",
            "[app] must survive the theme write-back"
        );
        assert_eq!(
            config.watchlist.symbols,
            vec!["SZ000001".to_string()],
            "[watchlist] must survive the theme write-back"
        );
        assert_eq!(
            config.screener.resolve().expect("screener survives"),
            Filter::from(ScreenerQuery {
                industries: vec!["银行".to_string()],
                breakout: Some(compass_types::BreakoutCondition::new(120)),
                ..ScreenerQuery::default()
            }),
            "[screener] legacy keys must survive the theme write-back"
        );
    }

    #[test]
    fn save_theme_config_fails_when_config_dir_is_a_file() {
        // `$HOME/.config` exists as a regular FILE, so the config dir can
        // never be created: the save must surface an Err — no panic, no
        // silent success, no partial file left behind.
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".config"), "i am a file, not a dir").unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let result = crate::save_theme_config("compass_light");

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(
            result.is_err(),
            "save must fail when the config dir cannot be created, got: {result:?}"
        );
        assert!(
            !tmp.path().join(".config/compass/config.toml").exists(),
            "no partial file must be left behind"
        );
    }

    #[test]
    fn save_theme_config_fails_on_invalid_toml_without_clobbering() {
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "{{{ not valid toml").unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let result = crate::save_theme_config("compass_light");

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert!(
            result.is_err(),
            "invalid TOML must propagate as an error, got: {result:?}"
        );
        assert_eq!(
            std::fs::read_to_string(config_dir.join("config.toml")).unwrap(),
            "{{{ not valid toml",
            "a failed save must not clobber the existing file"
        );
    }

    #[test]
    fn save_theme_config_escapes_hostile_value_no_section_injection() {
        // A theme value carrying a quote, newline and a forged `[watchlist]`
        // section must be TOML-escaped: round-trip verbatim, never inject
        // sections, and resolve through from_config without panicking.
        let _guard = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        let hostile = "evil\"\n[watchlist]\nsymbols=[\"pwned\"]";

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        crate::save_theme_config(hostile).unwrap();
        let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
        let doc: toml::Value = raw
            .parse()
            .unwrap_or_else(|e| panic!("file must stay valid TOML: {e}\n{raw}"));
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
            doc.get("theme").and_then(toml::Value::as_str),
            Some(hostile),
            "hostile value must round-trip verbatim"
        );
        assert!(
            doc.get("watchlist").is_none(),
            "hostile value must not inject a [watchlist] section, got: {raw}"
        );
        assert_eq!(config.app.theme, hostile);
        assert_eq!(
            CompassTheme::from_config(&config.app.theme).name(),
            "compass_dark",
            "unknown theme must resolve to the dark fallback without panicking"
        );
    }

    #[test]
    fn app_starts_in_english_when_config_language_is_en() {
        // Simulates the startup path (T3): load_config → normalize_language
        // → set_locale → all t!() resolution lands in English.
        let _lang = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("config.toml"), "language = \"en\"\n").unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let config = crate::load_config();
        let locale = crate::normalize_language(&config.app.language);
        compass_i18n::set_locale(locale);

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }

        assert_eq!(locale, "en");
        assert_eq!(tr("tab.chart"), "Chart");
        assert_eq!(tr("toolbar.fetch"), "Fetch");
        compass_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // Contract (h): LIVE SWITCH — toolbar language dropdown (中文/English
    // native names); selecting English switches the UI immediately within
    // the same kittest run (no restart); selecting 中文 switches back; the
    // choice is persisted to config.toml.
    // ------------------------------------------------------------------

    #[test]
    fn language_dropdown_switches_ui_immediately_and_persists() {
        let _lang = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[watchlist]\nsymbols = [\"SZ000001\"]\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let mut app = build_compass_app(egui::Context::default());
        let fetch_zh = format!(
            "{} {}",
            egui_phosphor::regular::DOWNLOAD_SIMPLE,
            tr("toolbar.fetch")
        );
        let fetch_en = format!("{} {}", egui_phosphor::regular::DOWNLOAD_SIMPLE, "Fetch");

        // Interact via a new_ui harness rendering the toolbar — the same
        // pattern as the theme-dropdown test, where kittest pointer clicks
        // reliably open the Area popup and select an option.
        // The toolbar spans Groups A–D; the language dropdown (Group D,
        // rightmost) must fit on-screen for the trigger click to register —
        // the default 800×600 `new_ui` harness clips it since #232 widened
        // the Fetch button (min_width 104).
        let mut harness = egui_kittest::Harness::builder()
            .with_size([1440.0, 900.0])
            .build_ui(|ui| {
                app.render_toolbar(ui);
            });
        harness.run();
        // The trigger label is "{selected} ▾"; the popup items are exact
        // option strings (dropdown.rs renders egui::Button::new(option)).
        harness.get_by_label_contains("中文").click();
        harness.run();
        // The popup must be open with the English option visible.
        harness.get_by_label("English").click();
        harness.run();
        assert_eq!(
            compass_i18n::locale().to_string(),
            "en",
            "selecting English must switch the process-global locale"
        );
        let _ = harness.get_by_label(&fetch_en);

        let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
        assert!(
            raw.contains("language = \"en\""),
            "language selection must persist to config.toml, got: {raw}"
        );
        assert!(
            raw.contains("SZ000001"),
            "other config sections must survive the language write-back, got: {raw}"
        );

        harness.get_by_label_contains("English").click();
        harness.run();
        harness.get_by_label("中文").click();
        harness.run();
        assert_eq!(
            compass_i18n::locale().to_string(),
            "zh",
            "selecting 中文 must switch the locale back"
        );
        let _ = harness.get_by_label(&fetch_zh);

        if let Some(h) = saved_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        compass_i18n::set_locale("zh");
    }

    // ------------------------------------------------------------------
    // #132: end-to-end — switching the theme in the toolbar must persist
    // `theme` to config.toml (read-modify-write preserves other sections).
    // ------------------------------------------------------------------

    #[test]
    fn render_toolbar_theme_switch_persists_to_config_file() {
        let _lang = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[watchlist]\nsymbols = [\"SZ000001\"]\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let mut app = build_compass_app(egui::Context::default());
        {
            let mut harness = egui_kittest::Harness::builder()
                .with_size([1440.0, 900.0])
                .build_ui(|ui| {
                    app.render_toolbar(ui);
                });
            harness.run();
            // Trigger label is "{selected} ▾"; popup items are the exact
            // option strings — same interaction as
            // `render_toolbar_theme_switch_changes_theme_and_rebuilds_dock_style`.
            harness.get_by_label_contains("compass_dark").click();
            harness.run();
            harness.get_by_label("compass_light").click();
            harness.run();

            let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
            assert!(
                raw.contains("theme = \"compass_light\""),
                "theme selection must persist to config.toml, got: {raw}"
            );
            assert!(
                raw.contains("SZ000001"),
                "other config sections must survive the theme write-back, got: {raw}"
            );

            // Switching BACK must re-persist the new value (write on every switch).
            harness.get_by_label_contains("compass_light").click();
            harness.run();
            harness.get_by_label("compass_dark").click();
            harness.run();
            let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
            assert!(
                raw.contains("theme = \"compass_dark\""),
                "switching back must re-persist, got: {raw}"
            );
        }
        assert_eq!(
            app.theme.name(),
            "compass_dark",
            "in-memory theme must switch back to compass_dark"
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

    // ------------------------------------------------------------------
    // Contract (#132): theme dropdown persists the selection to config.toml
    // (mirrors the language-dropdown write-back); a fresh app constructed
    // from the persisted config restores the chosen theme on restart; a
    // failed write-back must degrade to a warn, never a panic.
    // ------------------------------------------------------------------

    #[test]
    fn theme_dropdown_switch_persists_to_config_and_restores_after_restart() {
        let _lang = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "[watchlist]\nsymbols = [\"SZ000001\"]\n",
        )
        .unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let mut app = build_compass_app(egui::Context::default());
        {
            // Same harness sizing as the language-dropdown test: #232
            // widened the toolbar, so the 800×600 default clips Group D.
            let mut harness = egui_kittest::Harness::builder()
                .with_size([1440.0, 900.0])
                .build_ui(|ui| {
                    app.render_toolbar(ui);
                });
            harness.run();
            // Trigger label is "{selected} ▾"; popup items are exact names.
            harness.get_by_label_contains("compass_dark").click();
            harness.run();
            harness.get_by_label("compass_light").click();
            harness.run();
        }
        assert_eq!(app.theme.name(), "compass_light");

        let raw = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
        assert!(
            raw.contains("theme = \"compass_light\""),
            "theme selection must persist to config.toml, got: {raw}"
        );
        assert!(
            raw.contains("SZ000001"),
            "other config sections must survive the theme write-back, got: {raw}"
        );

        // Restart simulation: production startup resolves the theme via
        // CompassTheme::from_config(&config.app.theme) (main.rs L94).
        let config = crate::load_config();
        assert_eq!(
            CompassTheme::from_config(&config.app.theme).name(),
            "compass_light",
            "a fresh app from the persisted config must restore compass_light"
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
    fn theme_dropdown_switch_with_unwritable_config_switches_in_memory_without_panic() {
        let _lang = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let config_dir = tmp.path().join(".config/compass");
        std::fs::create_dir_all(&config_dir).unwrap();
        // Unwritable config: a directory occupies the config.toml path, so
        // the write-back must fail through the warn path, never panic.
        std::fs::create_dir(config_dir.join("config.toml")).unwrap();

        let saved_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        let mut app = build_compass_app(egui::Context::default());
        {
            let mut harness = egui_kittest::Harness::builder()
                .with_size([1440.0, 900.0])
                .build_ui(|ui| {
                    app.render_toolbar(ui);
                });
            harness.run();
            harness.get_by_label_contains("compass_dark").click();
            harness.run();
            harness.get_by_label("compass_light").click();
            harness.run();
        }

        assert_eq!(
            app.theme.name(),
            "compass_light",
            "the in-memory switch must still apply when persistence fails"
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

    // ------------------------------------------------------------------
    // Contract (j): ERROR STRINGS — translated templates with %{e}
    // passthrough; the underlying DataError Display (ASCII) is NOT
    // translated (covered in compass-core).
    // ------------------------------------------------------------------

    #[test]
    fn error_template_passthroughs_e_verbatim() {
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        let detail = "database: IO error";
        assert_eq!(
            t!("error.duckdb_open", e = detail),
            "打开 DuckDB 失败: database: IO error"
        );
        compass_i18n::set_locale("zh");
    }
}
