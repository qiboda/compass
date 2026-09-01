// =============================================================================
// Integration tests for DuckDB provider and Parquet reader.
// =============================================================================

use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::parquet::ParquetReader;
use compass_core::data::provider::{DataProvider, DataWriter};
use egui_charts::model::Bar;

#[tokio::test]
async fn duckdb_in_memory_has_required_tables() {
    let db = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

    let tables = [
        "stock_daily",
        "stock_adj_factor",
        "stock_limit",
        "no_data_marks",
    ];

    let conn = db.lock_connection().expect("lock");
    for table in &tables {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM information_schema.tables WHERE table_name = ?1",
                duckdb::params![table],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| panic!("query for table {table}"));
        assert!(exists, "table '{table}' should exist in DuckDB schema");
    }
}

#[tokio::test]
#[ignore = "requires parquet_data/ with stock_daily.parquet — run `cargo run --bin compass-data -- import --limit 3`"]
async fn parquet_reader_loads_exported_data() {
    let reader = ParquetReader::new("/data/compass-data/parquet_data")
        .expect("failed to open ParquetReader (run import)");

    let symbols = reader.list_symbols().expect("failed to list symbols");
    assert!(
        !symbols.is_empty(),
        "parquet_data should have at least one symbol"
    );

    // Fetch 3 bars for the first symbol
    let first = &symbols[0];
    let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
    let end = chrono::Utc::now();

    let bars = reader
        .fetch_bars(&first.code, "1d", start, end, "qfq")
        .await
        .expect("fetch_bars failed");

    assert!(
        !bars.is_empty(),
        "symbol {} should have at least one bar",
        first.code
    );
}

/// End-to-end timeframe aggregation (ref #46): save daily bars through the
/// public `DataWriter` API, then fetch them back as 1d / 1w / 1M through the
/// public `DataProvider` API. 1d returns the raw daily series; 1w and 1M
/// return the DuckDB `DATE_TRUNC`-grouped resample with correct OHLCV
/// semantics (open=first day, high=max, low=min, close=last day, volume=sum).
#[tokio::test]
async fn duckdb_timeframe_aggregation_roundtrip() {
    let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

    // Two full ISO weeks: 2026-07-06 (Mon) .. 2026-07-10 (Fri) and
    // 2026-07-13 (Mon) .. 2026-07-17 (Fri). Values chosen so each aggregate
    // is hand-checkable: week 1 open=10 high=14 low=9 close=14 volume=1500,
    // week 2 open=15 high=19 low=14 close=19 volume=1500.
    let bar = |day: &str, open: f64, high: f64, low: f64, close: f64, volume: f64| {
        let naive = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .expect("valid bar date")
            .and_hms_opt(0, 0, 0)
            .expect("valid time");
        Bar::new(naive.and_utc(), open, high, low, close, volume)
    };
    let daily = vec![
        bar("2026-07-06", 10.0, 11.0, 9.0, 10.5, 100.0),
        bar("2026-07-07", 10.5, 12.0, 10.0, 11.0, 200.0),
        bar("2026-07-08", 11.0, 13.0, 11.0, 12.0, 300.0),
        bar("2026-07-09", 12.0, 14.0, 12.0, 13.0, 400.0),
        bar("2026-07-10", 13.0, 14.0, 12.5, 14.0, 500.0),
        bar("2026-07-13", 15.0, 16.0, 14.5, 15.5, 100.0),
        bar("2026-07-14", 15.5, 17.0, 15.0, 16.0, 200.0),
        bar("2026-07-15", 16.0, 18.0, 15.5, 17.0, 300.0),
        bar("2026-07-16", 17.0, 19.0, 16.5, 18.0, 400.0),
        bar("2026-07-17", 18.0, 19.0, 17.0, 19.0, 500.0),
    ];

    provider
        .save_bars("000001", "1d", &daily, true)
        .await
        .expect("save_bars failed");

    let start = chrono::DateTime::from_timestamp(0, 0).expect("valid epoch");
    let end = chrono::Utc::now();

    let day_bars = provider
        .fetch_bars("000001", "1d", start, end, "qfq")
        .await
        .expect("fetch 1d failed");
    assert_eq!(day_bars.len(), 10, "1d must return the 10 saved daily bars");
    assert_eq!(day_bars[0].open, 10.0);
    assert_eq!(day_bars[9].close, 19.0);

    let week_bars = provider
        .fetch_bars("000001", "1w", start, end, "qfq")
        .await
        .expect("fetch 1w failed");
    assert_eq!(week_bars.len(), 2, "1w must aggregate to 2 weekly bars");
    let w1 = &week_bars[0];
    assert_eq!(w1.open, 10.0, "week 1 open = Monday open");
    assert_eq!(w1.high, 14.0, "week 1 high = week max");
    assert_eq!(w1.low, 9.0, "week 1 low = week min");
    assert_eq!(w1.close, 14.0, "week 1 close = Friday close");
    assert_eq!(w1.volume, 1500.0, "week 1 volume = sum");
    let w2 = &week_bars[1];
    assert_eq!(w2.open, 15.0, "week 2 open = Monday open");
    assert_eq!(w2.high, 19.0, "week 2 high = week max");
    assert_eq!(w2.low, 14.5, "week 2 low = week min");
    assert_eq!(w2.close, 19.0, "week 2 close = Friday close");
    assert_eq!(w2.volume, 1500.0, "week 2 volume = sum");

    let month_bars = provider
        .fetch_bars("000001", "1M", start, end, "qfq")
        .await
        .expect("fetch 1M failed");
    assert_eq!(month_bars.len(), 1, "1M must aggregate to 1 monthly bar");
    let m1 = &month_bars[0];
    assert_eq!(m1.open, 10.0, "month open = first day open");
    assert_eq!(m1.high, 19.0, "month high = max of all days");
    assert_eq!(m1.low, 9.0, "month low = min of all days");
    assert_eq!(m1.close, 19.0, "month close = last day close");
    assert_eq!(m1.volume, 3000.0, "month volume = sum of all days");
}
