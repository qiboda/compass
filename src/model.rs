use chrono::{DateTime, Utc};
use egui_charts::model::Bar;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Symbol identifiers
// ---------------------------------------------------------------------------

/// Info returned by symbol search (code + display name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub code: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// App command (UI → worker thread)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Cmd {
    /// Fetch OHLCV bars for a symbol/timeframe/date-range.
    FetchBars {
        symbol: String,
        timeframe: String,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    },
    /// Search symbols by keyword.
    #[allow(dead_code)]
    SearchSymbols { query: String },
}

// ---------------------------------------------------------------------------
// Application config (loaded from ~/.config/compass/config.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub app: AppSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_retry_count")]
    #[allow(dead_code)]
    pub retry_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppSection {
    #[serde(default = "default_symbol")]
    pub default_symbol: String,
    #[serde(default = "default_timeframe")]
    pub default_timeframe: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            timeout_secs: default_timeout_secs(),
            retry_count: default_retry_count(),
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

fn default_db_path() -> String {
    "compass.db".into()
}
fn default_base_url() -> String {
    "https://push2his.eastmoney.com".into()
}
fn default_timeout_secs() -> u64 {
    10
}
fn default_retry_count() -> u32 {
    3
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

pub struct CompassState {
    /// All loaded OHLCV bars, keyed by (symbol, timeframe).
    pub bars: BarsMap,
    /// Currently viewed symbol.
    pub current_symbol: String,
    /// Currently viewed timeframe.
    pub current_timeframe: String,
    /// True while a fetch is in-flight.
    pub loading: bool,
    /// Search results (symbol list).
    pub search_results: Vec<SymbolInfo>,
    /// Last error message, if any.
    pub error: Option<String>,
    /// Incremented every time bars data changes (so UI knows to rebuild chart).
    pub bars_version: u64,
}

impl CompassState {
    pub fn new(default_symbol: &str, default_timeframe: &str) -> Self {
        Self {
            bars: BarsMap::new(),
            current_symbol: default_symbol.to_string(),
            current_timeframe: default_timeframe.to_string(),
            loading: false,
            search_results: Vec::new(),
            error: None,
            bars_version: 0,
        }
    }

    /// Replace bars for a given key and bump version.
    pub fn set_bars(&mut self, symbol: &str, timeframe: &str, new_bars: Vec<Bar>) {
        let key = (symbol.to_string(), timeframe.to_string());
        self.bars.insert(key, new_bars);
        self.bars_version = self.bars_version.wrapping_add(1);
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
    fn set_bars_stores_and_bumps_version() {
        let mut s = CompassState::new("000001", "1d");
        s.set_bars("000001", "1d", vec![make_bar(10.0, 12.0)]);
        assert_eq!(s.bars_version, 1);
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
    fn set_bars_version_wraps() {
        let mut s = CompassState::new("000001", "1d");
        s.bars_version = u64::MAX;
        s.set_bars("000001", "1d", vec![make_bar(1.0, 2.0)]);
        assert_eq!(s.bars_version, 0);
    }

    #[test]
    fn set_bars_stores_multiple_symbols() {
        let mut s = CompassState::new("000001", "1d");
        s.set_bars("000001", "1d", vec![make_bar(10.0, 12.0)]);
        s.set_bars("600519", "1d", vec![make_bar(20.0, 22.0)]);
        assert_eq!(s.bars.len(), 2);
    }
}
