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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::{Arc, Mutex};

    // ---- helpers ----

    fn make_bar(day_offset: u32, open: f64, close: f64, volume: f64) -> Bar {
        Bar {
            time: Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                + chrono::Duration::days(day_offset as i64),
            open,
            high: open + 1.0,
            low: close - 1.0,
            close,
            volume,
        }
    }

    fn fetch_all_start() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).unwrap()
    }

    fn fetch_all_end() -> DateTime<Utc> {
        DateTime::from_timestamp(4_000_000_000, 0).unwrap()
    }

    // ================================================================
    // MockRemote — returns predefined bars, tracks call count
    // ================================================================

    struct MockRemote {
        bars: Arc<Mutex<Vec<Bar>>>,
        call_count: Arc<Mutex<usize>>,
    }

    impl MockRemote {
        fn new(bars: Vec<Bar>) -> Self {
            Self {
                bars: Arc::new(Mutex::new(bars)),
                call_count: Arc::new(Mutex::new(0)),
            }
        }

        fn call_count(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl DataProvider for MockRemote {
        async fn fetch_bars(
            &self,
            _symbol: &str,
            _timeframe: &str,
            _range_start: DateTime<Utc>,
            _range_end: DateTime<Utc>,
        ) -> Result<Vec<Bar>, DataError> {
            *self.call_count.lock().unwrap() += 1;
            Ok(self.bars.lock().unwrap().clone())
        }

        async fn search_symbols(&self, _query: &str) -> Result<Vec<SymbolInfo>, DataError> {
            Ok(vec![])
        }
    }

    // ================================================================
    // MockWriter — captures save_bars calls for verification
    // ================================================================

    type SavedCall = (String, String, Vec<Bar>);

    struct MockWriter {
        saved: Arc<Mutex<Vec<SavedCall>>>,
    }

    impl MockWriter {
        fn new() -> Self {
            Self {
                saved: Arc::new(Mutex::new(vec![])),
            }
        }

        fn save_count(&self) -> usize {
            self.saved.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl DataWriter for MockWriter {
        async fn save_bars(
            &self,
            symbol: &str,
            timeframe: &str,
            bars: &[Bar],
        ) -> Result<(), DataError> {
            self.saved.lock().unwrap().push((
                symbol.to_string(),
                timeframe.to_string(),
                bars.to_vec(),
            ));
            Ok(())
        }
    }

    // ================================================================
    // Test: cache hit — data in cache, no remote call
    // ================================================================

    #[tokio::test]
    async fn cache_hit_returns_cached_data_without_remote_call() {
        let cache = sqlite::SqliteProvider::new(":memory:").unwrap();
        let bars = vec![
            make_bar(1, 10.0, 10.5, 1000.0),
            make_bar(2, 10.5, 11.0, 2000.0),
        ];
        cache.save_bars("000001", "1d", &bars).await.unwrap();

        let remote = MockRemote::new(vec![make_bar(99, 99.0, 99.0, 0.0)]);
        let writer = MockWriter::new();
        let provider = CachedProvider::new(remote, cache, writer);

        let result = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].open, 10.0);
        assert_eq!(result[1].open, 10.5);
        assert_eq!(
            provider.reader.call_count(),
            0,
            "remote should not be called on cache hit"
        );
        assert_eq!(provider.writer.save_count(), 0, "no writes on cache hit");
    }

    // ================================================================
    // Test: cache miss — no data in cache → calls remote → saves to cache
    // ================================================================

    #[tokio::test]
    async fn cache_miss_calls_remote_and_saves_to_cache() {
        let cache = sqlite::SqliteProvider::new(":memory:").unwrap();
        let expected_bars = vec![
            make_bar(1, 20.0, 21.0, 3000.0),
            make_bar(2, 21.0, 22.0, 4000.0),
        ];
        let remote = MockRemote::new(expected_bars.clone());
        let writer = MockWriter::new();
        let provider = CachedProvider::new(remote, cache, writer);

        let result = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].open, 20.0);
        assert_eq!(
            provider.reader.call_count(),
            1,
            "remote should be called on cache miss"
        );
        assert_eq!(
            provider.writer.save_count(),
            1,
            "writer.save_bars should be called on cache miss"
        );
    }

    // ================================================================
    // Test: negative cache hit — is_no_data → NoData without remote call
    // ================================================================

    #[tokio::test]
    async fn negative_cache_hit_returns_no_data_without_remote_call() {
        let cache = sqlite::SqliteProvider::new(":memory:").unwrap();
        cache.mark_no_data("000001", "1d").await.unwrap();

        let remote = MockRemote::new(vec![make_bar(1, 10.0, 10.5, 1000.0)]);
        let writer = MockWriter::new();
        let provider = CachedProvider::new(remote, cache, writer);

        let result = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await;

        match result {
            Err(DataError::NoData { symbol }) => {
                assert_eq!(symbol, "000001");
            }
            other => panic!("expected NoData error, got {other:?}"),
        }
        assert_eq!(
            provider.reader.call_count(),
            0,
            "remote should not be called on negative cache hit"
        );
    }

    // ================================================================
    // Test: successful fetch clears inflight dedup
    // ================================================================

    #[tokio::test]
    async fn successful_fetch_clears_inflight_dedup() {
        let cache = sqlite::SqliteProvider::new(":memory:").unwrap();
        let bars = vec![make_bar(1, 30.0, 31.0, 5000.0)];
        let remote = MockRemote::new(bars);
        let writer = MockWriter::new();
        let provider = CachedProvider::new(remote, cache, writer);

        let _ = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();

        assert!(
            provider.inflight.lock().unwrap().is_empty(),
            "inflight should be empty after successful fetch"
        );

        // Second fetch for same key — should succeed (not dedup'd)
        let _ = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();
    }

    // ================================================================
    // Test: empty results from remote — not cached, not marked no-data
    // ================================================================

    #[tokio::test]
    async fn empty_results_from_remote_not_cached_not_marked_no_data() {
        let cache = sqlite::SqliteProvider::new(":memory:").unwrap();
        let remote = MockRemote::new(vec![]);
        let writer = MockWriter::new();
        let provider = CachedProvider::new(remote, cache, writer);

        let result = provider
            .fetch_bars("000001", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();

        assert!(result.is_empty(), "should return empty vec");
        assert_eq!(
            provider.writer.save_count(),
            0,
            "empty results should NOT be saved to cache"
        );
        assert!(
            !provider
                .cache
                .is_no_data("000001", "1d", Utc::now().timestamp(), NO_DATA_TTL_SECS)
                .await
                .unwrap(),
            "empty results should NOT be marked as no-data"
        );
    }
}
