//! Shared data model types.
//!
//! Contains commands, application state, configuration, and stock metadata
//! used by both the GUI and CLI binaries.

use chrono::{DateTime, Utc};
use egui_charts::model::Bar;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Symbol identifiers
// ---------------------------------------------------------------------------

/// Info returned by symbol search (code + display name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// 6-digit stock code (e.g. "000001").
    pub code: String,
    /// Chinese display name (e.g. "平安银行").
    pub name: String,
}

/// Live market data for a stock.
///
/// Fetched from EastMoney realtime API. All fields are optional — the API
/// may return `null` for any field, especially outside trading hours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeQuote {
    /// Price-to-earnings ratio.
    pub pe: Option<f64>,
    /// Price-to-book ratio.
    pub pb: Option<f64>,
    /// Total share capital (万股).
    pub total_share: Option<f64>,
    /// Floating share capital (万股).
    pub float_share: Option<f64>,
    /// Daily price ceiling (涨停价).
    pub up_limit: Option<f64>,
    /// Daily price floor (跌停价).
    pub down_limit: Option<f64>,
}

/// Core stock metadata.
///
/// Contains the stock's identifying information: code, display name,
/// industry classification, market segment, exchange, and listing dates.
/// Stored in the `stock_basic` table in DuckDB and `stock_basic.parquet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockBasic {
    /// 6-digit stock code.
    pub symbol: String,
    /// Chinese display name.
    pub name: String,
    /// Geographic area.
    pub area: Option<String>,
    /// Industry classification.
    pub industry: Option<String>,
    /// Market segment (e.g. "主板", "创业板").
    pub market: Option<String>,
    /// Exchange code ("SH", "SZ", "BJ").
    pub exchange: Option<String>,
    /// First trading date.
    pub list_date: Option<chrono::NaiveDate>,
    /// Last trading date (if delisted).
    pub delist_date: Option<chrono::NaiveDate>,
}

/// Adjustment factor record from Baostock (per-day multiplier for price adjustment).
///
/// `adj_factor` is the cumulative adjustment factor for a given date. To compute
/// forward-adjusted (前复权) or backward-adjusted (后复权) prices, multiply the
/// unadjusted price by `adj_factor` and divide by the latest factor.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AdjFactor {
    /// Trade date in "YYYYMMDD" format (e.g. "20250722").
    pub trade_date: String,
    /// Cumulative adjustment factor. 1.0 = no adjustment.
    pub adj_factor: f64,
}

// ---------------------------------------------------------------------------
// App command (UI → worker thread)
// ---------------------------------------------------------------------------

/// Commands sent from the UI to the backend worker.
///
/// Kept for backward compatibility with compass-data CLI which
/// uses `Cmd` internally for retry logic and batch processing.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Fetch OHLCV bars for a symbol/timeframe/date-range.
    FetchBars {
        /// 6-digit stock code.
        symbol: String,
        /// Timeframe string (e.g. "1d", "1w").
        timeframe: String,
        /// Earliest date to fetch.
        range_start: DateTime<Utc>,
        /// Latest date to fetch.
        range_end: DateTime<Utc>,
    },
}

// ---------------------------------------------------------------------------
// Application config (loaded from ~/.config/compass/config.toml)
// ---------------------------------------------------------------------------

/// Root application configuration loaded from `~/.config/compass/config.toml`.
///
/// All fields use `#[serde(default)]` — missing keys fall back to the
/// per-struct `Default` implementation. Partial configs work.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    /// Data directory settings (default: parquet_dir = "parquet_data").
    pub database: DatabaseConfig,
    #[serde(default)]
    /// EastMoney API settings.
    pub api: ApiConfig,
    #[serde(default)]
    /// Application behavior (default symbol, timeframe).
    pub app: AppSection,
}

/// Database connection settings.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_parquet_dir")]
    /// Path to the parquet_data directory for OHLCV data.
    pub parquet_dir: String,
}

/// EastMoney API connection settings.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_base_url")]
    /// EastMoney K-line API base URL.
    pub base_url: String,
    #[serde(default = "default_timeout_secs")]
    /// HTTP request timeout in seconds.
    pub timeout_secs: u64,
}

/// Application-level settings: default stock and timeframe on startup.
#[derive(Debug, Clone, Deserialize)]
pub struct AppSection {
    #[serde(default = "default_symbol")]
    /// Stock code displayed on startup.
    pub default_symbol: String,
    #[serde(default = "default_timeframe")]
    /// Timeframe displayed on startup (e.g. "1d").
    pub default_timeframe: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            parquet_dir: default_parquet_dir(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            default_symbol: default_symbol(),
            default_timeframe: default_timeframe(),
        }
    }
}

fn default_parquet_dir() -> String {
    "parquet_data".into()
}
fn default_base_url() -> String {
    "https://push2his.eastmoney.com".into()
}
fn default_timeout_secs() -> u64 {
    10
}
fn default_symbol() -> String {
    "000001".into()
}
fn default_timeframe() -> String {
    "1d".into()
}

// ---------------------------------------------------------------------------
// Shared application state (UI + worker both access via Arc<Mutex<>>)
// ---------------------------------------------------------------------------

/// Bars keyed by (symbol, timeframe).
pub type BarsMap = std::collections::HashMap<(String, String), Vec<Bar>>;

/// Shared mutable state between the UI (main) and worker (tokio) threads.
///
/// Protected by `Arc<Mutex<>>` — the UI reads on every frame, the worker
/// writes after each async operation.
///
/// Note: bar version tracking (`bars_version`) has been removed. The egui-mobius
/// reactive architecture uses `Dynamic<T>` for automatic change propagation
/// instead of manual version comparison.
pub struct CompassState {
    /// All loaded OHLCV bars, keyed by (symbol, timeframe).
    pub bars: BarsMap,
    /// Currently viewed symbol.
    pub current_symbol: String,
    /// Currently viewed timeframe.
    pub current_timeframe: String,
    /// True while a fetch is in-flight.
    pub loading: bool,
    /// Last error message, if any.
    pub error: Option<String>,
}

impl CompassState {
    /// Create a new state with the given defaults.
    ///
    /// Bars start empty; loaded on first fetch.
    pub fn new(default_symbol: &str, default_timeframe: &str) -> Self {
        Self {
            bars: BarsMap::new(),
            current_symbol: default_symbol.to_string(),
            current_timeframe: default_timeframe.to_string(),
            loading: false,
            error: None,
        }
    }

    /// Replace bars for a given key.
    pub fn set_bars(&mut self, symbol: &str, timeframe: &str, new_bars: Vec<Bar>) {
        let key = (symbol.to_string(), timeframe.to_string());
        self.bars.insert(key, new_bars);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_bar(open: f64, close: f64) -> Bar {
        Bar {
            time: Utc::now(),
            open,
            high: open + 1.0,
            low: close - 1.0,
            close,
            volume: 1000.0,
        }
    }

    #[test]
    fn appconfig_empty_toml_falls_back_to_default_symbol() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config.app.default_symbol, "000001");
        assert_eq!(config.app.default_timeframe, "1d");
    }

    #[test]
    fn appconfig_from_toml_overrides_fields() {
        let config: AppConfig = toml::from_str(
            r#"[app]
default_symbol = "600519"
default_timeframe = "1w"
"#,
        )
        .unwrap();
        assert_eq!(config.app.default_symbol, "600519");
        assert_eq!(config.app.default_timeframe, "1w");
    }

    #[test]
    fn database_config_defaults_to_parquet_data() {
        let config = DatabaseConfig::default();
        assert_eq!(config.parquet_dir, "parquet_data");
    }

    #[test]
    fn appconfig_parses_parquet_dir_from_toml() {
        let config: AppConfig = toml::from_str(
            r#"[database]
parquet_dir = "/custom/parquet"
"#,
        )
        .unwrap();
        assert_eq!(config.database.parquet_dir, "/custom/parquet");
    }

    #[test]
    fn set_bars_stores_data() {
        let mut s = CompassState::new("000001", "1d");
        s.set_bars("000001", "1d", vec![make_bar(10.0, 12.0)]);
        assert_eq!(s.bars.len(), 1);
    }

    #[test]
    fn set_bars_overwrites_existing_key() {
        let mut s = CompassState::new("000001", "1d");
        s.set_bars("000001", "1d", vec![make_bar(10.0, 12.0)]);
        s.set_bars("000001", "1d", vec![make_bar(20.0, 22.0)]);
        assert_eq!(s.bars.len(), 1);
    }

    #[test]
    fn set_bars_stores_multiple_symbols() {
        let mut s = CompassState::new("000001", "1d");
        s.set_bars("000001", "1d", vec![make_bar(10.0, 12.0)]);
        s.set_bars("600519", "1d", vec![make_bar(20.0, 22.0)]);
        assert_eq!(s.bars.len(), 2);
    }
}
