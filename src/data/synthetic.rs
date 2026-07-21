use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use egui_charts::model::Bar;
use rand::Rng;

use crate::data::provider::{DataError, DataProvider};
use crate::model::SymbolInfo;

// ---------------------------------------------------------------------------
// SyntheticProvider — random OHLCV data for testing / demo
// ---------------------------------------------------------------------------

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
