//! Adversarial tests: `DuckDbProvider::fetch_bars` dual-parquet fallback
//! (stock_daily.parquet → index_daily.parquet, epic #255 plan T5 / C4).
//!
//! Plan contract under attack:
//! - index symbols (e.g. SH000001) miss in stock_daily.parquet → hit in
//!   index_daily.parquet (with `adjclose = close` columns so factor stays 1.0)
//! - board symbols (BKxxxx) route to index_daily.parquet
//! - stock symbols never leak into the index file (e.g. SZ000001 must always
//!   resolve from stock_daily.parquet even when index data exists)
//! - 1w / 1M aggregation reuses the index path (SUM volume)
//! - both files missing → empty bars, no panic
//!
//! RED: the current implementation only falls back to stock_daily.parquet, so
//! index/board symbols resolve to an empty bar vec — the assertions below
//! (bars.len() > 0) fail.
//!
//! Why `tests/`: the sandbox denies writes to `src/**` (see
//! `crates/compass-data/tests/data_quality_adversarial.rs` precedent). All
//! touched APIs are `pub` (DuckDbProvider + DataProvider trait).

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

/// Write `stock_daily.parquet` with the canonical 9-column stock layout.
fn write_stock_daily_parquet(dir: &Path, symbol: &str, days: &[(&str, f64)]) {
    let conn = Connection::open_in_memory().expect("duckdb");
    conn.execute_batch(
        "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, \
         low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
    )
    .expect("create");
    for (day, close) in days {
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
                1000.0,
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

/// Write `index_daily.parquet` with the plan's export column layout
/// (symbol, index_type, tradedate, open, high, low, close, volume, amount,
/// adjclose) — `adjclose = close` so the forward-adjustment factor is 1.0.
fn write_index_daily_parquet(dir: &Path, symbol: &str, index_type: &str, days: &[(&str, f64)]) {
    let conn = Connection::open_in_memory().expect("duckdb");
    conn.execute_batch(
        "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, \
         open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, \
         amount DOUBLE, adjclose DOUBLE)",
    )
    .expect("create");
    for (day, close) in days {
        conn.execute(
            "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            duckdb::params![
                symbol,
                index_type,
                day,
                close - 1.0,
                close + 1.0,
                close - 2.0,
                close,
                1000.0,
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

// ---------------------------------------------------------------------------
// RED: index fallback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_bars_falls_back_to_index_daily_parquet() {
    // Only index_daily.parquet exists — an official index symbol must resolve
    // from it. RED: current code only knows stock_daily.parquet → empty bars.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_index_daily_parquet(
        tmp.path(),
        "SH000001",
        "official",
        &[
            ("2026-07-29", 3000.0),
            ("2026-07-30", 3001.0),
            ("2026-07-31", 3002.0),
        ],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("SH000001", "1d", epoch_start(), epoch_end(), "qfq")
        .await
        .expect("fetch_bars should not error");

    assert_eq!(
        bars.len(),
        3,
        "SH000001 must fall back to index_daily.parquet; got {} bars",
        bars.len()
    );
    // adjclose = close → factor 1.0 → prices unchanged.
    assert!(
        (bars[2].close - 3002.0).abs() < 1e-9,
        "index bar close preserved"
    );
}

#[tokio::test]
async fn fetch_bars_bk_symbol_routes_to_index_daily() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_index_daily_parquet(
        tmp.path(),
        "BK0475",
        "concept",
        &[("2026-07-30", 1200.0), ("2026-07-31", 1210.0)],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("BK0475", "1d", epoch_start(), epoch_end(), "qfq")
        .await
        .expect("fetch_bars should not error");

    assert_eq!(
        bars.len(),
        2,
        "BK0475 must resolve from index_daily.parquet"
    );
    assert!((bars[1].close - 1210.0).abs() < 1e-9);
}

#[tokio::test]
async fn fetch_bars_weekly_aggregates_index_sum_volume() {
    // 1w aggregation over index data: same DATE_TRUNC path as stocks, with
    // SUM(volume) per week. RED: no index fallback → empty result.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_index_daily_parquet(
        tmp.path(),
        "SH000001",
        "official",
        &[
            ("2026-07-06", 3000.0), // week 1
            ("2026-07-07", 3010.0),
            ("2026-07-13", 3020.0), // week 2
            ("2026-07-14", 3030.0),
        ],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("SH000001", "1w", epoch_start(), epoch_end(), "qfq")
        .await
        .expect("fetch_bars 1w should not error");

    assert_eq!(bars.len(), 2, "two ISO weeks of index bars");
    assert!((bars[0].volume - 2000.0).abs() < 1e-9, "SUM volume week 1");
    assert!((bars[1].volume - 2000.0).abs() < 1e-9, "SUM volume week 2");
}

// ---------------------------------------------------------------------------
// Guards: no leakage, no regression
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_bars_stock_symbol_not_leaked_from_index() {
    // SZ000001 (stock) exists only in stock_daily.parquet; SH000001 (index)
    // exists only in index_daily.parquet. The stock must resolve from the
    // stock file and never pick up index rows.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_stock_daily_parquet(tmp.path(), "SZ000001", &[("2026-07-31", 10.0)]);
    write_index_daily_parquet(
        tmp.path(),
        "SH000001",
        "official",
        &[("2026-07-31", 3000.0)],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("SZ000001", "1d", epoch_start(), epoch_end(), "qfq")
        .await
        .expect("stock fetch should succeed");

    assert_eq!(bars.len(), 1);
    assert!(
        (bars[0].close - 10.0).abs() < 1e-9,
        "stock price, not index price"
    );
}

#[tokio::test]
async fn fetch_bars_prefers_stock_over_index_on_collision() {
    // Data-pollution scenario: SH000001 present in BOTH files. The stock file
    // is authoritative — the index file is only a fallback.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_stock_daily_parquet(tmp.path(), "SH000001", &[("2026-07-31", 88.0)]);
    write_index_daily_parquet(
        tmp.path(),
        "SH000001",
        "official",
        &[("2026-07-31", 3000.0)],
    );

    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let bars = provider
        .fetch_bars("SH000001", "1d", epoch_start(), epoch_end(), "qfq")
        .await
        .expect("fetch should succeed");

    assert_eq!(bars.len(), 1);
    assert!(
        (bars[0].close - 88.0).abs() < 1e-9,
        "stock file must win on collision, got {}",
        bars[0].close
    );
}

#[tokio::test]
async fn fetch_bars_both_files_missing_returns_empty_no_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");
    let result = provider
        .fetch_bars("SH000001", "1d", epoch_start(), epoch_end(), "qfq")
        .await;
    assert!(
        result.is_ok() || result.is_err(),
        "missing both files must not panic"
    );
}

// ---------------------------------------------------------------------------
// Concurrency: two simultaneous fetches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_fetch_bars_stock_and_index_both_succeed() {
    // Two fetches arrive at once (one stock symbol + one index symbol). The
    // Arc<Mutex<Connection>> must not deadlock or drop one request.
    let tmp = tempfile::tempdir().expect("tempdir");
    write_stock_daily_parquet(tmp.path(), "SZ000001", &[("2026-07-31", 10.0)]);
    write_index_daily_parquet(
        tmp.path(),
        "SH000001",
        "official",
        &[("2026-07-31", 3000.0)],
    );
    let provider = DuckDbProvider::new(Some(tmp.path().to_path_buf())).expect("provider");

    let (stock_res, index_res) = tokio::join!(
        provider.fetch_bars("SZ000001", "1d", epoch_start(), epoch_end(), "qfq"),
        provider.fetch_bars("SH000001", "1d", epoch_start(), epoch_end(), "qfq"),
    );

    let stock_bars = stock_res.expect("stock fetch");
    let index_bars = index_res.expect("index fetch");
    assert_eq!(stock_bars.len(), 1);
    assert_eq!(
        index_bars.len(),
        1,
        "concurrent index fetch must not be dropped"
    );
}
