//! Data provider trait system and error types.
//!
//! Defines the three core abstractions — [`DataProvider`], [`DataWriter`],
//! [`NegativeCache`] — that all data backends implement.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use egui_charts::model::Bar;

use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// Error type — shared across all providers
// ---------------------------------------------------------------------------

/// Unified error type for all data providers.
///
/// Distinguishes between network failures, database errors, parse issues,
/// rate limiting, and genuinely missing data. Implements `From` for
/// `reqwest::Error` and `duckdb::Error` for ergonomic `?` propagation.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    /// HTTP transport failure (DNS, timeout, connection refused).
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),

    /// DuckDB query or connection failure.
    #[error("database: {0}")]
    Database(#[from] duckdb::Error),

    /// Data parsing failed. Contains the raw string that could not be parsed.
    #[error("parse: {0}")]
    Parse(String),

    /// API rate limit hit. Contains retry-after seconds.
    #[error("rate limited, retry after {0}s")]
    #[allow(dead_code)]
    RateLimited(u64),

    /// The symbol has no data (delisted, invalid, or API returned null).
    #[error("no data for {symbol}")]
    NoData {
        /// The symbol that returned no data.
        symbol: String,
    },
}

// ---------------------------------------------------------------------------
// DataProvider — read-only data source
// ---------------------------------------------------------------------------

/// Read-only access to stock market data.
///
/// Implementors fetch OHLCV bars and search symbols. This is the core
/// abstraction that lets Compass swap between DuckDB, Parquet,
/// and synthetic data without changing consumer code.
#[async_trait]
pub trait DataProvider: Send + Sync {
    /// Fetch OHLCV bars for a symbol, timeframe, and date range. Bars are
    /// **forward-adjusted** (前复权): OHLC is scaled by
    /// `factor_i = adjclose_i / close_i`, so the latest bar's price equals the
    /// current market price.
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

/// Write-through persistence for fetched bars.
///
/// Called by the data pipeline after a cache miss to persist data locally.
/// existing rows are skipped (`false`, migration-style) or replaced (`true`).
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

/// TTL-managed cache for symbols known to have no data.
///
/// Prevents repeated HTTP calls for delisted or invalid symbols by marking
/// them as no-data with a timestamp. Queries check freshness against a
/// caller-supplied TTL before deciding to retry.
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

#[cfg(test)]
mod tests {
    use super::*;

    // #222 i18n contract (j): the underlying DataError Display is NOT
    // translated — it stays ASCII technical English while the GUI error.*
    // templates translate the surrounding text (passing through %{e}).
    // Already-true invariant; guards against a future locale-aware Display.
    #[test]
    fn data_error_display_stays_ascii_english() {
        for e in [
            DataError::NoData {
                symbol: "SH600519".into(),
            },
            DataError::Parse("bad csv".into()),
            DataError::RateLimited(42),
        ] {
            let s = e.to_string();
            assert!(
                s.chars().all(|c| c.is_ascii()),
                "DataError Display must stay ASCII English, got: {s}"
            );
        }
        assert_eq!(
            DataError::NoData {
                symbol: "SH600519".into()
            }
            .to_string(),
            "no data for SH600519"
        );
    }
}
