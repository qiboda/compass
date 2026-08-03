//! Whole-market market thermometer (epic #139 decisions 2/14/21).
//!
//! Five cross-sectional proxies, thresholds locked as module constants:
//!
//! 1. 沪深300 proxy — equal-weighted MA250-trend ratio of the top-300
//!    market-cap stocks, × 30.
//! 2. 中证1000 proxy — same ratio for cap ranks 801–1800, × 30.
//! 3. Limit-up count score — `min(limit_ups / 80, 1) × 15`, where a limit-up
//!    is a day-over-day change ≥ 9.8% (uniform across boards).
//! 4. Turnover score — `min(amount_trillion / 1.2, 1) × 15` over the whole
//!    market's latest-bar amounts.
//! 5. Breadth score — fraction of rising stocks × 10.
//!
//! Total ∈ [0, 100]; position bands: ≥ 80 → "80%-100%" (midpoint 90),
//! 60–80 → "40%-70%" (55), < 60 → "0%-20%" (10).
//!
//! Market cap = `total_share × close` (latest bar, raw close); the trend
//! comparison uses adjusted close against [`ma`](super::indicators::ma())
//! over 250 bars (stocks without 250 bars of history are excluded from both
//! the trend numerator and denominator). The function is pure — the caller
//! fetches and groups the data — so it is fully testable offline.

use std::collections::HashMap;

use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::{MarketThermometer, SepaIndicator};

use super::indicators::ma;

/// Limit-up count for a full turnover score (epic decision 2: bull markets
/// show > 80 limit-ups).
pub const TEMP_LIMIT_UP_FULL: usize = 80;

/// Whole-market amount (trillion yuan) for a full turnover score (epic
/// decision 2).
pub const TEMP_AMOUNT_FULL_TRILLION: f64 = 1.2;

/// Day-over-day change (%) counted as a limit-up; uniform across boards.
const TEMP_LIMIT_UP_PCT: f64 = 9.8;

/// Compute the market thermometer from per-symbol bar series and stock
/// metadata.
///
/// `bars_by_symbol` must be keyed by bare 6-digit codes with series in
/// ascending `trade_date` order; `basics_by_symbol` keyed the same way (the
/// exact shape produced by grouping
/// [`ParquetReader::fetch_cross_section`](compass_core::data::parquet::ParquetReader::fetch_cross_section)
/// and
/// [`ParquetReader::load_all_stock_basics`](compass_core::data::parquet::ParquetReader::load_all_stock_basics)
/// results — see [`crate::run_screener`]).
///
/// Breadth metrics (limit-ups, rising count, total amount) span **every**
/// symbol present in `bars_by_symbol`; the two index proxies additionally
/// require a basics row with a positive finite `total_share`. An empty or
/// degenerate market yields 0 contributions and the lowest position band —
/// never a panic.
pub fn compute_market_thermometer(
    bars_by_symbol: &HashMap<String, Vec<&CrossSectionBar>>,
    basics_by_symbol: &HashMap<String, &StockBasic>,
) -> MarketThermometer {
    // Latest-bar summary per symbol: day-over-day change (raw close), raw
    // close, latest adjusted close and its MA250, and latest amount.
    struct Summary {
        pct_change: Option<f64>,
        close: f64,
        adjclose: f64,
        ma250: Option<f64>,
        amount: f64,
    }
    let mut summaries: HashMap<&str, Summary> = HashMap::with_capacity(bars_by_symbol.len());
    for (symbol, series) in bars_by_symbol {
        let Some(latest) = series.last() else {
            continue;
        };
        if !latest.close.is_finite() || !latest.adjclose.is_finite() {
            continue;
        }
        let pct_change = if series.len() >= 2 {
            let prev = series[series.len() - 2];
            if prev.close.is_finite() && prev.close != 0.0 {
                Some((latest.close - prev.close) / prev.close * 100.0)
            } else {
                None
            }
        } else {
            None
        };
        summaries.insert(
            symbol.as_str(),
            Summary {
                pct_change,
                close: latest.close,
                adjclose: latest.adjclose,
                ma250: ma(series, 250),
                amount: if latest.amount.is_finite() {
                    latest.amount
                } else {
                    0.0
                },
            },
        );
    }

    // Market breadth ③④⑤ over every cross-section symbol.
    let mut limit_up = 0usize;
    let mut rising = 0usize;
    let mut pct_count = 0usize;
    let mut total_amount = 0.0f64;
    for s in summaries.values() {
        total_amount += s.amount;
        if let Some(pct) = s.pct_change {
            pct_count += 1;
            if pct >= TEMP_LIMIT_UP_PCT {
                limit_up += 1;
            }
            if pct > 0.0 {
                rising += 1;
            }
        }
    }

    // Index proxies ①②: rank every capped symbol, then per band count the
    // MA250 trend ratio over the band members that have one.
    let mut ranked: Vec<(f64, Option<bool>)> = Vec::new();
    for (symbol, basic) in basics_by_symbol {
        let Some(s) = summaries.get(symbol.as_str()) else {
            continue;
        };
        let Some(share) = basic.total_share else {
            continue;
        };
        if !share.is_finite() || share <= 0.0 {
            continue;
        }
        ranked.push((share * s.close, s.ma250.map(|m| s.adjclose > m)));
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    let trend_ratio = |band: &[(f64, Option<bool>)]| -> f64 {
        let denom = band.iter().filter(|(_, above)| above.is_some()).count();
        if denom == 0 {
            0.0
        } else {
            band.iter()
                .filter(|(_, above)| *above == Some(true))
                .count() as f64
                / denom as f64
        }
    };
    let band_hs300 = &ranked[..ranked.len().min(300)];
    let band_zz1000 = &ranked[800.min(ranked.len())..ranked.len().min(1800)];
    let ratio1 = trend_ratio(band_hs300);
    let ratio2 = trend_ratio(band_zz1000);

    let s1 = ratio1 * 30.0;
    let s2 = ratio2 * 30.0;
    let s3 = (limit_up as f64 / TEMP_LIMIT_UP_FULL as f64).min(1.0) * 15.0;
    let s4 = (total_amount / 1e12 / TEMP_AMOUNT_FULL_TRILLION).min(1.0) * 15.0;
    let up_ratio = if pct_count == 0 {
        0.0
    } else {
        rising as f64 / pct_count as f64
    };
    let s5 = up_ratio * 10.0;

    let score = (s1 + s2 + s3 + s4 + s5).clamp(0.0, 100.0);
    let (position, position_pct) = if score >= 80.0 {
        ("80%-100%", 90.0)
    } else if score >= 60.0 {
        ("40%-70%", 55.0)
    } else {
        ("0%-20%", 10.0)
    };

    // heat = contribution / contribution max, clamped into 0..1.
    let heat = |contribution: f64, max: f64| (contribution / max).clamp(0.0, 1.0);
    let indicators = vec![
        SepaIndicator {
            label: "沪深300趋势".to_string(),
            value_text: format!("{:.1}%", ratio1 * 100.0),
            delta_pct: None,
            heat: heat(s1, 30.0),
        },
        SepaIndicator {
            label: "中证1000趋势".to_string(),
            value_text: format!("{:.1}%", ratio2 * 100.0),
            delta_pct: None,
            heat: heat(s2, 30.0),
        },
        SepaIndicator {
            label: "涨停数".to_string(),
            value_text: format!("{limit_up} 家"),
            delta_pct: None,
            heat: heat(s3, 15.0),
        },
        SepaIndicator {
            label: "成交额".to_string(),
            value_text: format!("{:.2}万亿", total_amount / 1e12),
            delta_pct: None,
            heat: heat(s4, 15.0),
        },
        SepaIndicator {
            label: "赚钱效应".to_string(),
            value_text: format!("{:.1}%", up_ratio * 100.0),
            delta_pct: None,
            heat: heat(s5, 10.0),
        },
    ];

    MarketThermometer {
        score,
        position: position.to_string(),
        position_pct,
        indicators,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    const END: NaiveDate = NaiveDate::from_ymd_opt(2026, 7, 31).expect("valid date");

    /// 250-bar series: 249 flat bars at `base`, final bar at
    /// `base * (1 + pct/100)` carrying `amount`; `adjclose == close`.
    fn price_series(symbol: &str, base: f64, pct: f64, amount: f64) -> Vec<CrossSectionBar> {
        (0..250)
            .map(|k| {
                let close = if k == 249 {
                    base * (1.0 + pct / 100.0)
                } else {
                    base
                };
                CrossSectionBar {
                    symbol: symbol.to_string(),
                    trade_date: END - Duration::days(249 - k as i64),
                    open: close,
                    high: close,
                    low: close,
                    adjclose: close,
                    close,
                    volume: 0.0,
                    amount: if k == 249 { amount } else { 0.0 },
                }
            })
            .collect()
    }

    /// Two-bar series for degenerate-market cases (MA250 unavailable).
    fn two_bar_series(symbol: &str, base: f64, pct: f64, amount: f64) -> Vec<CrossSectionBar> {
        let closes = [base, base * (1.0 + pct / 100.0)];
        closes
            .iter()
            .enumerate()
            .map(|(i, close)| CrossSectionBar {
                symbol: symbol.to_string(),
                trade_date: END - Duration::days(1 - i as i64),
                open: *close,
                high: *close,
                low: *close,
                adjclose: *close,
                close: *close,
                volume: 0.0,
                amount: if i == 1 { amount } else { 0.0 },
            })
            .collect()
    }

    struct Market {
        bars: HashMap<String, Vec<CrossSectionBar>>,
        basics: HashMap<String, StockBasic>,
    }

    /// Build `total` stocks with deterministic cap ranks: `base = 10 +
    /// 0.01·i` and `total_share = 1e9 / (1 + pct/100)` cancel the close
    /// factor, so `cap = total_share × close ≈ 1e9 × base` is monotone in
    /// `i` — rank 0 is always index `total - 1`. `pct_by_rank` decides the
    /// final-bar change of each cap rank.
    fn build_market(total: usize, pct_by_rank: impl Fn(usize) -> f64, amount: f64) -> Market {
        let mut bars = HashMap::new();
        let mut basics = HashMap::new();
        for i in 0..total {
            let rank = total - 1 - i;
            let base = 10.0 + 0.01 * i as f64;
            let pct = pct_by_rank(rank);
            let symbol = format!("{:06}", i);
            bars.insert(symbol.clone(), price_series(&symbol, base, pct, amount));
            basics.insert(
                symbol.clone(),
                StockBasic {
                    symbol: symbol.clone(),
                    name: format!("S{i}"),
                    area: None,
                    industry: None,
                    market: None,
                    board: None,
                    full_name: None,
                    total_share: Some(1.0e9 / (1.0 + pct / 100.0)),
                    exchange: Some("SH".to_string()),
                    list_date: None,
                    delist_date: None,
                },
            );
        }
        Market { bars, basics }
    }

    fn refs(
        market: &Market,
    ) -> (
        HashMap<String, Vec<&CrossSectionBar>>,
        HashMap<String, &StockBasic>,
    ) {
        let bars: HashMap<String, Vec<&CrossSectionBar>> = market
            .bars
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().collect()))
            .collect();
        let basics: HashMap<String, &StockBasic> =
            market.basics.iter().map(|(k, v)| (k.clone(), v)).collect();
        (bars, basics)
    }

    #[test]
    fn bull_market_scores_full_heat() {
        // 2000 stocks, all limit-up and above MA250, amount 1e9 each (2.0
        // 万亿 total) → every component at its max.
        let market = build_market(2000, |_| 10.0, 1.0e9);
        let (bars, basics) = refs(&market);
        let tm = compute_market_thermometer(&bars, &basics);
        assert_eq!(tm.score, 100.0, "score {:.4}", tm.score);
        assert_eq!(tm.position, "80%-100%");
        assert_eq!(tm.position_pct, 90.0);
        assert_eq!(tm.indicators.len(), 5);
        assert!((tm.indicators[0].heat - 1.0).abs() < 1e-9);
        assert_eq!(tm.indicators[2].value_text, "2000 家");
    }

    #[test]
    fn bear_market_scores_lowest_band() {
        // All below MA250, zero limit-ups, amount 1e8 each (0.2 万亿) →
        // only the turnover score contributes: 0.2/1.2 × 15 = 2.5.
        let market = build_market(2000, |_| -5.0, 1.0e8);
        let (bars, basics) = refs(&market);
        let tm = compute_market_thermometer(&bars, &basics);
        assert!((tm.score - 2.5).abs() < 1e-9, "score {:.9}", tm.score);
        assert_eq!(tm.position, "0%-20%");
        assert_eq!(tm.position_pct, 10.0);
        // No limit-ups → component ③ is exactly 0.
        assert_eq!(tm.indicators[2].heat, 0.0);
        assert_eq!(tm.indicators[2].value_text, "0 家");
        // No rising stocks → component ⑤ is exactly 0.
        assert_eq!(tm.indicators[4].heat, 0.0);
    }

    #[test]
    fn structural_market_lands_in_middle_band() {
        // Index proxies mixed (45% above MA250 in cap ranks 1-300 and
        // 801-1800 → ① = ② = 13.5) while breadth is strong elsewhere (700
        // more limit-ups, amount 2.0 万亿, 64.25% rising) → total ≈ 63.4.
        let market = build_market(
            2000,
            |rank| {
                if rank < 300 || (800..1800).contains(&rank) {
                    if rank % 20 < 9 { 10.0 } else { -2.0 }
                } else {
                    10.0
                }
            },
            1.0e9,
        );
        let (bars, basics) = refs(&market);
        let tm = compute_market_thermometer(&bars, &basics);
        assert!(
            (60.0..80.0).contains(&tm.score),
            "score {:.4} must be in [60, 80)",
            tm.score
        );
        assert_eq!(tm.position, "40%-70%");
        assert_eq!(tm.position_pct, 55.0);
        assert_eq!(tm.indicators.len(), 5);
    }

    #[test]
    fn empty_market_never_panics_and_scores_zero() {
        let bars: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
        let basics: HashMap<String, &StockBasic> = HashMap::new();
        let tm = compute_market_thermometer(&bars, &basics);
        assert_eq!(tm.score, 0.0);
        assert_eq!(tm.position, "0%-20%");
        assert_eq!(tm.position_pct, 10.0);
        assert_eq!(tm.indicators.len(), 5);
        assert!(tm.indicators.iter().all(|i| i.heat == 0.0));
    }

    #[test]
    fn single_stock_market_never_panics() {
        // One 250-bar stock, limit-up, small amount; MA250 present → the
        // top-300 band contains it above its MA250 (① = 30), no 中证1000
        // band (fewer than 801 stocks), 1 limit-up, 100% rising:
        // 30 + 15/80 + 1e8/1e12/1.2×15 + 10 ≈ 40.19 → lowest band.
        let mut market = build_market(1, |_| 10.0, 1.0e8);
        market.basics.get_mut("000000").unwrap().total_share = Some(1.0e9);
        let (bars, basics) = refs(&market);
        let tm = compute_market_thermometer(&bars, &basics);
        let expected = 30.0 + 15.0 / 80.0 + 1.0e8 / 1e12 / 1.2 * 15.0 + 10.0;
        assert!((tm.score - expected).abs() < 1e-9, "score {:.9}", tm.score);
        assert_eq!(tm.position, "0%-20%");
        assert_eq!(tm.indicators.len(), 5);
    }

    #[test]
    fn single_stock_without_ma250_history_still_safe() {
        // Two-bar series → no MA250 → both index proxies 0 (empty trend
        // denominator), breadth still computed from the latest bar.
        let symbol = "000000".to_string();
        let mut bars: HashMap<String, Vec<CrossSectionBar>> = HashMap::new();
        bars.insert(symbol.clone(), two_bar_series(&symbol, 100.0, 10.0, 1.0e8));
        let mut basics: HashMap<String, StockBasic> = HashMap::new();
        basics.insert(
            symbol.clone(),
            StockBasic {
                symbol: symbol.clone(),
                name: "S0".to_string(),
                area: None,
                industry: None,
                market: None,
                board: None,
                full_name: None,
                total_share: Some(1.0e9),
                exchange: Some("SH".to_string()),
                list_date: None,
                delist_date: None,
            },
        );
        let bar_refs: HashMap<String, Vec<&CrossSectionBar>> = bars
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().collect()))
            .collect();
        let basic_refs: HashMap<String, &StockBasic> =
            basics.iter().map(|(k, v)| (k.clone(), v)).collect();
        let tm = compute_market_thermometer(&bar_refs, &basic_refs);
        assert!(tm.score.is_finite());
        // 1 limit-up (0.1875) + turnover 1e8→0.00125 + breadth 10 → ~10.19.
        assert!((tm.score - (15.0 / 80.0 + 1.0e8 / 1e12 / 1.2 * 15.0 + 10.0)).abs() < 1e-9);
        assert_eq!(tm.position, "0%-20%");
    }
}
