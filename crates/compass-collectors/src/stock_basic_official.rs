//! Official exchange stock-basic collector: SSE JSON + SZSE XLSX + BSE JSONP.
//!
//! Mirrors `collectors/fetch_stock_basic_official.py`: fetches A-share member
//! data directly from the three exchange official APIs, normalizes all three
//! sources to the 12-column `stock_basic` schema and writes one CSV.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::config::csv_dir;
use crate::dolt::{dolt_sql, dolt_sql_csv, dolt_table_import, load_name_en_mapping};
use crate::error::{CollectError, Result};
use crate::http::HttpClient;
use crate::proxy::{ProxyPool, make_proxy_pool};

pub const COLUMNS: [&str; 12] = [
    "symbol",
    "ts_code",
    "code",
    "name",
    "list_date",
    "delist_date",
    "board",
    "full_name",
    "total_share",
    "industry",
    "region",
    "update_date",
];

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";
const SSE_URL: &str = "https://query.sse.com.cn/sseQuery/commonQuery.do";
const SZSE_XLSX_URL: &str = "https://www.szse.cn/api/report/ShowReport";
const BSE_LISTED_URL: &str = "https://www.bse.cn/nq/listedcompany.html";
const BSE_API_URL: &str = "https://www.bse.cn/nqxxController/nqxxCnzq.do";
const MAX_RETRIES: usize = 3;

static ROW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<row[^>]*>(.*?)</row>").expect("valid row regex"));
static INLINE_STR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)<c\s+r="([A-Z]+)\d+"[^>]*t="inlineStr"[^>]*>\s*<is><t[^>]*>(.*?)</t></is>\s*</c>"#,
    )
    .expect("valid inline string regex")
});
static NUMBER_CELL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<c\s+r="([A-Z]+)\d+"[^>]*>\s*<v>([^<]*)</v>\s*</c>"#)
        .expect("valid number cell regex")
});

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OfficialRecord {
    pub symbol: String,
    pub ts_code: String,
    pub code: String,
    pub name: String,
    pub list_date: String,
    pub delist_date: String,
    pub board: String,
    pub full_name: String,
    pub total_share: String,
    pub industry: String,
    pub region: String,
    pub update_date: String,
}

fn today() -> String {
    chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn fmt_date(yyyymmdd: &str) -> String {
    let s = yyyymmdd.trim();
    if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8])
    } else {
        String::new()
    }
}

fn float_to_python_string(raw: &str) -> String {
    let s = raw.trim().replace(',', "");
    if s.is_empty() {
        return String::new();
    }
    let Ok(n) = s.parse::<f64>() else {
        return String::new();
    };
    if n.fract() == 0.0 {
        format!("{n:.1}")
    } else {
        n.to_string()
    }
}

fn json_scalar_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

pub fn infer_exchange(code: &str) -> &'static str {
    if code.starts_with('6') {
        "SH"
    } else if code.starts_with('4') || code.starts_with('8') || code.starts_with('9') {
        "BJ"
    } else {
        "SZ"
    }
}

fn to_symbol(code: &str) -> String {
    format!("{}{}", infer_exchange(code), code)
}

fn to_ts_code(code: &str) -> String {
    format!("{}.{}", code, infer_exchange(code))
}

/// Parse the SSE JSON response (pageHelp.data[]).
pub fn parse_sse_json(data: &Value, update_date: &str) -> Vec<OfficialRecord> {
    let Some(rows) = data
        .get("pageHelp")
        .and_then(|p| p.get("data"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for row in rows {
        let stock_type = row.get("STOCK_TYPE").and_then(Value::as_str).unwrap_or("");
        if stock_type != "1" && stock_type != "8" {
            continue;
        }
        let code = row
            .get("A_STOCK_CODE")
            .and_then(Value::as_str)
            .unwrap_or("");
        if code.is_empty() {
            continue;
        }
        let list_raw = row.get("LIST_DATE").and_then(Value::as_str).unwrap_or("");
        let delist_raw = row
            .get("DELIST_DATE")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let list_date = fmt_date(list_raw);
        let delist_date = if delist_raw == "-" {
            String::new()
        } else {
            fmt_date(delist_raw)
        };
        let board = if row.get("LIST_BOARD").and_then(Value::as_str) == Some("2") {
            "科创板"
        } else {
            "主板"
        };
        let full_name = row
            .get("FULL_NAME")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| row.get("SEC_NAME_FULL").and_then(Value::as_str))
            .unwrap_or("")
            .to_string();

        records.push(OfficialRecord {
            symbol: to_symbol(code),
            ts_code: to_ts_code(code),
            code: code.to_string(),
            name: row
                .get("COMPANY_ABBR")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            list_date,
            delist_date,
            board: board.to_string(),
            full_name,
            total_share: String::new(),
            industry: row
                .get("CSRC_CODE_DESC")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            region: row
                .get("AREA_NAME_DESC")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            update_date: update_date.to_string(),
        });
    }
    records
}

fn parse_xlsx_cells(row_xml: &str) -> HashMap<String, String> {
    let mut cells = HashMap::new();
    for cap in INLINE_STR_RE.captures_iter(row_xml) {
        cells.insert(cap[1].to_string(), cap[2].to_string());
    }
    for cap in NUMBER_CELL_RE.captures_iter(row_xml) {
        let col = cap[1].to_string();
        cells.entry(col).or_insert_with(|| cap[2].to_string());
    }
    cells
}

/// Parse the SZSE active-stock XLSX sheet (CATALOGID=1110).
pub fn parse_szse_xlsx(sheet_xml: &str, update_date: &str) -> Vec<OfficialRecord> {
    let rows: Vec<String> = ROW_RE
        .captures_iter(sheet_xml)
        .map(|c| c[1].to_string())
        .collect();
    let mut records = Vec::new();
    for row_xml in rows.iter().skip(1) {
        let cells = parse_xlsx_cells(row_xml);
        let code = cells
            .get("E")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        let total_share = float_to_python_string(cells.get("H").map(String::as_str).unwrap_or(""));
        records.push(OfficialRecord {
            symbol: to_symbol(&code),
            ts_code: to_ts_code(&code),
            code,
            name: cells.get("F").cloned().unwrap_or_default(),
            list_date: cells.get("G").cloned().unwrap_or_default(),
            delist_date: String::new(),
            board: cells.get("A").cloned().unwrap_or_default(),
            full_name: cells.get("B").cloned().unwrap_or_default(),
            total_share,
            industry: cells.get("R").cloned().unwrap_or_default(),
            region: cells.get("P").cloned().unwrap_or_default(),
            update_date: update_date.to_string(),
        });
    }
    records
}

/// Parse the SZSE delisted-stock XLSX sheet (CATALOGID=1793_ssgs).
pub fn parse_szse_delisted(sheet_xml: &str, update_date: &str) -> Vec<OfficialRecord> {
    let rows: Vec<String> = ROW_RE
        .captures_iter(sheet_xml)
        .map(|c| c[1].to_string())
        .collect();
    let mut records = Vec::new();
    for row_xml in rows.iter().skip(1) {
        let cells = parse_xlsx_cells(row_xml);
        let code = cells
            .get("A")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        records.push(OfficialRecord {
            symbol: to_symbol(&code),
            ts_code: to_ts_code(&code),
            code,
            name: cells.get("B").cloned().unwrap_or_default(),
            list_date: cells.get("C").cloned().unwrap_or_default(),
            delist_date: cells.get("D").cloned().unwrap_or_default(),
            board: String::new(),
            full_name: String::new(),
            total_share: String::new(),
            industry: String::new(),
            region: String::new(),
            update_date: update_date.to_string(),
        });
    }
    records
}

/// Parse the BSE JSONP response body (`null([{...}])`).
pub fn parse_bse_json(body: &str, update_date: &str) -> Vec<OfficialRecord> {
    if !body.starts_with("null(") || !body.ends_with(")") {
        return Vec::new();
    }
    let inner = &body[5..body.len() - 1];
    let Ok(data) = serde_json::from_str::<Value>(inner) else {
        return Vec::new();
    };
    let Some(arr) = data.as_array() else {
        return Vec::new();
    };
    let Some(meta) = arr.first() else {
        return Vec::new();
    };
    let Some(content) = meta.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for row in content {
        let code = row.get("xxzqdm").and_then(Value::as_str).unwrap_or("");
        if code.is_empty() {
            continue;
        }
        let total_share = match row.get("xxzgb") {
            Some(v) if !v.is_null() => float_to_python_string(&json_scalar_to_string(Some(v))),
            _ => String::new(),
        };
        records.push(OfficialRecord {
            symbol: to_symbol(code),
            ts_code: to_ts_code(code),
            code: code.to_string(),
            name: row
                .get("xxzqjc")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            list_date: fmt_date(row.get("fxssrq").and_then(Value::as_str).unwrap_or("")),
            delist_date: String::new(),
            board: "北交所".to_string(),
            full_name: String::new(),
            total_share,
            industry: row
                .get("xxhyzl")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            region: row
                .get("xxssdq")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            update_date: update_date.to_string(),
        });
    }
    records
}

/// Merge exchange record lists, de-duplicating by code (first occurrence wins),
/// sorted ascending by code.
pub fn merge_exchanges(record_lists: &[Vec<OfficialRecord>]) -> Vec<OfficialRecord> {
    let mut seen: HashMap<String, OfficialRecord> = HashMap::new();
    for records in record_lists {
        for r in records {
            if !r.code.is_empty() && !seen.contains_key(&r.code) {
                seen.insert(r.code.clone(), r.clone());
            }
        }
    }
    let mut out: Vec<OfficialRecord> = seen.into_values().collect();
    out.sort_by(|a, b| a.code.cmp(&b.code));
    out
}

/// Write the merged records to CSV with the exact 12-column order.
pub fn records_to_csv(records: &[OfficialRecord], path: &Path) -> Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all("\u{feff}".as_bytes())?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(file);
    let headers: Vec<&str> = COLUMNS.to_vec();
    writer.write_record(&headers)?;
    for r in records {
        writer.write_record([
            r.symbol.as_str(),
            r.ts_code.as_str(),
            r.code.as_str(),
            r.name.as_str(),
            r.list_date.as_str(),
            r.delist_date.as_str(),
            r.board.as_str(),
            r.full_name.as_str(),
            r.total_share.as_str(),
            r.industry.as_str(),
            r.region.as_str(),
            r.update_date.as_str(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

async fn pool_proxy(pool: &mut Option<ProxyPool>) -> Option<String> {
    if let Some(pool) = pool.as_mut() {
        pool.get_proxy().await
    } else {
        None
    }
}

async fn fetch_sse(client: &HttpClient) -> Result<Value> {
    let mut pool = make_proxy_pool();
    let mut params = HashMap::new();
    params.insert(
        "sqlId".to_string(),
        "COMMON_SSE_CP_GPJCTPZ_GPLB_GP_L".to_string(),
    );
    params.insert("type".to_string(), "inParams".to_string());
    params.insert("STYPE".to_string(), "3".to_string());
    params.insert("isPagination".to_string(), "true".to_string());
    params.insert("pageHelp.pageSize".to_string(), "3000".to_string());
    params.insert("pageHelp.pageNo".to_string(), "1".to_string());
    let mut request = client
        .client()
        .get(SSE_URL)
        .query(&params)
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://www.sse.com.cn/");
    if let Some(proxy_url) = pool_proxy(&mut pool).await {
        let p = wreq::Proxy::all(crate::proxy::ProxyPool::proxy_spec(&proxy_url)).map_err(|e| {
            crate::error::CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
        })?;
        request = request.proxy(p);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(crate::error::CollectError::HttpStatus(
            response.status().as_u16(),
        ));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(&text)?)
}

async fn fetch_szse_xlsx(client: &HttpClient, catalogid: &str, tabkey: &str) -> Result<String> {
    let mut pool = make_proxy_pool();
    let mut params = HashMap::new();
    params.insert("SHOWTYPE".to_string(), "xlsx".to_string());
    params.insert("CATALOGID".to_string(), catalogid.to_string());
    params.insert("TABKEY".to_string(), tabkey.to_string());
    params.insert("random".to_string(), "0.1".to_string());
    let mut request = client
        .client()
        .get(SZSE_XLSX_URL)
        .query(&params)
        .header("User-Agent", USER_AGENT)
        .header("Referer", "https://www.szse.cn/");
    if let Some(proxy_url) = pool_proxy(&mut pool).await {
        let p = wreq::Proxy::all(crate::proxy::ProxyPool::proxy_spec(&proxy_url)).map_err(|e| {
            crate::error::CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
        })?;
        request = request.proxy(p);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(crate::error::CollectError::HttpStatus(
            response.status().as_u16(),
        ));
    }
    let bytes = response.bytes().await?;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;
    let mut file = archive.by_name("xl/worksheets/sheet1.xml")?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

async fn fetch_bse(client: &HttpClient) -> Result<Vec<Value>> {
    let mut pool = make_proxy_pool();
    let mut warmup = client
        .client()
        .get(BSE_LISTED_URL)
        .header("User-Agent", USER_AGENT);
    if let Some(proxy_url) = pool_proxy(&mut pool).await {
        let p = wreq::Proxy::all(crate::proxy::ProxyPool::proxy_spec(&proxy_url)).map_err(|e| {
            crate::error::CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
        })?;
        warmup = warmup.proxy(p);
    }
    let _ = warmup.send().await;

    let mut all_rows = Vec::new();
    let mut page = 0usize;
    loop {
        let mut params = HashMap::new();
        params.insert("page".to_string(), page.to_string());
        params.insert("typejb".to_string(), "T".to_string());
        params.insert("xxfcbj[]".to_string(), "2".to_string());
        params.insert("xxzqdm".to_string(), String::new());
        params.insert("sortfield".to_string(), "xxzqdm".to_string());
        params.insert("sorttype".to_string(), "asc".to_string());
        let mut request = client
            .client()
            .post(BSE_API_URL)
            .form(&params)
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Referer", "https://www.bse.cn/nq/listedcompany.html")
            .header("X-Requested-With", "XMLHttpRequest");
        if let Some(proxy_url) = pool_proxy(&mut pool).await {
            let p =
                wreq::Proxy::all(crate::proxy::ProxyPool::proxy_spec(&proxy_url)).map_err(|e| {
                    crate::error::CollectError::InvalidInput(format!(
                        "invalid proxy {proxy_url:?}: {e}"
                    ))
                })?;
            request = request.proxy(p);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(crate::error::CollectError::HttpStatus(
                response.status().as_u16(),
            ));
        }
        let body = response.text().await?;
        if !body.starts_with("null(") || !body.ends_with(")") {
            break;
        }
        let inner = &body[5..body.len() - 1];
        let Ok(data) = serde_json::from_str::<Value>(inner) else {
            break;
        };
        let Some(arr) = data.as_array() else {
            break;
        };
        let Some(meta) = arr.first() else {
            break;
        };
        let Some(content) = meta.get("content").and_then(Value::as_array) else {
            break;
        };
        if content.is_empty() {
            break;
        }
        all_rows.extend(content.iter().cloned());
        let total_pages = meta.get("totalPages").and_then(Value::as_u64).unwrap_or(0);
        page += 1;
        if page as u64 >= total_pages {
            break;
        }
    }
    Ok(all_rows)
}

async fn with_retry<T, F, Fut>(mut f: F, desc: &str) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_err = None;
    for attempt in 0..MAX_RETRIES {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                eprintln!("  {desc} retry {}/{}: {e}", attempt + 1, MAX_RETRIES);
                last_err = Some(e);
                if attempt + 1 < MAX_RETRIES {
                    tokio::time::sleep(std::time::Duration::from_secs(1u64 << attempt)).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        crate::error::CollectError::InvalidInput(format!("{desc}: retries exhausted"))
    }))
}

/// Fetch all three exchange official lists, merge, and write the CSV.
pub async fn run(output: Option<&str>, update_date: Option<&str>) -> Result<PathBuf> {
    let update_date = update_date.unwrap_or(&today()).to_string();
    if chrono::NaiveDate::parse_from_str(&update_date, "%Y-%m-%d").is_err() {
        return Err(crate::error::CollectError::InvalidDate {
            label: "update_date".into(),
            value: update_date,
        });
    }
    let output_path = match output {
        Some(p) => PathBuf::from(p),
        None => csv_dir()?.join("stock_basic_official.csv"),
    };

    let client = HttpClient::new()?;
    let mut exchange_records: Vec<Vec<OfficialRecord>> = Vec::new();

    // 1. SSE
    eprintln!("[1/4] 上交所 SSE ...");
    let sse_data = with_retry(|| fetch_sse(&client), "上交所").await?;
    let sse_records = parse_sse_json(&sse_data, &update_date);
    if sse_records.is_empty() {
        return Err(crate::error::CollectError::InvalidInput(
            "上交所返回空记录".into(),
        ));
    }
    eprintln!("  ✓ 上交所: {} 条（含退市）", sse_records.len());
    exchange_records.push(sse_records);

    // 2. SZSE active
    eprintln!("[2/4] 深交所 SZSE 正常上市 ...");
    let szse_active_xml = with_retry(
        || fetch_szse_xlsx(&client, "1110", "tab1"),
        "深交所 正常上市",
    )
    .await?;
    let szse_records = parse_szse_xlsx(&szse_active_xml, &update_date);
    if szse_records.is_empty() {
        return Err(crate::error::CollectError::InvalidInput(
            "深交所 正常上市返回空记录".into(),
        ));
    }
    eprintln!("  ✓ 深交所 正常上市: {} 条", szse_records.len());
    exchange_records.push(szse_records);

    // 3. SZSE delisted
    eprintln!("[3/4] 深交所 SZSE 退市股 ...");
    let szse_delisted_xml = with_retry(
        || fetch_szse_xlsx(&client, "1793_ssgs", "tab2"),
        "深交所 退市",
    )
    .await?;
    let szse_delisted_records = parse_szse_delisted(&szse_delisted_xml, &update_date);
    if szse_delisted_records.is_empty() {
        return Err(crate::error::CollectError::InvalidInput(
            "深交所 退市返回空记录".into(),
        ));
    }
    eprintln!("  ✓ 深交所 退市: {} 条", szse_delisted_records.len());
    exchange_records.push(szse_delisted_records);

    // 4. BSE
    eprintln!("[4/4] 北交所 BSE ...");
    let bse_raw = with_retry(|| fetch_bse(&client), "北交所").await?;
    let bse_body = format!(
        "null([{{\"content\": {}}}])",
        serde_json::to_string(&bse_raw)?
    );
    let bse_records = parse_bse_json(&bse_body, &update_date);
    if bse_records.is_empty() {
        return Err(crate::error::CollectError::InvalidInput(
            "北交所返回空记录".into(),
        ));
    }
    eprintln!("  ✓ 北交所: {} 条", bse_records.len());
    exchange_records.push(bse_records);

    // Merge, dedupe, sort.
    eprintln!("合并各交易所数据...");
    let merged = merge_exchanges(&exchange_records);
    eprintln!("  合并后: {} 条（去重后）", merged.len());
    if merged.is_empty() {
        return Err(crate::error::CollectError::InvalidInput(
            "三大交易所返回空数据，拒绝写出 stock_basic CSV".into(),
        ));
    }

    records_to_csv(&merged, &output_path)?;
    eprintln!("完成 — {} 条 → {}", merged.len(), output_path.display());
    Ok(output_path)
}

const STOCK_BASIC_DDL: &str = "CREATE TABLE IF NOT EXISTS stock_basic (
    symbol varchar(20) NOT NULL PRIMARY KEY COMMENT 'Dolt格式',
    ts_code varchar(20) COMMENT 'ts_code格式',
    code varchar(10) COMMENT '6位代码',
    name varchar(100) COMMENT '股票名称',
    list_date varchar(20) COMMENT '上市日期',
    delist_date date,
    board varchar(50),
    full_name varchar(200),
    total_share double,
    industry varchar(50) COMMENT '行业',
    region varchar(20) COMMENT '地区板块',
    update_date varchar(20) COMMENT '更新日期',
    industry_en varchar(100) COMMENT '行业英文名'
)";

fn last_csv_cell(output: &str) -> String {
    output
        .trim()
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn dolt_table_exists(table: &str) -> Result<bool> {
    let out = dolt_sql_csv(&format!(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{table}'"
    ))
    .await?;
    Ok(last_csv_cell(&out) == "1")
}

async fn checked_sql(sql: &str, desc: &str) -> Result<()> {
    let output = dolt_sql(sql).await;
    match output {
        Ok(_) => Ok(()),
        Err(e) => Err(CollectError::Dolt {
            stderr: format!(
                "{desc}: {}",
                match e {
                    CollectError::Dolt { stderr } => stderr,
                    other => other.to_string(),
                }
            ),
        }),
    }
}

/// Import the official stock-basic CSV into Dolt.
///
/// Mirrors `main.py::_import_stock_basic`: replace-by-rename with a temporary
/// `_sb_backup`, guarded by staging/row-count sanity checks, and optionally
/// join the name-en mapping by exact or Roman-numeral-suffix-stripped
/// industry key. The previous table is restored on any failure.
pub async fn import_to_dolt(csv_path: Option<&Path>) -> Result<u64> {
    let path = match csv_path {
        Some(p) => p.to_path_buf(),
        None => csv_dir()?.join("stock_basic_official.csv"),
    };
    if !path.exists() {
        return Err(CollectError::InvalidInput(format!(
            "{} not found. Run fetch first.",
            path.display()
        )));
    }

    dolt_sql("DROP TABLE IF EXISTS _tmp_sb").await?;
    dolt_table_import("_tmp_sb", &path, None).await?;

    let tmp_out = dolt_sql_csv("SELECT COUNT(*) FROM _tmp_sb").await?;
    let tmp_total: u64 = last_csv_cell(&tmp_out).parse().map_err(|_| {
        CollectError::InvalidInput("stock_basic import: cannot read _tmp_sb count".into())
    })?;
    if tmp_total == 0 {
        let _ = dolt_sql("DROP TABLE IF EXISTS _tmp_sb").await;
        return Err(CollectError::InvalidInput(
            "stock_basic import: _tmp_sb is empty; refusing to overwrite stock_basic".into(),
        ));
    }

    let before_total: u64 = if dolt_table_exists("stock_basic").await? {
        let out = dolt_sql_csv("SELECT COUNT(*) FROM stock_basic").await?;
        last_csv_cell(&out).parse().unwrap_or(0)
    } else {
        0
    };
    if before_total > 0 && tmp_total < before_total / 2 {
        let _ = dolt_sql("DROP TABLE IF EXISTS _tmp_sb").await;
        return Err(CollectError::InvalidInput(format!(
            "stock_basic import: candidate is too small ({tmp_total} < {})",
            before_total / 2
        )));
    }

    let mapping = load_name_en_mapping().await?;
    let mut renamed = false;
    let result: Result<()> = async {
        if before_total > 0 {
            checked_sql(
                "DROP TABLE IF EXISTS _sb_backup",
                "drop old stock_basic backup",
            )
            .await?;
            checked_sql(
                "RENAME TABLE stock_basic TO _sb_backup",
                "rename stock_basic to backup",
            )
            .await?;
            renamed = true;
            checked_sql(
                "CREATE TABLE stock_basic LIKE _sb_backup",
                "recreate stock_basic schema",
            )
            .await?;
        } else {
            checked_sql(STOCK_BASIC_DDL, "create stock_basic schema").await?;
        }

        let (join, insert_en_cols, select_en_cols) = if mapping {
            (
                "LEFT JOIN _tmp_name_en m \
                   ON m.section = 'industry' \
                  AND (m.`key` = TRIM(t.industry) \
                       OR (TRIM(t.industry) REGEXP '[ⅠⅡⅢⅣⅤⅥⅦⅧⅨⅩ]$' \
                           AND m.`key` <> TRIM(t.industry) \
                           AND m.`key` = LEFT(TRIM(t.industry), \
                                              CHAR_LENGTH(TRIM(t.industry)) - 1)))",
                ", industry_en",
                ", m.value",
            )
        } else {
            ("", "", "")
        };
        let sql = format!(
            "INSERT INTO stock_basic (symbol, ts_code, code, name, list_date, \
             delist_date, board, full_name, total_share, industry, region, \
             update_date{insert_en_cols}) \
             SELECT t.symbol, t.ts_code, t.code, TRIM(t.name), t.list_date, \
             t.delist_date, TRIM(t.board), TRIM(t.full_name), t.total_share, \
             TRIM(t.industry), TRIM(t.region), t.update_date{select_en_cols} \
             FROM _tmp_sb t \
             {join}"
        );
        checked_sql(&sql, "insert stock_basic").await?;

        let out = dolt_sql_csv("SELECT COUNT(*) FROM stock_basic").await?;
        let total: u64 = last_csv_cell(&out).parse().unwrap_or(0);
        if total == 0 {
            return Err(CollectError::InvalidInput(
                "stock_basic import: final row count is empty".into(),
            ));
        }
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            let total: u64 =
                last_csv_cell(&dolt_sql_csv("SELECT COUNT(*) FROM stock_basic").await?)
                    .parse()
                    .unwrap_or(0);
            if renamed {
                let _ = dolt_sql("DROP TABLE IF EXISTS _sb_backup").await;
            }
            let _ = dolt_sql("DROP TABLE IF EXISTS _tmp_sb").await;
            let _ = dolt_sql("DROP TABLE IF EXISTS _tmp_name_en").await;
            dolt_sql(&format!(
                "INSERT INTO data_updates (table_name, last_updated, source, row_count) \
                 VALUES ('stock_basic', CURDATE(), 'SSE/SZSE/BSE official', {total}) \
                 ON DUPLICATE KEY UPDATE last_updated=CURDATE(), source=VALUES(source), \
                 row_count=VALUES(row_count)"
            ))
            .await?;
            eprintln!("  Done: {total} rows");
            Ok(total)
        }
        Err(e) => {
            if renamed {
                let _ = dolt_sql("DROP TABLE IF EXISTS stock_basic").await;
                let _ = checked_sql(
                    "RENAME TABLE _sb_backup TO stock_basic",
                    "restore stock_basic backup",
                )
                .await;
            }
            let _ = dolt_sql("DROP TABLE IF EXISTS _tmp_sb").await;
            let _ = dolt_sql("DROP TABLE IF EXISTS _tmp_name_en").await;
            let _ = dolt_sql("DROP TABLE IF EXISTS _sb_backup").await;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UPDATE_DATE: &str = "2026-07-31";

    #[test]
    fn infer_exchange_rules() {
        assert_eq!(infer_exchange("600000"), "SH");
        assert_eq!(infer_exchange("688001"), "SH");
        assert_eq!(infer_exchange("000001"), "SZ");
        assert_eq!(infer_exchange("830001"), "BJ");
        assert_eq!(infer_exchange("920000"), "BJ");
    }

    #[test]
    fn fmt_date_formats_eight_digits() {
        assert_eq!(fmt_date("19991110"), "1999-11-10");
        assert_eq!(fmt_date("bad"), "");
    }

    #[test]
    fn parse_sse_filters_b_share() {
        let data = serde_json::json!({
            "pageHelp": {"data": [
                {"A_STOCK_CODE": "900901", "COMPANY_ABBR": "云赛B股", "STOCK_TYPE": "2", "LIST_BOARD": "1", "LIST_DATE": "19911101", "DELIST_DATE": "-", "CSRC_CODE_DESC": "x", "AREA_NAME_DESC": "y"},
                {"A_STOCK_CODE": "600000", "COMPANY_ABBR": "浦发银行", "STOCK_TYPE": "1", "LIST_BOARD": "1", "LIST_DATE": "19991110", "DELIST_DATE": "-", "CSRC_CODE_DESC": "金融业", "AREA_NAME_DESC": "上海市", "FULL_NAME": "上海浦东发展银行股份有限公司"}
            ]}
        });
        let rows = parse_sse_json(&data, UPDATE_DATE);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].code, "600000");
        assert_eq!(rows[0].symbol, "SH600000");
        assert_eq!(rows[0].ts_code, "600000.SH");
        assert_eq!(rows[0].list_date, "1999-11-10");
        assert_eq!(rows[0].board, "主板");
        assert_eq!(rows[0].industry, "金融业");
    }

    #[test]
    fn parse_szse_active_row() {
        let xml = r#"<row><c r="A1" t="inlineStr"><is><t>板块</t></is></c><c r="B1" t="inlineStr"><is><t>公司全称</t></is></c><c r="E1" t="inlineStr"><is><t>A股代码</t></is></c><c r="F1" t="inlineStr"><is><t>A股简称</t></is></c><c r="G1" t="inlineStr"><is><t>A股上市日期</t></is></c><c r="H1" t="inlineStr"><is><t>A股总股本</t></is></c><c r="P1" t="inlineStr"><is><t>省份</t></is></c><c r="R1" t="inlineStr"><is><t>所属行业</t></is></c></row><row><c r="A2" t="inlineStr"><is><t>主板</t></is></c><c r="B2" t="inlineStr"><is><t>平安银行股份有限公司</t></is></c><c r="E2" t="inlineStr"><is><t>000001</t></is></c><c r="F2" t="inlineStr"><is><t>平安银行</t></is></c><c r="G2" t="inlineStr"><is><t>1991-04-03</t></is></c><c r="H2"><v>19405918198</v></c><c r="P2" t="inlineStr"><is><t>广东</t></is></c><c r="R2" t="inlineStr"><is><t>J 金融业</t></is></c></row>"#;
        let rows = parse_szse_xlsx(xml, UPDATE_DATE);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "SZ000001");
        assert_eq!(rows[0].name, "平安银行");
        assert_eq!(rows[0].total_share, "19405918198.0");
        assert_eq!(rows[0].board, "主板");
    }

    #[test]
    fn parse_szse_delisted_row() {
        let xml = r#"<row><c r="A1" t="inlineStr"><is><t>证券代码</t></is></c><c r="B1" t="inlineStr"><is><t>证券简称</t></is></c><c r="C1" t="inlineStr"><is><t>上市日期</t></is></c><c r="D1" t="inlineStr"><is><t>终止上市日期</t></is></c></row><row><c r="A2" t="inlineStr"><is><t>000003</t></is></c><c r="B2" t="inlineStr"><is><t>PT金田A</t></is></c><c r="C2" t="inlineStr"><is><t>1991-01-14</t></is></c><c r="D2" t="inlineStr"><is><t>2002-06-14</t></is></c></row>"#;
        let rows = parse_szse_delisted(xml, UPDATE_DATE);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].delist_date, "2002-06-14");
    }

    #[test]
    fn parse_bse_jsonp() {
        let body = r#"null([{"content": [{"xxzqdm": "920000", "xxzqjc": "安徽凤凰", "fxssrq": "20201223", "xxhyzl": "汽车制造业", "xxssdq": "安徽省", "xxzgb": 91680000}], "totalPages": 1}])"#;
        let rows = parse_bse_json(body, UPDATE_DATE);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "BJ920000");
        assert_eq!(rows[0].list_date, "2020-12-23");
        assert_eq!(rows[0].total_share, "91680000.0");
    }

    #[test]
    fn merge_dedupes_and_sorts_by_code() {
        let mut a = parse_bse_json(
            r#"null([{"content": [{"xxzqdm": "920000", "xxzqjc": "B", "fxssrq": "20200101", "xxhyzl": "", "xxssdq": "", "xxzgb": 1}], "totalPages": 1}])"#,
            UPDATE_DATE,
        );
        let b = parse_sse_json(
            &serde_json::json!({"pageHelp": {"data": [{"A_STOCK_CODE": "600000", "COMPANY_ABBR": "A", "STOCK_TYPE": "1", "LIST_BOARD": "1", "LIST_DATE": "19991110", "DELIST_DATE": "-"}]}}),
            UPDATE_DATE,
        );
        a.push(OfficialRecord {
            symbol: "SZ000001".into(),
            ts_code: "000001.SZ".into(),
            code: "000001".into(),
            name: "dup".into(),
            list_date: "".into(),
            delist_date: "".into(),
            board: "".into(),
            full_name: "".into(),
            total_share: "".into(),
            industry: "".into(),
            region: "".into(),
            update_date: UPDATE_DATE.into(),
        });
        let merged = merge_exchanges(&[a, b]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].code, "000001");
        assert_eq!(merged[2].code, "920000");
    }
}
