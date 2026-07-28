use std::path::{Path, PathBuf};

use compass_data::import_dolt::{run_dolt_sql_csv, run_dolt_sql_parquet};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

/// Path to the Dolt `investment_data` directory.
fn dolt_dir() -> PathBuf {
    PathBuf::from("/data/compass-data/investment_data")
}

/// Check whether Dolt and investment_data are available at runtime.
fn dolt_available() -> bool {
    let dir = dolt_dir();
    if !dir.exists() {
        eprintln!("SKIP: investment_data/ not found at {:?}", dir);
        return false;
    }
    match std::process::Command::new("dolt").arg("version").output() {
        Ok(out) if out.status.success() => true,
        _ => {
            eprintln!("SKIP: `dolt` not found on PATH");
            false
        }
    }
}

/// Benchmark: `dolt sql -r parquet` for a single symbol (SZ000001).
///
/// Measures the full Dolt query + Parquet serialization round-trip.
/// Skipped if `investment_data/` is missing or `dolt` is not on PATH.
fn bench_dolt_sql_parquet(c: &mut Criterion) {
    if !dolt_available() {
        return;
    }

    let query = "SELECT tradedate, open, high, low, close, adjclose, volume, amount \
                 FROM final_a_stock_eod_price \
                 WHERE symbol = 'SZ000001' ORDER BY tradedate";

    c.bench_function("dolt_sql_parquet_SZ000001", |b| {
        b.iter(|| {
            let result = run_dolt_sql_parquet(&dolt_dir(), black_box(query));
            black_box(result.unwrap());
        })
    });
}

/// Benchmark: write a 300 KB binary blob to a tempfile via `std::fs::write`.
///
/// 300 KB is roughly the Parquet size for one symbol's full OHLCV history.
fn bench_parquet_file_write(c: &mut Criterion) {
    let data: Vec<u8> = (0..300 * 1024).map(|i| (i % 256) as u8).collect();
    let tmp = tempfile::TempDir::new().expect("tempdir");

    c.bench_function("parquet_file_write_300KB", |b| {
        b.iter(|| {
            let path = tmp.path().join("bench.parquet");
            std::fs::write(&path, black_box(&data)).unwrap();
        })
    });
}

/// Benchmark: `dolt sql -r csv` for full symbol enumeration.
///
/// Queries all distinct symbols (~6000+) and counts the result lines
/// to prevent dead-code elimination. Skipped if Dolt is unavailable.
fn bench_symbol_enumeration(c: &mut Criterion) {
    if !dolt_available() {
        return;
    }

    let query = "SELECT DISTINCT symbol FROM final_a_stock_eod_price ORDER BY symbol";

    c.bench_function("dolt_sql_csv_symbol_enumeration", |b| {
        b.iter(|| {
            let result = run_dolt_sql_csv(&dolt_dir(), black_box(query));
            let csv = result.unwrap();
            black_box(csv.lines().count());
        })
    });
}

criterion_group!(
    benches,
    bench_dolt_sql_parquet,
    bench_parquet_file_write,
    bench_symbol_enumeration,
);
criterion_main!(benches);
