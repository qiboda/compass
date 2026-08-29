#![warn(missing_docs)]
//! Stock screening engine.
//!
//! `run_screener` evaluates a [`Filter`] AST against whole-market daily bars
//! (via [`ParquetReader::fetch_cross_section`]) and stock metadata (via
//! [`ParquetReader::load_all_stock_basics`]), returning a market-cap sorted,
//! capped result set.
//!
//! The [`Filter`] AST is evaluated directly by the general recursive
//! evaluator in [`screener_eval`] (Batch 3, issue #246): metadata constraints
//! against [`StockBasic`], series conditions against the daily bar slice, and
//! boolean combinators (`And`/`Or`/`Not`) — no reverse compilation, no
//! restricted accept-grammar. The legacy `ScreenerQuery` type survives in
//! compass-types only as the config/migration surface (`From<ScreenerQuery>
//! for Filter`).
//!
//! All technical indicators are computed from **adjusted close** (前复权);
//! the latest raw close is used for display price and market cap.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::{Filter, ScreenerRow};
use thiserror::Error;

pub mod screener_eval;
pub mod screener_series;
pub mod sepa;

/// Errors produced by the screening engine.
#[derive(Debug, Error)]
pub enum ScreenerError {
    /// Underlying data access failure.
    #[error("data error: {0}")]
    Data(#[from] compass_core::data::provider::DataError),
}

/// Result of a screener run.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenerResult {
    /// Matched rows, market-cap descending, capped at `MAX_RESULTS`.
    pub rows: Vec<ScreenerRow>,
    /// Total matches **before** the cap (for the "共 N 只" display).
    pub total: usize,
}

/// Maximum number of result rows returned.
pub const MAX_RESULTS: usize = 100;

/// Read window (calendar days) covering MA60 + breakout 60 + momentum 20
/// with headroom (~268 trading days in 400 calendar days).
const READ_WINDOW_DAYS: i64 = 400;

/// Evaluate `filter` against the market data behind `reader`.
///
/// Every symbol with a basics row and at least one bar is evaluated with
/// [`screener_eval::evaluate`]; matched symbols are assembled into
/// [`ScreenerRow`]s, sorted by market cap descending (symbol ascending as
/// tie-break) and capped at [`MAX_RESULTS`].
pub fn run_screener(
    filter: &Filter,
    reader: &ParquetReader,
    now: NaiveDate,
) -> Result<ScreenerResult, ScreenerError> {
    let started = std::time::Instant::now();
    let range_start = now - Duration::days(READ_WINDOW_DAYS);
    let bars = reader.fetch_cross_section(range_start, now)?;
    let basics = reader.load_all_stock_basics()?;

    // Index basics by symbol; daily-only symbols (no basics row) are dropped.
    let basics_by_symbol: HashMap<&str, &StockBasic> =
        basics.iter().map(|b| (b.symbol.as_str(), b)).collect();

    // Group bars per symbol (already ordered by symbol, trade_date).
    let mut bars_by_symbol: HashMap<&str, Vec<&CrossSectionBar>> = HashMap::new();
    for bar in &bars {
        bars_by_symbol
            .entry(bar.symbol.as_str())
            .or_default()
            .push(bar);
    }

    let mut rows: Vec<ScreenerRow> = Vec::new();
    for (symbol, basics_row) in &basics_by_symbol {
        let Some(series) = bars_by_symbol.get(symbol) else {
            // Basics row without any bars: cannot compute price/cap — drop.
            continue;
        };
        if !screener_eval::evaluate(filter, basics_row, series, now) {
            continue;
        }
        let Some(row) = assemble_row(basics_row, series) else {
            continue;
        };
        rows.push(row);
    }

    let total = rows.len();
    rows.sort_by(|a, b| {
        b.market_cap
            .total_cmp(&a.market_cap)
            .then(a.symbol.cmp(&b.symbol))
    });
    rows.truncate(MAX_RESULTS);

    tracing::debug!(
        bars_loaded = bars.len(),
        basics_loaded = basics.len(),
        matched = total,
        returned = rows.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "screener run completed"
    );

    Ok(ScreenerResult { rows, total })
}

/// Assemble a [`ScreenerRow`] for a symbol that passed the filter.
///
/// `None` only for an empty series (unreachable through `run_screener`, which
/// drops bar-less symbols before evaluating). Market cap is
/// `total_share × latest.close / 1e8` (亿元); a missing `total_share` is
/// treated as `0.0` (sorts to the bottom — symbols with missing share that
/// reach this point passed the evaluator, which only excludes them when a
/// market-cap bound is active).
fn assemble_row(basic: &StockBasic, series: &[&CrossSectionBar]) -> Option<ScreenerRow> {
    let latest = series.last()?;
    let market_cap = match basic.total_share {
        Some(total_share) => total_share * latest.close / 1e8,
        None => 0.0,
    };
    let change_20d = change_over(series, 20);
    let industry = basic.industry.clone().unwrap_or_default();
    Some(ScreenerRow {
        symbol: basic.symbol.clone(),
        name: basic.name.clone(),
        latest_price: latest.close,
        change_20d,
        market_cap,
        industry,
        industry_en: basic.industry_en.clone(),
    })
}

/// Adjusted-close return over the last `n` bars (display column; uses
/// available bars when fewer than `n`, 0.0 when fewer than 2).
fn change_over(series: &[&CrossSectionBar], n: usize) -> f64 {
    if series.len() < 2 {
        return 0.0;
    }
    let base_idx = series.len().saturating_sub(n);
    let base = series[base_idx].adjclose;
    let latest = series.last().expect("non-empty series").adjclose;
    if base == 0.0 {
        return 0.0;
    }
    (latest - base) / base * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Weekday};
    use compass_types::{
        BreakoutCondition, MaCondition, MomentumCondition, ScreenerQuery, VolumeCondition,
    };

    // --- Fixtures ----------------------------------------------------------

    /// One daily bar's values; only adjclose/close/volume are used.
    #[derive(Clone)]
    struct TestBar {
        date: String,
        close: f64,
        volume: f64,
    }

    /// One fixture stock.
    struct TestStock {
        symbol: &'static str,
        name: &'static str,
        industry: Option<&'static str>,
        board: Option<&'static str>,
        list_date: Option<&'static str>,
        delist_date: Option<&'static str>,
        total_share: Option<f64>,
        bars: Vec<TestBar>,
    }

    /// Build a tempdir with stock_daily.parquet + stock_basic.parquet.
    fn build_fixture(stocks: &[TestStock]) -> (tempfile::TempDir, ParquetReader) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");

        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        for s in stocks {
            for b in &s.bars {
                conn.execute(
                    "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        s.symbol,
                        b.date.as_str(),
                        b.close - 1.0,
                        b.close + 1.0,
                        b.close - 0.5,
                        b.close,
                        b.close,
                        b.volume,
                        0.0
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
            "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
        )
        .expect("create basic");
        for s in stocks {
            conn.execute(
                "INSERT INTO basic VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)",
                duckdb::params![
                    s.symbol,
                    s.name,
                    s.list_date,
                    s.delist_date,
                    s.board,
                    s.name,
                    s.total_share,
                    s.industry,
                ],
            )
            .expect("insert basic");
        }
        conn.execute_batch(&format!(
            "COPY basic TO '{}' (FORMAT PARQUET)",
            tmp.path().join("stock_basic.parquet").display()
        ))
        .expect("copy basic");

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        (tmp, reader)
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    /// Weekday-only daily bars ending at `end` (inclusive), values from `closes`.
    fn daily_series(end: &str, closes: &[f64], volume: f64) -> Vec<TestBar> {
        let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
        let mut out = Vec::new();
        for close in closes.iter().rev() {
            while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                day -= Duration::days(1);
            }
            out.push(TestBar {
                date: day.format("%Y-%m-%d").to_string(),
                close: *close,
                volume,
            });
            day -= Duration::days(1);
        }
        out.reverse();
        out
    }

    /// Series with a clear rising trend: closes rise linearly `low`→`high`
    /// (enough bars for MA60 / BullishAlign plus headroom).
    fn rising_series(end: &str, bars: usize, low: f64, high: f64, volume: f64) -> Vec<TestBar> {
        let mut closes = Vec::new();
        for i in 0..bars {
            closes.push(low + i as f64 * (high - low) / (bars - 1) as f64);
        }
        daily_series(end, &closes, volume)
    }

    fn stock_000001(bars: Vec<TestBar>) -> TestStock {
        TestStock {
            symbol: "SZ000001",
            name: "平安银行",
            industry: Some("银行"),
            board: Some("主板"),
            list_date: Some("1991-04-03"),
            delist_date: None,
            total_share: Some(1.0e10),
            bars,
        }
    }

    fn stock_600519(bars: Vec<TestBar>) -> TestStock {
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1_256_197_800.0),
            bars,
        }
    }

    // --- Engine entry: run_screener(&Filter, ...) --------------------------

    #[test]
    fn run_screener_up_days_matches_rising_streak() {
        // UpDays now evaluates through the general evaluator: a 3-day streak
        // with each daily gain > 1.5% matches; a flat series does not.
        let stocks = vec![
            stock_000001(daily_series(
                "2026-07-28",
                &[100.0, 101.0, 102.03, 104.04],
                1000.0,
            )),
            stock_600519(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
        ];
        let (_tmp, reader) = build_fixture(&stocks);
        let f = Filter::Series(compass_types::SeriesCond::UpDays { n: 3, min_pct: 0.5 });
        let res = run_screener(&f, &reader, date(2026, 7, 28)).expect("run");
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0].symbol, "SZ000001");
    }

    #[test]
    fn run_screener_nested_combo_matches_engine_semantics() {
        // industries=["白酒"] + BullishAlign + momentum(20, 0..100) through the
        // Filter entry: only 贵州茅台 (白酒, rising series) passes; 平安银行 is
        // filtered out by industry.
        let stocks = vec![
            stock_000001(rising_series("2026-07-28", 80, 10.0, 20.0, 1000.0)),
            stock_600519(rising_series("2026-07-28", 80, 10.0, 20.0, 1000.0)),
        ];
        let (_tmp, reader) = build_fixture(&stocks);
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ma: Some(MaCondition::BullishAlign),
            momentum: Some(MomentumCondition::new(20, 0.0, 100.0)),
            ..ScreenerQuery::default()
        };
        let res = run_screener(&Filter::from(q), &reader, date(2026, 7, 28)).expect("run");
        assert_eq!(res.rows.len(), 1);
        assert_eq!(res.rows[0].symbol, "SH600519");
        assert_eq!(res.total, 1);
    }

    #[test]
    fn run_screener_delisted_excluded_via_default_filter() {
        let stocks = vec![
            stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
            TestStock {
                symbol: "SZ000004",
                name: "国华退",
                industry: Some("医药"),
                board: Some("主板"),
                list_date: Some("1991-01-01"),
                delist_date: Some("2026-07-14"),
                total_share: Some(1.0e9),
                bars: daily_series("2026-07-01", &[3.0; 5], 1000.0),
            },
        ];
        let (_tmp, reader) = build_fixture(&stocks);

        // Filter::from(ScreenerQuery::default()) → Meta(Delisted(false)) →
        // the delisted stock is dropped by the evaluator.
        let res = run_screener(
            &Filter::from(ScreenerQuery::default()),
            &reader,
            date(2026, 7, 28),
        )
        .expect("run");
        assert_eq!(res.rows.len(), 1, "delisted excluded by default");
        assert_eq!(res.rows[0].symbol, "SZ000001");
    }

    #[test]
    fn run_screener_delisted_included_when_exclude_false() {
        let stocks = vec![
            stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
            TestStock {
                symbol: "SZ000004",
                name: "国华退",
                industry: Some("医药"),
                board: Some("主板"),
                list_date: Some("1991-01-01"),
                delist_date: Some("2026-07-14"),
                total_share: Some(1.0e9),
                bars: daily_series("2026-07-01", &[3.0; 5], 1000.0),
            },
        ];
        let (_tmp, reader) = build_fixture(&stocks);

        // exclude_delisted = false emits no Delisted node → both stocks pass
        // the evaluator (no constraint).
        let q = ScreenerQuery {
            exclude_delisted: false,
            ..ScreenerQuery::default()
        };
        let res = run_screener(&Filter::from(q), &reader, date(2026, 7, 28)).expect("run");
        assert_eq!(res.rows.len(), 2, "delisted included when disabled");
    }

    #[test]
    fn run_screener_flat_conditions_match_engine_semantics() {
        // ma=AboveMa20 + breakout(60) + volume(10,1.5) + market cap window:
        // flat Cmp/VolumeSurge nodes inside a top-level And. 茅台 alone gets a
        // 3× volume spike on its last 10 bars, so only it clears
        // volume(10,1.5); its cap 1.256e9×200/1e8 = 2512亿 clears min-cap
        // 1000亿 (平安's 20000亿 would too, but the flat volume fails it).
        let mut moutai_bars = rising_series("2026-07-28", 80, 100.0, 200.0, 1000.0);
        for b in moutai_bars.iter_mut().skip(70) {
            b.volume = 3000.0;
        }
        let stocks = vec![
            stock_000001(rising_series("2026-07-28", 80, 100.0, 200.0, 1000.0)),
            stock_600519(moutai_bars),
        ];
        let (_tmp, reader) = build_fixture(&stocks);
        let q = ScreenerQuery {
            market_cap_min: Some(1000.0),
            ma: Some(MaCondition::AboveMa20),
            breakout: Some(BreakoutCondition::new(60)),
            volume: Some(VolumeCondition::new(10, 1.5)),
            ..ScreenerQuery::default()
        };
        let res = run_screener(&Filter::from(q), &reader, date(2026, 7, 28)).expect("run");
        assert_eq!(res.rows.len(), 1);
        assert_eq!(
            res.rows[0].symbol, "SH600519",
            "茅台 cap ≈ 2512亿 ≥ 1000亿 and passes volume; 平安 fails volume"
        );
    }
}
