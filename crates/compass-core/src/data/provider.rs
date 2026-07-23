use async_trait::async_trait;
use chrono::{DateTime, Utc};
use egui_charts::model::Bar;

use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// Error type — shared across all providers
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),

    #[error("database: {0}")]
    Database(#[from] duckdb::Error),

    #[error("parse: {0}")]
    Parse(String),

    #[error("rate limited, retry after {0}s")]
    #[allow(dead_code)]
    RateLimited(u64),

    #[error("no data for {symbol}")]
    NoData { symbol: String },
}

// ---------------------------------------------------------------------------
// DataProvider — read-only data source
// ---------------------------------------------------------------------------

#[async_trait]
pub trait DataProvider: Send + Sync {
    /// Fetch OHLCV bars for a symbol, timeframe, and date range.
    async fn fetch_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError>;

    /// Search for symbols matching a query string.
    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError>;
}

// ---------------------------------------------------------------------------
// DataWriter — write-through cache interface
// ---------------------------------------------------------------------------

#[async_trait]
pub trait DataWriter: Send + Sync {
    /// Persist a batch of bars to storage.
    ///
    /// When `overwrite` is false, existing rows are skipped (migration-style).
    /// When true, existing rows are replaced.
    async fn save_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        bars: &[Bar],
        overwrite: bool,
    ) -> Result<(), DataError>;
}

// ---------------------------------------------------------------------------
// NegativeCache — mark/fetch negative cache entries
// ---------------------------------------------------------------------------

#[async_trait]
pub trait NegativeCache: Send + Sync {
    /// Mark a (symbol, timeframe) as having no data, with a current timestamp.
    async fn mark_no_data(&self, symbol: &str, timeframe: &str) -> Result<(), DataError>;

    /// Check whether a (symbol, timeframe) has a fresh no-data mark (within TTL seconds).
    async fn is_no_data(
        &self,
        symbol: &str,
        timeframe: &str,
        now_ts: i64,
        ttl_secs: i64,
    ) -> Result<bool, DataError>;
}
