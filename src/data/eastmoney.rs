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
}
