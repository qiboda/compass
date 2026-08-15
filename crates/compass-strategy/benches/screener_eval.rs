//! Criterion benchmarks for the screener Filter AST evaluator (issue #246).
//!
//! Benchmarks:
//! - `representative_filter` — whole-market run with a mixed Meta+Series
//!   filter (legacy-expressible shape: Industry + MarketCap + Close>Sma20 +
//!   Close>NDayHigh(60))
//! - `empty_filter`          — whole-market scan with an empty `And` (no
//!   constraints; measures the raw per-symbol pass cost)
//!
//! The fixture is a deterministic synthetic market: 6000 symbols × 400 daily
//! bars (seeded RNG, fixed seed so runs are comparable). Benchmark this
//! branch and the pre-migration commit (git worktree at `a1dbcad`, before
//! Batch 3) to prove the evaluator stays within the same order of magnitude.

use chrono::{Datelike, Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::run_screener;
use compass_types::{CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor};
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use duckdb::Connection;
use rand::{Rng, SeedableRng, rngs::StdRng};

const SYMBOLS: usize = 6000;
const BARS_PER_SYMBOL: usize = 400;

/// Deterministic daily bars for `symbols` symbols ending `now`, with a mild
/// rising trend per symbol (seeded so every run is identical).
type StockTuple = (String, String, f64, f64, String);

fn synthetic_market() -> (tempfile::TempDir, Vec<StockTuple>) {
    let mut rng = StdRng::seed_from_u64(42);
    let now = NaiveDate::from_ymd_opt(2026, 7, 28).expect("date");

    let mut stocks: Vec<StockTuple> = Vec::new();
    for s in 0..SYMBOLS {
        let exchange = if s % 3 == 0 { "SH" } else { "SZ" };
        let symbol = format!("{exchange}{:06}", 1 + s);
        let name = format!("股票{s}");
        let industry = if s % 5 == 0 { "白酒" } else { "银行" };
        // Market cap = total_share × close / 1e8; pick shares so caps spread
        // across 0.1亿..3000亿 (as 亿).
        let total_share = (s as f64 + 1.0) * 1.0e7;
        stocks.push((symbol, name, total_share, 0.0, industry.to_string()));
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let conn = Connection::open_in_memory().expect("duckdb");
    conn.execute_batch(
        "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
    )
    .expect("create daily");
    conn.execute_batch(
        "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
    )
    .expect("create basic");

    let day = now;
    for (s, stock) in stocks.iter().enumerate() {
        let (symbol, name, total_share, _, industry) = stock;
        // Deterministic per-symbol walk: trend + noise.
        let trend = 10.0 + (s % 7) as f64 * 0.5;
        let mut price = trend;
        let mut prev = day;
        for _ in 0..BARS_PER_SYMBOL {
            while matches!(prev.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
                prev -= Duration::days(1);
            }
            let drift = (s as f64 % 3.0 - 1.0) * 0.05; // -0.05..0.05 per bar
            let noise = rng.random_range(-0.2..0.2);
            price = (price + drift + noise).max(1.0);
            let volume = rng.random_range(1.0e5..5.0e6);
            conn.execute(
                "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    symbol,
                    prev.format("%Y-%m-%d").to_string(),
                    price - 0.1,
                    price + 0.2,
                    price - 0.2,
                    price,
                    price,
                    volume,
                    0.0
                ],
            )
            .expect("insert daily");
            prev -= Duration::days(1);
        }
        conn.execute(
            "INSERT INTO basic VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)",
            duckdb::params![
                symbol,
                name,
                "2001-01-01",
                None::<String>,
                "主板",
                name,
                total_share,
                industry,
            ],
        )
        .expect("insert basic");
    }

    conn.execute_batch(&format!(
        "COPY daily TO '{}' (FORMAT PARQUET)",
        tmp.path().join("stock_daily.parquet").display()
    ))
    .expect("copy daily");
    conn.execute_batch(&format!(
        "COPY basic TO '{}' (FORMAT PARQUET)",
        tmp.path().join("stock_basic.parquet").display()
    ))
    .expect("copy basic");

    (tmp, stocks)
}

/// A legacy-expressible mixed filter: Industry + MarketCap + Close>Sma20 +
/// Close>NDayHigh(60) — every node is inside the pre-Batch-3 accept-grammar,
/// so the baseline commit's `filter_to_query` path can run it too.
fn representative_filter() -> Filter {
    Filter::And(vec![
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        Filter::Meta(MetaCond::MarketCap {
            min: Some(100.0),
            max: None,
        }),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        }),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(60)),
        }),
    ])
}

fn bench_run_screener(c: &mut Criterion) {
    let (tmp, _stocks) = synthetic_market();
    let reader = ParquetReader::new(tmp.path()).expect("reader");
    let now = NaiveDate::from_ymd_opt(2026, 7, 28).expect("date");

    let filters = [
        ("empty_filter", Filter::And(Vec::new())),
        ("representative_filter", representative_filter()),
    ];
    for (name, filter) in filters {
        c.bench_function(name, |b| {
            b.iter_batched(
                || filter.clone(),
                |f| {
                    black_box(
                        run_screener(&f, &reader, now)
                            .expect("run must succeed")
                            .rows
                            .len(),
                    )
                },
                BatchSize::SmallInput,
            );
        });
    }
}

criterion_group!(screener, bench_run_screener);
criterion_main!(screener);
