use std::path::{Path, PathBuf};

use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::parquet::ParquetReader;
use compass_core::data::provider::{DataError, DataProvider};
use tracing::{error, info, warn};

/// Export Parquet data to another format.
pub async fn run_export(input: PathBuf, format: String, output: PathBuf, overwrite: bool) {
    match format.as_str() {
        "duckdb" => {
            info!("Exporting Parquet → DuckDB: {}", output.display());
            let reader = match ParquetReader::new(&input) {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to open Parquet directory: {e}");
                    return;
                }
            };
            let symbols = match reader.list_symbols() {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to list symbols: {e}");
                    return;
                }
            };

            let db = match DuckDbProvider::new_file(output.to_str().unwrap_or("export.duckdb")) {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to create DuckDB: {e}");
                    return;
                }
            };

            for info in &symbols {
                let bars = match reader
                    .fetch_bars(
                        &info.code,
                        "1d",
                        chrono::DateTime::from_timestamp(0, 0).unwrap(),
                        chrono::Utc::now(),
                    )
                    .await
                {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                use compass_core::data::duckdb::DailyRecord;
                let records: Vec<DailyRecord> = bars
                    .iter()
                    .map(|b| DailyRecord {
                        trade_date: b.time.date_naive(),
                        open: b.open,
                        high: b.high,
                        low: b.low,
                        close: b.close,
                        adjclose: b.close,
                        volume: b.volume,
                        amount: 0.0,
                    })
                    .collect();

                if let Err(e) = db.save_stock_daily(&info.code, &records, overwrite).await {
                    warn!("save_stock_daily failed for {}: {}", info.code, e);
                }
            }
            export_index_tables(&db, &input, overwrite).await;
            info!("Exported {} symbols to {}", symbols.len(), output.display());
        }
        other => {
            warn!("Unknown export format: {other}. Supported: duckdb");
        }
    }
}

/// Mirror the standalone index parquet files into the output DuckDB.
///
/// `index_daily.parquet` / `index_basic.parquet` are kept out of
/// `stock_daily` by design (ref #201), so they are copied into the DuckDB
/// file as tables of the same name. A missing or empty parquet is skipped —
/// the export must never fail the whole run over optional index data. With
/// `overwrite` the existing table is dropped first (replace semantics).
async fn export_index_tables(db: &DuckDbProvider, input: &Path, overwrite: bool) {
    const INDEX_TABLES: &[(&str, &str)] = &[
        ("index_daily", "index_daily.parquet"),
        ("index_basic", "index_basic.parquet"),
    ];
    for (table, file_name) in INDEX_TABLES {
        let parquet_path = input.join(file_name);
        if !parquet_path.exists() {
            info!("{table}: parquet not present, skipping");
            continue;
        }
        let count_sql = format!(
            "SELECT COUNT(*) FROM read_parquet('{}')",
            parquet_path.display()
        );
        let has_rows = match db.table_has_rows(&count_sql).await {
            Ok(rows) => rows,
            Err(e) => {
                warn!("{table}: unreadable parquet ({e}), skipping");
                continue;
            }
        };
        if !has_rows {
            info!("{table}: 0 rows, skipping");
            continue;
        }
        if overwrite
            && let Err(e) = db
                .execute_batch(&format!("DROP TABLE IF EXISTS {table}"))
                .await
        {
            warn!("{table}: drop failed: {e}");
        }
        let copy_sql = format!(
            "CREATE TABLE IF NOT EXISTS {table} AS SELECT * FROM read_parquet('{}')",
            parquet_path.display()
        );
        match db.execute_batch(&copy_sql).await {
            Ok(()) => info!("{table}: exported → duckdb"),
            Err(e) => warn!("{table}: export failed: {e}"),
        }
    }
}

#[allow(dead_code)]
const TABLES: &[(&str, &str)] = &[
    ("stock_daily", "ORDER BY symbol, trade_date"),
    ("stock_adj_factor", "ORDER BY symbol, trade_date"),
    ("stock_limit", "ORDER BY symbol, trade_date"),
];

#[allow(dead_code)]
pub async fn export_all_tables(db: &DuckDbProvider, dir: &Path) -> Result<(), DataError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        DataError::Parse(format!(
            "failed to create export dir {}: {e}",
            dir.display()
        ))
    })?;

    for (table, order_clause) in TABLES {
        let file_name = format!("{table}.parquet");
        let file_path = dir.join(&file_name);
        let file_path_str = file_path.display().to_string();

        let count_sql = format!("SELECT COUNT(*) FROM {table}");
        let has_rows = db.table_has_rows(&count_sql).await.unwrap_or(false);

        if !has_rows {
            info!("{table}: 0 rows, skipping");
            continue;
        }

        let copy_sql = format!(
            "COPY (SELECT * FROM {table} {order_clause}) TO '{file_path_str}' (FORMAT PARQUET, COMPRESSION ZSTD)"
        );

        match db.execute_batch(&copy_sql).await {
            Ok(()) => info!("{table}: exported → {file_path_str}"),
            Err(e) => warn!("{table}: export failed: {e}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use compass_core::data::duckdb::DailyRecord;

    #[tokio::test]
    async fn export_all_tables_creates_parquet_files() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d1 = NaiveDate::from_ymd_opt(2025, 3, 1).expect("valid date");
        let d2 = NaiveDate::from_ymd_opt(2025, 3, 2).expect("valid date");

        let records = vec![
            DailyRecord {
                trade_date: d1,
                open: 15.0,
                high: 16.0,
                low: 14.5,
                close: 15.5,
                adjclose: 15.5,
                volume: 1000.0,
                amount: 15000.0,
            },
            DailyRecord {
                trade_date: d2,
                open: 15.5,
                high: 17.0,
                low: 15.0,
                close: 16.5,
                adjclose: 16.5,
                volume: 2000.0,
                amount: 33000.0,
            },
        ];

        provider
            .save_stock_daily("000001", &records, true)
            .await
            .expect("save_stock_daily failed");

        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        let dir = tmp.path();

        export_all_tables(&provider, dir)
            .await
            .expect("export_all_tables failed");

        let parquet_path = dir.join("stock_daily.parquet");
        assert!(parquet_path.exists(), "stock_daily.parquet should exist");
        assert!(
            std::fs::metadata(&parquet_path).expect("metadata").len() > 0,
            "stock_daily.parquet should be non-empty"
        );

        for empty_table in &["stock_adj_factor", "stock_limit"] {
            let p = dir.join(format!("{empty_table}.parquet"));
            assert!(!p.exists(), "{empty_table} should be skipped (empty)");
        }
    }

    #[tokio::test]
    async fn export_all_tables_creates_directory_if_missing() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");

        let d = NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date");
        provider
            .save_stock_daily(
                "000001",
                &[DailyRecord {
                    trade_date: d,
                    open: 10.0,
                    high: 11.0,
                    low: 9.0,
                    close: 10.5,
                    adjclose: 10.5,
                    volume: 100.0,
                    amount: 1000.0,
                }],
                true,
            )
            .await
            .expect("save_stock_daily failed");

        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        let dir = tmp.path().join("nested").join("subdir");

        export_all_tables(&provider, &dir)
            .await
            .expect("export_all_tables failed");

        assert!(dir.join("stock_daily.parquet").exists());
    }

    #[tokio::test]
    async fn run_export_duckdb_creates_database() {
        // Create temp single-file Parquet data with symbol column
        let parquet_tmp = tempfile::tempdir().expect("tempdir");

        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t(symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);
             INSERT INTO t VALUES ('000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0);",
        ).expect("create");
        let pq_path = parquet_tmp.path().join("stock_daily.parquet");
        conn.execute_batch(&format!(
            "COPY t TO '{}' (FORMAT PARQUET)",
            pq_path.display()
        ))
        .expect("copy");

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");

        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        assert!(duckdb_path.exists(), "DuckDB file should be created");
    }

    #[tokio::test]
    async fn run_export_unknown_format_warns() {
        // Unknown format should hit the `other =>` warn branch without panicking
        let tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = tmp.path().join("export.duckdb");

        run_export(
            tmp.path().to_path_buf(),
            "csv".to_string(),
            duckdb_path,
            true,
        )
        .await;

        // No assertion needed — just verifying the call doesn't panic
    }

    #[tokio::test]
    async fn run_export_duckdb_success_path_writes_records() {
        // Full happy path: symbols.txt + real parquet data with a prefixed
        // symbol. fetch_bars returns Ok → records are mapped and saved into
        // the output DuckDB (covers the success loop, not just error skips).
        let parquet_tmp = tempfile::tempdir().expect("tempdir");

        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE t(symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);
             INSERT INTO t VALUES ('SZ000001', '2024-01-02', 9, 11, 8, 10, 10, 1000, 0);",
        )
        .expect("create");
        conn.execute_batch(&format!(
            "COPY t TO '{}' (FORMAT PARQUET)",
            parquet_tmp.path().join("stock_daily.parquet").display()
        ))
        .expect("copy");

        std::fs::write(
            parquet_tmp.path().join("stock_daily.symbols.txt"),
            "SZ000001\n",
        )
        .expect("write symbols.txt");

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");

        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        let out_conn = duckdb::Connection::open(&duckdb_path).expect("open export db");
        let count: usize = out_conn
            .query_row("SELECT COUNT(*) FROM stock_daily", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1, "exported row should be written to stock_daily");
    }

    #[tokio::test]
    async fn run_export_duckdb_fails_silently_when_list_symbols_errors() {
        // Corrupt stock_daily.parquet with no symbols.txt forces list_symbols
        // down the SQL fallback path, which errors on the unreadable parquet
        // → run_export logs and returns without panicking.
        let parquet_tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            parquet_tmp.path().join("stock_daily.parquet"),
            b"this is not a parquet file",
        )
        .expect("write corrupt parquet");

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");

        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path,
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn run_export_duckdb_fails_silently_when_output_dir_missing() {
        // Connection::open fails when the parent directory of the target
        // DuckDB file does not exist → the new_file error path is logged.
        let parquet_tmp = tempfile::tempdir().expect("tempdir");

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("nonexistent").join("export.duckdb");

        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path,
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn export_all_tables_returns_error_when_dir_is_a_file() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let tmp = tempfile::tempdir().expect("tempdir");
        let file_path = tmp.path().join("not_a_dir");
        std::fs::write(&file_path, b"file").expect("write file");

        let result = export_all_tables(&provider, &file_path).await;
        assert!(result.is_err(), "create_dir_all on a file should fail");
    }

    #[tokio::test]
    async fn export_all_tables_warns_when_copy_target_is_directory() {
        let provider = DuckDbProvider::new_in_memory().expect("failed to open in-memory DuckDB");
        let d = NaiveDate::from_ymd_opt(2025, 1, 1).expect("valid date");
        provider
            .save_stock_daily(
                "SZ000001",
                &[DailyRecord {
                    trade_date: d,
                    open: 10.0,
                    high: 11.0,
                    low: 9.0,
                    close: 10.5,
                    adjclose: 10.5,
                    volume: 100.0,
                    amount: 1000.0,
                }],
                true,
            )
            .await
            .expect("save_stock_daily failed");

        // Pre-create a directory where the COPY target file would go — DuckDB
        // COPY to an existing directory fails → the warn path is exercised
        // and the loop continues with the remaining tables.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tmp.path().join("stock_daily.parquet")).expect("mkdir");

        export_all_tables(&provider, tmp.path())
            .await
            .expect("export should complete despite per-table warnings");
    }

    #[tokio::test]
    async fn run_export_fetch_bars_error_continues_when_file_missing() {
        // When stock_daily.symbols.txt lists symbols but stock_daily.parquet is
        // missing, fetch_bars_blocking returns NoData → continue to next symbol.
        let parquet_tmp = tempfile::tempdir().expect("tempdir");

        // Create stock_daily.symbols.txt so list_symbols returns symbols
        std::fs::write(
            parquet_tmp.path().join("stock_daily.symbols.txt"),
            "000001\n",
        )
        .expect("write symbols.txt");

        // Do NOT create stock_daily.parquet — fetch_bars will return NoData

        // stock_basic.parquet is not strictly needed since ParquetReader::new
        // doesn't check, but fetch_bars doesn't use it either.
        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");

        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        // Should not panic; DuckDB file created even though no data was exported
        assert!(duckdb_path.exists(), "DuckDB file should be created");
    }

    fn write_index_parquet(dir: &Path, file_name: &str, create_sql: &str, insert_sql: &str) {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(&format!("{create_sql}; {insert_sql};"))
            .expect("seed");
        conn.execute_batch(&format!(
            "COPY t TO '{}' (FORMAT PARQUET)",
            dir.join(file_name).display()
        ))
        .expect("copy");
    }

    #[tokio::test]
    async fn run_export_duckdb_exports_index_tables() {
        let parquet_tmp = tempfile::tempdir().expect("tempdir");
        write_index_parquet(
            parquet_tmp.path(),
            "index_daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, amount DOUBLE, adjclose DOUBLE)",
            "INSERT INTO t VALUES ('SH000001', 'official', '2026-01-05', 3000, 3002, 2998, 3001, 1e8, 1e10, 3001)",
        );
        write_index_parquet(
            parquet_tmp.path(),
            "index_basic.parquet",
            "CREATE TABLE t (symbol VARCHAR, name VARCHAR, index_type VARCHAR)",
            "INSERT INTO t VALUES ('BK0475', '半导体', 'concept')",
        );

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");
        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        let out = duckdb::Connection::open(&duckdb_path).expect("open export db");
        let daily: usize = out
            .query_row("SELECT COUNT(*) FROM index_daily", [], |row| row.get(0))
            .expect("index_daily count");
        assert_eq!(daily, 1, "index_daily must be mirrored into DuckDB");
        let basic: usize = out
            .query_row("SELECT COUNT(*) FROM index_basic", [], |row| row.get(0))
            .expect("index_basic count");
        assert_eq!(basic, 1, "index_basic must be mirrored into DuckDB");
        let adjclose: f64 = out
            .query_row(
                "SELECT adjclose FROM index_daily WHERE symbol = 'SH000001'",
                [],
                |row| row.get(0),
            )
            .expect("adjclose");
        assert!((adjclose - 3001.0).abs() < 1e-9, "adjclose carried through");
    }

    /// Epic #266 B2 (plan acceptance #4): the DuckDB export mirror must carry
    /// the new `name_en` column (and its data) into the `index_basic` table.
    ///
    /// The mirror is `CREATE TABLE AS SELECT * FROM read_parquet(...)`, so it
    /// is column-agnostic: this test guards that a parquet shipping `name_en`
    /// (produced once import-compass writes it) surfaces verbatim in the
    /// DuckDB mirror. It is expected to be GREEN even before B2 touches
    /// export.rs — the genuine RED gap lives upstream in the import-compass
    /// SELECT (see tests/requirement_name_en_data.rs).
    /// Manufacturability note: `export` is private to the binary (main.rs
    /// `mod export;`), so this must live in export.rs's unit tests rather than
    /// an integration test.
    #[tokio::test]
    async fn run_export_duckdb_mirror_carries_name_en() {
        let parquet_tmp = tempfile::tempdir().expect("tempdir");
        write_index_parquet(
            parquet_tmp.path(),
            "index_basic.parquet",
            "CREATE TABLE t (symbol VARCHAR, name VARCHAR, name_en VARCHAR, index_type VARCHAR)",
            "INSERT INTO t VALUES ('SH000001', '上证指数', 'SSE Composite', 'official')",
        );

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");
        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        let out = duckdb::Connection::open(&duckdb_path).expect("open export db");
        let basic: usize = out
            .query_row("SELECT COUNT(*) FROM index_basic", [], |row| row.get(0))
            .expect("index_basic count");
        assert_eq!(basic, 1, "index_basic must be mirrored into DuckDB");
        let name_en: Option<String> = out
            .query_row(
                "SELECT name_en FROM index_basic WHERE symbol = 'SH000001'",
                [],
                |row| row.get(0),
            )
            .expect("DuckDB mirror index_basic must expose the name_en column");
        assert_eq!(
            name_en.as_deref(),
            Some("SSE Composite"),
            "DuckDB mirror must carry name_en data from the parquet"
        );
    }

    #[tokio::test]
    async fn run_export_duckdb_skips_empty_index_parquet() {
        let parquet_tmp = tempfile::tempdir().expect("tempdir");
        write_index_parquet(
            parquet_tmp.path(),
            "index_daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, amount DOUBLE, adjclose DOUBLE)",
            "",
        );

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");
        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        let out = duckdb::Connection::open(&duckdb_path).expect("open export db");
        let has_table: usize = out
            .query_row(
                "SELECT COUNT(*) FROM information_schema.tables \
                 WHERE table_name = 'index_daily'",
                [],
                |row| row.get(0),
            )
            .expect("table check");
        assert_eq!(has_table, 0, "empty index parquet must be skipped");
    }

    #[tokio::test]
    async fn run_export_duckdb_skips_unreadable_index_parquet() {
        let parquet_tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            parquet_tmp.path().join("index_daily.parquet"),
            b"not a parquet file",
        )
        .expect("write corrupt");

        let duckdb_tmp = tempfile::tempdir().expect("tempdir");
        let duckdb_path = duckdb_tmp.path().join("export.duckdb");
        run_export(
            parquet_tmp.path().to_path_buf(),
            "duckdb".to_string(),
            duckdb_path.clone(),
            true,
        )
        .await;

        // Must not panic; the unreadable parquet is logged and skipped.
        assert!(duckdb_path.exists(), "DuckDB file should be created");
    }
}
