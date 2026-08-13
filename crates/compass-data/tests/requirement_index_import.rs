//! Requirement-acceptance tests for C2 (epic #255, plan T3): CompassTable
//! `IndexDaily` / `IndexBasic` import + export semantics.
//!
//! The adversarial suite (`index_import_compass.rs`) proves the *FromStr
//! surface* — `"index_daily".parse::<CompassTable>()` accepted, typos
//! rejected. This file proves the *export semantics* declared by the plan's
//! acceptance criteria:
//! - incremental merge: a `--since` re-run must NOT lose old parquet rows
//!   (PK `(symbol, trade_date)` merge, plan QA: "增量 merge（--since 后新行
//!   并入不丢旧行）")
//! - export column contract: `index_daily.parquet` carries `index_type` +
//!   `adjclose` (= close), row count == Dolt source (plan T3)
//! - `IndexBasic` is a full overwrite: a re-run reflects the current Dolt
//!   state (deleted rows disappear, plan T3 "全量覆盖")
//!
//! RED vs current code: `"index_daily".parse::<CompassTable>()` is Err, so
//! every test panics at the `.expect(...)` on the parse. The tests compile
//! today (the enum *variants* are not referenced anywhere) and turn GREEN
//! once T3 lands the variants + routing.

use std::path::{Path, PathBuf};
use std::process::Command;

use compass_data::import_compass::{self, CompassTable};
use duckdb::Connection;

const INDEX_DAILY_DDL: &str = "CREATE TABLE index_daily (\
     symbol VARCHAR(20) NOT NULL, \
     trade_date DATE NOT NULL, \
     index_type VARCHAR(20) NOT NULL, \
     open DOUBLE, close DOUBLE, high DOUBLE, low DOUBLE, \
     volume DOUBLE, amount DOUBLE, update_date DATE, \
     PRIMARY KEY (symbol, trade_date))";

const INDEX_BASIC_DDL: &str = "CREATE TABLE index_basic (\
     symbol VARCHAR(20) NOT NULL PRIMARY KEY, \
     name VARCHAR(100), index_type VARCHAR(20))";

fn setup_dolt(dir: &Path) {
    for (key, val) in [
        ("user.email", "req@compass.local"),
        ("user.name", "ReqTest"),
    ] {
        let out = Command::new("dolt")
            .arg("config")
            .arg("--global")
            .arg("--add")
            .arg(key)
            .arg(val)
            .output()
            .expect("dolt config");
        assert!(out.status.success());
    }
    let init = Command::new("dolt")
        .arg("--data-dir")
        .arg(dir)
        .arg("init")
        .output()
        .expect("dolt init");
    assert!(init.status.success());
}

fn dolt_sql(dir: &Path, sql: &str) {
    let out = Command::new("dolt")
        .arg("--data-dir")
        .arg(dir)
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

/// Build a multi-row INSERT for index_daily. Every row gets a distinct
/// trade_date so the exported parquet is large enough to clear the
/// append-table skip guard (new_data.len() < 500 bytes) and so the `--since`
/// filter splits the batches deterministically.
fn insert_index_daily_rows(dir: &Path, start_day: i64, count: i64, symbol: &str, index_type: &str) {
    let base = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("valid date");
    let mut sql = String::from(
        "INSERT INTO index_daily (symbol, trade_date, index_type, open, close, \
         high, low, volume, amount, update_date) VALUES ",
    );
    for i in 0..count {
        let day = (base + chrono::Duration::days(start_day + i)).format("%Y-%m-%d");
        if i > 0 {
            sql.push(',');
        }
        sql.push_str(&format!(
            "('{symbol}', '{day}', '{index_type}', 3000.0, 3001.0, 3002.0, 2998.0, \
             120000000.0, 50000000000.0, '2026-08-02')"
        ));
    }
    dolt_sql(dir, &sql);
}

fn parquet_row_count(path: &Path) -> i64 {
    let conn = Connection::open_in_memory().expect("duckdb");
    conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM read_parquet('{}')",
            path.display().to_string().replace('\'', "''")
        ),
        [],
        |row| row.get(0),
    )
    .expect("count parquet rows")
}

/// Parse `index_daily` through the public CLI surface. RED: this panics
/// until T3 adds the variant.
fn parse_index_daily() -> CompassTable {
    "index_daily"
        .parse::<CompassTable>()
        .expect("CompassTable must accept 'index_daily' (plan T3)")
}

fn parse_index_basic() -> CompassTable {
    "index_basic"
        .parse::<CompassTable>()
        .expect("CompassTable must accept 'index_basic' (plan T3)")
}

// ---------------------------------------------------------------------------
// Incremental merge: old rows survive a --since re-run
// ---------------------------------------------------------------------------

#[test]
fn index_daily_incremental_merge_keeps_old_rows() {
    // Plan QA happy path: 增量 merge（--since 后新行并入不丢旧行）.
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_DAILY_DDL);

    // 600 rows before the increment window (distinct dates → parquet > 500 B).
    insert_index_daily_rows(dolt_tmp.path(), 0, 600, "SH000001", "official");

    let out_tmp = tempfile::tempdir().expect("output tmp");
    let table = parse_index_daily(); // RED: panics here today
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        table,
        false,
        None,
    )
    .expect("full export");
    let parquet = out_tmp.path().join("index_daily.parquet");
    assert_eq!(parquet_row_count(&parquet), 600, "full export row count");

    // 200 new rows dated after the increment window.
    insert_index_daily_rows(dolt_tmp.path(), 1000, 200, "SH000001", "official");
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        table,
        false,
        Some("2026-09-01"),
    )
    .expect("incremental merge");

    assert_eq!(
        parquet_row_count(&parquet),
        800,
        "incremental merge must keep the 600 old rows and add the 200 new ones"
    );
}

// ---------------------------------------------------------------------------
// Export column contract: index_type + adjclose(= close)
// ---------------------------------------------------------------------------

#[test]
fn index_daily_export_column_contract() {
    // Plan T3: parquet 含 index_type + adjclose=close 占位列，行数 = 源.
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_DAILY_DDL);
    insert_index_daily_rows(dolt_tmp.path(), 0, 600, "SH000001", "official");
    insert_index_daily_rows(dolt_tmp.path(), 0, 600, "BK0475", "concept");

    let out_tmp = tempfile::tempdir().expect("output tmp");
    let table = parse_index_daily(); // RED: panics here today
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        table,
        false,
        None,
    )
    .expect("export");
    let parquet = out_tmp.path().join("index_daily.parquet");

    assert_eq!(
        parquet_row_count(&parquet),
        1200,
        "row count == Dolt source"
    );

    // Column presence is proven by the SELECT compiling; index_type survives
    // and adjclose == close on every sample row.
    let conn = Connection::open_in_memory().expect("duckdb");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT index_type, adjclose, close FROM read_parquet('{}') \
             WHERE symbol = ? LIMIT 3",
            parquet.display().to_string().replace('\'', "''")
        ))
        .expect("parquet must expose index_type + adjclose + close columns");
    let rows = stmt
        .query_map(duckdb::params!["SH000001"], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    assert_eq!(rows.len(), 3);
    for (index_type, adjclose, close) in rows {
        assert_eq!(
            index_type, "official",
            "index_type column must carry the tag"
        );
        assert!(
            (adjclose - close).abs() < 1e-9,
            "adjclose must equal close (placeholder); got adjclose={adjclose} close={close}"
        );
    }
}

// ---------------------------------------------------------------------------
// IndexBasic: full overwrite
// ---------------------------------------------------------------------------

#[test]
fn index_basic_full_overwrite_reflects_dolt_state() {
    // Plan T3: IndexBasic 全量覆盖（仿 ConceptMember）— a re-run must mirror
    // the current Dolt state: rows deleted upstream disappear from parquet.
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_BASIC_DDL);
    for (sym, name, idx_type) in [
        ("BK0475", "半导体", "concept"),
        ("BK0476", "白酒", "industry"),
        ("SH000001", "上证指数", "official"),
    ] {
        dolt_sql(
            dolt_tmp.path(),
            &format!(
                "INSERT INTO index_basic (symbol, name, index_type) VALUES \
                 ('{sym}', '{name}', '{idx_type}')"
            ),
        );
    }

    let out_tmp = tempfile::tempdir().expect("output tmp");
    let table = parse_index_basic(); // RED: panics here today
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        table,
        false,
        None,
    )
    .expect("first export");
    let parquet = out_tmp.path().join("index_basic.parquet");
    assert_eq!(parquet_row_count(&parquet), 3);

    // Upstream removes a board → the overwrite must drop it from parquet.
    dolt_sql(
        dolt_tmp.path(),
        "DELETE FROM index_basic WHERE symbol = 'BK0476'",
    );
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        table,
        false,
        None,
    )
    .expect("re-export");
    assert_eq!(
        parquet_row_count(&parquet),
        2,
        "full overwrite must reflect the current Dolt state (deleted row gone)"
    );

    // The remaining rows still carry name + index_type for the picker.
    let conn = Connection::open_in_memory().expect("duckdb");
    let name: String = conn
        .query_row(
            &format!(
                "SELECT name FROM read_parquet('{}') WHERE symbol = 'BK0475'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("name column");
    assert_eq!(name, "半导体", "index_basic name must survive the export");
}

// ---------------------------------------------------------------------------
// Guard: existing tables keep parsing (no regression on the enum surface)
// ---------------------------------------------------------------------------

#[test]
fn existing_compass_tables_unaffected() {
    assert!("stock_basic".parse::<CompassTable>().is_ok());
    assert!("concept_member".parse::<CompassTable>().is_ok());
    assert!("capital_main_flow".parse::<CompassTable>().is_ok());
    assert!("index_daily_typo".parse::<CompassTable>().is_err());
}

// Silence unused-import warnings for PathBuf when no test needs it directly.
#[allow(dead_code)]
fn _type_check(_: PathBuf) {}
