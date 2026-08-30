//! Orchestration CLI for the Rust collector migration.
//!
//! Mirrors `collectors/main.py`: fetch/import dispatch, `sync` (full
//! do_sync order), auto-heal/backfill, progress display, and
//! `sync-investment` Dolt repo sync.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use chrono::{Duration, Local};

use crate::config::{csv_dir, dolt_dir, dolt_exists, investment_data_dir};
use crate::dolt::{dolt_sql, dolt_sql_csv, set_last_report_date};
use crate::error::{CollectError, Result};
use crate::timing::{TimingEvent, TimingWriter};
use crate::{
    balance_sheet, block_trade, cash_flow, dragon, fin_indicators, income, index_daily,
    institution_survey, main_flow, progress, stock_basic_official,
};

/// Default financial report periods (comma-separated).
pub const DEFAULT_PERIODS: &str = "Q1,Q2,Q3,FY";
/// Default page size for paginated EastMoney fetches.
pub const DEFAULT_PAGE_SIZE: usize = 100;
/// Daily auto-heal tables: (dolt table, date column) pairs.
pub const DAILY_AUTO_HEAL_TABLES: &[(&str, &str)] = &[
    ("capital_main_flow", "trade_date"),
    ("index_daily", "trade_date"),
    ("dragon_list", "trade_date"),
    ("block_trade", "trade_date"),
];

fn last_csv_cell(output: &str) -> String {
    output
        .trim()
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn require_nonzero(rows: u64, label: &str) -> Result<()> {
    if rows == 0 {
        Err(CollectError::InvalidInput(format!(
            "sync failed: {label} import returned 0 rows"
        )))
    } else {
        Ok(())
    }
}

/// Decision layer for 0-row daily imports (issue #338): a no-op only when the
/// window contains no trading day, or when the calendar itself is unavailable
/// / inverted; any other calendar error propagates (a broken data source must
/// not be silently papered over).
fn daily_zero_row_decision(
    calendar_days: std::result::Result<Vec<String>, CollectError>,
) -> Result<()> {
    match calendar_days {
        Ok(days) if days.is_empty() => {
            eprintln!("[sync] 0-row import but no trading days in window — no-op");
            Ok(())
        }
        Ok(_) => require_nonzero(0, "daily"),
        Err(CollectError::EmptyCalendar) => {
            eprintln!("[sync] trade calendar unavailable; 0-row import treated as no-op");
            Ok(())
        }
        Err(CollectError::InvertedRange { .. }) => {
            eprintln!("[sync] inverted calendar window; 0-row import treated as no-op");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn next_day(date: &str) -> Result<String> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
        CollectError::InvalidDate {
            label: "last_report_date".into(),
            value: date.into(),
        }
    })?;
    Ok((d + Duration::days(1)).format("%Y-%m-%d").to_string())
}

/// Daily-table row guard: `rows > 0` succeeds without touching the calendar;
/// zero rows consult the trade calendar (anchor+1 .. today) — the calendar
/// result is handed to `daily_zero_row_decision`.
async fn require_daily_rows(table: &str, rows: u64) -> Result<()> {
    if rows > 0 {
        return Ok(());
    }
    let end = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let start = match crate::dolt::last_report_date(table).await? {
        Some(since) => next_day(&since)?,
        None => (Local::now().date_naive() - Duration::days(90))
            .format("%Y-%m-%d")
            .to_string(),
    };
    let calendar = crate::calendar::trade_calendar(&start, &end).await;
    daily_zero_row_decision(calendar)
}

/// Run a sync phase, record its wall time to the optional timing writer, and
/// return the original result. Timing failures are warnings only.
macro_rules! timed {
    ($writer:expr, $source:expr, $phase:expr, $body:expr) => {{
        let __start = Instant::now();
        let __result = $body;
        let __status = if __result.is_ok() {
            "success"
        } else {
            "failed"
        };
        if let Some(__w) = $writer.as_ref() {
            let __event = TimingEvent::collector($source, $phase, __status, __start.elapsed());
            if let Err(__e) = __w.record(&__event) {
                eprintln!(
                    "[sync] warning: timing write failed ({} {}): {__e}",
                    $source, $phase
                );
            }
        }
        __result
    }};
}

/// Fetch one collector into a CSV (dispatch equivalent of `main.py`).
pub async fn fetch(
    target: &str,
    years: Option<&[i32]>,
    incremental: bool,
) -> Result<std::path::PathBuf> {
    let periods = DEFAULT_PERIODS;
    let page_size = DEFAULT_PAGE_SIZE;
    match target {
        "stock_basic" => stock_basic_official::run(None, None).await,
        "fin_indicators" => fin_indicators::run(years, periods, page_size, incremental).await,
        "balance_sheet" => balance_sheet::run(years, periods, page_size, incremental).await,
        "income" => income::run(years, periods, page_size, incremental).await,
        "cash_flow" => cash_flow::run(years, periods, page_size, incremental).await,
        "dragon" => dragon::run(None, None, page_size).await,
        "block_trade" => block_trade::run(None, None, None, page_size).await,
        "institution_survey" => institution_survey::run(None, page_size).await,
        "main_flow" => main_flow::run().await,
        "index_daily" => index_daily::run().await,
        other => Err(CollectError::InvalidInput(format!(
            "unknown fetch target: {other}"
        ))),
    }
}

/// Import the CSV for one collector into Dolt.
pub async fn import_target(target: &str) -> Result<()> {
    match target {
        "stock_basic" => {
            let rows = stock_basic_official::import_to_dolt(None).await?;
            require_nonzero(rows, "stock_basic")
        }
        "fin_indicators" => {
            let rows = fin_indicators::import_to_dolt(None).await?;
            require_nonzero(rows, "fin_indicators")
        }
        "balance_sheet" => {
            let rows = balance_sheet::import_to_dolt(None).await?;
            require_nonzero(rows, "fin_balance_sheet")
        }
        "income" => {
            let rows = income::import_to_dolt(None).await?;
            require_nonzero(rows, "fin_income")
        }
        "cash_flow" => {
            let rows = cash_flow::import_to_dolt(None).await?;
            require_nonzero(rows, "fin_cash_flow")
        }
        "dragon" => {
            let rows = dragon::import_to_dolt(None).await?;
            require_nonzero(rows, "dragon_list")
        }
        "block_trade" => {
            let rows = block_trade::import_to_dolt(None).await?;
            require_nonzero(rows, "block_trade")
        }
        "institution_survey" => {
            let rows = institution_survey::import_to_dolt(None).await?;
            require_nonzero(rows, "institution_survey")
        }
        "main_flow" => {
            let rows = main_flow::import_to_dolt(None).await?;
            require_nonzero(rows, "capital_main_flow")
        }
        "index_daily" => {
            let basic = index_daily::import_index_basic(None).await?;
            require_nonzero(basic, "index_basic")?;
            let daily = index_daily::import_to_dolt(None).await?;
            require_nonzero(daily, "index_daily")
        }
        other => Err(CollectError::InvalidInput(format!(
            "unknown import target: {other}"
        ))),
    }
}

fn print_progress(data: &progress::ProgressState) {
    let name = data.name.as_str();
    let status = data.status.as_str();
    let percent = data
        .percent
        .map(|p| format!("{p:.1}%"))
        .unwrap_or_else(|| "n/a".to_string());
    println!("[{name}] {status} {percent} — {}", data.message);
    match data.total_items {
        Some(total) => println!(
            "  completed: {}/{}  rows: {}",
            data.completed_items, total, data.fetched_rows
        ),
        None => println!("  rows: {}", data.fetched_rows),
    }
    if let Some(item) = &data.current_item {
        println!("  current: {item}");
    }
    if let Some(err) = &data.error {
        println!("  error: {err}");
    }
}

/// Show live progress for one collector or all collectors.
pub async fn progress(target: Option<&str>, as_json: bool) -> Result<()> {
    if let Some(name) = target {
        let Some(data) = progress::read_progress(name)? else {
            eprintln!("No progress file for {name} (fetch not started?)");
            return Err(CollectError::InvalidInput(format!(
                "no progress file for {name}"
            )));
        };
        if as_json {
            println!("{}", serde_json::to_string_pretty(&data)?);
        } else {
            print_progress(&data);
        }
        return Ok(());
    }

    let dir = csv_dir()?;
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.ends_with(".progress.json") {
            names.push(file_name.trim_end_matches(".progress.json").to_string());
        }
    }
    names.sort();
    if names.is_empty() {
        if as_json {
            println!("[]");
        } else {
            eprintln!("No fetch progress files found.");
        }
        return Ok(());
    }

    let mut entries: Vec<progress::ProgressState> = Vec::new();
    for name in names {
        if let Some(data) = progress::read_progress(&name)? {
            entries.push(data);
        }
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for data in &entries {
            print_progress(data);
        }
    }
    Ok(())
}

/// Backfill one or all auto-heal daily tables for explicit ranges.
pub async fn backfill(ranges: &[(String, (String, String))]) -> Result<()> {
    for (table, (start, end)) in ranges {
        match table.as_str() {
            "capital_main_flow" => {
                let path = main_flow::backfill(start, end, None).await?;
                let rows = main_flow::import_to_dolt(Some(&path)).await?;
                require_nonzero(rows, "capital_main_flow")?;
            }
            "index_daily" => {
                let path = index_daily::backfill(start, end).await?;
                if path.exists() {
                    let rows = index_daily::import_to_dolt(Some(&path)).await?;
                    require_nonzero(rows, "index_daily")?;
                } else {
                    eprintln!(
                        "[sync] Auto-heal: index_daily: no rows in backfill range, skipping import"
                    );
                }
            }
            "dragon_list" => {
                let path = dragon::run(Some(start), Some(end), DEFAULT_PAGE_SIZE).await?;
                if path.exists() {
                    let rows = dragon::import_to_dolt(Some(&path)).await?;
                    require_nonzero(rows, "dragon_list")?;
                } else {
                    eprintln!(
                        "[sync] Auto-heal: dragon_list: no rows in backfill range, skipping import"
                    );
                }
            }
            "block_trade" => {
                let path =
                    block_trade::run(None, Some(start), Some(end), DEFAULT_PAGE_SIZE).await?;
                if path.exists() {
                    let rows = block_trade::import_to_dolt(Some(&path)).await?;
                    require_nonzero(rows, "block_trade")?;
                } else {
                    eprintln!(
                        "[sync] Auto-heal: block_trade: no rows in backfill range, skipping import"
                    );
                }
            }
            other => {
                return Err(CollectError::InvalidInput(format!(
                    "unknown auto-heal table: {other}"
                )));
            }
        }
    }
    Ok(())
}

fn date_str(delta: i64) -> String {
    (Local::now().date_naive() - Duration::days(delta))
        .format("%Y-%m-%d")
        .to_string()
}

async fn auto_heal_table_range(table: &str, col: &str) -> Result<(String, String)> {
    let end = date_str(1);
    let fallback_start = date_str(90);
    if !dolt_exists(&dolt_dir()) {
        return Ok((fallback_start, end));
    }

    let exists_out = dolt_sql_csv(&format!(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{table}'"
    ))
    .await?;
    let exists = last_csv_cell(&exists_out) == "1";
    if !exists {
        return Ok((fallback_start, end));
    }

    let out = dolt_sql_csv(&format!("SELECT MIN({col}) FROM {table}")).await?;
    let value = last_csv_cell(&out);
    let start = if value.is_empty() || value == "NULL" {
        fallback_start
    } else {
        value
    };
    Ok((start, end))
}

async fn auto_heal() -> Result<()> {
    eprintln!("[sync] Auto-heal: checking missing trading dates...");
    let mut ranges: Vec<(String, (String, String))> = Vec::new();
    let mut total_missing = 0usize;
    for (table, col) in DAILY_AUTO_HEAL_TABLES {
        let (start, end) = auto_heal_table_range(table, col).await?;
        let missing = crate::calendar::missing_dates(table, col, &start, &end).await?;
        if missing.is_empty() {
            continue;
        }
        let min = missing
            .iter()
            .min()
            .cloned()
            .unwrap_or_else(|| start.clone());
        let max = missing.iter().max().cloned().unwrap_or_else(|| end.clone());
        eprintln!("[sync] Auto-heal: {table} missing {} dates", missing.len());
        ranges.push((table.to_string(), (min, max)));
        total_missing += missing.len();
    }
    if ranges.is_empty() {
        return Ok(());
    }
    eprintln!("[sync] Auto-heal: backfilling {total_missing} dates per table");
    backfill(&ranges).await?;
    for (table, (_start, end)) in &ranges {
        set_last_report_date(table, end).await?;
    }
    Ok(())
}

async fn run_dolt_investment(args: &[&str]) -> Result<()> {
    let dir = investment_data_dir();
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 investment path".into()))?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        tokio::process::Command::new("dolt")
            .kill_on_drop(true)
            .args(["--data-dir", dir_str])
            .args(args)
            .output(),
    )
    .await
    .map_err(|_| {
        CollectError::InvalidInput(format!("dolt {} timed out after 300s", args.join(" ")))
    })??;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(CollectError::Dolt {
            stderr: format!("dolt {} failed: {stderr}", args.join(" ")),
        })
    }
}

/// Sync `investment_data` from chenditc upstream and push to the skwy fork.
pub async fn sync_investment(restart: bool) -> Result<()> {
    let invest_dir = investment_data_dir();
    if !invest_dir.join(".dolt").exists() {
        eprintln!("[sync-investment] ERROR: investment_data not found");
        return Ok(());
    }

    if restart {
        eprintln!("[sync-investment] Stopping Dolt SQL server...");
        let _ = Command::new("pkill")
            .args(["-f", "dolt sql-server.*investment_data"])
            .status();
    }

    eprintln!("[sync-investment] Fetching from origin...");
    run_dolt_investment(&["fetch", "origin"]).await?;
    eprintln!("[sync-investment] Merging origin/master...");
    run_dolt_investment(&["checkout", "master"]).await?;
    run_dolt_investment(&["pull", "origin", "master"]).await?;
    eprintln!("[sync-investment] Pushing to skwy...");
    run_dolt_investment(&["push", "skwy", "master"]).await?;

    if restart {
        let server_script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("scripts/start-dolt-server.sh");
        if server_script.exists() {
            eprintln!("[sync-investment] Restarting server...");
            let _ = Command::new("nohup")
                .arg("bash")
                .arg(&server_script)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
    eprintln!("[sync-investment] Done.");
    Ok(())
}

/// Full sync: auto-heal, fetch every collector, import every CSV, update
/// `data_updates` row counts. Mirrors `main.py::do_sync`.
pub async fn sync(_restart: bool) -> Result<()> {
    let timing = TimingWriter::from_env();

    let auto_heal_enabled = std::env::var("COMPASS_AUTO_HEAL")
        .map(|v| v != "0")
        .unwrap_or(true);
    if !auto_heal_enabled {
        eprintln!("[sync] Auto-heal disabled (COMPASS_AUTO_HEAL=0)");
    } else {
        auto_heal().await?;
    }

    eprintln!("[sync] Fetching stock_basic...");
    let _ = timed!(
        &timing,
        "stock_basic",
        "fetch",
        stock_basic_official::run(None, None).await
    )?;
    let rows = timed!(
        &timing,
        "stock_basic",
        "import",
        stock_basic_official::import_to_dolt(None).await
    )?;
    require_nonzero(rows, "stock_basic")?;

    eprintln!("\n[sync] Fetching fin_indicators (incremental)...");
    let path = timed!(
        &timing,
        "fin_indicators",
        "fetch",
        fin_indicators::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await
    )?;
    eprintln!("  -> {}", path.display());
    let rows = timed!(
        &timing,
        "fin_indicators",
        "import",
        fin_indicators::import_to_dolt(None).await
    )?;
    require_nonzero(rows, "fin_indicators")?;

    eprintln!("\n[sync] Fetching balance_sheet (incremental)...");
    let path = timed!(
        &timing,
        "balance_sheet",
        "fetch",
        balance_sheet::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await
    )?;
    eprintln!("  -> {}", path.display());
    let rows = timed!(
        &timing,
        "balance_sheet",
        "import",
        balance_sheet::import_to_dolt(None).await
    )?;
    require_nonzero(rows, "fin_balance_sheet")?;

    eprintln!("\n[sync] Fetching income (incremental)...");
    let path = timed!(
        &timing,
        "income",
        "fetch",
        income::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await
    )?;
    eprintln!("  -> {}", path.display());
    let rows = timed!(
        &timing,
        "income",
        "import",
        income::import_to_dolt(None).await
    )?;
    require_nonzero(rows, "fin_income")?;

    eprintln!("\n[sync] Fetching cash_flow (incremental)...");
    let path = timed!(
        &timing,
        "cash_flow",
        "fetch",
        cash_flow::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await
    )?;
    eprintln!("  -> {}", path.display());
    let rows = timed!(
        &timing,
        "cash_flow",
        "import",
        cash_flow::import_to_dolt(None).await
    )?;
    require_nonzero(rows, "fin_cash_flow")?;

    eprintln!("\n[sync] Fetching dragon_list...");
    let _ = timed!(
        &timing,
        "dragon",
        "fetch",
        dragon::run(None, None, DEFAULT_PAGE_SIZE).await
    )?;
    let rows = timed!(
        &timing,
        "dragon",
        "import",
        dragon::import_to_dolt(None).await
    )?;
    require_daily_rows("dragon_list", rows).await?;

    eprintln!("\n[sync] Fetching block_trade...");
    let _ = timed!(
        &timing,
        "block_trade",
        "fetch",
        block_trade::run(None, None, None, DEFAULT_PAGE_SIZE).await
    )?;
    let rows = timed!(
        &timing,
        "block_trade",
        "import",
        block_trade::import_to_dolt(None).await
    )?;
    require_daily_rows("block_trade", rows).await?;

    eprintln!("\n[sync] Fetching institution_survey...");
    let _ = timed!(
        &timing,
        "institution_survey",
        "fetch",
        institution_survey::run(None, DEFAULT_PAGE_SIZE).await
    )?;
    let rows = timed!(
        &timing,
        "institution_survey",
        "import",
        institution_survey::import_to_dolt(None).await
    )?;
    require_nonzero(rows, "institution_survey")?;

    eprintln!("\n[sync] Fetching main_flow...");
    let _ = timed!(&timing, "main_flow", "fetch", main_flow::run().await)?;
    let rows = timed!(
        &timing,
        "main_flow",
        "import",
        main_flow::import_to_dolt(None).await
    )?;
    require_daily_rows("capital_main_flow", rows).await?;

    eprintln!("\n[sync] Fetching index_daily...");
    let _ = timed!(&timing, "index_daily", "fetch", index_daily::run().await)?;
    let basic_rows = timed!(
        &timing,
        "index_basic",
        "import",
        index_daily::import_index_basic(None).await
    )?;
    require_nonzero(basic_rows, "index_basic")?;
    let daily_rows = timed!(
        &timing,
        "index_daily",
        "import",
        index_daily::import_to_dolt(None).await
    )?;
    require_daily_rows("index_daily", daily_rows).await?;

    eprintln!("\n[sync] Updating data_updates...");
    for tbl in [
        "stock_basic",
        "fin_indicators",
        "fin_balance_sheet",
        "fin_income",
        "fin_cash_flow",
        "index_daily",
    ] {
        dolt_sql(&format!(
            "INSERT INTO data_updates (table_name, last_updated, row_count) \
             VALUES ('{tbl}', CURDATE(), (SELECT COUNT(*) FROM {tbl})) \
             ON DUPLICATE KEY UPDATE last_updated=CURDATE(), row_count=VALUES(row_count)"
        ))
        .await?;
    }
    eprintln!("[sync] Complete.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn last_csv_cell_ignores_blank_lines() {
        assert_eq!(last_csv_cell("\nCOUNT(*)\n42\n"), "42");
        assert_eq!(last_csv_cell("  "), "");
    }

    #[test]
    fn require_nonzero_errors_on_zero() {
        assert!(require_nonzero(0, "test").is_err());
        assert!(require_nonzero(1, "test").is_ok());
    }

    #[test]
    fn auto_heal_table_names_match_python() {
        assert_eq!(
            DAILY_AUTO_HEAL_TABLES.len(),
            4,
            "capital_main_flow, index_daily, dragon_list, block_trade"
        );
    }

    // ── Adversarial: daily zero-row calendar decision (#338) ──────────────
    //
    // daily_zero_row_decision is the pure decision layer over the calendar
    // query result.  RED before #338: the function does not exist yet
    // (compile error); after a first compilable interface commit the GREEN
    // contract below must hold exactly.

    /// Trading days exist in (anchor+1 .. today], yet zero rows were imported:
    /// the pipeline must FAIL (require_nonzero semantics) — a silent Ok here
    /// would mask a broken data source exactly like the #306 weekend bug but
    /// on a trading day.
    #[test]
    fn daily_zero_row_decision_nonempty_calendar_fails() {
        let days = vec!["2026-08-28".to_string()];
        let err = daily_zero_row_decision(Ok(days)).unwrap_err();
        assert!(
            matches!(err, CollectError::InvalidInput(_)),
            "trading days exist but zero rows imported: must fail, got {err:?}"
        );
    }

    /// Ok(empty) must be a no-op.  Adversarial: `trade_calendar` today returns
    /// Err(EmptyCalendar) for an empty range, but the pure decision function
    /// must not unwrap/short-circuit — a future calendar refactor that returns
    /// Ok(empty) must still be treated as a no-op, not as a failure.
    #[test]
    fn daily_zero_row_decision_empty_list_is_noop() {
        assert!(daily_zero_row_decision(Ok(Vec::new())).is_ok());
    }

    /// Err(EmptyCalendar) = calendar unavailable → weekend/no-trading-day
    /// window → zero rows is legitimate; must be a no-op.
    #[test]
    fn daily_zero_row_decision_empty_calendar_error_is_noop() {
        assert!(daily_zero_row_decision(Err(CollectError::EmptyCalendar)).is_ok());
    }

    /// Inverted range (anchor computed after today) must be a no-op, not a
    /// hard failure: a clock skew or empty anchor must never abort sync.
    #[test]
    fn daily_zero_row_decision_inverted_range_is_noop() {
        assert!(
            daily_zero_row_decision(Err(CollectError::InvertedRange {
                start: "2026-08-29".into(),
                end: "2026-08-28".into(),
            }))
            .is_ok()
        );
    }

    /// MissingRepo (investment_data absent) is NOT one of the two tolerated
    /// calendar errors: it must propagate — otherwise a missing repo would be
    /// silently papered over as "no trading days".
    #[test]
    fn daily_zero_row_decision_propagates_missing_repo() {
        let err = daily_zero_row_decision(Err(CollectError::MissingRepo(PathBuf::from(
            "/nonexistent/investment_data",
        ))))
        .unwrap_err();
        assert!(matches!(err, CollectError::MissingRepo(_)));
    }

    /// Dolt subprocess failure must propagate — same rationale as above.
    #[test]
    fn daily_zero_row_decision_propagates_dolt_error() {
        let err = daily_zero_row_decision(Err(CollectError::Dolt {
            stderr: "mock dolt failure".into(),
        }))
        .unwrap_err();
        assert!(matches!(err, CollectError::Dolt { .. }));
    }

    /// Adversarial whitelist-complement check: ANY error outside
    /// {EmptyCalendar, InvertedRange} must propagate.  A naive implementation
    /// that only special-cases Ok(empty)/Err(EmptyCalendar) but swallows
    /// everything else (e.g. `let _ = ...; Ok(())`) would hide real failures.
    #[test]
    fn daily_zero_row_decision_propagates_other_errors() {
        let err =
            daily_zero_row_decision(Err(CollectError::InvalidInput("boom".into()))).unwrap_err();
        assert!(matches!(err, CollectError::InvalidInput(_)));
        let err = daily_zero_row_decision(Err(CollectError::InvalidDate {
            label: "start".into(),
            value: "2026-08-99".into(),
        }))
        .unwrap_err();
        assert!(matches!(err, CollectError::InvalidDate { .. }));
    }

    /// require_nonzero is still the backfill-path guard (plan #338 keeps it
    /// for capital_main_flow backfill: 0 rows = broken source, must fail).
    /// Adversarial: locks the error message shape ("0 rows" + table label) so
    /// a refactor that silently turns the backfill path into a no-op gets
    /// caught at the message-contract level even if the call-site changes.
    #[test]
    fn require_nonzero_zero_row_message_contract() {
        let err = require_nonzero(0, "capital_main_flow").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("capital_main_flow"),
            "table label must appear in the error: {msg}"
        );
        assert!(
            msg.contains("0 rows"),
            "row count must appear in the error: {msg}"
        );
    }

    // ── Requirement: daily zero-row calendar decision (#338) ───────────────
    //
    // Acceptance contract from plan fix-mainflow-sina-remove-sepa (#338):
    // the Ok(non-empty) branch must fail with InvalidInput whose message
    // contains "import returned 0 rows" (require_nonzero wording), and the
    // require_daily_rows quick path must return Ok for rows>0 without touching
    // the calendar.  The no-op/propagate branches are locked by the
    // adversarial tests above — these two cover the remaining contract text.

    #[test]
    fn daily_zero_row_decision_nonempty_calendar_error_mentions_zero_rows() {
        let err = daily_zero_row_decision(Ok(vec!["2026-08-28".to_string()])).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("import returned 0 rows"),
            "plan #338: Ok(non-empty calendar) + 0 rows must carry \
             'import returned 0 rows', got: {msg}"
        );
    }

    /// require_daily_rows(table, rows>0) must succeed immediately without
    /// querying the calendar (plan #338: `if rows > 0 { return Ok(()); }`).
    #[tokio::test]
    async fn require_daily_rows_nonzero_returns_ok_without_calendar() {
        assert!(require_daily_rows("capital_main_flow", 5).await.is_ok());
    }
}
