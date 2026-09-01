//! Criterion benchmarks for ParquetReader::fetch_bars_blocking.
//!
//! Benchmarks cold-read (fresh ParquetReader per iteration), warm-read
//! (reused ParquetReader across iterations), and real data (SZ000001
//! from the production parquet_data/stock_daily.parquet if available).

use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use compass_core::data::parquet::ParquetReader;
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use duckdb::Connection;
use rand::Rng;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return a date range wide enough to cover all synthetic and real data.
fn wide_date_range() -> (DateTime<Utc>, DateTime<Utc>) {
    let to_dt = |y: i32, m: u32, d: u32| -> DateTime<Utc> {
        DateTime::from_naive_utc_and_offset(
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            Utc,
        )
    };
    (to_dt(2020, 1, 1), to_dt(2030, 12, 31))
}

/// Create a temp directory with a single synthetic `stock_daily.parquet` for SZ000001.
///
/// Generates `rows` sequential daily bars starting from 2024-01-01 with
/// random price movement around 10.0 and random volume in [1k, 100k).
fn create_synthetic_parquet_dir(rows: usize) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let daily_path = tmp.path().join("stock_daily.parquet");

    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE t (\
            symbol VARCHAR, \
            tradedate DATE, \
            open DOUBLE, \
            high DOUBLE, \
            low DOUBLE, \
            close DOUBLE, \
            volume DOUBLE\
        )",
    )
    .unwrap();

    let mut rng = rand::rng();
    let base_date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
    let mut price = 10.0;

    for i in 0..rows {
        let date = base_date + chrono::Duration::days(i as i64);
        let date_str = date.format("%Y-%m-%d").to_string();
        let open: f64 = price + rng.random_range(-0.5..0.5);
        let close: f64 = price + rng.random_range(-1.0..1.0);
        let high: f64 = open.max(close) + rng.random_range(0.0..0.5);
        let low: f64 = open.min(close) - rng.random_range(0.0..0.5);
        let volume = rng.random_range(1000.0..100_000.0);
        price = close;

        conn.execute(
            "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?)",
            duckdb::params!["SZ000001", date_str, open, high, low, close, volume],
        )
        .unwrap();
    }

    conn.execute_batch(&format!(
        "COPY t TO '{}' (FORMAT PARQUET)",
        daily_path.display()
    ))
    .unwrap();

    std::fs::write(tmp.path().join("stock_daily.symbols.txt"), "SZ000001\n").unwrap();

    tmp
}

// ---------------------------------------------------------------------------
// Cold read
// ---------------------------------------------------------------------------

/// Cold read: fresh ParquetReader (new DuckDB in-memory connection) for
/// each iteration. Measures connection creation + first `read_parquet()`
/// query cost.
fn bench_cold_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_read");
    let (start, end) = wide_date_range();

    for &rows in &[100, 1000, 5000] {
        let tmp = create_synthetic_parquet_dir(rows);
        let dir_path = tmp.path().to_path_buf();

        group.bench_function(format!("{}_rows", rows), |b| {
            b.iter_batched(
                || ParquetReader::new(&dir_path).unwrap(),
                |reader| {
                    let bars = reader
                        .fetch_bars_blocking("SZ000001", start, end, "qfq")
                        .unwrap();
                    black_box(bars);
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Warm read
// ---------------------------------------------------------------------------

/// Warm read: reuse the same ParquetReader across all iterations.
/// Measures the cost of repeated queries on a warm DuckDB connection
/// (query-plan caching, mutex acquisition, row materialization).
fn bench_warm_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("warm_read");
    let (start, end) = wide_date_range();

    for &rows in &[100, 1000, 5000] {
        let tmp = create_synthetic_parquet_dir(rows);
        let reader = ParquetReader::new(tmp.path()).unwrap();

        group.bench_function(format!("{}_rows", rows), |b| {
            b.iter(|| {
                let bars = reader
                    .fetch_bars_blocking("SZ000001", start, end, "qfq")
                    .unwrap();
                black_box(bars);
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Real data
// ---------------------------------------------------------------------------

/// Benchmark against the production SZ000001 dataset (warm read).
/// Skipped gracefully when `parquet_data/` is not present.
fn bench_real_data(c: &mut Criterion) {
    let real_dir = Path::new("../../parquet_data");
    let test_file = real_dir.join("stock_daily.parquet");

    if !test_file.exists() {
        eprintln!("Skipping real data benchmark: {:?} not found", test_file);
        return;
    }

    let reader = ParquetReader::new(real_dir).unwrap();
    let (start, end) = wide_date_range();

    let mut group = c.benchmark_group("real_data");
    group.bench_function("SZ000001", |b| {
        b.iter(|| {
            let bars = reader
                .fetch_bars_blocking("SZ000001", start, end, "qfq")
                .unwrap();
            black_box(bars);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_cold_read, bench_warm_read, bench_real_data);
criterion_main!(benches);
