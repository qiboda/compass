//! Requirement-acceptance tests for C4 (epic #255, plan T5): the monthly
//! (1M) aggregation path over the dual-parquet fallback.
//!
//! The adversarial suite (`index_duckdb_fallback.rs`) proves the 1d daily
//! fallback, the 1w weekly aggregate, collision precedence and concurrency.
//! This file proves the remaining declared contract — **1M aggregation over
//! index data** (plan T5 QA: "1w/1M 聚合对 index 正确（SUM volume）") — plus
//! the regression guard that the stock path's 1M aggregation is unchanged.
//!
//! RED vs current code: the fallback only knows `stock_daily.parquet`, so an
//! index symbol resolves to zero bars (assertions below fail).

use std::path::Path;

use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::provider::DataProvider;
use duckdb::Connection;

fn epoch_start() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(0, 0).expect("valid epoch")
}

fn epoch_end() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(4_000_000_000, 0).expect("valid end")
}

/// Write `index_daily.parquet` with the plan's export column layout
/// (symbol, index_type, tradedate, open, high, low, close, volume, amount,
/// adjclose) — `adjclose = close` so the forward-adjustment factor is 1.0.
fn write_index_daily_parquet(dir: &Path, symbol: &str, days: &[(&str, f64, f64)]) {
    let conn = Connection::open_in_memory().expect("duckdb");
    conn.execute_batch(
        "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, \
         open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, \
         amount DOUBLE, adjclose DOUBLE)",
    )
    .expect("create");
    for (day, close, volume) in days {
        conn.execute(
            "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                symbol,
                "official",
                day,
                close - 1.0,
                close + 1.0,
                close - 2.0,
                close,
                volume,
                0.0,
                close
            ],
        )
        .expect("insert");
    }
    conn.execute_batch(&format!(
        "COPY t TO '{}' (FORMAT PARQUET)",
        dir.join("index_daily.parquet").display()
    ))
    .expect("copy");
}

/// Write `stock_daily.parquet` with the canonical 9-column stock layout.
fn write_stock_daily_parquet(dir: &Path, symbol: &str, days: &[(&str, f64, f64)]) {
    let conn = Connection::open_in_memory().expect("duckdb");
    conn.execute_batch(
        "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, \
         low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
    )
    .expect("create");
    for (day, close, volume) in days {
        conn.execute(
            "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                symbol,
                day,
                close - 1.0,
                close + 1.0,
                close - 2.0,
                close,
                close,
                volume,
                0.0
            ],
        )
        .expect("insert");
    }
    conn.execute_batch(&format!(
        "COPY t TO '{}' (FORMAT PARQUET)",
        dir.join("stock_daily.parquet").display()
    ))
    .expect("copy");
}

// ---------------------------------------------------------------------------
// 1M aggregation over index data
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_bars_monthly_aggregates_index_sum_volume() {
    // Three calendar months: June (1 bar), July (2 bars), August (1 bar).
    let tmp = tempfile::tempdir().expect("tempdir");
    write_index_daily_parquet(
        tmp.path(),
        "SH000001",
        &[
            ("2026-06-30", 3000.0, 1000.0),
            ("2026-07-01", 3010.0, 1000.0),
            ("2026-07-31", 3020.0, 2000.0),
            ("2026-08-31", 3030.0, 500.0),
        ],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("SH000001", "1M", epoch_start(), epoch_end())
        .await
        .expect("fetch_bars 1M over index must not error");

    assert_eq!(
        bars.len(),
        3,
        "three calendar months of index bars; got {}",
        bars.len()
    );
    assert!((bars[0].volume - 1000.0).abs() < 1e-9, "June SUM volume");
    assert!(
        (bars[1].volume - 3000.0).abs() < 1e-9,
        "July SUM volume (2 rows)"
    );
    assert!((bars[2].volume - 500.0).abs() < 1e-9, "August SUM volume");
    // The last close of the month is preserved (date_trunc + LAST).
    assert!(
        (bars[1].close - 3020.0).abs() < 1e-9,
        "July close = 31st close"
    );
}

// ---------------------------------------------------------------------------
// Regression guard: stock 1M aggregation is unchanged
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_bars_monthly_aggregates_stock_unchanged() {
    // The 1M path must keep working for ordinary stocks (SZ000001 across two
    // months), proving the index fallback does not disturb stock aggregation.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_stock_daily_parquet(
        tmp.path(),
        "SZ000001",
        &[
            ("2026-06-30", 10.0, 1000.0),
            ("2026-07-01", 11.0, 1000.0),
            ("2026-07-02", 12.0, 2000.0),
        ],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("SZ000001", "1M", epoch_start(), epoch_end())
        .await
        .expect("fetch_bars 1M over stock must not error");

    assert_eq!(bars.len(), 2, "two calendar months of stock bars");
    assert!((bars[0].volume - 1000.0).abs() < 1e-9, "June SUM volume");
    assert!((bars[1].volume - 3000.0).abs() < 1e-9, "July SUM volume");
    assert!(
        (bars[1].close - 12.0).abs() < 1e-9,
        "July close = last day close"
    );
}
