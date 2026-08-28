use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::config::csv_dir;
use crate::csv::write_csv_ordered;
use crate::eastmoney::Record;
use crate::error::Result;
use crate::http::{EM_MAX_RETRIES, EM_MIN_INTERVAL, HttpClient, Throttle};

pub const EM_LIST_URL: &str = "https://push2delay.eastmoney.com/api/qt/clist/get";
pub const EM_FS: &str = "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81";
pub const EM_FIELDS: &str = "f12,f13,f14,f26,f100,f101,f102,f103,f127,f128,f134,f189,f124,f221";
pub const MAX_PAGES: usize = 100;

fn em_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), crate::http::EM_UA.to_string());
    headers.insert("Accept".to_string(), "*/*".to_string());
    headers.insert(
        "Accept-Language".to_string(),
        "zh-CN,zh;q=0.9,en;q=0.8".to_string(),
    );
    headers.insert(
        "Referer".to_string(),
        "https://quote.eastmoney.com/".to_string(),
    );
    headers.insert(
        "Sec-Ch-Ua".to_string(),
        "\"Chromium\";v=\"142\", \"Google Chrome\";v=\"142\", \"Not_A Brand\";v=\"99\"".to_string(),
    );
    headers.insert("Sec-Ch-Ua-Mobile".to_string(), "?0".to_string());
    headers.insert("Sec-Ch-Ua-Platform".to_string(), "\"Windows\"".to_string());
    headers.insert("Sec-Fetch-Dest".to_string(), "empty".to_string());
    headers.insert("Sec-Fetch-Mode".to_string(), "cors".to_string());
    headers.insert("Sec-Fetch-Site".to_string(), "same-site".to_string());
    headers.insert("Connection".to_string(), "keep-alive".to_string());
    headers
}

pub fn infer_exchange(code: &str) -> &'static str {
    if code.starts_with('6') {
        "SH"
    } else if code.starts_with('8') {
        "BJ"
    } else {
        "SZ"
    }
}

pub fn to_ts_code(code: &str) -> String {
    format!("{code}.{}", infer_exchange(code))
}

pub fn to_symbol(code: &str) -> String {
    format!("{}{}", infer_exchange(code), code)
}

fn field_sort_key(key: &str) -> (bool, i64) {
    let digits = key.strip_prefix('f').unwrap_or(key);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        (false, digits.parse::<i64>().unwrap_or(0))
    } else {
        (true, 0)
    }
}

fn build_record(item: &Value) -> Option<Record> {
    let obj = item.as_object()?;
    let code = obj.get("f12")?.as_str()?;
    if code.is_empty() {
        return None;
    }
    let mut record: Record = Vec::new();
    record.push(("symbol".to_string(), to_symbol(code)));
    record.push(("ts_code".to_string(), to_ts_code(code)));

    let mut api_fields: Vec<&String> = obj
        .keys()
        .filter(|k| k.as_str() != "symbol" && k.as_str() != "ts_code")
        .collect();
    api_fields.sort_by_key(|k| field_sort_key(k));
    for key in api_fields {
        let value = match obj.get(key) {
            Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        record.push((key.clone(), value));
    }
    Some(record)
}

fn params(page: usize, page_size: usize) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("pn".to_string(), page.to_string());
    params.insert("pz".to_string(), page_size.to_string());
    params.insert("po".to_string(), "1".to_string());
    params.insert("np".to_string(), "1".to_string());
    params.insert("fltt".to_string(), "2".to_string());
    params.insert("invt".to_string(), "2".to_string());
    params.insert("fid".to_string(), "f3".to_string());
    params.insert("fs".to_string(), EM_FS.to_string());
    params.insert("fields".to_string(), EM_FIELDS.to_string());
    params.insert(
        "ut".to_string(),
        "bd1d9ddb04089700cf9c27f6f7426281".to_string(),
    );
    params
}

async fn fetch_page(
    client: &HttpClient,
    throttle: &mut Throttle,
    page: usize,
    page_size: usize,
) -> Result<Vec<Record>> {
    let headers = em_headers();
    let params = params(page, page_size);
    for attempt in 0..EM_MAX_RETRIES {
        throttle.acquire().await;
        match client
            .get_json_with_headers_and_proxy(EM_LIST_URL, &params, &headers, None)
            .await
        {
            Ok(data) => {
                let Some(items) = data
                    .get("data")
                    .and_then(|d| d.get("diff"))
                    .and_then(Value::as_array)
                else {
                    return Ok(Vec::new());
                };
                let mut records = Vec::new();
                for item in items {
                    if let Some(record) = build_record(item) {
                        records.push(record);
                    }
                }
                return Ok(records);
            }
            Err(e) => {
                if attempt + 1 < EM_MAX_RETRIES {
                    let wait = ((1u64 << attempt) * 1000).min(30_000) as f64 / 1000.0
                        + rand::random::<f64>() * 2.0;
                    eprintln!("  Retry {} in {wait:.1}s: {e}", attempt + 1);
                    tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(crate::error::CollectError::InvalidInput(
        "stock_basic fetch loop exhausted without a result".into(),
    ))
}

/// Fetch A-share stock basic info from EastMoney into a CSV.
pub async fn run(output: Option<&str>, page_size: usize, max_pages: usize) -> Result<PathBuf> {
    let output_path = match output {
        Some(p) => PathBuf::from(p),
        None => csv_dir()?.join("stock_basic.csv"),
    };
    let client = HttpClient::new()?;
    let mut throttle = Throttle::new(EM_MIN_INTERVAL);
    let headers = em_headers();

    // First tiny page to read the reported total.
    let first_params = params(1, 1);
    let first = client
        .get_json_with_headers_and_proxy(EM_LIST_URL, &first_params, &headers, None)
        .await?;
    let total_count = first
        .get("data")
        .and_then(|d| d.get("total"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if total_count > 0 {
        eprintln!("  Total stocks reported: {total_count}");
    }
    let total_pages = if total_count > 0 {
        let pages = total_count.div_ceil(page_size as u64) as usize;
        pages.min(max_pages)
    } else {
        max_pages
    };

    let mut records: Vec<Record> = Vec::new();
    for page in 1..=total_pages {
        let items = fetch_page(&client, &mut throttle, page, page_size).await?;
        if items.is_empty() {
            eprintln!("  Page {page}: empty, stopping.");
            break;
        }
        records.extend(items);
        eprintln!("  Page {page}/{total_pages} | {} stashed", records.len());
    }

    if records.is_empty() {
        return Ok(output_path);
    }
    write_csv_ordered(&output_path, &records)?;
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_inference_matches_python() {
        assert_eq!(infer_exchange("600519"), "SH");
        assert_eq!(infer_exchange("000001"), "SZ");
        assert_eq!(infer_exchange("830001"), "BJ");
    }

    #[test]
    fn symbol_and_ts_code() {
        assert_eq!(to_symbol("600519"), "SH600519");
        assert_eq!(to_ts_code("600519"), "600519.SH");
        assert_eq!(to_ts_code("000001"), "000001.SZ");
    }

    #[test]
    fn build_record_sorts_api_fields_by_numeric_suffix() {
        let item = serde_json::json!({
            "f124": "2024-01-01",
            "f14": "贵州茅台",
            "f12": "600519",
            "f13": "1"
        });
        let record = build_record(&item).unwrap();
        let fields: Vec<&str> = record.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(&fields[..2], &["symbol", "ts_code"]);
        assert_eq!(&fields[2..], &["f12", "f13", "f14", "f124"]);
    }
}
