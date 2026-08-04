//! SEPA quantitative backtest engine (issue #154).
//!
//! Replays the five-module scoring engine day by day via
//! [`run_sepa`](super::run_sepa) over a historical window, simulates a
//! TOP-N equal-weight portfolio with N-trading-day rebalancing and a
//! per-side transaction cost, and compares its equity curve against a
//! market-cap top-300 equal-weight benchmark proxy.
//!
//! All functions are pure — the caller fetches and groups the data — so
//! they are fully testable offline. Return conventions are locked to
//! avoid look-ahead and `NaN` propagation (see module-level tests).

use std::collections::HashMap;

use chrono::NaiveDate;
use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::SepaRow;

/// Backtest window start (calendar default): 2025-01-01 (locked decision 3).
pub const DEFAULT_BACKTEST_START: &str = "2025-01-01";

/// Default holding period in trading days (locked decision 4: N = 5).
pub const DEFAULT_HOLD_DAYS: usize = 5;

/// Default per-side transaction cost (locked decision 6: 0.1% each side).
pub const DEFAULT_COST: f64 = 0.001;

/// Backtest configuration (locked grill-me decisions 3/4/6).
#[derive(Debug, Clone, PartialEq)]
pub struct BacktestParams {
    /// Window start (inclusive). The initial position is built at the
    /// trading day before `start`; `start` is the first output day.
    pub start: NaiveDate,
    /// Window end (inclusive). `None` resolves to the latest trade date
    /// inside [`crate::sepa::backtest::run_backtest`].
    pub end: Option<NaiveDate>,
    /// Portfolio size: top-N by that day's score (locked decision 4).
    pub top_n: usize,
    /// Holding period in trading days between rebalances (locked decision 4).
    pub hold_days: usize,
    /// Per-side transaction cost as a fraction (locked decision 6).
    pub cost: f64,
}

impl Default for BacktestParams {
    fn default() -> Self {
        Self {
            start: NaiveDate::parse_from_str(DEFAULT_BACKTEST_START, "%Y-%m-%d")
                .expect("static default start parses"),
            end: None,
            top_n: 50,
            hold_days: DEFAULT_HOLD_DAYS,
            cost: DEFAULT_COST,
        }
    }
}

/// One row of the daily equity curve (strategy vs benchmark).
#[derive(Debug, Clone, PartialEq)]
pub struct EquityPoint {
    /// Trading date (output window only: `start..=end`).
    pub trade_date: NaiveDate,
    /// Strategy net-asset value (1.0 at window start, net of costs).
    pub strategy_nav: f64,
    /// Benchmark net-asset value (1.0 at the day before `start`).
    pub benchmark_nav: f64,
}

/// Simulate the TOP-N equal-weight rebalancing strategy.
///
/// `ranked_daily` is the per-day ranked row list (ascending by date, each
/// entry the day's [`SepaRow`] slice — empty slices are allowed and keep the
/// NAV unchanged); `daily_returns` maps symbol → date → day-over-day return
/// (the first calendar day has return 0). Returns `(nav_series,
/// rebalance_indices)` where `nav_series[i]` is the NAV at
/// `ranked_daily[i].0` and `rebalance_indices` are the indices (into
/// `nav_series`) at which a rebalance occurs (the initial position day is
/// not a rebalance).
///
/// Return conventions (locked, no look-ahead):
/// - Day 1 (initial position day): NAV = 1.0, return 0, buy cost `cost`
///   deducted once.
/// - Day t: return = equal-weight mean of the current holdings' adjclose
///   day-over-day returns; non-finite/missing components are skipped; if all
///   are missing the day's return is 0 (never `NaN`).
/// - Rebalance day: the old portfolio's day-t return is credited first, then
///   `2 × cost` is deducted (sell old + buy new) and the top-N re-selected.
/// - Tail truncation: when fewer than `hold_days` days remain, holdings are
///   kept to the window end with no extra sell cost.
pub fn simulate_portfolio(
    ranked_daily: &[(NaiveDate, Vec<&SepaRow>)],
    daily_returns: &HashMap<String, HashMap<NaiveDate, f64>>,
    params: &BacktestParams,
) -> (Vec<f64>, Vec<usize>) {
    if ranked_daily.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let mut nav = Vec::with_capacity(ranked_daily.len());
    let mut rebalances = Vec::new();

    // Current holdings: (symbol, ...) selected at the most recent rebalance
    // (or the initial position day).
    let mut holdings: Vec<&SepaRow> = Vec::new();
    let mut last_rebalance = 0usize;

    for (i, (date, rows)) in ranked_daily.iter().enumerate() {
        if i == 0 {
            // Initial position day: build holdings, NAV 1.0, buy cost once.
            holdings = top_n_rows(rows, params.top_n);
            let mut v = 1.0;
            if params.cost > 0.0 {
                v *= 1.0 - params.cost;
            }
            nav.push(v);
            continue;
        }

        // Day t: credit the current holdings' return first.
        let day_return = holdings_return(&holdings, date, daily_returns);
        let prev = *nav.last().expect("nav non-empty after day 1");
        let mut v = prev * (1.0 + day_return);

        // Rebalance when the holding period has elapsed.
        if i - last_rebalance >= params.hold_days {
            v *= 1.0 - 2.0 * params.cost;
            holdings = top_n_rows(rows, params.top_n);
            last_rebalance = i;
            rebalances.push(i);
        }

        nav.push(v);
    }

    (nav, rebalances)
}

/// Select the top-N rows (by list order, which is already ranked).
fn top_n_rows<'a>(rows: &'a [&'a SepaRow], top_n: usize) -> Vec<&'a SepaRow> {
    rows.iter().copied().take(top_n).collect()
}

/// Equal-weight mean of the holdings' day-over-day returns on `date`,
/// skipping missing/non-finite entries; 0 when nothing is usable.
fn holdings_return(
    holdings: &[&SepaRow],
    date: &NaiveDate,
    daily_returns: &HashMap<String, HashMap<NaiveDate, f64>>,
) -> f64 {
    if holdings.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut count = 0usize;
    for row in holdings {
        if let Some(ret) = daily_returns
            .get(&row.symbol)
            .and_then(|m| m.get(date))
            .copied()
        {
            if ret.is_finite() {
                sum += ret;
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

/// Compute the market-cap top-300 equal-weight benchmark daily returns.
///
/// On each date, market cap = `total_share × close` (both must be finite and
/// > 0); the top 300 (or fewer) are equal-weighted and the day's return is
/// the mean of their adjclose day-over-day returns (non-finite skipped).
/// Membership is decided by that day's close market cap — a documented mild
/// look-ahead inherent to index proxies (locked convention, recorded in
/// `kb/design/backtest.md`). Days with no constituents yield return 0.
pub fn compute_benchmark_returns(
    bars_by_symbol: &HashMap<String, Vec<&CrossSectionBar>>,
    basics_by_symbol: &HashMap<String, &StockBasic>,
    dates: &[NaiveDate],
) -> HashMap<NaiveDate, f64> {
    let mut out = HashMap::with_capacity(dates.len());
    for &date in dates {
        // Market cap per symbol on this date.
        let mut ranked: Vec<(f64, &str)> = Vec::new();
        for (symbol, series) in bars_by_symbol {
            let Some(bar) = series.iter().find(|b| b.trade_date == date) else {
                continue;
            };
            if !bar.close.is_finite() || bar.close <= 0.0 {
                continue;
            }
            let Some(basic) = basics_by_symbol.get(symbol) else {
                continue;
            };
            let Some(share) = basic.total_share else {
                continue;
            };
            if !share.is_finite() || share <= 0.0 {
                continue;
            }
            ranked.push((share * bar.close, symbol.as_str()));
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));

        // Equal-weight mean of the top-300 members' returns.
        let mut sum = 0.0;
        let mut count = 0usize;
        for &(_, symbol) in ranked.iter().take(300) {
            let Some(series) = bars_by_symbol.get(symbol) else {
                continue;
            };
            let Some(ret) = day_return_on(series, date) else {
                continue;
            };
            if ret.is_finite() {
                sum += ret;
                count += 1;
            }
        }
        let ret = if count == 0 { 0.0 } else { sum / count as f64 };
        out.insert(date, ret);
    }
    out
}

/// Day-over-day return of `series` on `date` (previous trading day to
/// `date`), from adjusted close; `None` when either bar is missing.
fn day_return_on(series: &[&CrossSectionBar], date: NaiveDate) -> Option<f64> {
    let idx = series.iter().position(|b| b.trade_date == date)?;
    if idx == 0 {
        return None;
    }
    let cur = series[idx].adjclose;
    let prev = series[idx - 1].adjclose;
    if !cur.is_finite() || !prev.is_finite() || prev == 0.0 {
        return None;
    }
    Some(cur / prev - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_types::SepaDetails;

    fn row(symbol: &str, score: f64) -> SepaRow {
        SepaRow {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            rank: 0,
            total_score: score,
            trend: 0.0,
            theme: 0.0,
            capital: 0.0,
            pattern: 0.0,
            risk: 0.0,
            industry: "测试".to_string(),
            themes: Vec::new(),
            latest_price: 1.0,
            change_pct: 0.0,
            details: SepaDetails {
                trend: Vec::new(),
                theme: Vec::new(),
                capital: Vec::new(),
                pattern: Vec::new(),
                risk: Vec::new(),
            },
        }
    }

    fn params(start: &str, hold_days: usize, cost: f64) -> BacktestParams {
        BacktestParams {
            start: NaiveDate::parse_from_str(start, "%Y-%m-%d").unwrap(),
            end: None,
            top_n: 2,
            hold_days,
            cost,
        }
    }

    /// Ranked daily input: (date, rows) borrowed from caller-owned `owned`
    /// day slices (rows already sorted by score desc).
    fn ranked<'a>(
        owned: &'a [Vec<SepaRow>],
        dates: &[&str],
    ) -> Vec<(NaiveDate, Vec<&'a SepaRow>)> {
        assert_eq!(owned.len(), dates.len());
        owned
            .iter()
            .zip(dates)
            .map(|(rows, d)| {
                let date = NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap();
                let refs: Vec<&SepaRow> = rows.iter().collect();
                (date, refs)
            })
            .collect()
    }

    /// Build one day's owned rows from (symbol, score) pairs, sorted desc.
    fn owned_rows(syms: &[(&str, f64)]) -> Vec<SepaRow> {
        let mut v: Vec<SepaRow> = syms.iter().map(|(s, sc)| row(s, *sc)).collect();
        v.sort_by(|a, b| b.total_score.total_cmp(&a.total_score));
        v
    }

    /// Daily returns: symbol -> (date, return).
    fn returns(map: &[(&str, &[(&str, f64)])]) -> HashMap<String, HashMap<NaiveDate, f64>> {
        let mut out = HashMap::new();
        for (sym, pairs) in map {
            let m = out.entry((*sym).to_string()).or_insert_with(HashMap::new);
            for (d, r) in *pairs {
                m.insert(NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap(), *r);
            }
        }
        out
    }

    /// Case ①: 2 stocks A/B, 3 scoring days d1<d2<d3, hold_days=2, cost=0.
    /// d1 builds TOP2 equal-weight, d2 holds, d3 rebalances (re-select TOP2).
    #[test]
    fn simulate_two_stocks_three_days_no_cost() {
        let owned = vec![
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
        ];
        let days = ranked(&owned, &["2025-01-02", "2025-01-03", "2025-01-06"]);
        let rets = returns(&[
            ("A", &[("2025-01-02", 0.0), ("2025-01-03", 0.10), ("2025-01-06", 0.05)]),
            ("B", &[("2025-01-02", 0.0), ("2025-01-03", -0.05), ("2025-01-06", 0.15)]),
        ]);
        let (nav, reb) = simulate_portfolio(&days, &rets, &params("2025-01-02", 2, 0.0));
        // d1: NAV 1.0 (cost 0). d2: mean(0.10, -0.05) = 0.025 → 1.025.
        // d3 (rebalance day): old holdings A/B return mean(0.05,0.15)=0.10
        // first → 1.025*1.10 = 1.1275; then rebalance, cost 0.
        assert_eq!(nav.len(), 3);
        assert!((nav[0] - 1.0).abs() < 1e-12);
        assert!((nav[1] - 1.025).abs() < 1e-12);
        assert!((nav[2] - 1.1275).abs() < 1e-12);
        // Rebalance at index 2 (d3) only.
        assert_eq!(reb, vec![2]);
    }

    /// Case ②: cost=0.001 — initial buy day deducts 1×cost; rebalance day
    /// deducts 2×cost (sell old + buy new) after crediting the day's return.
    #[test]
    fn simulate_cost_deductions() {
        let owned = vec![
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
        ];
        let days = ranked(&owned, &["2025-01-02", "2025-01-03", "2025-01-06"]);
        let rets = returns(&[
            ("A", &[("2025-01-02", 0.0), ("2025-01-03", 0.0), ("2025-01-06", 0.0)]),
            ("B", &[("2025-01-02", 0.0), ("2025-01-03", 0.0), ("2025-01-06", 0.0)]),
        ]);
        let (nav, reb) = simulate_portfolio(&days, &rets, &params("2025-01-02", 2, 0.001));
        // d1: 1.0 * (1 - 0.001) = 0.999. d2: 0.999 (no return). d3: rebalance
        // day → 0.999 * (1 - 0.002) = 0.997002.
        assert!((nav[0] - 0.999).abs() < 1e-12);
        assert!((nav[1] - 0.999).abs() < 1e-12);
        assert!((nav[2] - 0.997002).abs() < 1e-12);
        assert_eq!(reb, vec![2]);
    }

    /// Case ③: hold_days=2 but window only 3 days — after d3 rebalance the
    /// truncated tail is held to the end with no extra sell cost.
    #[test]
    fn simulate_tail_truncation() {
        let owned = vec![
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
        ];
        let days = ranked(&owned, &["2025-01-02", "2025-01-03", "2025-01-06", "2025-01-07"]);
        let rets = returns(&[
            ("A", &[("2025-01-02", 0.0), ("2025-01-03", 0.0), ("2025-01-06", 0.0), ("2025-01-07", 0.10)]),
            ("B", &[("2025-01-02", 0.0), ("2025-01-03", 0.0), ("2025-01-06", 0.0), ("2025-01-07", 0.10)]),
        ]);
        // hold_days=3: rebalance at d4 only; d4 return credited (mean 0.10)
        // before the 2×cost → 0.999 * 1.10 * 0.998 = 1.0966... then tail ends.
        let (nav, reb) = simulate_portfolio(&days, &rets, &params("2025-01-02", 3, 0.001));
        assert_eq!(reb, vec![3]);
        let expected = 0.999 * 1.10 * (1.0 - 0.002);
        assert!((nav[3] - expected).abs() < 1e-12);
    }

    /// Case ④: an empty scoring day keeps the NAV unchanged (no rebalance,
    /// no panic); empty input yields empty output.
    #[test]
    fn simulate_empty_day_and_empty_input() {
        let owned = vec![
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            Vec::new(), // no candidates
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
        ];
        let days = ranked(&owned, &["2025-01-02", "2025-01-03", "2025-01-06"]);
        let rets = returns(&[
            ("A", &[("2025-01-02", 0.0), ("2025-01-03", 0.05), ("2025-01-06", 0.0)]),
            ("B", &[("2025-01-02", 0.0), ("2025-01-03", 0.05), ("2025-01-06", 0.0)]),
        ]);
        let (nav, reb) = simulate_portfolio(&days, &rets, &params("2025-01-02", 5, 0.0));
        // d1: 1.0. d2 (empty day): holdings A/B return mean(0.05,0.05)=0.05
        // → 1.05 (holdings persist). d3: no rebalance (i-last=2 < 5), return 0.
        assert!((nav[0] - 1.0).abs() < 1e-12);
        assert!((nav[1] - 1.05).abs() < 1e-12);
        assert!((nav[2] - 1.05).abs() < 1e-12);
        assert!(reb.is_empty());

        let empty: Vec<(NaiveDate, Vec<&SepaRow>)> = Vec::new();
        let (enav, ereb) = simulate_portfolio(&empty, &rets, &params("2025-01-02", 5, 0.0));
        assert!(enav.is_empty());
        assert!(ereb.is_empty());
    }

    /// Case ⑤: top_n limits holdings (3 ranked but top_n=2 → only 2 held).
    #[test]
    fn simulate_top_n_limits_holdings() {
        let owned = vec![
            owned_rows(&[("A", 90.0), ("B", 80.0), ("C", 70.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0), ("C", 70.0)]),
        ];
        let days = ranked(&owned, &["2025-01-02", "2025-01-03"]);
        let rets = returns(&[
            ("A", &[("2025-01-02", 0.0), ("2025-01-03", 0.10)]),
            ("B", &[("2025-01-02", 0.0), ("2025-01-03", 0.10)]),
            ("C", &[("2025-01-02", 0.0), ("2025-01-03", 0.90)]),
        ]);
        // C's 0.90 return must NOT be included (top_n=2 → A+B held):
        // d2 return = mean(0.10, 0.10) = 0.10 → 1.10.
        let (nav, _) = simulate_portfolio(&days, &rets, &params("2025-01-02", 5, 0.0));
        assert!((nav[1] - 1.10).abs() < 1e-12);
    }
}
