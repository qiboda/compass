//! Import data from `compass_data` Dolt repository into Parquet.
//!
//! Follows the same `dolt sql -r parquet` → `fs::write` pattern as `import_dolt.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use duckdb::Connection;
use tracing::{info, warn};

use crate::import_dolt::run_dolt_sql_parquet;

/// Freshness warn thresholds (issue #136, Q5: warn-only).
///
/// Financial tables report quarterly, so 120 days covers a missed quarter;
/// SEPA market tables update daily and go stale within a week.
const FIN_FRESHNESS_DAYS: i64 = 120;
const MARKET_FRESHNESS_DAYS: i64 = 7;

/// Tables in compass_data that can be imported.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompassTable {
    StockBasic,
    FinIndicators,
    FinBalanceSheet,
    FinIncome,
    FinCashFlow,
    /// Capital main flow (主力资金流), incremental merge on (symbol, trade_date).
    MainFlow,
    /// Dragon list (龙虎榜), incremental merge on (symbol, trade_date, seat_type).
    DragonList,
    /// Block trades (大宗交易), incremental merge on the full Dolt PK
    /// (symbol, trade_date, price, volume, amount, buyer, seller).
    BlockTrade,
    /// Institution surveys (机构调研), incremental merge on (symbol, survey_date, org_name).
    InstitutionSurvey,
    /// Index/board daily bars (指数/板块日线), incremental merge on (symbol, trade_date).
    ///
    /// The exported parquet renames Dolt `trade_date` → `tradedate` and adds
    /// `adjclose = close` (placeholder) so the GUI's stock-shaped daily
    /// queries work unchanged on index data (plan T3 column contract).
    IndexDaily,
    /// Index/board name table (指数/板块名称), full-overwrite import
    /// (mirrors Dolt state, DELETE+rewrite semantics).
    IndexBasic,
}

impl std::str::FromStr for CompassTable {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stock_basic" => Ok(CompassTable::StockBasic),
            "fin_indicators" => Ok(CompassTable::FinIndicators),
            "fin_balance_sheet" => Ok(CompassTable::FinBalanceSheet),
            "fin_income" => Ok(CompassTable::FinIncome),
            "fin_cash_flow" => Ok(CompassTable::FinCashFlow),
            "capital_main_flow" => Ok(CompassTable::MainFlow),
            "dragon_list" => Ok(CompassTable::DragonList),
            "block_trade" => Ok(CompassTable::BlockTrade),
            "institution_survey" => Ok(CompassTable::InstitutionSurvey),
            "index_daily" => Ok(CompassTable::IndexDaily),
            "index_basic" => Ok(CompassTable::IndexBasic),
            _ => Err(format!("unknown table: {s}")),
        }
    }
}

/// Import data from compass_data Dolt into Parquet.
pub fn run(
    dolt_dir: PathBuf,
    output: PathBuf,
    table: CompassTable,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate `--since` up front (B2, ref #181): the value is interpolated
    // raw into the WHERE clause, so anything else — quote chars, SQL comment
    // markers, short/non-digit input — is an injection vector. Contract:
    // exactly `YYYY-MM-DD` (matching every existing caller and test).
    if let Some(s) = since
        && !s.is_empty()
    {
        validate_since_arg("--since", s)?;
    }
    match table {
        CompassTable::StockBasic => import_stock_basic(&dolt_dir, &output),
        CompassTable::FinIndicators => import_fin_indicators(&dolt_dir, &output, overwrite, since),
        CompassTable::FinBalanceSheet => {
            import_financial_table("fin_balance_sheet", &dolt_dir, &output, overwrite, since)
        }
        CompassTable::FinIncome => {
            import_financial_table("fin_income", &dolt_dir, &output, overwrite, since)
        }
        CompassTable::FinCashFlow => {
            import_financial_table("fin_cash_flow", &dolt_dir, &output, overwrite, since)
        }
        CompassTable::MainFlow => {
            import_append_table(
                AppendTableSpec {
                    table_name: "capital_main_flow",
                    date_col: "trade_date",
                    partition_cols: "symbol, trade_date",
                    prefer_new: true,
                    dolt_order_cols: None,
                    select_cols: None,
                },
                &dolt_dir,
                &output,
                overwrite,
                since,
            )?;
            warn_if_stale(&dolt_dir, "capital_main_flow", MARKET_FRESHNESS_DAYS);
            Ok(())
        }
        CompassTable::DragonList => {
            import_append_table(
                AppendTableSpec {
                    table_name: "dragon_list",
                    date_col: "trade_date",
                    partition_cols: "symbol, trade_date, seat_type",
                    prefer_new: true,
                    dolt_order_cols: None,
                    select_cols: None,
                },
                &dolt_dir,
                &output,
                overwrite,
                since,
            )?;
            warn_if_stale(&dolt_dir, "dragon_list", MARKET_FRESHNESS_DAYS);
            Ok(())
        }
        CompassTable::BlockTrade => {
            import_append_table(
                AppendTableSpec {
                    table_name: "block_trade",
                    date_col: "trade_date",
                    partition_cols: "symbol, trade_date, price, volume, amount, buyer, seller",
                    prefer_new: true,
                    dolt_order_cols: None,
                    select_cols: None,
                },
                &dolt_dir,
                &output,
                overwrite,
                since,
            )?;
            warn_if_stale(&dolt_dir, "block_trade", MARKET_FRESHNESS_DAYS);
            Ok(())
        }
        CompassTable::InstitutionSurvey => {
            import_append_table(
                AppendTableSpec {
                    table_name: "institution_survey",
                    date_col: "survey_date",
                    partition_cols: "symbol, survey_date, org_name",
                    prefer_new: true,
                    dolt_order_cols: None,
                    select_cols: None,
                },
                &dolt_dir,
                &output,
                overwrite,
                since,
            )?;
            warn_if_stale(&dolt_dir, "institution_survey", MARKET_FRESHNESS_DAYS);
            Ok(())
        }
        CompassTable::IndexDaily => {
            import_append_table(
                AppendTableSpec {
                    table_name: "index_daily",
                    date_col: "trade_date",
                    // Parquet-side partition columns: the export renames Dolt
                    // `trade_date` → `tradedate`, so the merge dedups on the
                    // parquet column name (plan T3/C4 contract).
                    partition_cols: "symbol, tradedate",
                    prefer_new: true,
                    dolt_order_cols: Some("symbol, trade_date"),
                    select_cols: Some(
                        "symbol, index_type, trade_date AS tradedate, open, high, low, close, \
                         volume, amount, close AS adjclose",
                    ),
                },
                &dolt_dir,
                &output,
                overwrite,
                since,
            )?;
            warn_if_stale(&dolt_dir, "index_daily", MARKET_FRESHNESS_DAYS);
            Ok(())
        }
        CompassTable::IndexBasic => {
            import_index_basic(&dolt_dir, &output)?;
            warn_if_stale(&dolt_dir, "index_basic", MARKET_FRESHNESS_DAYS);
            Ok(())
        }
    }
}

/// Validate a `--since` CLI value: must be exactly `YYYY-MM-DD` (the
/// import-compass contract, distinct from import_dolt's YYYYMMDD). The value
/// is interpolated raw into the WHERE clause (B2, ref #181 — same hardening
/// as `import_dolt::validate_date_arg`).
fn validate_since_arg(flag: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 10
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if !valid {
        return Err(format!("{flag} must be YYYY-MM-DD (10 chars), got '{value}'").into());
    }
    Ok(())
}

/// Warn when the source data is stale (issue #136, Q5: warn-only).
/// Thresholds: fin_* tables 120 days (quarterly reports), market tables
/// (main_flow/dragon_list/block_trade/institution_survey/
/// index_daily/index_basic) 7 days. `stock_basic` is skipped: its
/// data_updates row has a NULL last_report_date (collectors write only
/// 4 columns, main.py:79-85).
fn warn_if_stale(dolt_dir: &Path, table: &str, threshold_days: i64) {
    let Ok(Some(last)) = crate::validate::data_updates_last_report_date(dolt_dir, table) else {
        return; // no data_updates row / NULL / missing table -> nothing to compare
    };
    let Ok(days) = crate::validate::freshness_days(&last, crate::validate::today_cn()) else {
        return; // unparseable date -> skip
    };
    if days > threshold_days {
        warn!(
            "freshness: {table} last_report_date {last} is {days} days old (threshold {threshold_days})"
        );
    }
}

fn import_stock_basic(dolt_dir: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting stock_basic...");
    let data = run_dolt_sql_parquet(
        dolt_dir,
        "SELECT symbol, \
         name, \
         CAST(list_date AS DATE) AS list_date, \
         CAST(delist_date AS DATE) AS delist_date, \
         board, \
         full_name, \
         CAST(total_share AS DOUBLE) AS total_share, \
         industry, \
         industry_en, \
         region \
         FROM stock_basic \
         WHERE symbol LIKE 'SH%' OR symbol LIKE 'SZ%' OR symbol LIKE 'BJ%' \
         ORDER BY symbol",
    )?;
    let path = output.join("stock_basic.parquet");
    std::fs::write(&path, &data)?;
    let src_count = crate::validate::dolt_count(
        dolt_dir,
        "stock_basic",
        "WHERE symbol LIKE 'SH%' OR symbol LIKE 'SZ%' OR symbol LIKE 'BJ%'",
    )?;
    let parquet_count = crate::validate::parquet_row_count(&path)?;
    crate::validate::verify_row_count(src_count, parquet_count, "stock_basic")?;
    info!("  → {}", path.display());
    Ok(())
}

/// Unique temp-file path under `compass_parquet_work` for incremental-merge
/// staging.
///
/// PID + per-process atomic sequence keep names distinct across nextest
/// processes and within one process (ref #184 — a fixed
/// `{table_name}.new.parquet` / `{table_name}.merged.parquet` name raced
/// between parallel test binaries running in different processes, clobbering
/// each other's stage files and silently falling back to full export).
fn unique_work_path(stem: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir()
        .join("compass_parquet_work")
        .join(format!(
            "{stem}_{}_{}.parquet",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ))
}

fn import_fin_indicators(
    dolt_dir: &Path,
    output: &Path,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // fin_indicators uses a fixed SELECT list; all merge/fallback logic is the
    // shared `import_append_table` path so partition/fallback fixes stay in one
    // place (bug #298).
    import_append_table(
        AppendTableSpec {
            table_name: "fin_indicators",
            date_col: "report_date",
            partition_cols: "symbol, report_date",
            prefer_new: false,
            dolt_order_cols: None,
            select_cols: Some(
                "report_date, update_date, notice_date, \
                 data_type, qdate, data_year, date_label, \
                 symbol, secucode, name, trade_market, trade_market_code, trade_market_zjg, \
                 security_type, security_type_code, industry, \
                 board_code, board_name, ori_board_code, org_code, is_new, \
                 basic_eps, deduct_basic_eps, revenue, net_profit, roe, bps, \
                 cash_flow_per_share, gross_margin, \
                 revenue_yoy, net_profit_yoy, operating_profit_yoy, net_profit_qoq, \
                 shares_growth, dividend_plan, dividend_year",
            ),
        },
        dolt_dir,
        output,
        overwrite,
        since,
    )?;
    warn_if_stale(dolt_dir, "fin_indicators", FIN_FRESHNESS_DAYS);
    Ok(())
}

fn import_financial_table(
    table_name: &str,
    dolt_dir: &Path,
    output: &Path,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    import_append_table(
        AppendTableSpec {
            table_name,
            date_col: "report_date",
            partition_cols: "symbol, report_date",
            prefer_new: false,
            dolt_order_cols: None,
            select_cols: None,
        },
        dolt_dir,
        output,
        overwrite,
        since,
    )?;
    warn_if_stale(dolt_dir, table_name, FIN_FRESHNESS_DAYS);
    Ok(())
}

/// Table definition for [`import_append_table`].
struct AppendTableSpec<'a> {
    /// Dolt table name (also the parquet file base name).
    table_name: &'a str,
    /// Column used for the `--since` filter.
    date_col: &'a str,
    /// Primary-key columns, used to dedupe old parquet rows against new Dolt
    /// rows on the same key (comma-separated, also the merge sort order).
    ///
    /// These are the **parquet-side** column names — use
    /// [`AppendTableSpec::dolt_order_cols`] when the export renames Dolt
    /// columns (e.g. index_daily `trade_date` → `tradedate`). They must match
    /// the production Dolt PK exactly; a narrower partition collapses distinct
    /// real rows and loses history (bug #298).
    partition_cols: &'a str,
    /// Dolt-side ORDER BY columns for the source query (defaults to
    /// `partition_cols` when the Dolt and parquet column names agree).
    dolt_order_cols: Option<&'a str>,
    /// SELECT list replacing `*` (defaults to `*`), for exports that add
    /// renamed or derived columns (e.g. `trade_date AS tradedate`,
    /// `close AS adjclose`).
    select_cols: Option<&'a str>,
    /// Which version wins when both sides hold the same key: SEPA capital
    /// tables are DELETE+rewritten by collectors each run, so the Dolt state
    /// is newer and must win (`true`); financial rows never change after
    /// publication, so old-wins behavior is preserved (`false`).
    prefer_new: bool,
}

/// Import an append-style table (financial statements, SEPA capital-flow
/// tables) with optional incremental merge.
///
/// When both sides hold the same key, `prefer_new` selects which version
/// wins, see [`AppendTableSpec::prefer_new`].
fn import_append_table<'a>(
    spec: AppendTableSpec<'a>,
    dolt_dir: &Path,
    output: &Path,
    overwrite: bool,
    since: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let AppendTableSpec {
        table_name,
        date_col,
        partition_cols,
        dolt_order_cols,
        select_cols,
        prefer_new,
    } = spec;
    let parquet_name = format!("{table_name}.parquet");
    let path = output.join(&parquet_name);

    let date_filter = match since {
        Some(s) if !s.is_empty() => format!(" WHERE {date_col} >= '{s}'"),
        _ => String::new(),
    };
    let select_cols = select_cols.unwrap_or("*");
    let order_cols = dolt_order_cols.unwrap_or(partition_cols);
    let query =
        format!("SELECT {select_cols} FROM {table_name}{date_filter} ORDER BY {order_cols}");

    info!("Exporting {table_name}...");
    let new_data = run_dolt_sql_parquet(dolt_dir, &query)?;
    if new_data.len() < 500 {
        warn!("{table_name} returned empty or tiny data, skipping");
        return Ok(());
    }

    if since.is_some() && !overwrite && path.exists() {
        info!("Merging incremental data with existing parquet...");
        std::fs::create_dir_all(std::env::temp_dir().join("compass_parquet_work"))?;

        // Row-count baseline for the no-loss check. A corrupt old parquet
        // (the fallback's recovery trigger) yields None, skipping the check.
        let old_count = crate::validate::parquet_row_count(&path).ok();

        let new_path = unique_work_path(&format!("{table_name}.new"));
        std::fs::write(&new_path, &new_data)?;

        let priority_order = if prefer_new {
            "priority DESC"
        } else {
            "priority"
        };
        let tmp_path = unique_work_path(&format!("{table_name}.merged"));
        let duck = Connection::open_in_memory()?;
        let sql = format!(
            "COPY (SELECT * FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY {partition_cols} ORDER BY {priority_order}) AS rn \
             FROM (SELECT *, 1 AS priority FROM read_parquet('{}') \
             UNION ALL SELECT *, 2 FROM read_parquet('{}'))) WHERE rn = 1 ORDER BY {partition_cols}) \
             TO '{}' (FORMAT PARQUET)",
            path.display(),
            new_path.display(),
            tmp_path.display(),
        );
        if let Err(e) = duck.execute_batch(&sql) {
            warn!("DuckDB merge failed: {e}, falling back to full export");
            // The fallback must NOT write the `--since`-filtered `new_data`
            // over the full parquet — that is exactly bug #298's history-loss
            // path. Instead, rerun the Dolt query without the date filter and
            // write a genuinely full export, then validate against the full
            // Dolt row count (the old parquet may be corrupt, so we cannot
            // rely on a merge-level no-loss comparison here).
            let full_query =
                format!("SELECT {select_cols} FROM {table_name} ORDER BY {order_cols}");
            let full_data = run_dolt_sql_parquet(dolt_dir, &full_query)?;
            std::fs::write(&path, &full_data)?;
            let src_count = crate::validate::dolt_count(dolt_dir, table_name, "")?;
            let parquet_count = crate::validate::parquet_row_count(&path)?;
            crate::validate::verify_row_count(src_count, parquet_count, table_name)?;
        } else {
            std::fs::copy(&tmp_path, &path)?;
            let merged_count = crate::validate::parquet_row_count(&path)?;
            if let Some(old_rows) = old_count
                && merged_count < old_rows
            {
                return Err(format!(
                    "row count mismatch: merge lost rows old={old_rows} parquet={merged_count} (table {table_name})"
                )
                .into());
            }
        }
        let _ = std::fs::remove_file(&new_path);
        let _ = std::fs::remove_file(&tmp_path);
    } else {
        std::fs::write(&path, &new_data)?;
        let src_count = crate::validate::dolt_count(dolt_dir, table_name, &date_filter)?;
        let parquet_count = crate::validate::parquet_row_count(&path)?;
        crate::validate::verify_row_count(src_count, parquet_count, table_name)?;
    }

    info!("  → {}", path.display());
    Ok(())
}

/// Import index_basic as a full overwrite.
///
/// `index_basic` is a version-tracked name table
/// (collectors rewrite the whole Dolt table on each full run), so the export
/// must always mirror the current Dolt state — boards/indices deleted
/// upstream must disappear from the parquet. No incremental merge, regardless
/// of the `--overwrite` / `--since` flags.
fn import_index_basic(dolt_dir: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    info!("Exporting index_basic...");
    let data = run_dolt_sql_parquet(
        dolt_dir,
        "SELECT symbol, name, index_type, name_en FROM index_basic ORDER BY symbol",
    )?;
    let path = output.join("index_basic.parquet");
    std::fs::write(&path, &data)?;
    let src_count = crate::validate::dolt_count(dolt_dir, "index_basic", "")?;
    let parquet_count = crate::validate::parquet_row_count(&path)?;
    crate::validate::verify_row_count(src_count, parquet_count, "index_basic")?;
    info!("  → {}", path.display());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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

    const MAIN_FLOW_SCHEMA: &str = "\
        CREATE TABLE capital_main_flow (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE, \
        super_large_net DOUBLE, large_net DOUBLE, \
        medium_net DOUBLE, small_net DOUBLE, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date))";

    const DRAGON_LIST_SCHEMA: &str = "\
        CREATE TABLE dragon_list (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        seat_type VARCHAR(10) NOT NULL, \
        buy_amount DOUBLE, sell_amount DOUBLE, net_amount DOUBLE, \
        institution_flag TINYINT, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date, seat_type))";

    const BLOCK_TRADE_SCHEMA: &str = "\
        CREATE TABLE block_trade (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        price DOUBLE NOT NULL, \
        volume DOUBLE NOT NULL, \
        amount DOUBLE NOT NULL, \
        buyer VARCHAR(100) NOT NULL, \
        seller VARCHAR(100) NOT NULL, \
        premium_rate DOUBLE, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date, price, volume, amount, buyer, seller))";

    const INSTITUTION_SURVEY_SCHEMA: &str = "\
        CREATE TABLE institution_survey (\
        symbol VARCHAR(20) NOT NULL, \
        survey_date DATE NOT NULL, \
        org_name VARCHAR(1000) NOT NULL, \
        survey_type VARCHAR(300), \
        update_date DATE, \
        PRIMARY KEY (symbol, survey_date, org_name))";

    fn setup_dolt(tmp: &std::path::Path) {
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
            .arg(tmp)
            .arg("init")
            .output()
            .expect("dolt init");
        assert!(
            init.status.success(),
            "dolt init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
    }

    #[test]
    fn validate_since_arg_accepts_iso_date() {
        assert!(validate_since_arg("--since", "2025-01-01").is_ok());
        assert!(validate_since_arg("--since", "1990-12-19").is_ok());
    }

    #[test]
    fn validate_since_arg_rejects_injection_and_malformed() {
        // B2 (ref #181): the value is interpolated raw into SQL — quote
        // chars, comment markers and wrong lengths must all be rejected.
        for bad in [
            "2025-01-01' OR '1'='1",
            "2025-01-01'; DROP TABLE index_daily; --",
            "20250101",    // import_dolt's YYYYMMDD contract does not apply here
            "2025-1-01",   // non-padded month
            "2025-01-01x", // trailing garbage
            "not-a-date",
            "",
        ] {
            assert!(
                validate_since_arg("--since", bad).is_err(),
                "must reject {bad:?}"
            );
        }
    }

    #[test]
    fn stock_basic_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), \
                  industry VARCHAR(50), industry_en VARCHAR(50), list_date VARCHAR(20), delist_date DATE, board VARCHAR(50), \
                  full_name VARCHAR(200), total_share DOUBLE, region VARCHAR(50))")
            .output().expect("create table");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO stock_basic (symbol, name, industry, industry_en, list_date, delist_date, board, full_name, total_share, region) \
                  VALUES ('SH600519', '贵州茅台', '白酒Ⅱ', 'Liquor', '2001-08-27', NULL, '主板', '贵州茅台酒股份有限公司', 12.56e8, '贵州')")
            .output()
            .expect("insert");

        import_stock_basic(tmp.path(), tmp.path()).expect("import");

        let parquet = tmp.path().join("stock_basic.parquet");
        assert!(parquet.exists());
        assert!(parquet.metadata().unwrap().len() > 500);

        // New columns present with expected values
        let duck = duckdb::Connection::open_in_memory().unwrap();
        let (symbol, list_date, board, full_name, total_share, industry, region): (
            String,
            String,
            String,
            String,
            f64,
            String,
            String,
        ) = duck
            .query_row(
                &format!(
                    "SELECT symbol, strftime(list_date, '%Y-%m-%d'), board, full_name, total_share, industry, region \
                     FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet.display()
                ),
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(symbol, "SH600519");
        assert_eq!(list_date, "2001-08-27");
        assert_eq!(board, "主板");
        assert_eq!(full_name, "贵州茅台酒股份有限公司");
        assert!((total_share - 12.56e8).abs() < 1.0);
        assert_eq!(industry, "白酒Ⅱ");
        assert_eq!(region, "贵州");

        let has_exchange: i64 = duck
            .prepare(&format!(
                "SELECT COUNT(*) FROM (DESCRIBE SELECT * FROM read_parquet('{}')) \
                 WHERE column_name = 'exchange'",
                parquet.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(has_exchange, 0, "exchange column must be dropped");

        // delist_date column exists and is NULL for this row
        let delist_date: Option<String> = duck
            .query_row(
                &format!(
                    "SELECT CAST(delist_date AS VARCHAR) FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(delist_date, None);
    }

    #[test]
    fn fin_indicators_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps) VALUES \
                ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66)")
            .output().expect("insert");

        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("import");

        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists());
        assert!(parquet.metadata().unwrap().len() > 500);
    }

    #[test]
    fn compass_table_from_str_all_valid_variants() {
        assert!(matches!(
            "stock_basic".parse::<CompassTable>(),
            Ok(CompassTable::StockBasic)
        ));
        assert!(matches!(
            "fin_indicators".parse::<CompassTable>(),
            Ok(CompassTable::FinIndicators)
        ));
        assert!(matches!(
            "fin_balance_sheet".parse::<CompassTable>(),
            Ok(CompassTable::FinBalanceSheet)
        ));
        assert!(matches!(
            "fin_income".parse::<CompassTable>(),
            Ok(CompassTable::FinIncome)
        ));
        assert!(matches!(
            "fin_cash_flow".parse::<CompassTable>(),
            Ok(CompassTable::FinCashFlow)
        ));
        assert!(matches!(
            "capital_main_flow".parse::<CompassTable>(),
            Ok(CompassTable::MainFlow)
        ));
        assert!(matches!(
            "dragon_list".parse::<CompassTable>(),
            Ok(CompassTable::DragonList)
        ));
        assert!(matches!(
            "block_trade".parse::<CompassTable>(),
            Ok(CompassTable::BlockTrade)
        ));
        assert!(matches!(
            "institution_survey".parse::<CompassTable>(),
            Ok(CompassTable::InstitutionSurvey)
        ));
        assert!(matches!(
            "index_daily".parse::<CompassTable>(),
            Ok(CompassTable::IndexDaily)
        ));
        assert!(matches!(
            "index_basic".parse::<CompassTable>(),
            Ok(CompassTable::IndexBasic)
        ));
    }

    #[test]
    fn compass_table_from_str_invalid_variant() {
        let result = "unknown_table".parse::<CompassTable>();
        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), "unknown table: unknown_table");
    }

    #[test]
    fn compass_table_from_str_empty_string() {
        let result = "".parse::<CompassTable>();
        assert!(result.is_err());
    }

    #[test]
    fn fin_indicators_skips_tiny_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Create fin_indicators with full schema but insert 0 rows.
        // When dolt's parquet output for empty result is <500 bytes, the
        // "empty or tiny data, skipping" path is triggered (lines 106-109).
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        // No INSERT — table is empty
        // The import_fin_indicators call should not panic regardless of whether
        // the parquet output falls below the 500-byte threshold.
        let result = import_fin_indicators(tmp.path(), tmp.path(), false, None);
        assert!(result.is_ok(), "import with empty table should succeed");
    }

    #[test]
    fn fin_indicators_since_merge_with_existing_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        // Insert data with report_date 2024-12-31
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2024-12-31', 1.5e11, 7e10, 59.0, '贵州茅台')")
            .output().expect("insert 2024");

        // First import — creates initial parquet
        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("first import");
        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists(), "initial parquet should exist");

        // Insert more data with later report_date
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66, '贵州茅台')")
            .output().expect("insert 2025");

        // Incremental import with since — triggers merge path
        import_fin_indicators(tmp.path(), tmp.path(), false, Some("2025-01-01"))
            .expect("incremental import");

        // Read back merged parquet and verify both rows exist
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: usize = duck
            .query_row(
                &format!("SELECT COUNT(*) FROM read_parquet('{}')", parquet.display()),
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 2, "merged parquet should have both rows");
    }

    #[test]
    fn run_dispatches_fin_indicators_arm() {
        // import_table's FinIndicators arm routes to import_fin_indicators
        // (the match arm was previously only covered indirectly).
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2024-12-31', 1.5e11, 7e10, 59.0, '贵州茅台')")
            .output().expect("insert");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            None,
        )
        .expect("run fin_indicators");

        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists(), "fin_indicators.parquet should be created");
    }

    #[test]
    fn fin_indicators_merge_failure_falls_back_to_full_export() {
        // Corrupt the existing parquet so the DuckDB incremental-merge SQL
        // fails; the import must fall back to overwriting the parquet with
        // the fresh Dolt data instead of erroring out.
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2024-12-31', 1.5e11, 7e10, 59.0, '贵州茅台')")
            .output().expect("insert 2024");

        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("first import");
        let parquet = tmp.path().join("fin_indicators.parquet");
        assert!(parquet.exists());

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES \
                ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66, '贵州茅台')")
            .output().expect("insert 2025");

        std::fs::write(&parquet, b"corrupted parquet").expect("corrupt parquet");

        import_fin_indicators(tmp.path(), tmp.path(), false, Some("2025-01-01"))
            .expect("fallback import");

        // The fallback rewrote the parquet with a genuine full Dolt export
        // (2024 + 2025 rows; bug #298 requires the fallback not to lose the
        // pre-`--since` history), so the garbage file was replaced by a
        // readable parquet that still contains both years.
        assert_eq!(read_parquet_row_count(&parquet), 2);
    }

    #[test]
    fn financial_table_merge_failure_falls_back_to_full_export() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE fin_balance_sheet (\
                symbol VARCHAR(20) NOT NULL, \
                report_date DATE NOT NULL, \
                total_assets DOUBLE, \
                net_profit DOUBLE, \
                PRIMARY KEY (symbol, report_date))",
            )
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
                ('SH600519', '2024-12-31', 2.6e11, 7e10)")
            .output().expect("insert 2024");

        import_financial_table("fin_balance_sheet", tmp.path(), tmp.path(), false, None)
            .expect("first import");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(parquet.exists());
        assert_eq!(read_parquet_row_count(&parquet), 1);

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
                ('SH600519', '2025-12-31', 2.8e11, 8e10)")
            .output().expect("insert 2025");

        std::fs::write(&parquet, b"corrupted parquet").expect("corrupt parquet");

        import_financial_table(
            "fin_balance_sheet",
            tmp.path(),
            tmp.path(),
            false,
            Some("2025-01-01"),
        )
        .expect("fallback import");

        // Fallback must perform a full export (not a `--since`-filtered
        // overwrite), so both 2024 and 2025 rows survive.
        assert_eq!(read_parquet_row_count(&parquet), 2);
    }

    /// Create a financial table with the F10 API schema (representative
    /// subset) and seed one Moutai row.
    ///
    /// The three financial tables are exported via `SELECT *`
    /// (import_append_table), so every column here must flow through to the
    /// parquet automatically — `assert_f10_columns_exported` locks that in.
    fn setup_financial_table(dolt_dir: &std::path::Path, table_name: &str) {
        let schema = format!(
            "CREATE TABLE {table_name} (\
             symbol VARCHAR(20) NOT NULL, \
             report_date DATE NOT NULL, \
             TOTAL_OPERATE_INCOME DOUBLE, \
             TOTAL_OPERATE_INCOME_YOY DOUBLE, \
             RESEARCH_EXPENSE DOUBLE, \
             BASIC_EPS DOUBLE, \
             MINORITY_INTEREST DOUBLE, \
             TOTAL_ASSETS DOUBLE, \
             NETCASH_OPERATE DOUBLE, \
             PRIMARY KEY (symbol, report_date))"
        );
        Command::new("dolt")
            .arg("--data-dir")
            .arg(dolt_dir)
            .arg("sql")
            .arg("-q")
            .arg(&schema)
            .output()
            .unwrap_or_else(|_| panic!("create {table_name}"));

        Command::new("dolt")
            .arg("--data-dir").arg(dolt_dir)
            .arg("sql").arg("-q")
            .arg(format!(
                "INSERT INTO {table_name} (symbol, report_date, TOTAL_OPERATE_INCOME, \
                 TOTAL_OPERATE_INCOME_YOY, RESEARCH_EXPENSE, BASIC_EPS, MINORITY_INTEREST, \
                 TOTAL_ASSETS, NETCASH_OPERATE) VALUES \
                 ('SH600519', '2024-12-31', 174144069958.25, 15.66, 2.79e8, 68.64, 8.5e8, 2.8e11, 9.0e10)"
            ))
            .output().unwrap_or_else(|_| panic!("insert {table_name}"));
    }

    /// Assert that the exported parquet carries the F10 schema columns.
    ///
    /// `import_append_table` exports financial tables via `SELECT *`, so new
    /// Dolt columns (F10 API fields) must appear in the parquet without any
    /// column-list maintenance. Guards against regressions when the F10
    /// schema is extended.
    fn assert_f10_columns_exported(parquet: &std::path::Path) {
        let duck = duckdb::Connection::open_in_memory().unwrap();
        let count: i64 = duck
            .prepare(&format!(
                "SELECT COUNT(*) FROM (DESCRIBE SELECT * FROM read_parquet('{}')) \
                 WHERE column_name IN ('TOTAL_OPERATE_INCOME','RESEARCH_EXPENSE','BASIC_EPS','TOTAL_OPERATE_INCOME_YOY')",
                parquet.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(
            count, 4,
            "F10 columns must be exported via SELECT * (schema regression?)"
        );
    }

    #[test]
    fn run_fin_balance_sheet_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        setup_financial_table(tmp.path(), "fin_balance_sheet");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinBalanceSheet,
            false,
            None,
        )
        .expect("run fin_balance_sheet");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(parquet.exists(), "parquet should exist");
        assert!(
            parquet.metadata().unwrap().len() > 500,
            "parquet should have data"
        );
        assert_f10_columns_exported(&parquet);
    }

    #[test]
    fn run_fin_income_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        setup_financial_table(tmp.path(), "fin_income");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIncome,
            false,
            None,
        )
        .expect("run fin_income");

        let parquet = tmp.path().join("fin_income.parquet");
        assert!(parquet.exists(), "parquet should exist");
        assert!(parquet.metadata().unwrap().len() > 500);
        assert_f10_columns_exported(&parquet);
    }

    #[test]
    fn run_fin_cash_flow_exports_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        setup_financial_table(tmp.path(), "fin_cash_flow");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinCashFlow,
            false,
            None,
        )
        .expect("run fin_cash_flow");

        let parquet = tmp.path().join("fin_cash_flow.parquet");
        assert!(parquet.exists(), "parquet should exist");
        assert!(parquet.metadata().unwrap().len() > 500);
        assert_f10_columns_exported(&parquet);
    }

    #[test]
    fn financial_table_skip_tiny_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Create fin_balance_sheet with minimal schema, 0 rows
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE fin_balance_sheet (\
                symbol VARCHAR(20) NOT NULL, \
                report_date DATE NOT NULL, \
                PRIMARY KEY (symbol, report_date))",
            )
            .output()
            .expect("create table");

        import_financial_table("fin_balance_sheet", tmp.path(), tmp.path(), false, None)
            .expect("import_financial_table");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(
            !parquet.exists(),
            "empty table should be skipped due to tiny data"
        );
    }

    fn read_parquet_row_count(path: &std::path::Path) -> usize {
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        duck.query_row(
            &format!("SELECT COUNT(*) FROM read_parquet('{}')", path.display()),
            [],
            |row| row.get(0),
        )
        .expect("count")
    }

    #[test]
    fn financial_table_since_merge_preserves_old_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "CREATE TABLE fin_balance_sheet (\
                symbol VARCHAR(20) NOT NULL, \
                report_date DATE NOT NULL, \
                total_assets DOUBLE, \
                net_profit DOUBLE, \
                PRIMARY KEY (symbol, report_date))",
            )
            .output()
            .expect("create table");

        // Insert 2024 data
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
                ('SH600519', '2024-12-31', 2.6e11, 7e10)")
            .output().expect("insert 2024");

        // First full import
        import_financial_table("fin_balance_sheet", tmp.path(), tmp.path(), false, None)
            .expect("first import");

        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert!(parquet.exists());
        assert_eq!(read_parquet_row_count(&parquet), 1);

        // Insert 2025 data
        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg("INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
                ('SH600519', '2025-12-31', 2.8e11, 8e10)")
            .output().expect("insert 2025");

        // Incremental import with since — triggers merge path
        import_financial_table(
            "fin_balance_sheet",
            tmp.path(),
            tmp.path(),
            false,
            Some("2025-01-01"),
        )
        .expect("incremental import");

        // Should have both rows after merge
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "merge should preserve both rows"
        );
    }

    #[test]
    fn parquet_data_matches_dolt_source() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create table");

        Command::new("dolt")
            .arg("--data-dir").arg(tmp.path())
            .arg("sql").arg("-q")
            .arg(
                "INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, roe, name) VALUES \
                 ('SH600519', '2025-12-31', 1720.54e8, 823.20e8, 65.66, 32.53, '贵州茅台'), \
                 ('SZ000001', '2025-12-31', 1000.00e8, 300.00e8, 2.50, 10.00, '平安银行')",
            )
            .output().expect("insert");

        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("import");

        let parquet_path = tmp.path().join("fin_indicators.parquet");
        assert!(parquet_path.exists());

        // Row count match
        let dolt_rows: usize = String::from_utf8(
            Command::new("dolt")
                .arg("--data-dir")
                .arg(tmp.path())
                .arg("sql")
                .arg("-r")
                .arg("csv")
                .arg("-q")
                .arg("SELECT COUNT(*) FROM fin_indicators")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

        let duck = duckdb::Connection::open_in_memory().unwrap();
        let parquet_rows: usize = duck
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM read_parquet('{}')",
                    parquet_path.display()
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dolt_rows, parquet_rows, "row count mismatch");

        // Data value match
        let revenue: f64 = duck
            .query_row(
                &format!(
                    "SELECT revenue FROM read_parquet('{}') WHERE symbol = 'SH600519'",
                    parquet_path.display()
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            (revenue - 1720.54e8).abs() < 1.0,
            "revenue mismatch: {revenue}"
        );

        // Symbol order preserved
        let symbols: Vec<String> = duck
            .prepare(&format!(
                "SELECT symbol FROM read_parquet('{}') ORDER BY symbol, report_date",
                parquet_path.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(symbols[0], "SH600519");
        assert_eq!(symbols[1], "SZ000001");
    }

    #[test]
    fn run_sepa_capital_tables_export_parquet() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(MAIN_FLOW_SCHEMA)
            .output()
            .expect("create main flow");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow, main_net_inflow_rate) \
                  VALUES ('SH600519', '2026-01-05', 1.2e8, 3.5)")
            .output()
            .expect("insert main flow");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(DRAGON_LIST_SCHEMA)
            .output()
            .expect("create dragon list");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount, sell_amount, net_amount, institution_flag) \
                  VALUES ('SH600519', '2026-01-05', '机构专用', 1e8, 2e8, -1e8, 1)")
            .output()
            .expect("insert dragon list");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(BLOCK_TRADE_SCHEMA)
            .output()
            .expect("create block trade");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO block_trade (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) \
                  VALUES ('SH600519', '2026-01-05', 1500.0, 1e5, 1.5e8, '机构专用', '东方证券', -0.02)")
            .output()
            .expect("insert block trade");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(INSTITUTION_SURVEY_SCHEMA)
            .output()
            .expect("create institution survey");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO institution_survey (symbol, survey_date, org_name, survey_type) \
                  VALUES ('SH600519', '2026-01-05', '华夏基金', '现场调研')",
            )
            .output()
            .expect("insert institution survey");

        for (table, parquet_name) in [
            (CompassTable::MainFlow, "capital_main_flow.parquet"),
            (CompassTable::DragonList, "dragon_list.parquet"),
            (CompassTable::BlockTrade, "block_trade.parquet"),
            (
                CompassTable::InstitutionSurvey,
                "institution_survey.parquet",
            ),
        ] {
            run(
                tmp.path().to_path_buf(),
                tmp.path().to_path_buf(),
                table,
                false,
                None,
            )
            .expect("run table");
            let parquet = tmp.path().join(parquet_name);
            assert!(parquet.exists(), "{parquet_name} should exist after import");
            assert_eq!(
                read_parquet_row_count(&parquet),
                1,
                "{parquet_name} row count"
            );
        }
    }

    #[test]
    fn capital_table_since_merge_new_value_wins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(MAIN_FLOW_SCHEMA)
            .output()
            .expect("create table");

        // Two rows: SZ000001 @ 01-04, SH600519 @ 01-05 (inflow 1.0)
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
                ('SZ000001', '2026-01-04', 0.5e8), \
                ('SH600519', '2026-01-05', 1.0e8)",
            )
            .output()
            .expect("insert initial");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first import");
        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Dolt updated: same PK 01-05 row corrected (1.0 → 2.0), plus new 01-06 row
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE capital_main_flow SET main_net_inflow = 2.0e8 \
                  WHERE symbol = 'SH600519' AND trade_date = '2026-01-05'",
            )
            .output()
            .expect("update 01-05");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
                ('SH600519', '2026-01-06', 3.0e8)",
            )
            .output()
            .expect("insert 01-06");

        // Incremental import with since — triggers merge path
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        // 3 rows: 01-04 preserved, 01-05 new value wins, 01-06 added
        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "merge should keep old rows, replace updated PK, add new rows"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let inflow: f64 = duck
            .query_row(
                &format!(
                    "SELECT main_net_inflow FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2026-01-05'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("inflow");
        assert!(
            (inflow - 2.0e8).abs() < 1.0,
            "new Dolt value must override old parquet value, got {inflow}"
        );
    }

    #[test]
    fn dragon_list_merge_preserves_multiple_seat_types() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(DRAGON_LIST_SCHEMA)
            .output()
            .expect("create table");

        // Same symbol+date, two seat types (three-column PK)
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount) VALUES \
                ('SH600519', '2026-01-05', '机构专用', 1e8), \
                ('SH600519', '2026-01-05', '营业部', 2e8)",
            )
            .output()
            .expect("insert initial");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            None,
        )
        .expect("first import");
        let parquet = tmp.path().join("dragon_list.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Both rows corrected in Dolt
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("UPDATE dragon_list SET buy_amount = 3e8 \
                  WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '机构专用'")
            .output()
            .expect("update institutional");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE dragon_list SET buy_amount = 4e8 \
                  WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '营业部'",
            )
            .output()
            .expect("update broker");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "merge must not collapse distinct seat_type rows"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (seat_type, expected) in [("机构专用", 3e8), ("营业部", 4e8)] {
            let buy: f64 = duck
                .query_row(
                    &format!(
                        "SELECT buy_amount FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '{}'",
                        parquet.display(),
                        seat_type
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("buy amount");
            assert!(
                (buy - expected).abs() < 1.0,
                "{seat_type} buy amount must be new value, got {buy}"
            );
        }
    }

    #[test]
    fn capital_table_skip_tiny_data() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Empty table — import must warn-skip without creating a parquet
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(MAIN_FLOW_SCHEMA)
            .output()
            .expect("create table");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("import with empty table should succeed");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert!(
            !parquet.exists(),
            "empty table should be skipped due to tiny data"
        );
    }

    /// Issue #136: when `data_updates.last_report_date` for a fin table is older
    /// than the 120-day freshness threshold, import-compass must emit a
    /// `freshness` warn (but still succeed — Q5 decision: warn only, never Err).
    /// RED: no warn is emitted today (validation not implemented yet).
    #[test]
    fn fin_indicators_warns_when_data_updates_stale() {
        use std::io::Write;
        use std::sync::{Arc, Mutex};
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // fin_indicators table + 1 row
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES ('SH600519', '2025-12-31', 1.72e11, 8.23e10, 65.66, '贵州茅台')")
            .output()
            .expect("insert");

        // data_updates table with stale last_report_date (200 days ago > 120 threshold)
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)")
            .output()
            .expect("create data_updates");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) VALUES ('fin_indicators', CURDATE(), 'test', 1, DATE_SUB(CURDATE(), INTERVAL 200 DAY))")
            .output()
            .expect("insert data_updates");

        // capture warn output (mirrors screener.rs:634-652 pattern)
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
        let buf = Arc::new(Mutex::new(String::new()));
        let writer = TestWriter(buf.clone());
        let _ = tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_writer(writer)
                .with_max_level(tracing::Level::WARN)
                .finish(),
        );

        import_fin_indicators(tmp.path(), tmp.path(), false, None)
            .expect("import must succeed (freshness is warn-only)");
        let log = buf.lock().expect("lock");
        assert!(
            log.contains("freshness"),
            "stale data_updates must produce a freshness warn, got: {log}"
        );
    }

    /// Issue #136: the incremental-merge path must not silently lose rows.
    /// Old parquet holds 3 physical rows (one duplicate key from a keyless Dolt
    /// table); the merge's ROW_NUMBER dedup collapses to 2 distinct rows →
    /// merged < old must surface as an Err. RED: merge returns Ok today.
    #[test]
    fn fin_indicators_merge_detects_row_loss() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Keyless table (no PRIMARY KEY) so duplicate (symbol, report_date)
        // rows are allowed — full FIN_SCHEMA column set minus the PK
        // constraint, so the 37-column SELECT in import_fin_indicators works.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("CREATE TABLE fin_indicators (symbol VARCHAR(20), report_date DATE, update_date DATE, notice_date DATE, data_type VARCHAR(20), qdate VARCHAR(8), eitime DATETIME, data_year INT, date_label VARCHAR(10), secucode VARCHAR(20), name VARCHAR(100), trade_market VARCHAR(20), trade_market_code VARCHAR(20), trade_market_zjg VARCHAR(10), security_type VARCHAR(10), security_type_code VARCHAR(20), industry VARCHAR(50), board_code VARCHAR(10), board_name VARCHAR(50), ori_board_code VARCHAR(10), org_code VARCHAR(20), is_new TINYINT, basic_eps DOUBLE, deduct_basic_eps DOUBLE, revenue DOUBLE, net_profit DOUBLE, roe DOUBLE, bps DOUBLE, cash_flow_per_share DOUBLE, gross_margin DOUBLE, revenue_yoy DOUBLE, net_profit_yoy DOUBLE, operating_profit_yoy DOUBLE, net_profit_qoq DOUBLE, shares_growth DOUBLE, dividend_plan TEXT, dividend_year VARCHAR(10))")
            .output()
            .expect("create keyless");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES ('SH600519','2024-12-31',1.0e11,'a'), ('SH600519','2024-12-31',1.1e11,'b'), ('SH600519','2025-12-31',1.2e11,'c')")
            .output()
            .expect("insert 3 rows (dup key)");

        // Full import → parquet has 3 physical rows
        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("full import");
        let parquet = tmp.path().join("fin_indicators.parquet");
        assert_eq!(
            crate::validate::parquet_row_count(&parquet).expect("count"),
            3
        );

        // Dolt: delete one dup + insert new 2025 row
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("DELETE FROM fin_indicators WHERE symbol='SH600519' AND report_date='2024-12-31' AND name='a'")
            .output()
            .expect("delete dup");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO fin_indicators VALUES ('SH600519','2025-12-31',1.3e11,'d')")
            .output()
            .expect("insert new");

        // Incremental merge (since filter, existing parquet, no overwrite) →
        // merge dedups on (symbol, report_date): 3 physical old rows → 2 distinct.
        let result = import_fin_indicators(tmp.path(), tmp.path(), false, Some("2025-01-01"));
        assert!(
            result.is_err(),
            "merge losing rows (old physical 3 > merged distinct) must error, got Ok"
        );
    }

    /// Baseline: a faithful full import writes exactly the Dolt row count.
    #[test]
    fn parquet_row_count_matches_dolt_count() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(FIN_SCHEMA)
            .output()
            .expect("create");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, net_profit, basic_eps, name) VALUES ('SH600519','2025-12-31',1.72e11,8.23e10,65.66,'贵州茅台'), ('SZ000001','2025-12-31',1.0e11,3.0e10,2.5,'平安银行')")
            .output()
            .expect("insert 2");

        import_fin_indicators(tmp.path(), tmp.path(), false, None).expect("import");
        let parquet = tmp.path().join("fin_indicators.parquet");
        let dolt_rows =
            crate::validate::dolt_count(tmp.path(), "fin_indicators", "").expect("dolt count");
        let parquet_rows = crate::validate::parquet_row_count(&parquet).expect("parquet count");
        assert_eq!(dolt_rows, 2);
        assert_eq!(parquet_rows, 2);
        crate::validate::verify_row_count(dolt_rows, parquet_rows, "fin_indicators")
            .expect("match");
    }

    /// Baseline: stock_basic full import is row-consistent with its source.
    #[test]
    fn stock_basic_full_import_consistent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY, name VARCHAR(100), industry VARCHAR(50), industry_en VARCHAR(50), list_date VARCHAR(20), delist_date DATE, board VARCHAR(50), full_name VARCHAR(200), total_share DOUBLE, region VARCHAR(50))")
            .output()
            .expect("create");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg("INSERT INTO stock_basic (symbol, name) VALUES ('SH600519','贵州茅台'), ('SZ000001','平安银行')")
            .output()
            .expect("insert 2");

        import_stock_basic(tmp.path(), tmp.path()).expect("import");
        let parquet = tmp.path().join("stock_basic.parquet");
        assert_eq!(
            crate::validate::parquet_row_count(&parquet).expect("count"),
            2
        );
    }

    // ------------------------------------------------------------------
    // bug #298 adversarial tests
    // ------------------------------------------------------------------

    /// Bug #298 RED: with the production Dolt block_trade PK, two real rows can
    /// share the narrow merge key `(symbol, trade_date, price)` but differ in
    /// `volume/amount/buyer/seller`. When a *new* third such row arrives and the
    /// incremental merge dedups on the narrow key, it collapses all three rows
    /// to one and the row-count guard fires as
    /// `row count mismatch: merge lost rows old=2 parquet=1`.
    #[test]
    fn block_trade_merge_preserves_distinct_rows_with_same_narrow_partition_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(BLOCK_TRADE_SCHEMA)
            .output()
            .expect("create block_trade");

        // Two real, distinct block trades: same (symbol, trade_date, price)
        // but different full PK suffix (volume/amount/buyer/seller). This is
        // exactly the data shape the existing narrow `BLOCK_TRADE_SCHEMA`
        // cannot express.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO block_trade \
                 (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) VALUES \
                 ('SH600519', '2026-01-05', 1500.0, 1e5, 1.5e8, '机构专用', '东方证券', -0.02), \
                 ('SH600519', '2026-01-05', 1500.0, 2e5, 3.0e8, 'QFII', '中信证券', 0.01)",
            )
            .output()
            .expect("insert initial rows");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            None,
        )
        .expect("full import");
        let parquet = tmp.path().join("block_trade.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "full import must physically preserve both full-PK rows"
        );

        // A third real row arrives with the same narrow key but yet another
        // full PK suffix.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO block_trade \
                 (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) VALUES \
                 ('SH600519', '2026-01-05', 1500.0, 3e5, 4.5e8, '个人', '国泰君安', 0.03)",
            )
            .output()
            .expect("insert third row");

        // Current code: merge `ROW_NUMBER() OVER (PARTITION BY symbol, trade_date, price)`
        // returns only 1 row → old=2 > merged=1 → Err. RED: this test expects
        // the merge to succeed and all three real rows to survive.
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge must keep all distinct full-PK rows");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all three distinct full-PK rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for volume in [1e5f64, 2e5f64, 3e5f64] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                           AND price = 1500.0 AND volume = {volume}",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by volume");
            assert_eq!(count, 1, "row with volume {volume} must be present");
        }
    }

    /// Bug #298 RED (silent-loss variant): when the old parquet holds exactly
    /// one row for a narrow key and the incremental batch contains a *different*
    /// full-PK row for the same narrow key, the merge is row-count-neutral
    /// (old=1, merged=1) so the "merge lost rows" guard does NOT fire. The old
    /// real row is silently replaced by the new one — historical block trades
    /// are lost without any error. This test asserts both full-PK rows survive.
    #[test]
    fn block_trade_incremental_does_not_silently_replace_distinct_full_pk_row() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(BLOCK_TRADE_SCHEMA)
            .output()
            .expect("create block_trade");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO block_trade \
                 (symbol, trade_date, price, volume, amount, buyer, seller) VALUES \
                 ('SH600519', '2026-01-05', 1500.0, 1e5, 1.5e8, '机构专用', '东方证券')",
            )
            .output()
            .expect("insert first row");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            None,
        )
        .expect("full import");
        let parquet = tmp.path().join("block_trade.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 1);

        // New distinct block trade at same narrow key, different full PK.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO block_trade \
                 (symbol, trade_date, price, volume, amount, buyer, seller) VALUES \
                 ('SH600519', '2026-01-05', 1500.0, 2e5, 3.0e8, 'QFII', '中信证券')",
            )
            .output()
            .expect("insert second row");

        // Current code: merge succeeds (old=1, merged=1) but the ROW_NUMBER
        // dedup on `(symbol, trade_date, price)` keeps only the new row.
        // RED: this expects both rows to be present.
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge should succeed");

        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "distinct full-PK rows must not silently replace each other"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (volume, buyer) in [(1e5f64, "机构专用"), (2e5f64, "QFII")] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                           AND price = 1500.0 AND volume = {volume} AND buyer = '{buyer}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by full-pk suffix");
            assert_eq!(count, 1, "row {volume}/{buyer} must survive");
        }
    }

    /// Bug #298 RED + prefer_new semantics: one existing full-PK row is
    /// corrected (non-PK column changes so the full PK stays identical), and a
    /// new distinct full-PK row arrives on the same narrow key. With the
    /// production PK the merge must keep the old sibling, apply the corrected
    /// value to the updated row, and add the new row. The current narrow
    /// partition collapses all of them to one row (new-priority wins) and
    /// errors with `row count mismatch`.
    #[test]
    fn block_trade_merge_prefer_new_updates_same_full_pk_without_losing_siblings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(BLOCK_TRADE_SCHEMA)
            .output()
            .expect("create block_trade");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO block_trade \
                 (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) VALUES \
                 ('SH600519', '2026-01-05', 1500.0, 1e5, 1.5e8, '机构专用', '东方证券', -0.02), \
                 ('SH600519', '2026-01-05', 1500.0, 2e5, 3.0e8, 'QFII', '中信证券', 0.01)",
            )
            .output()
            .expect("insert initial rows");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            None,
        )
        .expect("full import");
        let parquet = tmp.path().join("block_trade.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Correct the QFII row's non-PK premium_rate, preserving its full PK.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE block_trade SET premium_rate = 0.05 \
                 WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                   AND price = 1500.0 AND volume = 2e5 AND amount = 3.0e8 \
                   AND buyer = 'QFII' AND seller = '中信证券'",
            )
            .output()
            .expect("update same full PK");
        // Also add a brand-new distinct full-PK row on the same narrow key.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO block_trade \
                 (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) VALUES \
                 ('SH600519', '2026-01-05', 1500.0, 3e5, 4.5e8, '个人', '国泰君安', 0.03)",
            )
            .output()
            .expect("insert third row");

        // Current code: merge groups by narrow key, so it cannot express
        // "update the QFII row while preserving the institutional row and adding
        // the individual row"; it errors with old=2 parquet=1. RED.
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge must succeed");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "old sibling + updated row + new distinct row must all survive"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let premium: f64 = duck
            .query_row(
                &format!(
                    "SELECT premium_rate FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                       AND price = 1500.0 AND volume = 2e5 AND amount = 3.0e8 \
                       AND buyer = 'QFII' AND seller = '中信证券'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("premium");
        assert!(
            (premium - 0.05).abs() < 1e-9,
            "prefer_new must surface the corrected premium_rate for the same full PK"
        );
    }

    /// Non-block_trade drift guard: `dragon_list` uses the production PK
    /// `(symbol, trade_date, seat_type)` as its merge partition, so all three
    /// seat-type rows (same symbol/date but distinct PK) must survive an
    /// incremental merge with prefer_new corrections. This currently passes;
    /// it locks the invariant so a future change that narrows the partition
    /// (the bug #298 class) is caught.
    #[test]
    fn dragon_list_merge_with_production_pk_preserves_all_seat_types() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(DRAGON_LIST_SCHEMA)
            .output()
            .expect("create dragon_list");

        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount, sell_amount) VALUES \
                 ('SH600519', '2026-01-05', '机构专用', 1e8, 2e8), \
                 ('SH600519', '2026-01-05', '营业部', 3e8, 4e8)",
            )
            .output()
            .expect("insert initial seat types");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            None,
        )
        .expect("full import dragon_list");
        let parquet = tmp.path().join("dragon_list.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Correct both existing seat-type rows (same full PK, prefer_new) and
        // add a third distinct seat type.
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE dragon_list SET buy_amount = 5e8 \
                 WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '机构专用'",
            )
            .output()
            .expect("update institutional");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "UPDATE dragon_list SET buy_amount = 6e8 \
                 WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' AND seat_type = '营业部'",
            )
            .output()
            .expect("update broker");
        Command::new("dolt")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg("sql")
            .arg("-q")
            .arg(
                "INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount, sell_amount) VALUES \
                 ('SH600519', '2026-01-05', 'QFII', 7e8, 8e8)",
            )
            .output()
            .expect("insert new seat type");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge dragon_list");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all distinct seat types must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (seat, expected) in [("机构专用", 5e8), ("营业部", 6e8), ("QFII", 7e8)] {
            let buy: f64 = duck
                .query_row(
                    &format!(
                        "SELECT buy_amount FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                           AND seat_type = '{seat}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("buy amount");
            assert!(
                (buy - expected).abs() < 1.0,
                "{seat} buy_amount must be {expected}, got {buy}"
            );
        }
    }

    // ------------------------------------------------------------------
    // bug #298 requirement acceptance tests
    // ------------------------------------------------------------------
    //
    // These are independent from the adversarial block above. The
    // requirement contract (issue #298 + audited production Dolt schemas):
    // every append/import-compass table must deduplicate on the *full*
    // production Dolt primary key during incremental merge. Narrowing the
    // code's `partition_cols` must not silently drop real historical rows.

    /// Run a Dolt SQL statement in the test Dolt dir, asserting success.
    fn dolt_sql(dolt_dir: &std::path::Path, sql: &str) {
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
            "dolt sql failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Production Dolt `index_daily` schema: PK `(symbol, trade_date)`.
    /// The parquet export renames `trade_date` → `tradedate`, so the merge
    /// partition is the parquet-side `symbol, tradedate`.
    const INDEX_DAILY_PRODUCTION_SCHEMA: &str = "\
        CREATE TABLE index_daily (\
        symbol VARCHAR(20) NOT NULL, \
        trade_date DATE NOT NULL, \
        index_type VARCHAR(20) NOT NULL, \
        open DOUBLE, close DOUBLE, high DOUBLE, low DOUBLE, \
        volume DOUBLE, amount DOUBLE, \
        update_date DATE, \
        PRIMARY KEY (symbol, trade_date))";

    /// Production Dolt `index_basic` schema: full-overwrite table, PK
    /// `(symbol)` only; no incremental merge path.
    const INDEX_BASIC_PRODUCTION_SCHEMA: &str = "\
        CREATE TABLE index_basic (\
        symbol VARCHAR(20) NOT NULL, \
        name VARCHAR(100), index_type VARCHAR(20), name_en VARCHAR(100), \
        PRIMARY KEY (symbol))";

    /// Requirement contract (bug #298): `block_trade`'s production Dolt PK is
    /// `(symbol, trade_date, price, volume, amount, buyer, seller)`. Rows that
    /// share the current narrow merge key `(symbol, trade_date, price)` are
    /// distinct real rows and must all survive both full import and
    /// incremental `--since` merge.
    ///
    /// RED (current code): the incremental merge partitions by the narrow key
    /// and collapses all same-narrow-key rows to one, causing a
    /// `row count mismatch` error (old=2, merged=1) when more than one old
    /// row exists. The `run(...).expect(...)` below therefore fails on the
    /// current buggy code.
    ///
    /// GREEN (after fix): the merge partitions by the production full PK, so
    /// the three distinct full-PK rows survive and the count assertion passes.
    #[test]
    fn block_trade_requirement_acceptance_full_pk_dedup_preserves_all_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        dolt_sql(tmp.path(), BLOCK_TRADE_SCHEMA);

        // Two real, distinct block trades: same (symbol, trade_date, price)
        // narrow key but different production-PK suffix
        // (volume/amount/buyer/seller).
        dolt_sql(
            tmp.path(),
            "INSERT INTO block_trade \
             (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) VALUES \
             ('SH600519', '2026-01-05', 1500.0, 1e5, 1.5e8, '机构专用', '东方证券', -0.02), \
             ('SH600519', '2026-01-05', 1500.0, 2e5, 3.0e8, 'QFII', '中信证券', 0.01)",
        );

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            None,
        )
        .expect("full import block_trade");
        let parquet = tmp.path().join("block_trade.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "full import must physically preserve both production-PK rows"
        );

        // A third real row arrives with the same narrow key but yet another
        // production-PK suffix.
        dolt_sql(
            tmp.path(),
            "INSERT INTO block_trade \
             (symbol, trade_date, price, volume, amount, buyer, seller, premium_rate) VALUES \
             ('SH600519', '2026-01-05', 1500.0, 3e5, 4.5e8, '个人', '国泰君安', 0.03)",
        );

        // Current code: `ROW_NUMBER() OVER (PARTITION BY symbol, trade_date, price)`
        // returns one row for the three old/new rows, so old=2 > merged=1 and
        // `import_append_table` returns Err("row count mismatch ..."). RED.
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::BlockTrade,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge must keep all distinct production-PK rows");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all three distinct production-PK rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (volume, buyer) in [(1e5f64, "机构专用"), (2e5f64, "QFII"), (3e5f64, "个人")] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                           AND price = 1500.0 AND volume = {volume} AND buyer = '{buyer}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by full PK suffix");
            assert_eq!(
                count, 1,
                "production-PK row with volume {volume}/buyer {buyer} must be present"
            );
        }
    }

    /// Requirement contract (bug #298): `fin_indicators` production PK is
    /// `(symbol, report_date)`. The existing `FIN_SCHEMA` uses exactly that PK.
    /// Two different report dates for the same symbol are distinct real rows;
    /// if a future change narrows the merge partition to `symbol`, the merge
    /// collapses them and the row-count guard fires / rows are lost.
    ///
    /// GREEN (current code): partition is `symbol, report_date`, so all three
    /// rows survive.
    #[test]
    fn fin_indicators_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), FIN_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES \
             ('SH600519', '2024-12-31', 1.0e11, 'a'), \
             ('SH600519', '2025-12-31', 1.1e11, 'b')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            None,
        )
        .expect("full import fin_indicators");
        let parquet = tmp.path().join("fin_indicators.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES \
             ('SH600519', '2026-12-31', 1.2e11, 'c')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            Some("2026-01-01"),
        )
        .expect("incremental merge fin_indicators");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, report_date) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for report_date in ["2024-12-31", "2025-12-31", "2026-12-31"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND report_date = '{report_date}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by report_date");
            assert_eq!(count, 1, "report_date {report_date} must survive");
        }
    }

    /// Requirement contract (bug #298): `fin_balance_sheet` production PK is
    /// `(symbol, report_date)` (same as fin_indicators). This pair — two
    /// different report_dates for the same symbol — is the minimal detector
    /// for a regression that narrows the partition to `symbol`.
    ///
    /// GREEN (current code): partition is `symbol, report_date`.
    #[test]
    fn fin_balance_sheet_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE fin_balance_sheet (\
             symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
             total_assets DOUBLE, net_profit DOUBLE, \
             PRIMARY KEY (symbol, report_date))",
        );

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_balance_sheet (symbol, report_date, total_assets) VALUES \
             ('SH600519', '2024-12-31', 2.6e11), \
             ('SH600519', '2025-12-31', 2.7e11)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinBalanceSheet,
            false,
            None,
        )
        .expect("full import fin_balance_sheet");
        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_balance_sheet (symbol, report_date, total_assets) VALUES \
             ('SH600519', '2026-12-31', 2.8e11)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinBalanceSheet,
            false,
            Some("2026-01-01"),
        )
        .expect("incremental merge fin_balance_sheet");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, report_date) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for report_date in ["2024-12-31", "2025-12-31", "2026-12-31"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND report_date = '{report_date}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by report_date");
            assert_eq!(count, 1, "report_date {report_date} must survive");
        }
    }

    /// Requirement contract (bug #298): `fin_income` production PK is
    /// `(symbol, report_date)`. Same drift-guard shape as fin_balance_sheet.
    ///
    /// GREEN (current code): partition is `symbol, report_date`.
    #[test]
    fn fin_income_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE fin_income (\
             symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
             total_operate_income DOUBLE, net_profit DOUBLE, \
             PRIMARY KEY (symbol, report_date))",
        );

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_income (symbol, report_date, total_operate_income) VALUES \
             ('SH600519', '2024-12-31', 1.0e11), \
             ('SH600519', '2025-12-31', 1.1e11)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIncome,
            false,
            None,
        )
        .expect("full import fin_income");
        let parquet = tmp.path().join("fin_income.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_income (symbol, report_date, total_operate_income) VALUES \
             ('SH600519', '2026-12-31', 1.2e11)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIncome,
            false,
            Some("2026-01-01"),
        )
        .expect("incremental merge fin_income");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, report_date) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for report_date in ["2024-12-31", "2025-12-31", "2026-12-31"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND report_date = '{report_date}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by report_date");
            assert_eq!(count, 1, "report_date {report_date} must survive");
        }
    }

    /// Requirement contract (bug #298): `fin_cash_flow` production PK is
    /// `(symbol, report_date)`. Same drift-guard shape as fin_balance_sheet.
    ///
    /// GREEN (current code): partition is `symbol, report_date`.
    #[test]
    fn fin_cash_flow_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(
            tmp.path(),
            "CREATE TABLE fin_cash_flow (\
             symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
             netcash_operate DOUBLE, PRIMARY KEY (symbol, report_date))",
        );

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_cash_flow (symbol, report_date, netcash_operate) VALUES \
             ('SH600519', '2024-12-31', 9.0e10), \
             ('SH600519', '2025-12-31', 9.1e10)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinCashFlow,
            false,
            None,
        )
        .expect("full import fin_cash_flow");
        let parquet = tmp.path().join("fin_cash_flow.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_cash_flow (symbol, report_date, netcash_operate) VALUES \
             ('SH600519', '2026-12-31', 9.2e10)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinCashFlow,
            false,
            Some("2026-01-01"),
        )
        .expect("incremental merge fin_cash_flow");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, report_date) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for report_date in ["2024-12-31", "2025-12-31", "2026-12-31"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND report_date = '{report_date}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by report_date");
            assert_eq!(count, 1, "report_date {report_date} must survive");
        }
    }

    /// Requirement contract (bug #298): `capital_main_flow` production PK is
    /// `(symbol, trade_date)`. The existing `MAIN_FLOW_SCHEMA` uses exactly
    /// that PK. Two distinct trade dates for the same symbol are the minimal
    /// pair that detects a future partition narrowed to `symbol`.
    ///
    /// GREEN (current code): partition is `symbol, trade_date`.
    #[test]
    fn capital_main_flow_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-04', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("full import capital_main_flow");
        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-06', 3.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge capital_main_flow");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, trade_date) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for trade_date in ["2026-01-04", "2026-01-05", "2026-01-06"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '{trade_date}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by trade_date");
            assert_eq!(count, 1, "trade_date {trade_date} must survive");
        }
    }

    /// Requirement contract (bug #298): `dragon_list` production PK is
    /// `(symbol, trade_date, seat_type)`. `DRAGON_LIST_SCHEMA` uses exactly
    /// that PK. Two different seat_types on the same symbol/date are the
    /// minimal pair that detects a future partition narrowed to
    /// `(symbol, trade_date)` (which would collapse the seat types).
    ///
    /// GREEN (current code): partition is `symbol, trade_date, seat_type`.
    #[test]
    fn dragon_list_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), DRAGON_LIST_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount) VALUES \
             ('SH600519', '2026-01-05', '机构专用', 1e8), \
             ('SH600519', '2026-01-05', '营业部', 2e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            None,
        )
        .expect("full import dragon_list");
        let parquet = tmp.path().join("dragon_list.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO dragon_list (symbol, trade_date, seat_type, buy_amount) VALUES \
             ('SH600519', '2026-01-05', 'QFII', 3e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::DragonList,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge dragon_list");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, trade_date, seat_type) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for seat_type in ["机构专用", "营业部", "QFII"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '2026-01-05' \
                           AND seat_type = '{seat_type}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by seat_type");
            assert_eq!(count, 1, "seat_type {seat_type} must survive");
        }
    }

    /// Requirement contract (bug #298): `institution_survey` production PK is
    /// `(symbol, survey_date, org_name)`. The existing
    /// `INSTITUTION_SURVEY_SCHEMA` uses exactly that PK. Two different
    /// org_names on the same symbol/survey_date are the minimal pair that
    /// detects a future partition narrowed to `(symbol, survey_date)`.
    ///
    /// GREEN (current code): partition is `symbol, survey_date, org_name`.
    #[test]
    fn institution_survey_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), INSTITUTION_SURVEY_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO institution_survey (symbol, survey_date, org_name) VALUES \
             ('SH600519', '2026-01-05', '华夏基金'), \
             ('SH600519', '2026-01-05', '南方基金')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::InstitutionSurvey,
            false,
            None,
        )
        .expect("full import institution_survey");
        let parquet = tmp.path().join("institution_survey.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO institution_survey (symbol, survey_date, org_name) VALUES \
             ('SH600519', '2026-01-05', '易方达基金')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::InstitutionSurvey,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge institution_survey");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, survey_date, org_name) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for org_name in ["华夏基金", "南方基金", "易方达基金"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND survey_date = '2026-01-05' \
                           AND org_name = '{org_name}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by org_name");
            assert_eq!(count, 1, "org_name {org_name} must survive");
        }
    }

    /// Requirement contract (bug #298): `index_daily` production Dolt PK is
    /// `(symbol, trade_date)`, but the parquet export renames Dolt
    /// `trade_date` → `tradedate`. The merge partition must therefore use the
    /// parquet-side `symbol, tradedate` — a regression that narrows it to
    /// `symbol` would collapse distinct dates.
    ///
    /// GREEN (current code): partition is `symbol, tradedate` (the merge
    /// operates on the parquet column name).
    #[test]
    fn index_daily_requirement_drift_guard_preserves_full_pk_rows() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), INDEX_DAILY_PRODUCTION_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO index_daily \
             (symbol, trade_date, index_type, open, close, high, low, volume, amount) VALUES \
             ('SH000001', '2026-01-01', 'index', 3000.0, 3010.0, 3020.0, 2990.0, 1.0e8, 2.0e8), \
             ('SH000001', '2026-01-02', 'index', 3010.0, 3020.0, 3030.0, 3000.0, 1.1e8, 2.1e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexDaily,
            false,
            None,
        )
        .expect("full import index_daily");
        let parquet = tmp.path().join("index_daily.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        dolt_sql(
            tmp.path(),
            "INSERT INTO index_daily \
             (symbol, trade_date, index_type, open, close, high, low, volume, amount) VALUES \
             ('SH000001', '2026-01-03', 'index', 3020.0, 3030.0, 3040.0, 3010.0, 1.2e8, 2.2e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexDaily,
            false,
            Some("2026-01-03"),
        )
        .expect("incremental merge index_daily");

        assert_eq!(
            read_parquet_row_count(&parquet),
            3,
            "all (symbol, tradedate) rows must survive the merge"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        // Verify through the parquet-side `tradedate` column, not the Dolt
        // `trade_date` name.
        for tradedate in ["2026-01-01", "2026-01-02", "2026-01-03"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH000001' AND tradedate = '{tradedate}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by tradedate");
            assert_eq!(count, 1, "tradedate {tradedate} must survive");
        }
    }

    /// Requirement contract (bug #298): `index_basic` is a full-overwrite
    /// table with no incremental merge. Repeated `run` calls, including with
    /// a `--since` value (which must be ignored), must always mirror the full
    /// current Dolt state — no merge-based row loss or stale rows.
    ///
    /// GREEN (current code): `import_index_basic` always does a full export.
    #[test]
    fn index_basic_requirement_guard_always_full_overwrite() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), INDEX_BASIC_PRODUCTION_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO index_basic (symbol, name, index_type) VALUES \
             ('SH000001', '上证指数', 'index'), \
             ('SZ399001', '深证成指', 'index')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexBasic,
            false,
            None,
        )
        .expect("first full overwrite index_basic");
        let parquet = tmp.path().join("index_basic.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 2);

        // Dolt changes: one insert + one delete. A second run with a --since
        // value must still mirror the full Dolt state (the full-overwrite path
        // ignores since/overwrite), proving there is no incremental-merge loss.
        dolt_sql(
            tmp.path(),
            "INSERT INTO index_basic (symbol, name, index_type) VALUES \
             ('SH000300', '沪深300', 'index')",
        );
        dolt_sql(
            tmp.path(),
            "DELETE FROM index_basic WHERE symbol = 'SZ399001'",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexBasic,
            false,
            Some("2026-01-01"),
        )
        .expect("second full overwrite index_basic (since ignored)");

        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "index_basic must mirror Dolt after a full overwrite"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let symbols: Vec<String> = duck
            .prepare(&format!(
                "SELECT symbol FROM read_parquet('{}') ORDER BY symbol",
                parquet.display()
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(
            symbols,
            vec!["SH000001", "SH000300"],
            "deleted Dolt row must disappear and new row must appear"
        );
    }

    /// Requirement contract (bug #298): when the DuckDB incremental merge
    /// fails (e.g. corrupt existing parquet), the fallback in
    /// `import_append_table` must not silently discard historical rows by
    /// writing the `--since`-filtered export. The fallback must perform a
    /// true full export (no `--since` filter) and write the complete Dolt
    /// state back to the parquet, preserving history.
    ///
    /// RED (current code): on merge failure the fallback writes `new_data`,
    /// which was produced with `WHERE report_date >= '2025-01-01'`; the 2024
    /// historical row is lost, so the parquet contains only 1 row and the
    /// `assert_eq!(..., 2)` below fails.
    ///
    /// GREEN (after fix): the fallback re-exports the whole table without the
    /// `--since` filter, so both the 2024 and 2025 rows survive.
    #[test]
    fn financial_table_merge_failure_falls_back_to_full_export_preserves_history() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        dolt_sql(
            tmp.path(),
            "CREATE TABLE fin_balance_sheet (\
             symbol VARCHAR(20) NOT NULL, report_date DATE NOT NULL, \
             total_assets DOUBLE, net_profit DOUBLE, \
             PRIMARY KEY (symbol, report_date))",
        );

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
             ('SH600519', '2024-12-31', 2.6e11, 7e10)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinBalanceSheet,
            false,
            None,
        )
        .expect("first full import fin_balance_sheet");
        let parquet = tmp.path().join("fin_balance_sheet.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 1);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_balance_sheet (symbol, report_date, total_assets, net_profit) VALUES \
             ('SH600519', '2025-12-31', 2.8e11, 8e10)",
        );
        // Corrupt the existing parquet so the DuckDB incremental-merge step
        // fails and exercises the fallback path.
        std::fs::write(&parquet, b"corrupted parquet").expect("corrupt parquet");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinBalanceSheet,
            false,
            Some("2025-01-01"),
        )
        .expect("fallback after merge failure must succeed");

        // RED on current code: fallback writes only the since-filtered 2025
        // row, dropping the 2024 historical row -> count is 1, not 2.
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "fallback must preserve historical rows by doing a full export"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for report_date in ["2024-12-31", "2025-12-31"] {
            let count: i64 = duck
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND report_date = '{report_date}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("count by report_date");
            assert_eq!(count, 1, "report_date {report_date} must survive fallback");
        }
    }
}
