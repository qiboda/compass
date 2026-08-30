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
    /// Stock basic info (symbol/name/industry/board), full replace.
    StockBasic,
    /// Financial indicators (fin_indicators), incremental merge on report_date.
    FinIndicators,
    /// Balance sheet metrics (fin_balance_sheet), incremental merge on report_date.
    FinBalanceSheet,
    /// Income statement metrics (fin_income), incremental merge on report_date.
    FinIncome,
    /// Cash flow metrics (fin_cash_flow), incremental merge on report_date.
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
                    parquet_date_col: None,
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
                    parquet_date_col: None,
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
                    parquet_date_col: None,
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
                    parquet_date_col: None,
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
                    // #343: the export renames Dolt `trade_date` → `tradedate`,
                    // so the history check filters the parquet side on the
                    // parquet column name.
                    parquet_date_col: Some("tradedate"),
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
            parquet_date_col: None,
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
            parquet_date_col: None,
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
    /// Parquet-side date column that carries `date_col` after the export
    /// rename (e.g. index_daily `trade_date` → `tradedate`). `None` → use
    /// `date_col`. Only the #343 history check needs this mapping.
    parquet_date_col: Option<&'a str>,
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

/// #343: verify the old parquet mirrors Dolt for all rows older than `since`
/// before trusting the incremental merge. `Ok(true)` = identical row sets;
/// `Ok(false)` = divergent or unreadable — callers must fall back to a full
/// export. Both sides are produced by `run_dolt_sql_parquet` with the same
/// `select_cols`, so their column names/types line up for a bidirectional
/// EXCEPT comparison, which detects missing, stale, orphaned and deleted rows
/// alike. `parquet_date_col` is the parquet-side name of `date_col` after the
/// export rename (e.g. index_daily `trade_date` → `tradedate`). Any read or
/// type error is treated conservatively as divergence (`Ok(false)`), matching
/// the existing "corrupt parquet triggers a full-export recovery" semantics.
///
/// The `Result` wrapper is kept for a future strict-failure mode; today every
/// internal error is deliberately mapped to `Ok(false)` (conservative
/// fallback) and the `?` at the call site never fires. Cost note: this runs
/// two EXCEPTs over the FULL `< since` history on every incremental import —
/// O(history) but bounded by the table's pre-since rows, accepted in exchange
/// for detecting auto-heal divergence (see decision record).
#[allow(clippy::too_many_arguments)]
fn incremental_history_matches(
    dolt_dir: &Path,
    path: &Path,
    table_name: &str,
    date_col: &str,
    parquet_date_col: &str,
    select_cols: &str,
    order_cols: &str,
    since: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let query = format!(
        "SELECT {select_cols} FROM {table_name} WHERE {date_col} < '{since}' \
         ORDER BY {order_cols}"
    );
    let hist_data = match run_dolt_sql_parquet(dolt_dir, &query) {
        Ok(data) => data,
        Err(e) => {
            warn!("{table_name}: history slice export failed ({e}); assuming divergent");
            return Ok(false);
        }
    };
    let hist_path = unique_work_path(&format!("{table_name}.hist"));
    if let Err(e) = std::fs::write(&hist_path, &hist_data) {
        // Disk-full/interrupted writes can leave a partial staging file behind.
        let _ = std::fs::remove_file(&hist_path);
        warn!("{table_name}: could not stage history slice ({e}); assuming divergent");
        return Ok(false);
    }
    let result = (|| -> Result<bool, Box<dyn std::error::Error>> {
        let duck = Connection::open_in_memory()?;
        // Positional (SELECT *) EXCEPT on both sides — the invariant "old
        // parquet column order == current select_cols order" must hold (both
        // sides come from the same select_cols, see doc above); if a future
        // schema change breaks it, DuckDB raises a column-count/type mismatch
        // -> Err -> Ok(false) -> full export (safe, but worth knowing).
        // Dolt history rows not present (by value) in the old parquet slice.
        let dolt_extra: i64 = duck.query_row(
            &format!(
                "SELECT COUNT(*) FROM (SELECT * FROM read_parquet('{}') EXCEPT \
                 SELECT * FROM read_parquet('{}') WHERE {parquet_date_col} < '{since}')",
                hist_path.display(),
                path.display(),
            ),
            [],
            |row| row.get(0),
        )?;
        // Old-parquet history rows not present in Dolt (orphans to remove).
        let parquet_extra: i64 = duck.query_row(
            &format!(
                "SELECT COUNT(*) FROM (SELECT * FROM read_parquet('{}') \
                 WHERE {parquet_date_col} < '{since}' EXCEPT \
                 SELECT * FROM read_parquet('{}'))",
                path.display(),
                hist_path.display(),
            ),
            [],
            |row| row.get(0),
        )?;
        Ok(dolt_extra == 0 && parquet_extra == 0)
    })();
    let _ = std::fs::remove_file(&hist_path);
    match result {
        Ok(matches) => Ok(matches),
        Err(e) => {
            warn!("{table_name}: history comparison failed ({e}); assuming divergent");
            Ok(false)
        }
    }
}

/// #343: full-export recovery shared by the DuckDB-merge-failure fallback and
/// the history-divergence path: preserve the pre-merge parquet for diagnosis,
/// rerun the Dolt query WITHOUT the `--since` filter, write a genuinely full
/// export, then validate against the full Dolt row count (the old parquet may
/// be corrupt, so a merge-level no-loss comparison cannot be relied on).
///
/// Backup retention policy (decision): the `{table}.pre_merge_backup_*` files
/// are kept indefinitely for diagnosis, with no rotation — they live under
/// the OS temp dir, are named with pid+SEQ (locatable), and are expected to
/// be reclaimed by the system's temp cleaner; a fallback is rare (only on
/// divergence or merge failure), so the accumulation rate is bounded in
/// practice. Deliberately no auto-rotation here: deleting another process's
/// backup (or the only record of a divergence) would destroy the diagnostic
/// value this file exists for.
fn recover_full_export(
    dolt_dir: &Path,
    path: &Path,
    table_name: &str,
    select_cols: &str,
    order_cols: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Bug #298: preserve the pre-merge parquet before the fallback overwrites
    // it, so a schema mismatch can be diagnosed afterwards.
    if path.exists() {
        let backup_path = unique_work_path(&format!("{table_name}.pre_merge_backup"));
        match std::fs::copy(path, &backup_path) {
            Ok(_) => warn!(
                "preserved pre-merge parquet for diagnosis at {}",
                backup_path.display()
            ),
            Err(copy_err) => warn!("failed to preserve pre-merge parquet: {copy_err}"),
        }
    }
    let full_query = format!("SELECT {select_cols} FROM {table_name} ORDER BY {order_cols}");
    let full_data = run_dolt_sql_parquet(dolt_dir, &full_query)?;
    std::fs::write(path, &full_data)?;
    let src_count = crate::validate::dolt_count(dolt_dir, table_name, "")?;
    let parquet_count = crate::validate::parquet_row_count(path)?;
    crate::validate::verify_row_count(src_count, parquet_count, table_name)?;
    Ok(())
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
        parquet_date_col,
        partition_cols,
        dolt_order_cols,
        select_cols,
        prefer_new,
    } = spec;
    let parquet_name = format!("{table_name}.parquet");
    let path = output.join(&parquet_name);

    // First export: never apply `--since` filtering even when the Dolt table
    // already has a data_updates anchor. Writing a since-filtered slice as the
    // initial parquet would permanently lose history.
    let effective_since = if path.exists() { since } else { None };
    let select_cols = select_cols.unwrap_or("*");
    let order_cols = dolt_order_cols.unwrap_or(partition_cols);

    // #343: the history check stages its Dolt slice under
    // `temp_dir()/compass_parquet_work` (see `unique_work_path`), so the
    // directory must exist BEFORE the check runs — otherwise the stage write
    // fails, the check conservatively reports divergence, and every
    // incremental run would silently degrade to a full export without ever
    // reaching the merge branch's own create_dir_all below (the directory
    // would then never be created). Existing fallback/merge paths that also
    // write staging files keep their own create_dir_all as a defense.
    std::fs::create_dir_all(std::env::temp_dir().join("compass_parquet_work"))?;

    // #343: before trusting the incremental merge, verify the old parquet
    // mirrors Dolt for all rows older than `since`. This MUST run before the
    // date-filtered export and the tiny-data skip: an auto-heal backfill that
    // only adds rows older than `--since` yields an empty or tiny `>= since`
    // slice and would otherwise be skipped without ever repairing the parquet
    // (the skip below returns before the merge branch).
    if let Some(s) = effective_since
        && !overwrite
        && path.exists()
    {
        let parquet_date_col = parquet_date_col.unwrap_or(date_col);
        if !incremental_history_matches(
            dolt_dir,
            &path,
            table_name,
            date_col,
            parquet_date_col,
            select_cols,
            order_cols,
            s,
        )? {
            warn!(
                "{table_name}: history divergence before --since merge; \
                 falling back to full export"
            );
            recover_full_export(dolt_dir, &path, table_name, select_cols, order_cols)?;
            info!("  → {}", path.display());
            return Ok(());
        }
    }

    let date_filter = match effective_since {
        Some(s) if !s.is_empty() => format!(" WHERE {date_col} >= '{s}'"),
        _ => String::new(),
    };
    let query =
        format!("SELECT {select_cols} FROM {table_name}{date_filter} ORDER BY {order_cols}");

    info!("Exporting {table_name}...");
    let new_data = run_dolt_sql_parquet(dolt_dir, &query)?;
    if new_data.len() < 500 {
        warn!("{table_name} returned empty or tiny data, skipping");
        return Ok(());
    }

    if effective_since.is_some() && !overwrite && path.exists() {
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
            // #343: keep `priority`/`rn` for the WHERE dedupe only — never
            // write the internal columns into the production parquet.
            // Positional UNION ALL: both sides must expose the same column
            // order (old parquet == current select_cols); if a schema change
            // breaks it, DuckDB raises a mismatch -> fallback (safe).
            "COPY (SELECT * EXCLUDE (priority, rn) FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY {partition_cols} ORDER BY {priority_order}) AS rn \
             FROM (SELECT *, 1 AS priority FROM read_parquet('{}') \
             UNION ALL SELECT *, 2 FROM read_parquet('{}'))) WHERE rn = 1 ORDER BY {partition_cols}) \
             TO '{}' (FORMAT PARQUET)",
            path.display(),
            new_path.display(),
            tmp_path.display(),
        );
        if let Err(e) = duck.execute_batch(&sql) {
            warn!("DuckDB merge failed: {e}, falling back to full export");
            // #343/#298: the fallback must NOT write the `--since`-filtered
            // `new_data` over the full parquet — that is exactly bug #298's
            // history-loss path. `recover_full_export` preserves the pre-merge
            // parquet for diagnosis, reruns the Dolt query without the date
            // filter, and validates against the full Dolt row count (the old
            // parquet may be corrupt, so a merge-level no-loss comparison
            // cannot be relied on here).
            recover_full_export(dolt_dir, &path, table_name, select_cols, order_cols)?;
        } else {
            // No-loss guard against the merge result BEFORE it overwrites the
            // production parquet: if the guard trips (old parquet with exact
            // duplicate rows — EXCEPT is duplicate-insensitive; unreachable in
            // production because every append table has a unique PK), the
            // production file must remain untouched so `--overwrite` can
            // recover it.
            let merged_count = crate::validate::parquet_row_count(&tmp_path)?;
            if let Some(old_rows) = old_count
                && merged_count < old_rows
            {
                return Err(format!(
                    "row count mismatch: merge lost rows old={old_rows} parquet={merged_count} (table {table_name})"
                )
                .into());
            }
            std::fs::copy(&tmp_path, &path)?;
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
    fn append_table_first_export_with_since_imports_full_history() {
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

        // Two historical rows exist in Dolt before the first Parquet export.
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
            .expect("insert history");

        // First export with a --since anchor still writes the full history:
        // the parquet file does not exist yet, so `--since` must be ignored.
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("first export with since");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            2,
            "first export must not be truncated by --since"
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
    /// Before fix: no warn was emitted today (validation not implemented yet).
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

    /// Issue #136 + #343: the incremental-merge path must not silently lose
    /// rows. Old parquet holds 3 physical rows (one duplicate key from a
    /// keyless Dolt table) while Dolt has diverged (one dup deleted, a new
    /// 2025 row inserted). Before #136 the merge silently collapsed to 2
    /// distinct rows; the merged < old count guard surfaced the loss as Err.
    /// Since #343 the divergence is detected *before* the merge by the history
    /// check and repaired via a full export (parquet == Dolt, no loss); the
    /// count guard remains as the fast-path safety net (reachable when the
    /// parquet holds exact-duplicate rows that EXCEPT cannot distinguish).
    #[test]
    fn fin_indicators_merge_detects_row_loss() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());

        // Keyless table (no PRIMARY KEY) so duplicate (symbol, report_date)
        // rows are allowed — full FIN_SCHEMA column set minus the PK
        // constraint, so the selected columns in import_fin_indicators work.
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
            .arg("INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES ('SH600519','2025-12-31',1.3e11,'d')")
            .output()
            .expect("insert new");

        // Incremental merge (since filter, existing parquet, no overwrite):
        // #343 detects the orphaned parquet row ('a' deleted in Dolt) and
        // repairs via a full export — parquet must mirror Dolt exactly, with
        // none of the 3 Dolt physical rows lost.
        import_fin_indicators(tmp.path(), tmp.path(), false, Some("2025-01-01"))
            .expect("divergence must repair via full export (plan #343)");
        assert_eq!(
            crate::validate::parquet_row_count(&parquet).expect("count"),
            3,
            "full export keeps all 3 Dolt physical rows (no silent loss)"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let revenues: Vec<(String, f64)> = duck
            .prepare(&format!(
                "SELECT CAST(report_date AS VARCHAR), revenue FROM read_parquet('{}') \
                 ORDER BY report_date, revenue",
                parquet.display()
            ))
            .expect("prepare")
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .expect("query")
            .filter_map(|r| r.ok())
            // Dolt DATE exports as TIMESTAMP in parquet; compare by day only.
            .map(|(d, v)| (d.chars().take(10).collect::<String>(), v))
            .collect();
        assert_eq!(
            revenues,
            vec![
                ("2024-12-31".to_string(), 1.1e11),
                ("2025-12-31".to_string(), 1.2e11),
                ("2025-12-31".to_string(), 1.3e11),
            ],
            "parquet rows must equal the Dolt row set (deleted row 'a' gone, new row 'd' present)"
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

    /// Bug #298 regression guard (originally RED): with the production Dolt block_trade PK, two real rows can
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

        // Before fix: merge `ROW_NUMBER() OVER (PARTITION BY symbol, trade_date, price)`
        // returns only 1 row → old=2 > merged=1 → Err. Before fix, this test was RED and expected
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

    /// Bug #298 regression guard (silent-loss variant; originally RED): when the old parquet holds exactly
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

        // Before fix: merge succeeds (old=1, merged=1) but the ROW_NUMBER
        // dedup on `(symbol, trade_date, price)` keeps only the new row.
        // Before fix (RED) expected both rows to be present; now GREEN.
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

    /// Bug #298 regression guard + prefer_new semantics (originally RED): one existing full-PK row is
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

        // Before fix: merge groups by narrow key, so it cannot express
        // "update the QFII row while preserving the institutional row and adding
        // the individual row"; it errors with old=2 parquet=1. Before fix, this was RED.
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
    /// Regression guard (bug #298), originally RED: the incremental merge partitions by the narrow key
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

        // Before fix: `ROW_NUMBER() OVER (PARTITION BY symbol, trade_date, price)`
        // returns one row for the three old/new rows, so old=2 > merged=1 and
        // `import_append_table` returns Err("row count mismatch ..."). Before fix, this was RED.
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
    /// Regression guard (bug #298), originally RED: on merge failure the fallback writes `new_data`,
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

        // Before fix (RED): fallback wrote only the since-filtered 2025
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

    // ------------------------------------------------------------------
    // #343 adversarial tests: --since incremental merge history safety
    // ------------------------------------------------------------------
    //
    // Contract under attack (plan fix-backfill-retry-import-history #343):
    // auto-heal 补进 Dolt 的早于 since 的缺失/过期行必须被合并检出并修复
    // （Dolt vs 旧 parquet 的 "date_col < since" 历史切片双向校验，发散 →
    // 全量 fallback）；merge 成功路径输出不得带 priority/rn 内部列；
    // index_daily 的 tradedate 重命名列同样适用；fallback 保留 pre_merge_backup。
    //
    // 当前实现（RED 依据）：import_append_table 不做任何历史校验——merge 只
    // 合并 "旧 parquet ∪ Dolt >= since 切片"，早于 since 的缺失/过期行永久
    // 留缺；merge SQL 外层 SELECT * 把 priority/rn 写进正式 parquet。

    /// #343 test isolation (review P2-1): `pre_merge_backup` filenames embed
    /// only pid+SEQ, and several tests share a stem (capital_main_flow /
    /// index_daily). The no-fallback tests (`second_run`,
    /// `index_daily_fast_path`) assert that NO new backup files appear during
    /// their window; the divergence tests for the same stem create them. Under
    /// parallel `cargo test` execution those windows overlap, making the
    /// assertion order-dependent, so the stem is serialized via these locks.
    static CAPITAL_MAIN_FLOW_STEM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static INDEX_DAILY_STEM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Sorted column names of a parquet file (DuckDB DESCRIBE).
    fn parquet_columns(path: &std::path::Path) -> Vec<String> {
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let mut cols: Vec<String> = duck
            .prepare(&format!(
                "SELECT column_name FROM (DESCRIBE SELECT * FROM read_parquet('{}'))",
                path.display()
            ))
            .expect("prepare describe")
            .query_map([], |row| row.get(0))
            .expect("describe")
            .collect::<Result<Vec<String>, _>>()
            .expect("decode describe rows");
        cols.sort();
        cols
    }

    /// Whether a `pre_merge_backup` staging file for `stem` was created by
    /// this process (unique_work_path embeds the pid).
    fn this_process_pre_merge_backup_exists(stem: &str) -> bool {
        let dir = std::env::temp_dir().join("compass_parquet_work");
        if !dir.exists() {
            return false;
        }
        let prefix = format!("{stem}.pre_merge_backup_{}", std::process::id());
        std::fs::read_dir(&dir)
            .map(|rd| {
                rd.collect::<Result<Vec<_>, _>>()
                    .expect("read dir entries")
                    .iter()
                    .any(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            })
            .unwrap_or(false)
    }

    /// Snapshot of this process's `pre_merge_backup` files for `stem`.
    ///
    /// Used for before/after comparison instead of a bare existence check:
    /// parallel tests (e.g. the corrupt-parquet fallback test) may create
    /// backup files for the same stem concurrently, so an `exists` assertion
    /// would be order-dependent.
    fn this_process_pre_merge_backup_files(stem: &str) -> Vec<String> {
        let dir = std::env::temp_dir().join("compass_parquet_work");
        if !dir.exists() {
            return Vec::new();
        }
        let prefix = format!("{stem}.pre_merge_backup_{}", std::process::id());
        std::fs::read_dir(&dir)
            .map(|rd| {
                rd.collect::<Result<Vec<_>, _>>()
                    .expect("read dir entries")
                    .iter()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.starts_with(&prefix))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// #343 attack (a-historically-extreme): Dolt gains rows older than
    /// `--since` that never existed in the old parquet (a full auto-heal
    /// backfill of missing history). The incremental merge must detect the
    /// divergence and repair the parquet to the full Dolt row set.
    ///
    /// RED today: the merge succeeds (row-count guard is a no-op: old 6 ==
    /// merged 6), the backfilled row `2026-01-04` is permanently missing,
    /// and the assertions below fail.
    #[test]
    fn incremental_merge_repairs_missing_history_before_since() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        // Initial state: one pre-since row + five >= since rows (the slice
        // must stay above the 500-byte tiny-data skip).
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8), \
             ('SH600519', '2026-01-06', 3.0e8), \
             ('SH600519', '2026-01-07', 4.0e8), \
             ('SH600519', '2026-01-08', 5.0e8), \
             ('SH600519', '2026-01-09', 6.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");

        // Auto-heal backfills a row older than --since into Dolt.
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-04', 7.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            7,
            "backfilled history row must be repaired into the parquet"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let inflow: f64 = duck
            .query_row(
                &format!(
                    "SELECT main_net_inflow FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2026-01-04'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("row 2026-01-04");
        assert!(
            (inflow - 7.0e8).abs() < 1.0,
            "backfilled value must match Dolt, got {inflow}"
        );
    }

    /// #343 attack (b-stale): a row older than `--since` is corrected in Dolt
    /// (same PK, different value). The row is absent from the >= since slice,
    /// so only a history check can catch it; the merge must fall back to a
    /// full export, not trust the stale old-parquet value.
    ///
    /// RED today: merge keeps the old value 1.0e8 (the row is not in the
    /// slice), the assertion fails.
    #[test]
    fn incremental_merge_repairs_stale_history_values() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8), \
             ('SH600519', '2026-01-06', 3.0e8), \
             ('SH600519', '2026-01-07', 4.0e8), \
             ('SH600519', '2026-01-08', 5.0e8), \
             ('SH600519', '2026-01-09', 6.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");

        // Dolt corrects the historical row (same PK, new value).
        dolt_sql(
            tmp.path(),
            "UPDATE capital_main_flow SET main_net_inflow = 9.0e8 \
             WHERE symbol = 'SH600519' AND trade_date = '2026-01-03'",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            6,
            "row count must stay consistent with Dolt"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let inflow: f64 = duck
            .query_row(
                &format!(
                    "SELECT main_net_inflow FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2026-01-03'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("row 2026-01-03");
        assert!(
            (inflow - 9.0e8).abs() < 1.0,
            "stale history value must be repaired to the Dolt value, got {inflow}"
        );
    }

    /// #343 attack (b-two-way-divergence): the old parquet holds a row older
    /// than `--since` that Dolt no longer has (orphan — e.g. Dolt delete or a
    /// row replaced by a different PK). The merge keeps it (old parquet wins
    /// via UNION), so the parquet diverges from Dolt forever.
    ///
    /// RED today: merge result is 6 rows while Dolt has 5; the assertion
    /// fails.
    #[test]
    fn incremental_merge_removes_orphaned_parquet_rows() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8), \
             ('SH600519', '2026-01-06', 3.0e8), \
             ('SH600519', '2026-01-07', 4.0e8), \
             ('SH600519', '2026-01-08', 5.0e8), \
             ('SH600519', '2026-01-09', 6.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");

        // Dolt drops the historical row (it never existed upstream, or was
        // replaced by a corrected PK).
        dolt_sql(
            tmp.path(),
            "DELETE FROM capital_main_flow WHERE symbol = 'SH600519' AND trade_date = '2026-01-03'",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            5,
            "orphaned parquet row must be removed (parquet == Dolt)"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let count: i64 = duck
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2026-01-03'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("count deleted row");
        assert_eq!(count, 0, "row deleted in Dolt must not survive in parquet");
    }

    /// #343 attack (c-boundary): a row dated exactly `since - 1 day` is
    /// backfilled into Dolt after the first export — the seam between the
    /// history check (`< since`) and the incremental slice (`>= since`).
    /// Note: this scenario is inherently divergent (the `since - 1` row is
    /// missing from the old parquet), so the repair path taken here is the
    /// FULL-EXPORT fallback, not a merge — the `== since` row is naturally
    /// unique in the full export. The merge-deduplication contract for a
    /// `== since` row present on BOTH sides is covered by the fast-path
    /// tests (`incremental_merge_fast_path_*` / `second_run_no_fallback`).
    ///
    /// RED today: the merge keeps only the 5 original rows; the
    /// `2026-01-04` (since-1 day) row is silently missing.
    #[test]
    fn incremental_merge_since_boundary_day_before_since() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        // H1 (< since) + E (== since) + four > since rows.
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 5.0e8), \
             ('SH600519', '2026-01-06', 6.0e8), \
             ('SH600519', '2026-01-07', 7.0e8), \
             ('SH600519', '2026-01-08', 8.0e8), \
             ('SH600519', '2026-01-09', 9.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");

        // H dated exactly since - 1 day, backfilled after the first export.
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-04', 4.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            7,
            "since/-1 day row must be repaired; == since row must not duplicate"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (day, expected) in [
            ("2026-01-03", 1.0e8),
            ("2026-01-04", 4.0e8),
            ("2026-01-05", 5.0e8),
            ("2026-01-06", 6.0e8),
            ("2026-01-07", 7.0e8),
            ("2026-01-08", 8.0e8),
            ("2026-01-09", 9.0e8),
        ] {
            let inflow: f64 = duck
                .query_row(
                    &format!(
                        "SELECT main_net_inflow FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '{day}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| panic!("row {day} missing"));
            assert!(
                (inflow - expected).abs() < 1.0,
                "row {day}: expected {expected}, got {inflow}"
            );
        }
    }

    /// #343 attack (d-internal-column-leak): on the fast path (history
    /// consistent), the successful DuckDB merge must NOT write `priority` /
    /// `rn` into the production parquet — the column set must be exactly the
    /// production schema.
    ///
    /// RED today: the merge SQL is `SELECT * ... FROM (SELECT *, ROW_NUMBER()
    /// ... AS rn FROM (SELECT *, 1 AS priority ...)) WHERE rn = 1` — the
    /// outer `SELECT *` carries priority and rn into the output parquet.
    #[test]
    fn incremental_merge_fast_path_no_internal_columns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8), \
             ('SH600519', '2026-01-06', 3.0e8), \
             ('SH600519', '2026-01-07', 4.0e8), \
             ('SH600519', '2026-01-08', 5.0e8), \
             ('SH600519', '2026-01-09', 6.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");

        // History is fully consistent (2026-01-03 present on both sides);
        // the merge on the fast path must succeed and must not leak columns.
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            6,
            "fast-path merge must keep the full row set"
        );
        let mut expected: Vec<String> = [
            "symbol",
            "trade_date",
            "main_net_inflow",
            "main_net_inflow_rate",
            "super_large_net",
            "large_net",
            "medium_net",
            "small_net",
            "update_date",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        expected.sort();
        assert_eq!(
            parquet_columns(&parquet),
            expected,
            "production parquet must not carry internal priority/rn columns"
        );
    }

    /// #343 attack (e-non-prefer-new-value-alignment): a non-prefer_new table
    /// (fin_indicators: published financials never change, old wins) must
    /// still converge to the Dolt state after a historical value correction.
    /// The history check must compare VALUES, not just key existence —
    /// key-only comparison would declare the row "present" and skip repair.
    ///
    /// RED today: the merge keeps the old value 1.0e11; assertion fails.
    #[test]
    fn incremental_merge_non_prefer_new_table_history_values_aligned() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), FIN_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES \
             ('SH600519', '2024-12-31', 1.0e11, '贵州茅台'), \
             ('SH600519', '2025-12-31', 2.0e11, '贵州茅台'), \
             ('SH600519', '2026-03-31', 3.0e11, '贵州茅台'), \
             ('SH600519', '2026-06-30', 4.0e11, '贵州茅台'), \
             ('SH600519', '2026-09-30', 5.0e11, '贵州茅台'), \
             ('SH600519', '2026-12-31', 6.0e11, '贵州茅台')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            None,
        )
        .expect("first full export");

        // Historical correction (same PK, new published value).
        dolt_sql(
            tmp.path(),
            "UPDATE fin_indicators SET revenue = 9.0e11 \
             WHERE symbol = 'SH600519' AND report_date = '2024-12-31'",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            Some("2025-01-01"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("fin_indicators.parquet");
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let revenue: f64 = duck
            .query_row(
                &format!(
                    "SELECT revenue FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND report_date = '2024-12-31'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("row 2024-12-31");
        assert!(
            (revenue - 9.0e11).abs() < 1.0,
            "corrected historical value must converge to Dolt, got {revenue}"
        );
        assert_eq!(
            read_parquet_row_count(&parquet),
            6,
            "row count must stay consistent with Dolt"
        );
    }

    /// #343 attack (g-renamed-date-column): index_daily renames Dolt
    /// `trade_date` → parquet `tradedate`; the history check must apply the
    /// parquet-side name, otherwise it cannot compare the old parquet
    /// historical slice at all (Binder error on `trade_date`).
    ///
    /// RED today: no check exists; the backfilled `2026-01-01` row (older
    /// than since 2026-01-05) is missing after the merge.
    #[test]
    fn incremental_merge_index_daily_tradedate_detects_history_divergence() {
        let _stem = INDEX_DAILY_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), INDEX_DAILY_PRODUCTION_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO index_daily \
             (symbol, trade_date, index_type, open, close, high, low, volume, amount) VALUES \
             ('SH000001', '2026-01-02', 'index', 2900.0, 2910.0, 2920.0, 2890.0, 1.0e8, 2.0e8), \
             ('SH000001', '2026-01-05', 'index', 3000.0, 3010.0, 3020.0, 2990.0, 1.1e8, 2.1e8), \
             ('SH000001', '2026-01-06', 'index', 3100.0, 3110.0, 3120.0, 3090.0, 1.2e8, 2.2e8), \
             ('SH000001', '2026-01-07', 'index', 3200.0, 3210.0, 3220.0, 3190.0, 1.3e8, 2.3e8), \
             ('SH000001', '2026-01-08', 'index', 3300.0, 3310.0, 3320.0, 3290.0, 1.4e8, 2.4e8), \
             ('SH000001', '2026-01-09', 'index', 3400.0, 3410.0, 3420.0, 3390.0, 1.5e8, 2.5e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexDaily,
            false,
            None,
        )
        .expect("first full export");

        // Backfilled bar older than --since (missing history).
        dolt_sql(
            tmp.path(),
            "INSERT INTO index_daily \
             (symbol, trade_date, index_type, open, close, high, low, volume, amount) VALUES \
             ('SH000001', '2026-01-01', 'index', 2800.0, 2810.0, 2820.0, 2790.0, 1.2e8, 2.2e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexDaily,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        let parquet = tmp.path().join("index_daily.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            7,
            "backfilled index bar must be repaired into the parquet \
             (full Dolt row set = 6 original + 1 backfilled)"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let close: f64 = duck
            .query_row(
                &format!(
                    "SELECT close FROM read_parquet('{}') \
                     WHERE symbol = 'SH000001' AND tradedate = '2026-01-01'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("bar 2026-01-01");
        assert!(
            (close - 2810.0).abs() < 1.0,
            "backfilled bar value must match Dolt, got {close}"
        );
    }

    /// #343 review P2-1 (positive lock): `parquet_date_col` must map the
    /// export rename (`trade_date` → `tradedate`) so a CONSISTENT history
    /// takes the fast-path merge. A wrong mapping (e.g. `None` → filtering
    /// the parquet side by `trade_date`) raises a Binder error → Ok(false) →
    /// full-export fallback, which the divergence test above cannot
    /// distinguish from a correct fast path. Lock: no new pre_merge_backup
    /// (no fallback), row set intact, no internal columns.
    #[test]
    fn incremental_merge_index_daily_fast_path_no_fallback() {
        let _stem = INDEX_DAILY_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), INDEX_DAILY_PRODUCTION_SCHEMA);

        // One pre-since bar + five >= since bars (the >= since slice must
        // stay above the 500-byte tiny-data skip).
        dolt_sql(
            tmp.path(),
            "INSERT INTO index_daily \
             (symbol, trade_date, index_type, open, close, high, low, volume, amount) VALUES \
             ('SH000001', '2026-01-02', 'index', 2900.0, 2910.0, 2920.0, 2890.0, 1.0e8, 2.0e8), \
             ('SH000001', '2026-01-05', 'index', 3000.0, 3010.0, 3020.0, 2990.0, 1.1e8, 2.1e8), \
             ('SH000001', '2026-01-06', 'index', 3100.0, 3110.0, 3120.0, 3090.0, 1.2e8, 2.2e8), \
             ('SH000001', '2026-01-07', 'index', 3200.0, 3210.0, 3220.0, 3190.0, 1.3e8, 2.3e8), \
             ('SH000001', '2026-01-08', 'index', 3300.0, 3310.0, 3320.0, 3290.0, 1.4e8, 2.4e8), \
             ('SH000001', '2026-01-09', 'index', 3400.0, 3410.0, 3420.0, 3390.0, 1.5e8, 2.5e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexDaily,
            false,
            None,
        )
        .expect("first full export");

        // Consistent history (no Dolt change before since): snapshot the
        // backup files, then run the incremental import.
        let backups_before = this_process_pre_merge_backup_files("index_daily");
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::IndexDaily,
            false,
            Some("2026-01-05"),
        )
        .expect("fast-path incremental merge");

        let parquet = tmp.path().join("index_daily.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            6,
            "consistent history must keep the full row set"
        );
        assert!(
            !parquet_columns(&parquet).contains(&"priority".to_string())
                && !parquet_columns(&parquet).contains(&"rn".to_string()),
            "fast-path merge output must not carry internal columns"
        );
        let backups_after = this_process_pre_merge_backup_files("index_daily");
        let new_backups: Vec<&String> = backups_after
            .iter()
            .filter(|f| !backups_before.contains(f))
            .collect();
        assert!(
            new_backups.is_empty(),
            "consistent history must take the fast path — no pre_merge_backup, got {new_backups:?}"
        );
    }

    /// #343 attack (f-scale-shape): a large old parquet (40 rows) with a
    /// single missing historical row must still trigger the repair path.
    /// Row-count guards cannot detect this (old 40 == merged 40), and the
    /// tiny-data skip must not swallow it (the >= since slice is non-empty).
    ///
    /// RED today: the merge keeps 40 rows; the backfilled `2025-11-30` row
    /// is missing, assertion fails.
    #[test]
    fn incremental_merge_large_history_single_missing_row() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        // 40 rows spanning both sides of since=2026-01-05:
        // 2025-12-02..2026-01-10 (n = 1..40 days from 2025-12-01).
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) \
             WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 40) \
             SELECT 'SH600519', DATE_ADD('2025-12-01', INTERVAL n DAY), n * 1e6 FROM s",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");
        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(read_parquet_row_count(&parquet), 40);

        // One single missing historical row (before since).
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2025-11-30', 0.5e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        assert_eq!(
            read_parquet_row_count(&parquet),
            41,
            "single missing historical row must be repaired even with a large parquet"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        let inflow: f64 = duck
            .query_row(
                &format!(
                    "SELECT main_net_inflow FROM read_parquet('{}') \
                     WHERE symbol = 'SH600519' AND trade_date = '2025-11-30'",
                    parquet.display()
                ),
                [],
                |row| row.get(0),
            )
            .expect("row 2025-11-30");
        assert!(
            (inflow - 0.5e8).abs() < 1.0,
            "backfilled row value must match Dolt, got {inflow}"
        );
    }

    /// #343 attack (g-repeated-merge): after a fast-path merge the parquet
    /// must be re-mergeable. Today the first merge leaks priority/rn into the
    /// production parquet; the next incremental run then hits duplicate
    /// columns in `SELECT *, 1 AS priority` and silently falls back
    /// (pre_merge_backup appears). The planned fix (EXCLUDE) must make the
    /// second run a clean merge with no backup.
    #[test]
    fn incremental_merge_second_run_no_fallback_no_leak() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8), \
             ('SH600519', '2026-01-06', 3.0e8), \
             ('SH600519', '2026-01-07', 4.0e8), \
             ('SH600519', '2026-01-08', 5.0e8), \
             ('SH600519', '2026-01-09', 6.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            None,
        )
        .expect("first full export");

        // First incremental: fast-path merge (today: leaks priority/rn).
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("first incremental merge");

        // A new row arrives, second incremental merge. Snapshot backup files
        // before and after: the second run must be a clean merge, i.e. it
        // must not create a new pre_merge_backup (a fallback would prove the
        // leaked internal columns of the first merge poisoned the parquet).
        let backups_before = this_process_pre_merge_backup_files("capital_main_flow");
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-10', 7.0e8)",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::MainFlow,
            false,
            Some("2026-01-05"),
        )
        .expect("second incremental merge");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            7,
            "second merge must keep the full row set"
        );
        assert!(
            !parquet_columns(&parquet).contains(&"priority".to_string())
                && !parquet_columns(&parquet).contains(&"rn".to_string()),
            "second merge output must not carry internal columns"
        );
        let backups_after = this_process_pre_merge_backup_files("capital_main_flow");
        let new_backups: Vec<&String> = backups_after
            .iter()
            .filter(|f| !backups_before.contains(f))
            .collect();
        assert!(
            new_backups.is_empty(),
            "second merge must be a clean merge — no new pre_merge_backup, got {new_backups:?}"
        );
    }

    /// #343 attack (g-corrupt-recovery, anti-regression): an unreadable old
    /// parquet must fall back to a full export (never silently), keep the
    /// pre-merge backup, and end up aligned with Dolt. This mirrors the
    /// existing corrupt-parquet tests but locks the *backup* + divergence
    /// combo the planned `incremental_history_matches` (read error →
    /// Ok(false)) must preserve. GREEN today (merge-failure fallback already
    /// implemented); the test guards the rework.
    ///
    /// Uses the `fin_indicators` stem deliberately (not `capital_main_flow`)
    /// so its backup files cannot race the no-fallback snapshot assertion in
    /// `incremental_merge_second_run_no_fallback_no_leak` under parallel
    /// test execution.
    #[test]
    fn incremental_merge_corrupt_parquet_falls_back_with_backup() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), FIN_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES \
             ('SH600519', '2024-12-31', 1.0e11, '贵州茅台'), \
             ('SH600519', '2025-03-31', 2.0e11, '贵州茅台'), \
             ('SH600519', '2025-06-30', 3.0e11, '贵州茅台'), \
             ('SH600519', '2025-09-30', 4.0e11, '贵州茅台'), \
             ('SH600519', '2025-12-31', 5.0e11, '贵州茅台'), \
             ('SH600519', '2026-03-31', 6.0e11, '贵州茅台')",
        );
        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            None,
        )
        .expect("first full export");

        // New row in Dolt + unreadable old parquet.
        dolt_sql(
            tmp.path(),
            "INSERT INTO fin_indicators (symbol, report_date, revenue, name) VALUES \
             ('SH600519', '2026-06-30', 7.0e11, '贵州茅台')",
        );
        let parquet = tmp.path().join("fin_indicators.parquet");
        std::fs::write(&parquet, b"corrupted parquet").expect("corrupt parquet");

        run(
            tmp.path().to_path_buf(),
            tmp.path().to_path_buf(),
            CompassTable::FinIndicators,
            false,
            Some("2025-01-01"),
        )
        .expect("corrupt parquet must recover, not error");

        assert_eq!(
            read_parquet_row_count(&parquet),
            7,
            "recovered parquet must be aligned with Dolt"
        );
        assert!(
            this_process_pre_merge_backup_exists("fin_indicators"),
            "fallback must preserve the pre-merge parquet for diagnosis"
        );
    }

    // ------------------------------------------------------------------
    // #343 requirement acceptance tests
    // ------------------------------------------------------------------
    //
    // Acceptance contract (plan fix-backfill-retry-import-history #343 +
    // issue #343): a `--since` incremental import must converge the parquet
    // to the full Dolt row set. When auto-heal backfills rows dated before
    // the --since anchor, those rows are in NEITHER the `>= since` Dolt slice
    // NOR the old parquet — only a pre-merge history check (Dolt < since vs
    // old parquet < since) can detect and repair them.
    //
    // Gap under test that the adversarial set does NOT cover:
    // import_append_table:423 `new_data.len() < 500` skips the whole import
    // when the Dolt `>= since` slice is tiny. Measured with the
    // MAIN_FLOW_SCHEMA export (`SELECT *`, `dolt sql -r parquet`): a 0-row
    // slice is 311 bytes (< 500 → skip), a 1-row slice is 1568 bytes
    // (> 500 → merge path). So the skip only fires on an EMPTY slice — the
    // auto-heal scenario where nothing new was collected after the anchor.
    // The fix must run the history check before (or instead of) the skip.

    #[test]
    fn incremental_merge_empty_after_since_slice_still_repairs_auto_healed_history() {
        let _stem = CAPITAL_MAIN_FLOW_STEM_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        // Initial state: three rows ALL OLDER than the future `--since`
        // anchor (2026-01-05), so the later `>= since` slice is empty.
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-02', 1.0e8), \
             ('SH600519', '2026-01-03', 2.0e8), \
             ('SH600519', '2026-01-04', 3.0e8)",
        );
        // First export (no --since): effective_since = None, full write.
        import_append_table(
            AppendTableSpec {
                table_name: "capital_main_flow",
                date_col: "trade_date",
                parquet_date_col: None,
                partition_cols: "symbol, trade_date",
                prefer_new: true,
                dolt_order_cols: None,
                select_cols: None,
            },
            tmp.path(),
            tmp.path(),
            false,
            None,
        )
        .expect("first full export");
        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert!(parquet.exists(), "initial parquet must exist");
        assert_eq!(read_parquet_row_count(&parquet), 3);

        // Auto-heal backfills MORE history older than --since into Dolt
        // (issue #343: these rows are before the anchor, so they are in
        // neither the `>= since` slice nor the old parquet).
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2025-12-31', 0.5e8), \
             ('SH600519', '2026-01-01', 0.7e8)",
        );

        // Incremental import with --since: the Dolt slice `>= 2026-01-05` is
        // empty (311 bytes < 500). RED today: the tiny-data skip at
        // import_append_table:423 returns Ok without touching the parquet,
        // so the backfilled pre-since rows stay missing forever; only a
        // history check BEFORE the skip can repair them.
        import_append_table(
            AppendTableSpec {
                table_name: "capital_main_flow",
                date_col: "trade_date",
                parquet_date_col: None,
                partition_cols: "symbol, trade_date",
                prefer_new: true,
                dolt_order_cols: None,
                select_cols: None,
            },
            tmp.path(),
            tmp.path(),
            false,
            Some("2026-01-05"),
        )
        .expect("incremental import");

        // Contract: final parquet row set == full Dolt row set.
        assert_eq!(
            read_parquet_row_count(&parquet),
            5,
            "backfilled pre-since rows must be repaired into the parquet (parquet == Dolt)"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (day, expected) in [("2025-12-31", 0.5e8), ("2026-01-01", 0.7e8)] {
            let inflow: f64 = duck
                .query_row(
                    &format!(
                        "SELECT main_net_inflow FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '{day}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| panic!("backfilled row {day} missing"));
            assert!(
                (inflow - expected).abs() < 1.0,
                "row {day}: expected Dolt value {expected}, got {inflow}"
            );
        }
    }

    /// #343 requirement acceptance (anti-regression — GREEN today): with a
    /// consistent pre-since history, a `--since` incremental merge must fold
    /// in the new `>= since` rows, keep every pre-since old row, and carry
    /// values identical to Dolt. Content-only assertions: the priority/rn
    /// column-leak contract belongs to the adversarial test
    /// (`incremental_merge_fast_path_no_internal_columns`), so it is not
    /// duplicated here.
    #[test]
    fn incremental_merge_fast_path_keeps_new_rows_and_matches_dolt_values() {
        let tmp = tempfile::tempdir().expect("tempdir");
        setup_dolt(tmp.path());
        dolt_sql(tmp.path(), MAIN_FLOW_SCHEMA);

        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-03', 1.0e8), \
             ('SH600519', '2026-01-05', 2.0e8), \
             ('SH600519', '2026-01-06', 3.0e8), \
             ('SH600519', '2026-01-07', 4.0e8), \
             ('SH600519', '2026-01-08', 5.0e8), \
             ('SH600519', '2026-01-09', 6.0e8)",
        );
        import_append_table(
            AppendTableSpec {
                table_name: "capital_main_flow",
                date_col: "trade_date",
                parquet_date_col: None,
                partition_cols: "symbol, trade_date",
                prefer_new: true,
                dolt_order_cols: None,
                select_cols: None,
            },
            tmp.path(),
            tmp.path(),
            false,
            None,
        )
        .expect("first full export");

        // A new row after the --since anchor arrives (normal daily
        // increment, not auto-heal): pre-since history stays consistent.
        dolt_sql(
            tmp.path(),
            "INSERT INTO capital_main_flow (symbol, trade_date, main_net_inflow) VALUES \
             ('SH600519', '2026-01-10', 7.0e8)",
        );
        import_append_table(
            AppendTableSpec {
                table_name: "capital_main_flow",
                date_col: "trade_date",
                parquet_date_col: None,
                partition_cols: "symbol, trade_date",
                prefer_new: true,
                dolt_order_cols: None,
                select_cols: None,
            },
            tmp.path(),
            tmp.path(),
            false,
            Some("2026-01-05"),
        )
        .expect("incremental merge");

        let parquet = tmp.path().join("capital_main_flow.parquet");
        assert_eq!(
            read_parquet_row_count(&parquet),
            7,
            "fast-path merge must keep the old rows AND add the new >= since row"
        );
        let duck = duckdb::Connection::open_in_memory().expect("duckdb");
        for (day, expected) in [
            ("2026-01-03", 1.0e8),
            ("2026-01-05", 2.0e8),
            ("2026-01-06", 3.0e8),
            ("2026-01-07", 4.0e8),
            ("2026-01-08", 5.0e8),
            ("2026-01-09", 6.0e8),
            ("2026-01-10", 7.0e8),
        ] {
            let inflow: f64 = duck
                .query_row(
                    &format!(
                        "SELECT main_net_inflow FROM read_parquet('{}') \
                         WHERE symbol = 'SH600519' AND trade_date = '{day}'",
                        parquet.display()
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| panic!("row {day} missing"));
            assert!(
                (inflow - expected).abs() < 1.0,
                "row {day}: expected Dolt value {expected}, got {inflow}"
            );
        }
    }
}
