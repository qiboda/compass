use std::collections::BTreeSet;

use crate::config::{dolt_dir, ensure_dolt_repo, investment_data_dir};
use crate::error::{CollectError, Result};

fn parse_iso_date(value: &str, label: &str) -> Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
        CollectError::InvalidDate {
            label: label.to_string(),
            value: value.to_string(),
        }
    })
}

async fn dolt_investment_csv(sql: &str) -> Result<String> {
    let dir = investment_data_dir();
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 investment path".into()))?;
    let output = tokio::process::Command::new("dolt")
        .args(["--data-dir", dir_str, "sql", "-r", "csv", "-q", sql])
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(CollectError::Dolt { stderr })
    }
}

async fn dolt_compass_csv(sql: &str) -> Result<String> {
    let dir = dolt_dir();
    if !ensure_dolt_repo(&dir).is_ok() {
        return Err(CollectError::MissingRepo(dir));
    }
    let dir_str = dir
        .to_str()
        .ok_or_else(|| CollectError::InvalidInput("non-UTF8 dolt path".into()))?;
    let output = tokio::process::Command::new("dolt")
        .args(["--data-dir", dir_str, "sql", "-r", "csv", "-q", sql])
        .output()
        .await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(CollectError::Dolt { stderr })
    }
}

/// Return SSE open trading days in `[start, end]` from investment_data.
pub async fn trade_calendar(start: &str, end: &str) -> Result<Vec<String>> {
    let start_dt = parse_iso_date(start, "start")?;
    let end_dt = parse_iso_date(end, "end")?;
    if start_dt > end_dt {
        return Err(CollectError::InvertedRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }
    let dir = investment_data_dir();
    if !ensure_dolt_repo(&dir).is_ok() {
        return Err(CollectError::MissingRepo(dir));
    }
    let sql = format!(
        "SELECT DISTINCT DATE_FORMAT(`date`, '%Y-%m-%d') AS d \
         FROM ts_trade_day_calendar \
         WHERE exchange = 'SSE' AND is_open = 1 \
         AND `date` BETWEEN '{start}' AND '{end}' \
         ORDER BY `date`"
    );
    let out = dolt_investment_csv(&sql).await?;
    let days: Vec<String> = out
        .trim()
        .lines()
        .skip(1)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if days.is_empty() {
        return Err(CollectError::EmptyCalendar);
    }
    let set: BTreeSet<String> = days.into_iter().collect();
    Ok(set.into_iter().collect())
}

/// Return trading days in `[start, end]` absent from `table`.
pub async fn missing_dates(
    table: &str,
    date_col: &str,
    start: &str,
    end: &str,
) -> Result<Vec<String>> {
    let calendar = trade_calendar(start, end).await?;
    let sql = format!(
        "SELECT DISTINCT DATE_FORMAT({date_col}, '%Y-%m-%d') AS d \
         FROM {table} WHERE {date_col} BETWEEN '{start}' AND '{end}'"
    );
    let out = dolt_compass_csv(&sql).await?;
    let existing: BTreeSet<String> = out
        .trim()
        .lines()
        .skip(1)
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(calendar
        .into_iter()
        .filter(|d| !existing.contains(d))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso_rejects_invalid() {
        let err = parse_iso_date("2026/01/01", "start").unwrap_err();
        assert!(matches!(err, CollectError::InvalidDate { .. }));
    }
}
