//! Shared boundary types for the stock screener.
//!
//! These types cross crate boundaries (GUI ↔ strategy ↔ core) and live in
//! their own crate so no crate in the dependency graph needs to depend on
//! another just to reach a shared type. Dependency direction:
//! `strategy → types` and `GUI → types`; `core` must NOT depend on this crate.

use serde::{Deserialize, Serialize};

/// Condition on the relationship between the latest adjusted close and
/// moving averages.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaCondition {
    /// Latest adjclose is above MA20.
    AboveMa20,
    /// Latest adjclose is above MA60.
    AboveMa60,
    /// MA5 > MA20 > MA60 (bullish alignment).
    BullishAlign,
}

/// N-day new-high breakout condition.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BreakoutCondition {
    /// Lookback window in trading days.
    #[serde(default = "default_breakout_days")]
    pub days: u32,
}

fn default_breakout_days() -> u32 {
    60
}

impl BreakoutCondition {
    /// Default condition: 60-day new high.
    pub fn new(days: u32) -> Self {
        Self { days }
    }
}

/// Default: 60 trading days.
impl Default for BreakoutCondition {
    fn default() -> Self {
        Self { days: 60 }
    }
}

/// N-day momentum (return) condition.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MomentumCondition {
    /// Lookback window in trading days.
    #[serde(default = "default_momentum_days")]
    pub days: u32,
    /// Minimum return percent (inclusive).
    #[serde(default = "default_momentum_min_pct")]
    pub min_pct: f64,
    /// Maximum return percent (inclusive).
    #[serde(default = "default_momentum_max_pct")]
    pub max_pct: f64,
}

fn default_momentum_days() -> u32 {
    20
}

fn default_momentum_min_pct() -> f64 {
    0.0
}

fn default_momentum_max_pct() -> f64 {
    100.0
}

impl MomentumCondition {
    /// Create a momentum condition with the given window and bounds.
    pub fn new(days: u32, min_pct: f64, max_pct: f64) -> Self {
        Self {
            days,
            min_pct,
            max_pct,
        }
    }
}

/// Default: 20-day momentum between 0% and 100%.
impl Default for MomentumCondition {
    fn default() -> Self {
        Self {
            days: 20,
            min_pct: 0.0,
            max_pct: 100.0,
        }
    }
}

/// Volume-surge condition: recent N-day average volume ≥ times × baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VolumeCondition {
    /// Recent window in trading days.
    #[serde(default = "default_volume_days")]
    pub days: u32,
    /// Multiplier against the 3×N-day baseline average.
    #[serde(default = "default_volume_times")]
    pub times: f64,
}

fn default_volume_days() -> u32 {
    20
}

fn default_volume_times() -> f64 {
    2.0
}

impl VolumeCondition {
    /// Create a volume condition with the given window and multiplier.
    pub fn new(days: u32, times: f64) -> Self {
        Self { days, times }
    }
}

/// Default: 20-day average volume ≥ 2× the 60-day baseline.
impl Default for VolumeCondition {
    fn default() -> Self {
        Self {
            days: 20,
            times: 2.0,
        }
    }
}

/// Full set of screener conditions. All conditions are AND-ed together;
/// multi-value fields (industries/exchanges/boards) are OR-ed within the field.
///
/// `None`/empty fields mean "no constraint".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenerQuery {
    /// Industries to include (OR). Empty = all.
    #[serde(default)]
    pub industries: Vec<String>,
    /// Exchanges to include, e.g. "SH"/"SZ"/"BJ" (OR). Empty = all.
    #[serde(default)]
    pub exchanges: Vec<String>,
    /// Boards to include, e.g. "主板"/"创业板" (OR). Empty = all.
    #[serde(default)]
    pub boards: Vec<String>,
    /// Minimum listing age in years. `None` = no constraint.
    #[serde(default)]
    pub list_years: Option<u32>,
    /// Minimum market cap in 亿元. `None` = no constraint.
    #[serde(default)]
    pub market_cap_min: Option<f64>,
    /// Maximum market cap in 亿元. `None` = no constraint.
    #[serde(default)]
    pub market_cap_max: Option<f64>,
    /// Exclude delisted stocks (default true).
    #[serde(default = "default_exclude_delisted")]
    pub exclude_delisted: bool,
    /// Moving-average condition. `None` = disabled.
    #[serde(default)]
    pub ma: Option<MaCondition>,
    /// Breakout condition. `None` = disabled.
    #[serde(default)]
    pub breakout: Option<BreakoutCondition>,
    /// Momentum condition. `None` = disabled.
    #[serde(default)]
    pub momentum: Option<MomentumCondition>,
    /// Volume condition. `None` = disabled.
    #[serde(default)]
    pub volume: Option<VolumeCondition>,
}

fn default_exclude_delisted() -> bool {
    true
}

/// Default: all conditions empty, delisted stocks excluded.
impl Default for ScreenerQuery {
    fn default() -> Self {
        Self {
            industries: Vec::new(),
            exchanges: Vec::new(),
            boards: Vec::new(),
            list_years: None,
            market_cap_min: None,
            market_cap_max: None,
            exclude_delisted: true,
            ma: None,
            breakout: None,
            momentum: None,
            volume: None,
        }
    }
}

// --- Screener expression AST (epic #243) ------------------------------------
//
// LLM-friendly screener expression AST: a serializable tag-union of filter
// expressions consumed by the strategy engine and produced by the future LLM
// client (Batch 4). Definitions live in `screener.rs`.

mod screener;

pub use screener::{CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor, validate_filter};

/// One result row of a screener run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenerRow {
    /// Exchange-prefixed stock symbol (e.g. "SZ000001").
    pub symbol: String,
    /// Chinese display name.
    pub name: String,
    /// Latest raw close price.
    pub latest_price: f64,
    /// 20-day adjusted-close return in percent.
    pub change_20d: f64,
    /// Market cap in 亿元 (0.0 when total_share is missing).
    pub market_cap: f64,
    /// Industry classification.
    pub industry: String,
}

// --- SEPA engine contract types (epic #139) ---------------------------------
//
// Shared boundary types of the SEPA (Stage Analysis + VCP) scoring engine,
// consumed by the engine (compass-strategy), the CLI and the GUI. These are
// deliberately NOT serde-serializable: the GUI renders them as-is without any
// parsing, and no persistence/configuration path needs them (unlike the
// `ScreenerQuery` family above).

/// Backend-side SEPA query. Currently only the result cap.
#[derive(Debug, Clone, PartialEq)]
pub struct SepaQuery {
    /// Backend truncation cap for the result list (default 50).
    pub top_n: usize,
}

/// One scoring sub-item of a SEPA module. The GUI renders it as-is without
/// any parsing (label + score/max as a bar, note as the raw value display).
///
/// The label and note carry **semantic i18n keys** instead of display text
/// (issue #222): the GUI resolves them via `t!()`, so the same scoring data
/// renders in every locale. `note_args` holds the positional numeric values
/// interpolated into the note template in declaration order (`%{0}` …);
/// unit/format precision is owned by the consuming renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct SepaFactor {
    /// i18n key of the sub-item label (e.g. `"sepa.factor.ma_structure"`).
    pub label_key: &'static str,
    /// Achieved sub-score.
    pub score: f64,
    /// Maximum possible sub-score.
    pub max: f64,
    /// i18n key of the optional raw-value note; `None` renders no note.
    pub note_key: Option<&'static str>,
    /// Numeric note arguments, positionally mapped onto the note template.
    /// `None` (or an empty vec) for notes without arguments.
    pub note_args: Option<Vec<f64>>,
}

/// Sub-item detail breakdown of the five SEPA scoring modules.
#[derive(Debug, Clone, PartialEq)]
pub struct SepaDetails {
    /// Trend module sub-items.
    pub trend: Vec<SepaFactor>,
    /// Theme module sub-items.
    pub theme: Vec<SepaFactor>,
    /// Capital module sub-items.
    pub capital: Vec<SepaFactor>,
    /// Pattern (VCP) module sub-items.
    pub pattern: Vec<SepaFactor>,
    /// Risk module sub-items.
    pub risk: Vec<SepaFactor>,
}

/// One SEPA result row (one ranked stock).
#[derive(Debug, Clone, PartialEq)]
pub struct SepaRow {
    /// Exchange-prefixed stock code (e.g. `SH600519`, `SZ000001`).
    pub symbol: String,
    /// Chinese display name.
    pub name: String,
    /// Final rank (1-based, after sorting).
    pub rank: usize,
    /// Total score in 0..100.
    pub total_score: f64,
    /// Trend module score (weighted contribution).
    pub trend: f64,
    /// Theme module score (weighted contribution).
    pub theme: f64,
    /// Capital module score (weighted contribution).
    pub capital: f64,
    /// Pattern module score (weighted contribution).
    pub pattern: f64,
    /// Risk module penalty in -3.75..0 (deduction contribution, review
    /// revision: at most 75 × 0.05).
    pub risk: f64,
    /// Industry classification.
    pub industry: String,
    /// Concept themes; may be empty.
    pub themes: Vec<String>,
    /// Latest raw close price.
    pub latest_price: f64,
    /// Day-over-day change percent (A-share red-up convention).
    pub change_pct: f64,
    /// Per-module sub-item breakdown.
    pub details: SepaDetails,
}

/// One thermometer indicator chip, rendered generically by the GUI.
///
/// The label and the value unit carry **semantic i18n keys** (issue #222):
/// the GUI resolves them via `t!()` and formats the raw `value` per the
/// unit-key precision contract (percent → 1 decimal, count → integer,
/// trillion → 2 decimals).
#[derive(Debug, Clone, PartialEq)]
pub struct SepaIndicator {
    /// i18n key of the indicator label (e.g. `"sepa.indicator.hs300_trend"`).
    pub label_key: &'static str,
    /// Raw numeric value in the unit named by `unit_key`.
    pub value: f64,
    /// i18n key of the value unit (`"sepa.unit.percent"`, `"sepa.unit.count"`
    /// or `"sepa.unit.trillion"`), driving the renderer's format precision.
    pub unit_key: &'static str,
    /// Change vs yesterday; `None` when not applicable. A-share coloring:
    /// red = up, green = down.
    pub delta_pct: Option<f64>,
    /// Heat value in 0..1 driving the color-scale tint.
    pub heat: f64,
}

/// Whole-market thermometer: market breadth/regime summary with 5 indicators.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketThermometer {
    /// Overall market score in 0..100.
    pub score: f64,
    /// i18n key of the position band (e.g. `"sepa.position.full"`).
    pub position_key: &'static str,
    /// Position band midpoint percent in 0..100.
    pub position_pct: f64,
    /// The 5 indicator chips.
    pub indicators: Vec<SepaIndicator>,
}

/// Full SEPA engine response payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SepaData {
    /// Complete ranked TOP-N rows (official order, no client re-sort).
    pub rows: Vec<SepaRow>,
    /// Whole-market thermometer.
    pub thermometer: MarketThermometer,
    /// Scoring date (e.g. "2026-08-02").
    pub date: String,
}

/// One index/board row of the market snapshot (epic #255 C4).
///
/// The market tab renders these in its ranking table (板块轮动) and the
/// core-index card consumes the `official` subset. `index_type` is one of
/// `"official"` / `"concept"` / `"industry"` (index_daily.parquet column).
#[derive(Debug, Clone, PartialEq)]
pub struct IndexRow {
    /// Exchange-prefixed symbol (e.g. `SH000001`, `BK0475`).
    pub symbol: String,
    /// Chinese display name (from index_basic.parquet).
    pub name: String,
    /// `"official"` | `"concept"` | `"industry"`.
    pub index_type: String,
    /// Latest close (点位).
    pub latest: f64,
    /// Day-over-day change percent vs the previous close (A-share red-up).
    pub change_pct: f64,
    /// Latest turnover in yuan (成交额).
    pub amount: f64,
}

/// Full market snapshot payload: every index/board symbol with its latest
/// quote, computed by the fourth `AsyncDispatcher` channel.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexSnapshot {
    /// All symbols (one row each), unsorted — the GUI filters/sorts locally.
    pub rows: Vec<IndexRow>,
    /// Snapshot date (latest tradedate, e.g. "2026-08-13"); empty when no data.
    pub date: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_query_excludes_delisted_and_has_no_conditions() {
        let q = ScreenerQuery::default();
        assert!(q.exclude_delisted);
        assert!(q.industries.is_empty());
        assert!(q.exchanges.is_empty());
        assert!(q.boards.is_empty());
        assert_eq!(q.list_years, None);
        assert_eq!(q.market_cap_min, None);
        assert_eq!(q.market_cap_max, None);
        assert_eq!(q.ma, None);
        assert_eq!(q.breakout, None);
        assert_eq!(q.momentum, None);
        assert_eq!(q.volume, None);
    }

    #[test]
    fn condition_defaults_match_contract() {
        assert_eq!(BreakoutCondition::default().days, 60);
        assert_eq!(MomentumCondition::default().days, 20);
        assert_eq!(MomentumCondition::default().min_pct, 0.0);
        assert_eq!(MomentumCondition::default().max_pct, 100.0);
        assert_eq!(VolumeCondition::default().days, 20);
        assert_eq!(VolumeCondition::default().times, 2.0);
    }

    #[test]
    fn serde_roundtrip_preserves_query() {
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string(), "银行".to_string()],
            exchanges: vec!["SH".to_string()],
            boards: vec!["主板".to_string()],
            list_years: Some(3),
            market_cap_min: Some(100.0),
            market_cap_max: Some(5000.0),
            exclude_delisted: true,
            ma: Some(MaCondition::BullishAlign),
            breakout: Some(BreakoutCondition::new(120)),
            momentum: Some(MomentumCondition::new(30, -5.0, 50.0)),
            volume: Some(VolumeCondition::new(10, 1.5)),
        };
        let toml_str = toml::to_string(&q).expect("serialize");
        let back: ScreenerQuery = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(back, q);
    }

    #[test]
    fn serde_roundtrip_preserves_enum_snake_case() {
        let q = ScreenerQuery {
            ma: Some(MaCondition::AboveMa20),
            ..ScreenerQuery::default()
        };
        let toml_str = toml::to_string(&q).expect("serialize");
        assert!(
            toml_str.contains("above_ma20"),
            "enum serialized snake_case: {toml_str}"
        );
        let back: ScreenerQuery = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(back.ma, Some(MaCondition::AboveMa20));
    }

    #[test]
    fn partial_section_missing_exclude_delisted_defaults_true() {
        // B2 regression: per-field default for the bool must be true,
        // not bool::default() = false.
        let src = "industries = [\"白酒\"]\n";
        let q: ScreenerQuery = toml::from_str(src).expect("partial section parses");
        assert!(
            q.exclude_delisted,
            "missing exclude_delisted must default true"
        );
        assert_eq!(q.industries, vec!["白酒".to_string()]);
    }

    #[test]
    fn empty_condition_table_uses_struct_default() {
        // B2 regression: `breakout = {}` must deserialize via the manual
        // Default impl instead of failing.
        let src = "breakout = {}\n";
        let q: ScreenerQuery = toml::from_str(src).expect("empty table parses");
        assert_eq!(q.breakout, Some(BreakoutCondition::default()));
        assert_eq!(q.breakout.unwrap().days, 60);
    }

    #[test]
    fn absent_section_uses_query_default() {
        let src = "";
        let q: ScreenerQuery = toml::from_str(src).expect("absent section parses");
        assert_eq!(q, ScreenerQuery::default());
    }

    #[test]
    fn explicit_false_respected() {
        let src = "exclude_delisted = false\n";
        let q: ScreenerQuery = toml::from_str(src).expect("parses");
        assert!(!q.exclude_delisted);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let src = "future_key = 42\n";
        let q: ScreenerQuery = toml::from_str(src).expect("unknown keys ignored");
        assert_eq!(q, ScreenerQuery::default());
    }
}
