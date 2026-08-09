//! Data quality validation helpers for the import commands (issue #136).
//!
//! The import pipeline must prove that what landed in Parquet matches what
//! was in the Dolt source. These functions cover three checks:
//!
//! - **Row counts**: [`dolt_count`] vs [`parquet_row_count`], compared by
//!   [`verify_row_count`].
//! - **Date ranges**: [`dolt_date_range`] vs [`parquet_date_range`], compared
//!   by [`verify_date_range`].
//! - **Freshness**: [`data_updates_last_report_date`] against [`today_cn`]
//!   via [`freshness_days`].
//!
//! Verification functions are pure logic; the readers talk to Dolt
//! (`dolt sql -r csv`, see [`crate::import_dolt::run_dolt_sql_csv`]) or to
//! Parquet via an in-memory DuckDB.

use std::path::Path;

/// Count the data rows in a Parquet file via an in-memory DuckDB.
///
/// Promoted from the test helper at `import_compass.rs:1016-1024`: DuckDB's
/// `read_parquet` handles the physical layout, so the count is exact
/// regardless of row groups or compression.
pub fn parquet_row_count(path: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let conn = duckdb::Connection::open_in_memory()?;
    let count: usize = conn.query_row(
        &format!("SELECT COUNT(*) FROM read_parquet('{}')", path.display()),
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Return the `[min, max]` date range of `col` in a Parquet file.
///
/// The column is cast to DATE so TIMESTAMP columns (e.g. `tradedate` in
/// stock_daily, see kb/design/data-providers.md) are normalized to
/// `YYYY-MM-DD`; MIN/MAX are then cast to VARCHAR. The outer cast is
/// required: without the duckdb `chrono` feature, DATE results come back as
/// `Date32` values that duckdb-rs refuses to decode as `String` — VARCHAR
/// arrives as text and round-trips cleanly. An empty file yields NULL
/// MIN/MAX, reported as `None`.
pub fn parquet_date_range(
    path: &Path,
    col: &str,
) -> Result<(Option<String>, Option<String>), Box<dyn std::error::Error>> {
    let conn = duckdb::Connection::open_in_memory()?;
    let (min, max) = conn.query_row(
        &format!(
            "SELECT CAST(MIN(CAST({col} AS DATE)) AS VARCHAR), \
             CAST(MAX(CAST({col} AS DATE)) AS VARCHAR) FROM read_parquet('{}')",
            path.display()
        ),
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((min, max))
}

/// Count rows in a Dolt table via `dolt sql -r csv`.
///
/// `where_clause` may be empty; the trailing space in the interpolated SQL is
/// harmless. The CSV (header + one data line) is parsed **strictly**: unlike
/// the `.unwrap_or(0)` pattern at `import_dolt.rs:306-310`, a malformed
/// count is an error, not a silent 0 that would mask a real mismatch.
pub fn dolt_count(
    dolt_dir: &Path,
    table: &str,
    where_clause: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let csv = crate::import_dolt::run_dolt_sql_csv(
        dolt_dir,
        &format!("SELECT COUNT(*) AS cnt FROM {table} {where_clause}"),
    )
    .map_err(|e| format!("dolt count query failed: {e}"))?;
    parse_count_csv(&csv).map_err(|e| e.into())
}

/// Parse a `COUNT(*)` CSV (header + one data line) into a row count.
///
/// Returns an error for malformed output instead of silently defaulting to
/// 0 — a false 0 would let a truncated export pass validation.
fn parse_count_csv(csv: &str) -> Result<usize, String> {
    let line = csv
        .lines()
        .nth(1)
        .ok_or_else(|| format!("no data row in COUNT output: {csv:?}"))?;
    line.trim()
        .parse::<usize>()
        .map_err(|e| format!("COUNT output is not a number ('{line}'): {e}"))
}

/// Return the `[min, max]` date range of `col` in a Dolt table.
///
/// `where_clause` may be empty (trailing space in the SQL is harmless).
/// NULL MIN/MAX (empty table or all-NULL column) are reported as `None`.
pub fn dolt_date_range(
    dolt_dir: &Path,
    table: &str,
    where_clause: &str,
    col: &str,
) -> Result<(Option<String>, Option<String>), Box<dyn std::error::Error>> {
    let csv = crate::import_dolt::run_dolt_sql_csv(
        dolt_dir,
        &format!("SELECT MIN({col}), MAX({col}) FROM {table} {where_clause}"),
    )
    .map_err(|e| format!("dolt date range query failed: {e}"))?;
    parse_date_range_csv(&csv).map_err(|e| e.into())
}

/// Parse a `SELECT MIN(col), MAX(col)` CSV (header + one data line with two
/// comma-separated fields) into an `(Option, Option)` pair.
///
/// Dolt renders NULL as an empty CSV field; both empty and the literal
/// `NULL` map to `None`.
fn parse_date_range_csv(csv: &str) -> Result<(Option<String>, Option<String>), String> {
    let line = csv
        .lines()
        .nth(1)
        .ok_or_else(|| format!("no data row in MIN/MAX output: {csv:?}"))?;
    let mut fields = line.split(',');
    let min = fields
        .next()
        .ok_or_else(|| format!("malformed MIN/MAX row: {line:?}"))?;
    let max = fields
        .next()
        .ok_or_else(|| format!("malformed MIN/MAX row: {line:?}"))?;
    Ok((dolt_value(min), dolt_value(max)))
}

/// Map a Dolt CSV field to `Some` value, or `None` for NULL/empty.
fn dolt_value(field: &str) -> Option<String> {
    match field.trim() {
        "" | "NULL" => None,
        v => Some(v.to_string()),
    }
}

/// Read the freshness marker `last_report_date` for `table` from the
/// `data_updates` table.
///
/// Missing row / NULL / empty all report `Ok(None)`. **Any** query error
/// also reports `Ok(None)` by design: a fresh Dolt repo without the
/// `UPDATES_SCHEMA` has no `data_updates` table, and a freshness check must
/// never fail the import (Q5 decision — stale data only warns). A broken
/// `dolt_dir` surfaces later when the import itself queries Dolt.
pub fn data_updates_last_report_date(
    dolt_dir: &Path,
    table: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let csv = match crate::import_dolt::run_dolt_sql_csv(
        dolt_dir,
        &format!("SELECT last_report_date FROM data_updates WHERE table_name = '{table}'"),
    ) {
        Ok(csv) => csv,
        Err(_) => return Ok(None),
    };
    Ok(csv.lines().nth(1).map(str::trim).and_then(dolt_value))
}

/// Today's date in the Asia/Shanghai timezone (CN trading calendar).
///
/// China observes no DST, so a fixed UTC+8 offset is exact — `UTC now + 8h`
/// then take the date. Using the naive UTC date directly would be off by one
/// day for ~16 hours of every UTC day (ref #136, Metis BLOCKING#5).
pub fn today_cn() -> chrono::NaiveDate {
    (chrono::Utc::now() + chrono::Duration::hours(8)).date_naive()
}

/// Verify the Dolt source row count equals the exported Parquet row count.
///
/// Pure logic, no I/O. The error message format is part of the test
/// contract — do not reword it.
pub fn verify_row_count(
    dolt_count: usize,
    parquet_count: usize,
    table: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if dolt_count == parquet_count {
        Ok(())
    } else {
        Err(format!(
            "row count mismatch: dolt={dolt_count} parquet={parquet_count} (table {table})"
        )
        .into())
    }
}

/// Verify the Dolt and Parquet `(min, max)` date ranges are identical.
///
/// Pure logic, no I/O. Both-`None` ranges are consistent (empty vs empty).
/// The error message contains "date range mismatch" and the table name —
/// both are asserted by tests; `None` renders as empty.
pub fn verify_date_range(
    dolt: (Option<String>, Option<String>),
    parquet: (Option<String>, Option<String>),
    table: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if dolt == parquet {
        return Ok(());
    }
    Err(format!(
        "date range mismatch: dolt={}..{} parquet={}..{} (table {table})",
        opt_date_str(&dolt.0),
        opt_date_str(&dolt.1),
        opt_date_str(&parquet.0),
        opt_date_str(&parquet.1),
    )
    .into())
}

/// Render an optional date for error messages: `None` shows as empty.
fn opt_date_str(d: &Option<String>) -> &str {
    match d {
        Some(v) => v,
        None => "",
    }
}

/// Days between `last_report_date` (YYYY-MM-DD) and `today`.
///
/// A future `last_report_date` (test fixtures use e.g. 2099-12-31) clamps to
/// 0 — a future date is "fresh", never a negative staleness. Malformed input
/// is an error: silently treating garbage as fresh would mask a broken
/// pipeline.
pub fn freshness_days(
    last_report_date: &str,
    today: chrono::NaiveDate,
) -> Result<i64, Box<dyn std::error::Error>> {
    let report_date = chrono::NaiveDate::parse_from_str(last_report_date, "%Y-%m-%d")?;
    Ok((today - report_date).num_days().max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    /// Write a parquet file from an in-memory DuckDB table. `ddl` must create
    /// a table named `t`; `values_sql` is the full INSERT statement (may be
    /// empty for a 0-row file).
    fn write_parquet(dir: &Path, name: &str, ddl: &str, values_sql: &str) -> PathBuf {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(ddl).expect("create table");
        if !values_sql.is_empty() {
            conn.execute_batch(values_sql).expect("insert rows");
        }
        let path = dir.join(name);
        conn.execute_batch(&format!("COPY t TO '{}' (FORMAT PARQUET)", path.display()))
            .expect("copy to parquet");
        path
    }

    fn setup_dolt(dir: &Path) {
        for (key, val) in [("user.email", "test@compass.local"), ("user.name", "Test")] {
            let out = Command::new("dolt")
                .arg("config")
                .arg("--global")
                .arg("--add")
                .arg(key)
                .arg(val)
                .output()
                .expect("dolt config");
            assert!(
                out.status.success(),
                "dolt config {key} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let init = Command::new("dolt")
            .arg("--data-dir")
            .arg(dir)
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            init.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
    }

    fn dolt_sql(dolt_dir: &Path, sql: &str) {
        let out = Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_dir)
            .arg("sql")
            .arg("-q")
            .arg(sql)
            .output()
            .expect("dolt sql");
        assert!(
            out.status.success(),
            "dolt sql failed: {}\nsql: {sql}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn parquet_row_count_counts_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_parquet(
            dir.path(),
            "daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP)",
            "INSERT INTO t VALUES ('SH600519','2026-07-01 09:30:00'), \
             ('SH600519','2026-07-02 09:30:00'), ('SZ000001','2026-07-03 09:30:00')",
        );
        assert_eq!(parquet_row_count(&path).expect("row count"), 3);
    }

    #[test]
    fn parquet_row_count_empty_file_is_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_parquet(
            dir.path(),
            "empty.parquet",
            "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP)",
            "",
        );
        assert_eq!(parquet_row_count(&path).expect("row count"), 0);
    }

    #[test]
    fn parquet_row_count_missing_file_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            parquet_row_count(&dir.path().join("nope.parquet")).is_err(),
            "missing parquet must error, not return a count"
        );
    }

    #[test]
    fn parquet_date_range_normalizes_timestamp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_parquet(
            dir.path(),
            "daily.parquet",
            "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP)",
            "INSERT INTO t VALUES ('SH600519','2026-07-01 09:30:00'), \
             ('SH600519','2026-07-31 15:00:00')",
        );
        assert_eq!(
            parquet_date_range(&path, "tradedate").expect("date range"),
            (Some("2026-07-01".into()), Some("2026-07-31".into()))
        );
    }

    #[test]
    fn parquet_date_range_empty_file_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_parquet(
            dir.path(),
            "empty.parquet",
            "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP)",
            "",
        );
        assert_eq!(
            parquet_date_range(&path, "tradedate").expect("date range"),
            (None, None)
        );
    }

    #[test]
    fn dolt_count_returns_row_count() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE t (symbol VARCHAR(20), tradedate DATE)",
        );
        dolt_sql(
            tmp.path(),
            "INSERT INTO t VALUES ('SH600519','2026-07-01'), \
             ('SH600519','2026-07-02'), ('SZ000001','2026-07-03')",
        );
        assert_eq!(
            dolt_count(tmp.path(), "t", "").expect("count"),
            3,
            "empty where_clause must work"
        );
        assert_eq!(
            dolt_count(tmp.path(), "t", "WHERE symbol = 'SH600519'").expect("count"),
            2
        );
    }

    #[test]
    fn dolt_count_missing_table_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        assert!(
            dolt_count(tmp.path(), "no_such_table", "").is_err(),
            "a failed query must propagate as Err"
        );
    }

    #[test]
    fn parse_count_csv_strictly_rejects_malformed_output() {
        // The strict-parse contract: a non-numeric count is an error, never
        // a silent 0 (which would pass validation on a truncated export).
        assert!(parse_count_csv("cnt\nabc\n").is_err());
        assert!(parse_count_csv("cnt\n").is_err(), "no data line");
        assert_eq!(parse_count_csv("cnt\n123\n").expect("count"), 123);
    }

    #[test]
    fn dolt_date_range_returns_min_max_with_nulls() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE t (symbol VARCHAR(20), tradedate DATE)",
        );
        dolt_sql(
            tmp.path(),
            "INSERT INTO t VALUES ('SH600519','2026-07-01'), \
             ('SH600519',NULL), ('SH600519','2026-07-31')",
        );
        assert_eq!(
            dolt_date_range(tmp.path(), "t", "", "tradedate").expect("date range"),
            (Some("2026-07-01".into()), Some("2026-07-31".into()))
        );
    }

    #[test]
    fn dolt_date_range_all_null_is_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE t (symbol VARCHAR(20), tradedate DATE)",
        );
        dolt_sql(tmp.path(), "INSERT INTO t VALUES ('SH600519',NULL)");
        assert_eq!(
            dolt_date_range(tmp.path(), "t", "", "tradedate").expect("date range"),
            (None, None)
        );
    }

    #[test]
    fn parse_date_range_csv_maps_empty_and_null_to_none() {
        assert_eq!(
            parse_date_range_csv("min,max\n2026-07-01,2026-07-31\n").expect("range"),
            (Some("2026-07-01".into()), Some("2026-07-31".into()))
        );
        assert_eq!(
            parse_date_range_csv("min,max\n,\n").expect("range"),
            (None, None)
        );
        assert_eq!(
            parse_date_range_csv("min,max\nNULL,2026-07-31\n").expect("range"),
            (None, Some("2026-07-31".into()))
        );
        assert!(parse_date_range_csv("min,max\n").is_err(), "no data line");
    }

    #[test]
    fn data_updates_missing_table_is_ok_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        assert_eq!(
            data_updates_last_report_date(tmp.path(), "stock_daily").expect("freshness"),
            None,
            "a repo without data_updates must not fail the import"
        );
    }

    #[test]
    fn data_updates_null_and_missing_row_are_ok_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE data_updates (table_name VARCHAR(50), \
             last_report_date DATE)",
        );
        // No row at all.
        assert_eq!(
            data_updates_last_report_date(tmp.path(), "stock_daily").expect("freshness"),
            None
        );
        // Row with NULL last_report_date.
        dolt_sql(
            tmp.path(),
            "INSERT INTO data_updates (table_name, last_report_date) \
             VALUES ('stock_daily', NULL)",
        );
        assert_eq!(
            data_updates_last_report_date(tmp.path(), "stock_daily").expect("freshness"),
            None
        );
    }

    #[test]
    fn data_updates_returns_report_date() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE data_updates (table_name VARCHAR(50), \
             last_report_date DATE)",
        );
        dolt_sql(
            tmp.path(),
            "INSERT INTO data_updates (table_name, last_report_date) \
             VALUES ('stock_daily', '2026-07-31')",
        );
        assert_eq!(
            data_updates_last_report_date(tmp.path(), "stock_daily").expect("freshness"),
            Some("2026-07-31".into())
        );
    }

    #[test]
    fn today_cn_is_within_one_day_of_utc_now() {
        let today = today_cn();
        let utc = chrono::Utc::now().date_naive();
        let diff = (today - utc).num_days().abs();
        assert!(
            diff <= 1,
            "today_cn() = {today} differs from utc {utc} by {diff} days"
        );
        assert_eq!(
            today.format("%Y-%m-%d").to_string().len(),
            10,
            "today_cn must format as YYYY-MM-DD"
        );
    }

    #[test]
    fn verify_row_count_ok_on_match() {
        verify_row_count(3, 3, "stock_daily").expect("matching counts are fine");
    }

    #[test]
    fn verify_row_count_reports_exact_mismatch_message() {
        let err = verify_row_count(10, 8, "stock_daily")
            .expect_err("mismatch must error")
            .to_string();
        assert_eq!(
            err,
            "row count mismatch: dolt=10 parquet=8 (table stock_daily)"
        );
    }

    #[test]
    fn verify_date_range_both_none_is_ok() {
        verify_date_range((None, None), (None, None), "stock_daily")
            .expect("empty vs empty is consistent");
    }

    #[test]
    fn verify_date_range_ok_on_equal_ranges() {
        let range = (Some("2026-07-01".into()), Some("2026-07-31".into()));
        verify_date_range(range.clone(), range, "stock_daily").expect("identical ranges are fine");
    }

    #[test]
    fn verify_date_range_reports_mismatch() {
        let dolt = (Some("2026-07-01".into()), Some("2026-07-31".into()));
        let parquet = (Some("2026-07-02".into()), Some("2026-07-31".into()));
        let err = verify_date_range(dolt, parquet, "stock_daily")
            .expect_err("mismatch must error")
            .to_string();
        assert!(
            err.contains("date range mismatch"),
            "message must contain 'date range mismatch': {err}"
        );
        assert!(
            err.contains("stock_daily"),
            "message must name the table: {err}"
        );
        assert!(
            err.contains("2026-07-01..2026-07-31") && err.contains("2026-07-02..2026-07-31"),
            "message must show both ranges: {err}"
        );
    }

    #[test]
    fn freshness_days_normal() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 31).expect("date");
        assert_eq!(freshness_days("2026-07-01", today).expect("days"), 30);
        assert_eq!(freshness_days("2026-07-31", today).expect("days"), 0);
    }

    #[test]
    fn freshness_days_future_date_clamps_to_zero() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 31).expect("date");
        assert_eq!(
            freshness_days("2099-12-31", today).expect("days"),
            0,
            "a future report date is fresh, never negative"
        );
    }

    #[test]
    fn freshness_days_rejects_malformed_date() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 31).expect("date");
        assert!(
            freshness_days("not-a-date", today).is_err(),
            "garbage input must error, not report 0"
        );
        assert!(
            freshness_days("2026/07/31", today).is_err(),
            "wrong format must error"
        );
    }
}
