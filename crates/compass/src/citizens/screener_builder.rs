//! Screener condition builder — Metabase-style AND/OR card group UI model.
//!
//! This module is the **view model** layer of the visual condition builder
//! (Epic #243 Batch 2, issue #245). The builder operates directly on the
//! Batch 1 `Filter` AST (`compass-types::screener`) instead of a bespoke
//! query model: cards are a *view* over the AST, and the two pure mapping
//! functions below are the round-trip bridge:
//!
//! - [`filter_to_items`] — reverse: `Filter` AST → card items (render/restore)
//! - [`leaf_to_filter`] / [`group_to_filter`] — forward: cards → `Filter` AST
//!
//! Round-trip equivalence (structural for multi-member/nested shapes, leaf-level
//! for bare single nodes) is the "no regression" guarantee — the existing
//! conditions must be expressible as cards without behavior change.

use compass_types::{CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor};

/// Boolean operator of a condition group (AND / OR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoolOp {
    /// All items must hold.
    #[default]
    And,
    /// At least one item must hold.
    Or,
}

/// One item in a condition group: either a single condition card or a nested group.
#[derive(Debug, Clone, PartialEq)]
pub enum CondItem {
    /// A single condition card.
    Leaf(CondLeaf),
    /// A nested AND/OR group (arbitrary depth).
    Group(CondGroup),
}

/// A nested group of condition items with a boolean operator.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CondGroup {
    /// AND or OR semantics for this group's items.
    pub operator: BoolOp,
    /// The group's children (cards and/or nested groups).
    pub items: Vec<CondItem>,
}

/// A single condition card.
#[derive(Debug, Clone, PartialEq)]
pub struct CondLeaf {
    /// Which condition template this card represents.
    pub kind: LeafKind,
    /// Card parameters interpreted by `kind`.
    pub params: LeafParams,
    /// Negated (`Not` wrapping) — the "exclude/取反" toggle.
    pub negated: bool,
}

/// The condition templates a card can represent.
///
/// Count is deliberately absent (deferred to a later batch, user-confirmed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeafKind {
    /// Industry multi-select (OR semantics, empty = no constraint).
    Industry,
    /// Exchange multi-select (fixed SH/SZ/BJ).
    Exchange,
    /// Board multi-select.
    Board,
    /// Listed for at least N years (不限 = `None`).
    ListYears,
    /// Market cap range in 亿元.
    MarketCap,
    /// Delisted exclusion checkbox (checked = card exists).
    Delisted,
    /// MA condition (above MA20 / above MA60 / bullish alignment).
    #[default]
    Ma,
    /// Close > N-day high breakout.
    Breakout,
    /// N-day return within [min, max].
    Momentum,
    /// Recent days-average volume ≥ times× baseline.
    VolumeSurge,
    /// N consecutive up days (通达信 UPNDAY style).
    UpDays,
    /// Unrecognized AST shape — read-only summary card (robustness fallback).
    Unknown,
}

/// Card parameters interpreted by [`LeafKind`].
#[derive(Debug, Clone, PartialEq, Default)]
pub enum LeafParams {
    /// Multi-select values (Industry / Exchange / Board).
    MultiSelect(Vec<String>),
    /// ListYears: 不限 = `None`.
    ListYears(Option<u32>),
    /// MarketCap range in 亿元; `None` side = unbounded.
    MarketCap { min: Option<f64>, max: Option<f64> },
    /// Delisted exclusion (checked = exclude).
    Delisted(bool),
    /// MA kind selection.
    Ma(MaKind),
    /// Breakout N-day high window.
    Breakout(u32),
    /// Momentum N-day return bounds.
    Momentum {
        days: u32,
        min_pct: f64,
        max_pct: f64,
    },
    /// VolumeSurge window / multiplier.
    VolumeSurge { days: u32, times: f64 },
    /// UpDays consecutive count / min daily gain.
    UpDays { n: u32, min_pct: f64 },
    /// Unknown AST shape: serialized JSON summary (read-only display).
    Unknown(String),
    /// Default: empty multi-select.
    #[default]
    None,
}

/// MA condition selector (mirrors the fixed-form enum, issue #245).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaKind {
    /// Close above the 20-day SMA.
    #[default]
    AboveMa20,
    /// Close above the 60-day SMA.
    AboveMa60,
    /// Bullish alignment: MA5 > MA20 > MA60.
    BullishAlign,
}

/// Reverse mapping: `Filter` AST → card items.
///
/// Recognizes the template shapes produced by `From<ScreenerQuery>` plus the
/// UPNDAY series card; folds single-member `And`/`Or` to the member itself;
/// wraps `Not` into `negated`; falls back to an [`LeafKind::Unknown`] card
/// for unrecognized shapes.
pub fn filter_to_items(f: &Filter) -> Vec<CondItem> {
    match f {
        Filter::Meta(m) => vec![CondItem::Leaf(meta_to_leaf(m))],
        Filter::Series(s) => vec![CondItem::Leaf(series_to_leaf(s))],
        Filter::And(v) => and_or_to_items(BoolOp::And, v),
        Filter::Or(v) => and_or_to_items(BoolOp::Or, v),
        Filter::Not(inner) => match recognize_leaf(inner) {
            Some(mut leaf) => {
                leaf.negated = true;
                vec![CondItem::Leaf(leaf)]
            }
            None => vec![CondItem::Leaf(unknown_leaf(inner))],
        },
    }
}

/// One `And`/`Or` node → items.
///
/// Order matters: a two-member node that matches a recognized pair shape
/// (momentum / bullish alignment) collapses into a single card; a
/// single-member node folds to its member's own items (no spurious subgroup);
/// anything else becomes a [`CondGroup`] of the recursively mapped members.
fn and_or_to_items(operator: BoolOp, members: &[Filter]) -> Vec<CondItem> {
    if members.is_empty() {
        return Vec::new();
    }
    if members.len() == 2 {
        if let Some(leaf) = try_momentum_pair(members) {
            return vec![CondItem::Leaf(leaf)];
        }
        if let Some(leaf) = try_bullish_pair(members) {
            return vec![CondItem::Leaf(leaf)];
        }
    }
    if members.len() == 1 {
        return filter_to_items(&members[0]);
    }
    let items = members.iter().flat_map(filter_to_items).collect();
    vec![CondItem::Group(CondGroup { operator, items })]
}

/// Recognize a single card shape from a filter node (`None` = unrecognized).
fn recognize_leaf(f: &Filter) -> Option<CondLeaf> {
    match f {
        Filter::Meta(m) => Some(meta_to_leaf(m)),
        Filter::Series(s) => Some(series_to_leaf(s)),
        Filter::And(v) if v.len() == 2 => try_momentum_pair(v).or_else(|| try_bullish_pair(v)),
        Filter::Not(inner) => recognize_leaf(inner),
        _ => None,
    }
}

/// A metadata constraint → its card.
fn meta_to_leaf(m: &MetaCond) -> CondLeaf {
    let (kind, params) = match m {
        MetaCond::Industry(v) => (LeafKind::Industry, LeafParams::MultiSelect(v.clone())),
        MetaCond::Exchange(v) => (LeafKind::Exchange, LeafParams::MultiSelect(v.clone())),
        MetaCond::Board(v) => (LeafKind::Board, LeafParams::MultiSelect(v.clone())),
        MetaCond::ListYears(n) => (LeafKind::ListYears, LeafParams::ListYears(Some(*n))),
        MetaCond::Delisted(b) => (LeafKind::Delisted, LeafParams::Delisted(*b)),
        MetaCond::MarketCap { min, max } => (
            LeafKind::MarketCap,
            LeafParams::MarketCap {
                min: *min,
                max: *max,
            },
        ),
    };
    CondLeaf {
        kind,
        params,
        negated: false,
    }
}

/// A series condition → its card (`Unknown` for shapes outside the templates).
fn series_to_leaf(s: &SeriesCond) -> CondLeaf {
    let (kind, params) = match s {
        SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        } => (LeafKind::Ma, LeafParams::Ma(MaKind::AboveMa20)),
        SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(60)),
        } => (LeafKind::Ma, LeafParams::Ma(MaKind::AboveMa60)),
        SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(n)),
        } => (LeafKind::Breakout, LeafParams::Breakout(*n)),
        SeriesCond::VolumeSurge { days, times } => (
            LeafKind::VolumeSurge,
            LeafParams::VolumeSurge {
                days: *days,
                times: *times,
            },
        ),
        SeriesCond::UpDays { n, min_pct } => (
            LeafKind::UpDays,
            LeafParams::UpDays {
                n: *n,
                min_pct: *min_pct,
            },
        ),
        _ => return unknown_leaf_from(s),
    };
    CondLeaf {
        kind,
        params,
        negated: false,
    }
}

/// Build an `Unknown` summary card from an unrecognized series condition.
fn unknown_leaf_from(s: &SeriesCond) -> CondLeaf {
    CondLeaf {
        kind: LeafKind::Unknown,
        params: LeafParams::Unknown(format!("{s:?}")),
        negated: false,
    }
}

/// Build an `Unknown` summary card from an unrecognized filter node.
fn unknown_leaf(f: &Filter) -> CondLeaf {
    CondLeaf {
        kind: LeafKind::Unknown,
        params: LeafParams::Unknown(format!("{f:?}")),
        negated: false,
    }
}

/// Momentum pair recognition: `And[Cmp{ChangePct(n),Ge,Const(min)},
/// Cmp{ChangePct(n),Le,Const(max)}]` with identical `n` (Ge first).
fn try_momentum_pair(members: &[Filter]) -> Option<CondLeaf> {
    match (&members[0], &members[1]) {
        (
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(n1),
                op: CmpOp::Ge,
                value: FactorRef::Const(min),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(n2),
                op: CmpOp::Le,
                value: FactorRef::Const(max),
            }),
        ) if n1 == n2 => Some(CondLeaf {
            kind: LeafKind::Momentum,
            params: LeafParams::Momentum {
                days: *n1,
                min_pct: *min,
                max_pct: *max,
            },
            negated: false,
        }),
        _ => None,
    }
}

/// Bullish-alignment pair recognition: `And[Cmp{Sma(5),Gt,Sma(20)},
/// Cmp{Sma(20),Gt,Sma(60)}]`.
fn try_bullish_pair(members: &[Filter]) -> Option<CondLeaf> {
    match (&members[0], &members[1]) {
        (
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
        ) => Some(CondLeaf {
            kind: LeafKind::Ma,
            params: LeafParams::Ma(MaKind::BullishAlign),
            negated: false,
        }),
        _ => None,
    }
}

/// Forward mapping: one card → its `Filter` AST shape.
///
/// `negated` wraps the result in `Filter::Not`. Unknown cards cannot be
/// rebuilt (they carry only a JSON summary) — they map to `Filter::And(vec![])`.
pub fn leaf_to_filter(l: &CondLeaf) -> Filter {
    let base = match (&l.kind, &l.params) {
        (LeafKind::Industry, LeafParams::MultiSelect(v)) => {
            Filter::Meta(MetaCond::Industry(v.clone()))
        }
        (LeafKind::Exchange, LeafParams::MultiSelect(v)) => {
            Filter::Meta(MetaCond::Exchange(v.clone()))
        }
        (LeafKind::Board, LeafParams::MultiSelect(v)) => Filter::Meta(MetaCond::Board(v.clone())),
        (LeafKind::ListYears, LeafParams::ListYears(Some(n))) => {
            Filter::Meta(MetaCond::ListYears(*n))
        }
        (LeafKind::ListYears, LeafParams::ListYears(None)) => {
            // 不限 = "listed ≥ 0 years" — a no-op constraint that keeps the
            // node expressible (never emitted by `From` for `None`).
            Filter::Meta(MetaCond::ListYears(0))
        }
        (LeafKind::MarketCap, LeafParams::MarketCap { min, max }) => {
            Filter::Meta(MetaCond::MarketCap {
                min: *min,
                max: *max,
            })
        }
        (LeafKind::Delisted, LeafParams::Delisted(b)) => Filter::Meta(MetaCond::Delisted(*b)),
        (LeafKind::Ma, LeafParams::Ma(MaKind::AboveMa20)) => Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        }),
        (LeafKind::Ma, LeafParams::Ma(MaKind::AboveMa60)) => Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(60)),
        }),
        (LeafKind::Ma, LeafParams::Ma(MaKind::BullishAlign)) => Filter::And(vec![
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
        ]),
        (LeafKind::Breakout, LeafParams::Breakout(n)) => Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(*n)),
        }),
        (
            LeafKind::Momentum,
            LeafParams::Momentum {
                days,
                min_pct,
                max_pct,
            },
        ) => Filter::And(vec![
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(*days),
                op: CmpOp::Ge,
                value: FactorRef::Const(*min_pct),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(*days),
                op: CmpOp::Le,
                value: FactorRef::Const(*max_pct),
            }),
        ]),
        (LeafKind::VolumeSurge, LeafParams::VolumeSurge { days, times }) => {
            Filter::Series(SeriesCond::VolumeSurge {
                days: *days,
                times: *times,
            })
        }
        (LeafKind::UpDays, LeafParams::UpDays { n, min_pct }) => {
            Filter::Series(SeriesCond::UpDays {
                n: *n,
                min_pct: *min_pct,
            })
        }
        // Unknown (and any kind/params mismatch) cannot be rebuilt — an empty
        // And is a semantic no-op; the negated flag is deliberately ignored
        // here (`Not(And([]))` would match nothing, a silent data-loss bug).
        _ => return Filter::And(Vec::new()),
    };
    if l.negated { base.negate() } else { base }
}

/// Forward mapping: a group → its `Filter` AST shape.
///
/// Single-member groups emit the bare member node (aligned with
/// `From<ScreenerQuery>`'s `1 => nodes.pop()` shape, for both `And` and
/// `Or`); empty groups emit `Filter::And(vec![])`.
pub fn group_to_filter(g: &CondGroup) -> Filter {
    if g.items.is_empty() {
        return Filter::And(Vec::new());
    }
    if g.items.len() == 1 {
        return item_to_filter(&g.items[0]);
    }
    let children: Vec<Filter> = g.items.iter().map(item_to_filter).collect();
    match g.operator {
        BoolOp::And => Filter::And(children),
        BoolOp::Or => Filter::Or(children),
    }
}

/// One card item → its `Filter` AST shape.
fn item_to_filter(item: &CondItem) -> Filter {
    match item {
        CondItem::Leaf(l) => leaf_to_filter(l),
        CondItem::Group(g) => group_to_filter(g),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compass_types::{
        BreakoutCondition, MaCondition, MomentumCondition, ScreenerQuery, VolumeCondition,
    };

    // --- helpers ------------------------------------------------------------

    /// A query with every condition empty and delisted exclusion disabled, so
    /// that single-condition recognition tests observe exactly one emitted AST
    /// node (mirrors the `bare_query` helper in compass-types screener.rs
    /// tests).
    fn bare_query() -> ScreenerQuery {
        ScreenerQuery {
            exclude_delisted: false,
            ..ScreenerQuery::default()
        }
    }

    /// Unwrap the single expected leaf card from `filter_to_items` output.
    fn sole_leaf(items: &[CondItem]) -> &CondLeaf {
        assert_eq!(items.len(), 1, "expected exactly one card, got: {items:?}");
        match &items[0] {
            CondItem::Leaf(leaf) => leaf,
            CondItem::Group(g) => panic!("expected a leaf card, got a group: {g:?}"),
        }
    }

    // --- 1. Recognition: every shape produced by From<ScreenerQuery> ---------
    // Acceptance (issue #245): existing basic conditions must remain
    // expressible — "现有基础条件功能不回归". Every output shape of
    // `From<ScreenerQuery>` (crates/compass-types/src/screener.rs L221-307)
    // must be recognized by `filter_to_items` as its corresponding LeafKind
    // card.

    #[test]
    fn from_industries_recognized_as_industry_card() {
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string(), "银行".to_string()],
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Industry);
        assert_eq!(
            leaf.params,
            LeafParams::MultiSelect(vec!["白酒".to_string(), "银行".to_string()])
        );
        assert!(!leaf.negated, "From shapes are never negated");
    }

    #[test]
    fn from_exchanges_recognized_as_exchange_card() {
        let q = ScreenerQuery {
            exchanges: vec!["SH".to_string(), "SZ".to_string()],
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Exchange);
        assert_eq!(
            leaf.params,
            LeafParams::MultiSelect(vec!["SH".to_string(), "SZ".to_string()])
        );
        assert!(!leaf.negated);
    }

    #[test]
    fn from_boards_recognized_as_board_card() {
        let q = ScreenerQuery {
            boards: vec!["主板".to_string()],
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Board);
        assert_eq!(
            leaf.params,
            LeafParams::MultiSelect(vec!["主板".to_string()])
        );
        assert!(!leaf.negated);
    }

    #[test]
    fn from_list_years_recognized_as_list_years_card() {
        let q = ScreenerQuery {
            list_years: Some(3),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::ListYears);
        assert_eq!(leaf.params, LeafParams::ListYears(Some(3)));
        assert!(!leaf.negated);
    }

    #[test]
    fn from_market_cap_recognized_as_market_cap_card() {
        let q = ScreenerQuery {
            market_cap_min: Some(100.0),
            market_cap_max: None,
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::MarketCap);
        assert_eq!(
            leaf.params,
            LeafParams::MarketCap {
                min: Some(100.0),
                max: None,
            }
        );
        assert!(!leaf.negated);
    }

    #[test]
    fn from_exclude_delisted_recognized_as_delisted_card() {
        // Default query (exclude_delisted = true, nothing else) compiles to a
        // bare Meta(Delisted(false)) node — must be recognized as a Delisted
        // card (checked = exclude).
        let items = filter_to_items(&Filter::from(ScreenerQuery::default()));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Delisted);
        assert!(!leaf.negated);
    }

    #[test]
    fn from_ma_above_ma20_recognized_as_ma_card() {
        let q = ScreenerQuery {
            ma: Some(MaCondition::AboveMa20),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Ma);
        assert_eq!(leaf.params, LeafParams::Ma(MaKind::AboveMa20));
        assert!(!leaf.negated);
    }

    #[test]
    fn from_ma_above_ma60_recognized_as_ma_card() {
        let q = ScreenerQuery {
            ma: Some(MaCondition::AboveMa60),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Ma);
        assert_eq!(leaf.params, LeafParams::Ma(MaKind::AboveMa60));
        assert!(!leaf.negated);
    }

    #[test]
    fn from_ma_bullish_recognized_as_ma_card() {
        // BullishAlign compiles to a nested And[Sma5>Gt>Sma20, Sma20>Gt>Sma60]
        // pair (screener.rs L260-271) — the pair must fold into ONE Ma card,
        // not a nested group.
        let q = ScreenerQuery {
            ma: Some(MaCondition::BullishAlign),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Ma);
        assert_eq!(leaf.params, LeafParams::Ma(MaKind::BullishAlign));
        assert!(!leaf.negated);
    }

    #[test]
    fn from_breakout_recognized_as_breakout_card() {
        let q = ScreenerQuery {
            breakout: Some(BreakoutCondition::new(60)),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Breakout);
        assert_eq!(leaf.params, LeafParams::Breakout(60));
        assert!(!leaf.negated);
    }

    #[test]
    fn from_momentum_recognized_as_momentum_card() {
        // Momentum compiles to a nested And of two ChangePct Cmps (Ge then Le)
        // sharing the same window (screener.rs L281-294) — the pair must fold
        // into ONE Momentum card.
        let q = ScreenerQuery {
            momentum: Some(MomentumCondition::new(30, -5.0, 50.0)),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Momentum);
        assert_eq!(
            leaf.params,
            LeafParams::Momentum {
                days: 30,
                min_pct: -5.0,
                max_pct: 50.0,
            }
        );
        assert!(!leaf.negated);
    }

    #[test]
    fn from_volume_recognized_as_volume_surge_card() {
        let q = ScreenerQuery {
            volume: Some(VolumeCondition::new(10, 1.5)),
            ..bare_query()
        };
        let items = filter_to_items(&Filter::from(q));
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::VolumeSurge);
        assert_eq!(
            leaf.params,
            LeafParams::VolumeSurge {
                days: 10,
                times: 1.5
            }
        );
        assert!(!leaf.negated);
    }

    // --- 2. Forward mapping: leaf_to_filter builds the template AST ----------
    // Each LeafKind + params must build the exact AST shape from the template
    // table (.dsh/designs/llm-screener-ui.md §2), matching the shapes produced
    // by From<ScreenerQuery> (screener.rs L221-307).

    #[test]
    fn leaf_industry_builds_meta_industry() {
        let leaf = CondLeaf {
            kind: LeafKind::Industry,
            params: LeafParams::MultiSelect(vec!["白酒".to_string()]),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()]))
        );
    }

    #[test]
    fn leaf_exchange_builds_meta_exchange() {
        let leaf = CondLeaf {
            kind: LeafKind::Exchange,
            params: LeafParams::MultiSelect(vec!["SH".to_string(), "SZ".to_string()]),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Meta(MetaCond::Exchange(vec!["SH".to_string(), "SZ".to_string()]))
        );
    }

    #[test]
    fn leaf_board_builds_meta_board() {
        let leaf = CondLeaf {
            kind: LeafKind::Board,
            params: LeafParams::MultiSelect(vec!["主板".to_string()]),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Meta(MetaCond::Board(vec!["主板".to_string()]))
        );
    }

    #[test]
    fn leaf_list_years_builds_meta_list_years() {
        let leaf = CondLeaf {
            kind: LeafKind::ListYears,
            params: LeafParams::ListYears(Some(3)),
            negated: false,
        };
        assert_eq!(leaf_to_filter(&leaf), Filter::Meta(MetaCond::ListYears(3)));
    }

    #[test]
    fn leaf_market_cap_builds_meta_market_cap() {
        let leaf = CondLeaf {
            kind: LeafKind::MarketCap,
            params: LeafParams::MarketCap {
                min: Some(100.0),
                max: None,
            },
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Meta(MetaCond::MarketCap {
                min: Some(100.0),
                max: None,
            })
        );
    }

    #[test]
    fn leaf_ma_above_ma20_builds_close_gt_sma20() {
        let leaf = CondLeaf {
            kind: LeafKind::Ma,
            params: LeafParams::Ma(MaKind::AboveMa20),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(20)),
            })
        );
    }

    #[test]
    fn leaf_ma_above_ma60_builds_close_gt_sma60() {
        let leaf = CondLeaf {
            kind: LeafKind::Ma,
            params: LeafParams::Ma(MaKind::AboveMa60),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(60)),
            })
        );
    }

    #[test]
    fn leaf_ma_bullish_builds_alignment_pair() {
        // Engine semantics (C2 revision, screener.rs L258-271): ma5 > ma20 &&
        // ma20 > ma60 — NOT Close > Sma20.
        let leaf = CondLeaf {
            kind: LeafKind::Ma,
            params: LeafParams::Ma(MaKind::BullishAlign),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
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
        );
    }

    #[test]
    fn leaf_breakout_builds_close_gt_n_day_high() {
        let leaf = CondLeaf {
            kind: LeafKind::Breakout,
            params: LeafParams::Breakout(60),
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::NDayHigh(60)),
            })
        );
    }

    #[test]
    fn leaf_momentum_builds_bounds_pair() {
        let leaf = CondLeaf {
            kind: LeafKind::Momentum,
            params: LeafParams::Momentum {
                days: 30,
                min_pct: -5.0,
                max_pct: 50.0,
            },
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::And(vec![
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::ChangePct(30),
                    op: CmpOp::Ge,
                    value: FactorRef::Const(-5.0),
                }),
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::ChangePct(30),
                    op: CmpOp::Le,
                    value: FactorRef::Const(50.0),
                }),
            ])
        );
    }

    #[test]
    fn leaf_volume_surge_builds_series() {
        let leaf = CondLeaf {
            kind: LeafKind::VolumeSurge,
            params: LeafParams::VolumeSurge {
                days: 10,
                times: 1.5,
            },
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Series(SeriesCond::VolumeSurge {
                days: 10,
                times: 1.5
            })
        );
    }

    #[test]
    fn leaf_up_days_builds_series() {
        let leaf = CondLeaf {
            kind: LeafKind::UpDays,
            params: LeafParams::UpDays { n: 3, min_pct: 1.0 },
            negated: false,
        };
        assert_eq!(
            leaf_to_filter(&leaf),
            Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.0 })
        );
    }

    // --- 3. Round-trip guarantee (the builder operates on the Filter AST) ----

    #[test]
    fn multi_member_query_round_trips_structurally() {
        // industries + exclude_delisted + BullishAlign + momentum → top-level
        // And of 4 nodes (screener.rs L717-758). filter_to_items → root group
        // → group_to_filter must reproduce the original Filter exactly.
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ma: Some(MaCondition::BullishAlign),
            momentum: Some(MomentumCondition::new(20, 0.0, 100.0)),
            ..ScreenerQuery::default() // exclude_delisted defaults to true
        };
        let f = Filter::from(q);
        let root = CondGroup {
            operator: BoolOp::And,
            items: filter_to_items(&f),
        };
        assert_eq!(
            group_to_filter(&root),
            f,
            "multi-member filter must round-trip structurally (cards → AST == original)"
        );
    }

    #[test]
    fn bare_delisted_leaf_round_trips() {
        // Bare single node (the From output of the default query): exactly one
        // Delisted card, and leaf-level round-trip (no group wrapping).
        let f = Filter::Meta(MetaCond::Delisted(false));
        let items = filter_to_items(&f);
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Delisted);
        assert!(!leaf.negated);
        assert_eq!(leaf_to_filter(leaf), f);
    }

    #[test]
    fn bare_volume_surge_leaf_round_trips() {
        let f = Filter::Series(SeriesCond::VolumeSurge {
            days: 10,
            times: 1.5,
        });
        let items = filter_to_items(&f);
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::VolumeSurge);
        assert_eq!(leaf_to_filter(leaf), f);
    }

    // --- 4. Grouping / nesting ------------------------------------------------

    #[test]
    fn and_group_round_trips() {
        let f = Filter::And(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            Filter::Series(SeriesCond::VolumeSurge {
                days: 10,
                times: 1.5,
            }),
        ]);
        let root = CondGroup {
            operator: BoolOp::And,
            items: filter_to_items(&f),
        };
        assert_eq!(group_to_filter(&root), f);
    }

    #[test]
    fn or_group_round_trips() {
        // An Or node must surface as an Or group (operator preserved through
        // the round-trip — the And-root default must not swallow it).
        let f = Filter::Or(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            Filter::Meta(MetaCond::Exchange(vec!["SH".to_string()])),
        ]);
        let root = CondGroup {
            operator: BoolOp::And,
            items: filter_to_items(&f),
        };
        assert_eq!(group_to_filter(&root), f);
    }

    #[test]
    fn nested_two_level_round_trips() {
        // Acceptance: AND/OR nesting of at least 2 levels, with a negated
        // leaf — And[Or[Industry, VolumeSurge], Not(Industry)].
        let f = Filter::And(vec![
            Filter::Or(vec![
                Filter::Meta(MetaCond::Industry(vec!["a".to_string()])),
                Filter::Series(SeriesCond::VolumeSurge {
                    days: 5,
                    times: 3.0,
                }),
            ]),
            Filter::Not(Box::new(Filter::Meta(MetaCond::Industry(vec![
                "b".to_string(),
            ])))),
        ]);
        let root = CondGroup {
            operator: BoolOp::And,
            items: filter_to_items(&f),
        };
        assert_eq!(group_to_filter(&root), f);
    }

    #[test]
    fn nested_three_level_round_trips() {
        // Deep 3-level And/Or mix (plan Todo 1: 深层嵌套 3 层 And/Or 混合).
        let f = Filter::And(vec![
            Filter::Or(vec![
                Filter::And(vec![
                    Filter::Meta(MetaCond::Industry(vec!["a".to_string()])),
                    Filter::Meta(MetaCond::Board(vec!["主板".to_string()])),
                ]),
                Filter::Series(SeriesCond::VolumeSurge {
                    days: 5,
                    times: 3.0,
                }),
            ]),
            Filter::Meta(MetaCond::Industry(vec!["b".to_string()])),
        ]);
        let root = CondGroup {
            operator: BoolOp::And,
            items: filter_to_items(&f),
        };
        assert_eq!(group_to_filter(&root), f);
    }

    // --- 5. Negation toggle ---------------------------------------------------
    // Acceptance: 取反开关 — Not(x) must set negated = true on the recognized
    // card, and leaf_to_filter must wrap the result back in Not.

    #[test]
    fn not_wrapped_leaf_is_negated_and_round_trips() {
        let f = Filter::Not(Box::new(Filter::Meta(MetaCond::Industry(vec![]))));
        let items = filter_to_items(&f);
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Industry);
        assert!(
            leaf.negated,
            "Not-wrapped shape must set the negated toggle"
        );
        assert_eq!(leaf_to_filter(leaf), f);
    }

    // --- 6. Error paths / robustness ------------------------------------------

    #[test]
    fn unknown_shape_produces_summary_card_without_panic() {
        // Count is deferred (no LeafKind::Count in this batch) — an
        // unrecognized shape must degrade to a read-only Unknown summary card,
        // never panic.
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 10,
            at_least: 5,
        });
        let items = filter_to_items(&f);
        let leaf = sole_leaf(&items);
        assert_eq!(leaf.kind, LeafKind::Unknown);
        assert!(
            matches!(leaf.params, LeafParams::Unknown(_)),
            "Unknown card carries a JSON summary"
        );
    }

    #[test]
    fn empty_filter_produces_no_items_without_panic() {
        // An empty And (the From output of an empty query) renders as an empty
        // root group — the UI shows the empty state. Must not panic.
        let items = filter_to_items(&Filter::And(Vec::new()));
        assert!(
            items.is_empty(),
            "empty filter must yield no cards, got: {items:?}"
        );
    }

    // --- 7. Folding rules (plan Todo 1 acceptance) ----------------------------

    #[test]
    fn single_member_and_folds_to_bare_card_shape() {
        let bare = Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()]));
        let wrapped = Filter::And(vec![bare.clone()]);
        let items_bare = filter_to_items(&bare);
        let items_wrapped = filter_to_items(&wrapped);
        // Single-member And/Or folding: And(vec![x]) renders the same single
        // card as the bare node x (no spurious nested group).
        assert_eq!(items_wrapped, items_bare);
        assert_eq!(
            items_wrapped.len(),
            1,
            "fold must yield one card, got: {items_wrapped:?}"
        );
    }

    #[test]
    fn momentum_pair_with_different_days_is_not_combined() {
        // Two ChangePct Cmps with different windows must NOT collapse into one
        // momentum card (pair recognition requires the same factor).
        let f = Filter::And(vec![
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(20),
                op: CmpOp::Ge,
                value: FactorRef::Const(0.0),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(30),
                op: CmpOp::Le,
                value: FactorRef::Const(100.0),
            }),
        ]);
        let items = filter_to_items(&f);
        assert!(
            !items.is_empty(),
            "an And of two conditions must yield cards, got: {items:?}"
        );
        let is_sole_momentum = items.len() == 1
            && matches!(
                &items[0],
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Momentum,
                    ..
                })
            );
        assert!(
            !is_sole_momentum,
            "different-n Cmps must not combine into one momentum card"
        );
    }

    #[test]
    fn momentum_pair_with_reversed_ops_is_not_combined() {
        // Le-before-Ge ordering must not be recognized as a momentum pair
        // (pair recognition requires Ge then Le).
        let f = Filter::And(vec![
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(30),
                op: CmpOp::Le,
                value: FactorRef::Const(50.0),
            }),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::ChangePct(30),
                op: CmpOp::Ge,
                value: FactorRef::Const(-5.0),
            }),
        ]);
        let items = filter_to_items(&f);
        assert!(
            !items.is_empty(),
            "an And of two conditions must yield cards, got: {items:?}"
        );
        let is_sole_momentum = items.len() == 1
            && matches!(
                &items[0],
                CondItem::Leaf(CondLeaf {
                    kind: LeafKind::Momentum,
                    ..
                })
            );
        assert!(
            !is_sole_momentum,
            "reversed Le/Ge order must not combine into one momentum card"
        );
    }
}
