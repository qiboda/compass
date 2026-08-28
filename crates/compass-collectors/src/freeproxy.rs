//! Freeproxy → proxy_pool Redis seeding.
//!
//! Mirrors `collectors/fetch_freeproxy.py` for the JSON snapshot source:
//! download `proxies.json`, normalize entries, score/sort, and write the
//! resulting records into the proxy_pool Redis hash (`use_proxy`).

use std::collections::HashMap;
use std::net::IpAddr;

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CollectError, Result};
use crate::http::HttpClient;

pub const DEFAULT_JSON_URL: &str =
    "https://raw.githubusercontent.com/CharlesPikachu/freeproxy/master/proxies.json";
pub const DEFAULT_REDIS_URL: &str = "redis://@127.0.0.1:6379/0";
pub const DEFAULT_TABLE: &str = "use_proxy";
pub const DEFAULT_LIMIT: usize = 300;
pub const DEFAULT_REALTIME_SOURCES: [&str; 2] =
    ["ProxiflyProxiedSession", "TrustyTechProxiedSession"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProxyRecord {
    pub proxy: String,
    pub https: bool,
    pub fail_count: u64,
    pub region: String,
    pub anonymous: String,
    pub source: String,
    pub check_count: u64,
    pub last_status: bool,
    pub last_time: String,
}

fn is_http_protocol(protocol: &str) -> bool {
    protocol.to_lowercase().contains("http")
}

const NON_PUBLIC_NETS: [&str; 14] = [
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.0.2.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "224.0.0.0/4",
    "240.0.0.0/4",
    "::1/128",
    "fc00::/7",
];

fn is_public_ip(host: &str) -> bool {
    let Ok(addr) = host.parse::<IpAddr>() else {
        return false;
    };
    if addr.is_loopback() || addr.is_multicast() || addr.is_unspecified() {
        return false;
    }
    if let IpAddr::V6(v6) = addr
        && v6.is_unicast_link_local()
    {
        return false;
    }
    !NON_PUBLIC_NETS.iter().any(|s| {
        s.parse::<IpNet>()
            .map(|net| net.contains(&addr))
            .unwrap_or(false)
    })
}

fn safe_proxy(host: &str, port: &str) -> Option<String> {
    if host.is_empty()
        || !is_public_ip(host)
        || host
            .chars()
            .any(|c| c.is_whitespace() || c == '/' || c == '@')
        || port.is_empty()
        || port.chars().any(|c| c.is_whitespace() || (c as u32) < 32)
    {
        return None;
    }
    let port_int = port.parse::<u16>().ok()?;
    if port_int == 0 {
        return None;
    }
    Some(format!("{host}:{port_int}"))
}

fn safe_proxy_str(proxy: &str) -> Option<String> {
    if proxy.is_empty()
        || proxy
            .chars()
            .any(|c| c.is_whitespace() || c == '/' || c == '@')
    {
        return None;
    }
    let (host, port) = proxy.rsplit_once(':')?;
    safe_proxy(host, port)
}

fn score_item(item: &Value) -> i32 {
    let mut score = 0;
    let protocol = item
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    if protocol.contains("https") {
        score += 2;
    }
    if item
        .get("country")
        .and_then(Value::as_str)
        .unwrap_or("")
        .eq_ignore_ascii_case("CN")
    {
        score += 1;
    }
    if item
        .get("anonymity")
        .and_then(Value::as_str)
        .unwrap_or("")
        .eq_ignore_ascii_case("elite")
    {
        score += 1;
    }
    score
}

fn normalize_json_item(item: &Value) -> Result<ProxyRecord> {
    let host = item.get("ip").and_then(Value::as_str).unwrap_or("");
    let port = item
        .get("port")
        .map(|v| match v {
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            _ => String::new(),
        })
        .unwrap_or_default();
    let proxy = safe_proxy(host, &port)
        .ok_or_else(|| CollectError::InvalidInput(format!("invalid proxy entry: {host}:{port}")))?;
    Ok(ProxyRecord {
        proxy,
        https: false,
        fail_count: 0,
        region: item
            .get("country")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        anonymous: item
            .get("anonymity")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        source: "freeproxy".to_string(),
        check_count: 0,
        last_status: true,
        last_time: String::new(),
    })
}

pub fn normalize_proxy_info(proxy: &str, region: &str, anonymous: &str) -> Result<ProxyRecord> {
    let raw = proxy
        .strip_prefix("http://")
        .or_else(|| proxy.strip_prefix("https://"))
        .unwrap_or(proxy);
    let safe = safe_proxy_str(raw).ok_or_else(|| {
        CollectError::InvalidInput(format!(
            "pyfreeproxy returned an invalid proxy string: {proxy}"
        ))
    })?;
    Ok(ProxyRecord {
        proxy: safe,
        https: false,
        fail_count: 0,
        region: region.to_string(),
        anonymous: anonymous.to_string(),
        source: "freeproxy".to_string(),
        check_count: 0,
        last_status: true,
        last_time: String::new(),
    })
}

pub fn records_from_json_data(payload: &Value, limit: usize) -> Vec<ProxyRecord> {
    let data = payload.get("data").and_then(Value::as_array);
    let Some(data) = data else {
        return Vec::new();
    };
    let mut items: Vec<&Value> = data
        .iter()
        .filter(|item| {
            item.get("protocol")
                .and_then(Value::as_str)
                .map(is_http_protocol)
                .unwrap_or(false)
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(score_item(item)));
    let mut records = Vec::new();
    for item in items.into_iter().take(limit) {
        if let Ok(record) = normalize_json_item(item) {
            records.push(record);
        }
    }
    records
}

pub async fn fetch_json_payload(url: &str) -> Result<Value> {
    let client = HttpClient::new()?;
    let empty = HashMap::new();
    client
        .get_json_with_headers_and_proxy(url, &empty, &HashMap::new(), None)
        .await
}

pub async fn fetch_json_proxies(url: &str, limit: usize) -> Result<Vec<ProxyRecord>> {
    let payload = fetch_json_payload(url).await?;
    Ok(records_from_json_data(&payload, limit))
}

pub fn write_to_redis(redis_url: &str, table: &str, records: &[ProxyRecord]) -> Result<usize> {
    let client = redis::Client::open(redis_url)?;
    let mut con = client.get_connection()?;
    let mut written = 0;
    for record in records {
        let Some(proxy) = safe_proxy_str(&record.proxy) else {
            continue;
        };
        let json = serde_json::to_string(record)?;
        redis::cmd("HSET")
            .arg(table)
            .arg(&proxy)
            .arg(json)
            .exec(&mut con)?;
        written += 1;
    }
    Ok(written)
}

/// Seed from the JSON source; returns the number of records written.
pub async fn seed_json(
    json_url: &str,
    redis_url: &str,
    table: &str,
    limit: usize,
) -> Result<usize> {
    let records = fetch_json_proxies(json_url, limit).await?;
    if records.is_empty() {
        return Ok(0);
    }
    write_to_redis(redis_url, table, &records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_proxy_rejects_private_and_invalid() {
        assert!(safe_proxy("1.2.3.4", "8080").is_some());
        assert!(safe_proxy("127.0.0.1", "8080").is_none());
        assert!(safe_proxy("192.168.1.1", "8080").is_none());
        assert!(safe_proxy("1.2.3.4", "0").is_none());
        assert!(safe_proxy("bad host", "80").is_none());
    }

    #[test]
    fn score_prefers_https_cn_elite() {
        let item =
            serde_json::json!({"protocol": "Http, Https", "country": "CN", "anonymity": "Elite"});
        assert_eq!(score_item(&item), 4);
    }

    #[test]
    fn records_filter_http_and_sort_by_score() {
        let payload = serde_json::json!({
            "data": [
                {"ip": "1.1.1.1", "port": 8080, "protocol": "Http", "country": "US", "anonymity": "Transparent"},
                {"ip": "2.2.2.2", "port": 8080, "protocol": "Http, Https", "country": "CN", "anonymity": "Elite"},
                {"ip": "3.3.3.3", "port": 8080, "protocol": "Socks5"},
            ]
        });
        let records = records_from_json_data(&payload, 10);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].proxy, "2.2.2.2:8080");
    }

    #[test]
    fn normalize_proxy_info_strips_scheme() {
        let r = normalize_proxy_info("http://1.2.3.4:8080", "CN", "Elite").unwrap();
        assert_eq!(r.proxy, "1.2.3.4:8080");
    }
}
