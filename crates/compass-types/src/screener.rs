//! Screener expression AST types (epic #243).
//!
//! A serializable tag-union of filter expressions shared by the GUI, the
//! strategy engine and the future LLM client (Batch 4). The AST is the
//! on-the-wire/config format: `ScreenerQuery` (legacy) compiles into this AST
//! via `From<ScreenerQuery> for Filter`, and the engine consumes it back.

use serde::{Deserialize, Serialize};

use super::{MaCondition, ScreenerQuery};

/// A screener filter expression.
///
/// Recursive tag-union: metadata constraints (`Meta`), series conditions
/// (`Series`) and boolean combinators (`And`/`Or`/`Not`). The AST serializes
/// to JSON as tagged values, shared by config persistence and LLM output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    /// Metadata constraint (industry, exchange, board, market cap, ...).
    Meta(MetaCond),
    /// Series (price/volume) condition.
    Series(SeriesCond),
    /// All sub-filters must hold.
    And(Vec<Filter>),
    /// At least one sub-filter must hold.
    Or(Vec<Filter>),
    /// Negated sub-filter.
    Not(Box<Filter>),
}

impl Filter {
    /// All of `self` and `other` must hold. Chained `And` operands flatten
    /// into a single `And` (left-associative `(a & b) & c` yields one level).
    pub fn and(self, other: Filter) -> Filter {
        match self {
            Filter::And(mut v) => {
                v.push(other);
                Filter::And(v)
            }
            _ => Filter::And(vec![self, other]),
        }
    }

    /// At least one of `self` or `other` must hold. Chained `Or` operands
    /// flatten into a single `Or` (left-associative `(a | b) | c` yields one
    /// level).
    pub fn or(self, other: Filter) -> Filter {
        match self {
            Filter::Or(mut v) => {
                v.push(other);
                Filter::Or(v)
            }
            _ => Filter::Or(vec![self, other]),
        }
    }

    /// Negation of `self`. Named `negate` (not `not`) to avoid clashing with
    /// the `std::ops::Not::not` trait method (clippy::should_implement_trait).
    pub fn negate(self) -> Filter {
        Filter::Not(Box::new(self))
    }
}

/// `a & b` builds an `And` combinator.
impl std::ops::BitAnd for Filter {
    type Output = Filter;

    fn bitand(self, rhs: Filter) -> Filter {
        self.and(rhs)
    }
}

/// `a | b` builds an `Or` combinator.
impl std::ops::BitOr for Filter {
    type Output = Filter;

    fn bitor(self, rhs: Filter) -> Filter {
        self.or(rhs)
    }
}

/// `~a` builds a `Not` combinator.
impl std::ops::Not for Filter {
    type Output = Filter;

    fn not(self) -> Filter {
        Filter::Not(Box::new(self))
    }
}

/// Metadata constraint on a stock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetaCond {
    /// Industry must be in the set (OR semantics; empty set = no constraint).
    Industry(Vec<String>),
    /// Exchange must be in the set, e.g. "SH"/"SZ"/"BJ" (OR semantics).
    Exchange(Vec<String>),
    /// Board must be in the set, e.g. "主板"/"创业板" (OR semantics).
    Board(Vec<String>),
    /// Listed for at least N years.
    ListYears(u32),
    /// Whether delisted stocks are excluded.
    Delisted(bool),
    /// Market cap range in 亿元; `None` side means unbounded.
    MarketCap {
        /// Minimum market cap in 亿元 (inclusive).
        min: Option<f64>,
        /// Maximum market cap in 亿元 (inclusive).
        max: Option<f64>,
    },
}

/// Default: empty industry set (no industry constraint).
impl Default for MetaCond {
    fn default() -> Self {
        Self::Industry(Vec::new())
    }
}

/// A price/volume series factor used as a comparison operand.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SeriesFactor {
    /// Latest adjusted close.
    Close,
    /// N-day simple moving average.
    Sma(u32),
    /// N-day return in percent.
    ChangePct(u32),
    /// Day-over-day change percent (A-share red-up convention).
    DayPct,
    /// N-day average volume.
    AvgVolume(u32),
    /// N-day high (breakout reference).
    NDayHigh(u32),
}

/// Default: the latest adjusted close.
impl Default for SeriesFactor {
    fn default() -> Self {
        Self::Close
    }
}

/// Comparison operator between a factor and a reference value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    /// Equal to.
    Eq,
    /// Not equal to.
    Ne,
    /// Greater than.
    Gt,
    /// Greater than or equal to.
    Ge,
    /// Less than.
    Lt,
    /// Less than or equal to.
    Le,
}

/// Right-hand side of a factor comparison: a constant or another factor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FactorRef {
    /// Plain numeric constant.
    Const(f64),
    /// Another series factor (e.g. Close > Sma(20)).
    Factor(SeriesFactor),
}

/// A condition evaluated on a price/volume series.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SeriesCond {
    /// Factor compared against a reference value.
    Cmp {
        /// Left-hand series factor.
        factor: SeriesFactor,
        /// Comparison operator.
        op: CmpOp,
        /// Right-hand reference: constant or another factor.
        value: FactorRef,
    },
    /// At least `n` consecutive days each rising more than `min_pct` percent.
    UpDays {
        /// Consecutive up days required.
        n: u32,
        /// Minimum daily gain in percent (exclusive).
        min_pct: f64,
    },
    /// `at_least` of the last `window` days satisfy the comparison.
    Count {
        /// Series factor tested per day.
        factor: SeriesFactor,
        /// Comparison operator.
        op: CmpOp,
        /// Right-hand reference: constant or another factor.
        value: FactorRef,
        /// Lookback window in trading days.
        window: u32,
        /// Minimum number of qualifying days.
        at_least: u32,
    },
    /// Recent `days`-day average volume is at least `times`× the baseline.
    VolumeSurge {
        /// Recent window in trading days.
        days: u32,
        /// Multiplier against the baseline average.
        times: f64,
    },
}

/// Compile the legacy 11-field `ScreenerQuery` into the expression AST.
///
/// One-way conversion (the reverse is a restricted accept-grammar inside
/// compass-strategy, Batch 3): each condition maps to one or more nodes in
/// `ScreenerQuery` field order, and all nodes AND together. `exclude_delisted`
/// is compiled to `Delisted(false)` — the engine reads "Delisted(false)
/// present" as "delisted excluded" and "absent" as "not excluded", which is
/// why the locked default query (exclude_delisted = true) emits a bare
/// `Meta(Delisted(false))` rather than an empty `And`.
impl From<ScreenerQuery> for Filter {
    fn from(query: ScreenerQuery) -> Self {
        let mut nodes: Vec<Filter> = Vec::new();

        if !query.industries.is_empty() {
            nodes.push(Filter::Meta(MetaCond::Industry(query.industries)));
        }
        if !query.exchanges.is_empty() {
            nodes.push(Filter::Meta(MetaCond::Exchange(query.exchanges)));
        }
        if !query.boards.is_empty() {
            nodes.push(Filter::Meta(MetaCond::Board(query.boards)));
        }
        if let Some(years) = query.list_years {
            nodes.push(Filter::Meta(MetaCond::ListYears(years)));
        }
        if query.market_cap_min.is_some() || query.market_cap_max.is_some() {
            nodes.push(Filter::Meta(MetaCond::MarketCap {
                min: query.market_cap_min,
                max: query.market_cap_max,
            }));
        }
        if query.exclude_delisted {
            nodes.push(Filter::Meta(MetaCond::Delisted(false)));
        }
        if let Some(ma) = query.ma {
            nodes.push(match ma {
                MaCondition::AboveMa20 => Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::Close,
                    op: CmpOp::Gt,
                    value: FactorRef::Factor(SeriesFactor::Sma(20)),
                }),
                MaCondition::AboveMa60 => Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::Close,
                    op: CmpOp::Gt,
                    value: FactorRef::Factor(SeriesFactor::Sma(60)),
                }),
                // Engine semantics: ma5 > ma20 && ma20 > ma60 (not
                // Close > Sma20; C2 revision, lib.rs:234-238).
                MaCondition::BullishAlign => Filter::And(vec![
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
            });
        }
        if let Some(bc) = query.breakout {
            nodes.push(Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::NDayHigh(bc.days)),
            }));
        }
        if let Some(mc) = query.momentum {
            nodes.push(Filter::And(vec![
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::ChangePct(mc.days),
                    op: CmpOp::Ge,
                    value: FactorRef::Const(mc.min_pct),
                }),
                Filter::Series(SeriesCond::Cmp {
                    factor: SeriesFactor::ChangePct(mc.days),
                    op: CmpOp::Le,
                    value: FactorRef::Const(mc.max_pct),
                }),
            ]));
        }
        if let Some(vc) = query.volume {
            nodes.push(Filter::Series(SeriesCond::VolumeSurge {
                days: vc.days,
                times: vc.times,
            }));
        }

        match nodes.len() {
            0 => Filter::And(Vec::new()),
            1 => nodes.pop().unwrap_or(Filter::And(Vec::new())),
            _ => Filter::And(nodes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BreakoutCondition, MaCondition, MomentumCondition, ScreenerQuery, VolumeCondition,
    };

    /// An empty query with no conditions and delisted exclusion disabled, so
    /// that single-condition mapping tests observe exactly one emitted node.
    fn bare_query() -> ScreenerQuery {
        ScreenerQuery {
            exclude_delisted: false,
            ..ScreenerQuery::default()
        }
    }

    /// Serialize a filter to JSON and parse it back.
    fn roundtrip(filter: &Filter) -> Filter {
        let json = serde_json::to_string(filter).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn meta_industry_roundtrip() {
        let f = Filter::Meta(MetaCond::Industry(vec![
            "白酒".to_string(),
            "银行".to_string(),
        ]));
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn meta_exchange_roundtrip() {
        let f = Filter::Meta(MetaCond::Exchange(vec!["SH".to_string(), "SZ".to_string()]));
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn meta_board_roundtrip() {
        let f = Filter::Meta(MetaCond::Board(vec!["主板".to_string()]));
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn meta_list_years_roundtrip() {
        let f = Filter::Meta(MetaCond::ListYears(3));
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn meta_delisted_roundtrip() {
        let f = Filter::Meta(MetaCond::Delisted(false));
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn meta_market_cap_roundtrip() {
        let f = Filter::Meta(MetaCond::MarketCap {
            min: Some(100.0),
            max: Some(5000.0),
        });
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn series_cmp_const_value_roundtrip() {
        // Momentum shape: value is a plain number.
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::ChangePct(20),
            op: CmpOp::Ge,
            value: FactorRef::Const(0.0),
        });
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn series_cmp_factor_value_roundtrip() {
        // MA / breakout shape: value is another series factor.
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        });
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn series_up_days_roundtrip() {
        let f = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.0 });
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn series_count_roundtrip() {
        let f = Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 10,
            at_least: 5,
        });
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn series_volume_surge_roundtrip() {
        let f = Filter::Series(SeriesCond::VolumeSurge {
            days: 20,
            times: 2.0,
        });
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn and_or_not_nested_roundtrip() {
        let f = Filter::And(vec![
            Filter::Or(vec![
                Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
                Filter::Series(SeriesCond::VolumeSurge {
                    days: 5,
                    times: 3.0,
                }),
            ]),
            Filter::Not(Box::new(Filter::Meta(MetaCond::Delisted(true)))),
        ]);
        assert_eq!(roundtrip(&f), f);
    }

    #[test]
    fn cmp_op_serializes_snake_case() {
        let f = Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Const(5.0),
        });
        let json = serde_json::to_string(&f).expect("serialize");
        assert!(
            json.contains("\"gt\""),
            "op must serialize snake_case: {json}"
        );
    }

    #[test]
    fn unknown_variant_json_is_rejected() {
        let src = r#"{"Bogus": []}"#;
        let res: Result<Filter, _> = serde_json::from_str(src);
        assert!(res.is_err(), "unknown tag must be rejected");
    }

    #[test]
    fn missing_field_json_is_rejected() {
        // Struct variant missing the non-Option `factor` field.
        let src = r#"{"Series": {"Cmp": {"op": "gt", "value": {"Const": 5.0}}}}"#;
        let res: Result<Filter, _> = serde_json::from_str(src);
        assert!(res.is_err(), "missing field must be rejected");
    }

    #[test]
    fn missing_option_field_defaults_to_none() {
        // Serde contract: missing Option-typed fields deserialize as None,
        // not an error.
        let src = r#"{"Meta": {"MarketCap": {"min": 100.0}}}"#;
        let f: Filter = serde_json::from_str(src).expect("missing Option field is accepted");
        assert_eq!(
            f,
            Filter::Meta(MetaCond::MarketCap {
                min: Some(100.0),
                max: None,
            })
        );
    }

    #[test]
    fn wrong_type_json_is_rejected() {
        // MarketCap.min expects a number, not a string.
        let src = r#"{"Meta": {"MarketCap": {"min": "not-a-number"}}}"#;
        let res: Result<Filter, _> = serde_json::from_str(src);
        assert!(res.is_err(), "wrong field type must be rejected");
    }

    #[test]
    fn meta_cond_default_is_empty_industry() {
        assert_eq!(MetaCond::default(), MetaCond::Industry(vec![]));
    }

    #[test]
    fn series_factor_default_is_close() {
        assert_eq!(SeriesFactor::default(), SeriesFactor::Close);
    }

    // --- From<ScreenerQuery> mapping (Todo 3, ref #244) ----------------------

    #[test]
    fn from_industries_maps_to_meta_industry() {
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string(), "银行".to_string()],
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Meta(MetaCond::Industry(vec![
                "白酒".to_string(),
                "银行".to_string()
            ]))
        );
    }

    #[test]
    fn from_empty_industries_emits_no_industry_node() {
        let q = bare_query();
        assert_eq!(Filter::from(q), Filter::And(vec![]));
    }

    #[test]
    fn from_exchanges_maps_to_meta_exchange() {
        let q = ScreenerQuery {
            exchanges: vec!["SH".to_string(), "SZ".to_string()],
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Meta(MetaCond::Exchange(vec!["SH".to_string(), "SZ".to_string()]))
        );
    }

    #[test]
    fn from_boards_maps_to_meta_board() {
        let q = ScreenerQuery {
            boards: vec!["主板".to_string()],
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Meta(MetaCond::Board(vec!["主板".to_string()]))
        );
    }

    #[test]
    fn from_list_years_maps_to_meta_list_years() {
        let q = ScreenerQuery {
            list_years: Some(3),
            ..bare_query()
        };
        assert_eq!(Filter::from(q), Filter::Meta(MetaCond::ListYears(3)));
    }

    #[test]
    fn from_market_cap_both_bounds_pass_through() {
        let q = ScreenerQuery {
            market_cap_min: Some(100.0),
            market_cap_max: Some(5000.0),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Meta(MetaCond::MarketCap {
                min: Some(100.0),
                max: Some(5000.0),
            })
        );
    }

    #[test]
    fn from_market_cap_min_only_keeps_max_none() {
        let q = ScreenerQuery {
            market_cap_min: Some(100.0),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Meta(MetaCond::MarketCap {
                min: Some(100.0),
                max: None,
            })
        );
    }

    #[test]
    fn from_default_query_is_bare_delisted_false() {
        // Default query (exclude_delisted = true, nothing else) compiles to a
        // single bare Delisted(false) node, not an And wrapper.
        let q = ScreenerQuery::default();
        assert_eq!(Filter::from(q), Filter::Meta(MetaCond::Delisted(false)));
    }

    #[test]
    fn from_exclude_delisted_false_emits_no_delisted_node() {
        let q = ScreenerQuery {
            exclude_delisted: false,
            ..ScreenerQuery::default()
        };
        assert_eq!(Filter::from(q), Filter::And(vec![]));
    }

    #[test]
    fn from_ma_above_ma20_maps_to_close_gt_sma20() {
        let q = ScreenerQuery {
            ma: Some(MaCondition::AboveMa20),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(20)),
            })
        );
    }

    #[test]
    fn from_ma_above_ma60_maps_to_close_gt_sma60() {
        let q = ScreenerQuery {
            ma: Some(MaCondition::AboveMa60),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::Sma(60)),
            })
        );
    }

    #[test]
    fn from_ma_bullish_align_maps_to_sma5_gt_sma20_and_sma20_gt_sma60() {
        // Engine semantics (compass-strategy lib.rs:234-238): ma5 > ma20 &&
        // ma20 > ma60 — the Close>Sma20 shape from the original handoff table
        // was revised (C2).
        let q = ScreenerQuery {
            ma: Some(MaCondition::BullishAlign),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
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
    fn from_breakout_maps_to_close_gt_n_day_high() {
        let q = ScreenerQuery {
            breakout: Some(BreakoutCondition::new(120)),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Series(SeriesCond::Cmp {
                factor: SeriesFactor::Close,
                op: CmpOp::Gt,
                value: FactorRef::Factor(SeriesFactor::NDayHigh(120)),
            })
        );
    }

    #[test]
    fn from_momentum_maps_to_nested_and_with_both_bounds() {
        let q = ScreenerQuery {
            momentum: Some(MomentumCondition::new(30, -5.0, 50.0)),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
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
    fn from_volume_maps_to_volume_surge() {
        let q = ScreenerQuery {
            volume: Some(VolumeCondition::new(10, 1.5)),
            ..bare_query()
        };
        assert_eq!(
            Filter::from(q),
            Filter::Series(SeriesCond::VolumeSurge {
                days: 10,
                times: 1.5,
            })
        );
    }

    #[test]
    fn from_combined_query_keeps_field_order_and_nests_and_nodes() {
        // industries + exclude_delisted + BullishAlign + momentum: emitted in
        // ScreenerQuery field order; BullishAlign/momentum keep their nested
        // And shapes inside the top-level And.
        let q = ScreenerQuery {
            industries: vec!["白酒".to_string()],
            ma: Some(MaCondition::BullishAlign),
            momentum: Some(MomentumCondition::new(20, 0.0, 100.0)),
            ..ScreenerQuery::default()
        };
        assert_eq!(
            Filter::from(q),
            Filter::And(vec![
                Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
                Filter::Meta(MetaCond::Delisted(false)),
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
                ]),
                Filter::And(vec![
                    Filter::Series(SeriesCond::Cmp {
                        factor: SeriesFactor::ChangePct(20),
                        op: CmpOp::Ge,
                        value: FactorRef::Const(0.0),
                    }),
                    Filter::Series(SeriesCond::Cmp {
                        factor: SeriesFactor::ChangePct(20),
                        op: CmpOp::Le,
                        value: FactorRef::Const(100.0),
                    }),
                ]),
            ])
        );
    }

    fn leaf_industry(name: &str) -> Filter {
        Filter::Meta(MetaCond::Industry(vec![name.to_string()]))
    }

    #[test]
    fn bitand_builds_and() {
        let a = leaf_industry("a");
        let b = leaf_industry("b");
        assert_eq!(a.clone() & b.clone(), Filter::And(vec![a, b]));
    }

    #[test]
    fn bitor_builds_or_chain() {
        let a = leaf_industry("a");
        let b = leaf_industry("b");
        let c = leaf_industry("c");
        assert_eq!(a.clone() | b.clone() | c.clone(), Filter::Or(vec![a, b, c]));
    }

    #[test]
    fn bitnot_builds_not() {
        let a = leaf_industry("a");
        assert_eq!(!a.clone(), Filter::Not(Box::new(a)));
    }

    #[test]
    fn and_method_equals_bitand() {
        let a = leaf_industry("a");
        let b = leaf_industry("b");
        assert_eq!(a.clone().and(b.clone()), a & b);
    }

    #[test]
    fn or_method_equals_bitor() {
        let a = leaf_industry("a");
        let b = leaf_industry("b");
        assert_eq!(a.clone().or(b.clone()), a | b);
    }

    #[test]
    fn negate_method_equals_bitnot() {
        let a = leaf_industry("a");
        assert_eq!(a.clone().negate(), !a);
    }

    #[test]
    fn method_chain_negate_nests() {
        let a = leaf_industry("a");
        let b = leaf_industry("b");
        assert_eq!(
            a.clone().and(b.clone()).negate(),
            Filter::Not(Box::new(Filter::And(vec![a, b])))
        );
    }

    #[test]
    fn compound_and_or_not_matches_exact_ast() {
        let a = leaf_industry("a");
        let b = leaf_industry("b");
        let c = leaf_industry("c");
        let expr = (a.clone() | b.clone()) & !c.clone();
        let expected = Filter::And(vec![Filter::Or(vec![a, b]), Filter::Not(Box::new(c))]);
        assert_eq!(expr, expected);
    }
}
