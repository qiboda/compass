use std::collections::HashMap;
use std::time::{Duration, Instant};

use wreq::{Client, Proxy};
use wreq_util::Emulation;

use crate::error::{CollectError, Result};

/// EastMoney datacenter API base URL (v1 data endpoint).
pub const EM_BASE: &str = "https://datacenter-web.eastmoney.com/api/data/v1/get";
/// Chrome 142 desktop User-Agent string used for EastMoney requests.
pub const EM_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";
/// Minimum interval between EastMoney requests.
pub const EM_MIN_INTERVAL: Duration = Duration::from_secs(2);
/// Random jitter range (as a fraction) added to the minimum interval.
pub const EM_JITTER: (f64, f64) = (0.1, 0.3);
/// Maximum retry count for transient EastMoney request failures.
pub const EM_MAX_RETRIES: usize = 4;
/// Minimum interval between Sina money-flow requests (per-symbol window).
pub const SINA_MIN_INTERVAL: Duration = Duration::from_millis(100);

/// HTTP/TLS client based on `wreq` (successor of rquest), emulating Chrome 142.
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    /// Default headers sent with every EastMoney request.
    pub default_headers: HashMap<String, String>,
}

impl HttpClient {
    /// Build a client with the default header set and Chrome 142 TLS emulation.
    pub fn new() -> Result<Self> {
        let client = Client::builder().emulation(Emulation::Chrome142).build()?;
        Ok(Self {
            client,
            default_headers: default_em_headers(),
        })
    }

    /// Wrap an existing `wreq` client with the default EastMoney headers.
    pub fn with_client(client: Client) -> Self {
        Self {
            client,
            default_headers: default_em_headers(),
        }
    }

    /// Return the underlying `wreq` client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Perform a GET request and return the parsed JSON body (direct).
    pub async fn get_json(
        &self,
        url: &str,
        params: &HashMap<String, String>,
    ) -> Result<serde_json::Value> {
        self.get_json_with_proxy(url, params, None).await
    }

    /// Perform a GET request through an optional HTTP proxy and return JSON.
    pub async fn get_json_with_proxy(
        &self,
        url: &str,
        params: &HashMap<String, String>,
        proxy: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut request = self.client.get(url);
        for (k, v) in &self.default_headers {
            request = request.header(k, v);
        }
        if !params.is_empty() {
            request = request.query(&params);
        }
        if let Some(proxy_url) = proxy {
            let p = Proxy::all(crate::proxy::ProxyPool::proxy_spec(proxy_url)).map_err(|e| {
                CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
            })?;
            request = request.proxy(p);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(CollectError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.json().await?)
    }

    /// Perform a GET request through an optional HTTP proxy with extra headers
    /// and return the parsed JSON body. Extra headers override defaults.
    pub async fn get_json_with_headers_and_proxy(
        &self,
        url: &str,
        params: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        proxy: Option<&str>,
    ) -> Result<serde_json::Value> {
        let mut request = self.client.get(url);
        for (k, v) in &self.default_headers {
            request = request.header(k, v);
        }
        for (k, v) in headers {
            request = request.header(k, v);
        }
        if !params.is_empty() {
            request = request.query(&params);
        }
        if let Some(proxy_url) = proxy {
            let p = Proxy::all(crate::proxy::ProxyPool::proxy_spec(proxy_url)).map_err(|e| {
                CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
            })?;
            request = request.proxy(p);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(CollectError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.json().await?)
    }

    /// Perform a GET request through an optional HTTP proxy with extra headers
    /// and return the response body as text.
    pub async fn get_text_with_headers_and_proxy(
        &self,
        url: &str,
        params: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        proxy: Option<&str>,
    ) -> Result<String> {
        let mut request = self.client.get(url);
        for (k, v) in &self.default_headers {
            request = request.header(k, v);
        }
        for (k, v) in headers {
            request = request.header(k, v);
        }
        if !params.is_empty() {
            request = request.query(&params);
        }
        if let Some(proxy_url) = proxy {
            let p = Proxy::all(crate::proxy::ProxyPool::proxy_spec(proxy_url)).map_err(|e| {
                CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
            })?;
            request = request.proxy(p);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(CollectError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.text().await?)
    }

    /// Perform a GET request through an optional HTTP proxy with extra headers
    /// and return the raw response bytes.
    pub async fn get_bytes_with_headers_and_proxy(
        &self,
        url: &str,
        params: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        proxy: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut request = self.client.get(url);
        for (k, v) in &self.default_headers {
            request = request.header(k, v);
        }
        for (k, v) in headers {
            request = request.header(k, v);
        }
        if !params.is_empty() {
            request = request.query(&params);
        }
        if let Some(proxy_url) = proxy {
            let p = Proxy::all(crate::proxy::ProxyPool::proxy_spec(proxy_url)).map_err(|e| {
                CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
            })?;
            request = request.proxy(p);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(CollectError::HttpStatus(response.status().as_u16()));
        }
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Perform a POST form request through an optional HTTP proxy with extra
    /// headers and return the response body as text.
    pub async fn post_form_text_with_headers_and_proxy(
        &self,
        url: &str,
        params: &HashMap<String, String>,
        headers: &HashMap<String, String>,
        proxy: Option<&str>,
    ) -> Result<String> {
        let mut request = self.client.post(url).form(params);
        for (k, v) in &self.default_headers {
            request = request.header(k, v);
        }
        for (k, v) in headers {
            request = request.header(k, v);
        }
        if let Some(proxy_url) = proxy {
            let p = Proxy::all(crate::proxy::ProxyPool::proxy_spec(proxy_url)).map_err(|e| {
                CollectError::InvalidInput(format!("invalid proxy {proxy_url:?}: {e}"))
            })?;
            request = request.proxy(p);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(CollectError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.text().await?)
    }

    /// Perform a POST request and return the parsed JSON body.
    pub async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut request = self.client.post(url).json(body);
        for (k, v) in &self.default_headers {
            request = request.header(k, v);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(CollectError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.json().await?)
    }
}

fn default_em_headers() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("User-Agent".to_string(), EM_UA.to_string());
    map.insert("Accept".to_string(), "*/*".to_string());
    map.insert(
        "Accept-Language".to_string(),
        "zh-CN,zh;q=0.9,en;q=0.8".to_string(),
    );
    map.insert(
        "Referer".to_string(),
        "https://data.eastmoney.com/".to_string(),
    );
    map.insert(
        "Sec-Ch-Ua".to_string(),
        "\"Chromium\";v=\"142\", \"Google Chrome\";v=\"142\", \"Not_A Brand\";v=\"99\"".to_string(),
    );
    map.insert("Sec-Ch-Ua-Mobile".to_string(), "?0".to_string());
    map.insert("Sec-Ch-Ua-Platform".to_string(), "\"Windows\"".to_string());
    map.insert("Sec-Fetch-Dest".to_string(), "empty".to_string());
    map.insert("Sec-Fetch-Mode".to_string(), "cors".to_string());
    map.insert("Sec-Fetch-Site".to_string(), "same-site".to_string());
    map.insert("Connection".to_string(), "keep-alive".to_string());
    map
}

/// Rate limiter with jitter, mirroring Python `common.Throttle`.
#[derive(Debug)]
pub struct Throttle {
    min_interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    /// Create a throttle enforcing the given minimum interval between calls.
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last: None,
        }
    }

    /// Sleep until the min interval has elapsed since the previous call,
    /// adding a random jitter, then record the wake-up instant.
    pub async fn acquire(&mut self) {
        let now = Instant::now();
        let wait = if let Some(last) = self.last {
            let since = now.duration_since(last);
            if since < self.min_interval {
                self.min_interval - since
                    + Duration::from_millis((rand::random::<f64>() * 200.0 + 100.0) as u64)
            } else {
                Duration::from_millis((rand::random::<f64>() * 150.0) as u64)
            }
        } else {
            Duration::ZERO
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        self.last = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn throttle_respects_min_interval() {
        let mut throttle = Throttle::new(Duration::from_millis(50));
        let t0 = Instant::now();
        throttle.acquire().await;
        throttle.acquire().await;
        assert!(t0.elapsed() >= Duration::from_millis(45));
    }
}
