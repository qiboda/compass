use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use egui_charts::model::Bar;
use serde_json::Value;

use crate::data::provider::{DataError, DataProvider};
use crate::model::SymbolInfo;

/// EastMoney (东方财富) HTTP API data provider.
///
/// Fetches OHLCV K-line data and searches stock symbols via the public
/// EastMoney HTTP endpoints.
pub struct EastMoneyProvider {
    client: reqwest::Client,
    base_url: String,
}

impl EastMoneyProvider {
    /// Create a new provider wrapping a `reqwest::Client` and an API base URL.
    pub fn new(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }

    /// Map a user-supplied symbol to an EastMoney `secid` string (`"{market}.{code}"`).
    ///
    /// Supports explicit market prefixes for disambiguation (case-insensitive):
    /// - `sh.000001` → `1.000001` (Shanghai, 上证指数)
    /// - `sz.000001` → `0.000001` (Shenzhen, 平安银行)
    /// - `bj.8xxxxx` → `0.8xxxxx` (Beijing, 北交所)
    ///
    /// Without prefix, infers the exchange from A-share code ranges:
    /// - `6xxxxx` → Shanghai (主板 600/601/603/605, 科创板 688)
    /// - `000xxx`–`004xxx` → Shenzhen (主板; use `sh.` for SH indices)
    /// - `300xxx`, `301xxx` → Shenzhen (创业板)
    /// - `002xxx`, `003xxx` → Shenzhen
    /// - `8xxxxx` → Beijing (北交所, market 0)
    fn to_secid(symbol: &str) -> String {
        let lower = symbol.to_lowercase();

        if let Some(code) = lower.strip_prefix("sh.") {
            return format!("1.{}", code);
        }
        if let Some(code) = lower.strip_prefix("sz.") {
            return format!("0.{}", code);
        }
        if let Some(code) = lower.strip_prefix("bj.") {
            return format!("0.{}", code);
        }

        let market = if symbol.starts_with('6') { 1 } else { 0 };
        format!("{}.{}", market, symbol)
    }

    /// Convert a user-facing timeframe string into an EastMoney `klt` value.
    ///
    /// Falls back to passing the string through unchanged for unrecognised
    /// inputs (so numeric strings like `"101"` work directly).
    fn timeframe_to_klt(tf: &str) -> &str {
        match tf {
            "1m" => "1",
            "5m" => "5",
            "15m" => "15",
            "30m" => "30",
            "60m" | "1h" => "60",
            "1d" | "daily" | "day" => "101",
            "1w" | "weekly" | "week" => "102",
            "1M" | "monthly" | "month" => "103",
            other => other,
        }
    }

    /// Parse a "YYYY-MM-DD" date string from an EastMoney kline into a `DateTime<Utc>`.
    fn parse_kline_date(date_str: &str) -> Option<DateTime<Utc>> {
        let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
        let naive_dt = naive.and_hms_opt(0, 0, 0)?;
        Some(DateTime::from_naive_utc_and_offset(naive_dt, Utc))
    }
}

#[async_trait]
impl DataProvider for EastMoneyProvider {
    /// Fetch OHLCV bars for *symbol* from the EastMoney K-line HTTP API.
    ///
    /// Each kline is a comma-separated string: `"date,open,close,high,low,volume,amount,…"`.
    /// The parsed bars are returned sorted by time ascending.
    async fn fetch_bars(
        &self,
        symbol: &str,
        timeframe: &str,
        range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        let secid = Self::to_secid(symbol);
        let klt = Self::timeframe_to_klt(timeframe);
        let beg = range_start.format("%Y%m%d").to_string();
        let end = range_end.format("%Y%m%d").to_string();

        let url = format!(
            "{}/api/qt/stock/kline/get",
            self.base_url.trim_end_matches('/')
        );

        let resp = self
            .client
            .get(&url)
            .query(&[
                ("secid", secid.as_str()),
                ("klt", klt),
                ("fqt", "1"),
                ("beg", &beg),
                ("end", &end),
                ("lmt", "2000"),
                ("fields1", "f1,f2,f3,f4,f5,f6"),
                ("fields2", "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61"),
            ])
            .send()
            .await?; // reqwest::Error → DataError::Network via #[from]

        let json: Value = resp.json().await?;

        let klines = json["data"]["klines"]
            .as_array()
            .ok_or_else(|| DataError::NoData {
                symbol: symbol.to_string(),
            })?;

        if klines.is_empty() {
            return Err(DataError::NoData {
                symbol: symbol.to_string(),
            });
        }

        // Format of each kline string:
        //   date, open, close, high, low, volume, amount, …
        //   [0]   [1]   [2]    [3]   [4]  [5]
        let mut bars: Vec<Bar> = klines
            .iter()
            .filter_map(|v| {
                let line = v.as_str()?;
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() < 6 {
                    return None;
                }
                let open: f64 = parts[1].parse().ok()?;
                let close: f64 = parts[2].parse().ok()?;
                let high: f64 = parts[3].parse().ok()?;
                let low: f64 = parts[4].parse().ok()?;
                let volume: f64 = parts[5].parse().ok()?;
                let time = Self::parse_kline_date(parts[0])?;
                Some(Bar::new(time, open, high, low, close, volume))
            })
            .collect();

        if bars.is_empty() {
            return Err(DataError::NoData {
                symbol: symbol.to_string(),
            });
        }

        bars.sort_by_key(|b| b.time);
        Ok(bars)
    }

    /// Search EastMoney for stock symbols matching *query*.
    ///
    /// Returns an empty `Vec` on any parse / network failure — search is
    /// best-effort and should never propagate an error to the caller.
    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        let url = format!("{}/api/qt/clist/get", self.base_url.trim_end_matches('/'));

        let resp = match self
            .client
            .get(&url)
            .query(&[
                ("pn", "1"),
                ("pz", "20"),
                ("po", "1"),
                ("np", "1"),
                ("fltt", "2"),
                ("invt", "2"),
                ("fid", "f3"),
                ("fs", "b:DLMK014"),
                ("fields", "f12,f14"),
                ("keyword", query),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(_) => return Ok(Vec::new()),
        };

        let diff = match json["data"]["diff"].as_array() {
            Some(arr) => arr,
            None => return Ok(Vec::new()),
        };

        let results: Vec<SymbolInfo> = diff
            .iter()
            .filter_map(|item| {
                let code = item["f12"].as_str()?.to_string();
                let name = item["f14"].as_str()?.to_string();
                Some(SymbolInfo { code, name })
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::MockServer;
    use rstest::rstest;

    #[rstest]
    #[case("600519", "1.600519")] // 贵州茅台 — SH 主板
    #[case("688001", "1.688001")] // 华兴源创 — 科创板
    #[case("601318", "1.601318")] // 中国平安 — SH 主板
    #[case("000001", "0.000001")] // 平安银行 — SZ 主板 (ambiguous, defaults to SZ)
    #[case("000002", "0.000002")] // 万科A — SZ 主板
    #[case("300750", "0.300750")] // 宁德时代 — 创业板
    #[case("002415", "0.002415")] // 海康威视 — SZ
    #[case("8xxxxx", "0.8xxxxx")] // 北交所
    // Explicit prefix overrides heuristic
    #[case("sh.000001", "1.000001")] // 上证指数 — explicit SH
    #[case("SH.000001", "1.000001")] // case-insensitive
    #[case("sz.000001", "0.000001")] // explicit SZ (same as default)
    #[case("sh.688001", "1.688001")] // explicit SH (same as heuristic)
    #[case("bj.830799", "0.830799")] // 艾融软件 — 北交所
    fn to_secid_maps_correctly(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(EastMoneyProvider::to_secid(input), expected);
    }

    // ===================================================================
    // timeframe_to_klt
    // ===================================================================

    #[rstest]
    #[case("1m", "1")]
    #[case("5m", "5")]
    #[case("15m", "15")]
    #[case("30m", "30")]
    #[case("60m", "60")]
    #[case("1h", "60")]
    #[case("1d", "101")]
    #[case("daily", "101")]
    #[case("day", "101")]
    #[case("1w", "102")]
    #[case("weekly", "102")]
    #[case("week", "102")]
    #[case("1M", "103")]
    #[case("monthly", "103")]
    #[case("month", "103")]
    fn timeframe_to_klt_maps_correctly(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(EastMoneyProvider::timeframe_to_klt(input), expected);
    }

    #[test]
    fn timeframe_to_klt_passthrough_unknown() {
        assert_eq!(EastMoneyProvider::timeframe_to_klt("unknown"), "unknown");
    }

    #[test]
    fn timeframe_to_klt_passthrough_numeric() {
        assert_eq!(EastMoneyProvider::timeframe_to_klt("101"), "101");
        assert_eq!(EastMoneyProvider::timeframe_to_klt("102"), "102");
    }

    // ===================================================================
    // parse_kline_date
    // ===================================================================

    #[test]
    fn parse_kline_date_valid_ymd() {
        let dt = EastMoneyProvider::parse_kline_date("2025-07-21").unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2025-07-21");
    }

    #[test]
    fn parse_kline_date_valid_leap_day() {
        let dt = EastMoneyProvider::parse_kline_date("2024-02-29").unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2024-02-29");
    }

    #[rstest]
    #[case("")]
    #[case("not-a-date")]
    #[case("2025-13-01")]
    #[case("2025-02-29")]
    #[case("07/21/2025")]
    #[case("20250721")]
    fn parse_kline_date_invalid(#[case] input: &str) {
        assert!(
            EastMoneyProvider::parse_kline_date(input).is_none(),
            "expected None for input '{input}'"
        );
    }

    // ===================================================================
    // fetch_bars — httpmock-backed
    // ===================================================================

    fn kline(date: &str, open: f64, close: f64, high: f64, low: f64, volume: f64) -> String {
        format!("{date},{open},{close},{high},{low},{volume},13000000.00,1.50,0.80,0.10,2.30")
    }

    #[tokio::test]
    async fn fetch_bars_returns_parsed_bars() {
        let server = MockServer::start_async().await;

        let payload = serde_json::json!({
            "data": {
                "klines": [
                    kline("2025-07-21", 12.04, 12.01, 12.11, 11.95, 1_079_027.0),
                    kline("2025-07-22", 12.10, 12.20, 12.30, 12.05, 980_000.0),
                ]
            }
        });

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(payload);
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let bars = provider
            .fetch_bars("000001", "1d", start, end)
            .await
            .unwrap();
        assert_eq!(bars.len(), 2);
        assert!((bars[0].open - 12.04).abs() < 0.001);
        assert!((bars[0].close - 12.01).abs() < 0.001);
        assert!((bars[0].high - 12.11).abs() < 0.001);
        assert!((bars[0].low - 11.95).abs() < 0.001);
        assert!((bars[0].volume - 1_079_027.0).abs() < 0.001);
        assert!((bars[1].open - 12.10).abs() < 0.001);
        assert!((bars[1].close - 12.20).abs() < 0.001);
    }

    #[tokio::test]
    async fn fetch_bars_sorts_by_time_ascending() {
        let server = MockServer::start_async().await;

        let payload = serde_json::json!({
            "data": {
                "klines": [
                    kline("2025-07-23", 13.0, 13.5, 14.0, 12.8, 500_000.0),
                    kline("2025-07-21", 12.0, 12.5, 13.0, 11.8, 300_000.0),
                    kline("2025-07-22", 12.5, 13.0, 13.5, 12.3, 400_000.0),
                ]
            }
        });

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(payload);
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let bars = provider
            .fetch_bars("000001", "1d", start, end)
            .await
            .unwrap();
        assert_eq!(bars.len(), 3);
        assert!(bars[0].time < bars[1].time);
        assert!(bars[1].time < bars[2].time);
        assert_eq!(bars[0].open, 12.0);
        assert_eq!(bars[1].open, 12.5);
        assert_eq!(bars[2].open, 13.0);
    }

    #[tokio::test]
    async fn fetch_bars_shanghai_symbol() {
        let server = MockServer::start_async().await;

        let payload = serde_json::json!({
            "data": {
                "klines": [kline("2025-07-21", 1500.0, 1510.0, 1520.0, 1490.0, 50_000.0)]
            }
        });

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get")
                .query_param("secid", "1.600519");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(payload);
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let bars = provider
            .fetch_bars("600519", "1d", start, end)
            .await
            .unwrap();
        assert_eq!(bars.len(), 1);
        assert!((bars[0].open - 1500.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn fetch_bars_empty_klines_returns_no_data() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"data": {"klines": []}}));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let result = provider.fetch_bars("000001", "1d", start, end).await;
        match result {
            Err(DataError::NoData { symbol }) => assert_eq!(symbol, "000001"),
            other => panic!("expected NoData, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fetch_bars_missing_data_key_returns_no_data() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"other": true}));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let result = provider.fetch_bars("000001", "1d", start, end).await;
        assert!(matches!(result, Err(DataError::NoData { .. })));
    }

    #[tokio::test]
    async fn fetch_bars_all_kline_unparseable_returns_no_data() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"data": {"klines": ["too,short"]}}));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let result = provider.fetch_bars("000001", "1d", start, end).await;
        assert!(matches!(result, Err(DataError::NoData { .. })));
    }

    #[tokio::test]
    async fn fetch_bars_bad_json_returns_network_error() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .body("this is not json");
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let result = provider.fetch_bars("000001", "1d", start, end).await;
        assert!(
            matches!(result, Err(DataError::Network(_))),
            "expected Network error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn fetch_bars_connection_refused_returns_network_error() {
        let provider = EastMoneyProvider::new(reqwest::Client::new(), "http://127.0.0.1:1".into());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let result = provider.fetch_bars("000001", "1d", start, end).await;
        assert!(
            matches!(result, Err(DataError::Network(_))),
            "expected Network error, got {result:?}"
        );
    }

    #[tokio::test]
    async fn fetch_bars_skips_invalid_dates() {
        let server = MockServer::start_async().await;

        let payload = serde_json::json!({
            "data": {
                "klines": [
                    kline("bad-date", 1.0, 2.0, 3.0, 0.5, 100.0),
                    kline("2025-07-21", 12.04, 12.01, 12.11, 11.95, 1_079_027.0),
                ]
            }
        });

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(payload);
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let bars = provider
            .fetch_bars("000001", "1d", start, end)
            .await
            .unwrap();
        assert_eq!(bars.len(), 1, "bad-date kline should be filtered out");
        assert!((bars[0].open - 12.04).abs() < 0.001);
    }

    #[tokio::test]
    async fn fetch_bars_skips_non_numeric_fields() {
        let server = MockServer::start_async().await;

        let payload = serde_json::json!({
            "data": {
                "klines": [
                    "2025-07-21,not-a-number,12.01,12.11,11.95,1079027,x,x,x,x,x",
                    kline("2025-07-22", 13.0, 13.5, 14.0, 12.8, 500_000.0),
                ]
            }
        });

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(payload);
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(30);
        let end = chrono::Utc::now();

        let bars = provider
            .fetch_bars("000001", "1d", start, end)
            .await
            .unwrap();
        assert_eq!(bars.len(), 1);
        assert!((bars[0].open - 13.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn fetch_bars_uses_correct_klt_for_timeframe() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/stock/kline/get")
                .query_param("klt", "60")
                .query_param("secid", "0.300750");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "data": {"klines": [kline("2025-07-21", 300.0, 305.0, 310.0, 295.0, 10_000.0)]}
                }));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let start = chrono::Utc::now() - chrono::Duration::days(7);
        let end = chrono::Utc::now();

        let bars = provider
            .fetch_bars("300750", "1h", start, end)
            .await
            .unwrap();
        assert_eq!(bars.len(), 1);
    }

    // ===================================================================
    // search_symbols — httpmock-backed
    // ===================================================================

    #[tokio::test]
    async fn search_symbols_returns_parsed_results() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/qt/clist/get")
                .query_param("keyword", "平安");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "data": {
                        "diff": [
                            {"f12": "000001", "f14": "平安银行"},
                            {"f12": "601318", "f14": "中国平安"},
                        ]
                    }
                }));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());

        let results = provider.search_symbols("平安").await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].code, "000001");
        assert_eq!(results[0].name, "平安银行");
        assert_eq!(results[1].code, "601318");
        assert_eq!(results[1].name, "中国平安");
    }

    #[tokio::test]
    async fn search_symbols_empty_diff_returns_empty_vec() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/api/qt/clist/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"data": {"diff": []}}));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let results = provider.search_symbols("nonexistent").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_symbols_missing_data_returns_empty_vec() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/api/qt/clist/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({"other": true}));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let results = provider.search_symbols("test").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_symbols_bad_json_returns_empty_vec() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/api/qt/clist/get");
            then.status(200)
                .header("content-type", "application/json")
                .body("not json");
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let results = provider.search_symbols("test").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_symbols_network_error_returns_empty_vec() {
        let provider = EastMoneyProvider::new(reqwest::Client::new(), "http://127.0.0.1:1".into());
        let results = provider.search_symbols("test").await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_symbols_skips_missing_fields() {
        let server = MockServer::start_async().await;

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/api/qt/clist/get");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(serde_json::json!({
                    "data": {
                        "diff": [
                            {"f14": "no code"},
                            {"f12": "000001"},
                            {"f12": "000002", "f14": "万科A"},
                        ]
                    }
                }));
        });

        let provider = EastMoneyProvider::new(reqwest::Client::new(), server.base_url());
        let results = provider.search_symbols("test").await.unwrap();
        assert_eq!(
            results.len(),
            1,
            "only entry with both f12 and f14 should survive"
        );
        assert_eq!(results[0].code, "000002");
        assert_eq!(results[0].name, "万科A");
    }
}
