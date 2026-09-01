//! Criterion benchmarks for DuckDbProvider — in-memory DuckDB read/write performance.
//!
//! Benchmarks:
//! - `cache_hit`        — fetch_bars for pre-populated symbol (spawn_blocking + query)
//! - `cache_miss`       — fetch_bars for non-existent symbol (empty result path)
//! - `save_10/100/1000/5000_rows` — save_bars write throughput at varying sizes

use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use egui_charts::model::Bar;

use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::provider::{DataProvider, DataWriter};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fixed `DateTime<Utc>` from year/month/day at midnight.
fn dt_utc(year: i32, month: u32, day: u32) -> DateTime<Utc> {
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("invalid date")
        .and_hms_opt(0, 0, 0)
        .expect("invalid time")
        .and_utc()
}

/// Generate `n` synthetic OHLCV bars starting at 2024-01-01, one per day.
fn make_bars(n: usize) -> Vec<Bar> {
    let base = dt_utc(2024, 1, 1);
    (0..n)
        .map(|i| {
            let time = base + chrono::Duration::days(i as i64);
            let open = 10.0 + (i as f64) * 0.01;
            Bar::new(
                time,
                open,                      // open
                open + 2.0,                // high
                open - 0.5,                // low
                open + 1.0,                // close
                1000.0 * (i as f64 + 1.0), // volume
            )
        })
        .collect()
}

/// Date range wide enough to cover any synthetic data (2000–2100).
fn wide_range() -> (DateTime<Utc>, DateTime<Utc>) {
    (dt_utc(2000, 1, 1), dt_utc(2100, 1, 1))
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

/// Cache hit: pre-populate in-memory DB, then repeatedly query the same symbol.
fn bench_cache_hit(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let provider =
        rt.block_on(async { DuckDbProvider::new_in_memory().expect("failed to open DuckDB") });

    // Populate 500 rows once before the timing loop.
    let bars = make_bars(500);
    rt.block_on(async {
        provider
            .save_bars("000001.SZ", "1d", &bars, true)
            .await
            .expect("save_bars failed")
    });

    let (start, end) = wide_range();

    let mut group = c.benchmark_group("duckdb_cache_hit");
    group.measurement_time(Duration::from_secs(10));
    group.bench_function("fetch_bars_500_rows", |b| {
        b.iter(|| {
            let result = rt.block_on(async {
                provider
                    .fetch_bars("000001.SZ", "1d", start, end, "qfq")
                    .await
                    .expect("fetch_bars failed")
            });
            black_box(result);
        });
    });
    group.finish();
}

/// Cache miss: query a symbol that has never been inserted — measures empty-result path.
fn bench_cache_miss(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let provider =
        rt.block_on(async { DuckDbProvider::new_in_memory().expect("failed to open DuckDB") });

    let (start, end) = wide_range();

    c.bench_function("duckdb_cache_miss", |b| {
        b.iter(|| {
            let result = rt.block_on(async {
                provider
                    .fetch_bars("NONEXIST", "1d", start, end, "qfq")
                    .await
                    .expect("fetch_bars failed")
            });
            black_box(result);
        });
    });
}

/// Save throughput: insert N rows into a fresh in-memory DB.
fn bench_save_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("duckdb_save");

    for size in [10usize, 100, 1000, 5000] {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        let bars = make_bars(size);

        group.bench_function(format!("save_{size}_rows"), |b| {
            b.iter(|| {
                // Fresh DB each iteration to avoid INSERT OR REPLACE semantics
                // on an already-populated table.
                let provider = rt.block_on(async {
                    DuckDbProvider::new_in_memory().expect("failed to open DuckDB")
                });
                rt.block_on(async {
                    provider
                        .save_bars("600519.SH", "1d", &bars, true)
                        .await
                        .expect("save_bars failed")
                });
                black_box(());
            });
        });
    }

    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().significance_level(0.1).sample_size(100);
    targets = bench_cache_hit, bench_cache_miss, bench_save_throughput
);
criterion_main!(benches);
