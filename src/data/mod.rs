pub mod eastmoney;
pub mod provider;
pub mod sqlite;
mod synthetic;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use egui_charts::model::Bar;
use tracing::instrument;

use crate::model::SymbolInfo;
use provider::{DataError, DataProvider, DataWriter};

// ---------------------------------------------------------------------------
// CachedProvider — reader-first, cache fallback
// ---------------------------------------------------------------------------

/// A `DataProvider` that first checks a cache (`DataWriter` + implicit reader),
/// and falls back to a remote reader, writing fetched data into the cache.
pub struct CachedProvider<R: DataProvider, W: DataWriter> {
    /// Remote data source (e.g. EastMoney).
    reader: R,
    /// Local persistent cache (e.g. SQLite, also implements DataProvider).
    cache: sqlite::SqliteProvider,
    /// Write-through destination (same backing store as cache).
    writer: W,
}

impl<R: DataProvider, W: DataWriter> CachedProvider<R, W> {
    pub fn new(reader: R, cache: sqlite::SqliteProvider, writer: W) -> Self {
        Self {
            reader,
            cache,
            writer,
        }
    }
}

#[async_trait]
impl<R: DataProvider, W: DataWriter> DataProvider for CachedProvider<R, W> {
    #[instrument(skip(self), fields(symbol = %symbol, timeframe = %timeframe))]
    async fn fetch_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        // 1. Try cache first.
        if let Ok(bars) = self
            .cache
            .fetch_bars(symbol, timeframe, range_start, range_end)
            .await
            && !bars.is_empty()
        {
            tracing::debug!(count = bars.len(), "cache hit");
            return Ok(bars);
        }

        // 2. Fetch from remote.
        tracing::info!("cache miss, fetching from remote");
        let bars = self
            .reader
            .fetch_bars(symbol, timeframe, range_start, range_end)
            .await?;

        // 3. Persist to cache.
        if !bars.is_empty() {
            self.writer.save_bars(symbol, timeframe, &bars).await?;
            tracing::debug!(count = bars.len(), "cached to local storage");
        }

        Ok(bars)
    }

    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        self.reader.search_symbols(query).await
    }
}
