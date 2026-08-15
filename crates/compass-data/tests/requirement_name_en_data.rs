//! Requirement-acceptance tests for epic #266 B2 (#269): data-name i18n —
//! the Rust data layer carrying `name_en` (index_basic) and `industry_en`
//! (stock_basic) end-to-end through import-compass and the DuckDB export
//! mirror.
//!
//! These tests assert the *behavioral contract* declared by plan B2's
//! acceptance criteria 3 & 4:
//! - criterion 3: import-compass exports a parquet that *contains* the new
//!   columns (`name_en` / `industry_en`) with their values preserved from the
//!   Dolt source.
//! - criterion 4: the DuckDB export mirror (`export_index_tables`,
//!   `CREATE TABLE AS SELECT *`) carries the new column + data through.
//!
//! RED vs current code: `import_index_basic` (import_compass.rs#L566) SELECTs
//! `symbol, name, index_type` — no `name_en`; `import_stock_basic`
//! (L246-257) SELECTs 9 columns — no `industry_en`. So the parquet produced
//! today lacks the new columns, and the `SELECT name_en / industry_en FROM
//! read_parquet(...)` assertions below fail (missing column) until B2 wires
//! them into the export SELECT. The DuckDB mirror test fails at that same
//! upstream link.

use std::path::{Path, PathBuf};
use std::process::Command;

use compass_core::data::parquet::ParquetReader;
use compass_data::import_compass::{self, CompassTable};
use duckdb::Connection;

/// index_basic DDL *with* the new `name_en` column (post-B1 Dolt schema).
const INDEX_BASIC_DDL_NEW: &str = "CREATE TABLE index_basic (\
     symbol VARCHAR(20) NOT NULL PRIMARY KEY, \
     name VARCHAR(100), name_en VARCHAR(100), index_type VARCHAR(20))";

/// stock_basic DDL *with* the new `industry_en` column (post-B1 Dolt schema).
/// Mirrors the import_compass.rs unit-test DDL, plus `industry_en`.
const STOCK_BASIC_DDL_NEW: &str = "CREATE TABLE stock_basic (\
     symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), \
     industry VARCHAR(50), industry_en VARCHAR(50), \
     list_date VARCHAR(20), delist_date DATE, board VARCHAR(50), \
     full_name VARCHAR(200), total_share DOUBLE, region VARCHAR(50))";

fn setup_dolt(dir: &Path) {
    for (key, val) in [
        ("user.email", "reqen@compass.local"),
        ("user.name", "ReqEnTest"),
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

// ---------------------------------------------------------------------------
// Criterion 3: import-compass keeps `name_en` / `industry_en` in the parquet
// ---------------------------------------------------------------------------

#[test]
fn import_index_basic_carries_name_en_column() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_BASIC_DDL_NEW);
    // Post-B1 schema: index rows carry an official English name; unmapped
    // concept/industry rows stay NULL.
    for row in [
        ("SH000001", "上证指数", "SSE Composite", "official"),
        ("SH000300", "沪深300", "CSI 300", "official"),
        ("BK0475", "半导体", "Semiconductors", "concept"),
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
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        parse("index_basic"),
        false,
        None,
    )
    .expect("index_basic export");

    let parquet = out_tmp.path().join("index_basic.parquet");
    assert!(parquet.exists(), "index_basic.parquet must be produced");

    let conn = Connection::open_in_memory().expect("duckdb");
    // RED: today the SELECT omits name_en, so this column read fails.
    let name_en: Option<String> = conn
        .query_row(
            &format!(
                "SELECT name_en FROM read_parquet('{}') WHERE symbol = 'SH000001'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("index_basic parquet must expose the name_en column (RED until B2)");
    assert_eq!(
        name_en.as_deref(),
        Some("SSE Composite"),
        "name_en value must survive the Dolt → parquet export"
    );

    let concept_en: Option<String> = conn
        .query_row(
            &format!(
                "SELECT name_en FROM read_parquet('{}') WHERE symbol = 'BK0475'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("index_basic parquet must expose name_en for concept rows");
    assert_eq!(concept_en.as_deref(), Some("Semiconductors"));
}

#[test]
fn import_stock_basic_carries_industry_en_column() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), STOCK_BASIC_DDL_NEW);
    // industry rows: one mapped (industry_en present), one unmapped (NULL).
    dolt_sql(
        dolt_tmp.path(),
        "INSERT INTO stock_basic (symbol, name, industry, industry_en, list_date, delist_date, \
         board, full_name, total_share, region) VALUES \
         ('SH600519', '贵州茅台', '白酒', 'Liquor', '2001-08-27', NULL, '主板', \
          '贵州茅台酒股份有限公司', 12.56e8, '贵州'), \
         ('SZ000001', '平安银行', '银行', NULL, '1991-04-03', NULL, '主板', \
          '平安银行股份有限公司', 1.94e10, '广东')",
    );

    let out_tmp = tempfile::tempdir().expect("output tmp");
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        out_tmp.path().to_path_buf(),
        parse("stock_basic"),
        false,
        None,
    )
    .expect("stock_basic export");

    let parquet = out_tmp.path().join("stock_basic.parquet");
    assert!(parquet.exists(), "stock_basic.parquet must be produced");

    let conn = Connection::open_in_memory().expect("duckdb");
    // RED: today the SELECT (9 cols) omits industry_en → column read fails.
    let industry_en: Option<String> = conn
        .query_row(
            &format!(
                "SELECT industry_en FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("stock_basic parquet must expose the industry_en column (RED until B2)");
    assert_eq!(
        industry_en.as_deref(),
        Some("Liquor"),
        "industry_en value must survive the Dolt → parquet export"
    );

    // Unmapped (NULL) industry_en stays NULL on the parquet.
    let unmapped: Option<String> = conn
        .query_row(
            &format!(
                "SELECT industry_en FROM read_parquet('{}') WHERE symbol = 'SZ000001'",
                parquet.display().to_string().replace('\'', "''")
            ),
            [],
            |row| row.get(0),
        )
        .expect("industry_en column readable");
    assert_eq!(unmapped, None, "NULL industry_en is preserved as NULL");
}

// ---------------------------------------------------------------------------
// Criterion 4: DuckDB export mirror carries the new column through
// ---------------------------------------------------------------------------
//
// Manufacturability note (&ref #236): the DuckDB export mirror
// (`export::run_export` / `export_index_tables`, export.rs#L85) lives in a
// module private to the `compass-data` *binary* (`main.rs mod export;`), not
// in lib.rs — so an integration test (which links the lib target) cannot
// invoke it, and running `cargo run --bin compass-data -- export` is out of
// scope for a test agent. The mirror uses `CREATE TABLE AS SELECT *`, i.e. it
// is column-agnostic and carries any column the parquet exposes. Its
// acceptance therefore collapses into the *upstream* RED gap: once
// import-compass writes `name_en`/`industry_en` into the parquet (asserted
// above), the mirror carries them verbatim. The mirror-specific DDL+data
// guard belongs in export.rs's unit `mod tests` (where `run_export` and
// `write_index_parquet` are directly reachable) — see
// `run_export_duckdb_mirror_carries_name_en` there.
#[test]
fn index_basic_reader_sees_name_en_on_export_artifact() {
    // Cross-check the compass-core read contract (plan acceptance #5) on the
    // exact artifact import-compass produces for the mirror to consume.
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), INDEX_BASIC_DDL_NEW);
    dolt_sql(
        dolt_tmp.path(),
        "INSERT INTO index_basic (symbol, name, name_en, index_type) VALUES \
         ('SH000001', '上证指数', 'SSE Composite', 'official')",
    );

    let parquet_tmp = tempfile::tempdir().expect("parquet tmp");
    import_compass::run(
        dolt_tmp.path().to_path_buf(),
        parquet_tmp.path().to_path_buf(),
        parse("index_basic"),
        false,
        None,
    )
    .expect("index_basic export");
    assert!(
        parquet_tmp.path().join("index_basic.parquet").exists(),
        "index_basic.parquet must exist for the export mirror"
    );

    let reader = ParquetReader::new(parquet_tmp.path()).expect("create reader");
    let basics = reader.load_all_index_basics().expect("load");
    assert_eq!(basics.len(), 1);
    assert_eq!(basics[0].symbol, "SH000001", "mirror source row present");
    assert_eq!(
        basics[0].name_en.as_deref(),
        Some("SSE Composite"),
        "reader must see name_en on the export artifact (RED until B2)"
    );
}

// Silence unused-import warnings for PathBuf when only used via helpers.
#[allow(dead_code)]
fn _type_check(_: PathBuf) {}
