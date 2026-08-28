use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::csv_dir;
use crate::csv::write_csv;
use crate::dolt::import_replace_table;
use crate::eastmoney::{Record, fetch_paginated, record_get};
use crate::error::Result;
use crate::http::{EM_MIN_INTERVAL, HttpClient, Throttle};
use crate::progress::Progress;

pub const REPORT_NAME: &str = "RPT_DAILYBILLBOARD_DETAILSNEW";
pub const BUY_REPORT_NAME: &str = "RPT_BILLBOARD_DAILYDETAILSBUY";
pub const SELL_REPORT_NAME: &str = "RPT_BILLBOARD_DAILYDETAILSSELL";
pub const FILTER_COLUMN: &str = "TRADE_DATE";
pub const DOLT_TABLE: &str = "dragon_list";
pub const START_DATE: &str = "2020-01-01";

const DDL: &str = r#"CREATE TABLE IF NOT EXISTS dragon_list (
    symbol              VARCHAR(20) NOT NULL,
    trade_date          DATE NOT NULL,
    seat_type           VARCHAR(10) NOT NULL,
    buy_amount          DOUBLE,
    sell_amount         DOUBLE,
    net_amount          DOUBLE,
    institution_flag    TINYINT,
    update_date         DATE,
    PRIMARY KEY (symbol, trade_date, seat_type)
)"#;

const INSERT_COLS: &str = "SEAT_TYPE, BUY_AMOUNT, SELL_AMOUNT, NET_AMOUNT, INSTITUTION_FLAG";

#[derive(Serialize, Clone)]
struct DragonRecord {
    #[serde(rename = "SECUCODE")]
    secucode: String,
    #[serde(rename = "SECURITY_CODE")]
    security_code: String,
    #[serde(rename = "TRADE_DATE")]
    trade_date: String,
    #[serde(rename = "SEAT_TYPE")]
    seat_type: String,
    #[serde(rename = "BUY_AMOUNT")]
    buy_amount: f64,
    #[serde(rename = "SELL_AMOUNT")]
    sell_amount: f64,
    #[serde(rename = "NET_AMOUNT")]
    net_amount: f64,
    #[serde(rename = "INSTITUTION_FLAG")]
    institution_flag: i32,
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

fn as_float(value: &str) -> f64 {
    if value.is_empty() {
        return 0.0;
    }
    value.parse::<f64>().unwrap_or(0.0)
}

/// Classify a seat from its EastMoney name.
fn seat_type(name: &str) -> (String, i32) {
    if name == "机构专用" {
        ("机构专用".to_string(), 1)
    } else if name.ends_with("专用") {
        (name.to_string(), 0)
    } else {
        ("营业部".to_string(), 0)
    }
}

/// Merge BUY/SELL seat records into one row per (symbol, trade_date, seat_type).
fn merge_seats(records: &[Record]) -> Vec<DragonRecord> {
    let mut seen: HashSet<(String, String, String, String, String, String)> = HashSet::new();
    let mut raw: Vec<&Record> = Vec::new();

    for r in records {
        let key = (
            record_get(r, "SECUCODE").unwrap_or("").to_string(),
            record_get(r, "TRADE_DATE")
                .unwrap_or("")
                .chars()
                .take(10)
                .collect(),
            record_get(r, "OPERATEDEPT_NAME").unwrap_or("").to_string(),
            record_get(r, "BUY").unwrap_or("").to_string(),
            record_get(r, "SELL").unwrap_or("").to_string(),
            record_get(r, "NET").unwrap_or("").to_string(),
        );
        if seen.insert(key) {
            raw.push(r);
        }
    }

    let mut order: Vec<(String, String, String)> = Vec::new();
    let mut index: HashMap<(String, String, String), usize> = HashMap::new();
    let mut rows: Vec<DragonRecord> = Vec::new();

    for r in raw {
        let secucode = record_get(r, "SECUCODE").unwrap_or("").to_string();
        let security_code = record_get(r, "SECURITY_CODE").unwrap_or("").to_string();
        let day: String = record_get(r, "TRADE_DATE")
            .unwrap_or("")
            .chars()
            .take(10)
            .collect();
        let (seat, inst) = seat_type(record_get(r, "OPERATEDEPT_NAME").unwrap_or(""));
        let key = (secucode.clone(), day.clone(), seat.clone());
        let idx = if let Some(&idx) = index.get(&key) {
            idx
        } else {
            let idx = rows.len();
            index.insert(key.clone(), idx);
            order.push(key);
            rows.push(DragonRecord {
                secucode,
                security_code,
                trade_date: day,
                seat_type: seat,
                buy_amount: 0.0,
                sell_amount: 0.0,
                net_amount: 0.0,
                institution_flag: 0,
            });
            idx
        };
        let row = &mut rows[idx];
        row.buy_amount += as_float(record_get(r, "BUY").unwrap_or(""));
        row.sell_amount += as_float(record_get(r, "SELL").unwrap_or(""));
        row.net_amount += as_float(record_get(r, "NET").unwrap_or(""));
        row.institution_flag = row.institution_flag.max(inst);
    }

    // The order field is redundant with rows, but kept to make the
    // deliberate insertion order explicit and auditable.
    let _ = order;
    rows
}

fn symbol_expr() -> &'static str {
    "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)"
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
    date: &str,
    page_size: usize,
) -> Result<Vec<Record>> {
    let buy = fetch_paginated(
        client,
        throttle,
        BUY_REPORT_NAME,
        FILTER_COLUMN,
        date,
        page_size,
    )
    .await?;
    let sell = fetch_paginated(
        client,
        throttle,
        SELL_REPORT_NAME,
        FILTER_COLUMN,
        date,
        page_size,
    )
    .await?;
    let mut records = buy;
    records.extend(sell);
    Ok(records)
}

/// Fetch daily billboard seat data into a CSV.
pub async fn run(start: Option<&str>, end: Option<&str>, page_size: usize) -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}.csv"));
    let end_date = match end {
        Some(e) => e.to_string(),
        None => chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string(),
    };

    let explicit_range = start.is_some();
    let start_date = if explicit_range {
        start.unwrap().to_string()
    } else {
        let since = crate::dolt::last_report_date(DOLT_TABLE).await?;
        if let Some(since) = since {
            eprintln!("Last trade date in Dolt: {since}, fetching only newer days");
            next_day(&since)?
        } else {
            START_DATE.to_string()
        }
    };

    if !explicit_range && start_date.as_str() > end_date.as_str() {
        eprintln!("No new trading days to fetch.");
        return Ok(output_path);
    }
    let dates = date_range(&start_date, &end_date)?;
    if !explicit_range && dates.is_empty() {
        eprintln!("No new trading days to fetch.");
        return Ok(output_path);
    }

    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut progress = Progress::new(
        "dragon",
        Some(dates.len() as u64),
        Some(output_path.clone()),
        "start",
    )?;
    let mut all_records = Vec::new();
    let mut failure: Option<String> = None;

    for (i, day) in dates.iter().enumerate() {
        eprintln!("[{day}] ...");
        match fetch_date(&client, &mut throttle, day, page_size).await {
            Ok(records) => {
                let merged = merge_seats(&records);
                eprintln!("{} records", merged.len());
                all_records.extend(merged);
                let _ = progress.update(
                    Some((i + 1) as u64),
                    Some(all_records.len() as u64),
                    Some(day.clone()),
                    Some(format!("Fetched {day}")),
                    None,
                );
            }
            Err(e) => {
                failure = Some(format!("{day}: {e}"));
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

    if all_records.is_empty() {
        let _ = std::fs::remove_file(&output_path);
    } else {
        write_csv(&output_path, &all_records)?;
    }
    let _ = progress.finish(Some(all_records.len() as u64), "Done");
    Ok(output_path)
}

/// Import the fetched CSV into Dolt `dragon_list` (merge mode).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{REPORT_NAME}.csv")),
    };
    let insert_sql = format!(
        "INSERT IGNORE INTO {DOLT_TABLE} (symbol, trade_date, {INSERT_COLS}, update_date) \
         SELECT {symbol}, TRADE_DATE, {cols}, CURDATE() \
         FROM _tmp_dr \
         WHERE {symbol} IN (SELECT symbol FROM stock_basic)",
        symbol = symbol_expr(),
        cols = INSERT_COLS,
    );
    import_replace_table(
        &path,
        "_tmp_dr",
        DDL,
        &insert_sql,
        DOLT_TABLE,
        &format!("EastMoney datacenter {REPORT_NAME}"),
        "MAX(trade_date)",
        None,
        true,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(pairs: &[(&str, &str)]) -> Record {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn seat_type_classifies_institution_special_and_branch() {
        assert_eq!(seat_type("机构专用"), ("机构专用".to_string(), 1));
        assert_eq!(seat_type("深股通专用"), ("深股通专用".to_string(), 0));
        assert_eq!(seat_type("中信证券北京总部"), ("营业部".to_string(), 0));
    }

    #[test]
    fn as_float_parses_empty_as_zero() {
        assert_eq!(as_float(""), 0.0);
        assert_eq!(as_float("12.5"), 12.5);
        assert_eq!(as_float("abc"), 0.0);
    }

    #[test]
    fn merge_seats_dedupes_and_aggregates() {
        let records = vec![
            rec(&[
                ("SECUCODE", "000001.SZ"),
                ("SECURITY_CODE", "000001"),
                ("TRADE_DATE", "2026-08-27 00:00:00"),
                ("OPERATEDEPT_NAME", "机构专用"),
                ("BUY", "10"),
                ("SELL", "2"),
                ("NET", "8"),
            ]),
            rec(&[
                ("SECUCODE", "000001.SZ"),
                ("SECURITY_CODE", "000001"),
                ("TRADE_DATE", "2026-08-27 00:00:00"),
                ("OPERATEDEPT_NAME", "机构专用"),
                ("BUY", "1"),
                ("SELL", "1"),
                ("NET", "0"),
            ]),
            rec(&[
                ("SECUCODE", "000001.SZ"),
                ("SECURITY_CODE", "000001"),
                ("TRADE_DATE", "2026-08-27 00:00:00"),
                ("OPERATEDEPT_NAME", "机构专用"),
                ("BUY", "10"),
                ("SELL", "2"),
                ("NET", "8"),
            ]),
        ];
        let rows = merge_seats(&records);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].buy_amount, 11.0);
        assert_eq!(rows[0].sell_amount, 3.0);
        assert_eq!(rows[0].net_amount, 8.0);
        assert_eq!(rows[0].institution_flag, 1);
    }

    #[test]
    fn date_range_rejects_inverted() {
        let err = date_range("2026-08-28", "2026-08-27").unwrap_err();
        assert!(matches!(
            err,
            crate::error::CollectError::InvertedRange { .. }
        ));
    }
}
