// =============================================================================
// Integration tests for DuckDB provider and Parquet reader.
// =============================================================================

use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::parquet::ParquetReader;
use compass_core::data::provider::DataProvider;

#[tokio::test]
async fn duckdb_in_memory_has_required_tables() {
    let db = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

    let tables = [
        "stock_daily",
        "stock_adj_factor",
        "stock_basic",
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
            .expect(&format!("query for table {table}"));
        assert!(exists, "table '{table}' should exist in DuckDB schema");
    }
}

#[tokio::test]
#[ignore = "requires parquet_data/ with stock_daily.parquet — run `cargo run --bin compass-data -- import --limit 3`"]
async fn parquet_reader_loads_exported_data() {
    let reader =
        ParquetReader::new("parquet_data").expect("failed to open ParquetReader (run import)");

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
        .fetch_bars(&first.code, "1d", start, end)
        .await
        .expect("fetch_bars failed");

    assert!(
        !bars.is_empty(),
        "symbol {} should have at least one bar",
        first.code
    );
}
