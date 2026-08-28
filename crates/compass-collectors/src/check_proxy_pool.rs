//! Proxy-pool trial harness for THS (10jqka) endpoints.
//!
//! Mirrors `collectors/check_proxy_pool.py`: fetch candidates from the local
//! proxy_pool `/all/` API, probe THS list/kline URLs through each proxy with
//! Chrome fingerprint TLS, and aggregate a PASS/FAIL verdict.

use std::time::{Duration, Instant};

use chrono::Datelike;
use serde::Serialize;
use serde_json::Value;

use crate::error::{CollectError, Result};
use crate::http::HttpClient;

pub const DEFAULT_API_URL: &str = "http://127.0.0.1:5010";
pub const DEFAULT_COUNT: usize = 15;
pub const DEFAULT_TIMEOUT: f64 = 10.0;
pub const THS_LIST_URL: &str = "https://q.10jqka.com.cn/thshy/";
pub const THS_KLINE_URL_TEMPLATE: &str = "https://d.10jqka.com.cn/v4/line/bk_881101/01/{year}.js";

#[derive(Debug, Clone, Serialize)]
pub struct TrialResult {
    pub target: String,
    pub total: u64,
    pub success: u64,
    pub failures: Vec<String>,
    pub success_rate: f64,
    pub avg_elapsed: f64,
}

fn current_kline_url() -> String {
    let year = chrono::Local::now().date_naive().year();
    THS_KLINE_URL_TEMPLATE.replace("{year}", &year.to_string())
}

fn proxy_pool_all_url(api_url: &str) -> String {
    format!("{}/all/", api_url.trim_end_matches('/'))
}

pub async fn get_proxies(api_url: &str, count: usize) -> Vec<String> {
    let Ok(client) = HttpClient::new() else {
        return Vec::new();
    };
    let empty = std::collections::HashMap::new();
    let url = proxy_pool_all_url(api_url);
    let Ok(data) = client
        .get_json_with_headers_and_proxy(&url, &empty, &empty, None)
        .await
    else {
        return Vec::new();
    };

    let raw: Vec<Value> = if let Some(arr) = data.as_array() {
        arr.iter()
            .filter_map(|item| {
                item.get("proxy")
                    .and_then(Value::as_str)
                    .map(|s| Value::String(s.to_string()))
            })
            .collect()
    } else if let Some(arr) = data.get("proxies").and_then(Value::as_array) {
        arr.iter()
            .filter_map(|item| item.as_str().map(|s| Value::String(s.to_string())))
            .collect()
    } else if let Some(single) = data.get("proxy").and_then(Value::as_str) {
        vec![Value::String(single.to_string())]
    } else {
        Vec::new()
    };

    raw.into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .take(count)
        .collect()
}

pub async fn fetch_with_proxy(url: &str, proxy: &str, timeout: f64) -> (bool, f64, Option<String>) {
    let start = Instant::now();
    let client = match HttpClient::new() {
        Ok(c) => c,
        Err(e) => return (false, start.elapsed().as_secs_f64(), Some(e.to_string())),
    };
    let mut request = client.client().get(url);
    if let Ok(p) = wreq::Proxy::all(proxy) {
        request = request.proxy(p);
    } else {
        return (
            false,
            start.elapsed().as_secs_f64(),
            Some(format!("invalid proxy {proxy}")),
        );
    }
    request = request.timeout(Duration::from_secs_f64(timeout));
    match request.send().await {
        Ok(resp) if resp.status().is_success() => (true, start.elapsed().as_secs_f64(), None),
        Ok(resp) => (
            false,
            start.elapsed().as_secs_f64(),
            Some(format!("HTTP {}", resp.status().as_u16())),
        ),
        Err(e) => (false, start.elapsed().as_secs_f64(), Some(e.to_string())),
    }
}

pub async fn run_trial(
    url: &str,
    count: usize,
    api_url: &str,
    timeout: f64,
) -> Result<TrialResult> {
    if count == 0 {
        return Ok(TrialResult {
            target: url.to_string(),
            total: 0,
            success: 0,
            failures: Vec::new(),
            success_rate: 0.0,
            avg_elapsed: 0.0,
        });
    }
    let proxies = get_proxies(api_url, count).await;
    let total = proxies.len() as u64;
    if total == 0 {
        return Ok(TrialResult {
            target: url.to_string(),
            total: 0,
            success: 0,
            failures: Vec::new(),
            success_rate: 0.0,
            avg_elapsed: 0.0,
        });
    }

    let mut success = 0u64;
    let mut total_elapsed = 0.0;
    let mut failures = Vec::new();
    for proxy in &proxies {
        // Default to a failed attempt; fetch_with_proxy should not throw, but a
        // panic-style error at this layer must not abort the trial.
        let (ok, elapsed, err) = fetch_with_proxy(url, proxy, timeout).await;
        total_elapsed += elapsed;
        if ok {
            success += 1;
        } else {
            failures.push(err.unwrap_or_else(|| "unknown error".to_string()));
        }
    }
    Ok(TrialResult {
        target: url.to_string(),
        total,
        success,
        failures,
        success_rate: success as f64 / total as f64,
        avg_elapsed: total_elapsed / total as f64,
    })
}

pub fn judge(
    result: &TrialResult,
    success_threshold: f64,
    max_avg_elapsed: f64,
) -> Result<(bool, String)> {
    if success_threshold <= 0.0 {
        return Err(CollectError::InvalidInput(
            "success_threshold must be > 0".into(),
        ));
    }
    if max_avg_elapsed <= 0.0 {
        return Err(CollectError::InvalidInput(
            "max_avg_elapsed must be > 0".into(),
        ));
    }
    let rate_ok = result.success_rate >= success_threshold;
    let time_ok = result.avg_elapsed < max_avg_elapsed;
    let passed = rate_ok && time_ok;
    let reason = format!(
        "success_rate={:.3} (>={:.3}: {}), avg_elapsed={:.3}s (<{:.3}s: {})",
        result.success_rate,
        success_threshold,
        rate_ok,
        result.avg_elapsed,
        max_avg_elapsed,
        time_ok
    );
    Ok((
        passed,
        format!("{}: {reason}", if passed { "PASS" } else { "FAIL" }),
    ))
}

/// Run both THS probe targets and return a JSON summary payload.
pub async fn run() -> Result<Value> {
    let list_result = run_trial(
        THS_LIST_URL,
        DEFAULT_COUNT,
        DEFAULT_API_URL,
        DEFAULT_TIMEOUT,
    )
    .await?;
    let kline_result = run_trial(
        &current_kline_url(),
        DEFAULT_COUNT,
        DEFAULT_API_URL,
        DEFAULT_TIMEOUT,
    )
    .await?;

    let combined_total = list_result.total + kline_result.total;
    let combined_success = list_result.success + kline_result.success;
    let (combined_rate, combined_avg) = if combined_total > 0 {
        (
            combined_success as f64 / combined_total as f64,
            (list_result.total as f64 * list_result.avg_elapsed
                + kline_result.total as f64 * kline_result.avg_elapsed)
                / combined_total as f64,
        )
    } else {
        (0.0, 0.0)
    };
    let mut failures = list_result.failures.clone();
    failures.extend(kline_result.failures.clone());
    let combined = TrialResult {
        target: "ALL".to_string(),
        total: combined_total,
        success: combined_success,
        failures,
        success_rate: combined_rate,
        avg_elapsed: combined_avg,
    };
    let (passed, reason) = judge(&combined, 0.5, 5.0)?;
    Ok(serde_json::json!({
        "success_rate": combined.success_rate,
        "avg_elapsed": combined.avg_elapsed,
        "verdict": if passed { "PASS" } else { "FAIL" },
        "judge_reason": reason,
        "failures": combined.failures,
        "targets": [
            {
                "target": list_result.target,
                "total": list_result.total,
                "success": list_result.success,
                "success_rate": list_result.success_rate,
                "avg_elapsed": list_result.avg_elapsed,
            },
            {
                "target": kline_result.target,
                "total": kline_result.total,
                "success": kline_result.success,
                "success_rate": kline_result.success_rate,
                "avg_elapsed": kline_result.avg_elapsed,
            },
        ],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_requires_positive_thresholds() {
        let r = TrialResult {
            target: "x".into(),
            total: 1,
            success: 1,
            failures: vec![],
            success_rate: 1.0,
            avg_elapsed: 0.1,
        };
        assert!(judge(&r, 0.0, 5.0).is_err());
        assert!(judge(&r, 0.5, 0.0).is_err());
    }

    #[test]
    fn judge_strict_on_avg_elapsed() {
        let r = TrialResult {
            target: "x".into(),
            total: 1,
            success: 1,
            failures: vec![],
            success_rate: 1.0,
            avg_elapsed: 5.0,
        };
        let (passed, _) = judge(&r, 0.5, 5.0).unwrap();
        assert!(!passed);
    }

    #[test]
    fn judge_passes_at_exact_success_threshold() {
        let r = TrialResult {
            target: "x".into(),
            total: 2,
            success: 1,
            failures: vec![],
            success_rate: 0.5,
            avg_elapsed: 1.0,
        };
        let (passed, _) = judge(&r, 0.5, 5.0).unwrap();
        assert!(passed);
    }
}
