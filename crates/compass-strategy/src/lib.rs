//! Stock screening engine.
//!
//! `run_screener` evaluates a [`Filter`] AST against whole-market daily bars
//! (via [`ParquetReader::fetch_cross_section`]) and stock metadata (via
//! [`ParquetReader::load_all_stock_basics`]), returning a market-cap sorted,
//! capped result set.
//!
//! The [`Filter`] AST is reverse-compiled into the legacy [`ScreenerQuery`]
//! by the restricted accept-grammar in `filter_to_query` — only shapes the
//! compile layer (`From<ScreenerQuery> for Filter` in compass-types) can emit
//! are accepted; anything else fails with
//! [`ScreenerError::UnsupportedFilter`]. There is no general `Filter`
//! evaluator (Batch 3): all technical conditions still run through the
//! existing `screen_symbol` engine.
//!
//! All technical indicators are computed from **adjusted close** (前复权);
//! the latest raw close is used for display price and market cap.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::{
    BreakoutCondition, CmpOp, FactorRef, Filter, MaCondition, MetaCond, MomentumCondition,
    ScreenerQuery, ScreenerRow, SeriesCond, SeriesFactor, VolumeCondition,
};
use thiserror::Error;

pub mod screener_series;
pub mod sepa;

/// Errors produced by the screening engine.
#[derive(Debug, Error)]
pub enum ScreenerError {
    /// Underlying data access failure.
    #[error("data error: {0}")]
    Data(#[from] compass_core::data::provider::DataError),
    /// A `Filter` shape outside the restricted accept-grammar of
    /// `filter_to_query` (i.e. one that `From<ScreenerQuery>` can never
    /// produce).
    #[error("unsupported filter shape: {0}")]
    UnsupportedFilter(String),
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
/// The AST is reverse-compiled into a legacy [`ScreenerQuery`] by
/// `filter_to_query` and screened with the existing engine; shapes outside
/// the restricted accept-grammar fail with
/// [`ScreenerError::UnsupportedFilter`].
pub fn run_screener(
    filter: &Filter,
    reader: &ParquetReader,
    now: NaiveDate,
) -> Result<ScreenerResult, ScreenerError> {
    let query = filter_to_query(filter)?;
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
        let Some(row) = screen_symbol(&query, basics_row, series, now)? else {
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

/// Reverse-compile a [`Filter`] AST into the legacy [`ScreenerQuery`].
///
/// Restricted accept-grammar mirroring `From<ScreenerQuery> for Filter`
/// (compass-types): only shapes that the compile layer can emit are accepted,
/// everything else fails with [`ScreenerError::UnsupportedFilter`].
///
/// Accepted nodes: `Meta` (Industry/Exchange/Board/ListYears/MarketCap;
/// `Delisted(false)` → `exclude_delisted = true`, `Delisted(true)` rejected),
/// `Series(Cmp{Close, Gt, Sma(20|60)})` → ma, `Series(Cmp{Close, Gt,
/// NDayHigh(days)})` → breakout, the momentum double-bound pair, `Series(
/// VolumeSurge)` → volume, the BullishAlign pair, empty `And`, and `And`
/// combinations of those (nested sub-`And`s must be a momentum or BullishAlign
/// pair). Rejected: UpDays/Count, `Not`, any `Or`, Const-valued comparisons
/// outside the momentum pair, isolated Sma left operands, single-sided
/// momentum Cmps, and duplicate nodes mapping to the same `ScreenerQuery`
/// field (`From<ScreenerQuery>` never emits such shapes).
///
/// `pub` since the GUI builder reuses it as the single legacy-save
/// compressibility oracle (issue #245 — no third accept-grammar copy).
pub fn filter_to_query(filter: &Filter) -> Result<ScreenerQuery, ScreenerError> {
    let mut query = ScreenerQuery {
        exclude_delisted: false,
        ..ScreenerQuery::default()
    };
    let mut seen = SeenFields::default();
    convert_filter(filter, &mut query, &mut seen)?;
    Ok(query)
}

/// Per-field duplicate detection for the accept-grammar conversion.
/// `From<ScreenerQuery> for Filter` maps each query field to at most one
/// filter node, so an `And` containing two nodes targeting the same field is
/// outside the accept-grammar and must be rejected instead of silently
/// last-win.
#[derive(Default)]
struct SeenFields {
    industries: bool,
    exchanges: bool,
    boards: bool,
    list_years: bool,
    market_cap: bool,
    exclude_delisted: bool,
    ma: bool,
    breakout: bool,
    momentum: bool,
    volume: bool,
}

/// Mark `field` as seen; a second mark on the same field is a duplicate node.
fn mark_seen(seen: &mut bool, field: &str) -> Result<(), ScreenerError> {
    if *seen {
        return Err(ScreenerError::UnsupportedFilter(format!(
            "duplicate {field} node in And: {field} can only occur once"
        )));
    }
    *seen = true;
    Ok(())
}

/// Classify one filter node (or top-level `And` combo) into `query` fields.
fn convert_filter(
    filter: &Filter,
    query: &mut ScreenerQuery,
    seen: &mut SeenFields,
) -> Result<(), ScreenerError> {
    match filter {
        Filter::And(children) => {
            if children.is_empty() {
                // Empty And: no constraints at all (exclude_delisted stays false).
                return Ok(());
            }
            // The whole And may itself be a momentum or BullishAlign pair
            // (ScreenerQuery with a single technical condition compiles to a
            // bare pair, not a wrapped one).
            if let Some(mc) = momentum_pair(children)? {
                mark_seen(&mut seen.momentum, "momentum")?;
                query.momentum = Some(mc);
                return Ok(());
            }
            if let Some(ma) = bullish_pair(children) {
                mark_seen(&mut seen.ma, "ma")?;
                query.ma = Some(ma);
                return Ok(());
            }
            for child in children {
                match child {
                    Filter::Meta(meta) => convert_meta(meta, query, seen)?,
                    Filter::Series(cond) => convert_series(cond, query, seen)?,
                    Filter::And(sub) => {
                        // Nested sub-Ands are only legal as pairs.
                        if let Some(mc) = momentum_pair(sub)? {
                            mark_seen(&mut seen.momentum, "momentum")?;
                            query.momentum = Some(mc);
                        } else if let Some(ma) = bullish_pair(sub) {
                            mark_seen(&mut seen.ma, "ma")?;
                            query.ma = Some(ma);
                        } else {
                            return Err(ScreenerError::UnsupportedFilter(format!(
                                "sub-And is neither a momentum nor a BullishAlign pair: {child:?}"
                            )));
                        }
                    }
                    Filter::Or(_) => {
                        return Err(ScreenerError::UnsupportedFilter(format!(
                            "Or node: {child:?}"
                        )));
                    }
                    Filter::Not(_) => {
                        return Err(ScreenerError::UnsupportedFilter(format!(
                            "Not node: {child:?}"
                        )));
                    }
                }
            }
            Ok(())
        }
        Filter::Meta(meta) => convert_meta(meta, query, seen),
        Filter::Series(cond) => convert_series(cond, query, seen),
        Filter::Or(_) => Err(ScreenerError::UnsupportedFilter(format!(
            "Or node: {filter:?}"
        ))),
        Filter::Not(_) => Err(ScreenerError::UnsupportedFilter(format!(
            "Not node: {filter:?}"
        ))),
    }
}

/// Apply a single metadata node.
fn convert_meta(
    meta: &MetaCond,
    query: &mut ScreenerQuery,
    seen: &mut SeenFields,
) -> Result<(), ScreenerError> {
    match meta {
        MetaCond::Industry(v) => {
            mark_seen(&mut seen.industries, "industry")?;
            query.industries = v.clone();
        }
        MetaCond::Exchange(v) => {
            mark_seen(&mut seen.exchanges, "exchange")?;
            query.exchanges = v.clone();
        }
        MetaCond::Board(v) => {
            mark_seen(&mut seen.boards, "board")?;
            query.boards = v.clone();
        }
        MetaCond::ListYears(n) => {
            mark_seen(&mut seen.list_years, "list_years")?;
            query.list_years = Some(*n);
        }
        MetaCond::MarketCap { min, max } => {
            mark_seen(&mut seen.market_cap, "market_cap")?;
            query.market_cap_min = *min;
            query.market_cap_max = *max;
        }
        // Delisted(true) cannot be expressed by ScreenerQuery and is never
        // produced by the compile layer.
        MetaCond::Delisted(true) => {
            return Err(ScreenerError::UnsupportedFilter(format!("{meta:?}")));
        }
        MetaCond::Delisted(false) => {
            mark_seen(&mut seen.exclude_delisted, "exclude_delisted")?;
            query.exclude_delisted = true;
        }
    }
    Ok(())
}

/// Apply a single series node (ma / breakout / volume shapes only).
fn convert_series(
    cond: &SeriesCond,
    query: &mut ScreenerQuery,
    seen: &mut SeenFields,
) -> Result<(), ScreenerError> {
    match cond {
        SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        } => {
            mark_seen(&mut seen.ma, "ma")?;
            query.ma = Some(MaCondition::AboveMa20);
        }
        SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(60)),
        } => {
            mark_seen(&mut seen.ma, "ma")?;
            query.ma = Some(MaCondition::AboveMa60);
        }
        SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(days)),
        } => {
            mark_seen(&mut seen.breakout, "breakout")?;
            query.breakout = Some(BreakoutCondition::new(*days));
        }
        SeriesCond::VolumeSurge { days, times } => {
            mark_seen(&mut seen.volume, "volume")?;
            query.volume = Some(VolumeCondition::new(*days, *times));
        }
        other => {
            return Err(ScreenerError::UnsupportedFilter(format!(
                "series condition outside the accept-grammar: {other:?}"
            )));
        }
    }
    Ok(())
}

/// Match the momentum double-bound shape
/// `And([Cmp{ChangePct(d), Ge, Const(min)}, Cmp{ChangePct(d), Le, Const(max)}])`.
/// The two lookback days must be equal. Returns `Ok(None)` when `children` is
/// not this shape.
fn momentum_pair(children: &[Filter]) -> Result<Option<MomentumCondition>, ScreenerError> {
    let [
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::ChangePct(ge_days),
            op: CmpOp::Ge,
            value: FactorRef::Const(min_pct),
        }),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::ChangePct(le_days),
            op: CmpOp::Le,
            value: FactorRef::Const(max_pct),
        }),
    ] = children
    else {
        return Ok(None);
    };
    if ge_days != le_days {
        return Err(ScreenerError::UnsupportedFilter(format!(
            "momentum pair with mismatched lookback days ({ge_days} vs {le_days})"
        )));
    }
    Ok(Some(MomentumCondition::new(*ge_days, *min_pct, *max_pct)))
}

/// Match the BullishAlign shape
/// `And([Cmp{Sma(5), Gt, Factor(Sma(20))}, Cmp{Sma(20), Gt, Factor(Sma(60))}])`.
fn bullish_pair(children: &[Filter]) -> Option<MaCondition> {
    match children {
        [
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Sma(5),
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(20)),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Sma(20),
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(60)),
            }),
        ] => Some(MaCondition::BullishAlign),
        _ => None,
    }
}

/// Screen a single symbol; `None` means it does not match.
fn screen_symbol(
    query: &ScreenerQuery,
    basic: &StockBasic,
    series: &[&CrossSectionBar],
    now: NaiveDate,
) -> Result<Option<ScreenerRow>, ScreenerError> {
    // --- Metadata conditions -------------------------------------------------
    if !query.industries.is_empty()
        && !basic
            .industry
            .as_deref()
            .is_some_and(|i| query.industries.iter().any(|q| q == i))
    {
        return Ok(None);
    }
    if !query.exchanges.is_empty() {
        // Exchange derived from the symbol's explicit prefix (the
        // StockBasic.exchange column was removed, issue #181), falling
        // back to the legacy bare-code shape heuristic for pre-migration
        // data — same policy as the GUI layer (parse_explicit_prefix is
        // case-insensitive by construction).
        let exchange = compass_core::data::symbol::exchange_of_symbol(&basic.symbol);
        if !query.exchanges.iter().any(|e| e == exchange) {
            return Ok(None);
        }
    }
    if !query.boards.is_empty()
        && !basic
            .board
            .as_deref()
            .is_some_and(|b| query.boards.iter().any(|q| q == b))
    {
        return Ok(None);
    }
    if let Some(min_years) = query.list_years {
        let Some(list_date) = basic.list_date else {
            // Cannot determine listing age — exclude when constrained.
            return Ok(None);
        };
        if now - list_date < Duration::days(min_years as i64 * 365) {
            return Ok(None);
        }
    }
    if query.exclude_delisted && basic.delist_date.is_some() {
        return Ok(None);
    }

    // Latest bar defines price and market cap.
    let latest = series.last().expect("non-empty series");

    let market_cap = match basic.total_share {
        Some(total_share) => total_share * latest.close / 1e8,
        None => {
            // Missing total_share: excluded when a cap condition is active,
            // otherwise treated as 0.0 (sorts to the bottom).
            if query.market_cap_min.is_some() || query.market_cap_max.is_some() {
                return Ok(None);
            }
            0.0
        }
    };
    if let Some(min) = query.market_cap_min
        && market_cap < min
    {
        return Ok(None);
    }
    if let Some(max) = query.market_cap_max
        && market_cap > max
    {
        return Ok(None);
    }

    // --- Technical conditions (bar-counted windows, adjclose-based) ---------
    if let Some(ma) = query.ma {
        let needed = match ma {
            MaCondition::AboveMa20 => 20,
            MaCondition::AboveMa60 | MaCondition::BullishAlign => 60,
        };
        if series.len() < needed || !matches_ma(ma, series) {
            return Ok(None);
        }
    }
    if let Some(bc) = query.breakout {
        let needed = bc.days as usize + 1;
        if series.len() < needed || !matches_breakout(bc.days, series) {
            return Ok(None);
        }
    }
    if let Some(mc) = query.momentum {
        let needed = mc.days as usize + 1;
        if series.len() < needed {
            return Ok(None);
        }
        let ret = momentum_return(mc.days, series);
        if !(ret >= mc.min_pct && ret <= mc.max_pct) {
            return Ok(None);
        }
    }
    if let Some(vc) = query.volume {
        let needed = (vc.days as usize) * 3;
        if series.len() < needed || !matches_volume(vc.days, vc.times, series) {
            return Ok(None);
        }
    }

    // --- Assemble row ---------------------------------------------------------
    let change_20d = change_over(series, 20);
    let industry = basic.industry.clone().unwrap_or_default();
    Ok(Some(ScreenerRow {
        symbol: basic.symbol.clone(),
        name: basic.name.clone(),
        latest_price: latest.close,
        change_20d,
        market_cap,
        industry,
    }))
}

/// Simple moving average of the last `n` adjusted closes.
fn ma(series: &[&CrossSectionBar], n: usize) -> f64 {
    let start = series.len() - n;
    let sum: f64 = series[start..].iter().map(|b| b.adjclose).sum();
    sum / n as f64
}

/// Match the moving-average condition against the latest bars.
fn matches_ma(ma_cond: MaCondition, series: &[&CrossSectionBar]) -> bool {
    let latest = series.last().expect("non-empty").adjclose;
    match ma_cond {
        MaCondition::AboveMa20 => latest > ma(series, 20),
        MaCondition::AboveMa60 => latest > ma(series, 60),
        MaCondition::BullishAlign => {
            let ma5 = ma(series, 5);
            let ma20 = ma(series, 20);
            let ma60 = ma(series, 60);
            ma5 > ma20 && ma20 > ma60
        }
    }
}

/// True when the latest adjclose is strictly above the max of the previous
/// `days` bars (new N-day high).
fn matches_breakout(days: u32, series: &[&CrossSectionBar]) -> bool {
    let window_start = series.len() - days as usize - 1;
    let prev_max = series[window_start..series.len() - 1]
        .iter()
        .map(|b| b.adjclose)
        .fold(f64::NEG_INFINITY, f64::max);
    let latest = series.last().expect("non-empty").adjclose;
    latest > prev_max
}

/// Return percent over the last `days` bars: (last - last-days) / last-days * 100.
fn momentum_return(days: u32, series: &[&CrossSectionBar]) -> f64 {
    let base = series[series.len() - days as usize - 1].adjclose;
    let latest = series.last().expect("non-empty").adjclose;
    (latest - base) / base * 100.0
}

/// True when the recent `days`-bar average volume ≥ `times` × the last
/// `3×days`-bar average volume (nested baseline window).
fn matches_volume(days: u32, times: f64, series: &[&CrossSectionBar]) -> bool {
    let recent: f64 = series[series.len() - days as usize..]
        .iter()
        .map(|b| b.volume)
        .sum::<f64>()
        / days as f64;
    let baseline: f64 = series[series.len() - 3 * days as usize..]
        .iter()
        .map(|b| b.volume)
        .sum::<f64>()
        / (3 * days) as f64;
    recent >= times * baseline
}

/// Adjusted-close return over the last `n` bars (display column; uses
/// available bars when fewer than `n`, 0.0 when fewer than 2).
fn change_over(series: &[&CrossSectionBar], n: usize) -> f64 {
    if series.len() < 2 {
        return 0.0;
    }
    let base_idx = series.len().saturating_sub(n);
    let base = series[base_idx].adjclose;
    let latest = series.last().expect("non-empty").adjclose;
    if base == 0.0 {
        return 0.0;
    }
    (latest - base) / base * 100.0
}

#[cfg(test)]
mod tests {
    //! Todo 5 (ref #244): `run_screener` accepts the `Filter` AST via the
    //! restricted reverse conversion `filter_to_query`. The existing
    //! `tests/screener.rs` integration suite still calls `run_screener` with a
    //! `&ScreenerQuery` — those 23 call sites are migrated in task 5B, so this
    //! module carries the new-entry tests instead.

    use super::*;
    use chrono::{Datelike, Weekday};
    use compass_types::{
        BreakoutCondition, CmpOp, FactorRef, Filter, MaCondition, MetaCond, MomentumCondition,
        ScreenerQuery, SeriesCond, SeriesFactor, VolumeCondition,
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

    /// A query with every field set, exercising every branch of the reverse
    /// conversion at once (nested pairs for momentum, flat Cmps for MA/breakout).
    fn full_query() -> ScreenerQuery {
        ScreenerQuery {
            industries: vec!["白酒".to_string()],
            exchanges: vec!["SH".to_string()],
            boards: vec!["主板".to_string()],
            list_years: Some(3),
            market_cap_min: Some(100.0),
            market_cap_max: Some(5000.0),
            exclude_delisted: true,
            ma: Some(MaCondition::AboveMa20),
            breakout: Some(BreakoutCondition::new(60)),
            momentum: Some(MomentumCondition::new(20, 0.0, 100.0)),
            volume: Some(VolumeCondition::new(10, 1.5)),
        }
    }

    /// Run the private reverse conversion; test helper so call sites stay terse.
    fn convert(filter: &Filter) -> Result<ScreenerQuery, ScreenerError> {
        filter_to_query(filter)
    }

    // --- UnsupportedFilter (RED shapes) ------------------------------------

    #[test]
    fn up_days_predicate_is_unsupported() {
        let f = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.0 });
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn count_predicate_is_unsupported() {
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 10,
            at_least: 5,
        });
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn not_node_is_unsupported() {
        let f = Filter::Not(Box::new(Filter::Meta(MetaCond::Industry(vec![
            "白酒".to_string(),
        ]))));
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn or_node_is_unsupported() {
        let f = Filter::Or(vec![Filter::Meta(MetaCond::Industry(vec![
            "白酒".to_string(),
        ]))]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn delisted_true_is_unsupported() {
        let f = Filter::Meta(MetaCond::Delisted(true));
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn const_value_cmp_is_unsupported() {
        // Close > Const(5.0): the engine cannot express a raw price threshold.
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Const(5.0),
        });
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn isolated_sma_left_operand_is_unsupported() {
        // Sma(5) > Sma(20) outside the BullishAlign pair shape.
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Sma(5),
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        });
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn top_level_single_change_pct_cmp_is_unsupported() {
        // Momentum must arrive as a bounded pair, never a bare lower bound.
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::ChangePct(20),
            op: CmpOp::Ge,
            value: FactorRef::Const(0.0),
        });
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn momentum_pair_with_mismatched_days_is_unsupported() {
        // Both bounds must share the same lookback window.
        let f = Filter::And(vec![
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(5),
                op: CmpOp::Ge,
                value: FactorRef::Const(0.0),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(10),
                op: CmpOp::Le,
                value: FactorRef::Const(100.0),
            }),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn non_pair_sub_and_is_unsupported() {
        // A nested And that is neither a momentum pair nor a BullishAlign pair.
        let f = Filter::And(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            Filter::And(vec![Filter::Series(SeriesCond::UpDays {
                n: 2,
                min_pct: 1.0,
            })]),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn momentum_with_reversed_bounds_is_unsupported() {
        // From<ScreenerQuery> emits Ge first, Le second; a reversed order is
        // not a shape the compile layer produces.
        let f = Filter::And(vec![
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(20),
                op: CmpOp::Le,
                value: FactorRef::Const(100.0),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(20),
                op: CmpOp::Ge,
                value: FactorRef::Const(0.0),
            }),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_industry_nodes_are_unsupported() {
        // Two Industry metas map to the same query field — From<ScreenerQuery>
        // never emits this, so the accept-grammar rejects it instead of
        // silently last-winning.
        let f = Filter::And(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            Filter::Meta(MetaCond::Industry(vec!["银行".to_string()])),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_ma_cmps_are_unsupported() {
        // AboveMa20 + AboveMa60 both target the `ma` field, even though the
        // two nodes are otherwise valid accept-grammar shapes.
        let f = Filter::And(vec![
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(20)),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(60)),
            }),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_delisted_false_is_unsupported() {
        let f = Filter::And(vec![
            Filter::Meta(MetaCond::Delisted(false)),
            Filter::Meta(MetaCond::Delisted(false)),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_momentum_pairs_are_unsupported() {
        // Two nested momentum sub-Ands both target the `momentum` field.
        let pair = |min: f64, max: f64| {
            Filter::And(vec![
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::ChangePct(20),
                    op: CmpOp::Ge,
                    value: FactorRef::Const(min),
                }),
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::ChangePct(20),
                    op: CmpOp::Le,
                    value: FactorRef::Const(max),
                }),
            ])
        };
        let f = Filter::And(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            pair(0.0, 100.0),
            pair(5.0, 50.0),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_volume_surge_is_unsupported() {
        let surge = || {
            Filter::Series(SeriesCond::VolumeSurge {
                days: 10,
                times: 1.5,
            })
        };
        let f = Filter::And(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            surge(),
            surge(),
        ]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_bullish_align_pairs_are_unsupported() {
        // Two nested BullishAlign sub-Ands both target the `ma` field.
        let pair = || {
            Filter::And(vec![
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::Sma(5),
                    op: CmpOp::Gt,
                    value: FactorRef::Factor(SeriesFactor::Sma(20)),
                }),
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::Sma(20),
                    op: CmpOp::Gt,
                    value: FactorRef::Factor(SeriesFactor::Sma(60)),
                }),
            ])
        };
        let f = Filter::And(vec![pair(), pair()]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    #[test]
    fn duplicate_breakout_cmps_are_unsupported() {
        let breakout = |days: u32| {
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::NDayHigh(days)),
            })
        };
        let f = Filter::And(vec![breakout(20), breakout(60)]);
        assert!(matches!(
            convert(&f),
            Err(ScreenerError::UnsupportedFilter(_))
        ));
    }

    // --- Round-trip: Filter::from(q) → filter_to_query == q ----------------

    #[test]
    fn empty_and_roundtrips_to_empty_query() {
        let q = ScreenerQuery {
            exclude_delisted: false,
            ..ScreenerQuery::default()
        };
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    #[test]
    fn default_query_roundtrips_through_delisted_false() {
        // ScreenerQuery::default() compiles to a bare Meta(Delisted(false))
        // node; the reverse must restore exclude_delisted = true.
        let q = ScreenerQuery::default();
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    #[test]
    fn explicit_false_query_roundtrips() {
        // exclude_delisted = false emits no Delisted node; the reverse must
        // keep the flag false.
        let q = ScreenerQuery {
            exclude_delisted: false,
            ..ScreenerQuery::default()
        };
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    #[test]
    fn full_query_roundtrips_exactly() {
        let q = full_query();
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    #[test]
    fn above_ma60_roundtrips() {
        let q = ScreenerQuery {
            ma: Some(MaCondition::AboveMa60),
            ..ScreenerQuery {
                exclude_delisted: false,
                ..ScreenerQuery::default()
            }
        };
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    #[test]
    fn bullish_align_roundtrips_via_nested_and() {
        // BullishAlign alone compiles to a top-level And pair.
        let q = ScreenerQuery {
            ma: Some(MaCondition::BullishAlign),
            ..ScreenerQuery {
                exclude_delisted: false,
                ..ScreenerQuery::default()
            }
        };
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    #[test]
    fn nested_combo_roundtrips_exactly() {
        // industries + BullishAlign + momentum: the From layer emits a nested
        // And (pair inside pair) — classification of every element must be
        // exact (plan Todo 5 acceptance case).
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ma: Some(MaCondition::BullishAlign),
            momentum: Some(MomentumCondition::new(20, 0.0, 100.0)),
            ..ScreenerQuery::default()
        };
        assert_eq!(convert(&Filter::from(q.clone())).expect("convert"), q);
    }

    // --- Engine entry: run_screener(&Filter, ...) --------------------------

    #[test]
    fn run_screener_unsupported_filter_shape_returns_error() {
        let stocks = vec![stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0))];
        let (_tmp, reader) = build_fixture(&stocks);
        let f = Filter::Series(SeriesCond::UpDays { n: 2, min_pct: 1.0 });
        let err = run_screener(&f, &reader, date(2026, 7, 28)).expect_err("must reject");
        assert!(matches!(err, ScreenerError::UnsupportedFilter(_)));
    }

    #[test]
    fn run_screener_nested_combo_matches_engine_semantics() {
        // industries=["白酒"] + BullishAlign + momentum(20, 0..100) through the
        // Filter entry: only 贵州茅台 (白酒, rising series) passes; 平安银行 is
        // filtered out by industry. Mirrors the tests/screener.rs assertions
        // that task 5B will migrate.
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
        // exclude_delisted = true: the delisted stock is dropped.
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

        // exclude_delisted = false emits no Delisted node → flag stays false:
        // both stocks appear.
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
        // flat Cmp nodes inside a top-level And (no nested pairs) — the
        // ⑧ combo path with only single-node children. 茅台 alone gets a 3×
        // volume spike on its last 10 bars, so only it clears volume(10,1.5);
        // its cap 1.256e9×200/1e8 = 2512亿 clears min-cap 1000亿 (平安's
        // 20000亿 would too, but the flat volume fails it).
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
