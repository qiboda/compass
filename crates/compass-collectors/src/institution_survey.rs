use std::path::{Path, PathBuf};

use crate::config::csv_dir;
use crate::csv::write_csv_ordered;
use crate::dolt::import_replace_table;
use crate::eastmoney::{Record, fetch_paginated};
use crate::error::Result;
use crate::http::{EM_MIN_INTERVAL, HttpClient, Throttle};
use crate::progress::Progress;

/// EastMoney institution-survey report name.
pub const REPORT_NAME: &str = "RPT_ORG_SURVEYNEW";
/// Filter column used for incremental paging.
pub const FILTER_COLUMN: &str = "NOTICE_DATE";
/// Dolt target table name.
pub const DOLT_TABLE: &str = "institution_survey";
/// First notice date to fetch.
pub const START_DATE: &str = "2025-08-01";

const DDL: &str = r#"CREATE TABLE IF NOT EXISTS institution_survey (
    symbol      VARCHAR(20) NOT NULL,
    survey_date DATE NOT NULL,
    org_name    VARCHAR(1000) NOT NULL,
    survey_type VARCHAR(300),
    update_date DATE,
    PRIMARY KEY (symbol, survey_date, org_name)
)"#;

const INSERT_COLS: &str = "org_name, survey_type";

const CREATE_TMP_SQL: &str = r#"CREATE TABLE _tmp_svy (
    SECUCODE VARCHAR(20), SECURITY_CODE VARCHAR(20),
    RECEIVE_START_DATE DATETIME, RECEIVE_OBJECT VARCHAR(1000),
    RECEIVE_WAY_EXPLAIN VARCHAR(500))"#;

fn symbol_expr() -> &'static str {
    "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)"
}

fn next_day(date_str: &str) -> Result<String> {
    let day = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").map_err(|_| {
        crate::error::CollectError::InvalidDate {
            label: "date".into(),
            value: date_str.into(),
        }
    })?;
    let next = day
        .succ_opt()
        .ok_or_else(|| crate::error::CollectError::InvalidDate {
            label: "date".into(),
            value: date_str.into(),
        })?;
    Ok(next.format("%Y-%m-%d").to_string())
}

fn date_range(start: &str, end: &str) -> Result<Vec<String>> {
    let start_dt = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").map_err(|_| {
        crate::error::CollectError::InvalidDate {
            label: "start".into(),
            value: start.into(),
        }
    })?;
    let end_dt = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d").map_err(|_| {
        crate::error::CollectError::InvalidDate {
            label: "end".into(),
            value: end.into(),
        }
    })?;
    if start_dt > end_dt {
        return Err(crate::error::CollectError::InvertedRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }
    let mut out = Vec::new();
    let mut d = start_dt;
    while d <= end_dt {
        out.push(d.format("%Y-%m-%d").to_string());
        d = d.succ_opt().expect("valid calendar date");
    }
    Ok(out)
}

async fn fetch_date(
    client: &HttpClient,
    throttle: &mut Throttle,
    notice_date: &str,
    page_size: usize,
) -> Result<Vec<Record>> {
    fetch_paginated(
        client,
        throttle,
        REPORT_NAME,
        FILTER_COLUMN,
        notice_date,
        page_size,
    )
    .await
}

/// Fetch institution survey rows into a CSV.
pub async fn run(start_date: Option<&str>, page_size: usize) -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}.csv"));

    let since = crate::dolt::last_report_date(DOLT_TABLE).await?;
    let start = if let Some(since) = since {
        next_day(&since)?
    } else {
        start_date.unwrap_or(START_DATE).to_string()
    };

    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    if start.as_str() > today.as_str() {
        eprintln!("No new survey dates to fetch.");
        return Ok(output_path);
    }
    let dates = date_range(&start, &today)?;
    if dates.is_empty() {
        eprintln!("No new survey dates to fetch.");
        return Ok(output_path);
    }

    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut progress = Progress::new(
        "institution_survey",
        Some(dates.len() as u64),
        Some(output_path.clone()),
        "start",
    )?;
    let mut all_records = Vec::new();
    let mut failure: Option<String> = None;

    for (i, notice_date) in dates.iter().enumerate() {
        eprintln!("[{i}/{len}] {notice_date} ...", len = dates.len());
        match fetch_date(&client, &mut throttle, notice_date, page_size).await {
            Ok(records) => {
                eprintln!("{} records", records.len());
                all_records.extend(records);
                let _ = progress.update(
                    Some((i + 1) as u64),
                    Some(all_records.len() as u64),
                    Some(notice_date.clone()),
                    Some(format!("Fetched {notice_date}")),
                    None,
                );
            }
            Err(e) => {
                failure = Some(format!("{notice_date}: {e}"));
                eprintln!("FAILED: {e}");
                break;
            }
        }
    }

    if let Some(failure) = failure {
        let _ = std::fs::remove_file(&output_path);
        let _ = progress.fail(&failure, "failed");
        return Err(crate::error::CollectError::InvalidInput(format!(
            "Fetch aborted at {failure} — no CSV written"
        )));
    }

    write_csv_ordered(&output_path, &all_records)?;
    let _ = progress.finish(Some(all_records.len() as u64), "Done");
    Ok(output_path)
}

/// Import the fetched CSV into Dolt `institution_survey` (merge mode).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{REPORT_NAME}.csv")),
    };
    let insert_sql = format!(
        "INSERT IGNORE INTO {DOLT_TABLE} (symbol, survey_date, {INSERT_COLS}, update_date) \
         SELECT MAX(s), MAX(d), MAX(TRIM(o)), MAX(TRIM(st)), MAX(u) FROM ( \
           SELECT {symbol} AS s, DATE(RECEIVE_START_DATE) AS d, \
                  RECEIVE_OBJECT AS o, RECEIVE_WAY_EXPLAIN AS st, CURDATE() AS u, \
                  HEX(TRIM(RECEIVE_OBJECT)) AS gk \
           FROM _tmp_svy \
           WHERE {symbol} IN (SELECT symbol FROM stock_basic) \
             AND RECEIVE_START_DATE IS NOT NULL \
         ) t GROUP BY s, d, gk",
        symbol = symbol_expr(),
    );
    import_replace_table(
        &path,
        "_tmp_svy",
        DDL,
        &insert_sql,
        DOLT_TABLE,
        &format!("EastMoney datacenter {REPORT_NAME}"),
        "MAX(survey_date)",
        Some(CREATE_TMP_SQL),
        true,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_range_rejects_inverted() {
        let err = date_range("2026-08-28", "2026-08-27").unwrap_err();
        assert!(matches!(
            err,
            crate::error::CollectError::InvertedRange { .. }
        ));
    }

    #[test]
    fn next_day_advances_date() {
        assert_eq!(next_day("2026-08-27").unwrap(), "2026-08-28");
    }
}
