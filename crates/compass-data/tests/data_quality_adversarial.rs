//! Adversarial tests for the data-quality validation pipeline (issue #136).
//!
//! These assertions exercise the *public* API of the validation module and
//! the import entry points (`validate.rs`, `import_dolt::run`,
//! `import_compass::run`) through the lib target. They attack the six
//! adversarial dimensions — boundary values, error paths, invalid inputs,
//! concurrency races, resource exhaustion — against the plan's declared
//! commitments.
//!
//! Rationale for living in `tests/` instead of the in-source `#[cfg(test)]`
//! modules: the sandbox denies writes to `src/**` (edit/write only allowed
//! under `**/tests/**`), mirroring the precedent documented in
//! `crates/compass/tests/adversarial_219_fork_formats.rs`. The covered
//! functions are all `pub`, so integration tests reach them without any
//! test-only surface changes.
//!
//! All tests are expected GREEN: the implementation landed at commits
//! ec4985c (validate.rs) and dec0e32 (import wiring). A RED means a hidden
//! defect was found.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use compass_data::import_compass::{self, CompassTable};
use compass_data::import_dolt;
use compass_data::validate;

/// Full fin_indicators schema matching the production Dolt DDL (mirrors the
/// in-source test fixture); the fixed SELECT in `import_fin_indicators` does
/// not export `eitime`, but the schema stays aligned with production.
const FIN_SCHEMA: &str = "\
    CREATE TABLE fin_indicators (\
    symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
    update_date DATE, notice_date DATE, \
    data_type VARCHAR(20), qdate VARCHAR(8), eitime DATETIME, data_year INT, date_label VARCHAR(10), \
    secucode VARCHAR(20), name VARCHAR(100), \
    trade_market VARCHAR(20), trade_market_code VARCHAR(20), trade_market_zjg VARCHAR(10), \
    security_type VARCHAR(10), security_type_code VARCHAR(20), industry VARCHAR(50), \
    board_code VARCHAR(10), board_name VARCHAR(50), ori_board_code VARCHAR(10), org_code VARCHAR(20), is_new TINYINT, \
    basic_eps DOUBLE, deduct_basic_eps DOUBLE, revenue DOUBLE, net_profit DOUBLE, roe DOUBLE, bps DOUBLE, \
    cash_flow_per_share DOUBLE, gross_margin DOUBLE, \
    revenue_yoy DOUBLE, net_profit_yoy DOUBLE, operating_profit_yoy DOUBLE, net_profit_qoq DOUBLE, \
    shares_growth DOUBLE, dividend_plan TEXT, dividend_year VARCHAR(10), \
    PRIMARY KEY (symbol, report_date))";

/// EOD schema matching `import_dolt::run`'s `final_a_stock_eod_price` query.
const EOD_SCHEMA: &str = "CREATE TABLE final_a_stock_eod_price (\
     symbol VARCHAR(20) NOT NULL, \
     tradedate DATE NOT NULL, \
     open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, \
     adjclose DOUBLE, volume DOUBLE, amount DOUBLE, \
     PRIMARY KEY (symbol, tradedate))";

// ---------------------------------------------------------------------------
// Test infrastructure (self-contained; no dependency on the real Dolt repos)
// ---------------------------------------------------------------------------

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

/// Write a parquet file from an in-memory DuckDB table. `ddl` must create a
/// table named `t`; `values_sql` is the full INSERT (may be empty for a
/// 0-row schema-only file).
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

fn read_parquet_row_count(path: &Path) -> usize {
    let duck = duckdb::Connection::open_in_memory().expect("duckdb");
    duck.query_row(
        &format!("SELECT COUNT(*) FROM read_parquet('{}')", path.display()),
        [],
        |row| row.get(0),
    )
    .expect("count")
}

// ---------------------------------------------------------------------------
// validate.rs — boundary / error-path / invalid-input attacks
// ---------------------------------------------------------------------------

/// Missing parquet file must error in `parquet_date_range` — never fabricate
/// a (None, None) range that would pass an empty-vs-empty date check.
#[test]
fn date_range_missing_file_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(
        validate::parquet_date_range(&dir.path().join("nope.parquet"), "tradedate").is_err(),
        "missing parquet must error"
    );
}

/// Unknown column name is a DuckDB binder error; the reader must propagate it
/// as Err instead of silently returning (None, None) (which would pass a
/// date-range check against an empty source by accident).
#[test]
fn date_range_unknown_column_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_parquet(
        dir.path(),
        "daily.parquet",
        "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP)",
        "INSERT INTO t VALUES ('SH600519','2026-07-01 09:30:00')",
    );
    assert!(
        validate::parquet_date_range(&path, "no_such_column").is_err(),
        "unknown column must error"
    );
}

/// Boundary: 00:00:00 and 23:59:59 on the same day are distinct instants but
/// the same trading date; CAST AS DATE must collapse them so min == max.
#[test]
fn date_range_same_day_timestamps_normalize() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_parquet(
        dir.path(),
        "daily.parquet",
        "CREATE TABLE t (symbol VARCHAR, tradedate TIMESTAMP)",
        "INSERT INTO t VALUES ('SH600519','2026-07-01 00:00:00'), \
         ('SH600519','2026-07-01 23:59:59')",
    );
    assert_eq!(
        validate::parquet_date_range(&path, "tradedate").expect("date range"),
        (Some("2026-07-01".into()), Some("2026-07-01".into())),
        "same-day timestamps must normalize to one date"
    );
}

/// A broken `where_clause` (nonexistent column) must surface as Err in
/// `dolt_count` — never a silent 0 that would pass validation on a filter
/// that produced nothing.
#[test]
fn dolt_count_bad_where_clause_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dolt(tmp.path());
    dolt_sql(
        tmp.path(),
        "CREATE TABLE t (symbol VARCHAR(20), tradedate DATE)",
    );
    dolt_sql(
        tmp.path(),
        "INSERT INTO t VALUES ('SH600519','2026-07-01'), ('SZ000001','2026-07-02')",
    );
    assert!(
        validate::dolt_count(tmp.path(), "t", "WHERE no_such_column = 1").is_err(),
        "a broken where_clause must surface as Err, not silently return 0"
    );
}

/// 0-row table: MIN/MAX are NULLs, reported as (None, None) — the empty
/// range that `verify_date_range` accepts as consistent with an empty
/// parquet.
#[test]
fn dolt_date_range_empty_table_is_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dolt(tmp.path());
    dolt_sql(
        tmp.path(),
        "CREATE TABLE t (symbol VARCHAR(20), tradedate DATE)",
    );
    assert_eq!(
        validate::dolt_date_range(tmp.path(), "t", "", "tradedate").expect("date range"),
        (None, None),
        "empty table must report an empty range"
    );
}

/// `data_updates` exists but lacks `last_report_date` — the query fails and
/// freshness must degrade to Ok(None), never fail the import.
#[test]
fn data_updates_missing_column_is_ok_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dolt(tmp.path());
    dolt_sql(
        tmp.path(),
        "CREATE TABLE data_updates (table_name VARCHAR(50), last_updated DATE)",
    );
    dolt_sql(
        tmp.path(),
        "INSERT INTO data_updates (table_name, last_updated) VALUES ('stock_daily', '2026-07-31')",
    );
    assert_eq!(
        validate::data_updates_last_report_date(tmp.path(), "stock_daily").expect("freshness"),
        None,
        "a query error for a broken schema must degrade to Ok(None)"
    );
}

/// 0 vs 0 is the empty-consistent boundary.
#[test]
fn verify_row_count_zero_vs_zero_is_ok() {
    validate::verify_row_count(0, 0, "stock_daily").expect("empty vs empty is consistent");
}

/// Boundary in both directions: a truncation that produced 0 rows must be
/// caught, and a phantom export must be caught too; the message must name
/// both values and the table.
#[test]
fn verify_row_count_zero_vs_nonzero_reports_values() {
    let err = validate::verify_row_count(0, 3, "stock_daily")
        .expect_err("0 source rows but 3 parquet rows must error")
        .to_string();
    assert_eq!(
        err,
        "row count mismatch: dolt=0 parquet=3 (table stock_daily)"
    );

    let err = validate::verify_row_count(3, 0, "stock_daily")
        .expect_err("3 source rows but 0 parquet rows must error")
        .to_string();
    assert_eq!(
        err,
        "row count mismatch: dolt=3 parquet=0 (table stock_daily)"
    );
}

/// One side empty while the other is populated: a truncation that wiped the
/// parquet date range must error, not pass as "consistent".
#[test]
fn verify_date_range_one_side_none_errors() {
    let dolt = (Some("2026-07-01".into()), Some("2026-07-31".into()));
    let parquet: (Option<String>, Option<String>) = (None, None);
    let err = validate::verify_date_range(dolt, parquet, "stock_daily")
        .expect_err("populated vs empty range must error")
        .to_string();
    assert_eq!(
        err,
        "date range mismatch: dolt=2026-07-01..2026-07-31 parquet=.. (table stock_daily)"
    );
}

/// Exact freshness thresholds from import_compass: market tables warn at >7
/// days, fin tables at >120. A staleness exactly equal to the threshold is
/// not yet stale (strictly greater-than comparison), so `freshness_days`
/// must report the exact value.
#[test]
fn freshness_days_exact_threshold_boundaries() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 31).expect("date");
    assert_eq!(
        validate::freshness_days("2026-07-24", today).expect("days"),
        7,
        "exactly the market threshold (7d) is not stale yet"
    );
    assert_eq!(
        validate::freshness_days("2026-04-02", today).expect("days"),
        120,
        "exactly the fin threshold (120d) is not stale yet"
    );
    assert_eq!(
        validate::freshness_days("2026-07-30", today).expect("days"),
        1,
        "1-day staleness is minimal but positive"
    );
}

// ---------------------------------------------------------------------------
// import_dolt::run — integration boundary attacks
// ---------------------------------------------------------------------------

fn setup_eod(dolt_dir: &Path, rows: &str) {
    dolt_sql(dolt_dir, EOD_SCHEMA);
    dolt_sql(
        dolt_dir,
        &format!("INSERT INTO final_a_stock_eod_price VALUES {rows}"),
    );
}

/// limit == source COUNT (3 of 3): min boundary, must be Ok with exactly 3
/// rows in the parquet.
#[test]
fn import_dolt_limit_equal_to_source_count_ok() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    setup_eod(
        dolt_tmp.path(),
        "('SZ000001','2024-01-02',9,11,8,10,10,1000,0), \
         ('SZ000001','2024-01-03',10,12,9,11,11,1200,0), \
         ('SZ000001','2024-01-04',11,13,10,12,12,1400,0)",
    );

    let output_tmp = tempfile::tempdir().expect("output tmp");
    import_dolt::run(
        dolt_tmp.path().to_path_buf(),
        output_tmp.path().to_path_buf(),
        3, // == source count
        None,
        None,
        None,
        None,
    )
    .expect("limit == source count must be Ok");

    let parquet = output_tmp.path().join("stock_daily.parquet");
    assert_eq!(
        read_parquet_row_count(&parquet),
        3,
        "parquet must hold exactly the source count"
    );
}

/// limit > source COUNT (5 vs 3): expected = min(COUNT, limit) = COUNT, so
/// the parquet must hold the full source and still verify Ok.
#[test]
fn import_dolt_limit_greater_than_source_count_ok() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    setup_eod(
        dolt_tmp.path(),
        "('SZ000001','2024-01-02',9,11,8,10,10,1000,0), \
         ('SZ000001','2024-01-03',10,12,9,11,11,1200,0), \
         ('SZ000001','2024-01-04',11,13,10,12,12,1400,0)",
    );

    let output_tmp = tempfile::tempdir().expect("output tmp");
    import_dolt::run(
        dolt_tmp.path().to_path_buf(),
        output_tmp.path().to_path_buf(),
        5, // > source count
        None,
        None,
        None,
        None,
    )
    .expect("limit > source count must be Ok (expected = min(COUNT, limit))");

    let parquet = output_tmp.path().join("stock_daily.parquet");
    assert_eq!(
        read_parquet_row_count(&parquet),
        3,
        "parquet must hold the full source, not a padded 5 rows"
    );
}

/// `--symbols` filter with a well-formed but non-matching symbol: 0 rows in
/// source and 0 rows in parquet — the 0-vs-0 consistency boundary, must be
/// Ok (not an Err from a mismatched count).
#[test]
fn import_dolt_symbols_filter_no_match_is_ok() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    setup_eod(
        dolt_tmp.path(),
        "('SZ000001','2024-01-02',9,11,8,10,10,1000,0), \
         ('SH600519','2024-01-02',99,101,98,100,100,2000,0)",
    );

    let output_tmp = tempfile::tempdir().expect("output tmp");
    import_dolt::run(
        dolt_tmp.path().to_path_buf(),
        output_tmp.path().to_path_buf(),
        0,
        Some("SH999999"), // valid format, matches nothing
        None,
        None,
        None,
    )
    .expect("0 matching rows must be Ok: 0 vs 0 is consistent");

    let parquet = output_tmp.path().join("stock_daily.parquet");
    assert_eq!(
        read_parquet_row_count(&parquet),
        0,
        "no symbol matched, parquet must be empty"
    );
    let symbols_txt = std::fs::read_to_string(output_tmp.path().join("stock_daily.symbols.txt"))
        .expect("symbols.txt");
    assert_eq!(
        symbols_txt.trim(),
        "",
        "symbols.txt must be empty when no symbol matches"
    );
}

/// `--start-date` beyond all data: filter collapses to an empty set, the
/// 0-vs-0 boundary, must be Ok.
#[test]
fn import_dolt_start_date_filters_to_empty_is_ok() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    setup_eod(
        dolt_tmp.path(),
        "('SZ000001','2024-01-02',9,11,8,10,10,1000,0), \
         ('SZ000001','2024-03-01',12,13,11,12.5,12.5,1500,0)",
    );

    let output_tmp = tempfile::tempdir().expect("output tmp");
    import_dolt::run(
        dolt_tmp.path().to_path_buf(),
        output_tmp.path().to_path_buf(),
        0,
        None,
        Some("20260101"), // after all rows
        None,
        None,
    )
    .expect("empty result from --start-date must be Ok (0 vs 0)");

    let parquet = output_tmp.path().join("stock_daily.parquet");
    assert_eq!(read_parquet_row_count(&parquet), 0);
}

/// `--since` filtering to an empty set: same 0-vs-0 boundary, must be Ok.
#[test]
fn import_dolt_since_filters_to_empty_is_ok() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    setup_eod(
        dolt_tmp.path(),
        "('SZ000001','2024-01-02',9,11,8,10,10,1000,0), \
         ('SZ000001','2024-03-01',12,13,11,12.5,12.5,1500,0)",
    );

    let output_tmp = tempfile::tempdir().expect("output tmp");
    import_dolt::run(
        dolt_tmp.path().to_path_buf(),
        output_tmp.path().to_path_buf(),
        0,
        None,
        None,
        None,
        Some("20260101"), // after all rows
    )
    .expect("empty result from --since must be Ok (0 vs 0)");

    let parquet = output_tmp.path().join("stock_daily.parquet");
    assert_eq!(read_parquet_row_count(&parquet), 0);
}

// ---------------------------------------------------------------------------
// import_compass::run — integration boundary attacks
// ---------------------------------------------------------------------------

/// Merge boundary: old parquet holds 1 row, Dolt updates that same key, the
/// incremental merge keeps exactly 1 row. merged == old must NOT error (the
/// no-loss check only fails on merged < old).
#[test]
fn fin_merge_row_count_equal_to_old_is_ok() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dolt(tmp.path());
    dolt_sql(tmp.path(), FIN_SCHEMA);
    dolt_sql(
        tmp.path(),
        "INSERT INTO fin_indicators (symbol, report_date, revenue, name) \
         VALUES ('SH600519', '2024-12-31', 1.5e11, '贵州茅台')",
    );

    import_compass::run(
        tmp.path().to_path_buf(),
        tmp.path().to_path_buf(),
        CompassTable::FinIndicators,
        false,
        None,
    )
    .expect("full import");
    let parquet = tmp.path().join("fin_indicators.parquet");
    assert_eq!(read_parquet_row_count(&parquet), 1);

    dolt_sql(
        tmp.path(),
        "UPDATE fin_indicators SET revenue = 1.6e11 \
         WHERE symbol = 'SH600519' AND report_date = '2024-12-31'",
    );

    import_compass::run(
        tmp.path().to_path_buf(),
        tmp.path().to_path_buf(),
        CompassTable::FinIndicators,
        false,
        Some("2024-01-01"),
    )
    .expect("merge with merged == old must be Ok (no row loss)");

    assert_eq!(
        read_parquet_row_count(&parquet),
        1,
        "merged row count must equal the old count"
    );
    let duck = duckdb::Connection::open_in_memory().expect("duckdb");
    let revenue: f64 = duck
        .query_row(
            &format!(
                "SELECT revenue FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                parquet.display()
            ),
            [],
            |row| row.get(0),
        )
        .expect("revenue");
    assert!(
        (revenue - 1.5e11).abs() < 1.0,
        "fin merge is old-wins (prefer_new=false): published financial rows never change, got {revenue}"
    );
}

/// Tiny-data skip must fire before any validation: an empty table with
/// `--since` still returns Ok without touching the row-count check.
#[test]
fn fin_income_empty_with_since_skips_ok() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dolt(tmp.path());
    dolt_sql(
        tmp.path(),
        "CREATE TABLE fin_income (\
         symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
         PRIMARY KEY (symbol, report_date))",
    );

    import_compass::run(
        tmp.path().to_path_buf(),
        tmp.path().to_path_buf(),
        CompassTable::FinIncome,
        false,
        Some("2025-01-01"),
    )
    .expect("empty table + since must skip via tiny-data and be Ok");
}

/// Capture warn output locally (does not touch the global default subscriber).
struct TestWriter(Arc<Mutex<String>>);
impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("lock")
            .push_str(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TestWriter {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        TestWriter(self.0.clone())
    }
}

/// stock_basic must NOT run freshness validation: even with a stale
/// data_updates row (200 days old), the import succeeds with no `freshness`
/// warn. Positive control first (fin_indicators with the same stale row must
/// warn) proves the capture mechanism works — no false positive.
#[test]
fn stock_basic_stale_data_updates_does_not_warn() {
    let tmp = tempfile::tempdir().expect("tempdir");
    setup_dolt(tmp.path());

    dolt_sql(
        tmp.path(),
        "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), industry VARCHAR(50), industry_en VARCHAR(50), list_date VARCHAR(20), delist_date DATE, board VARCHAR(50), full_name VARCHAR(200), total_share DOUBLE, region VARCHAR(50))",
    );
    dolt_sql(
        tmp.path(),
        "INSERT INTO stock_basic (symbol, name) VALUES ('SH600519','贵州茅台'), ('SZ000001','平安银行')",
    );
    dolt_sql(tmp.path(), FIN_SCHEMA);
    dolt_sql(
        tmp.path(),
        "INSERT INTO fin_indicators (symbol, report_date, revenue, name) \
         VALUES ('SH600519', '2025-12-31', 1.72e11, '贵州茅台')",
    );
    dolt_sql(
        tmp.path(),
        "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)",
    );
    dolt_sql(
        tmp.path(),
        "INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) \
         VALUES ('fin_indicators', CURDATE(), 'test', 1, DATE_SUB(CURDATE(), INTERVAL 200 DAY)), \
                ('stock_basic', CURDATE(), 'test', 2, DATE_SUB(CURDATE(), INTERVAL 200 DAY))",
    );

    let buf = Arc::new(Mutex::new(String::new()));
    let make_subscriber = || {
        tracing_subscriber::fmt()
            .with_writer(TestWriter(buf.clone()))
            .with_max_level(tracing::Level::WARN)
            .finish()
    };

    tracing::subscriber::with_default(make_subscriber(), || {
        import_compass::run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            None,
        )
        .expect("fin_indicators import must succeed (warn-only)");
    });
    {
        let mut log = buf.lock().expect("lock");
        assert!(
            log.contains("freshness"),
            "positive control failed: stale fin_indicators must warn, got: {log}"
        );
        log.clear();
    }

    tracing::subscriber::with_default(make_subscriber(), || {
        import_compass::run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::StockBasic,
            false,
            None,
        )
        .expect("stock_basic import must succeed");
    });
    {
        let log = buf.lock().expect("lock");
        assert!(
            !log.contains("freshness"),
            "stock_basic must not emit a freshness warn, got: {log}"
        );
    }
}

/// Concurrency regression guard for `unique_work_path` (ref #184): two
/// threads running incremental merges in parallel share the
/// `compass_parquet_work` temp namespace. Each thread's staged parquet must
/// stay intact — a regression to fixed stage-file names would clobber the
/// other thread's merge, leaking foreign symbols into its output.
#[test]
fn concurrent_fin_merges_do_not_clobber_each_other() {
    let d1 = tempfile::tempdir().expect("d1");
    let o1 = tempfile::tempdir().expect("o1");
    setup_dolt(d1.path());
    dolt_sql(d1.path(), FIN_SCHEMA);
    dolt_sql(
        d1.path(),
        "INSERT INTO fin_indicators (symbol, report_date, revenue, name) \
         VALUES ('SH600519', '2024-12-31', 1.5e11, '贵州茅台')",
    );
    import_compass::run(
        d1.path().to_path_buf(),
        o1.path().to_path_buf(),
        CompassTable::FinIndicators,
        false,
        None,
    )
    .expect("thread1 full import");
    dolt_sql(
        d1.path(),
        "INSERT INTO fin_indicators (symbol, report_date, revenue, name) \
         VALUES ('SH600519', '2025-12-31', 1.72e11, '贵州茅台')",
    );

    let d2 = tempfile::tempdir().expect("d2");
    let o2 = tempfile::tempdir().expect("o2");
    setup_dolt(d2.path());
    dolt_sql(d2.path(), FIN_SCHEMA);
    dolt_sql(
        d2.path(),
        "INSERT INTO fin_indicators (symbol, report_date, revenue, name) \
         VALUES ('SZ000001', '2024-12-31', 1.0e11, '平安银行')",
    );
    import_compass::run(
        d2.path().to_path_buf(),
        o2.path().to_path_buf(),
        CompassTable::FinIndicators,
        false,
        None,
    )
    .expect("thread2 full import");
    dolt_sql(
        d2.path(),
        "INSERT INTO fin_indicators (symbol, report_date, revenue, name) \
         VALUES ('SZ000001', '2025-12-31', 1.1e11, '平安银行')",
    );

    let h1 = std::thread::spawn(move || -> Result<(), String> {
        import_compass::run(
            d1.path().to_path_buf(),
            o1.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            Some("2025-01-01"),
        )
        .map_err(|e| format!("thread1 merge failed: {e}"))?;
        let parquet = o1.path().join("fin_indicators.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "thread1 parquet must hold both SH600519 rows"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let foreign: usize = duck
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM read_parquet('{}') WHERE symbol = 'SZ000001'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("foreign count");
        assert_eq!(
            foreign, 0,
            "thread1 parquet must not contain the other thread's rows"
        );
        Ok(())
    });

    let h2 = std::thread::spawn(move || -> Result<(), String> {
        import_compass::run(
            d2.path().to_path_buf(),
            o2.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            Some("2025-01-01"),
        )
        .map_err(|e| format!("thread2 merge failed: {e}"))?;
        let parquet = o2.path().join("fin_indicators.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "thread2 parquet must hold both SZ000001 rows"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let foreign: usize = duck
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("foreign count");
        assert_eq!(
            foreign, 0,
            "thread2 parquet must not contain the other thread's rows"
        );
        Ok(())
    });

    h1.join().expect("thread1 panicked").expect("thread1 merge");
    h2.join().expect("thread2 panicked").expect("thread2 merge");
}
