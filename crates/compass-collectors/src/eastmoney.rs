use std::collections::HashMap;

use serde_json::Value;

use crate::error::Result;
use crate::http::{EM_BASE, EM_MAX_RETRIES, HttpClient, Throttle};
use crate::proxy::{ProxyPool, make_proxy_pool};

/// Flatten a JSON object into `String` values (None becomes empty string).
pub fn flatten_record(item: &Value) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(obj) = item.as_object() {
        for (k, v) in obj {
            let value = match v {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            };
            out.insert(k.clone(), value);
        }
    }
    out
}

async fn request_json(
    client: &HttpClient,
    throttle: &mut Throttle,
    params: &HashMap<String, String>,
    _pool: &mut Option<ProxyPool>,
) -> Result<Value> {
    let mut last_err = None;
    for attempt in 0..EM_MAX_RETRIES {
        throttle.acquire().await;
        match client.get_json(EM_BASE, params).await {
            Ok(data) => return Ok(data),
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < EM_MAX_RETRIES {
                    let wait = ((1u64 << attempt.min(5)) * 1000).min(30_000);
                    tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        crate::error::CollectError::InvalidInput("request failed without error".into())
    }))
}

/// Fetch all pages for a single EastMoney report period.
pub async fn fetch_paginated(
    client: &HttpClient,
    throttle: &mut Throttle,
    report_name: &str,
    filter_column: &str,
    report_date: &str,
    page_size: usize,
) -> Result<Vec<HashMap<String, String>>> {
    let mut pool = make_proxy_pool();
    let mut all = Vec::new();
    let mut page = 1;
    let mut total_pages = 1;

    while page <= total_pages {
        let params = paginated_params(report_name, filter_column, report_date, page_size, page);
        let data = request_json(client, throttle, &params, &mut pool).await?;
        if data.get("success").and_then(|v| v.as_bool()) != Some(true) {
            break;
        }
        let Some(result) = data.get("result") else {
            break;
        };
        let Some(items) = result.get("data").and_then(|v| v.as_array()) else {
            break;
        };
        if items.is_empty() {
            break;
        }
        for item in items {
            all.push(flatten_record(item));
        }
        total_pages = result.get("pages").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        total_pages = total_pages.min(500);
        page += 1;
    }
    Ok(all)
}

/// Fetch all rows with `UPDATE_DATE >= anchor`.
pub async fn fetch_by_update_date(
    client: &HttpClient,
    throttle: &mut Throttle,
    report_name: &str,
    anchor: &str,
    page_size: usize,
) -> Result<Vec<HashMap<String, String>>> {
    let mut pool = make_proxy_pool();
    let mut all = Vec::new();
    let mut page = 1;
    let mut total_pages = 1;

    while page <= total_pages {
        let mut params = paginated_params(report_name, "UPDATE_DATE", anchor, page_size, page);
        params.insert("filter".to_string(), format!("(UPDATE_DATE>='{anchor}')"));
        params.insert("sortColumns".to_string(), "UPDATE_DATE".to_string());
        let data = request_json(client, throttle, &params, &mut pool).await?;
        if data.get("success").and_then(|v| v.as_bool()) != Some(true) {
            break;
        }
        let Some(result) = data.get("result") else {
            break;
        };
        let Some(items) = result.get("data").and_then(|v| v.as_array()) else {
            break;
        };
        if items.is_empty() {
            break;
        }
        for item in items {
            all.push(flatten_record(item));
        }
        total_pages = result.get("pages").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        total_pages = total_pages.min(500);
        page += 1;
    }
    Ok(all)
}

fn paginated_params(
    report_name: &str,
    filter_column: &str,
    report_date: &str,
    page_size: usize,
    page: usize,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("reportName".to_string(), report_name.to_string());
    params.insert("columns".to_string(), "ALL".to_string());
    params.insert(
        "filter".to_string(),
        format!("({filter_column}='{report_date}')"),
    );
    params.insert("sortColumns".to_string(), "SECURITY_CODE".to_string());
    params.insert("sortTypes".to_string(), "1".to_string());
    params.insert("pageSize".to_string(), page_size.to_string());
    params.insert("pageNumber".to_string(), page.to_string());
    params.insert("source".to_string(), "WEB".to_string());
    params.insert("client".to_string(), "WEB".to_string());
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_handles_null_and_numbers() {
        let v = serde_json::json!({"a": null, "b": 1, "c": "x"});
        let map = flatten_record(&v);
        assert_eq!(map["a"], "");
        assert_eq!(map["b"], "1");
        assert_eq!(map["c"], "x");
    }
}
