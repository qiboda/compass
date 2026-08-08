//! Five-module SEPA scoring engine + hard filters + `run_sepa` entry
//! (epic #139, sub-issue #149).
//!
//! Scores every stock that passes the hard filters across five modules with
//! the review-locked formulas (no implementation freedom):
//!
//! - **趋势 30%** — MA structure 45 (close>MA250 +18, MA60>MA120 +9,
//!   MA120>MA250 +9, MA250 rising +9), price position 20 (drawdown from the
//!   250-day high), RS percentile 35 (top decile full, linear below).
//! - **题材 25%** — `min((板块涨幅30 + 成交额30 + 扩散20 + news)/90 × 25, 25)`;
//!   the denominator is always 90 and the cap is explicit; v1 has no news
//!   data so `news` defaults to 10/20.
//! - **资金 20%** — 量价配合 40, 筹码集中 30, 大资金流入 30 (main-flow
//!   percentile 20 + dragon-list institution 10 + survey 5 + block-trade ±5,
//!   capped at 30 after the block adjustment).
//! - **形态 20%** — VCP quality 15 + breakout confirmation 5 scaled by the
//!   thermometer band (≥60 full / 40-60 half / <40 zero).
//! - **风险 −5%** — `−(ATR>5% −20 + 120d drawdown>30% −30 + surge −25,
//!   capped 75) × 0.05 ∈ [−3.75, 0]` — never "100 − deductions".
//!
//! Hard filters (excluded before scoring): name contains "ST"/"退", listed
//! fewer than ~60 trading days, 20-day average amount below 3000 万,
//! suspended for the last ~5 trading days, BJ (北交所) exchange.

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_core::model::{
    BlockTradeRow, CapitalMainFlow, ConceptMember, CrossSectionBar, DragonListRow,
    InstitutionSurveyRow, StockBasic,
};
use compass_types::{SepaData, SepaDetails, SepaFactor, SepaQuery, SepaRow};

use super::SEPA_WINDOW_DAYS;
use super::aggregation::aggregate_concept_daily;
use super::indicators::{
    atr20, drawdown_from_high, ma, momentum_return, rs_score, vcp_score, volume_ratio,
};
use super::temperature::compute_market_thermometer;
use crate::ScreenerError;

/// Backend truncation cap when `SepaQuery.top_n` is 0 (locked default 50).
pub const DEFAULT_TOP_N: usize = 50;

/// Locked module weights.
const TREND_WEIGHT: f64 = 0.30;
const THEME_WEIGHT: f64 = 0.25;
const CAPITAL_WEIGHT: f64 = 0.20;

/// 题材 formula constants (locked): denominator always 90, news default 10/20.
const NEWS_SCORE_DEFAULT: f64 = 10.0;
const NEWS_SCORE_MAX: f64 = 20.0;
const THEME_DENOMINATOR: f64 = 90.0;
const THEME_CAP: f64 = 25.0;

/// 风险 constants (locked): per-rule deductions and the 75-point cap.
const RISK_ATR_DEDUCTION: f64 = 20.0;
const RISK_DRAWDOWN_DEDUCTION: f64 = 30.0;
const RISK_SURGE_DEDUCTION: f64 = 25.0;
const RISK_MAX_DEDUCTIONS: f64 = 75.0;

/// Hard-filter constants.
const MIN_LISTED_CAL_DAYS: i64 = 90; // ≈ 60 trading days
const MIN_AVG_AMOUNT: f64 = 30_000_000.0; // 3000 万
const SUSPEND_CAL_DAYS: i64 = 7; // ≈ 5 trading days

/// Evaluate `query` against the market data behind `reader` for date `now`.
///
/// Flow mirrors [`crate::run_screener`]: fetch the cross-section window
/// (`now - 550` days) plus stock basics and the five SEPA tables, compute the
/// whole-market thermometer **before** the per-stock loop (the breakout
/// factor consumes it), hard-filter and score every symbol, sort by total
/// score (NaN-safe), truncate to `query.top_n` (default 50) and assemble
/// ranked [`SepaRow`]s with per-module [`SepaFactor`] breakdowns.
///
/// Symbols are exchange-prefixed (`SH600519`); theme names come from
/// `fetch_concept_member` grouped by symbol. Missing SEPA parquet files
/// degrade to empty vecs — their modules score 0 without a panic.
/// Pre-fetched SEPA scoring window (full range, sliced per-day by
/// [`score_sepa`]). Fetching once and scoring many days avoids re-reading
/// the parquet files for every backtest day (the original per-day
/// `run_sepa` re-fetched 7 datasets per day, ~3s/day of which ~93% was I/O).
pub(crate) struct SepaWindow {
    bars: Vec<CrossSectionBar>,
    basics: Vec<StockBasic>,
    members: Vec<ConceptMember>,
    flows: Vec<CapitalMainFlow>,
    dragons: Vec<DragonListRow>,
    blocks: Vec<BlockTradeRow>,
    surveys: Vec<InstitutionSurveyRow>,
}

/// Keep the last row per (symbol, date). Real parquet data occasionally
/// carries duplicate rows for one symbol on one day (index-like symbols
/// mixing two sources); deduplicating keeps day-over-day returns
/// comparable. Input order is preserved for the surviving rows.
pub(crate) fn dedup_bars(bars: Vec<CrossSectionBar>) -> Vec<CrossSectionBar> {
    let mut seen: HashSet<(String, NaiveDate)> = HashSet::new();
    let mut deduped: Vec<CrossSectionBar> = Vec::with_capacity(bars.len());
    for bar in bars.into_iter().rev() {
        if seen.insert((bar.symbol.clone(), bar.trade_date)) {
            deduped.push(bar);
        }
    }
    deduped.reverse();
    deduped
}

/// Fetch the full scoring window `[range_start, range_end]` once. The
/// window must cover `[now - SEPA_WINDOW_DAYS, now]` for every `now` that
/// [`score_sepa`] will be called with.
pub(crate) fn fetch_sepa_window(
    reader: &ParquetReader,
    range_start: NaiveDate,
    range_end: NaiveDate,
) -> Result<SepaWindow, ScreenerError> {
    let started = std::time::Instant::now();
    let t = std::time::Instant::now();
    let bars = dedup_bars(reader.fetch_cross_section(range_start, range_end)?);
    tracing::debug!(
        fetch = "cross_section",
        elapsed_ms = t.elapsed().as_millis()
    );
    let t = std::time::Instant::now();
    let basics = reader.load_all_stock_basics()?;
    tracing::debug!(fetch = "stock_basics", elapsed_ms = t.elapsed().as_millis());
    let t = std::time::Instant::now();
    let members = reader.fetch_concept_member()?;
    tracing::debug!(
        fetch = "concept_member",
        elapsed_ms = t.elapsed().as_millis()
    );
    let t = std::time::Instant::now();
    let flows = reader.fetch_capital_main_flow(range_start, range_end)?;
    tracing::debug!(
        fetch = "capital_main_flow",
        elapsed_ms = t.elapsed().as_millis()
    );
    let t = std::time::Instant::now();
    let dragons = reader.fetch_dragon_list(range_start, range_end)?;
    tracing::debug!(fetch = "dragon_list", elapsed_ms = t.elapsed().as_millis());
    let t = std::time::Instant::now();
    let blocks = reader.fetch_block_trade(range_start, range_end)?;
    tracing::debug!(fetch = "block_trade", elapsed_ms = t.elapsed().as_millis());
    let t = std::time::Instant::now();
    let surveys = reader.fetch_institution_survey(range_start, range_end)?;
    tracing::debug!(
        fetch = "institution_survey",
        elapsed_ms = t.elapsed().as_millis()
    );
    tracing::debug!(
        bars_loaded = bars.len(),
        window_start = %range_start,
        window_end = %range_end,
        fetch_ms = started.elapsed().as_millis(),
        "sepa window fetched"
    );
    Ok(SepaWindow {
        bars,
        basics,
        members,
        flows,
        dragons,
        blocks,
        surveys,
    })
}

pub fn run_sepa(
    query: &SepaQuery,
    reader: &ParquetReader,
    now: NaiveDate,
) -> Result<SepaData, ScreenerError> {
    let range_start = now - Duration::days(SEPA_WINDOW_DAYS);
    let window = fetch_sepa_window(reader, range_start, now)?;
    score_sepa(query, &window, now)
}

pub(crate) fn score_sepa(
    query: &SepaQuery,
    window: &SepaWindow,
    now: NaiveDate,
) -> Result<SepaData, ScreenerError> {
    let started = std::time::Instant::now();
    let range_start = now - Duration::days(SEPA_WINDOW_DAYS);

    // Slice the pre-fetched window to [range_start, now] (references, no copy).
    let bars: Vec<&CrossSectionBar> = window
        .bars
        .iter()
        .filter(|b| b.trade_date >= range_start && b.trade_date <= now)
        .collect();
    let flows: Vec<&CapitalMainFlow> = window
        .flows
        .iter()
        .filter(|f| f.trade_date >= range_start && f.trade_date <= now)
        .collect();
    let dragons: Vec<&DragonListRow> = window
        .dragons
        .iter()
        .filter(|d| d.trade_date >= range_start && d.trade_date <= now)
        .collect();
    let blocks: Vec<&BlockTradeRow> = window
        .blocks
        .iter()
        .filter(|b| b.trade_date >= range_start && b.trade_date <= now)
        .collect();
    let surveys: Vec<&InstitutionSurveyRow> = window
        .surveys
        .iter()
        .filter(|s| s.survey_date >= range_start && s.survey_date <= now)
        .collect();
    let basics = &window.basics;
    let members = &window.members;
    let slice_ms = started.elapsed().as_millis();

    // --- Group raw rows by exchange-prefixed symbol ------------------------
    let basics_by_symbol: HashMap<String, &StockBasic> =
        basics.iter().map(|b| (b.symbol.clone(), b)).collect();
    let mut bars_by_symbol: HashMap<String, Vec<&CrossSectionBar>> = HashMap::new();
    for bar in bars.iter().copied() {
        bars_by_symbol
            .entry(bar.symbol.clone())
            .or_default()
            .push(bar);
    }

    // Membership rows carry exchange-prefixed symbols → group by prefixed
    // symbol (the same key format as bars and basics).
    let mut memberships: HashMap<String, Vec<&ConceptMember>> = HashMap::new();
    for m in members {
        memberships.entry(m.symbol.clone()).or_default().push(m);
    }

    // Concept display names per symbol (deduped, sorted).
    let mut themes: HashMap<String, Vec<String>> = HashMap::new();
    for (symbol, ms) in &memberships {
        let mut names: Vec<String> = ms.iter().filter_map(|m| m.concept_name.clone()).collect();
        names.sort();
        names.dedup();
        if !names.is_empty() {
            themes.insert(symbol.clone(), names);
        }
    }

    // --- Thermometer first (breakout confirmation consumes its score) ------
    let thermometer = compute_market_thermometer(&bars_by_symbol, &basics_by_symbol);

    // --- Concept-board theme pass -------------------------------------------
    let concept_daily = aggregate_concept_daily(members, &bars_by_symbol);
    let board_stats = board_momentums(members, &bars_by_symbol);
    let gain_values: Vec<f64> = board_stats
        .values()
        .filter(|s| s.gain_count > 0)
        .map(|s| s.gain_mean)
        .collect();
    let amount_values: Vec<f64> = concept_daily.values().map(|d| d.amount).collect();
    let board_gain30 = |code: &str| -> f64 {
        let Some(s) = board_stats.get(code) else {
            return 0.0;
        };
        if s.gain_count == 0 {
            return 0.0;
        }
        rank_percentile(&gain_values, s.gain_mean) * 30.0
    };
    let board_amount30 = |code: &str| -> f64 {
        let Some(d) = concept_daily.get(code) else {
            return 0.0;
        };
        rank_percentile(&amount_values, d.amount) * 30.0
    };
    let board_diffusion20 = |code: &str| -> f64 {
        let Some(d) = concept_daily.get(code) else {
            return 0.0;
        };
        let up = d.up_ratio * 10.0;
        let leaders = board_stats.get(code).map_or(0, |s| {
            s.top_pcts.iter().take(5).filter(|p| **p > 5.0).count()
        });
        up + if leaders >= 2 { 10.0 } else { 0.0 }
    };

    // Per symbol keep the concept scoring highest (multi-membership stocks
    // are scored on their best board).
    let mut best_theme: HashMap<String, ThemeComponents> = HashMap::new();
    for (symbol, ms) in &memberships {
        let mut best: Option<(f64, ThemeComponents)> = None;
        for m in ms {
            let components = ThemeComponents {
                gain30: board_gain30(&m.concept_code),
                amount30: board_amount30(&m.concept_code),
                diffusion20: board_diffusion20(&m.concept_code),
            };
            let theme = theme_from_components(
                components.gain30,
                components.amount30,
                components.diffusion20,
                NEWS_SCORE_DEFAULT,
            );
            if best.as_ref().is_none_or(|(s, _)| theme > *s) {
                best = Some((theme, components));
            }
        }
        if let Some((_, components)) = best {
            best_theme.insert(symbol.clone(), components);
        }
    }

    // --- Capital pass --------------------------------------------------------
    // 5-day cumulative main net inflow per symbol, ranked across symbols that
    // have flow data.
    let mut flow_group: HashMap<String, Vec<&CapitalMainFlow>> = HashMap::new();
    for f in flows.iter().copied() {
        flow_group.entry(f.symbol.clone()).or_default().push(f);
    }
    let mut flow_5d: HashMap<String, f64> = HashMap::new();
    for (symbol, rows) in &flow_group {
        let cum: f64 = rows
            .iter()
            .rev()
            .take(5)
            .map(|r| r.main_net_inflow)
            .filter(|v| v.is_finite())
            .sum();
        flow_5d.insert(symbol.clone(), cum);
    }
    let flow_values: Vec<f64> = flow_5d.values().copied().collect();
    let main_flow_pct: HashMap<String, f64> = flow_5d
        .iter()
        .map(|(s, v)| (s.clone(), rank_percentile(&flow_values, *v)))
        .collect();

    let mut institution_buy: HashSet<String> = HashSet::new();
    for d in dragons.iter().copied() {
        if d.institution_flag == Some(1) && d.net_amount.unwrap_or(0.0) > 0.0 {
            institution_buy.insert(d.symbol.clone());
        }
    }

    let mut surveyed: HashSet<String> = HashSet::new();
    for s in surveys.iter().copied() {
        surveyed.insert(s.symbol.clone());
    }

    // Block-trade ±5 adjustment from the last 5 rows per symbol: a discount
    // (>2%) adds, a premium (>2%) subtracts.
    let mut block_group: HashMap<String, Vec<&BlockTradeRow>> = HashMap::new();
    for b in blocks.iter().copied() {
        block_group.entry(b.symbol.clone()).or_default().push(b);
    }
    let mut block_adj: HashMap<String, f64> = HashMap::new();
    for (symbol, rows) in &block_group {
        let mut discount = false;
        let mut premium = false;
        for r in rows.iter().rev().take(5) {
            if let Some(p) = r.premium_rate {
                if p < -2.0 {
                    discount = true;
                }
                if p > 2.0 {
                    premium = true;
                }
            }
        }
        let mut adj = 0.0;
        if discount {
            adj += 5.0;
        }
        if premium {
            adj -= 5.0;
        }
        block_adj.insert(symbol.clone(), adj);
    }

    // --- RS pass -------------------------------------------------------------
    let market_momentums: Vec<(String, f64)> = bars_by_symbol
        .iter()
        .filter_map(|(s, series)| momentum_for(series).map(|m| (s.clone(), m)))
        .collect();
    let mut sector_momentums: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    let mut sector_member_count: HashMap<String, usize> = HashMap::new();
    for (symbol, ms) in &memberships {
        for m in ms {
            *sector_member_count
                .entry(m.concept_code.clone())
                .or_default() += 1;
            if let Some(series) = bars_by_symbol.get(symbol)
                && let Some(mom) = momentum_for(series)
            {
                sector_momentums
                    .entry(m.concept_code.clone())
                    .or_default()
                    .push((symbol.clone(), mom));
            }
        }
    }
    // Each symbol's most representative sector (most members) for RS ranking.
    let mut sector_of: HashMap<String, String> = HashMap::new();
    for (symbol, ms) in &memberships {
        if let Some(best) = ms.iter().max_by_key(|m| {
            sector_member_count
                .get(&m.concept_code)
                .copied()
                .unwrap_or(0)
        }) {
            sector_of.insert(symbol.clone(), best.concept_code.clone());
        }
    }

    let ctx = MarketContext {
        best_theme,
        themes,
        main_flow_pct,
        institution_buy,
        surveyed,
        block_adj,
        market_momentums,
        sector_momentums,
        sector_of,
        sector_member_count,
    };

    // --- Per-symbol filter + score -------------------------------------------
    let market_latest = bars.iter().map(|b| b.trade_date).max();
    let top_n = if query.top_n == 0 {
        DEFAULT_TOP_N
    } else {
        query.top_n
    };
    let mut rows: Vec<SepaRow> = Vec::new();
    for (symbol, basic) in &basics_by_symbol {
        let Some(series) = bars_by_symbol.get(symbol) else {
            continue;
        };
        if is_filtered(basic, series, now, market_latest) {
            continue;
        }
        rows.push(score_symbol(symbol, basic, series, &ctx, thermometer.score));
    }
    let total = rows.len();
    rows.sort_by(|a, b| {
        b.total_score
            .total_cmp(&a.total_score)
            .then(a.symbol.cmp(&b.symbol))
    });
    rows.truncate(top_n);
    for (i, row) in rows.iter_mut().enumerate() {
        row.rank = i + 1;
    }

    tracing::debug!(
        bars_loaded = bars.len(),
        basics_loaded = basics.len(),
        concept_members = members.len(),
        matched = total,
        returned = rows.len(),
        slice_ms = slice_ms,
        compute_ms = started.elapsed().as_millis() - slice_ms,
        elapsed_ms = started.elapsed().as_millis(),
        "sepa run completed"
    );

    Ok(SepaData {
        rows,
        thermometer,
        date: now.format("%Y-%m-%d").to_string(),
    })
}

/// Precomputed market-wide inputs shared by the per-symbol scoring loop.
struct MarketContext {
    best_theme: HashMap<String, ThemeComponents>,
    themes: HashMap<String, Vec<String>>,
    main_flow_pct: HashMap<String, f64>,
    institution_buy: HashSet<String>,
    surveyed: HashSet<String>,
    block_adj: HashMap<String, f64>,
    market_momentums: Vec<(String, f64)>,
    sector_momentums: HashMap<String, Vec<(String, f64)>>,
    sector_of: HashMap<String, String>,
    sector_member_count: HashMap<String, usize>,
}

/// Per-board 20/60-day momentum aggregates consumed by the theme module.
struct BoardMomentums {
    gain_mean: f64,
    gain_count: usize,
    top_pcts: Vec<f64>,
}

/// Board momentum pass: equal-weighted mean of the members' weighted
/// 20/60-day momentum plus the members' day-over-day changes (for the 领涨
/// 带动 check), per concept code. Members without computable windows are
/// skipped.
fn board_momentums(
    members: &[ConceptMember],
    bars_by_symbol: &HashMap<String, Vec<&CrossSectionBar>>,
) -> HashMap<String, BoardMomentums> {
    let mut out: HashMap<String, BoardMomentums> = HashMap::new();
    for m in members {
        let Some(series) = bars_by_symbol.get(m.symbol.as_str()) else {
            continue;
        };
        let entry = out.entry(m.concept_code.clone()).or_insert(BoardMomentums {
            gain_mean: 0.0,
            gain_count: 0,
            top_pcts: Vec::new(),
        });
        if series.len() >= 2 {
            let latest = series[series.len() - 1];
            let prev = series[series.len() - 2];
            if latest.close.is_finite() && prev.close.is_finite() && prev.close != 0.0 {
                entry
                    .top_pcts
                    .push((latest.close - prev.close) / prev.close * 100.0);
            }
        }
        if series.len() >= 61
            && let (Some(m20), Some(m60)) =
                (momentum_return(series, 20), momentum_return(series, 60))
        {
            entry.gain_mean += m20 * 0.7 + m60 * 0.3;
            entry.gain_count += 1;
        }
    }
    for s in out.values_mut() {
        if s.gain_count > 0 {
            s.gain_mean /= s.gain_count as f64;
        }
        s.top_pcts.sort_by(|a, b| b.total_cmp(a));
    }
    out
}

/// Locked 题材 formula: `min((涨幅30 + 成交额30 + 扩散20 + news)/90 × 25, 25)`.
///
/// The denominator is always 90 — never ÷80 — so a news-fed board
/// (100/90×25 ≈ 27.8) caps at 25 while a news-less board still reaches the
/// full 25 when the other three components are maxed.
fn theme_from_components(gain30: f64, amount30: f64, diffusion20: f64, news: f64) -> f64 {
    ((gain30 + amount30 + diffusion20 + news) / THEME_DENOMINATOR * THEME_WEIGHT * 100.0)
        .min(THEME_CAP)
}

/// One best-board theme candidate for a symbol.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ThemeComponents {
    gain30: f64,
    amount30: f64,
    diffusion20: f64,
}

/// Score the trend module (module 100 → ×0.30).
fn score_trend(series: &[&CrossSectionBar], rs_pct: f64) -> (f64, Vec<SepaFactor>) {
    let latest_adj = series.last().expect("caller guarantees non-empty").adjclose;
    let ma250 = ma(series, 250);
    let ma120 = ma(series, 120);
    let ma60 = ma(series, 60);
    let mut structure = 0.0;
    if ma250.is_some_and(|m| latest_adj > m) {
        structure += 18.0;
    }
    if let (Some(m60), Some(m120)) = (ma60, ma120)
        && m60 > m120
    {
        structure += 9.0;
    }
    if let (Some(m120), Some(m250)) = (ma120, ma250)
        && m120 > m250
    {
        structure += 9.0;
    }
    if series.len() >= 255
        && let (Some(m_now), Some(m_prev)) = (ma250, ma(&series[..series.len() - 5], 250))
        && m_now > m_prev
    {
        structure += 9.0;
    }

    let window = series.len().min(250);
    let drawdown = drawdown_from_high(series, window).unwrap_or(0.0);
    let position = price_position(drawdown);
    let rs = rs_score_from_percentile(rs_pct);

    let module = structure + position + rs;
    (
        module * TREND_WEIGHT,
        vec![
            SepaFactor {
                label: "均线结构".to_string(),
                score: structure,
                max: 45.0,
                note: None,
            },
            SepaFactor {
                label: "价格位置".to_string(),
                score: position,
                max: 20.0,
                note: Some(format!("距一年高点回撤 {drawdown:.1}%")),
            },
            SepaFactor {
                label: "相对强度".to_string(),
                score: rs,
                max: 35.0,
                note: Some(format!("动量分位 {:.0}%", rs_pct * 100.0)),
            },
        ],
    )
}

/// Locked price-position bands: <10% → 20, 10-20% → 16, 20-30% → 10, >50% → 0.
/// The 30-50% gap is a linear ramp 10 → 0 (closest to the review intent of a
/// monotone penalty).
fn price_position(drawdown_pct: f64) -> f64 {
    if drawdown_pct < 10.0 {
        20.0
    } else if drawdown_pct < 20.0 {
        16.0
    } else if drawdown_pct < 30.0 {
        10.0
    } else if drawdown_pct < 50.0 {
        (50.0 - drawdown_pct) / 2.0
    } else {
        0.0
    }
}

/// RS score from a momentum percentile: top decile → full 35, below it linear
/// `pct/0.9 × 35` (0 at the bottom, 35 at the 90th percentile).
fn rs_score_from_percentile(pct: f64) -> f64 {
    (pct / 0.9).min(1.0) * 35.0
}

/// Score the theme module (locked formula, `None` = no concept membership).
fn score_theme(best: Option<&ThemeComponents>) -> (f64, Vec<SepaFactor>) {
    let Some(c) = best else {
        return (
            0.0,
            vec![
                SepaFactor {
                    label: "板块涨幅".to_string(),
                    score: 0.0,
                    max: 30.0,
                    note: Some("无板块数据".to_string()),
                },
                SepaFactor {
                    label: "板块成交额".to_string(),
                    score: 0.0,
                    max: 30.0,
                    note: None,
                },
                SepaFactor {
                    label: "板块扩散".to_string(),
                    score: 0.0,
                    max: 20.0,
                    note: None,
                },
                SepaFactor {
                    label: "新闻热度".to_string(),
                    score: 0.0,
                    max: NEWS_SCORE_MAX,
                    note: Some("v1 无新闻数据".to_string()),
                },
            ],
        );
    };
    let theme = theme_from_components(c.gain30, c.amount30, c.diffusion20, NEWS_SCORE_DEFAULT);
    (
        theme,
        vec![
            SepaFactor {
                label: "板块涨幅".to_string(),
                score: c.gain30,
                max: 30.0,
                note: None,
            },
            SepaFactor {
                label: "板块成交额".to_string(),
                score: c.amount30,
                max: 30.0,
                note: None,
            },
            SepaFactor {
                label: "板块扩散".to_string(),
                score: c.diffusion20,
                max: 20.0,
                note: None,
            },
            SepaFactor {
                label: "新闻热度".to_string(),
                score: NEWS_SCORE_DEFAULT,
                max: NEWS_SCORE_MAX,
                note: Some("v1 默认 10/20".to_string()),
            },
        ],
    )
}

/// Capital inputs precomputed per symbol from the four flow tables.
struct CapitalInputs {
    main_flow_pct: f64,
    has_institution_buy: bool,
    has_survey: bool,
    block_adj: f64,
}

/// Score the capital module (module 100 → ×0.20).
fn score_capital(series: &[&CrossSectionBar], inputs: &CapitalInputs) -> (f64, Vec<SepaFactor>) {
    let volume_price = up_day_volume_ratio(series).map_or(0.0, |r| (r / 1.5).min(1.0) * 40.0);
    let chip = chip_compliance(series);
    let main_flow = inputs.main_flow_pct * 20.0;
    let dragon = if inputs.has_institution_buy {
        10.0
    } else {
        0.0
    };
    let survey = if inputs.has_survey { 5.0 } else { 0.0 };
    // min(30, 合计) — the cap applies after the block-trade ±5 adjustment;
    // the floor keeps a negative-only adjustment from dragging below 0.
    let big_capital = (main_flow + dragon + survey + inputs.block_adj).clamp(0.0, 30.0);

    let module = volume_price + chip + big_capital;
    (
        module * CAPITAL_WEIGHT,
        vec![
            SepaFactor {
                label: "量价配合".to_string(),
                score: volume_price,
                max: 40.0,
                note: None,
            },
            SepaFactor {
                label: "筹码集中".to_string(),
                score: chip,
                max: 30.0,
                note: None,
            },
            SepaFactor {
                label: "大资金流入".to_string(),
                score: big_capital,
                max: 30.0,
                note: Some(format!(
                    "主力{main_flow:.0}+龙虎{dragon:.0}+调研{survey:.0}+大宗{:+.0}",
                    inputs.block_adj
                )),
            },
        ],
    )
}

/// 量价配合 40: average up-day volume vs average down-day volume over the
/// last 20 bars (21 needed for the deltas). All-up → +inf (full score),
/// all-down → 0.0, insufficient window → `None`.
fn up_day_volume_ratio(series: &[&CrossSectionBar]) -> Option<f64> {
    if series.len() < 21 {
        return None;
    }
    let window = &series[series.len() - 21..];
    let mut up_vol = 0.0;
    let mut down_vol = 0.0;
    let mut up_n = 0usize;
    let mut down_n = 0usize;
    for pair in window.windows(2) {
        let (prev, cur) = (pair[0], pair[1]);
        if !prev.close.is_finite() || !cur.close.is_finite() || !cur.volume.is_finite() {
            continue;
        }
        if cur.close > prev.close {
            up_vol += cur.volume;
            up_n += 1;
        } else if cur.close < prev.close {
            down_vol += cur.volume;
            down_n += 1;
        }
    }
    if up_n == 0 {
        return Some(0.0);
    }
    if down_n == 0 {
        return Some(f64::INFINITY);
    }
    let down_avg = down_vol / down_n as f64;
    if down_avg == 0.0 {
        return Some(f64::INFINITY);
    }
    Some(up_vol / up_n as f64 / down_avg)
}

/// 筹码集中 30: 60-day gain in 20-40% (+10), last-20-bar sideways with
/// amplitude ≤ 10% (+10), 20-day volume shrink 量比 < 0.7 (+10). The
/// 10%-amplitude threshold is an implementation detail of the locked
/// qualitative rule.
fn chip_compliance(series: &[&CrossSectionBar]) -> f64 {
    if series.len() < 61 {
        return 0.0;
    }
    let mut score = 0.0;
    if let Some(g60) = momentum_return(series, 60)
        && (20.0..=40.0).contains(&g60)
    {
        score += 10.0;
    }
    let win = &series[series.len() - 20..];
    let closes: Vec<f64> = win.iter().map(|b| b.close).collect();
    if closes.iter().all(|c| c.is_finite()) {
        let min = closes.iter().fold(f64::INFINITY, |a, c| a.min(*c));
        let max = closes.iter().fold(f64::NEG_INFINITY, |a, c| a.max(*c));
        if min > 0.0 && (max - min) / min <= 0.10 {
            score += 10.0;
        }
    }
    if let Some(vr) = volume_ratio(series, 20)
        && vr < 0.7
    {
        score += 10.0;
    }
    score
}

/// Score the pattern module: VCP quality 15 + breakout confirmation 5 scaled
/// by the thermometer band (locked linkage).
fn score_pattern(series: &[&CrossSectionBar], thermometer_score: f64) -> (f64, Vec<SepaFactor>) {
    let vcp = vcp_score(series).unwrap_or(0.0) * 15.0;
    let breakout = breakout_base_score(series) * thermometer_multiplier(thermometer_score);
    (
        vcp + breakout,
        vec![
            SepaFactor {
                label: "VCP质量".to_string(),
                score: vcp,
                max: 15.0,
                note: None,
            },
            SepaFactor {
                label: "突破确认".to_string(),
                score: breakout,
                max: 5.0,
                note: Some(format!("温度计 {thermometer_score:.0}")),
            },
        ],
    )
}

/// Breakout base (before the thermometer band): close breaks the 120-day
/// platform high with 量比 ≥ 1.5 → 5; within 3% of the platform high
/// (including above it without volume) → 3; else 0.
fn breakout_base_score(series: &[&CrossSectionBar]) -> f64 {
    let window = series.len().min(120);
    if window < 2 {
        return 0.0;
    }
    let Some(latest) = series.last() else {
        return 0.0;
    };
    let platform_high = series[series.len() - window..series.len() - 1]
        .iter()
        .map(|b| b.high)
        .fold(f64::NEG_INFINITY, f64::max);
    if !platform_high.is_finite() || platform_high <= 0.0 {
        return 0.0;
    }
    let distance = (platform_high - latest.close) / platform_high;
    let volume_ok = volume_ratio(series, 20).is_some_and(|vr| vr >= 1.5);
    if latest.close > platform_high && volume_ok {
        5.0
    } else if distance < 0.03 {
        3.0
    } else {
        0.0
    }
}

/// Locked thermometer band for the breakout factor: ≥60 → full, 40-60 →
/// half, <40 → zero.
fn thermometer_multiplier(score: f64) -> f64 {
    if score >= 60.0 {
        1.0
    } else if score >= 40.0 {
        0.5
    } else {
        0.0
    }
}

/// Score the risk module. Locked formula: `risk = −(deductions capped at 75)
/// × 0.05 ∈ [−3.75, 0]` — never "100 − deductions" (a clean stock would then
/// score −5 and an all-penalized one −1.25, inverting the direction).
fn score_risk(series: &[&CrossSectionBar]) -> (f64, Vec<SepaFactor>) {
    let mut atr_deduction = 0.0;
    if let Some(latest) = series.last()
        && let Some(atr) = atr20(series)
        && latest.close.is_finite()
        && latest.close > 0.0
        && atr / latest.close > 0.05
    {
        atr_deduction = RISK_ATR_DEDUCTION;
    }
    let mut drawdown_deduction = 0.0;
    let window = series.len().min(120);
    if let Some(dd) = drawdown_from_high(series, window)
        && dd > 30.0
    {
        drawdown_deduction = RISK_DRAWDOWN_DEDUCTION;
    }
    let mut surge_deduction = 0.0;
    if let (Some(m20), Some(vr)) = (momentum_return(series, 20), volume_ratio(series, 20))
        && m20 > 30.0
        && vr > 3.0
    {
        surge_deduction = RISK_SURGE_DEDUCTION;
    }
    let total = (atr_deduction + drawdown_deduction + surge_deduction).min(RISK_MAX_DEDUCTIONS);
    (
        -total * 0.05,
        vec![
            SepaFactor {
                label: "波动惩罚(ATR)".to_string(),
                score: -atr_deduction,
                max: RISK_ATR_DEDUCTION,
                note: None,
            },
            SepaFactor {
                label: "深度回撤".to_string(),
                score: -drawdown_deduction,
                max: RISK_DRAWDOWN_DEDUCTION,
                note: None,
            },
            SepaFactor {
                label: "放量滞涨".to_string(),
                score: -surge_deduction,
                max: RISK_SURGE_DEDUCTION,
                note: None,
            },
        ],
    )
}

/// Hard filters (locked, applied before scoring). `market_latest` is the
/// market's own latest bar, so long holidays do not false-positive the
/// suspension check.
fn is_filtered(
    basic: &StockBasic,
    series: &[&CrossSectionBar],
    now: NaiveDate,
    market_latest: Option<NaiveDate>,
) -> bool {
    if basic.name.contains("ST") || basic.name.contains("退") {
        return true;
    }
    let Some(list_date) = basic.list_date else {
        // Unknown listing age cannot verify the 60-trading-day rule.
        return true;
    };
    if now - list_date < Duration::days(MIN_LISTED_CAL_DAYS) {
        return true;
    }
    let window = series.len().min(20);
    let avg_amount: f64 = series[series.len() - window..]
        .iter()
        .map(|b| b.amount)
        .sum::<f64>()
        / window as f64;
    if !avg_amount.is_finite() || avg_amount < MIN_AVG_AMOUNT {
        return true;
    }
    let Some(latest) = series.last() else {
        return true;
    };
    if market_latest.is_some_and(|m| m - latest.trade_date > Duration::days(SUSPEND_CAL_DAYS)) {
        return true;
    }
    // Exchange derived from the symbol's explicit prefix
    // (StockBasic.exchange was removed): BJ (北交所) stocks are hard-filtered.
    // parse_explicit_prefix is case-insensitive; the bare-code fallback
    // keeps legacy pre-migration 8xxxxx codes classified BJ.
    compass_core::data::symbol::exchange_of_symbol(&basic.symbol) == "BJ"
}

/// Score one symbol into a row (filters already applied by the caller).
fn score_symbol(
    symbol: &str,
    basic: &StockBasic,
    series: &[&CrossSectionBar],
    ctx: &MarketContext,
    thermometer_score: f64,
) -> SepaRow {
    let rs_pct = rs_percentile(symbol, series, ctx);
    let (trend, trend_factors) = score_trend(series, rs_pct);
    let (theme, theme_factors) = score_theme(ctx.best_theme.get(symbol));
    let capital_inputs = CapitalInputs {
        main_flow_pct: ctx.main_flow_pct.get(symbol).copied().unwrap_or(0.0),
        has_institution_buy: ctx.institution_buy.contains(symbol),
        has_survey: ctx.surveyed.contains(symbol),
        block_adj: ctx.block_adj.get(symbol).copied().unwrap_or(0.0),
    };
    let (capital, capital_factors) = score_capital(series, &capital_inputs);
    let (pattern, pattern_factors) = score_pattern(series, thermometer_score);
    let (risk, risk_factors) = score_risk(series);
    let latest = series.last().expect("caller guarantees non-empty");

    // Clamped into the documented 0..100 range; the raw sum dips slightly
    // negative only for heavily penalized names.
    let total = (trend + theme + capital + pattern + risk).clamp(0.0, 100.0);

    SepaRow {
        symbol: symbol.to_string(),
        name: basic.name.clone(),
        rank: 0, // official rank assigned after sorting/truncation
        total_score: total,
        trend,
        theme,
        capital,
        pattern,
        risk,
        industry: basic.industry.clone().unwrap_or_default(),
        themes: ctx.themes.get(symbol).cloned().unwrap_or_default(),
        latest_price: latest.close,
        change_pct: day_change(series),
        details: SepaDetails {
            trend: trend_factors,
            theme: theme_factors,
            capital: capital_factors,
            pattern: pattern_factors,
            risk: risk_factors,
        },
    }
}

/// RS percentile for one symbol: sector ranking when its most-representative
/// concept has ≥5 members with computable peers, whole-market ranking
/// otherwise (locked rule).
fn rs_percentile(symbol: &str, series: &[&CrossSectionBar], ctx: &MarketContext) -> f64 {
    rs_score(series, &rs_peers_for(symbol, ctx))
}

fn rs_peers_for(symbol: &str, ctx: &MarketContext) -> Vec<f64> {
    if let Some(code) = ctx.sector_of.get(symbol) {
        let count = ctx.sector_member_count.get(code).copied().unwrap_or(0);
        if count >= 5
            && let Some(members) = ctx.sector_momentums.get(code)
        {
            let others: Vec<f64> = members
                .iter()
                .filter(|(s, _)| s.as_str() != symbol)
                .map(|(_, m)| *m)
                .collect();
            if !others.is_empty() {
                return others;
            }
        }
    }
    ctx.market_momentums
        .iter()
        .filter(|(s, _)| s.as_str() != symbol)
        .map(|(_, m)| *m)
        .collect()
}

/// Own-momentum definition matching `rs_score`'s internal formula (weighted
/// 60d×0.7 + 20d×0.3, degrading to 20-day-only below 61 bars).
fn momentum_for(series: &[&CrossSectionBar]) -> Option<f64> {
    if series.len() >= 61 {
        Some(momentum_return(series, 60)? * 0.7 + momentum_return(series, 20)? * 0.3)
    } else if series.len() >= 21 {
        momentum_return(series, 20)
    } else {
        None
    }
}

/// Day-over-day change percent of the latest two raw closes (guarded).
fn day_change(series: &[&CrossSectionBar]) -> f64 {
    if series.len() < 2 {
        return 0.0;
    }
    let prev = series[series.len() - 2].close;
    let latest = series[series.len() - 1].close;
    if !prev.is_finite() || !latest.is_finite() || prev == 0.0 {
        return 0.0;
    }
    (latest - prev) / prev * 100.0
}

/// Percentile rank of `value` within `values`: fraction of strictly smaller
/// entries normalized over the *other* entries, so the top entry always
/// ranks 1.0 (a single-entry pool ranks 1.0 too). Empty pool → 0.0.
fn rank_percentile(values: &[f64], value: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    if values.len() == 1 {
        return 1.0;
    }
    let below = values.iter().filter(|v| **v < value).count();
    below as f64 / (values.len() - 1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Weekday};

    struct TestBar {
        date: String,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    }

    fn daily_series(end: &str, closes: &[f64], up: f64, down: f64, volume: f64) -> Vec<TestBar> {
        let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
        let mut out = Vec::new();
        for close in closes.iter().rev() {
            while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                day -= Duration::days(1);
            }
            out.push(TestBar {
                date: day.format("%Y-%m-%d").to_string(),
                high: close + up,
                low: close - down,
                close: *close,
                volume,
            });
            day -= Duration::days(1);
        }
        out.reverse();
        out
    }

    fn rising300() -> Vec<TestBar> {
        let mut closes: Vec<f64> = (0..300).map(|i| 10.0 + i as f64 * 10.0 / 299.0).collect();
        closes[299] = 20.0;
        daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6)
    }

    fn to_cross_section(bars: &[TestBar]) -> Vec<CrossSectionBar> {
        bars.iter()
            .map(|b| CrossSectionBar {
                symbol: "000001".to_string(),
                trade_date: NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").expect("parse date"),
                open: b.close - 1.0,
                high: b.high,
                low: b.low,
                adjclose: b.close,
                close: b.close,
                volume: b.volume,
                amount: 0.0,
            })
            .collect()
    }

    #[test]
    fn theme_formula_never_exceeds_25() {
        // News default (10): full 25 reachable with the other three maxed.
        assert_eq!(theme_from_components(30.0, 30.0, 20.0, 10.0), 25.0);
        // With news (15/20): 95/90×25 and 100/90×25 both cap at 25.
        assert_eq!(theme_from_components(30.0, 30.0, 20.0, 15.0), 25.0);
        assert_eq!(theme_from_components(30.0, 30.0, 20.0, 20.0), 25.0);
        // Denominator stays 90 even without news: 80/90×25 ≈ 22.2.
        assert!((theme_from_components(30.0, 30.0, 20.0, 0.0) - 80.0 / 90.0 * 25.0).abs() < 1e-9);
        assert_eq!(theme_from_components(0.0, 0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn risk_contribution_is_deduction_times_minus_0_05() {
        // Clean rising series → no deductions → 0.
        let clean_owned = to_cross_section(&rising300());
        let clean: Vec<&CrossSectionBar> = clean_owned.iter().collect();
        let (contribution, factors) = score_risk(&clean);
        assert_eq!(contribution, 0.0);
        assert!(factors.iter().all(|f| f.score == 0.0));

        // All-deduction series: ATR 22% > 5%, 120d drawdown 55%, 20d surge
        // 35% on 4× volume → deductions 75 → exactly −3.75, never −5.
        let mut closes: Vec<f64> = Vec::new();
        closes.extend((0..200).map(|i| 10.0 + i as f64 * 0.1));
        closes.extend((0..20).map(|i| 30.0 - i as f64));
        closes.extend((0..60).map(|_| 10.0));
        closes.extend((1..=20).map(|j| 10.0 + j as f64 * 3.5 / 20.0));
        let mut bars = daily_series("2026-07-31", &closes, 1.5, 1.5, 1.0e6);
        for b in bars.iter_mut().skip(280) {
            b.volume = 4.0e6;
        }
        let risky_owned = to_cross_section(&bars);
        let risky: Vec<&CrossSectionBar> = risky_owned.iter().collect();
        let (contribution, factors) = score_risk(&risky);
        assert!(
            (contribution - -3.75).abs() < 1e-9,
            "expected -3.75, got {contribution}"
        );
        assert_eq!(factors[0].score, -20.0);
        assert_eq!(factors[1].score, -30.0);
        assert_eq!(factors[2].score, -25.0);
    }

    #[test]
    fn thermometer_multiplier_bands() {
        assert_eq!(thermometer_multiplier(100.0), 1.0);
        assert_eq!(thermometer_multiplier(60.0), 1.0);
        assert_eq!(thermometer_multiplier(59.9), 0.5);
        assert_eq!(thermometer_multiplier(40.0), 0.5);
        assert_eq!(thermometer_multiplier(39.9), 0.0);
    }

    #[test]
    fn price_position_locked_bands() {
        assert_eq!(price_position(0.0), 20.0);
        assert_eq!(price_position(9.99), 20.0);
        assert_eq!(price_position(10.0), 16.0);
        assert_eq!(price_position(19.99), 16.0);
        assert_eq!(price_position(20.0), 10.0);
        assert_eq!(price_position(29.99), 10.0);
        assert_eq!(price_position(30.0), 10.0, "30-50% ramp starts at 10");
        assert_eq!(price_position(40.0), 5.0);
        assert_eq!(price_position(50.0), 0.0);
        assert_eq!(price_position(90.0), 0.0);
    }

    #[test]
    fn rank_percentile_normalizes_to_top_entry() {
        assert_eq!(rank_percentile(&[], 5.0), 0.0);
        assert_eq!(
            rank_percentile(&[5.0], 5.0),
            1.0,
            "single entry is its own top"
        );
        assert_eq!(rank_percentile(&[1.0, 2.0, 3.0, 4.0], 4.0), 1.0);
        assert_eq!(rank_percentile(&[1.0, 2.0, 3.0, 4.0], 1.0), 0.0);
        assert_eq!(rank_percentile(&[1.0, 2.0, 3.0, 4.0], 2.5), 2.0 / 3.0);
    }

    #[test]
    fn breakout_base_scores_volume_and_proximity() {
        // 130 flat bars at 10.0 → platform high = 10.1.
        let mut closes = vec![10.0; 130];
        closes[129] = 12.0;
        let mut bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        for b in bars.iter_mut().skip(110) {
            b.volume = 2.0e6;
        }
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(breakout_base_score(&s), 5.0, "new high + 量比 2");

        // Same breakout without the volume surge → 3 (within 3% above high).
        let mut closes = vec![10.0; 130];
        closes[129] = 12.0;
        let bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(breakout_base_score(&s), 3.0);

        // Within 3% below the platform high → 3.
        let mut closes = vec![10.0; 130];
        closes[129] = 9.9;
        let bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(breakout_base_score(&s), 3.0);

        // 5% below the platform high → 0.
        let mut closes = vec![10.0; 130];
        closes[129] = 9.5;
        let bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(breakout_base_score(&s), 0.0);
    }

    #[test]
    fn up_day_volume_ratio_handles_extremes_and_mixed() {
        // All-up rising series → +inf → full score (40).
        let owned = to_cross_section(&rising300());
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(up_day_volume_ratio(&s), Some(f64::INFINITY));

        // All-down → 0.0.
        let mut closes: Vec<f64> = (0..41).map(|i| 20.0 - i as f64 * 10.0 / 40.0).collect();
        let bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(up_day_volume_ratio(&s), Some(0.0));

        // Mixed: up days at 2× the down-day volume → ratio 2.
        closes = (0..41)
            .map(|i| if i % 2 == 0 { 10.0 } else { 10.5 })
            .collect();
        let mut bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        for b in bars.iter_mut() {
            if b.close > 10.0 {
                b.volume = 2.0e6;
            }
        }
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert!((up_day_volume_ratio(&s).expect("enough bars") - 2.0).abs() < 1e-9);

        // Too short → None.
        let short_owned = to_cross_section(&rising300()[..20]);
        let short: Vec<&CrossSectionBar> = short_owned.iter().collect();
        assert_eq!(up_day_volume_ratio(&short), None);
    }

    #[test]
    fn chip_compliance_scores_three_conditions() {
        // 40 rising bars 10→13 (30% gain) then 21 flat bars at 13 with
        // halved volume → gain ✓, sideways ✓, shrink ✓ → 30.
        let mut closes: Vec<f64> = (0..40).map(|i| 10.0 + i as f64 * 3.0 / 39.0).collect();
        closes.extend(vec![13.0; 21]);
        let mut bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        for b in bars.iter_mut().skip(40) {
            b.volume = 0.5e6;
        }
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(chip_compliance(&s), 30.0);

        // Falling 20→10: no gain, 15% amplitude (not sideways), flat volume → 0.
        let closes: Vec<f64> = (0..61).map(|i| 20.0 - i as f64 * 10.0 / 60.0).collect();
        let bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(chip_compliance(&s), 0.0);

        // Too short → 0.
        let short_owned = to_cross_section(&rising300()[..60]);
        let short: Vec<&CrossSectionBar> = short_owned.iter().collect();
        assert_eq!(chip_compliance(&short), 0.0);
    }

    #[test]
    fn big_capital_is_capped_at_30_after_block_adjustment() {
        let owned = to_cross_section(&rising300());
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        let full = CapitalInputs {
            main_flow_pct: 1.0,
            has_institution_buy: true,
            has_survey: true,
            block_adj: 5.0,
        };
        let (_, factors) = score_capital(&s, &full);
        assert_eq!(factors[2].score, 30.0, "20+10+5+5 = 40 → capped at 30");

        let discounted = CapitalInputs {
            main_flow_pct: 1.0,
            has_institution_buy: true,
            has_survey: true,
            block_adj: -5.0,
        };
        let (_, factors) = score_capital(&s, &discounted);
        assert_eq!(factors[2].score, 30.0, "20+10+5−5 = 30 → still 30");

        let negative_only = CapitalInputs {
            main_flow_pct: 0.0,
            has_institution_buy: false,
            has_survey: false,
            block_adj: -5.0,
        };
        let (_, factors) = score_capital(&s, &negative_only);
        assert_eq!(factors[2].score, 0.0, "−5 alone must clamp to 0, not −5");
    }

    #[test]
    fn trend_module_full_on_rising_series_and_zero_on_falling() {
        let rising_owned = to_cross_section(&rising300());
        let rising: Vec<&CrossSectionBar> = rising_owned.iter().collect();
        let (contribution, factors) = score_trend(&rising, 1.0);
        assert!(
            (contribution - 30.0).abs() < 1e-9,
            "100 module points × 0.30: {contribution}"
        );
        assert_eq!(factors[0].score, 45.0, "all four MA-structure points");
        assert_eq!(factors[1].score, 20.0, "at the 250-day high");
        assert_eq!(factors[2].score, 35.0, "top-decile RS");

        let mut closes: Vec<f64> = (0..300).map(|i| 20.0 - i as f64 * 15.0 / 299.0).collect();
        closes[299] = 5.0;
        let falling_bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let falling_owned = to_cross_section(&falling_bars);
        let falling: Vec<&CrossSectionBar> = falling_owned.iter().collect();
        let (contribution, _) = score_trend(&falling, 0.0);
        assert!(
            (contribution - 0.0).abs() < 1e-9,
            "falling scores 0 (drawdown 73% > 50%): {contribution}"
        );
    }

    #[test]
    fn rs_peers_use_sector_when_large_enough_else_market() {
        let market = vec![
            ("a".to_string(), 1.0),
            ("b".to_string(), 2.0),
            ("c".to_string(), 3.0),
        ];
        let sector = vec![
            ("a".to_string(), 1.0),
            ("b".to_string(), 2.0),
            ("c".to_string(), 3.0),
            ("d".to_string(), 4.0),
            ("e".to_string(), 5.0),
        ];
        let ctx = MarketContext {
            best_theme: HashMap::new(),
            themes: HashMap::new(),
            main_flow_pct: HashMap::new(),
            institution_buy: HashSet::new(),
            surveyed: HashSet::new(),
            block_adj: HashMap::new(),
            market_momentums: market.clone(),
            sector_momentums: HashMap::from([("BK1".to_string(), sector.clone())]),
            sector_of: HashMap::from([("a".to_string(), "BK1".to_string())]),
            sector_member_count: HashMap::from([("BK1".to_string(), 5)]),
        };
        let peers = rs_peers_for("a", &ctx);
        assert_eq!(peers, vec![2.0, 3.0, 4.0, 5.0], "sector peers exclude own");

        // <5 members → whole-market fallback.
        let mut ctx_small = MarketContext {
            market_momentums: market.clone(),
            sector_momentums: HashMap::from([("BK1".to_string(), sector.clone())]),
            sector_of: HashMap::from([("a".to_string(), "BK1".to_string())]),
            sector_member_count: HashMap::from([("BK1".to_string(), 4)]),
            best_theme: HashMap::new(),
            themes: HashMap::new(),
            main_flow_pct: HashMap::new(),
            institution_buy: HashSet::new(),
            surveyed: HashSet::new(),
            block_adj: HashMap::new(),
        };
        let peers = rs_peers_for("a", &ctx_small);
        assert_eq!(peers, vec![2.0, 3.0], "market peers exclude own");

        // 5 members but no computable peers → whole-market fallback.
        ctx_small.sector_member_count = HashMap::from([("BK1".to_string(), 5)]);
        ctx_small.sector_momentums = HashMap::new();
        let peers = rs_peers_for("a", &ctx_small);
        assert_eq!(peers, vec![2.0, 3.0]);
    }

    #[test]
    fn momentum_for_degrades_by_window() {
        let long_owned = to_cross_section(&rising300());
        let long: Vec<&CrossSectionBar> = long_owned.iter().collect();
        assert!(
            momentum_for(&long).is_some(),
            "≥61 bars → weighted momentum"
        );

        let mid_bars = daily_series("2026-07-31", &vec![10.0; 40], 0.1, 0.1, 1.0e6);
        let mid_owned = to_cross_section(&mid_bars);
        let mid: Vec<&CrossSectionBar> = mid_owned.iter().collect();
        assert!(momentum_for(&mid).is_some(), "21-60 bars → 20-day momentum");

        let short_bars = daily_series("2026-07-31", &[10.0; 20], 0.1, 0.1, 1.0e6);
        let short_owned = to_cross_section(&short_bars);
        let short: Vec<&CrossSectionBar> = short_owned.iter().collect();
        assert_eq!(momentum_for(&short), None, "below 21 bars → None");
    }

    #[test]
    fn bj_filter_derives_exchange_from_symbol_prefix() {
        // StockBasic.exchange was removed; the BJ hard filter derives the
        // exchange from the symbol's prefix. A prefixed BJ symbol must be
        // filtered while SH/SZ prefixed symbols pass.
        let now = NaiveDate::parse_from_str("2026-07-31", "%Y-%m-%d").expect("date");
        let series: Vec<CrossSectionBar> = (0..300)
            .map(|k| CrossSectionBar {
                symbol: "SH600519".to_string(),
                trade_date: now - Duration::days(300 - k as i64),
                open: 10.0,
                high: 10.1,
                low: 9.9,
                adjclose: 10.0,
                close: 10.0,
                volume: 1.0e6,
                amount: 5.0e8,
            })
            .collect();
        let refs: Vec<&CrossSectionBar> = series.iter().collect();
        let mk_basic = |symbol: &str| StockBasic {
            symbol: symbol.to_string(),
            name: "S".to_string(),
            area: None,
            industry: None,
            market: None,
            board: None,
            full_name: None,
            total_share: None,
            list_date: Some(now - Duration::days(3650)),
            delist_date: None,
        };
        assert!(
            is_filtered(&mk_basic("BJ830001"), &refs, now, Some(now)),
            "BJ prefix must be hard-filtered"
        );
        assert!(
            is_filtered(&mk_basic("bj830001"), &refs, now, Some(now)),
            "lowercase BJ prefix must be hard-filtered (case-insensitive)"
        );
        assert!(
            is_filtered(&mk_basic("830001"), &refs, now, Some(now)),
            "legacy bare 8xxxxx must fall back to BJ and be hard-filtered"
        );
        assert!(
            !is_filtered(&mk_basic("600519"), &refs, now, Some(now)),
            "legacy bare 6xxxxx must fall back to SH and pass"
        );
        assert!(
            !is_filtered(&mk_basic("SH600519"), &refs, now, Some(now)),
            "SH prefix must pass"
        );
        assert!(
            !is_filtered(&mk_basic("SZ000001"), &refs, now, Some(now)),
            "SZ prefix must pass"
        );
    }

    #[test]
    fn day_change_guards_zero_previous_close() {
        let mut closes: Vec<f64> = (0..10).map(|i| 10.0 + i as f64).collect();
        closes[8] = 0.0;
        let bars = daily_series("2026-07-31", &closes, 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(day_change(&s), 0.0, "zero previous close guarded");

        let bars = daily_series("2026-07-31", &[10.0, 10.5], 0.1, 0.1, 1.0e6);
        let owned = to_cross_section(&bars);
        let s: Vec<&CrossSectionBar> = owned.iter().collect();
        assert_eq!(day_change(&s), 5.0);
    }
}
