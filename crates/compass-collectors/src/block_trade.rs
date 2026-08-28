use std::path::{Path, PathBuf};

use chrono::Datelike;

use crate::config::csv_dir;
use crate::csv::write_csv_ordered;
use crate::dolt::import_replace_table;
use crate::eastmoney::{Record, fetch_paginated};
use crate::error::Result;
use crate::http::{EM_MIN_INTERVAL, HttpClient, Throttle};
use crate::progress::Progress;

pub const REPORT_NAME: &str = "RPT_DATA_BLOCKTRADE";
pub const FILTER_COLUMN: &str = "TRADE_DATE";
pub const DOLT_TABLE: &str = "block_trade";
pub const START_YEAR: i32 = 2024;

const DDL: &str = r#"CREATE TABLE IF NOT EXISTS block_trade (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    price DOUBLE NOT NULL,
    volume DOUBLE,
    amount DOUBLE,
    buyer VARCHAR(100),
    seller VARCHAR(100),
    premium_rate DOUBLE,
    update_date DATE,
    PRIMARY KEY (symbol, trade_date, price, volume, amount, buyer, seller)
)"#;

const INSERT_COLS: &str =
    "symbol, trade_date, price, volume, amount, buyer, seller, premium_rate, update_date";

fn symbol_expr() -> &'static str {
    "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)"
}

/// Generate all calendar days for the given years (like Python block_trade).
pub fn daily_dates(years: &[i32]) -> Vec<String> {
    let mut dates = Vec::new();
    for &year in years {
        let start = chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
        let end = chrono::NaiveDate::from_ymd_opt(year, 12, 31).unwrap();
        let mut d = start;
        while d <= end {
            dates.push(d.format("%Y-%m-%d").to_string());
            d = d.succ_opt().unwrap();
        }
    }
    dates
}

fn explicit_range_dates(start: Option<&str>, end: Option<&str>) -> Result<Vec<String>> {
    let start = match start {
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
            crate::error::CollectError::InvalidDate {
                label: "start".into(),
                value: s.into(),
            }
        })?,
        None => chrono::NaiveDate::from_ymd_opt(START_YEAR, 1, 1).unwrap(),
    };
    let end = match end {
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
            crate::error::CollectError::InvalidDate {
                label: "end".into(),
                value: s.into(),
            }
        })?,
        None => chrono::Local::now().date_naive(),
    };
    if start > end {
        return Err(crate::error::CollectError::InvertedRange {
            start: start.format("%Y-%m-%d").to_string(),
            end: end.format("%Y-%m-%d").to_string(),
        });
    }
    let mut dates = Vec::new();
    let mut d = start;
    while d <= end {
        dates.push(d.format("%Y-%m-%d").to_string());
        d = d.succ_opt().unwrap();
    }
    Ok(dates)
}

/// Fetch block-trade records for one date.
pub async fn fetch_date(
    client: &HttpClient,
    throttle: &mut Throttle,
    date: &str,
    page_size: usize,
) -> Result<Vec<Record>> {
    fetch_paginated(
        client,
        throttle,
        REPORT_NAME,
        FILTER_COLUMN,
        date,
        page_size,
    )
    .await
}

/// Run the block_trade collector: fetch all dates into a CSV.
#[allow(clippy::collapsible_if)]
pub async fn run(
    years: Option<&[i32]>,
    start: Option<&str>,
    end: Option<&str>,
    page_size: usize,
) -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}.csv"));
    let all_dates = if start.is_some() || end.is_some() {
        explicit_range_dates(start, end)?
    } else {
        let years_owned = years.map(|v| v.to_vec()).unwrap_or_else(|| {
            let current = chrono::Local::now().date_naive().year();
            (START_YEAR..=current).collect()
        });
        daily_dates(&years_owned)
    };

    // Normal-year path keeps the incremental "newer than watermark" filter.
    if start.is_none() && end.is_none() {
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let all_dates = all_dates
            .into_iter()
            .filter(|d| d.as_str() <= today.as_str())
            .collect::<Vec<_>>();
        if !all_dates.is_empty() {
            if let Some(since) = crate::dolt::last_report_date(DOLT_TABLE).await? {
                let filtered: Vec<String> = all_dates
                    .into_iter()
                    .filter(|d| d.as_str() > since.as_str())
                    .collect();
                if filtered.is_empty() {
                    eprintln!("No new trade dates to fetch.");
                    return Ok(output_path);
                }
                let client = HttpClient::new()?;
                let mut throttle = Throttle::new(EM_MIN_INTERVAL);
                let mut progress = Progress::new(
                    "block_trade",
                    Some(filtered.len() as u64),
                    Some(output_path.clone()),
                    "start",
                )?;
                let records =
                    fetch_dates_inner(&client, &mut throttle, &filtered, page_size).await?;
                finish_block_trade(&output_path, records, &mut progress).await?;
                return Ok(output_path);
            }
        }
        let client = HttpClient::new()?;
        let mut throttle = Throttle::new(EM_MIN_INTERVAL);
        let mut progress = Progress::new(
            "block_trade",
            Some(all_dates.len() as u64),
            Some(output_path.clone()),
            "start",
        )?;
        let records = fetch_dates_inner(&client, &mut throttle, &all_dates, page_size).await?;
        finish_block_trade(&output_path, records, &mut progress).await?;
        return Ok(output_path);
    }

    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut progress = Progress::new(
        "block_trade",
        Some(all_dates.len() as u64),
        Some(output_path.clone()),
        "start",
    )?;
    let records = fetch_dates_inner(&client, &mut throttle, &all_dates, page_size).await?;
    finish_block_trade(&output_path, records, &mut progress).await?;
    Ok(output_path)
}

async fn fetch_dates_inner(
    client: &HttpClient,
    throttle: &mut Throttle,
    dates: &[String],
    page_size: usize,
) -> Result<Vec<Record>> {
    let mut all_records = Vec::new();
    for (i, date) in dates.iter().enumerate() {
        let records = fetch_date(client, throttle, date, page_size).await?;
        eprintln!("[{}] {}", i + 1, date);
        all_records.extend(records);
        // Progress update is best-effort here; a failed progress write must
        // not abort collection.
        // TODO: switch to best-effort Progress when available.
    }
    Ok(all_records)
}

async fn finish_block_trade(
    output_path: &Path,
    records: Vec<Record>,
    progress: &mut Progress,
) -> Result<()> {
    if records.is_empty() {
        let _ = std::fs::remove_file(output_path);
    } else {
        write_csv_ordered(output_path, &records)?;
    }
    let _ = progress.finish(Some(records.len() as u64), "Done");
    Ok(())
}

/// Import the fetched CSV into Dolt `block_trade` (merge mode).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{REPORT_NAME}.csv")),
    };
    let insert_sql = format!(
        "INSERT IGNORE INTO {DOLT_TABLE} ({INSERT_COLS}) \
         SELECT DISTINCT {symbol}, DATE(TRADE_DATE), DEAL_PRICE, DEAL_VOLUME, DEAL_AMT, \
                TRIM(BUYER_NAME), TRIM(SELLER_NAME), PREMIUM_RATIO, CURDATE() \
         FROM _tmp_bt \
         WHERE {symbol} IN (SELECT symbol FROM stock_basic) AND DEAL_PRICE IS NOT NULL",
        symbol = symbol_expr(),
    );
    import_replace_table(
        &path,
        "_tmp_bt",
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

    #[test]
    fn daily_dates_covers_year_bounds() {
        let dates = daily_dates(&[2024]);
        assert_eq!(dates.first().unwrap(), "2024-01-01");
        assert_eq!(dates.last().unwrap(), "2024-12-31");
        assert_eq!(dates.len(), 366); // 2024 is a leap year
    }

    #[test]
    fn explicit_range_inverted_is_error() {
        let err = explicit_range_dates(Some("2025-01-02"), Some("2025-01-01")).unwrap_err();
        assert!(matches!(
            err,
            crate::error::CollectError::InvertedRange { .. }
        ));
    }
}
