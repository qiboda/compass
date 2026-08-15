//! SEPA quantitative backtest engine (issue #154).
//!
//! Replays the five-module scoring engine day by day via `score_sepa`
//! over a historical window, simulates a
//! TOP-N equal-weight portfolio with N-trading-day rebalancing and a
//! per-side transaction cost, and compares its equity curve against a
//! market-cap top-300 equal-weight benchmark proxy.
//!
//! All functions are pure — the caller fetches and groups the data — so
//! they are fully testable offline. Return conventions are locked to
//! avoid look-ahead and `NaN` propagation (see module-level tests).

use std::collections::HashMap;

use chrono::NaiveDate;
use compass_core::data::parquet::ParquetReader;
use compass_core::data::provider::DataError;
use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::{SepaData, SepaQuery, SepaRow};

use super::{SEPA_WINDOW_DAYS, dedup_bars, fetch_sepa_window, score_sepa};
use crate::ScreenerError;

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
            .filter(|r| r.is_finite())
        {
            sum += ret;
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

/// Compute the market-cap top-300 equal-weight benchmark daily returns.
///
/// On each date, market cap = `total_share × close` (both must be finite
/// and greater than 0); the top 300 (or fewer) are equal-weighted and the
/// day's return is the mean of their adjclose day-over-day returns
/// (non-finite skipped). Membership is decided by that day's close market
/// cap — a documented mild look-ahead inherent to index proxies (locked
/// convention, recorded in `.dsh/kb/design/backtest.md`). Days with no
/// constituents yield return 0.
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
            // Sanity guard: A-share daily moves are bounded by ±30% price
            // limits (ST 5%, main 10%, ChiNext/STAR 20%, BSE 30%). A return
            // beyond ±100% cannot be a real single-day move — it is a data
            // artifact. This guard is retained as a data-quality defense:
            // the historical cross-source duplicate rows (e.g. 000905 index
            // mixing two sources, issue #181) that once produced such
            // returns are gone after symbol prefix canonicalization, but a
            // bad row must never distort the mean.
            // Skipping the member keeps the mean representative.
            if ret.is_finite() && ret.abs() < 1.0 {
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

/// Full backtest result: daily equity curve plus summary metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct BacktestResult {
    /// Daily equity points (`start..=end` window only).
    pub points: Vec<EquityPoint>,
    /// Summary performance metrics.
    pub metrics: BacktestMetrics,
}

/// Run a backtest over `[start..end]` by replaying `score_sepa` per
/// trading day (window pre-fetched once by `fetch_sepa_window`).
///
/// Calendar: distinct trading dates from
/// `fetch_cross_section(start - 1 day, end)` (ascending). `start - 1` (or
/// the first available trading day at/before `start`) is the initial
/// position day: the position is built at its close with a single buy cost,
/// its NAV is not part of the output. Output points cover `start..=end`.
///
/// Errors: `ScreenerError` when `start > end`, when `start` is later than
/// the latest data, or when a data fetch fails. Missing/empty parquet files
/// degrade to empty rows (not an error) — the result may then have an empty
/// curve.
pub fn run_backtest(
    params: &BacktestParams,
    reader: &ParquetReader,
) -> Result<BacktestResult, ScreenerError> {
    use std::collections::BTreeSet;

    let latest = reader.latest_trade_date()?;
    let end = match params.end {
        Some(e) => e,
        None => latest.ok_or_else(|| ScreenerError::Data(no_latest_data_error()))?,
    };
    if params.start > end {
        return Err(ScreenerError::Data(start_after_end_error(
            params.start,
            end,
        )));
    }
    if let Some(l) = latest
        && params.start > l
    {
        return Err(ScreenerError::Data(start_after_data_error(params.start, l)));
    }
    if params.hold_days == 0 {
        return Err(ScreenerError::Data(invalid_param_error(
            "hold_days must be >= 1",
        )));
    }
    if params.top_n == 0 {
        return Err(ScreenerError::Data(invalid_param_error(
            "top_n must be >= 1",
        )));
    }
    if !params.cost.is_finite() || params.cost < 0.0 || params.cost >= 1.0 {
        return Err(ScreenerError::Data(invalid_param_error(
            "cost must be in [0, 1)",
        )));
    }

    // Calendar from the day before start (for day-1 returns).
    let cal_start = params.start - chrono::Duration::days(1);
    let all_bars = reader.fetch_cross_section(cal_start, end)?;
    // Dedup keeps the last row per (symbol, date) as a data-quality
    // defense: symbol prefix canonicalization (issue #181) fixed the
    // historical cross-source duplicate rows (index codes like 000905
    // mixing two sources), but a duplicate row in source data would still
    // otherwise produce absurd day-over-day returns.
    let all_bars = dedup_bars(all_bars);
    let calendar: Vec<NaiveDate> = all_bars
        .iter()
        .map(|b| b.trade_date)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    // Per-day ranked rows via score_sepa. The full scoring window is
    // pre-fetched once (fetch_sepa_window) and each calendar day slices
    // `[now - SEPA_WINDOW_DAYS, now]` from it in memory — the original
    // per-day run_sepa re-read 7 parquet datasets per day (~3s/day, 93%
    // I/O), which made a full-year backtest take 20+ minutes.
    let query = SepaQuery {
        top_n: params.top_n,
    };
    let window = fetch_sepa_window(
        reader,
        cal_start - chrono::Duration::days(SEPA_WINDOW_DAYS),
        end,
    )?;
    let scoring_started = std::time::Instant::now();
    let mut sepa_datas: Vec<SepaData> = Vec::with_capacity(calendar.len());
    for &date in &calendar {
        let t = std::time::Instant::now();
        sepa_datas.push(score_sepa(&query, &window, date)?);
        tracing::debug!(date = %date, day_ms = t.elapsed().as_millis(), "score_sepa day");
    }
    tracing::info!(
        days = calendar.len(),
        scoring_ms = scoring_started.elapsed().as_millis(),
        avg_day_ms = scoring_started.elapsed().as_millis() / calendar.len().max(1) as u128,
        "backtest scoring phase"
    );
    // Borrowed view of the owned rows, kept alive by `sepa_datas` for the
    // duration of simulate_portfolio.
    let ranked_daily: Vec<(NaiveDate, Vec<&SepaRow>)> = calendar
        .iter()
        .zip(sepa_datas.iter())
        .map(|(date, data)| (*date, data.rows.iter().collect()))
        .collect();

    // Daily returns from the same window: symbol -> date -> day return.
    let mut daily_returns: HashMap<String, HashMap<NaiveDate, f64>> = HashMap::new();
    for (symbol, series) in group_by_symbol(&all_bars) {
        let mut m = HashMap::new();
        let mut prev: Option<&CrossSectionBar> = None;
        for bar in series {
            let r = match prev {
                None => 0.0, // first calendar day has return 0
                Some(p)
                    if p.adjclose.is_finite() && bar.adjclose.is_finite() && p.adjclose != 0.0 =>
                {
                    bar.adjclose / p.adjclose - 1.0
                }
                Some(_) => 0.0,
            };
            m.insert(bar.trade_date, r);
            prev = Some(bar);
        }
        daily_returns.insert(symbol, m);
    }

    let (nav, rebalances) = simulate_portfolio(&ranked_daily, &daily_returns, params);
    let out_dates: Vec<NaiveDate> = calendar
        .iter()
        .copied()
        .filter(|d| *d >= params.start)
        .collect();

    // Benchmark daily returns on the same dates.
    let basics = reader.load_all_stock_basics()?;
    let basics_by_symbol: HashMap<String, &StockBasic> =
        basics.iter().map(|b| (b.symbol.clone(), b)).collect();
    let bars_by_symbol = group_by_symbol(&all_bars);
    let bench_returns = compute_benchmark_returns(&bars_by_symbol, &basics_by_symbol, &calendar);

    // Strategy NAV per output date, plus benchmark NAV compounded from 1.0.
    let nav_by_date: HashMap<NaiveDate, f64> =
        calendar.iter().copied().zip(nav.iter().copied()).collect();
    let mut bench_nav = 1.0;
    let mut bench_nav_by_date: HashMap<NaiveDate, f64> = HashMap::new();
    for &date in &calendar {
        bench_nav *= 1.0 + bench_returns.get(&date).copied().unwrap_or(0.0);
        bench_nav_by_date.insert(date, bench_nav);
    }

    let mut points = Vec::with_capacity(out_dates.len());
    for date in &out_dates {
        points.push(EquityPoint {
            trade_date: *date,
            strategy_nav: nav_by_date.get(date).copied().unwrap_or(1.0),
            benchmark_nav: bench_nav_by_date.get(date).copied().unwrap_or(1.0),
        });
    }

    let strat_nav: Vec<f64> = points.iter().map(|p| p.strategy_nav).collect();
    let bench_series: Vec<f64> = points.iter().map(|p| p.benchmark_nav).collect();
    // `rebalances` are indices into the full-calendar NAV (simulate_portfolio
    // includes the initial position day at index 0); the output window drops
    // `k` calendar days before `start`, so shift the indices into output
    // coordinates before computing period metrics (review #154 off-by-one).
    let k = calendar
        .iter()
        .position(|d| *d >= params.start)
        .unwrap_or(calendar.len());
    let output_rebalances: Vec<usize> = rebalances.iter().map(|&i| i.saturating_sub(k)).collect();
    let metrics = compute_metrics(&strat_nav, &out_dates, &output_rebalances, &bench_series);

    Ok(BacktestResult { points, metrics })
}

/// Group bars by symbol preserving ascending date order.
fn group_by_symbol(bars: &[CrossSectionBar]) -> HashMap<String, Vec<&CrossSectionBar>> {
    let mut m: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
    for b in bars {
        m.entry(b.symbol.clone()).or_default().push(b);
    }
    m
}

/// Serialize the equity curve to CSV (header + one row per point).
///
/// Columns: `trade_date,strategy_nav,benchmark_nav`. Dates use `%Y-%m-%d`;
/// numbers use up to 6 decimals without exponent notation.
pub fn equity_csv(points: &[EquityPoint]) -> String {
    let mut out = String::from("trade_date,strategy_nav,benchmark_nav\n");
    for p in points {
        out.push_str(&format!(
            "{},{},{}\n",
            p.trade_date.format("%Y-%m-%d"),
            fmt_double(p.strategy_nav),
            fmt_double(p.benchmark_nav)
        ));
    }
    out
}

/// Format a double: up to 6 decimals, no exponent, `.1` for integral values.
fn fmt_double(v: f64) -> String {
    if !v.is_finite() {
        return String::from("0");
    }
    if v == v.trunc() {
        format!("{v:.1}")
    } else {
        format!("{v:.6}")
    }
}

fn no_latest_data_error() -> DataError {
    DataError::Parse("backtest: no trade data available".to_string())
}

fn start_after_end_error(start: NaiveDate, end: NaiveDate) -> DataError {
    DataError::Parse(format!("backtest: start {start} is after end {end}"))
}

fn start_after_data_error(start: NaiveDate, latest: NaiveDate) -> DataError {
    DataError::Parse(format!(
        "backtest: start {start} is after latest data {latest}"
    ))
}

fn invalid_param_error(msg: &str) -> DataError {
    DataError::Parse(format!("backtest: invalid parameter: {msg}"))
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
            industry_en: None,
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
    fn ranked<'a>(owned: &'a [Vec<SepaRow>], dates: &[&str]) -> Vec<(NaiveDate, Vec<&'a SepaRow>)> {
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
            (
                "A",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.10),
                    ("2025-01-06", 0.05),
                ],
            ),
            (
                "B",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", -0.05),
                    ("2025-01-06", 0.15),
                ],
            ),
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
            (
                "A",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.0),
                    ("2025-01-06", 0.0),
                ],
            ),
            (
                "B",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.0),
                    ("2025-01-06", 0.0),
                ],
            ),
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
        let days = ranked(
            &owned,
            &["2025-01-02", "2025-01-03", "2025-01-06", "2025-01-07"],
        );
        let rets = returns(&[
            (
                "A",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.0),
                    ("2025-01-06", 0.0),
                    ("2025-01-07", 0.10),
                ],
            ),
            (
                "B",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.0),
                    ("2025-01-06", 0.0),
                    ("2025-01-07", 0.10),
                ],
            ),
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
            (
                "A",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.05),
                    ("2025-01-06", 0.0),
                ],
            ),
            (
                "B",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.05),
                    ("2025-01-06", 0.0),
                ],
            ),
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

    /// Metrics: NAV [1.0, 1.1, 0.99, 1.2] → cumulative 0.2, max drawdown
    /// 0.1; 3 periods [win, loss, win] with rebalance_indices=[1,2] → 2/3.
    #[test]
    fn metrics_hand_calculated() {
        let nav = vec![1.0, 1.1, 0.99, 1.2];
        let dates = vec![
            NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap(),
            NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap(),
            NaiveDate::parse_from_str("2025-01-06", "%Y-%m-%d").unwrap(),
            NaiveDate::parse_from_str("2025-01-07", "%Y-%m-%d").unwrap(),
        ];
        let bench = vec![1.0, 1.05, 1.02, 1.1];
        let m = compute_metrics(&nav, &dates, &[1, 2], &bench);
        assert!((m.cumulative_return - 0.2).abs() < 1e-12);
        assert!((m.max_drawdown - 0.1).abs() < 1e-12);
        assert!((m.win_rate - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(m.rebalance_count, 2);
        assert!((m.benchmark_cumulative_return - 0.1).abs() < 1e-12);
        assert!((m.excess_return - 0.1).abs() < 1e-12);
        assert!(m.profit_loss_ratio > 0.0);
    }

    /// Regression (review #154): `simulate_portfolio` returns rebalance
    /// indices in full-calendar coordinates (index 0 = initial position
    /// day), but `run_backtest` passes the output-window NAV (initial day
    /// dropped, k ≥ 1 when `start` is not the first calendar day) to
    /// `compute_metrics`. Indices must be shifted by k — otherwise period
    /// boundaries are misaligned and win rate / profit-loss ratio are
    /// systematically wrong. Locked with the Goal-review scenario: 7-day
    /// calendar, hold_days=4, k=1 → true win rate 0.5, unshifted → 0.0.
    #[test]
    fn metrics_period_boundaries_shifted_by_initial_day() {
        let dates = [
            "2025-01-02",
            "2025-01-03",
            "2025-01-06",
            "2025-01-07",
            "2025-01-08",
            "2025-01-09",
            "2025-01-10",
        ];
        let owned = vec![
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
            owned_rows(&[("A", 90.0), ("B", 80.0)]),
        ];
        let days = ranked(&owned, &dates);
        // A and B share identical returns; holdings = A+B (equal weight).
        // d0 (initial day) 0; d1 +20%; d2 +10%; d3 0; d4 0 (rebalance at
        // close, cost 0); d5 -10%; d6 0.
        let rets = returns(&[
            (
                "A",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.20),
                    ("2025-01-06", 0.10),
                    ("2025-01-07", 0.0),
                    ("2025-01-08", 0.0),
                    ("2025-01-09", -0.10),
                    ("2025-01-10", 0.0),
                ],
            ),
            (
                "B",
                &[
                    ("2025-01-02", 0.0),
                    ("2025-01-03", 0.20),
                    ("2025-01-06", 0.10),
                    ("2025-01-07", 0.0),
                    ("2025-01-08", 0.0),
                    ("2025-01-09", -0.10),
                    ("2025-01-10", 0.0),
                ],
            ),
        ]);
        let (nav, reb) = simulate_portfolio(&days, &rets, &params("2025-01-02", 4, 0.0));
        // nav: [1.0, 1.2, 1.32, 1.32, 1.32, 1.188, 1.188]; rebalance at
        // calendar index 4 (d4 close).
        assert_eq!(reb, vec![4]);
        assert!((nav[4] - 1.32).abs() < 1e-12);

        // run_backtest drops the initial day (k=1): output NAV = nav[1..].
        let k = 1;
        let out_nav: Vec<f64> = nav.iter().copied().skip(k).collect();
        assert_eq!(out_nav.len(), 6);
        let out_dates: Vec<NaiveDate> = dates[1..]
            .iter()
            .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap())
            .collect();
        let bench = vec![1.0; out_nav.len()];

        // Fixed (shifted): boundaries [3, 5] → period 1 = nav[1..4] = +10%
        // (win), period 2 = nav[4..6] = -10% (loss) → win rate 0.5.
        let shifted: Vec<usize> = reb.iter().map(|&i| i - k).collect();
        let m_fixed = compute_metrics(&out_nav, &out_dates, &shifted, &bench);
        assert!(
            (m_fixed.win_rate - 0.5).abs() < 1e-12,
            "shifted win rate expected 0.5, got {}",
            m_fixed.win_rate
        );

        // Buggy (unshifted): boundaries [4, 5] → period 1 = nav[1..5] =
        // -1%, period 2 = nav[5..6] = 0% → both losses → win rate 0.0.
        let m_bug = compute_metrics(&out_nav, &out_dates, &reb, &bench);
        assert!(
            (m_bug.win_rate - 0.0).abs() < 1e-12,
            "unshifted win rate expected 0.0, got {}",
            m_bug.win_rate
        );
    }

    /// Metrics: empty and single-point series return zeros without panic;
    /// all-win periods yield profit/loss ratio 0.
    #[test]
    fn metrics_empty_and_all_win() {
        let m = compute_metrics(&[], &[], &[], &[]);
        assert_eq!(m.cumulative_return, 0.0);
        assert_eq!(m.annualized_return, 0.0);
        assert_eq!(m.win_rate, 0.0);
        assert_eq!(m.max_drawdown, 0.0);

        let nav = vec![1.0, 1.1, 1.21];
        let dates = vec![
            NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap(),
            NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap(),
            NaiveDate::parse_from_str("2025-01-06", "%Y-%m-%d").unwrap(),
        ];
        let bench = vec![1.0, 1.0, 1.0];
        // One period 0→2, win → win_rate 1.0, all-win → PL ratio 0.
        let m = compute_metrics(&nav, &dates, &[], &bench);
        assert!((m.win_rate - 1.0).abs() < 1e-12);
        assert_eq!(m.profit_loss_ratio, 0.0);
    }

    /// Benchmark proxy: 3 stocks with market caps [1e9, 2e9, 3e9]; top 2 by
    /// cap (equal weight) have day returns [0.1, 0.2] → benchmark 0.15.
    fn mk_bar(symbol: &str, date: NaiveDate, close: f64) -> CrossSectionBar {
        CrossSectionBar {
            symbol: symbol.to_string(),
            trade_date: date,
            open: close,
            high: close,
            low: close,
            adjclose: close,
            close,
            volume: 0.0,
            amount: 0.0,
        }
    }

    /// Deduplication: duplicate (symbol, date) rows keep the last row.
    #[test]
    fn dedup_bars_keeps_last_row_per_symbol_date() {
        let d1 = NaiveDate::from_ymd_opt(2025, 1, 2).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2025, 1, 3).unwrap();
        let bars = vec![
            mk_bar("A", d1, 10.0),
            mk_bar("A", d1, 99.0),
            mk_bar("A", d2, 11.0),
            mk_bar("B", d1, 5.0),
        ];
        let deduped = dedup_bars(bars);
        assert_eq!(deduped.len(), 3);
        assert_eq!(deduped[0].adjclose, 99.0, "last row for (A, d1) wins");
        assert_eq!(deduped[1].adjclose, 11.0);
        assert_eq!(deduped[2].adjclose, 5.0);
    }

    fn mk_basic(symbol: &str, share: f64, list: NaiveDate) -> StockBasic {
        StockBasic {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            name_en: None,
            area: None,
            industry: Some("测试".to_string()),
            industry_en: None,
            market: Some("主板".to_string()),
            board: Some("主板".to_string()),
            full_name: Some(symbol.to_string()),
            total_share: Some(share),
            list_date: Some(list),
            delist_date: None,
        }
    }

    #[test]
    fn benchmark_top_two_equal_weight() {
        let d1 = NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap();
        let d0 = NaiveDate::parse_from_str("2025-01-01", "%Y-%m-%d").unwrap();
        // Owned bars at function scope so references outlive the map.
        let a0 = mk_bar("A", d0, 10.0);
        let a1 = mk_bar("A", d1, 11.0); // A return 0.1
        let b0 = mk_bar("B", d0, 20.0);
        let b1 = mk_bar("B", d1, 24.0); // B return 0.2
        let c0 = mk_bar("C", d0, 30.0);
        let c1 = mk_bar("C", d1, 30.0); // C return 0.0
        let mut bars: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
        bars.insert("A".to_string(), vec![&a0, &a1]);
        bars.insert("B".to_string(), vec![&b0, &b1]);
        bars.insert("C".to_string(), vec![&c0, &c1]);

        let ba = mk_basic("A", 1e9, d0);
        let bb = mk_basic("B", 2e9, d0);
        let bc = mk_basic("C", 3e9, d0);
        let mut basics: HashMap<String, &StockBasic> = HashMap::new();
        basics.insert("A".to_string(), &ba);
        basics.insert("B".to_string(), &bb);
        basics.insert("C".to_string(), &bc);

        let rets = compute_benchmark_returns(&bars, &basics, &[d1]);
        // Market caps on d1: A=1e9×11=1.1e10, B=2e9×24=4.8e10,
        // C=3e9×30=9.0e10 → top-2 = C, B; returns C=0.0, B=0.2 → mean 0.10.
        let r = rets.get(&d1).copied().unwrap_or(f64::NAN);
        assert!((r - 0.10).abs() < 1e-12, "expected 0.10, got {r}");
    }

    /// Benchmark: non-finite/zero close or total_share excluded; no
    /// constituents on a date → return 0 (not NaN).
    #[test]
    fn benchmark_edge_cases() {
        let d1 = NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap();
        let d0 = NaiveDate::parse_from_str("2025-01-01", "%Y-%m-%d").unwrap();
        // X has zero close on d1 (excluded); Y has NaN total_share (excluded).
        let x0 = mk_bar("X", d0, 10.0);
        let x1 = mk_bar("X", d1, 0.0);
        let y0 = mk_bar("Y", d0, 10.0);
        let y1 = mk_bar("Y", d1, 10.0);
        let mut bars: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
        bars.insert("X".to_string(), vec![&x0, &x1]);
        bars.insert("Y".to_string(), vec![&y0, &y1]);

        let bx = mk_basic("X", 1e9, d0);
        let by = mk_basic("Y", f64::NAN, d0);
        let mut basics: HashMap<String, &StockBasic> = HashMap::new();
        basics.insert("X".to_string(), &bx);
        basics.insert("Y".to_string(), &by);

        let rets = compute_benchmark_returns(&bars, &basics, &[d1]);
        let r = rets.get(&d1).copied().unwrap_or(f64::NAN);
        assert!(r == 0.0, "expected 0.0 (no constituents), got {r}");

        // Empty bars → empty output, no panic.
        let empty: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
        let rets2 = compute_benchmark_returns(&empty, &basics, &[d1]);
        assert_eq!(rets2.get(&d1), Some(&0.0));
    }

    /// Benchmark sanity guard: a member whose day-over-day return exceeds
    /// ±100% (impossible for real A-share daily moves, bounded by ±30%
    /// limits) is a cross-source data artifact and must be skipped, not
    /// averaged into the mean (issue #181 index-code mixing).
    #[test]
    fn benchmark_skips_absurd_returns() {
        let d1 = NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap();
        let d0 = NaiveDate::parse_from_str("2025-01-01", "%Y-%m-%d").unwrap();
        // A: normal +10% move. Z: 21.5 → 9028.93 (cross-source, +41895%).
        let a0 = mk_bar("A", d0, 10.0);
        let a1 = mk_bar("A", d1, 11.0);
        let z0 = mk_bar("Z", d0, 21.5);
        let z1 = mk_bar("Z", d1, 9028.93);
        let mut bars: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
        bars.insert("A".to_string(), vec![&a0, &a1]);
        bars.insert("Z".to_string(), vec![&z0, &z1]);

        let ba = mk_basic("A", 1e9, d0);
        let bz = mk_basic("Z", 1e9, d0);
        let mut basics: HashMap<String, &StockBasic> = HashMap::new();
        basics.insert("A".to_string(), &ba);
        basics.insert("Z".to_string(), &bz);

        let rets = compute_benchmark_returns(&bars, &basics, &[d1]);
        let r = rets.get(&d1).copied().unwrap_or(f64::NAN);
        assert!(
            (r - 0.10).abs() < 1e-12,
            "Z's +41895% must be skipped, only A's +10% averaged, got {r}"
        );
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
    /// CSV serialization: header + rows, format `%Y-%m-%d`, ≤6 decimals.
    #[test]
    fn equity_csv_format() {
        let pts = vec![
            EquityPoint {
                trade_date: NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap(),
                strategy_nav: 1.0,
                benchmark_nav: 1.0,
            },
            EquityPoint {
                trade_date: NaiveDate::parse_from_str("2025-01-03", "%Y-%m-%d").unwrap(),
                strategy_nav: 1.123456789,
                benchmark_nav: 1.0005,
            },
        ];
        let csv = equity_csv(&pts);
        let lines: Vec<&str> = csv.trim().split('\n').collect();
        assert_eq!(lines[0], "trade_date,strategy_nav,benchmark_nav");
        assert_eq!(lines[1], "2025-01-02,1.0,1.0");
        assert_eq!(lines[2], "2025-01-03,1.123457,1.000500");
    }

    /// Integration: run_backtest over a 10-trading-day fixture with 3
    /// stocks; asserts points.len()==10 and points[0] numeric convention.
    #[test]
    fn run_backtest_integration() {
        use compass_core::data::parquet::ParquetReader;
        use duckdb::Connection;
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let conn = Connection::open_in_memory().expect("duckdb");

        // 10 trading days ending 2025-01-14 (weekday-only), 3 stocks with
        // engineered closes so SEPA ranking is stable (A highest score).
        // Prices rise ~1%/day so run_sepa's momentum/trend modules score
        // every symbol positively.
        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        let dates = [
            "2024-12-30",
            "2024-12-31",
            "2025-01-02",
            "2025-01-03",
            "2025-01-06",
            "2025-01-07",
            "2025-01-08",
            "2025-01-09",
            "2025-01-10",
            "2025-01-13",
            "2025-01-14",
        ];
        for (i, d) in dates.iter().enumerate() {
            for (sym, base) in [("600001", 10.0), ("600002", 20.0), ("600003", 30.0)] {
                let close = base * (1.0 + 0.01 * i as f64);
                conn.execute(
                    "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        sym,
                        d,
                        close - 0.01,
                        close,
                        close - 0.02,
                        close,
                        close,
                        5e7,
                        5e8
                    ],
                )
                .expect("insert daily");
            }
        }
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_daily.parquet").display()
        ))
        .expect("copy daily");

        conn.execute_batch(
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, exchange VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
        )
        .expect("create basic");
        for (sym, name) in [("600001", "A"), ("600002", "B"), ("600003", "C")] {
            conn.execute(
                "INSERT INTO basic VALUES (?, ?, ?, '2024-01-01', NULL, '主板', ?, 1e9, '测试', NULL)",
                duckdb::params![sym, name, "SH", name],
            )
            .expect("insert basic");
        }
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_basic.parquet").display()
        ))
        .expect("copy basic");

        let reader = ParquetReader::new(tmp.path()).expect("reader");
        let params = BacktestParams {
            start: NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap(),
            end: Some(NaiveDate::parse_from_str("2025-01-14", "%Y-%m-%d").unwrap()),
            top_n: 2,
            hold_days: 5,
            cost: 0.0,
        };
        let result = run_backtest(&params, &reader).expect("backtest runs");
        // 9 output days: 2025-01-02 .. 2025-01-14 (excl. initial day).
        assert_eq!(result.points.len(), 9);
        assert_eq!(
            result.points[0].trade_date,
            NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap()
        );
        // cal_start = start - 1 = 2025-01-01 (holiday): the first trading
        // day in the fetch window is start itself, so the initial position
        // day IS the first output day (k=0) and points[0].strategy_nav =
        // 1.0 * (1 - cost) with no day-1 return. cost=0 → 1.0.
        assert!(result.points[0].strategy_nav > 0.0);
        assert!(result.points[0].benchmark_nav > 0.0);
        assert!(result.metrics.cumulative_return > 0.0);
    }

    /// Integration: start > end and start after latest data both error.
    #[test]
    fn run_backtest_validation_errors() {
        use compass_core::data::parquet::ParquetReader;
        use duckdb::Connection;
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let conn = Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        conn.execute(
            "INSERT INTO daily VALUES ('600001', '2025-01-02', 10, 10, 10, 10, 10, 5e7, 5e8)",
            duckdb::params![],
        )
        .expect("insert");
        conn.execute_batch(&format!(
            "COPY daily TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_daily.parquet").display()
        ))
        .expect("copy");
        let reader = ParquetReader::new(tmp.path()).expect("reader");

        // start > end → Err.
        let p1 = BacktestParams {
            start: NaiveDate::parse_from_str("2025-02-01", "%Y-%m-%d").unwrap(),
            end: Some(NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap()),
            ..BacktestParams::default()
        };
        assert!(run_backtest(&p1, &reader).is_err());

        // start after latest data (explicit end given) → Err.
        let p2 = BacktestParams {
            start: NaiveDate::parse_from_str("2026-01-01", "%Y-%m-%d").unwrap(),
            end: Some(NaiveDate::parse_from_str("2026-06-01", "%Y-%m-%d").unwrap()),
            ..BacktestParams::default()
        };
        assert!(run_backtest(&p2, &reader).is_err());

        // Degenerate params → Err (review #154 MINOR): hold_days=0 and
        // out-of-range cost would produce nonsense NAVs.
        let p3 = BacktestParams {
            hold_days: 0,
            ..BacktestParams::default()
        };
        let e3 = run_backtest(&p3, &reader).expect_err("hold_days=0 rejected");
        assert!(
            format!("{e3:?}").contains("hold_days"),
            "error mentions hold_days, got {e3:?}"
        );
        for bad_cost in [-0.1, 1.0, f64::NAN] {
            let p4 = BacktestParams {
                cost: bad_cost,
                ..BacktestParams::default()
            };
            let e4 = run_backtest(&p4, &reader).expect_err("bad cost rejected");
            assert!(
                format!("{e4:?}").contains("cost"),
                "error mentions cost, got {e4:?}"
            );
        }
        let p5 = BacktestParams {
            top_n: 0,
            ..BacktestParams::default()
        };
        let e5 = run_backtest(&p5, &reader).expect_err("top_n=0 rejected");
        assert!(
            format!("{e5:?}").contains("top_n"),
            "error mentions top_n, got {e5:?}"
        );
    }

    /// Integration: missing parquet (empty fixture) with explicit end →
    /// Ok with empty points (run_sepa degrades, no panic).
    #[test]
    fn run_backtest_empty_fixture_degrades() {
        use compass_core::data::parquet::ParquetReader;
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let reader = ParquetReader::new(tmp.path()).expect("reader");
        let params = BacktestParams {
            start: NaiveDate::parse_from_str("2025-01-02", "%Y-%m-%d").unwrap(),
            end: Some(NaiveDate::parse_from_str("2025-01-14", "%Y-%m-%d").unwrap()),
            top_n: 2,
            hold_days: 5,
            cost: 0.0,
        };
        let result = run_backtest(&params, &reader).expect("empty fixture degrades to Ok");
        assert!(result.points.is_empty());
    }
}

/// Performance metrics over a strategy equity curve (locked decision 8).
#[derive(Debug, Clone, PartialEq)]
pub struct BacktestMetrics {
    /// Cumulative return: `last/first - 1` (0 for empty/single point).
    pub cumulative_return: f64,
    /// Annualized return over 252 trading days (0 when no return period).
    pub annualized_return: f64,
    /// Win rate: fraction of rebalance periods with positive return.
    pub win_rate: f64,
    /// Profit/loss ratio: mean win / mean loss; 0 when no losing period.
    pub profit_loss_ratio: f64,
    /// Maximum peak-to-trough drawdown of the NAV series.
    pub max_drawdown: f64,
    /// Number of rebalances performed.
    pub rebalance_count: usize,
    /// Benchmark cumulative return (same convention as strategy).
    pub benchmark_cumulative_return: f64,
    /// Strategy cumulative minus benchmark cumulative.
    pub excess_return: f64,
    /// Strategy annualized minus benchmark annualized.
    pub annualized_excess: f64,
}

/// Number of trading days used for annualization (A-share convention).
pub const TRADING_DAYS_PER_YEAR: f64 = 252.0;

/// Compute performance metrics from the strategy NAV series, the output
/// dates, the rebalance indices and the benchmark NAV series.
///
/// Period boundaries for win rate / profit-loss ratio are derived from
/// `rebalance_indices` (indices into `nav`): the first period runs from
/// index 0 to the first rebalance, middle periods between consecutive
/// rebalances, and the last period includes the truncated tail. Each period
/// return = `nav[end]/nav[start] - 1` (boundary costs are already inside
/// the NAV series). `nav` and `benchmark_nav` must have equal length.
pub fn compute_metrics(
    nav: &[f64],
    dates: &[NaiveDate],
    rebalance_indices: &[usize],
    benchmark_nav: &[f64],
) -> BacktestMetrics {
    debug_assert_eq!(nav.len(), benchmark_nav.len());

    let cumulative = cumulative_return(nav);
    let annualized = annualize(cumulative, dates.len());
    let (win_rate, profit_loss_ratio) = period_stats(nav, rebalance_indices);

    let benchmark_cumulative = cumulative_return(benchmark_nav);
    let benchmark_annualized = annualize(benchmark_cumulative, dates.len());

    BacktestMetrics {
        cumulative_return: cumulative,
        annualized_return: annualized,
        win_rate,
        profit_loss_ratio,
        max_drawdown: max_drawdown(nav),
        rebalance_count: rebalance_indices.len(),
        benchmark_cumulative_return: benchmark_cumulative,
        excess_return: cumulative - benchmark_cumulative,
        annualized_excess: annualized - benchmark_annualized,
    }
}

/// `last/first - 1`; 0 for empty or single-point series.
fn cumulative_return(nav: &[f64]) -> f64 {
    if nav.len() < 2 {
        return 0.0;
    }
    let first = nav[0];
    if !first.is_finite() || first == 0.0 {
        return 0.0;
    }
    nav.last().expect("len >= 2") / first - 1.0
}

/// `(1 + cum)^(252/days) - 1`; 0 when there are no return periods.
fn annualize(cumulative: f64, days: usize) -> f64 {
    if days < 2 || cumulative <= -1.0 || !cumulative.is_finite() {
        return 0.0;
    }
    let periods = (days - 1) as f64;
    (1.0 + cumulative).powf(TRADING_DAYS_PER_YEAR / periods) - 1.0
}

/// Win rate and profit/loss ratio over rebalance periods.
fn period_stats(nav: &[f64], rebalance_indices: &[usize]) -> (f64, f64) {
    if nav.len() < 2 {
        return (0.0, 0.0);
    }
    let mut boundaries = rebalance_indices.to_vec();
    boundaries.retain(|&i| i > 0 && i < nav.len());
    boundaries.push(nav.len() - 1);

    let mut start = 0usize;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut win_sum = 0.0;
    let mut loss_sum = 0.0;
    let mut periods = 0usize;

    for &end in &boundaries {
        if end <= start {
            continue;
        }
        let a = nav[start];
        let b = nav[end];
        if a.is_finite() && b.is_finite() && a != 0.0 {
            let ret = b / a - 1.0;
            if ret > 0.0 {
                wins += 1;
                win_sum += ret;
            } else {
                losses += 1;
                loss_sum += ret;
            }
            periods += 1;
        }
        start = end;
    }

    if periods == 0 {
        return (0.0, 0.0);
    }
    let win_rate = wins as f64 / periods as f64;
    let pl = if losses == 0 {
        0.0
    } else {
        let avg_win = if wins == 0 {
            0.0
        } else {
            win_sum / wins as f64
        };
        let avg_loss = loss_sum / losses as f64;
        if avg_loss == 0.0 {
            0.0
        } else {
            avg_win / avg_loss.abs()
        }
    };
    (win_rate, pl)
}

/// Maximum peak-to-trough decline of the NAV series.
fn max_drawdown(nav: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &v in nav {
        if !v.is_finite() {
            continue;
        }
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = 1.0 - v / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}
