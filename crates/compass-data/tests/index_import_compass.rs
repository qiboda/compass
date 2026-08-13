//! Adversarial tests: C2 import-compass IndexDaily/IndexBasic + C3 import_dolt
//! BK filter (epic #255 plan T3 / T2).
//!
//! Plan contract under attack:
//! - `CompassTable` gains `IndexDaily` (incremental, PK (symbol, trade_date))
//!   and `IndexBasic` (full overwrite) — both must parse from their CLI names
//!   ("index_daily" / "index_basic", import_compass.rs FromStr).
//! - `normalize_symbol_filter` (import_dolt.rs) accepts BK + exactly 4 digits
//!   ("BK0475") without opening a SQL-injection hole (plan T2: 加 BK+4 位校验).
//!
//! RED vs current code:
//! - `"index_daily".parse::<CompassTable>()` → Err("unknown table")
//! - `import_dolt::run(..., Some("BK0475"), ...)` → Err("not exchange-prefixed")
//!
//! Why `tests/`: the sandbox denies writes to `src/**` (precedent:
//! `crates/compass-data/tests/data_quality_adversarial.rs`). The `IndexDaily`
//! / `IndexBasic` enum *variants* do not exist yet, so no test references them
//! — the FromStr assertions express the plan contract through the public API.
//! Once the variants land, deeper export tests belong in the same file.

use std::path::Path;
use std::process::Command;

use compass_data::import_compass::CompassTable;
use compass_data::import_dolt;

const EOD_SCHEMA: &str = "CREATE TABLE final_a_stock_eod_price (\
     symbol VARCHAR(20) NOT NULL, \
     tradedate DATE NOT NULL, \
     open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, \
     adjclose DOUBLE, volume DOUBLE, amount DOUBLE, \
     PRIMARY KEY (symbol, tradedate))";

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

// ---------------------------------------------------------------------------
// CompassTable FromStr (C2)
// ---------------------------------------------------------------------------

#[test]
fn compass_table_from_str_index_daily() {
    // RED: currently Err("unknown table: index_daily").
    assert!(
        "index_daily".parse::<CompassTable>().is_ok(),
        "CompassTable must accept the CLI table name 'index_daily'"
    );
}

#[test]
fn compass_table_from_str_index_basic() {
    // RED: currently Err("unknown table: index_basic").
    assert!(
        "index_basic".parse::<CompassTable>().is_ok(),
        "CompassTable must accept the CLI table name 'index_basic'"
    );
}

#[test]
fn compass_table_from_str_typo_still_rejected() {
    // Guard: adding the two new names must not loosen the parser.
    assert!("index_daily_typo".parse::<CompassTable>().is_err());
    assert!("INDEX_DAILY".parse::<CompassTable>().is_err());
}

#[test]
fn compass_table_from_str_existing_tables_unaffected() {
    assert!("stock_basic".parse::<CompassTable>().is_ok());
    assert!("capital_main_flow".parse::<CompassTable>().is_ok());
}

// ---------------------------------------------------------------------------
// import_dolt --symbols BK filter (C3, T2)
// ---------------------------------------------------------------------------

#[test]
fn import_dolt_run_accepts_bk_symbols_filter() {
    // RED: normalize_symbol_filter("BK0475") currently errors because
    // parse_explicit_prefix does not recognize the BK branch → the whole
    // `import --symbols BK0475` run fails up front.
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), EOD_SCHEMA);

    let output_tmp = tempfile::tempdir().expect("output tmp");
    let result = import_dolt::run(
        dolt_tmp.path().to_path_buf(),
        output_tmp.path().to_path_buf(),
        0,
        Some("BK0475"),
        None,
        None,
        None,
    );
    assert!(
        result.is_ok(),
        "--symbols BK0475 must pass validation (BK + 4 digits): {:?}",
        result.err()
    );
}

#[test]
fn import_dolt_run_accepts_bk_boundary_codes() {
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), EOD_SCHEMA);

    for bad_ok in ["BK0000", "BK9999"] {
        let output_tmp = tempfile::tempdir().expect("output tmp");
        let result = import_dolt::run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            Some(bad_ok),
            None,
            None,
            None,
        );
        assert!(
            result.is_ok(),
            "BK boundary code {bad_ok:?} must be accepted: {:?}",
            result.err()
        );
    }
}

#[test]
fn import_dolt_run_rejects_malformed_bk() {
    // Guard: BK + 3 digits, BK + 5 digits and BK + letters must stay rejected
    // (the BK branch must not become a wildcard).
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), EOD_SCHEMA);

    for bad in ["BK047", "BK04755", "BKAB12"] {
        let output_tmp = tempfile::tempdir().expect("output tmp");
        let result = import_dolt::run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            Some(bad),
            None,
            None,
            None,
        );
        assert!(
            result.is_err(),
            "malformed BK code {bad:?} must be rejected, got Ok"
        );
    }
}

#[test]
fn import_dolt_run_rejects_bk_injection() {
    // Guard: the BK branch must not weaken the existing injection defenses
    // (import_dolt.rs normalize_symbol_filter quote/semicolon closure).
    let dolt_tmp = tempfile::tempdir().expect("dolt tmp");
    setup_dolt(dolt_tmp.path());
    dolt_sql(dolt_tmp.path(), EOD_SCHEMA);

    for bad in [
        "BK0475'",                       // quote close
        "BK0475;DROP TABLE stock_basic", // stacked statement
        "BK0475' OR '1'='1",             // tautology
    ] {
        let output_tmp = tempfile::tempdir().expect("output tmp");
        let result = import_dolt::run(
            dolt_tmp.path().to_path_buf(),
            output_tmp.path().to_path_buf(),
            0,
            Some(bad),
            None,
            None,
            None,
        );
        assert!(
            result.is_err(),
            "injection payload {bad:?} must be rejected, got Ok"
        );
    }
}
