use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use egui_charts::model::Bar;
use rand::Rng;

use crate::data::provider::{DataError, DataProvider};
use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// SyntheticProvider — random OHLCV data for testing / demo
// ---------------------------------------------------------------------------

/// In-memory provider that generates random OHLCV data for testing and demos.
///
/// Produces 200 bars of random price movement starting from 100.0. Symbol
/// search returns a fixed set of five well-known A-share stocks.
#[allow(dead_code)]
pub struct SyntheticProvider;

impl SyntheticProvider {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DataProvider for SyntheticProvider {
    async fn fetch_bars(
        &self,
        _symbol: &str,
        _timeframe: &str,
        _range_start: DateTime<Utc>,
        range_end: DateTime<Utc>,
    ) -> Result<Vec<Bar>, DataError> {
        const COUNT: usize = 200;
        const START_PRICE: f64 = 100.0;

        let mut rng = rand::rng();
        let mut bars = Vec::with_capacity(COUNT);
        let mut price = START_PRICE;

        for i in 0..COUNT {
            let time = range_end - Duration::days((COUNT - i) as i64);

            let change: f64 = rng.random_range(-2.0..2.0);
            let open = price;
            let close = price + change;
            let high = open.max(close) + rng.random_range(0.0..1.5);
            let low = open.min(close) - rng.random_range(0.0..1.5);
            let volume = rng.random_range(1_000_000.0..10_000_000.0);

            bars.push(Bar::new(time, open, high, low, close, volume));
            price = close;
        }

        Ok(bars)
    }

    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError> {
        const SYMBOLS: &[(&str, &str)] = &[
            ("000001", "平安银行"),
            ("000002", "万科A"),
            ("600000", "浦发银行"),
            ("600036", "招商银行"),
            ("300750", "宁德时代"),
        ];

        let q = query.trim().to_lowercase();
        let results: Vec<SymbolInfo> = SYMBOLS
            .iter()
            .filter(|(code, name)| {
                code.contains(&q) || name.to_lowercase().contains(&q) || q.is_empty()
            })
            .map(|(code, name)| SymbolInfo {
                code: code.to_string(),
                name: name.to_string(),
            })
            .collect();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    fn fetch_all_start() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).unwrap()
    }

    fn fetch_all_end() -> DateTime<Utc> {
        DateTime::from_timestamp(4_000_000_000, 0).unwrap()
    }

    #[tokio::test]
    async fn fetch_bars_has_price_movement() {
        let p = SyntheticProvider;
        let bars = p
            .fetch_bars("any", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();
        assert_eq!(bars.len(), 200);
        let closes: Vec<_> = bars.iter().map(|b| b.close).collect();
        let all_same = closes.windows(2).all(|w| (w[0] - w[1]).abs() < 0.0001);
        assert!(!all_same, "random bars should have price variation");
    }

    #[tokio::test]
    async fn fetch_bars_consistent_ohlc() {
        let p = SyntheticProvider;
        let bars = p
            .fetch_bars("any", "1d", fetch_all_start(), fetch_all_end())
            .await
            .unwrap();
        for w in bars.windows(2) {
            assert!(w[0].time <= w[1].time);
        }
        for b in &bars {
            assert!(b.high >= b.open.max(b.close));
            assert!(b.low <= b.open.min(b.close));
            assert!(b.volume >= 1_000_000.0);
        }
    }

    #[tokio::test]
    async fn search_symbols_empty_query_returns_all_five() {
        let p = SyntheticProvider;
        let results = p.search_symbols("").await.unwrap();
        let codes: Vec<&str> = results.iter().map(|s| s.code.as_str()).collect();
        assert!(codes.contains(&"000001"));
        assert!(codes.contains(&"000002"));
        assert!(codes.contains(&"600000"));
        assert!(codes.contains(&"600036"));
        assert!(codes.contains(&"300750"));
    }

    #[tokio::test]
    async fn search_symbols_by_code() {
        let p = SyntheticProvider;
        let results = p.search_symbols("000001").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].code, "000001");
        assert_eq!(results[0].name, "平安银行");
    }

    #[tokio::test]
    async fn search_symbols_case_insensitive() {
        let p = SyntheticProvider;
        let by_lower = p.search_symbols("平安").await.unwrap();
        let by_upper = p.search_symbols("PINGAN").await.unwrap();
        assert!(!by_lower.is_empty());
        assert!(by_upper.is_empty()); // code/name are Chinese, English doesn't match
    }

    #[tokio::test]
    async fn search_symbols_no_match() {
        let p = SyntheticProvider;
        let results = p.search_symbols("NOTEXIST").await.unwrap();
        assert!(results.is_empty());
    }
}
