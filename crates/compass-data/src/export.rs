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
            info!("Exported {} symbols to {}", symbols.len(), output.display());
        }
        other => {
            warn!("Unknown export format: {other}. Supported: duckdb");
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
}
