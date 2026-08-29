use std::path::{Path, PathBuf};

use chrono::Datelike;

use crate::config::csv_dir;
use crate::csv::{build_dates, write_csv_ordered};
use crate::dolt::import_replace_table;
use crate::eastmoney::{Record, fetch_paginated};
use crate::error::Result;
use crate::http::{EM_MIN_INTERVAL, HttpClient, Throttle};
use crate::progress::Progress;

/// Static configuration for one F10 financial-statement collector.
pub struct FinancialConfig {
    /// EastMoney report name used in fetch requests.
    pub report_name: &'static str,
    /// Filter column used for incremental paging.
    pub filter_column: &'static str,
    /// Dolt target table name.
    pub dolt_table: &'static str,
    /// First year to fetch.
    pub start_year: i32,
    /// Anchor for the initial update (inclusive).
    pub initial_anchor: &'static str,
    /// API column list (comma-separated, in output order).
    pub cols: &'static str,
    /// DDL of the Dolt target table.
    pub ddl: &'static str,
    /// DDL of the temporary staging table.
    pub tmp_ddl: &'static str,
    /// Name of the temporary staging table.
    pub tmp_name: &'static str,
    /// Columns trimmed with SQL `TRIM` during import.
    pub trim_text_cols: &'static [&'static str],
    /// Whether the non-incremental path uses the last report date in Dolt.
    pub non_incremental_uses_last_report_date: bool,
}

fn symbol_expr() -> &'static str {
    "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)"
}

fn trim_select_cols(cfg: &FinancialConfig) -> String {
    cfg.cols
        .split(", ")
        .map(|col| {
            if cfg.trim_text_cols.contains(&col) {
                format!("TRIM({col}) AS _{col}")
            } else {
                format!("{col} AS _{col}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn upsert_updates(cfg: &FinancialConfig) -> String {
    cfg.cols
        .split(", ")
        .map(|col| format!("{col}=_{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn current_years(cfg: &FinancialConfig) -> Vec<i32> {
    let current = chrono::Local::now().date_naive().year();
    (cfg.start_year..=current).collect()
}

fn dates_for_years(years: &[i32], periods: &str) -> Vec<String> {
    let period_list: Vec<&str> = periods.split(',').map(str::trim).collect();
    build_dates(years, &period_list)
}

async fn run_non_incremental(
    cfg: &FinancialConfig,
    years: Option<&[i32]>,
    periods: &str,
    page_size: usize,
) -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{}.csv", cfg.report_name));
    let years_owned = years
        .map(|v| v.to_vec())
        .unwrap_or_else(|| current_years(cfg));
    let all_dates = dates_for_years(&years_owned, periods);

    let all_dates = if cfg.non_incremental_uses_last_report_date {
        let since = crate::dolt::last_report_date(cfg.dolt_table).await?;
        if let Some(since) = since {
            eprintln!("Last report date in Dolt: {since}, fetching only newer periods");
            let filtered: Vec<String> = all_dates
                .into_iter()
                .filter(|d| d.as_str() >= since.as_str())
                .collect();
            if filtered.is_empty() {
                eprintln!("No new report periods to fetch.");
                return Ok(output_path);
            }
            filtered
        } else {
            all_dates
        }
    } else {
        all_dates
    };

    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut progress = Progress::new(
        cfg.report_name,
        Some(all_dates.len() as u64),
        Some(output_path.clone()),
        "start",
    )?;
    let mut all_records: Vec<Record> = Vec::new();
    for (i, report_date) in all_dates.iter().enumerate() {
        eprintln!("[{}/{}] {} ...", i + 1, all_dates.len(), report_date);
        match fetch_paginated(
            &client,
            &mut throttle,
            cfg.report_name,
            cfg.filter_column,
            report_date,
            page_size,
        )
        .await
        {
            Ok(records) => {
                all_records.extend(records);
                let _ = progress.update(
                    Some((i + 1) as u64),
                    Some(all_records.len() as u64),
                    Some(report_date.clone()),
                    Some(format!("Fetched {report_date}")),
                    None,
                );
            }
            Err(e) => {
                eprintln!("FAILED: {e}");
                // Python's non-incremental path prints and continues on a
                // single period failure; keep that behavior here.
            }
        }
    }
    write_csv_ordered(&output_path, &all_records)?;
    let _ = progress.finish(Some(all_records.len() as u64), "Done");
    Ok(output_path)
}

/// Run one F10 financial collector (incremental or period-enumerated).
pub async fn run(
    cfg: &FinancialConfig,
    years: Option<&[i32]>,
    periods: &str,
    page_size: usize,
    incremental: bool,
) -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{}.csv", cfg.report_name));
    if incremental {
        let state_path = csv_dir()?.join(format!("{}.state.json", cfg.report_name));
        let client = HttpClient::new()?;
        let mut throttle = Throttle::new(EM_MIN_INTERVAL);
        let total = crate::incremental::fetch_incremental(
            &client,
            &mut throttle,
            cfg.report_name,
            cfg.dolt_table,
            &output_path,
            &state_path,
            page_size,
            cfg.initial_anchor,
        )
        .await?;
        eprintln!("Done: {total} records → {}", output_path.display());
        return Ok(output_path);
    }
    run_non_incremental(cfg, years, periods, page_size).await
}

/// Import one F10 collector's CSV into Dolt with upsert semantics.
pub async fn import_to_dolt(cfg: &FinancialConfig, csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{}.csv", cfg.report_name)),
    };
    let insert_sql = format!(
        "INSERT INTO {table} (symbol, report_date, {cols}) \
         SELECT {symbol} AS _sym, CAST(REPORT_DATE AS DATE) AS _rpt, {selects} \
         FROM {tmp} \
         WHERE {symbol} IN (SELECT symbol FROM stock_basic) \
         ON DUPLICATE KEY UPDATE {updates}",
        table = cfg.dolt_table,
        cols = cfg.cols,
        symbol = symbol_expr(),
        selects = trim_select_cols(cfg),
        tmp = cfg.tmp_name,
        updates = upsert_updates(cfg),
    );
    let source_label = format!("EastMoney datacenter {}", cfg.report_name);
    import_replace_table(
        &path,
        cfg.tmp_name,
        cfg.ddl,
        &insert_sql,
        cfg.dolt_table,
        &source_label,
        "MAX(report_date)",
        Some(cfg.tmp_ddl),
        true,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> FinancialConfig {
        FinancialConfig {
            report_name: "RPT_TEST",
            filter_column: "REPORT_DATE",
            dolt_table: "fin_test",
            start_year: 2020,
            initial_anchor: "2020-01-01",
            cols: "A, TEXT_B, C",
            ddl: "",
            tmp_ddl: "",
            tmp_name: "_tmp_test",
            trim_text_cols: &["TEXT_B"],
            non_incremental_uses_last_report_date: true,
        }
    }

    #[test]
    fn upsert_select_aliases_and_trims() {
        let cfg = test_cfg();
        let selects = trim_select_cols(&cfg);
        assert_eq!(selects, "A AS _A, TRIM(TEXT_B) AS _TEXT_B, C AS _C");
    }

    #[test]
    fn upsert_updates_use_aliases() {
        let cfg = test_cfg();
        assert_eq!(upsert_updates(&cfg), "A=_A, TEXT_B=_TEXT_B, C=_C");
    }

    #[test]
    fn dates_for_years_builds_sorted_periods() {
        let dates = dates_for_years(&[2024, 2025], "Q2,FY");
        assert_eq!(
            dates,
            vec!["2024-06-30", "2024-12-31", "2025-06-30", "2025-12-31"]
        );
    }
}
