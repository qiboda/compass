use std::path::Path;

use compass_rs::data::duckdb::DuckDbProvider;
use compass_rs::data::provider::DataError;
use tracing::{info, warn};

const TABLES: &[(&str, &str)] = &[
    ("stock_daily", "ORDER BY ts_code, trade_date"),
    ("stock_adj_factor", "ORDER BY ts_code, trade_date"),
    ("stock_basic", "ORDER BY ts_code"),
    ("stock_status", "ORDER BY ts_code, trade_date"),
    ("stock_limit", "ORDER BY ts_code, trade_date"),
    ("daily_indicator", "ORDER BY ts_code, trade_date"),
    ("stock_share", "ORDER BY ts_code, trade_date"),
];

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
    use compass_rs::data::duckdb::DailyRecord;

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
                change: 0.5,
                pct_chg: 3.33,
                vol: 1000.0,
                amount: 15000.0,
            },
            DailyRecord {
                trade_date: d2,
                open: 15.5,
                high: 17.0,
                low: 15.0,
                close: 16.5,
                change: 1.0,
                pct_chg: 6.45,
                vol: 2000.0,
                amount: 33000.0,
            },
        ];

        provider
            .save_stock_daily("000001.SZ", &records)
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

        for empty_table in &[
            "stock_adj_factor",
            "stock_basic",
            "stock_status",
            "stock_limit",
            "daily_indicator",
            "stock_share",
        ] {
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
                "000001.SZ",
                &[DailyRecord {
                    trade_date: d,
                    open: 10.0,
                    high: 11.0,
                    low: 9.0,
                    close: 10.5,
                    change: 0.0,
                    pct_chg: 0.0,
                    vol: 100.0,
                    amount: 1000.0,
                }],
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
}
