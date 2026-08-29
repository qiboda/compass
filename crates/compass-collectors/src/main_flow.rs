use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::config::csv_dir;
use crate::csv::write_csv;
use crate::dolt::import_replace_table;
use crate::error::{CollectError, Result};
use crate::http::{EM_MIN_INTERVAL, HttpClient, Throttle};
use crate::progress::Progress;
use crate::proxy::{DEFAULT_PROXY_MAX_ATTEMPTS, ProxyPool, make_proxy_pool};

/// EastMoney main-capital-flow report name.
pub const REPORT_NAME: &str = "RPT_MAIN_MONEY_FLOW";
/// Dolt target table name.
pub const DOLT_TABLE: &str = "capital_main_flow";
/// Source label recorded in `data_updates` for this table.
pub const SOURCE: &str = "EastMoney push2 clist f62";

const PUSH2_DELAY: &str = "https://push2delay.eastmoney.com/api/qt/clist/get";
const PUSH2_MAIN: &str = "https://push2.eastmoney.com/api/qt/clist/get";
const PUSH2_URLS: [&str; 2] = [PUSH2_DELAY, PUSH2_MAIN];

const FFLOW_DAYKLINE_URL: &str = "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get";

fn push2_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), crate::http::EM_UA.to_string());
    headers.insert("Accept".to_string(), "*/*".to_string());
    headers.insert(
        "Referer".to_string(),
        "https://quote.eastmoney.com/".to_string(),
    );
    headers
}

const DDL: &str = r#"CREATE TABLE IF NOT EXISTS capital_main_flow (
    symbol              VARCHAR(20) NOT NULL,
    trade_date          DATE NOT NULL,
    main_net_inflow     DOUBLE,
    main_net_inflow_rate DOUBLE,
    super_large_net     DOUBLE,
    large_net           DOUBLE,
    medium_net          DOUBLE,
    small_net           DOUBLE,
    update_date         DATE,
    PRIMARY KEY (symbol, trade_date)
)"#;

const INSERT_COLS: &str = "main_net_inflow, main_net_inflow_rate, super_large_net, large_net, medium_net, small_net, update_date";

#[derive(Serialize, Clone)]
struct FlowRecord {
    symbol: String,
    trade_date: String,
    main_net_inflow: String,
    main_net_inflow_rate: String,
    super_large_net: String,
    large_net: String,
    medium_net: String,
    small_net: String,
    update_date: String,
}

fn today() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn exchange_prefix(code: &str) -> &'static str {
    if code.starts_with('6') {
        "SH"
    } else if code.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

fn normalize_num(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.is_empty() || s == "-" => String::new(),
        Some(Value::String(s)) => match s.parse::<f64>() {
            Ok(n) => n.to_string(),
            Err(_) => s.clone(),
        },
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

fn trade_date_from_quotes(diff: &[Value]) -> String {
    let mut latest: i64 = 0;
    for item in diff {
        if let Some(ts) = item.get("f124").and_then(Value::as_i64)
            && ts > 0
        {
            latest = latest.max(ts);
        }
    }
    if latest > 0
        && let Some(dt) = chrono::DateTime::from_timestamp(latest, 0)
    {
        let bj = dt + chrono::Duration::hours(8);
        return bj.date_naive().format("%Y-%m-%d").to_string();
    }
    today()
}

fn build_records(diff: &[Value], trade_date: &str) -> Vec<FlowRecord> {
    let update_date = today();
    let mut records = Vec::new();
    for item in diff {
        let Some(code) = item.get("f12").and_then(Value::as_str) else {
            continue;
        };
        if code.is_empty() {
            continue;
        }
        let symbol = format!("{}{}", exchange_prefix(code), code);
        records.push(FlowRecord {
            symbol,
            trade_date: trade_date.to_string(),
            main_net_inflow: normalize_num(item.get("f62")),
            main_net_inflow_rate: normalize_num(item.get("f184")),
            super_large_net: normalize_num(item.get("f66")),
            large_net: normalize_num(item.get("f72")),
            medium_net: normalize_num(item.get("f78")),
            small_net: normalize_num(item.get("f84")),
            update_date: update_date.clone(),
        });
    }
    records
}

fn snapshot_params(page_number: usize, page_size: usize) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("fid".to_string(), "f62".to_string());
    params.insert("po".to_string(), "1".to_string());
    params.insert("pz".to_string(), page_size.to_string());
    params.insert("pn".to_string(), page_number.to_string());
    params.insert("np".to_string(), "1".to_string());
    params.insert("fltt".to_string(), "2".to_string());
    params.insert("invt".to_string(), "2".to_string());
    params.insert(
        "fs".to_string(),
        "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23".to_string(),
    );
    params.insert(
        "fields".to_string(),
        "f12,f14,f2,f3,f62,f184,f66,f69,f72,f75,f78,f81,f84,f87,f204,f205,f124".to_string(),
    );
    params
}

async fn fetch_page(
    client: &HttpClient,
    throttle: &mut Throttle,
    pool: &mut Option<ProxyPool>,
    page_number: usize,
    page_size: usize,
) -> Result<(Vec<Value>, u64)> {
    let params = snapshot_params(page_number, page_size);
    let headers = push2_headers();

    for base in PUSH2_URLS {
        for attempt in 0..4 {
            let mut last_err: Option<CollectError> = None;
            let mut empty_response = false;
            for proxy_attempt in 0..=DEFAULT_PROXY_MAX_ATTEMPTS {
                let proxy: Option<String> = if proxy_attempt < DEFAULT_PROXY_MAX_ATTEMPTS {
                    if let Some(pool) = pool.as_mut() {
                        pool.get_proxy().await
                    } else {
                        None
                    }
                } else {
                    None
                };
                throttle.acquire().await;
                match client
                    .get_json_with_headers_and_proxy(base, &params, &headers, proxy.as_deref())
                    .await
                {
                    Ok(data) => {
                        let empty = data
                            .get("data")
                            .and_then(|d| d.get("diff"))
                            .map(|d| d.as_array().map(|a| a.is_empty()).unwrap_or(true))
                            .unwrap_or(true);
                        if empty {
                            eprintln!("empty response from {base}");
                            empty_response = true;
                            break;
                        }
                        let diff = data
                            .get("data")
                            .and_then(|d| d.get("diff"))
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        let total = data
                            .get("data")
                            .and_then(|d| d.get("total"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        return Ok((diff, total));
                    }
                    Err(e) => {
                        if let Some(proxy_url) = proxy {
                            if !matches!(e, CollectError::HttpStatus(_)) {
                                if let Some(pool) = pool.as_ref() {
                                    pool.delete_proxy(&proxy_url).await;
                                }
                                continue;
                            }
                            last_err = Some(e);
                            break;
                        }
                        last_err = Some(e);
                        break;
                    }
                }
            }
            if empty_response {
                break;
            }
            if let Some(e) = last_err {
                let is_429 = matches!(e, CollectError::HttpStatus(429));
                if is_429 {
                    let wait = 15.0 + rand::random::<f64>() * 5.0;
                    eprintln!("    429, waiting {wait:.0}s...");
                    tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
                    continue;
                }
                if attempt < 3 {
                    let wait = ((1u64 << attempt) * 1000).min(30_000) as f64 / 1000.0
                        + rand::random::<f64>() * 3.0;
                    eprintln!("    retry {} in {wait:.0}s: {e}", attempt + 1);
                    tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
                    continue;
                }
                eprintln!("    FAILED {base}: {e}");
            }
        }
    }
    Ok((Vec::new(), 0))
}

async fn fetch_snapshot(
    client: &HttpClient,
    throttle: &mut Throttle,
    page_size: usize,
) -> Result<Vec<Value>> {
    let mut pool = make_proxy_pool();
    let mut all_items = Vec::new();
    let mut total = 0u64;
    let mut page = 1usize;
    loop {
        let (items, data_total) = fetch_page(client, throttle, &mut pool, page, page_size).await?;
        if items.is_empty() {
            break;
        }
        if data_total > 0 {
            total = data_total;
        }
        all_items.extend(items);
        if total > 0 && all_items.len() >= total as usize {
            break;
        }
        page += 1;
    }
    Ok(all_items)
}

/// Fetch the latest-day full-market main capital flow snapshot into a CSV.
pub async fn run(page_size: usize) -> Result<PathBuf> {
    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}.csv"));
    let today = today();

    let last = crate::dolt::last_report_date(DOLT_TABLE).await?;
    if last.as_deref() == Some(today.as_str()) {
        eprintln!("Data up to date ({today}); skipping fetch");
        return Ok(output_path);
    }

    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut progress = Progress::new("main_flow", None, Some(output_path.clone()), "start")?;
    let diff = fetch_snapshot(&client, &mut throttle, page_size).await?;
    if diff.is_empty() {
        let _ = std::fs::remove_file(&output_path);
        let _ = progress.fail("No data from push2", "failed");
        return Err(CollectError::InvalidInput(
            "No data from push2 (rate-limited or empty) — aborting, no CSV written".into(),
        ));
    }

    let trade_date = trade_date_from_quotes(&diff);
    let _ = progress.update(
        Some(diff.len() as u64),
        Some(diff.len() as u64),
        Some(trade_date.clone()),
        Some(format!("Snapshot fetched, trade_date={trade_date}")),
        None,
    );
    eprintln!("Snapshot: {} items, trade_date={trade_date}", diff.len());

    if last.as_deref() == Some(trade_date.as_str()) {
        eprintln!("Trade date {trade_date} already imported; skipping");
        let _ = progress.finish(Some(diff.len() as u64), "already imported");
        return Ok(output_path);
    }

    let records = build_records(&diff, &trade_date);
    write_csv(&output_path, &records)?;
    let _ = progress.finish(Some(records.len() as u64), "Done");
    Ok(output_path)
}

/// Import the fetched CSV into Dolt `capital_main_flow` (merge mode).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join(format!("{REPORT_NAME}.csv")),
    };
    let insert_sql = format!(
        "INSERT IGNORE INTO {DOLT_TABLE} (symbol, trade_date, {INSERT_COLS}) \
         SELECT symbol, trade_date, {INSERT_COLS} FROM _tmp_mf \
         WHERE symbol IN (SELECT symbol FROM stock_basic)",
    );
    import_replace_table(
        &path,
        "_tmp_mf",
        DDL,
        &insert_sql,
        DOLT_TABLE,
        SOURCE,
        "MAX(trade_date)",
        None,
        true,
    )
    .await
}

// ── Historical per-symbol backfill (issue #308) ──────────────────────────

fn fflow_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), crate::http::EM_UA.to_string());
    headers.insert("Accept".to_string(), "*/*".to_string());
    headers.insert(
        "Referer".to_string(),
        "https://quote.eastmoney.com/".to_string(),
    );
    headers
}

async fn backfill_symbols() -> Result<Vec<String>> {
    let dir = crate::config::dolt_dir();
    if dir.join(".dolt").exists() {
        let out =
            crate::dolt::dolt_sql_csv("SELECT symbol FROM stock_basic ORDER BY symbol").await?;
        let symbols: Vec<String> = out
            .trim()
            .lines()
            .skip(1)
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if symbols.is_empty() {
            return Err(CollectError::InvalidInput(
                "backfill: stock_basic contains no symbols".into(),
            ));
        }
        return Ok(symbols);
    }
    Ok(vec!["SH600519".to_string()])
}

fn symbol_to_secid(symbol: &str) -> Result<String> {
    if symbol.len() != 8 {
        return Err(CollectError::InvalidInput(format!(
            "cannot derive secid from symbol {symbol:?}"
        )));
    }
    let market = &symbol[..2];
    let code = &symbol[2..];
    Ok(match market {
        "SH" => format!("1.{code}"),
        _ => format!("0.{code}"),
    })
}

fn fflow_record(symbol: &str, row: &str) -> Option<FlowRecord> {
    let parts: Vec<&str> = row.split(',').collect();
    if parts.len() < 7 {
        return None;
    }
    Some(FlowRecord {
        symbol: symbol.to_string(),
        trade_date: parts[0].trim().to_string(),
        main_net_inflow: normalize_num(Some(&Value::String(parts[1].to_string()))),
        small_net: normalize_num(Some(&Value::String(parts[2].to_string()))),
        medium_net: normalize_num(Some(&Value::String(parts[3].to_string()))),
        large_net: normalize_num(Some(&Value::String(parts[4].to_string()))),
        super_large_net: normalize_num(Some(&Value::String(parts[5].to_string()))),
        main_net_inflow_rate: normalize_num(Some(&Value::String(parts[6].to_string()))),
        update_date: today(),
    })
}

/// Fetch missing per-symbol historical main capital flow via fflow API.
pub async fn backfill(start: &str, end: &str, symbols: Option<&[String]>) -> Result<PathBuf> {
    let start_dt = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").map_err(|_| {
        CollectError::InvalidDate {
            label: "start".into(),
            value: start.into(),
        }
    })?;
    let end_dt = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d").map_err(|_| {
        CollectError::InvalidDate {
            label: "end".into(),
            value: end.into(),
        }
    })?;
    if start_dt > end_dt {
        return Err(CollectError::InvertedRange {
            start: start.to_string(),
            end: end.to_string(),
        });
    }

    let symbol_list = match symbols {
        Some(s) => s.to_vec(),
        None => backfill_symbols().await?,
    };
    if symbol_list.is_empty() {
        return Err(CollectError::InvalidInput(
            "backfill: no symbols to fetch".into(),
        ));
    }

    let output_path: PathBuf = csv_dir()?.join(format!("{REPORT_NAME}_backfill.csv"));
    let mut seen: HashMap<(String, String), FlowRecord> = HashMap::new();
    let client = HttpClient::new()?;
    let headers = fflow_headers();

    for symbol in &symbol_list {
        let secid = symbol_to_secid(symbol)?;
        let mut params = HashMap::new();
        params.insert("secid".to_string(), secid);
        params.insert("fields1".to_string(), "f1,f2,f3,f7".to_string());
        params.insert(
            "fields2".to_string(),
            "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63,f64,f65".to_string(),
        );
        params.insert("klt".to_string(), "101".to_string());
        params.insert("lmt".to_string(), "0".to_string());

        let data = client
            .get_json_with_headers_and_proxy(FFLOW_DAYKLINE_URL, &params, &headers, None)
            .await?;
        let Some(rows) = data
            .get("data")
            .and_then(|d| d.get("klines"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for row in rows {
            let Some(row) = row.as_str() else {
                continue;
            };
            let Some(record) = fflow_record(symbol, row) else {
                continue;
            };
            let day = record.trade_date.clone();
            if day.as_str() < start || day.as_str() > end {
                continue;
            }
            seen.insert((symbol.clone(), day), record);
        }
    }

    if seen.is_empty() {
        return Err(CollectError::InvalidInput(format!(
            "backfill: no fflow data returned for {} symbols in {start}..{end}",
            symbol_list.len()
        )));
    }

    let mut records: Vec<FlowRecord> = seen.into_values().collect();
    records.sort_by(|a, b| {
        a.trade_date
            .cmp(&b.trade_date)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    write_csv(&output_path, &records)?;
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_prefix_matches_python() {
        assert_eq!(exchange_prefix("600519"), "SH");
        assert_eq!(exchange_prefix("000001"), "SZ");
        assert_eq!(exchange_prefix("830001"), "BJ");
    }

    #[test]
    fn normalize_num_blank_and_dash() {
        assert_eq!(normalize_num(Some(&Value::String("-".into()))), "");
        assert_eq!(normalize_num(None), "");
        assert_eq!(normalize_num(Some(&serde_json::json!(1.2))), "1.2");
    }

    #[test]
    fn trade_date_uses_max_f124_and_falls_back() {
        let diff = vec![
            serde_json::json!({"f124": 1_700_000_000u64}),
            serde_json::json!({"f124": 1_700_000_100u64}),
        ];
        let d = trade_date_from_quotes(&diff);
        assert_eq!(d, "2023-11-15");
    }

    #[test]
    fn trade_date_falls_back_to_today_without_f124() {
        let d = trade_date_from_quotes(&[serde_json::json!({})]);
        assert_eq!(d, today());
    }

    #[test]
    fn build_records_maps_push2_fields() {
        let diff = vec![serde_json::json!({
            "f12": "600519",
            "f62": "1.5",
            "f184": "-2",
            "f66": "3",
            "f72": "4",
            "f78": "5",
            "f84": "6",
            "f124": 1_700_000_000u64
        })];
        let records = build_records(&diff, "2023-11-15");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].symbol, "SH600519");
        assert_eq!(records[0].main_net_inflow, "1.5");
        assert_eq!(records[0].small_net, "6");
    }

    #[test]
    fn fflow_record_maps_fields_in_order() {
        let r = fflow_record("SH600519", "2026-07-31,1.0,2.0,3.0,4.0,5.0,6.0").unwrap();
        assert_eq!(r.trade_date, "2026-07-31");
        assert_eq!(r.main_net_inflow, "1");
        assert_eq!(r.small_net, "2");
        assert_eq!(r.medium_net, "3");
        assert_eq!(r.large_net, "4");
        assert_eq!(r.super_large_net, "5");
        assert_eq!(r.main_net_inflow_rate, "6");
    }

    #[test]
    fn fflow_record_short_row_returns_none() {
        assert!(fflow_record("SH600519", "2026-07-31,1").is_none());
    }

    #[tokio::test]
    async fn backfill_rejects_inverted_before_network() {
        let err = backfill("2026-08-28", "2026-08-27", Some(&[]))
            .await
            .unwrap_err();
        assert!(matches!(err, CollectError::InvertedRange { .. }));
    }

    #[test]
    fn symbol_to_secid_mapping() {
        assert_eq!(symbol_to_secid("SH600519").unwrap(), "1.600519");
        assert_eq!(symbol_to_secid("SZ000001").unwrap(), "0.000001");
        assert_eq!(symbol_to_secid("BJ830001").unwrap(), "0.830001");
    }
}
