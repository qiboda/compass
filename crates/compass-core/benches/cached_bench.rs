//! Criterion benchmarks for [`CachedProvider`] — cache hit, cache miss,
//! and negative‑cache hit scenarios.
//!
//! Uses in‑memory DuckDB (no filesystem) and a mock remote provider (no HTTP).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use criterion::{Criterion, criterion_group, criterion_main};
use egui_charts::model::Bar;

use compass_core::data::CachedProvider;
use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::provider::{DataError, DataProvider, DataWriter, NegativeCache};
use compass_core::model::SymbolInfo;

// ---------------------------------------------------------------------------
// helpers — synthetic bars & date ranges
// ---------------------------------------------------------------------------

/// Build `count` synthetic daily bars spaced 1 day apart, epoch‑aligned.
fn make_bars(count: u32) -> Vec<Bar> {
    let base = 1_000_000_000; // 2001-09-09T01:46:40Z — a valid epoch
    (0..count)
        .map(|i| Bar {
            time: DateTime::from_timestamp(base + i as i64 * 86400, 0).unwrap(),
            open: 10.0 + i as f64 * 0.1,
            high: 10.5 + i as f64 * 0.1,
            low: 9.8 + i as f64 * 0.1,
            close: 10.2 + i as f64 * 0.1,
            volume: 1_000_000.0 + i as f64 * 1000.0,
        })
        .collect()
}

/// Unlimited date range that covers all epoch‑based bars.
fn wide_range() -> (DateTime<Utc>, DateTime<Utc>) {
    (
        DateTime::from_timestamp(0, 0).unwrap(),
        DateTime::from_timestamp(4_000_000_000, 0).unwrap(),
    )
}

// ---------------------------------------------------------------------------
// MockRemote — returns predefined bars without touching the network
// ---------------------------------------------------------------------------

struct MockRemote {
    bars: Vec<Bar>,
}

impl MockRemote {
    fn new(bars: Vec<Bar>) -> Self {
        Self { bars }
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
        Ok(self.bars.clone())
    }

    async fn search_symbols(&self, _query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// EmptyCache — stateless cache that always misses (for cache-miss benches)
// ---------------------------------------------------------------------------

/// A lightweight, zero‑overhead cache that always returns empty results.
/// Useful for isolating the read‑through path cost.
struct EmptyCache;

#[async_trait]
impl DataProvider for EmptyCache {
    async fn fetch_bars(
        &self,
        _symbol: &str,
        _timeframe: &str,
        _range_start: DateTime<Utc>,
        _range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        Ok(vec![])
    }

    async fn search_symbols(&self, _query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        Ok(vec![])
    }
}

#[async_trait]
impl DataWriter for EmptyCache {
    async fn save_bars(
        &self,
        _symbol: &str,
        _timeframe: &str,
        _bars: &[Bar],
        _overwrite: bool,
    ) -> Result<(), DataError> {
        Ok(())
    }
}

#[async_trait]
impl NegativeCache for EmptyCache {
    async fn mark_no_data(&self, _symbol: &str, _timeframe: &str) -> Result<(), DataError> {
        Ok(())
    }

    async fn is_no_data(
        &self,
        _symbol: &str,
        _timeframe: &str,
        _now_ts: i64,
        _ttl_secs: i64,
    ) -> Result<bool, DataError> {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// benchmarks
// ---------------------------------------------------------------------------

fn bench_cached(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (start, end) = wide_range();

    // ------------------------------------------------------------------
    // 1. Cache hit — pre‑populate DuckDB, measure fetch latency
    // ------------------------------------------------------------------

    {
        let bars = make_bars(100);
        let cache = DuckDbProvider::new_in_memory().unwrap();

        // Pre‑populate the cache so every fetch hits.
        rt.block_on(async {
            cache.save_bars("000001", "1d", &bars, true).await.unwrap();
        });

        let reader = MockRemote::new(vec![]);
        let provider = CachedProvider::new(reader, cache);

        c.bench_function("cached_hit", |b| {
            b.iter(|| {
                rt.block_on(async {
                    provider
                        .fetch_bars("000001", "1d", start, end)
                        .await
                        .unwrap();
                });
            });
        });
    }

    // ------------------------------------------------------------------
    // 2. Cache miss — empty cache forces read‑through to MockRemote
    // ------------------------------------------------------------------

    {
        let bars = make_bars(100);
        // Use EmptyCache so every iteration is guaranteed a miss.
        let reader = MockRemote::new(bars);
        let provider = CachedProvider::new(reader, EmptyCache);

        c.bench_function("cached_miss", |b| {
            b.iter(|| {
                rt.block_on(async {
                    provider
                        .fetch_bars("000001", "1d", start, end)
                        .await
                        .unwrap();
                });
            });
        });
    }

    // ------------------------------------------------------------------
    // 3. Negative cache hit — mark no‑data, measure early return
    // ------------------------------------------------------------------

    {
        let cache = DuckDbProvider::new_in_memory().unwrap();

        rt.block_on(async {
            cache.mark_no_data("000001", "1d").await.unwrap();
        });

        let reader = MockRemote::new(make_bars(100));
        let provider = CachedProvider::new(reader, cache);

        c.bench_function("cached_negative", |b| {
            b.iter(|| {
                rt.block_on(async {
                    match provider.fetch_bars("000001", "1d", start, end).await {
                        Err(DataError::NoData { .. }) => {}
                        other => panic!("expected NoData error, got {other:?}"),
                    }
                });
            });
        });
    }
}

criterion_group!(benches, bench_cached);
criterion_main!(benches);
