//! Orchestration CLI for the Rust collector migration.
//!
//! Mirrors `collectors/main.py`: fetch/import dispatch, `sync` (full
//! do_sync order), auto-heal/backfill, progress display, and
//! `sync-investment` Dolt repo sync.

use std::path::Path;
use std::process::Command;

use chrono::{Duration, Local};

use crate::config::{csv_dir, dolt_dir, dolt_exists, investment_data_dir};
use crate::dolt::{dolt_sql, dolt_sql_csv, set_last_report_date};
use crate::error::{CollectError, Result};
use crate::{
    balance_sheet, block_trade, cash_flow, dragon, fin_indicators, income, index_daily,
    institution_survey, main_flow, progress, stock_basic_official,
};

pub const DEFAULT_PERIODS: &str = "Q1,Q2,Q3,FY";
pub const DEFAULT_PAGE_SIZE: usize = 100;
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
        "main_flow" => main_flow::run(DEFAULT_PAGE_SIZE * 10).await,
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
    let auto_heal_enabled = std::env::var("COMPASS_AUTO_HEAL")
        .map(|v| v != "0")
        .unwrap_or(true);
    if !auto_heal_enabled {
        eprintln!("[sync] Auto-heal disabled (COMPASS_AUTO_HEAL=0)");
    } else {
        auto_heal().await?;
    }

    eprintln!("[sync] Fetching stock_basic...");
    stock_basic_official::run(None, None).await?;
    require_nonzero(
        stock_basic_official::import_to_dolt(None).await?,
        "stock_basic",
    )?;

    eprintln!("\n[sync] Fetching fin_indicators (incremental)...");
    let path = fin_indicators::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await?;
    eprintln!("  -> {}", path.display());
    require_nonzero(
        fin_indicators::import_to_dolt(None).await?,
        "fin_indicators",
    )?;

    eprintln!("\n[sync] Fetching balance_sheet (incremental)...");
    let path = balance_sheet::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await?;
    eprintln!("  -> {}", path.display());
    require_nonzero(
        balance_sheet::import_to_dolt(None).await?,
        "fin_balance_sheet",
    )?;

    eprintln!("\n[sync] Fetching income (incremental)...");
    let path = income::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await?;
    eprintln!("  -> {}", path.display());
    require_nonzero(income::import_to_dolt(None).await?, "fin_income")?;

    eprintln!("\n[sync] Fetching cash_flow (incremental)...");
    let path = cash_flow::run(None, DEFAULT_PERIODS, DEFAULT_PAGE_SIZE, true).await?;
    eprintln!("  -> {}", path.display());
    require_nonzero(cash_flow::import_to_dolt(None).await?, "fin_cash_flow")?;

    eprintln!("\n[sync] Fetching dragon_list...");
    dragon::run(None, None, DEFAULT_PAGE_SIZE).await?;
    require_nonzero(dragon::import_to_dolt(None).await?, "dragon_list")?;

    eprintln!("\n[sync] Fetching block_trade...");
    block_trade::run(None, None, None, DEFAULT_PAGE_SIZE).await?;
    require_nonzero(block_trade::import_to_dolt(None).await?, "block_trade")?;

    eprintln!("\n[sync] Fetching institution_survey...");
    institution_survey::run(None, DEFAULT_PAGE_SIZE).await?;
    require_nonzero(
        institution_survey::import_to_dolt(None).await?,
        "institution_survey",
    )?;

    eprintln!("\n[sync] Fetching main_flow...");
    main_flow::run(DEFAULT_PAGE_SIZE * 10).await?;
    require_nonzero(main_flow::import_to_dolt(None).await?, "capital_main_flow")?;

    eprintln!("\n[sync] Fetching index_daily...");
    index_daily::run().await?;
    let basic_rows = index_daily::import_index_basic(None).await?;
    require_nonzero(basic_rows, "index_basic")?;
    let daily_rows = index_daily::import_to_dolt(None).await?;
    require_nonzero(daily_rows, "index_daily")?;

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
}
