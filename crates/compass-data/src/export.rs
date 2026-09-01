use std::path::{Path, PathBuf};

use compass_core::data::duckdb::DuckDbProvider;
use compass_core::data::parquet::ParquetReader;
use compass_core::data::provider::{DataError, DataProvider};
use tracing::{error, info, warn};

/// CSV header emitted by [`export_csv`]; the column order is the export
/// contract (symbol, trade_date, OHLC, adjclose, volume, amount).
const CSV_HEADER: &str = "symbol,trade_date,open,high,low,close,adjclose,volume,amount";

/// Escape single quotes in paths embedded in `read_parquet('...')` / `TO '...'`
/// SQL string literals (mirrors `parquet.rs::escape_sql_path`).
fn escape_sql_path(path: &str) -> String {
    path.replace('\'', "''")
}

/// Reject an export whose output would overwrite the input data directory:
/// writing a csv / parquet-dir into `input` itself would first delete the
/// source `stock_daily.parquet` (overwrite=true) and then fail the read,
/// destroying the primary dataset. A `None`-free canonical comparison keeps
/// it cheap; the only hard requirement is that the user cannot silently shoot
/// their own main database with a typo'd `--output`.
fn reject_output_inside_input(input: &Path, output: &Path) -> bool {
    if !input.exists() || !output.exists() {
        return false;
    }
    let in_canon = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
    let out_canon = output
        .canonicalize()
        .unwrap_or_else(|_| output.to_path_buf());
    let same = out_canon.starts_with(&in_canon);
    if same {
        warn!(
            "refusing to export into the input directory itself (input={}, output={})",
            input.display(),
            output.display()
        );
    }
    same
}

/// Forward-adjustment factor SQL: `adjclose/close` when close > 0 and adjclose
/// is finite and > 0, else 1.0 — the same ratio/validity rule as the chart
/// read path (`indicators::adjust_ohlc`, ref #345; qfq mode further divides
/// by the last valid ratio's anchor) so the csv/parquet-dir exports
/// never drift from the chart reading path. Exports use the raw ratio
/// (equivalent to hfq scaling).
const ADJ_FACTOR_SQL: &str = "CASE WHEN close > 0 AND adjclose IS NOT NULL AND \
     isfinite(adjclose) AND adjclose > 0 THEN adjclose / close ELSE 1.0 END";

/// Symbol-shape filter: only canonical exchange-prefixed forms are exported
/// (`SH/SZ/BJ` + 6 digits, `BK` + 4-6 digits — same rule as
/// `ParquetReader::validate_symbol`). Bare codes or other shapes are skipped
/// even if a stray row exists in the source parquet.
const SYMBOL_SHAPE_SQL: &str = "(regexp_matches(symbol, '^(SH|SZ|BJ)[0-9]{6}$') \
     OR regexp_matches(symbol, '^BK[0-9]{4,6}$'))";

/// Index parquet files mirrored by [`export_parquet_dir`] /
/// [`export_index_parquet_files`] into an output directory or DuckDB.
const INDEX_FILES: &[&str] = &["index_daily.parquet", "index_basic.parquet"];

/// Export Parquet data to another format.
pub async fn run_export(input: PathBuf, format: String, output: PathBuf, overwrite: bool) {
    match format.as_str() {
        "csv" => {
            export_csv(&input, &output, overwrite);
        }
        "parquet-dir" => {
            export_parquet_dir(&input, &output, overwrite);
        }
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
                        "qfq",
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
            warn!("Unknown export format: {other}. Supported: duckdb, csv, parquet-dir");
        }
    }
}

/// Export `stock_daily.parquet` to a single CSV file (one row per bar).
///
/// Reads the raw parquet rows directly (not via `fetch_bars`, whose `Bar`
/// carries no `amount` field) and applies the same forward-adjustment factor
/// as the chart path (`ADJ_FACTOR_SQL`), so the exported prices match
/// `fetch_bars` semantics while `amount` is preserved. Only canonical
/// exchange-prefixed symbols are written (`SYMBOL_SHAPE_SQL`); corrupt input,
/// a missing input directory or a missing parquet file degrade to a warn
/// without panicking. An empty symbol set still produces the output file
/// (header-only) so callers can rely on its existence. Without `overwrite` an
/// existing output file is left untouched.
fn export_csv(input: &Path, output: &Path, overwrite: bool) {
    if output.exists() && !overwrite {
        warn!(
            "csv output {} already exists, skipping (use --overwrite to replace)",
            output.display()
        );
        return;
    }
    if reject_output_inside_input(input, output) {
        return;
    }
    if let Some(parent) = output.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!(
            "failed to create output directory {}: {e}",
            parent.display()
        );
        return;
    }

    let daily_path = input.join("stock_daily.parquet");
    if !daily_path.exists() {
        warn!(
            "stock_daily.parquet not present in {}, exporting header only",
            input.display()
        );
        write_csv_header_only(output);
        return;
    }

    let conn = match duckdb::Connection::open_in_memory() {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to open in-memory DuckDB: {e}");
            return;
        }
    };

    let src = escape_sql_path(&daily_path.to_string_lossy());
    let dst = escape_sql_path(&output.to_string_lossy());
    let sql = format!(
        "COPY (
             SELECT symbol,
                    CAST(CAST(tradedate AS DATE) AS VARCHAR) AS trade_date,
                    open * {factor} AS open,
                    high * {factor} AS high,
                    low * {factor} AS low,
                    close * {factor} AS close,
                    close * {factor} AS adjclose,
                    volume,
                    amount
             FROM read_parquet('{src}')
             WHERE {shape}
             ORDER BY symbol, tradedate ASC
         ) TO '{dst}' (FORMAT CSV, HEADER)",
        factor = ADJ_FACTOR_SQL,
        shape = SYMBOL_SHAPE_SQL
    );
    match conn.execute_batch(&sql) {
        Ok(()) => info!("Exported stock_daily → {} (csv)", output.display()),
        Err(e) => warn!("csv export failed: {e}"),
    }
}

/// Write a header-only CSV file (empty symbol set / missing source).
fn write_csv_header_only(output: &Path) {
    if let Err(e) = std::fs::write(output, format!("{CSV_HEADER}\n")) {
        warn!("failed to write header-only csv {}: {e}", output.display());
    }
}

/// Export `stock_daily.parquet` and the index parquets into a new Parquet
/// directory with the main-library layout (`stock_daily.parquet` + companion
/// `stock_daily.symbols.txt` + mirrored `index_daily.parquet` /
/// `index_basic.parquet`), readable again by `ParquetReader::new`.
///
/// Prices are forward-adjusted with the same factor as the chart path
/// (`ADJ_FACTOR_SQL`) so the exported directory round-trips through
/// `fetch_bars` with identical values; `amount`/`volume` pass through
/// verbatim. Only canonical exchange-prefixed symbols are exported
/// (`SYMBOL_SHAPE_SQL`) and the symbols.txt set equals the exported parquet
/// symbol set. Missing index parquets are skipped, empty index parquets do
/// not block the export, and `overwrite` replaces stale files (all four
/// outputs — daily + symbols + index mirrors — are removed first so a
/// re-export with a different input never leaves stale index files behind;
/// without it an existing output directory is left untouched).
fn export_parquet_dir(input: &Path, output: &Path, overwrite: bool) {
    if output.exists() && !overwrite {
        warn!(
            "parquet-dir output {} already exists, skipping (use --overwrite to replace)",
            output.display()
        );
        return;
    }
    if reject_output_inside_input(input, output) {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(output) {
        warn!(
            "failed to create output directory {}: {e}",
            output.display()
        );
        return;
    }

    let daily_path = input.join("stock_daily.parquet");
    let dst_daily = output.join("stock_daily.parquet");
    let dst_symbols = output.join("stock_daily.symbols.txt");

    if daily_path.exists() {
        if overwrite {
            // Remove all previous outputs (daily + symbols + index mirrors)
            // so a re-export with a different input never leaves stale
            // index files behind — the output must mirror the input.
            let _ = std::fs::remove_file(&dst_daily);
            let _ = std::fs::remove_file(&dst_symbols);
            for f in INDEX_FILES {
                let _ = std::fs::remove_file(output.join(f));
            }
        }
        let conn = match duckdb::Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to open in-memory DuckDB: {e}");
                return;
            }
        };
        let src = escape_sql_path(&daily_path.to_string_lossy());
        let dst = escape_sql_path(&dst_daily.to_string_lossy());
        let sql = format!(
            "COPY (
                 SELECT symbol,
                        tradedate,
                        open * {factor} AS open,
                        high * {factor} AS high,
                        low * {factor} AS low,
                        close * {factor} AS close,
                        close * {factor} AS adjclose,
                        volume,
                        amount
                 FROM read_parquet('{src}')
                 WHERE {shape}
                 ORDER BY symbol, tradedate ASC
             ) TO '{dst}' (FORMAT PARQUET, COMPRESSION ZSTD)",
            factor = ADJ_FACTOR_SQL,
            shape = SYMBOL_SHAPE_SQL
        );
        if let Err(e) = conn.execute_batch(&sql) {
            warn!("parquet-dir stock_daily export failed: {e}");
            return;
        }
        write_symbols_txt(&conn, &dst_daily, &dst_symbols);
        info!(
            "Exported stock_daily → {} (parquet-dir)",
            dst_daily.display()
        );
    } else {
        warn!(
            "stock_daily.parquet not present in {}, skipping stock export",
            input.display()
        );
        if overwrite {
            // The input no longer supplies a daily file — drop the stale
            // outputs (daily + symbols + index mirrors) so a re-export
            // mirror never serves last round's data.
            let _ = std::fs::remove_file(&dst_daily);
            let _ = std::fs::remove_file(&dst_symbols);
            for f in INDEX_FILES {
                let _ = std::fs::remove_file(output.join(f));
            }
        }
    }

    export_index_parquet_files(input, output);
}

/// Write `stock_daily.symbols.txt` (one symbol per line) from the exported
/// parquet's distinct symbol set.
fn write_symbols_txt(conn: &duckdb::Connection, dst_daily: &Path, dst_symbols: &Path) {
    let dst = escape_sql_path(&dst_daily.to_string_lossy());
    let sql = format!("SELECT DISTINCT symbol FROM read_parquet('{dst}') ORDER BY symbol ASC");
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to read exported symbols: {e}");
            return;
        }
    };
    let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
        Ok(rows) => rows,
        Err(e) => {
            warn!("failed to read exported symbols: {e}");
            return;
        }
    };
    let symbols: Vec<String> = rows.filter_map(Result::ok).collect();
    if let Err(e) = std::fs::write(dst_symbols, symbols.join("\n") + "\n") {
        warn!("failed to write {}: {e}", dst_symbols.display());
    }
}

/// Mirror `index_daily.parquet` / `index_basic.parquet` from the input
/// directory into the output directory (skip missing / empty parquets).
fn export_index_parquet_files(input: &Path, output: &Path) {
    for file_name in INDEX_FILES {
        let src = input.join(file_name);
        let dst = output.join(file_name);
        if !src.exists() {
            info!("{file_name}: not present in input, skipping");
            continue;
        }
        let conn = match duckdb::Connection::open_in_memory() {
            Ok(c) => c,
            Err(e) => {
                warn!("failed to open in-memory DuckDB: {e}");
                continue;
            }
        };
        let src_sql = escape_sql_path(&src.to_string_lossy());
        let count_sql = format!("SELECT COUNT(*) FROM read_parquet('{src_sql}')");
        let has_rows: bool = match conn
            .query_row(&count_sql, [], |row| row.get::<_, i64>(0))
            .map(|n| n > 0)
        {
            Ok(v) => v,
            Err(e) => {
                warn!("{file_name}: unreadable parquet ({e}), skipping");
                continue;
            }
        };
        if !has_rows {
            info!("{file_name}: 0 rows, skipping");
            continue;
        }
        let dst_sql = escape_sql_path(&dst.to_string_lossy());
        let copy_sql = format!(
            "COPY (SELECT * FROM read_parquet('{src_sql}')) TO '{dst_sql}' (FORMAT PARQUET, COMPRESSION ZSTD)"
        );
        match conn.execute_batch(&copy_sql) {
            Ok(()) => info!("{file_name}: exported → parquet-dir"),
            Err(e) => warn!("{file_name}: export failed: {e}"),
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
            escape_sql_path(&parquet_path.to_string_lossy())
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
            escape_sql_path(&parquet_path.to_string_lossy())
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
        let file_path_str = escape_sql_path(&file_path.to_string_lossy());

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
            "xml".to_string(),
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

    // ==================================================================
    // Issue #336 A1 — csv / parquet-dir requirement tests (RED, ref #336)
    // ==================================================================

    /// Row tuple: (symbol, date, open, high, low, close, adjclose, volume, amount).
    type BarRow = (
        &'static str,
        &'static str,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
    );

    /// Write a `stock_daily.parquet` (main-library schema) with the companion
    /// `stock_daily.symbols.txt` (sorted, deduped, one symbol per line).
    fn write_stock_daily_fixture(dir: &Path, rows: &[BarRow]) {
        let conn = duckdb::Connection::open_in_memory().expect("open in-memory duckdb");
        conn.execute_batch(
            "CREATE TABLE t (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, \
             low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE)",
        )
        .expect("create stock_daily table");
        for (symbol, date, open, high, low, close, adjclose, volume, amount) in rows {
            conn.execute(
                "INSERT INTO t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    symbol, date, open, high, low, close, adjclose, volume, amount
                ],
            )
            .expect("insert bar row");
        }
        conn.execute_batch(&format!(
            "COPY t TO '{}' (FORMAT PARQUET)",
            dir.join("stock_daily.parquet").display()
        ))
        .expect("copy stock_daily parquet");

        let mut symbols: Vec<String> = rows.iter().map(|r| r.0.to_string()).collect();
        symbols.sort();
        symbols.dedup();
        let mut txt = String::new();
        for s in &symbols {
            txt.push_str(s);
            txt.push('\n');
        }
        std::fs::write(dir.join("stock_daily.symbols.txt"), txt).expect("write symbols.txt");
    }

    /// Parse one CSV data row into (symbol, trade_date, [open, high, low,
    /// close, adjclose, volume, amount]).
    fn parse_csv_row(line: &str) -> (String, String, [f64; 7]) {
        let fields: Vec<&str> = line.split(',').collect();
        assert_eq!(
            fields.len(),
            9,
            "csv data row must have exactly 9 columns: {line}"
        );
        let mut nums = [0.0f64; 7];
        for (i, f) in fields[2..].iter().enumerate() {
            nums[i] = f.trim().parse::<f64>().expect("numeric csv field");
        }
        (fields[0].to_string(), fields[1].to_string(), nums)
    }

    /// #336 A1 csv happy path: single file, exact header, one row per bar,
    /// values forward-adjusted via fetch_bars (adjclose == close, OHLC scaled
    /// by the 前复权 factor), volume/amount carried, parent dir auto-created.
    #[tokio::test]
    async fn run_export_csv_writes_header_and_forward_adjusted_rows() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        // Two bars with distinct adjclose factors; latest bar is the anchor
        // (factor 1.0) and the older bar scales by 8/10 = 0.8.
        write_stock_daily_fixture(
            input_tmp.path(),
            &[
                (
                    "SZ000001",
                    "2024-01-02",
                    9.0,
                    11.0,
                    8.0,
                    10.0,
                    8.0,
                    1000.0,
                    15000.0,
                ),
                (
                    "SZ000001",
                    "2024-01-03",
                    11.0,
                    13.0,
                    10.5,
                    12.0,
                    12.0,
                    2000.0,
                    30000.0,
                ),
            ],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        // The parent directory does not exist yet — it must be auto-created.
        let csv_path = out_tmp.path().join("nested").join("data.csv");

        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            csv_path.clone(),
            true,
        )
        .await;

        let content =
            std::fs::read_to_string(&csv_path).expect("csv file must exist after a csv export");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines[0], "symbol,trade_date,open,high,low,close,adjclose,volume,amount",
            "csv header must match the contract"
        );
        assert_eq!(
            lines.len(),
            3,
            "header + 2 bar rows expected, got {} lines",
            lines.len()
        );

        // Older bar: forward-adjusted with factor 0.8.
        let (sym1, date1, n1) = parse_csv_row(lines[1]);
        assert_eq!(sym1, "SZ000001", "symbol column");
        assert_eq!(date1, "2024-01-02", "trade_date column");
        assert!(
            (n1[0] - 7.2).abs() < 1e-9,
            "open must be forward-adjusted: {}",
            n1[0]
        );
        assert!(
            (n1[1] - 8.8).abs() < 1e-9,
            "high must be forward-adjusted: {}",
            n1[1]
        );
        assert!(
            (n1[2] - 6.4).abs() < 1e-9,
            "low must be forward-adjusted: {}",
            n1[2]
        );
        assert!(
            (n1[3] - 8.0).abs() < 1e-9,
            "close must be forward-adjusted: {}",
            n1[3]
        );
        assert!(
            (n1[4] - n1[3]).abs() < 1e-9,
            "adjclose must equal close (前复权): adjclose={}, close={}",
            n1[4],
            n1[3]
        );
        assert!(
            (n1[5] - 1000.0).abs() < 1e-9,
            "volume must be carried: {}",
            n1[5]
        );
        assert!(
            (n1[6] - 15000.0).abs() < 1e-9,
            "amount must be preserved: {}",
            n1[6]
        );

        // Latest bar: anchor, factor 1.0.
        let (sym2, date2, n2) = parse_csv_row(lines[2]);
        assert_eq!(sym2, "SZ000001");
        assert_eq!(date2, "2024-01-03");
        assert!(
            (n2[0] - 11.0).abs() < 1e-9,
            "anchor open unchanged: {}",
            n2[0]
        );
        assert!(
            (n2[3] - 12.0).abs() < 1e-9,
            "anchor close unchanged: {}",
            n2[3]
        );
        assert!(
            (n2[4] - n2[3]).abs() < 1e-9,
            "adjclose must equal close on the anchor bar"
        );
        assert!(
            (n2[6] - 30000.0).abs() < 1e-9,
            "amount must be preserved: {}",
            n2[6]
        );
    }

    /// #336 A1 csv: an empty symbol set must not panic; the output file is
    /// still created (header-only or empty file).
    #[tokio::test]
    async fn run_export_csv_empty_symbol_set_does_not_panic() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        // Empty companion file → ParquetReader::list_symbols() returns an
        // empty set (stock_daily.parquet intentionally absent).
        std::fs::write(input_tmp.path().join("stock_daily.symbols.txt"), "")
            .expect("write empty symbols.txt");

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out = out_tmp.path().join("empty.csv");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            out.clone(),
            true,
        )
        .await;

        assert!(
            out.exists(),
            "csv file must be created even for an empty symbol set"
        );
        let content = std::fs::read_to_string(&out).expect("read csv");
        let non_empty: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert!(
            non_empty.len() <= 1,
            "no data rows expected for an empty symbol set, got {}",
            non_empty.len()
        );
        if let Some(first) = non_empty.first() {
            assert_eq!(
                *first, "symbol,trade_date,open,high,low,close,adjclose,volume,amount",
                "the only allowed row is the header"
            );
        }
    }

    /// #336 A1 csv: a bare code row inside `stock_daily.parquet` must be
    /// filtered out — the CSV export never copies non-canonical symbols even
    /// when they physically exist in the source file (the real attack surface:
    /// the WHERE shape filter is the only gate, NOT symbols.txt).
    #[tokio::test]
    async fn run_export_csv_filters_bare_code_rows_in_parquet() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[
                (
                    "SZ000001",
                    "2024-01-02",
                    10.0,
                    11.0,
                    9.0,
                    10.0,
                    8.0,
                    1000.0,
                    15000.0,
                ),
                (
                    "000001",
                    "2024-01-02",
                    5.0,
                    6.0,
                    4.0,
                    5.0,
                    5.0,
                    200.0,
                    3000.0,
                ),
                (
                    "SH600519",
                    "2024-01-03",
                    20.0,
                    21.0,
                    19.0,
                    20.0,
                    20.0,
                    500.0,
                    8000.0,
                ),
            ],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out = out_tmp.path().join("filtered.csv");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            out.clone(),
            true,
        )
        .await;

        let content = std::fs::read_to_string(&out).expect("read csv");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(
            lines.len(),
            3,
            "header + SZ000001 + SH600519 (bare 000001 must be dropped), got {content}"
        );
        assert!(lines.iter().any(|l| l.starts_with("SZ000001,")));
        assert!(lines.iter().any(|l| l.starts_with("SH600519,")));
        assert!(
            !content.lines().any(|l| l.starts_with("000001,")),
            "bare code row must not appear in the csv: {content}"
        );
    }

    /// #336 A1 csv: a parquet that exists but contains no data rows must still
    /// produce a header-only output file (empty result set is not an error —
    /// callers rely on the output file existing).
    #[tokio::test]
    async fn run_export_csv_empty_parquet_still_writes_header() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(input_tmp.path(), &[]);

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out = out_tmp.path().join("empty.csv");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            out.clone(),
            true,
        )
        .await;

        assert!(out.exists(), "csv must exist even for an empty parquet");
        let content = std::fs::read_to_string(&out).expect("read csv");
        assert_eq!(
            content.trim(),
            CSV_HEADER,
            "empty parquet must yield header-only csv, got {content}"
        );
    }

    /// #336 A1 csv: a nonexistent input directory must not panic — the export
    /// degrades gracefully (header-only output so callers can rely on the file
    /// existing) without panicking.
    #[tokio::test]
    async fn run_export_csv_missing_input_dir_does_not_panic() {
        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out = out_tmp.path().join("out.csv");

        run_export(
            out_tmp.path().join("no-such-input").to_path_buf(),
            "csv".to_string(),
            out.clone(),
            true,
        )
        .await;
        // Contract: no panic and a header-only file IS produced (implementation
        // writes write_csv_header_only for a missing source, mirroring the
        // empty-symbol-set contract).
        assert!(
            out.exists(),
            "header-only csv must exist after a missing input dir"
        );
        let content = std::fs::read_to_string(&out).expect("read csv");
        assert_eq!(
            content.trim(),
            CSV_HEADER,
            "missing input must yield header-only csv, got {content}"
        );
    }

    /// #336 A1 csv: `overwrite=false` with an existing output file must not
    /// silently replace it (conservative semantics, same as the duckdb branch).
    #[tokio::test]
    async fn run_export_csv_without_overwrite_keeps_existing_file() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                8.0,
                1000.0,
                15000.0,
            )],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let csv_path = out_tmp.path().join("data.csv");

        // First export with overwrite=true creates the file.
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            csv_path.clone(),
            true,
        )
        .await;
        let first = std::fs::read_to_string(&csv_path)
            .expect("first csv export must create the output file");

        // Change the source data so an overwrite would produce different content.
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                8.0,
                1000.0,
                99999.0,
            )],
        );

        // Second export without overwrite.
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            csv_path.clone(),
            false,
        )
        .await;
        let second = std::fs::read_to_string(&csv_path)
            .expect("existing csv must survive the non-overwrite export");
        assert_eq!(
            first, second,
            "non-overwrite csv export must not clobber the existing file"
        );
    }

    /// #336 A1 parquet-dir happy path: output directory carries
    /// stock_daily.parquet (with a symbol column), stock_daily.symbols.txt
    /// (exactly the parquet symbol set, one per line), and mirrors
    /// index_daily.parquet / index_basic.parquet when present in the input.
    #[tokio::test]
    async fn run_export_parquet_dir_writes_layout_and_symbol_files() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[
                (
                    "SZ000001",
                    "2024-01-02",
                    9.0,
                    11.0,
                    8.0,
                    10.0,
                    8.0,
                    1000.0,
                    15000.0,
                ),
                (
                    "SZ000001",
                    "2024-01-03",
                    11.0,
                    13.0,
                    10.5,
                    12.0,
                    12.0,
                    2000.0,
                    30000.0,
                ),
                (
                    "SH600519",
                    "2024-01-02",
                    1500.0,
                    1520.0,
                    1490.0,
                    1510.0,
                    1510.0,
                    500.0,
                    10000.0,
                ),
            ],
        );
        write_index_parquet(
            input_tmp.path(),
            "index_daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, amount DOUBLE, adjclose DOUBLE)",
            "INSERT INTO t VALUES ('SH000001', 'official', '2026-01-05', 3000, 3002, 2998, 3001, 1e8, 1e10, 3001)",
        );
        write_index_parquet(
            input_tmp.path(),
            "index_basic.parquet",
            "CREATE TABLE t (symbol VARCHAR, name VARCHAR, index_type VARCHAR)",
            "INSERT INTO t VALUES ('BK0475', '半导体', 'concept')",
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;

        assert!(
            out_dir.join("stock_daily.parquet").exists(),
            "stock_daily.parquet must be exported into the output directory"
        );

        // symbols.txt: one symbol per line, matching the input symbol set.
        let symbols_txt = std::fs::read_to_string(out_dir.join("stock_daily.symbols.txt"))
            .expect("stock_daily.symbols.txt must be exported");
        let mut txt_symbols: Vec<String> = symbols_txt
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        txt_symbols.sort();
        assert_eq!(
            txt_symbols,
            vec!["SH600519".to_string(), "SZ000001".to_string()],
            "symbols.txt must list exactly the input symbols, one per line"
        );

        // The parquet symbol set must equal the symbols.txt set.
        let conn = duckdb::Connection::open_in_memory().expect("open duckdb");
        let pq_path = out_dir.join("stock_daily.parquet");
        let mut stmt = conn
            .prepare(&format!(
                "SELECT DISTINCT symbol FROM read_parquet('{}')",
                pq_path.display()
            ))
            .expect("prepare distinct symbol query");
        let mut parquet_symbols: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .expect("query distinct symbols")
            .collect::<Result<Vec<String>, _>>()
            .expect("collect distinct symbols");
        parquet_symbols.sort();
        assert_eq!(
            parquet_symbols, txt_symbols,
            "parquet symbol set must match symbols.txt"
        );

        // Index parquets are mirrored when present in the input.
        assert!(
            out_dir.join("index_daily.parquet").exists(),
            "index_daily.parquet must be mirrored into the export"
        );
        assert!(
            out_dir.join("index_basic.parquet").exists(),
            "index_basic.parquet must be mirrored into the export"
        );
    }

    /// #336 A1 parquet-dir: the exported directory must be reopenable via
    /// `ParquetReader::new` — `list_symbols` matches the input and
    /// `fetch_bars` returns the same forward-adjusted bars.
    #[tokio::test]
    async fn run_export_parquet_dir_reopenable_by_parquet_reader() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[
                (
                    "SZ000001",
                    "2024-01-02",
                    9.0,
                    11.0,
                    8.0,
                    10.0,
                    8.0,
                    1000.0,
                    15000.0,
                ),
                (
                    "SZ000001",
                    "2024-01-03",
                    11.0,
                    13.0,
                    10.5,
                    12.0,
                    12.0,
                    2000.0,
                    30000.0,
                ),
            ],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;

        let reader = ParquetReader::new(&out_dir)
            .expect("ParquetReader::new must open the exported directory");
        let symbols = reader.list_symbols().expect("list_symbols on export");
        assert_eq!(
            symbols.len(),
            1,
            "exported symbol set must match the input (1 symbol)"
        );
        assert_eq!(symbols[0].code, "SZ000001", "exported symbol code");

        let bars = reader
            .fetch_bars(
                "SZ000001",
                "1d",
                chrono::DateTime::from_timestamp(0, 0).expect("epoch"),
                chrono::Utc::now(),
                "qfq",
            )
            .await
            .expect("fetch_bars on the exported directory must succeed");
        assert_eq!(
            bars.len(),
            2,
            "both bars must survive the parquet-dir round-trip"
        );
        // Forward-adjusted values (older bar factor 0.8; anchor 1.0) must
        // round-trip through ParquetReader regardless of whether the export
        // bakes the adjustment or copies raw rows.
        assert!(
            (bars[0].close - 8.0).abs() < 1e-9,
            "older bar close after round-trip: {}",
            bars[0].close
        );
        assert!(
            (bars[1].close - 12.0).abs() < 1e-9,
            "latest bar close after round-trip: {}",
            bars[1].close
        );
    }

    /// #336 A1 regression guard: a truly unknown format still warns and is
    /// skipped without touching the output (csv/parquet-dir must not break
    /// the `other =>` branch).
    #[tokio::test]
    async fn run_export_unknown_format_still_warns() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                8.0,
                1000.0,
                15000.0,
            )],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out = out_tmp.path().join("out.bin");
        run_export(
            input_tmp.path().to_path_buf(),
            "yaml-unknown".to_string(),
            out.clone(),
            true,
        )
        .await;

        assert!(
            !out.exists(),
            "unknown format must not create the output file"
        );
    }

    // ==================================================================
    // Issue #336 A1 — adversarial tests for csv / parquet-dir (ref #336)
    //
    // Attacks not covered by the requirement tests above: mixed prefixed /
    // bare / missing symbols (fetch_bars rejection semantics), scale-up
    // forward adjustment (factor > 1, guards against copying raw parquet
    // columns), corrupt input parquet, index_basic absence (must not be
    // fabricated), empty index_daily (must not block), stale-data replacement
    // on overwrite, and partial symbol failure.
    // ==================================================================

    #[tokio::test]
    async fn run_export_csv_mixed_prefixed_bare_and_missing_symbols() {
        // symbols.txt lists one served symbol plus a bare code (fetch_bars
        // validate_symbol rejects it → NoData) and a symbol with no rows.
        // The csv must carry exactly the fetch_bars-served symbols (same
        // semantics as the duckdb branch: failures are skipped, never
        // fabricated) without panicking.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                10.0,
                1000.0,
                0.0,
            )],
        );
        std::fs::write(
            input_tmp.path().join("stock_daily.symbols.txt"),
            "SZ000001\n000001\nSZ000002\n",
        )
        .expect("overwrite symbols.txt");

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_csv = out_tmp.path().join("mixed.csv");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            out_csv.clone(),
            true,
        )
        .await;

        let content = std::fs::read_to_string(&out_csv).expect("csv must exist");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            2,
            "header + exactly one served row expected, got {} lines: {content:?}",
            lines.len()
        );
        let (symbol, _date, _nums) = parse_csv_row(lines[1]);
        assert_eq!(
            symbol, "SZ000001",
            "only the fetch_bars-served symbol may appear"
        );
    }

    #[tokio::test]
    async fn run_export_csv_forward_adjusts_factor_above_one() {
        // adjclose = 15 vs close = 10 (factor 1.5): the csv row must carry
        // *adjusted* prices (close == adjclose == 15.0), never the raw
        // parquet close 10.0 — a naive "copy raw columns" implementation
        // fails here. Volume must pass through verbatim.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                15.0,
                777000.0,
                0.0,
            )],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_csv = out_tmp.path().join("adjusted.csv");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            out_csv.clone(),
            true,
        )
        .await;

        let content = std::fs::read_to_string(&out_csv).expect("csv must exist");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 2, "header + 1 row expected");
        let (_symbol, _date, nums) = parse_csv_row(lines[1]);
        assert!(
            (nums[0] - 13.5).abs() < 1e-9,
            "open must be forward-adjusted (9 * 1.5 = 13.5), got {}",
            nums[0]
        );
        assert!(
            (nums[1] - 16.5).abs() < 1e-9,
            "high must be forward-adjusted (11 * 1.5 = 16.5), got {}",
            nums[1]
        );
        assert!(
            (nums[2] - 12.0).abs() < 1e-9,
            "low must be forward-adjusted (8 * 1.5 = 12.0), got {}",
            nums[2]
        );
        assert!(
            (nums[3] - 15.0).abs() < 1e-9,
            "close must be adjusted to adjclose 15.0, not the raw 10.0, got {}",
            nums[3]
        );
        assert!(
            (nums[4] - 15.0).abs() < 1e-9,
            "adjclose column must equal the adjusted close, got {}",
            nums[4]
        );
        assert!(
            (nums[5] - 777000.0).abs() < 1e-9,
            "volume must pass through verbatim, got {}",
            nums[5]
        );
    }

    #[tokio::test]
    async fn run_export_csv_corrupt_input_parquet_does_not_panic() {
        // Corrupt stock_daily.parquet with no symbols.txt: list_symbols falls
        // back to reading the parquet, which must fail cleanly (warn + return)
        // like the duckdb branch — never panic.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        std::fs::write(
            input_tmp.path().join("stock_daily.parquet"),
            b"this is not a parquet file",
        )
        .expect("write corrupt parquet");

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_csv = out_tmp.path().join("bad.csv");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            out_csv.clone(),
            true,
        )
        .await;
    }

    #[tokio::test]
    async fn run_export_parquet_dir_missing_index_basic_not_fabricated() {
        // index_basic.parquet absent in the input: the export must succeed,
        // must NOT fabricate the file, and the rest of the layout must stay
        // intact and re-readable via ParquetReader.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                15.0,
                777000.0,
                0.0,
            )],
        );
        write_index_parquet(
            input_tmp.path(),
            "index_daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, amount DOUBLE, adjclose DOUBLE)",
            "INSERT INTO t VALUES ('SH000001', 'official', '2026-01-05', 3000, 3002, 2998, 3001, 1e8, 1e10, 3001)",
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;

        assert!(
            out_dir.join("stock_daily.parquet").exists(),
            "stock_daily.parquet must be exported"
        );
        assert!(
            out_dir.join("index_daily.parquet").exists(),
            "index_daily.parquet present in the input must be exported"
        );
        assert!(
            !out_dir.join("index_basic.parquet").exists(),
            "index_basic.parquet absent in the input must not be fabricated"
        );

        let reader = ParquetReader::new(&out_dir).expect("reopen exported dir");
        let bars = reader
            .fetch_bars(
                "SZ000001",
                "1d",
                chrono::DateTime::from_timestamp(0, 0).expect("epoch"),
                chrono::Utc::now(),
                "qfq",
            )
            .await
            .expect("fetch_bars must read the exported stock_daily.parquet");
        assert_eq!(bars.len(), 1, "exported row must be readable intact");
        assert!(
            (bars[0].close - 15.0).abs() < 1e-9,
            "exported bar must read back forward-adjusted (close == adjclose 15.0), got {}",
            bars[0].close
        );
    }

    #[tokio::test]
    async fn run_export_parquet_dir_empty_index_daily_does_not_block_export() {
        // An empty index_daily.parquet must not fail the whole export (same
        // skip-empty semantics as the duckdb branch).
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                10.0,
                1000.0,
                0.0,
            )],
        );
        write_index_parquet(
            input_tmp.path(),
            "index_daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, index_type VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, volume DOUBLE, amount DOUBLE, adjclose DOUBLE)",
            "",
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;

        let reader = ParquetReader::new(&out_dir).expect("reopen exported dir");
        let bars = reader
            .fetch_bars(
                "SZ000001",
                "1d",
                chrono::DateTime::from_timestamp(0, 0).expect("epoch"),
                chrono::Utc::now(),
                "qfq",
            )
            .await
            .expect("stock rows must survive an empty index parquet");
        assert_eq!(bars.len(), 1);
    }

    #[tokio::test]
    async fn run_export_parquet_dir_overwrite_replaces_stale_data() {
        // Round 1 exports SZ000001 into out_dir; round 2 exports SH600000 into
        // the SAME out_dir with overwrite=true. Stale round-1 data must not
        // survive: ParquetReader must see exactly the round-2 data, and the
        // round-1 symbol must be gone.
        let input_a = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_a.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                10.0,
                1000.0,
                0.0,
            )],
        );

        let input_b = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_b.path(),
            &[(
                "SH600000",
                "2024-02-01",
                20.0,
                22.0,
                19.0,
                21.0,
                21.0,
                2000.0,
                0.0,
            )],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        let epoch = chrono::DateTime::from_timestamp(0, 0).expect("epoch");
        let now = chrono::Utc::now();

        run_export(
            input_a.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;
        let reader = ParquetReader::new(&out_dir).expect("reopen after round 1");
        let round1 = reader
            .fetch_bars("SZ000001", "1d", epoch, now, "qfq")
            .await
            .expect("round 1 data must be present after its export");
        assert_eq!(round1.len(), 1, "round 1 must write exactly one row");

        run_export(
            input_b.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;

        let reader = ParquetReader::new(&out_dir).expect("reopen after round 2");
        let symbols = reader.list_symbols().expect("list_symbols after round 2");
        assert_eq!(
            symbols.iter().map(|s| s.code.as_str()).collect::<Vec<_>>(),
            ["SH600000"],
            "overwrite must replace stale stock data, not append or keep it"
        );
        let bars = reader
            .fetch_bars("SH600000", "1d", epoch, now, "qfq")
            .await
            .expect("round 2 data must be present");
        assert_eq!(bars.len(), 1);
        assert!((bars[0].volume - 2000.0).abs() < 1e-9);
        assert!(
            reader
                .fetch_bars("SZ000001", "1d", epoch, now, "qfq")
                .await
                .is_err(),
            "round-1 symbol must be gone after an overwrite export"
        );
    }

    /// #336 review: exporting into the input directory itself must be refused
    /// — a typo'd `--output` must never delete the primary dataset.
    #[tokio::test]
    async fn run_export_rejects_output_inside_input() {
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                8.0,
                1000.0,
                15000.0,
            )],
        );

        // csv: output == input/stock_daily.parquet (would destroy the source).
        let bad_csv = input_tmp.path().join("stock_daily.parquet");
        run_export(
            input_tmp.path().to_path_buf(),
            "csv".to_string(),
            bad_csv.clone(),
            true,
        )
        .await;
        // The source parquet must survive the refused export.
        assert!(
            bad_csv.exists(),
            "refused csv export must not delete the input parquet"
        );
        let reader =
            ParquetReader::new(input_tmp.path()).expect("input must still open after refusal");
        let epoch = chrono::DateTime::from_timestamp(0, 0).expect("epoch");
        assert!(
            reader
                .fetch_bars("SZ000001", "1d", epoch, chrono::Utc::now(), "qfq")
                .await
                .is_ok(),
            "input data must remain readable after a refused export"
        );

        // parquet-dir: output == input directory itself.
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            input_tmp.path().to_path_buf(),
            true,
        )
        .await;
        assert!(
            input_tmp.path().join("stock_daily.parquet").exists(),
            "refused parquet-dir export must not delete the input dataset"
        );
    }

    /// #336 review: overwrite must also drop stale index mirrors (round 1 has
    /// index files, round 2 has none — the old ones must not survive).
    #[tokio::test]
    async fn run_export_parquet_dir_overwrite_removes_stale_index_mirrors() {
        let input_a = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_a.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                8.0,
                1000.0,
                15000.0,
            )],
        );
        // Give input_a its own index parquets so round 1 mirrors them.
        let conn = duckdb::Connection::open_in_memory().expect("open duckdb");
        conn.execute_batch(
            "CREATE TABLE idx (symbol VARCHAR, name VARCHAR); \
             INSERT INTO idx VALUES ('SH000001', '上证指数');",
        )
        .expect("create index table");
        conn.execute_batch(&format!(
            "COPY idx TO '{}' (FORMAT PARQUET)",
            input_a.path().join("index_basic.parquet").display()
        ))
        .expect("copy index_basic");

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");

        run_export(
            input_a.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;
        assert!(
            out_dir.join("index_basic.parquet").exists(),
            "round 1 must mirror index_basic into the output"
        );

        // Round 2: a fresh input with NO index files at all.
        let input_b = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_b.path(),
            &[(
                "SH600000",
                "2024-02-01",
                20.0,
                22.0,
                19.0,
                21.0,
                21.0,
                2000.0,
                0.0,
            )],
        );

        run_export(
            input_b.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;
        assert!(
            !out_dir.join("index_basic.parquet").exists(),
            "round 2 (input without index) must not leave the stale index mirror"
        );
    }

    #[tokio::test]
    async fn run_export_parquet_dir_no_silent_overwrite_of_existing_output() {
        // Minimum standard: an existing output directory must never be
        // silently replaced when overwrite=false.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                10.0,
                1000.0,
                0.0,
            )],
        );

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        std::fs::create_dir_all(&out_dir).expect("mkdir exported");
        let stale = out_dir.join("stock_daily.parquet");
        std::fs::write(&stale, b"STALE OLD BYTES").expect("write stale output");

        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            false,
        )
        .await;

        assert_eq!(
            std::fs::read(&stale).expect("read stale output"),
            b"STALE OLD BYTES",
            "parquet-dir export without overwrite must not silently replace an existing output"
        );
    }

    #[tokio::test]
    async fn run_export_parquet_dir_partial_symbol_failure_writes_served_rows() {
        // symbols.txt lists one served symbol plus a bare code and a missing
        // symbol: partial fetch failures must be skipped (duckdb-branch
        // semantics) without panicking or corrupting the served row.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        write_stock_daily_fixture(
            input_tmp.path(),
            &[(
                "SZ000001",
                "2024-01-02",
                9.0,
                11.0,
                8.0,
                10.0,
                10.0,
                1000.0,
                0.0,
            )],
        );
        std::fs::write(
            input_tmp.path().join("stock_daily.symbols.txt"),
            "SZ000001\n000001\nSZ000002\n",
        )
        .expect("overwrite symbols.txt");

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;

        let reader = ParquetReader::new(&out_dir).expect("reopen exported dir");
        let bars = reader
            .fetch_bars(
                "SZ000001",
                "1d",
                chrono::DateTime::from_timestamp(0, 0).expect("epoch"),
                chrono::Utc::now(),
                "qfq",
            )
            .await
            .expect("the served symbol must survive partial failures");
        assert_eq!(bars.len(), 1);
    }

    #[tokio::test]
    async fn run_export_parquet_dir_corrupt_input_parquet_does_not_panic() {
        // Corrupt stock_daily.parquet with no symbols.txt: the export must
        // fail cleanly (warn + return) like the duckdb branch, never panic.
        let input_tmp = tempfile::tempdir().expect("input tempdir");
        std::fs::write(
            input_tmp.path().join("stock_daily.parquet"),
            b"this is not a parquet file",
        )
        .expect("write corrupt parquet");

        let out_tmp = tempfile::tempdir().expect("output tempdir");
        let out_dir = out_tmp.path().join("exported");
        run_export(
            input_tmp.path().to_path_buf(),
            "parquet-dir".to_string(),
            out_dir.clone(),
            true,
        )
        .await;
    }
}
