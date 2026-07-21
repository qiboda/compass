pub mod eastmoney;
pub mod provider;
pub mod sqlite;
mod synthetic;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use egui_charts::model::Bar;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tracing::instrument;

use crate::model::SymbolInfo;
use provider::{DataError, DataProvider, DataWriter};

/// TTL for negative cache entries (7 days in seconds).
const NO_DATA_TTL_SECS: i64 = 7 * 24 * 3600;

// ---------------------------------------------------------------------------
// CachedProvider — reader-first, cache fallback
// ---------------------------------------------------------------------------

pub struct CachedProvider<R: DataProvider, W: DataWriter> {
    reader: R,
    cache: sqlite::SqliteProvider,
    writer: W,
    inflight: Arc<Mutex<HashSet<(String, String)>>>,
}

impl<R: DataProvider, W: DataWriter> CachedProvider<R, W> {
    pub fn new(reader: R, cache: sqlite::SqliteProvider, writer: W) -> Self {
        Self {
            reader,
            cache,
            writer,
            inflight: Arc::new(Mutex::new(HashSet::new())),
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
        let key = (symbol.to_string(), timeframe.to_string());
        let now_ts = Utc::now().timestamp();

        if self
            .cache
            .is_no_data(symbol, timeframe, now_ts, NO_DATA_TTL_SECS)
            .await
            .unwrap_or(false)
        {
            tracing::debug!(%symbol, %timeframe, "negative cache hit, skipping fetch");
            return Err(DataError::NoData {
                symbol: symbol.to_string(),
            });
        }

        {
            let mut inflight = self.inflight.lock().unwrap();
            if inflight.contains(&key) {
                tracing::debug!(%symbol, %timeframe, "in-flight dedup, skipping");
                return Err(DataError::NoData {
                    symbol: symbol.to_string(),
                });
            }
            inflight.insert(key.clone());
        }

        if let Ok(bars) = self
            .cache
            .fetch_bars(symbol, timeframe, range_start, range_end)
            .await
            && !bars.is_empty()
        {
            self.inflight.lock().unwrap().remove(&key);
            tracing::debug!(count = bars.len(), "cache hit");
            return Ok(bars);
        }

        tracing::info!("cache miss, fetching from remote");
        let result = self
            .reader
            .fetch_bars(symbol, timeframe, range_start, range_end)
            .await;

        match &result {
            Ok(bars) if !bars.is_empty() => {
                if let Err(e) = self.writer.save_bars(symbol, timeframe, bars).await {
                    tracing::warn!(error = %e, "failed to persist bars to cache");
                }
                tracing::debug!(count = bars.len(), "cached to local storage");
            }
            Err(DataError::NoData { .. }) => {
                if let Err(e) = self.cache.mark_no_data(symbol, timeframe).await {
                    tracing::warn!(error = %e, "failed to mark negative cache");
                }
                tracing::debug!(%symbol, %timeframe, "marked as no-data (TTL 7d)");
            }
            _ => {}
        }

        self.inflight.lock().unwrap().remove(&key);
        result
    }

    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        self.reader.search_symbols(query).await
    }
}
