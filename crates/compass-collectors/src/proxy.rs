use std::path::PathBuf;

use serde::Serialize;

use crate::config::{default_proxy_state_path, proxy_enabled};
use crate::error::Result;
use crate::http::HttpClient;

pub const DEFAULT_API_URL: &str = "http://127.0.0.1:5010";
pub const DEFAULT_PROXY_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Serialize)]
struct ProxyState {
    timestamp: String,
    pool_count: u64,
    degraded: bool,
    reason: String,
}

/// Thin async client for the local jhao104/proxy_pool HTTP API.
#[derive(Debug, Clone)]
pub struct ProxyPool {
    api_url: String,
    state_path: PathBuf,
    warned_empty: bool,
}

impl ProxyPool {
    pub fn new(api_url: Option<String>, state_path: Option<PathBuf>) -> Result<Self> {
        let api_url = api_url
            .or_else(|| std::env::var("COMPASS_PROXY_API_URL").ok())
            .unwrap_or_else(|| DEFAULT_API_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let state_path = state_path.unwrap_or(default_proxy_state_path()?);
        Ok(Self {
            api_url,
            state_path,
            warned_empty: false,
        })
    }

    /// Return one `IP:PORT` proxy, or None when the pool is empty/unreachable
    /// (degradation to direct, never a hard error).
    pub async fn get_proxy(&mut self) -> Option<String> {
        let url = format!("{}/get/", self.api_url);
        let mut params = std::collections::HashMap::new();
        params.insert("type".to_string(), "https".to_string());
        let client = match HttpClient::new() {
            Ok(c) => c,
            Err(_) => return None,
        };
        match client.get_json(&url, &params).await {
            Ok(data) => {
                if let Some(proxy) = data.get("proxy").and_then(|v| v.as_str())
                    && !proxy.trim().is_empty()
                {
                    return Some(proxy.trim().to_string());
                }
                self.note_empty("https pool empty").await;
                None
            }
            Err(_) => {
                self.note_empty("proxy_pool API unreachable").await;
                None
            }
        }
    }

    pub async fn delete_proxy(&self, proxy: &str) {
        let url = format!("{}/delete/", self.api_url);
        let mut params = std::collections::HashMap::new();
        params.insert("proxy".to_string(), proxy.to_string());
        if let Ok(client) = HttpClient::new()
            && let Err(e) = client.get_json(&url, &params).await
        {
            tracing::warn!(proxy, error = %e, "failed to delete bad proxy");
        }
    }

    pub async fn pool_count(&self) -> u64 {
        let url = format!("{}/count/", self.api_url);
        let empty = std::collections::HashMap::new();
        if let Ok(client) = HttpClient::new()
            && let Ok(data) = client.get_json(&url, &empty).await
        {
            if let Some(n) = data.get("count").and_then(|v| v.as_u64()) {
                return n;
            }
            if let Some(n) = data.get("total").and_then(|v| v.as_u64()) {
                return n;
            }
            if let Some(s) = data.as_str()
                && let Ok(n) = s.parse()
            {
                return n;
            }
        }
        0
    }

    pub async fn record_state(&self, pool_count: u64, degraded: bool, reason: &str) -> Result<()> {
        let state = ProxyState {
            timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            pool_count,
            degraded,
            reason: reason.to_string(),
        };
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self
            .state_path
            .with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_string_pretty(&state)?)?;
        std::fs::rename(&tmp, &self.state_path)?;
        Ok(())
    }

    async fn note_empty(&mut self, reason: &str) {
        let count = self.pool_count().await;
        let _ = self.record_state(count, true, reason).await;
        if !self.warned_empty {
            eprintln!("[proxy] WARN/ERROR: https pool empty, falling back to direct");
            self.warned_empty = true;
        }
    }

    pub fn proxy_spec(proxy: &str) -> String {
        format!("http://{proxy}")
    }
}

pub fn make_proxy_pool() -> Option<ProxyPool> {
    if !proxy_enabled() {
        return None;
    }
    ProxyPool::new(None, None).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_spec_builds_http_url() {
        assert_eq!(ProxyPool::proxy_spec("1.2.3.4:8080"), "http://1.2.3.4:8080");
    }
}
