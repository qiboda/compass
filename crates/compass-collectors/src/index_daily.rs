//! Index daily collector: official EastMoney indices + THS industry boards.
//!
//! Mirrors `collectors/fetch_index_daily.py`: EastMoney push2his kline with
//! Tencent fallback for official indices, THS GBK industry list and per-year
//! BK klines, incremental per-symbol windows, fast-fail after consecutive
//! failures, and dual CSV output (index_daily.csv / index_basic.csv).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::Datelike;
use regex::Regex;
use serde_json::Value;

use crate::config::csv_dir;
use crate::csv::write_csv_ordered;
use crate::dolt::import_replace_table;
use crate::eastmoney::{Record, record_get};
use crate::error::{CollectError, Result};
use crate::http::{EM_MIN_INTERVAL, HttpClient, Throttle};
use crate::progress::Progress;
use crate::proxy::{ProxyPool, make_proxy_pool};

/// Dolt target table name.
pub const DOLT_TABLE: &str = "index_daily";
/// Source label recorded in `data_updates` for this table.
pub const SOURCE: &str = "EastMoney push2his kline + Tencent fallback + THS industry kline";

const PUSH2HIS: &str = "https://push2his.eastmoney.com/api/qt/stock/kline/get";
const PUSH2HIS_MIRRORS: [&str; 2] = [
    "https://91.push2his.eastmoney.com/api/qt/stock/kline/get",
    "https://79.push2his.eastmoney.com/api/qt/stock/kline/get",
];
const KLINE_HOSTS: [&str; 3] = [PUSH2HIS, PUSH2HIS_MIRRORS[0], PUSH2HIS_MIRRORS[1]];
const TENCENT_KLINE_URL: &str = "https://web.ifzq.gtimg.cn/appstock/app/newfqkline/get";
const TENCENT_PAGE_SIZE: usize = 2000;
const TENCENT_MAX_PAGES: usize = 10;
const THS_LIST_URL: &str = "https://q.10jqka.com.cn/thshy/";
const THS_KLINE_TPL: &str = "https://d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js";
const THS_FIRST_YEAR: i32 = 2007;
const MAX_HOSTS_TRIED: usize = 2;
const MAX_ATTEMPTS: usize = 3;
const MAX_CONSECUTIVE_FAILURES: usize = 5;

const DAILY_DDL: &str = r#"CREATE TABLE IF NOT EXISTS index_daily (
    symbol      VARCHAR(20) NOT NULL,
    trade_date  DATE NOT NULL,
    index_type  VARCHAR(20) NOT NULL,
    open        DOUBLE,
    close       DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    volume      DOUBLE,
    amount      DOUBLE,
    update_date DATE,
    PRIMARY KEY (symbol, trade_date)
)"#;

const BASIC_DDL: &str = r#"CREATE TABLE IF NOT EXISTS index_basic (
    symbol      VARCHAR(20) NOT NULL PRIMARY KEY,
    name        VARCHAR(100),
    index_type  VARCHAR(20),
    name_en     VARCHAR(100)
)"#;

const DAILY_INSERT_COLS: &str =
    "symbol, trade_date, index_type, open, close, high, low, volume, amount, update_date";

const KLINE_FIELDS: [&str; 7] = [
    "trade_date",
    "open",
    "close",
    "high",
    "low",
    "volume",
    "amount",
];

const OFFICIAL_INDICES: [(&str, &str, &str); 30] = [
    ("1.000001", "000001", "上证指数"),
    ("1.000016", "000016", "上证50"),
    ("1.000010", "000010", "上证180"),
    ("1.000009", "000009", "上证380"),
    ("1.000015", "000015", "上证红利"),
    ("1.000038", "000038", "上证180金融"),
    ("1.000104", "000104", "中证全指能源"),
    ("1.000300", "000300", "沪深300"),
    ("1.000903", "000903", "中证100"),
    ("1.000905", "000905", "中证500"),
    ("1.000852", "000852", "中证1000"),
    ("1.000906", "000906", "中证800"),
    ("1.000922", "000922", "中证红利"),
    ("1.000985", "000985", "中证全指"),
    ("1.000688", "000688", "科创50"),
    ("1.000932", "000932", "中证消费"),
    ("1.000933", "000933", "中证医药"),
    ("1.000934", "000934", "中证金融"),
    ("1.000819", "000819", "有色金属"),
    ("1.000827", "000827", "中证环保"),
    ("0.399001", "399001", "深证成指"),
    ("0.399006", "399006", "创业板指"),
    ("0.399005", "399005", "中小100"),
    ("0.399106", "399106", "深证综指"),
    ("0.399107", "399107", "深证A指"),
    ("0.399108", "399108", "深证B指"),
    ("0.399330", "399330", "深证100"),
    ("0.399007", "399007", "深证300"),
    ("0.399013", "399013", "深市精选"),
    ("1.000919", "000919", "300价值"),
];

static THS_HREF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"href="[^"]*?/thshy/(?:detail/code/)?(881\d{3})/"\s*[^>]*>([^<]+)</a>"#)
        .expect("valid THS href regex")
});

fn today() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn em_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), crate::http::EM_UA.to_string());
    headers.insert("Accept".to_string(), "*/*".to_string());
    headers.insert(
        "Referer".to_string(),
        "https://quote.eastmoney.com/".to_string(),
    );
    headers
}

fn ths_headers() -> HashMap<String, String> {
    let mut headers = em_headers();
    headers.insert(
        "Referer".to_string(),
        "https://q.10jqka.com.cn/".to_string(),
    );
    headers
}

fn normalize_num(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() || v == "-" {
        return String::new();
    }
    if let Ok(n) = v.parse::<i64>() {
        return n.to_string();
    }
    if let Ok(n) = v.parse::<f64>() {
        if n.fract() == 0.0 {
            return format!("{n:.1}");
        }
        return n.to_string();
    }
    String::new()
}

fn kline_records(
    symbol: &str,
    index_type: &str,
    klines: &[String],
    today_iso: &str,
) -> Vec<Record> {
    let mut records = Vec::new();
    for line in klines {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 7 {
            continue;
        }
        let trade_date = parts[0].trim();
        if trade_date > today_iso {
            continue;
        }
        let mut record: Record = Vec::new();
        record.push(("symbol".to_string(), symbol.to_string()));
        record.push(("trade_date".to_string(), trade_date.to_string()));
        record.push(("index_type".to_string(), index_type.to_string()));
        for (i, field) in KLINE_FIELDS.iter().enumerate().skip(1) {
            record.push((field.to_string(), normalize_num(parts[i])));
        }
        record.push(("update_date".to_string(), today_iso.to_string()));
        records.push(record);
    }
    records
}

async fn max_trade_date(symbol: &str) -> Result<Option<String>> {
    let dir = crate::config::dolt_dir();
    if !dir.join(".dolt").exists() {
        return Ok(None);
    }
    let escaped = symbol.replace('\'', "''");
    let out = match crate::dolt::dolt_sql_csv(&format!(
        "SELECT DATE_FORMAT(MAX(trade_date), '%Y-%m-%d') FROM {DOLT_TABLE} WHERE symbol = '{escaped}'"
    ))
    .await
    {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };
    let lines: Vec<&str> = out.trim().lines().collect();
    if lines.len() < 2 {
        return Ok(None);
    }
    let value = lines.last().unwrap().trim();
    if value.is_empty() || value == "NULL" {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn parse_max_date(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let dt = chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()?;
    let today_dt = chrono::Local::now().date_naive();
    Some(if dt > today_dt {
        today_dt.format("%Y-%m-%d").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    })
}

async fn pool_proxy(pool: &mut Option<ProxyPool>) -> Option<String> {
    if let Some(pool) = pool.as_mut() {
        pool.get_proxy().await
    } else {
        None
    }
}

async fn get_json(
    client: &HttpClient,
    throttle: &mut Throttle,
    hosts: &[&str],
    params: &HashMap<String, String>,
    pool: &mut Option<ProxyPool>,
) -> Result<Option<Value>> {
    for base in hosts.iter().take(MAX_HOSTS_TRIED) {
        for attempt in 0..MAX_ATTEMPTS {
            let proxy = pool_proxy(pool).await;
            throttle.acquire().await;
            match client
                .get_json_with_headers_and_proxy(base, params, &em_headers(), proxy.as_deref())
                .await
            {
                Ok(data) => {
                    if data.as_object().map(|o| o.is_empty()).unwrap_or(false) {
                        eprintln!("    empty response from {base}");
                        break;
                    }
                    return Ok(Some(data));
                }
                Err(e) => {
                    let is_429 = matches!(e, CollectError::HttpStatus(429));
                    if is_429 {
                        let wait = 15.0 + rand::random::<f64>() * 5.0;
                        eprintln!("    429, waiting {wait:.0}s...");
                        tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
                        continue;
                    }
                    let wait =
                        (1u64 << attempt.min(5)).min(30) as f64 + rand::random::<f64>() * 3.0;
                    eprintln!(
                        "    retry {}/{} in {wait:.0}s: {e}",
                        attempt + 1,
                        MAX_ATTEMPTS
                    );
                    if attempt + 1 < MAX_ATTEMPTS {
                        tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
                    } else {
                        eprintln!("    FAILED {base}: {e}");
                    }
                }
            }
        }
    }
    Ok(None)
}

async fn fetch_ths_industry_list(
    client: &HttpClient,
    throttle: &mut Throttle,
    pool: &mut Option<ProxyPool>,
) -> Result<Vec<(String, String)>> {
    let mut last_exc = None;
    let mut html = String::new();
    for attempt in 0..2 {
        let proxy = pool_proxy(pool).await;
        throttle.acquire().await;
        match client
            .get_bytes_with_headers_and_proxy(
                THS_LIST_URL,
                &HashMap::new(),
                &ths_headers(),
                proxy.as_deref(),
            )
            .await
        {
            Ok(bytes) => {
                let (text, _, _) = encoding_rs::GBK.decode(&bytes);
                html = text.into_owned();
                break;
            }
            Err(e) => {
                last_exc = Some(e);
                if attempt == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        500 + (rand::random::<f64>() * 500.0) as u64,
                    ))
                    .await;
                }
            }
        }
    }
    if html.is_empty() {
        eprintln!("    FAILED ths list: {last_exc:?}");
        return Ok(Vec::new());
    }

    let mut boards = Vec::new();
    let mut seen = HashSet::new();
    for cap in THS_HREF_RE.captures_iter(&html) {
        let code = cap[1].to_string();
        let name = cap[2].trim().to_string();
        if seen.insert(code.clone()) {
            boards.push((code, name));
        }
    }
    Ok(boards)
}

fn ths_date_iso(cell: &str) -> String {
    let cell = cell.trim();
    if cell.len() == 8 && cell.chars().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &cell[0..4], &cell[4..6], &cell[6..8])
    } else {
        cell.to_string()
    }
}

async fn fetch_ths_kline(
    client: &HttpClient,
    throttle: &mut Throttle,
    code: &str,
    year: i32,
    pool: &mut Option<ProxyPool>,
) -> Result<Option<Vec<String>>> {
    let url = THS_KLINE_TPL
        .replace("{code}", code)
        .replace("{year}", &year.to_string());
    let mut body = String::new();
    let mut last_exc = None;
    for attempt in 0..2 {
        let proxy = pool_proxy(pool).await;
        throttle.acquire().await;
        match client
            .get_text_with_headers_and_proxy(
                &url,
                &HashMap::new(),
                &ths_headers(),
                proxy.as_deref(),
            )
            .await
        {
            Ok(text) => {
                body = text;
                break;
            }
            Err(e) => {
                last_exc = Some(e);
                if attempt == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        500 + (rand::random::<f64>() * 500.0) as u64,
                    ))
                    .await;
                }
            }
        }
    }
    if body.is_empty() {
        eprintln!("    FAILED ths kline {code}/{year}: {last_exc:?}");
        return Ok(None);
    }
    let start = body.find('(');
    let end = body.rfind(')');
    let (Some(start), Some(end)) = (start, end) else {
        return Ok(None);
    };
    if end <= start {
        return Ok(None);
    }
    let payload_text = &body[start + 1..end];
    let payload: Option<Value> = serde_json::from_str(payload_text).ok();
    let data: String = if let Some(obj) = payload.as_ref().and_then(Value::as_object) {
        match obj.get("data") {
            Some(Value::String(s)) => s.clone(),
            _ => return Ok(None),
        }
    } else {
        payload_text.to_string()
    };

    let mut rows = Vec::new();
    for line in data.split([';', '\n']) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 7 {
            continue;
        }
        let row = format!(
            "{},{},{},{},{},{},{}",
            ths_date_iso(parts[0]),
            parts[1],
            parts[4],
            parts[2],
            parts[3],
            parts[5],
            parts[6]
        );
        rows.push(row);
    }
    Ok(Some(rows))
}

/// Probe one official index kline through the same EastMoney path used by
/// `run()`. Returns the normalized kline rows (raw 7-field CSV lines).
pub async fn probe_official(secid: &str, last_date: Option<&str>) -> Result<(Vec<String>, String)> {
    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut pool = make_proxy_pool();
    match fetch_kline(&client, &mut throttle, secid, last_date, &mut pool).await? {
        Some(v) => Ok(v),
        None => Err(CollectError::InvalidInput(format!(
            "index_daily probe failed for {secid}"
        ))),
    }
}

/// Probe the THS industry list parser against the live GBK page.
pub async fn probe_ths_industries() -> Result<Vec<(String, String)>> {
    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut pool = make_proxy_pool();
    fetch_ths_industry_list(&client, &mut throttle, &mut pool).await
}

async fn fetch_kline(
    client: &HttpClient,
    throttle: &mut Throttle,
    secid: &str,
    last_date: Option<&str>,
    pool: &mut Option<ProxyPool>,
) -> Result<Option<(Vec<String>, String)>> {
    let beg = if let Some(last_date) = last_date {
        let dt = chrono::NaiveDate::parse_from_str(last_date, "%Y-%m-%d").map_err(|_| {
            CollectError::InvalidDate {
                label: "last_date".into(),
                value: last_date.into(),
            }
        })?;
        let next = dt.succ_opt().ok_or_else(|| CollectError::InvalidDate {
            label: "last_date".into(),
            value: last_date.into(),
        })?;
        next.format("%Y%m%d").to_string()
    } else {
        "0".to_string()
    };
    let mut params = HashMap::new();
    params.insert("secid".to_string(), secid.to_string());
    params.insert("klt".to_string(), "101".to_string());
    params.insert("fqt".to_string(), "0".to_string());
    params.insert("beg".to_string(), beg);
    params.insert("end".to_string(), "20500000".to_string());
    params.insert("lmt".to_string(), "1000000".to_string());
    params.insert("fields1".to_string(), "f1,f2,f3,f4,f5,f6".to_string());
    params.insert(
        "fields2".to_string(),
        "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61".to_string(),
    );
    let data = get_json(client, throttle, &KLINE_HOSTS, &params, pool).await?;
    let Some(data) = data else {
        return Ok(None);
    };
    let payload = data.get("data").cloned().unwrap_or(Value::Null);
    let klines = payload
        .get("klines")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(Some((klines, code)))
}

fn tencent_code(secid: &str) -> Result<String> {
    let Some((market, code)) = secid.split_once('.') else {
        return Err(CollectError::InvalidInput(format!(
            "invalid EastMoney secid: {secid:?}"
        )));
    };
    if market != "1" && market != "0" {
        return Err(CollectError::InvalidInput(format!(
            "invalid EastMoney market prefix: {secid:?}"
        )));
    }
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err(CollectError::InvalidInput(format!(
            "invalid EastMoney code: {secid:?}"
        )));
    }
    Ok(format!(
        "{}{}",
        if market == "1" { "sh" } else { "sz" },
        code.to_lowercase()
    ))
}

fn tencent_amount_yuan(row: &[Value]) -> String {
    let raw = row
        .get(8)
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() || raw == "-" {
        return "0".to_string();
    }
    let Ok(amount_wan) = raw.parse::<f64>() else {
        return "0".to_string();
    };
    if !amount_wan.is_finite() {
        return "0".to_string();
    }
    let yuan = amount_wan * 10000.0;
    if !yuan.is_finite() || yuan < 0.0 {
        return "0".to_string();
    }
    if yuan.fract() == 0.0 {
        format!("{}", yuan as i64)
    } else {
        yuan.to_string()
    }
}

async fn fetch_tencent_kline(
    client: &HttpClient,
    throttle: &mut Throttle,
    secid: &str,
    last_date: Option<&str>,
    pool: &mut Option<ProxyPool>,
) -> Result<Option<Vec<String>>> {
    let tcode = match tencent_code(secid) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("    invalid Tencent code: {e}");
            return Ok(None);
        }
    };
    let mut pages: Vec<Vec<String>> = Vec::new();
    let mut end_date = String::new();
    let mut previous_min: Option<String> = None;

    for _ in 0..TENCENT_MAX_PAGES {
        let param = format!("{tcode},day,,{end_date},{TENCENT_PAGE_SIZE},qfq");
        let mut params = HashMap::new();
        params.insert("param".to_string(), param);
        let data = get_json(client, throttle, &[TENCENT_KLINE_URL], &params, pool).await?;
        let Some(data) = data else {
            return Ok(None);
        };
        let data_section = data.get("data");
        let payload = data_section.and_then(|d| d.get(&tcode));
        let Some(rows) = payload.and_then(|p| p.get("day")).and_then(Value::as_array) else {
            return Ok(None);
        };

        let mut page_klines = Vec::new();
        let mut min_date: Option<String> = None;
        let mut boundary_hit = false;
        let mut valid_row_count = 0usize;

        for row in rows {
            let Some(row) = row.as_array() else {
                continue;
            };
            if row.len() < 6 {
                continue;
            }
            let cells: Vec<String> = row
                .iter()
                .take(6)
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                })
                .collect();
            let date_cell = cells[0].trim().to_string();
            if date_cell.is_empty() {
                continue;
            }
            valid_row_count += 1;
            if let Some(last) = last_date
                && date_cell.as_str() <= last
            {
                boundary_hit = true;
                continue;
            }
            if min_date.as_deref().is_none_or(|m| date_cell.as_str() < m) {
                min_date = Some(date_cell.clone());
            }
            let amount = tencent_amount_yuan(row.as_slice());
            page_klines.push(format!("{},{amount}", cells.join(",")));
        }

        if !rows.is_empty() && valid_row_count == 0 {
            return Ok(None);
        }
        if boundary_hit {
            pages.push(page_klines);
            break;
        }
        if page_klines.is_empty() {
            break;
        }
        pages.push(page_klines);
        if rows.len() < TENCENT_PAGE_SIZE {
            break;
        }
        let Some(min_date) = min_date else {
            break;
        };
        let next_end = match chrono::NaiveDate::parse_from_str(&min_date, "%Y-%m-%d") {
            Ok(d) => d
                .pred_opt()
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            Err(_) => break,
        };
        if previous_min
            .as_deref()
            .is_some_and(|m| min_date.as_str() >= m)
        {
            break;
        }
        previous_min = Some(min_date);
        end_date = next_end;
    }

    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for page in pages.iter().rev() {
        for kline in page {
            let trade_date = kline.split(',').next().unwrap_or("").to_string();
            if seen.insert(trade_date.clone()) {
                merged.push(kline.clone());
            }
        }
    }
    Ok(Some(merged))
}

fn abort_reason(count: usize) -> String {
    format!("连续 {count} 个标的失败（疑似反爬或接口故障），终止采集")
}

fn bump_failure(consecutive: usize) -> (usize, Option<String>) {
    let count = consecutive + 1;
    if count >= MAX_CONSECUTIVE_FAILURES {
        (count, Some(abort_reason(count)))
    } else {
        (count, None)
    }
}

fn persist_outputs(
    daily_records: &[Record],
    basic_records: &[Record],
    daily_path: &Path,
    basic_path: &Path,
) -> Result<()> {
    if !daily_records.is_empty() {
        write_csv_ordered(daily_path, daily_records)?;
    }
    if !basic_records.is_empty() {
        write_csv_ordered(basic_path, basic_records)?;
    }
    if daily_records.is_empty() && basic_records.is_empty() {
        let _ = std::fs::remove_file(daily_path);
        let _ = std::fs::remove_file(basic_path);
    }
    Ok(())
}

/// Fetch official indices + THS industry boards into the two CSVs.
pub async fn run() -> Result<PathBuf> {
    let daily_path: PathBuf = csv_dir()?.join("index_daily.csv");
    let basic_path: PathBuf = csv_dir()?.join("index_basic.csv");
    let today_iso = today();

    let last = crate::dolt::last_report_date(DOLT_TABLE).await?;
    if last.as_deref() == Some(today_iso.as_str()) {
        eprintln!("Data up to date ({today_iso}); skipping fetch");
        return Ok(daily_path);
    }

    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut pool = make_proxy_pool();
    let mut progress = Progress::new("index_daily", None, Some(daily_path.clone()), "start")?;
    let mut daily_records: Vec<Record> = Vec::new();
    let mut basic_records: Vec<Record> = Vec::new();
    let mut consecutive_failures = 0usize;
    let mut abort_reason: Option<String> = None;

    let industries = fetch_ths_industry_list(&client, &mut throttle, &mut pool).await?;
    eprintln!("THS industries: {}", industries.len());
    if industries.is_empty() {
        eprintln!(
            "WARNING: THS 行业列表为空（抓取失败或页面结构变化）——本次仅采集官方指数，90 个行业未采"
        );
    }
    let _ = progress.update(
        None,
        None,
        None,
        Some(format!(
            "Fetching THS industry and index klines (total {})",
            industries.len() + OFFICIAL_INDICES.len()
        )),
        Some((industries.len() + OFFICIAL_INDICES.len()) as u64),
    );

    // THS industries
    for (i, (code, name)) in industries.iter().enumerate() {
        if abort_reason.is_some() {
            break;
        }
        let symbol = format!("BK{code}");
        basic_records.push(vec![
            ("symbol".to_string(), symbol.clone()),
            ("name".to_string(), name.clone()),
            ("index_type".to_string(), "industry".to_string()),
        ]);
        eprintln!("  [industry] {symbol} {name} ...");

        let max_raw = max_trade_date(&symbol).await?;
        let max_dt = parse_max_date(max_raw.as_deref());
        if max_dt.as_deref() == Some(today_iso.as_str()) {
            consecutive_failures = 0;
            eprintln!("up to date");
            continue;
        }

        let start_year = if let Some(max_dt) = max_dt.as_deref() {
            let dt = chrono::NaiveDate::parse_from_str(max_dt, "%Y-%m-%d").unwrap();
            let current_year = chrono::Local::now().date_naive().year();
            if dt.month() == 12 && dt.day() == 31 {
                (dt.year() + 1).min(current_year).max(THS_FIRST_YEAR)
            } else {
                dt.year().max(THS_FIRST_YEAR)
            }
        } else {
            THS_FIRST_YEAR
        };
        let max_iso = max_dt.clone();
        let current_year = chrono::Local::now().date_naive().year();
        let mut klines: Vec<String> = Vec::new();
        let mut saw_response = false;
        let mut fetch_failed = false;

        for year in (start_year..=current_year).rev() {
            let year_rows = fetch_ths_kline(&client, &mut throttle, code, year, &mut pool).await?;
            match year_rows {
                None => {
                    fetch_failed = true;
                    eprintln!("    year {year} fetch failed (kept going)");
                }
                Some(rows) => {
                    saw_response = true;
                    if rows.is_empty() {
                        if max_iso.is_none() {
                            break;
                        }
                        continue;
                    }
                    if let Some(max_iso) = max_iso.as_deref() {
                        let kept: Vec<String> = rows
                            .into_iter()
                            .filter(|row| {
                                row.split(',').next().unwrap_or("").to_string().as_str() > max_iso
                            })
                            .collect();
                        if kept.is_empty() {
                            continue;
                        }
                        klines.extend(kept);
                    } else {
                        klines.extend(rows);
                    }
                }
            }
        }

        if !klines.is_empty() && max_dt.is_some() && fetch_failed {
            let (count, reason) = bump_failure(consecutive_failures);
            consecutive_failures = count;
            abort_reason = reason;
            eprintln!("FAILED (partial year failure, rows discarded)");
        } else if klines.is_empty() {
            if max_dt.is_some() && saw_response && !fetch_failed {
                consecutive_failures = 0;
                eprintln!("no new bars");
            } else {
                let (count, reason) = bump_failure(consecutive_failures);
                consecutive_failures = count;
                abort_reason = reason;
                eprintln!("FAILED (no klines)");
            }
        } else {
            daily_records.extend(kline_records(&symbol, "industry", &klines, &today_iso));
            eprintln!("{} bars", klines.len());
            consecutive_failures = 0;
        }
        let _ = progress.update(
            Some((i + 1) as u64 + OFFICIAL_INDICES.len() as u64),
            Some(daily_records.len() as u64),
            Some(symbol.clone()),
            Some(format!("Fetched industry {symbol} {name}")),
            None,
        );
        if abort_reason.is_some() {
            break;
        }
    }

    // Official indices
    for (j, (secid, code, name)) in OFFICIAL_INDICES.iter().enumerate() {
        if abort_reason.is_some() {
            break;
        }
        let symbol = if secid.starts_with("1.") {
            format!("SH{code}")
        } else {
            format!("SZ{code}")
        };
        eprintln!("  [official] {secid} {name} ...");
        let max_raw = max_trade_date(&symbol).await?;
        let max_dt = parse_max_date(max_raw.as_deref());
        if max_dt.as_deref() == Some(today_iso.as_str()) {
            consecutive_failures = 0;
            basic_records.push(vec![
                ("symbol".to_string(), symbol.clone()),
                ("name".to_string(), name.to_string()),
                ("index_type".to_string(), "official".to_string()),
            ]);
            eprintln!("up to date");
            continue;
        }

        let last_date = max_dt.clone();
        let result = fetch_kline(
            &client,
            &mut throttle,
            secid,
            last_date.as_deref(),
            &mut pool,
        )
        .await?;
        if result.as_ref().map(|(k, _)| k.is_empty()).unwrap_or(true) || result.is_none() {
            let source_label = if result.is_none() {
                "FAILED"
            } else {
                "empty (skipped)"
            };
            eprintln!("{source_label} (eastmoney); trying tencent...");
            let tencent_klines = fetch_tencent_kline(
                &client,
                &mut throttle,
                secid,
                last_date.as_deref(),
                &mut pool,
            )
            .await?;
            if last_date.is_some() {
                if let Some(klines) = tencent_klines {
                    consecutive_failures = 0;
                    basic_records.push(vec![
                        ("symbol".to_string(), symbol.clone()),
                        ("name".to_string(), name.to_string()),
                        ("index_type".to_string(), "official".to_string()),
                    ]);
                    daily_records.extend(kline_records(&symbol, "official", &klines, &today_iso));
                    eprintln!("{} bars (tencent)", klines.len());
                } else {
                    let (count, reason) = bump_failure(consecutive_failures);
                    consecutive_failures = count;
                    abort_reason = reason;
                    eprintln!("FAILED (eastmoney+tencent)");
                }
            } else if let Some(klines) = tencent_klines
                && !klines.is_empty()
            {
                consecutive_failures = 0;
                basic_records.push(vec![
                    ("symbol".to_string(), symbol.clone()),
                    ("name".to_string(), name.to_string()),
                    ("index_type".to_string(), "official".to_string()),
                ]);
                daily_records.extend(kline_records(&symbol, "official", &klines, &today_iso));
                eprintln!("{} bars (tencent)", klines.len());
            } else {
                let (count, reason) = bump_failure(consecutive_failures);
                consecutive_failures = count;
                abort_reason = reason;
                eprintln!("FAILED (eastmoney+tencent)");
            }
        } else if let Some((klines, echoed_code)) = result {
            if echoed_code != *code && echoed_code != symbol {
                eprintln!("code mismatch ({echoed_code:?}), skipped");
            } else {
                consecutive_failures = 0;
                basic_records.push(vec![
                    ("symbol".to_string(), symbol.clone()),
                    ("name".to_string(), name.to_string()),
                    ("index_type".to_string(), "official".to_string()),
                ]);
                daily_records.extend(kline_records(&symbol, "official", &klines, &today_iso));
                eprintln!("{} bars", klines.len());
            }
        }
        let _ = progress.update(
            Some((j + 1) as u64),
            Some(daily_records.len() as u64),
            Some(name.to_string()),
            Some(format!("Fetched official {name}")),
            None,
        );
        if abort_reason.is_some() {
            break;
        }
    }

    if let Some(reason) = abort_reason {
        persist_outputs(&daily_records, &basic_records, &daily_path, &basic_path)?;
        let _ = progress.fail(&reason, "failed");
        return Err(CollectError::InvalidInput(reason));
    }
    if daily_records.is_empty() && basic_records.is_empty() {
        persist_outputs(&daily_records, &basic_records, &daily_path, &basic_path)?;
        let _ = progress.fail("No index data", "failed");
        return Err(CollectError::InvalidInput(
            "No index data (rate-limited or empty) — aborting, no CSV written".into(),
        ));
    }
    persist_outputs(&daily_records, &basic_records, &daily_path, &basic_path)?;
    let _ = progress.finish(Some(daily_records.len() as u64), "Done");
    eprintln!(
        "Done: {} daily rows, {} basic rows → {}, {}",
        daily_records.len(),
        basic_records.len(),
        daily_path.display(),
        basic_path.display()
    );
    Ok(daily_path)
}

fn csv_has_valid_dates(path: &Path) -> bool {
    let Ok(mut reader) = csv::ReaderBuilder::new().from_path(path) else {
        return false;
    };
    for result in reader.records() {
        let Ok(record) = result else {
            return false;
        };
        let Some(idx) = record.iter().position(|h| h == "trade_date") else {
            continue;
        };
        let cell = record.get(idx).unwrap_or("").trim();
        if cell.is_empty() {
            continue;
        }
        if chrono::NaiveDate::parse_from_str(cell, "%Y-%m-%d").is_err() {
            return false;
        }
    }
    true
}

/// Import `index_daily.csv` into Dolt (merge mode).
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join("index_daily.csv"),
    };
    if !path.exists() {
        return Err(CollectError::InvalidInput(format!(
            "{} not found. Run fetch first.",
            path.display()
        )));
    }
    if !csv_has_valid_dates(&path) {
        return Err(CollectError::InvalidInput(
            "invalid trade_date in CSV — import refused".into(),
        ));
    }
    let insert_sql = format!(
        "INSERT IGNORE INTO {DOLT_TABLE} ({DAILY_INSERT_COLS}) SELECT {DAILY_INSERT_COLS} FROM _tmp_ixd"
    );
    import_replace_table(
        &path,
        "_tmp_ixd",
        DAILY_DDL,
        &insert_sql,
        DOLT_TABLE,
        SOURCE,
        "MAX(trade_date)",
        None,
        true,
    )
    .await
}

/// Import `index_basic.csv` into Dolt (merge mode, with optional name_en mapping).
pub async fn import_index_basic(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join("index_basic.csv"),
    };
    let mapping = crate::dolt::load_name_en_mapping().await?;
    let (insert_cols, select_cols, joins) = if mapping {
        (
            "(symbol, name, index_type, name_en)",
            "t.symbol, t.name, t.index_type, COALESCE(m1.value, m2.value)",
            "LEFT JOIN _tmp_name_en m1 ON m1.section = 'index' AND m1.`key` = t.symbol \
             LEFT JOIN _tmp_name_en m2 ON m2.section = 'industry' AND m2.`key` = t.name",
        )
    } else {
        (
            "(symbol, name, index_type)",
            "t.symbol, t.name, t.index_type",
            "",
        )
    };
    let insert_sql = format!(
        "INSERT IGNORE INTO index_basic {insert_cols} SELECT {select_cols} FROM _tmp_ixb t {joins}"
    );
    let total = import_replace_table(
        &path,
        "_tmp_ixb",
        BASIC_DDL,
        &insert_sql,
        "index_basic",
        SOURCE,
        "CURDATE()",
        None,
        true,
    )
    .await?;
    let _ = crate::dolt::dolt_sql("DROP TABLE IF EXISTS _tmp_name_en").await;
    Ok(total)
}

/// Backfill explicit `[start, end]` index klines (middle-gap auto-heal).
pub async fn backfill(start: &str, end: &str) -> Result<PathBuf> {
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

    let output_path: PathBuf = csv_dir()?.join("index_daily_backfill.csv");
    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let mut pool = make_proxy_pool();
    let mut records: Vec<Record> = Vec::new();
    let today_iso = today();

    let industries = fetch_ths_industry_list(&client, &mut throttle, &mut pool).await?;
    if industries.is_empty() {
        return Err(CollectError::InvalidInput(
            "index_daily backfill: THS industry list is empty, refusing to leave industry gaps unhealed"
                .into(),
        ));
    }
    for (code, _name) in &industries {
        let symbol = format!("BK{code}");
        for year in start_dt.year()..=end_dt.year() {
            let klines = fetch_ths_kline(&client, &mut throttle, code, year, &mut pool).await?;
            let Some(klines) = klines else {
                return Err(CollectError::InvalidInput(format!(
                    "index_daily backfill failed for THS {symbol} year {year}"
                )));
            };
            records.extend(kline_records(&symbol, "industry", &klines, &today_iso));
        }
    }

    for (secid, code, _name) in OFFICIAL_INDICES {
        let symbol = if secid.starts_with("1.") {
            format!("SH{code}")
        } else {
            format!("SZ{code}")
        };
        let result = fetch_kline(&client, &mut throttle, secid, None, &mut pool).await?;
        let Some((klines, _code)) = result else {
            return Err(CollectError::InvalidInput(format!(
                "index_daily backfill failed for official {symbol}"
            )));
        };
        records.extend(kline_records(&symbol, "official", &klines, &today_iso));
    }

    let mut seen: HashMap<(String, String), Record> = HashMap::new();
    for record in records {
        let day = record_get(&record, "trade_date").unwrap_or("").to_string();
        if day.as_str() < start || day.as_str() > end {
            continue;
        }
        let key = (record_get(&record, "symbol").unwrap_or("").to_string(), day);
        seen.entry(key).or_insert(record);
    }
    if seen.is_empty() {
        let _ = std::fs::remove_file(&output_path);
        return Ok(output_path);
    }
    let mut out: Vec<Record> = seen.into_values().collect();
    out.sort_by(|a, b| {
        record_get(a, "trade_date")
            .unwrap_or("")
            .cmp(record_get(b, "trade_date").unwrap_or(""))
            .then_with(|| {
                record_get(a, "symbol")
                    .unwrap_or("")
                    .cmp(record_get(b, "symbol").unwrap_or(""))
            })
    });
    write_csv_ordered(&output_path, &out)?;
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_num_matches_python() {
        assert_eq!(normalize_num("0"), "0");
        assert_eq!(normalize_num("2999.0"), "2999.0");
        assert_eq!(normalize_num("-"), "");
        assert_eq!(normalize_num(""), "");
    }

    #[test]
    fn ths_date_iso_formats_compact() {
        assert_eq!(ths_date_iso("20260105"), "2026-01-05");
        assert_eq!(ths_date_iso("2026-01-05"), "2026-01-05");
    }

    #[test]
    fn kline_records_drops_future_and_builds_fields() {
        let records = kline_records(
            "BK881101",
            "industry",
            &[
                "2026-08-27,1,2,3,4,5,6".to_string(),
                "2099-01-01,1,2,3,4,5,6".to_string(),
            ],
            "2026-08-28",
        );
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(record_get(rec, "symbol"), Some("BK881101"));
        assert_eq!(record_get(rec, "open"), Some("1"));
        assert_eq!(record_get(rec, "close"), Some("2"));
        assert_eq!(record_get(rec, "amount"), Some("6"));
    }

    #[test]
    fn tencent_code_maps_secid() {
        assert_eq!(tencent_code("1.000001").unwrap(), "sh000001");
        assert_eq!(tencent_code("0.399001").unwrap(), "sz399001");
        assert!(tencent_code("bad").is_err());
    }

    #[test]
    fn tencent_amount_converts_wan_to_yuan() {
        assert_eq!(
            tencent_amount_yuan(
                serde_json::json!(["2026-08-27", "1", "2", "3", "4", "5", "6", "7", "12.5"])
                    .as_array()
                    .unwrap()
            ),
            "125000"
        );
        assert_eq!(
            tencent_amount_yuan(
                serde_json::json!(["d", "1", "2", "3", "4", "5", "6", "7", "-"])
                    .as_array()
                    .unwrap()
            ),
            "0"
        );
    }
}
