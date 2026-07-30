//! Shared data model types.
//!
//! Contains commands, application state, configuration, and stock metadata
//! used by both the GUI and CLI binaries.

use chrono::{DateTime, Utc};
use egui_charts::model::Bar;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Exchange
// ---------------------------------------------------------------------------

/// Stock exchange identifier for A-share markets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    /// No exchange filter — matches all stocks.
    All,
    /// Shanghai Stock Exchange.
    SH,
    /// Shenzhen Stock Exchange.
    SZ,
    /// Beijing Stock Exchange.
    BJ,
}

impl Exchange {
    /// Two-letter exchange code ("SH", "SZ", "BJ") or "" for All.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "",
            Self::SH => "SH",
            Self::SZ => "SZ",
            Self::BJ => "BJ",
        }
    }

    /// Maps a ComboBox index to an Exchange variant. 0=All, 1=SH, 2=SZ, 3=BJ.
    pub fn from_index(idx: usize) -> Self {
        match idx {
            1 => Self::SH,
            2 => Self::SZ,
            3 => Self::BJ,
            _ => Self::All,
        }
    }

    /// Prefixes a bare code with exchange marker (sh./sz./bj.). No prefix for All.
    pub fn prefix_code(&self, code: &str) -> String {
        match self {
            Self::All => code.to_string(),
            Self::SH => format!("sh.{code}"),
            Self::SZ => format!("sz.{code}"),
            Self::BJ => format!("bj.{code}"),
        }
    }

    /// True if the stock's exchange field matches this variant. All always matches.
    pub fn matches(&self, stock: &StockBasic) -> bool {
        match self {
            Self::All => true,
            _ => stock.exchange.as_deref() == Some(self.as_str()),
        }
    }
}

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
/// Fetched from realtime API. All fields are optional — the API
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
    /// Application behavior (default symbol, timeframe).
    pub app: AppSection,
    #[serde(default)]
    /// Parquet directory for stock_basic.parquet and stock_daily/.
    pub parquet: ParquetConfig,
    #[serde(default)]
    /// Dolt data directories for investment_data and compass_data.
    pub dolt: DoltConfig,
    #[serde(default = "default_theme")]
    /// GUI color theme name (e.g. "compass_dark").
    pub theme: String,
}

/// Parquet data directory configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ParquetConfig {
    #[serde(default = "default_parquet_dir")]
    /// Directory containing `stock_basic.parquet` and `stock_daily/` subdirectory.
    pub dir: String,
}

impl Default for ParquetConfig {
    fn default() -> Self {
        Self {
            dir: default_parquet_dir(),
        }
    }
}

fn default_parquet_dir() -> String {
    "/data/compass-data/parquet_data".into()
}

/// Dolt data directories — used by the data pipeline CLI.
#[derive(Debug, Clone, Deserialize)]
pub struct DoltConfig {
    #[serde(default = "default_investment_data_dir")]
    /// Directory for the Dolt `investment_data` repository (primary OHLCV source).
    pub investment_data_dir: String,
    #[serde(default = "default_compass_data_dir")]
    /// Directory for the Dolt `compass_data` repository (fundamentals, custom data).
    pub compass_data_dir: String,
}

impl Default for DoltConfig {
    fn default() -> Self {
        Self {
            investment_data_dir: default_investment_data_dir(),
            compass_data_dir: default_compass_data_dir(),
        }
    }
}

fn default_investment_data_dir() -> String {
    "/data/compass-data/investment_data".into()
}

fn default_compass_data_dir() -> String {
    "/data/compass-data/compass_data".into()
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

impl Default for AppSection {
    fn default() -> Self {
        Self {
            default_symbol: default_symbol(),
            default_timeframe: default_timeframe(),
        }
    }
}

fn default_symbol() -> String {
    "000001".into()
}
fn default_timeframe() -> String {
    "1d".into()
}
fn default_theme() -> String {
    "compass_dark".into()
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
    fn appconfig_parses_parquet_dir_from_toml() {
        let config: AppConfig = toml::from_str(
            r#"[parquet]
dir = "/custom/parquet"
"#,
        )
        .unwrap();
        assert_eq!(config.parquet.dir, "/custom/parquet");
    }

    #[test]
    fn appconfig_theme_defaults_to_compass_dark() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config.theme, "compass_dark");
    }

    #[test]
    fn appconfig_theme_parses_from_toml() {
        let config: AppConfig = toml::from_str("theme = \"custom_sky\"").unwrap();
        assert_eq!(config.theme, "custom_sky");
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

    #[test]
    fn exchange_as_str_returns_correct_codes() {
        assert_eq!(Exchange::SH.as_str(), "SH");
        assert_eq!(Exchange::SZ.as_str(), "SZ");
        assert_eq!(Exchange::BJ.as_str(), "BJ");
        assert_eq!(Exchange::All.as_str(), "");
    }

    #[test]
    fn exchange_from_index_roundtrips() {
        let indices = [
            (0, Exchange::All),
            (1, Exchange::SH),
            (2, Exchange::SZ),
            (3, Exchange::BJ),
        ];
        for (idx, expected) in indices {
            assert_eq!(Exchange::from_index(idx), expected);
        }
    }

    #[test]
    fn exchange_all_does_not_prefix() {
        assert_eq!(Exchange::All.prefix_code("000001"), "000001");
        assert_eq!(Exchange::All.prefix_code("600519"), "600519");
    }

    #[test]
    fn exchange_sh_prefixes_code() {
        assert_eq!(Exchange::SH.prefix_code("000001"), "sh.000001");
        assert_eq!(Exchange::SH.prefix_code("600519"), "sh.600519");
    }

    #[test]
    fn exchange_filter_matches_correct_records() {
        let basic = StockBasic {
            symbol: "000001".into(),
            name: "平安银行".into(),
            area: None,
            industry: None,
            market: None,
            exchange: Some("SZ".into()),
            list_date: None,
            delist_date: None,
        };
        assert!(!Exchange::SH.matches(&basic));
        assert!(Exchange::SZ.matches(&basic));
        assert!(Exchange::All.matches(&basic));
    }

    #[test]
    fn exchange_matches_none_exchange_returns_false_for_specific() {
        let basic = StockBasic {
            symbol: "UNKNOWN".into(),
            name: "未知".into(),
            area: None,
            industry: None,
            market: None,
            exchange: None,
            list_date: None,
            delist_date: None,
        };
        assert!(!Exchange::SH.matches(&basic));
        assert!(!Exchange::SZ.matches(&basic));
        assert!(!Exchange::BJ.matches(&basic));
        assert!(Exchange::All.matches(&basic));
    }

    #[test]
    fn parquet_config_default_dir() {
        let cfg = ParquetConfig::default();
        assert_eq!(cfg.dir, "/data/compass-data/parquet_data");
    }

    #[test]
    fn appconfig_parquet_section_parses_from_toml() {
        let config: AppConfig = toml::from_str(
            r#"[parquet]
dir = "/custom/parquet/path"
"#,
        )
        .unwrap();
        assert_eq!(config.parquet.dir, "/custom/parquet/path");
    }

    #[test]
    fn appconfig_parquet_section_falls_back_to_default() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config.parquet.dir, "/data/compass-data/parquet_data");
    }

    #[test]
    fn dolt_config_default_values() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(
            config.dolt.investment_data_dir,
            "/data/compass-data/investment_data"
        );
        assert_eq!(
            config.dolt.compass_data_dir,
            "/data/compass-data/compass_data"
        );
    }

    #[test]
    fn dolt_config_overrides_from_toml() {
        let config: AppConfig = toml::from_str(
            r#"[dolt]
investment_data_dir = "/custom/investment"
compass_data_dir = "/custom/compass"
"#,
        )
        .unwrap();
        assert_eq!(config.dolt.investment_data_dir, "/custom/investment");
        assert_eq!(config.dolt.compass_data_dir, "/custom/compass");
    }
}
