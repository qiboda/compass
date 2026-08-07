//! Stock screening engine.
//!
//! `run_screener` evaluates a [`ScreenerQuery`] against whole-market daily
//! bars (via [`ParquetReader::fetch_cross_section`]) and stock metadata
//! (via [`ParquetReader::load_all_stock_basics`]), returning a market-cap
//! sorted, capped result set.
//!
//! All technical indicators are computed from **adjusted close** (前复权);
//! the latest raw close is used for display price and market cap.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_core::model::{CrossSectionBar, StockBasic};
use compass_types::{MaCondition, ScreenerQuery, ScreenerRow};
use thiserror::Error;

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

/// Evaluate `query` against the market data behind `reader`.
pub fn run_screener(
    query: &ScreenerQuery,
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
        let Some(row) = screen_symbol(query, basics_row, series, now)? else {
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
