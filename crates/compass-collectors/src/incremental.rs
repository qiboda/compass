use std::path::Path;

use regex::Regex;

use crate::config::dolt_dir;
use crate::csv::{dedupe_csv, write_csv_ordered};
use crate::dolt::dolt_sql_csv;
use crate::eastmoney::{Record, fetch_by_update_date};
use crate::error::Result;
use crate::http::{HttpClient, Throttle};

/// Normalize an API `UPDATE_DATE` to YYYY-MM-DD, or None for empty/invalid.
pub fn normalize_update_date(value: &str) -> Option<String> {
    let s = value.trim();
    if s.is_empty() {
        return None;
    }
    let re = Regex::new(r"^(\d{4})[-/](\d{1,2})[-/](\d{1,2})").ok()?;
    if let Some(caps) = re.captures(s) {
        let year = &caps[1];
        let month = caps[2].parse::<u32>().ok()?;
        let day = caps[3].parse::<u32>().ok()?;
        return Some(format!("{year}-{month:02}-{day:02}"));
    }
    let re = Regex::new(r"^(\d{4})(\d{2})(\d{2})(?:\.0+)?$").ok()?;
    let caps = re.captures(s)?;
    Some(format!("{}-{}-{}", &caps[1], &caps[2], &caps[3]))
}

/// Resolve the incremental UPDATE_DATE anchor (earlier of Dolt/state sources).
pub async fn update_date_anchor(
    report_name: &str,
    state_path: &Path,
    dolt_table: Option<&str>,
) -> Result<String> {
    let mut sources = Vec::new();
    let dir = dolt_dir();
    if dir.join(".dolt").exists() {
        let table = dolt_table.unwrap_or_else(|| {
            if report_name == "RPT_LICO_FN_CPD" {
                "fin_indicators"
            } else {
                report_name
            }
        });
        let out = dolt_sql_csv(&format!(
            "SELECT last_updated FROM data_updates WHERE table_name='{table}'"
        ))
        .await?;
        if let Some(last) = out.trim().lines().last().map(str::trim)
            && !last.is_empty()
            && last != "NULL"
        {
            sources.push(last.to_string());
        }
    }
    if state_path.exists()
        && let Ok(text) = std::fs::read_to_string(state_path)
        && let Ok(state) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(v) = state.get("last_update_date").and_then(|v| v.as_str())
        && !v.is_empty()
    {
        sources.push(v.to_string());
    }
    if sources.is_empty() {
        return Ok(String::new());
    }
    let anchor = sources.into_iter().min().unwrap_or_default();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    Ok(if anchor > today { today } else { anchor })
}

/// Run one UPDATE_DATE incremental fetch and write CSV/state.
///
/// Simplified port of `common.py::fetch_incremental` for the F10 collectors.
#[allow(clippy::too_many_arguments)]
pub async fn fetch_incremental(
    client: &HttpClient,
    throttle: &mut Throttle,
    report_name: &str,
    dolt_table: &str,
    output_path: &Path,
    state_path: &Path,
    page_size: usize,
    initial_anchor: &str,
) -> Result<usize> {
    let anchor = update_date_anchor(report_name, state_path, Some(dolt_table)).await?;
    let anchor = if anchor.is_empty() {
        initial_anchor.to_string()
    } else {
        anchor
    };

    let records = fetch_by_update_date(client, throttle, report_name, &anchor, page_size).await?;
    let total = records.len();
    if total > 0 {
        let records: Vec<Record> = records;
        write_csv_ordered(output_path, &records)?;
        dedupe_csv(output_path, "REPORT_DATE")?;
        let mut max_report = String::new();
        let mut max_update = String::new();
        for r in &records {
            if let Some(v) =
                crate::eastmoney::record_get(r, "REPORT_DATE").and_then(normalize_update_date)
                && v > max_report
            {
                max_report = v;
            }
            if let Some(v) =
                crate::eastmoney::record_get(r, "UPDATE_DATE").and_then(normalize_update_date)
                && v > max_update
            {
                max_update = v;
            }
        }
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let max_update = if max_update.is_empty() {
            update_date_anchor(report_name, state_path, Some(dolt_table))
                .await?
                .to_string()
        } else if max_update > today {
            today
        } else {
            max_update
        };
        let state = serde_json::json!({
            "last_report_date": max_report,
            "total_rows": total,
            "last_run": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            "last_update_date": max_update,
        });
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(state_path, serde_json::to_string_pretty(&state)?)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_variants() {
        assert_eq!(
            normalize_update_date("2026-08-13 00:00:00"),
            Some("2026-08-13".into())
        );
        assert_eq!(normalize_update_date("2026/8/3"), Some("2026-08-03".into()));
        assert_eq!(normalize_update_date("20260805"), Some("2026-08-05".into()));
        assert_eq!(normalize_update_date(""), None);
    }
}
