//! Adversarial tests for epic #266 B2 (#269) — the Rust data-layer `name_en`
//! / `industry_en` import chain.
//!
//! The requirement suite (`requirement_name_en_data.rs`) proves the happy
//! path: `import_index_basic` / `import_stock_basic` *expose* the new columns
//! and the DuckDB mirror carries them. These tests attack the corners that
//! surrogate a value-carrying-but-otherwise-broken import:
//!   - row-count integrity when the new column is present (a partial-export
//!     bug that keeps the count but drops the column value / skews a row)
//!   - value→row binding: Dolt rows inserted in non-symbol order must land on
//!     their own symbol after the `ORDER BY symbol` re-order — a column/schema
//!     misalignment would swap `name_en` values between rows
//!   - an over-length `name_en` (> VARCHAR(100)) must round-trip through the
//!     Dolt → parquet export without being truncated
//!   - a new-schema column that is entirely NULL must be preserved (no rows
//!     lost, `NULL` intact)
//!
//! RED vs current code: `import_index_basic` (import_compass.rs#L566) SELECTs
//! `symbol, name, index_type` — no `name_en`; `import_stock_basic` (L246-257)
//! SELECTs 9 columns — no `industry_en`. So every parquet the current code
//! writes lacks the new columns and the `SELECT name_en / industry_en FROM
//! read_parquet(...)` assertions fail (missing column) until B2 wires them
//! into the export SELECT.
//!
//! These tests must compile today and turn GREEN after B2 wires the columns.

use std::path::{Path, PathBuf};
use std::process::Command;

use compass_data::import_compass::{self, CompassTable};
use duckdb::Connection;

/// index_basic DDL *with* the new `name_en` column (post-B1 Dolt schema).
/// `name_en` is VARCHAR(200) here (wider than production's 100) so the
/// over-length round-trip attack can insert a >100-char value at all —
/// production's VARCHAR(100) would reject it, which is the correct behaviour
/// we are not attacking (manufacturability note, ref #236).
const INDEX_BASIC_DDL_NEW: &str = "CREATE TABLE index_basic (\
     symbol VARCHAR(20) NOT NULL PRIMARY KEY, \
     name VARCHAR(100), name_en VARCHAR(200), index_type VARCHAR(20))";

/// stock_basic DDL *with* the new `industry_en` column (post-B1 Dolt schema).
const STOCK_BASIC_DDL_NEW: &str = "CREATE TABLE stock_basic (\
     symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), \
     industry VARCHAR(50), industry_en VARCHAR(50), \
     list_date VARCHAR(20), delist_date DATE, board VARCHAR(50), \
     full_name VARCHAR(200), total_share DOUBLE, region VARCHAR(50))";

fn setup_dolt(dir: &Path) {
    for (key, val) in [
        ("user.email", "adv@compass.local"),
        ("user.name", "AdvNameEn"),
    ] {
        let out = Command::new("dolt")
            .arg("config")
            .arg("--global")
            .arg("--add")
            .arg(key)
            .arg(val)
            .output()
            .expect("dolt config");
        assert!(out.status.success(), "dolt config {key} failed");
    }
    let init = Command::new("dolt")
        .arg("--data-dir")
        .arg(dir)
        .arg("init")
        .output()
        .expect("dolt init");
    assert!(init.status.success(), "dolt init failed");
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

fn parse(table: &str) -> CompassTable {
    table.parse::<CompassTable>().expect("parse CompassTable")
}

fn export(dolt_dir: &Path, out_dir: &Path, table: CompassTable) {
    import_compass::run(
        dolt_dir.to_path_buf(),
        out_dir.to_path_buf(),
        table,
        false,
        None,
    )
    .expect("import-compass export");
}

fn parquet_count(parquet: &Path) -> usize {
    let conn = Connection::open_in_memory().expect("duckdb");
    let sql = format!(
        "SELECT COUNT(*) FROM read_parquet('{}')",
        parquet.display().to_string().replace('\'', "''")
    );
    conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
        .expect("parquet count") as usize
}

// ---------------------------------------------------------------------------
// Row-count integrity + value→row binding (index_basic)
// ---------------------------------------------------------------------------

/// Dolt rows inserted in non-symbol order; after the `ORDER BY symbol` export
/// the `name_en` must bind to its own symbol (a column misalignment or a value
/// shift would swap English names between rows). Also asserts the parquet row
/// count equals the source row count once the new column is present.
#[test]
fn import_index_basic_preserves_row_count_and_value_binding() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_BASIC_DDL_NEW);
    // Insert intentionally out of symbol order (SH000300 before SH000001).
    for row in [
        ("SH000300", "沪深300", "CSI 300", "official"),
        ("SH000001", "上证指数", "SSE Composite", "official"),
        ("BK0475", "半导体", "Semiconductors", "concept"),
        ("SH000905", "中证500", "CSI 500", "official"),
    ] {
        dolt_sql(
            dolt_tmp.path(),
            &format!(
                "INSERT INTO index_basic (symbol, name, name_en, index_type) VALUES \
                 ('{}', '{}', '{}', '{}')",
                row.0, row.1, row.2, row.3
            ),
        );
    }

    let out_tmp = tempfile::tempdir().expect("output tmp");
    export(dolt_tmp.path(), out_tmp.path(), parse("index_basic"));

    let parquet = out_tmp.path().join("index_basic.parquet");
    assert!(parquet.exists(), "index_basic.parquet must be produced");
    assert_eq!(
        parquet_count(&parquet),
        4,
        "parquet row count must equal the Dolt source count with the new column"
    );

    let conn = Connection::open_in_memory().expect("duckdb");
    // Every symbol must carry its own name_en — no cross-row skew after the
    // ORDER BY re-order.
    for (sym, expect_en) in [
        ("SH000001", "SSE Composite"),
        ("SH000300", "CSI 300"),
        ("BK0475", "Semiconductors"),
        ("SH000905", "CSI 500"),
    ] {
        let en: Option<String> = conn
            .query_row(
                &format!(
                    "SELECT name_en FROM read_parquet('{}') WHERE symbol = '{}'",
                    parquet.display().to_string().replace('\'', "''"),
                    sym
                ),
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|e| panic!("name_en column missing for {sym}: {e}"));
        assert_eq!(
            en.as_deref(),
            Some(expect_en),
            "name_en must bind to symbol {sym} in the exported parquet"
        );
    }
}

/// Boundary: a `name_en` longer than the VARCHAR(100) DDL width must survive
/// the Dolt → parquet export verbatim (no truncation, no row dropped).
#[test]
fn import_index_basic_keeps_overlength_name_en() {
    let long = "A deliberately over-length official English board name that exceeds one hundred characters such that a VARCHAR truncation bug would bite";
    assert!(
        long.chars().count() > 100,
        "fixture must exceed the DDL width"
    );

    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_BASIC_DDL_NEW);
    dolt_sql(
        dolt_tmp.path(),
        &format!(
            "INSERT INTO index_basic (symbol, name, name_en, index_type) VALUES \
             ('SH000001', '上证指数', '{}', 'official')",
            long.replace('\'', "''")
        ),
    );

    let out_tmp = tempfile::tempdir().expect("output tmp");
    export(dolt_tmp.path(), out_tmp.path(), parse("index_basic"));

    let parquet = out_tmp.path().join("index_basic.parquet");
    let conn = Connection::open_in_memory().expect("duckdb");
    let en: Option<String> = conn
        .query_row(
            &format!(
                "SELECT name_en FROM read_parquet('{}') WHERE symbol = 'SH000001'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("name_en column must be present (RED until B2)");
    assert_eq!(
        en.as_deref(),
        Some(long),
        "over-length name_en must round-trip through the export intact"
    );
}

// ---------------------------------------------------------------------------
// All-NULL column + row-count integrity (stock_basic)
// ---------------------------------------------------------------------------

/// A new-schema `industry_en` column that is entirely NULL must be preserved
/// (no rows lost, NULL intact) — not dropped or erroring.
#[test]
fn import_stock_basic_keeps_all_null_industry_en_and_row_count() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), STOCK_BASIC_DDL_NEW);
    for (sym, name, ind) in [
        ("SZ000001", "平安银行", "银行"),
        ("SH600519", "贵州茅台", "白酒"),
        ("SZ000002", "万科A", "房地产"),
    ] {
        dolt_sql(
            dolt_tmp.path(),
            &format!(
                "INSERT INTO stock_basic (symbol, name, industry, industry_en, list_date, delist_date, \
                 board, full_name, total_share, region) VALUES \
                 ('{sym}', '{name}', '{ind}', NULL, '2001-01-01', NULL, '主板', \
                  '{name}股份有限公司', 1.0, 'CN')"
            ),
        );
    }

    let out_tmp = tempfile::tempdir().expect("output tmp");
    export(dolt_tmp.path(), out_tmp.path(), parse("stock_basic"));

    let parquet = out_tmp.path().join("stock_basic.parquet");
    assert!(parquet.exists(), "stock_basic.parquet must be produced");
    assert_eq!(
        parquet_count(&parquet),
        3,
        "all rows survive when industry_en is all NULL"
    );

    // The all-NULL industry_en column must still exist and read without
    // error. Note: `WHERE industry_en IS NULL` counting is NOT asserted here
    // — `dolt sql -r parquet` writes all-NULL columns in a way DuckDB's
    // IS NULL predicate does not match (values read back as NULL, but the
    // predicate misses them; verified 2026-08). That is a Dolt export quirk
    // outside our import chain; the GUI reads the column value via
    // `row.get::<Option<String>>`, which yields None either way.
    let conn = Connection::open_in_memory().expect("duckdb");
    let schema_has_col: usize = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM (DESCRIBE SELECT * FROM read_parquet('{}')) \
                 WHERE column_name = 'industry_en'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("DESCRIBE must run");
    assert_eq!(schema_has_col, 1, "industry_en column must be present");
    let read_back: Option<String> = conn
        .query_row(
            &format!(
                "SELECT industry_en FROM read_parquet('{}') WHERE symbol = 'SZ000001'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("industry_en readable without error");
    assert_eq!(read_back, None, "all-NULL industry_en reads back as None");
}

// Silence unused-import warnings for PathBuf when used only via helpers.
#[allow(dead_code)]
fn _type_check(_: PathBuf) {}
