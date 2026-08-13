//! Adversarial tests — epic #243 sub-issue #245 (visual condition builder
//! view-model pure functions).
//!
//! Targets `crates/compass/src/citizens/screener_builder.rs`:
//! `filter_to_items` (reverse: Filter AST → card items), `leaf_to_filter` /
//! `group_to_filter` (forward: cards → Filter AST).
//!
//! ## Why this file mounts the source with `#[path]`
//!
//! The compass crate is a **pure-bin crate** (Cargo.toml has only `[[bin]]
//! main.rs`, no `[lib]`), so the view-model module is normally only testable
//! from an in-source `#[cfg(test)]` module — a path this sandbox denies
//! writes to (only `**/tests/**` is writable). The view-model module is
//! pure logic (no egui / compass-ui dependency), so mounting the *current*
//! source into this integration test crate is safe: the tests compile and
//! run against whatever the source contains today (placeholder → RED) and
//! turn GREEN once Todo 1 implements the functions.
//!
//! Scope: view-model pure functions only. No UI rendering (Todo 3), no
//! engine (Batch 3), no kittest interactions (Todo 5).

#[path = "../src/citizens/screener_builder.rs"]
mod screener_builder;

use compass_types::{
    BreakoutCondition, CmpOp, FactorRef, Filter, MaCondition, MetaCond, MomentumCondition,
    ScreenerQuery, SeriesCond, SeriesFactor, VolumeCondition,
};
use screener_builder::{
    BoolOp, CondGroup, CondItem, CondLeaf, LeafKind, LeafParams, MaKind, filter_to_items,
    group_to_filter, leaf_to_filter,
};

// ------------------------------------------------------------------
// Helpers (self-contained — the mounted module's own `mod tests` is
// private and cannot be reused from here).
// ------------------------------------------------------------------

/// `Series(Cmp{factor, op, value})` shorthand.
fn cmp(factor: SeriesFactor, op: CmpOp, value: FactorRef) -> Filter {
    Filter::Series(SeriesCond::Cmp { factor, op, value })
}

/// `Series(VolumeSurge{days, times})` shorthand.
fn vol(days: u32, times: f64) -> Filter {
    Filter::Series(SeriesCond::VolumeSurge { days, times })
}

/// One leaf card item.
fn leaf(kind: LeafKind, params: LeafParams, negated: bool) -> CondItem {
    CondItem::Leaf(CondLeaf {
        kind,
        params,
        negated,
    })
}

/// One leaf card (not wrapped in an item).
fn card(kind: LeafKind, params: LeafParams, negated: bool) -> CondLeaf {
    CondLeaf {
        kind,
        params,
        negated,
    }
}

/// Unwrap the single expected leaf card (fails loudly if the count differs).
fn sole_leaf(items: &[CondItem]) -> &CondLeaf {
    assert_eq!(items.len(), 1, "expected exactly one card, got: {items:?}");
    match &items[0] {
        CondItem::Leaf(l) => l,
        CondItem::Group(g) => panic!("expected a leaf card, got a group: {g:?}"),
    }
}

/// Collect every leaf card in document order (recurses through groups).
fn all_leaves(items: &[CondItem]) -> Vec<&CondLeaf> {
    let mut out = Vec::new();
    for item in items {
        match item {
            CondItem::Leaf(l) => out.push(l),
            CondItem::Group(g) => out.extend(all_leaves(&g.items)),
        }
    }
    out
}

/// Rebuild a `Filter` from card items by wrapping them in an implicit root
/// `And` group (the round-trip reconstruction path).
fn rebuild(items: &[CondItem]) -> Filter {
    group_to_filter(&CondGroup {
        operator: BoolOp::And,
        items: items.to_vec(),
    })
}

// === A. filter_to_items: unknown/illegal AST shapes → Unknown card ===

/// Cmp{Close,Gt,Const(5.0)} is no template (Ma/Breakout need a Factor RHS) —
/// must become an Unknown card, never dropped, never panic.
#[test]
fn adversarial_245_unknown_cmp_const_shape_yields_unknown_card() {
    let f = cmp(SeriesFactor::Close, CmpOp::Gt, FactorRef::Const(5.0));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

/// NaN constant must not poison recognition (engine contract: NaN→no panic).
#[test]
fn adversarial_245_unknown_cmp_with_nan_const_yields_unknown_no_panic() {
    let f = cmp(SeriesFactor::Close, CmpOp::Gt, FactorRef::Const(f64::NAN));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

/// Sma(5) as the *left* operand is not the Ma template (Close > Sma(n)).
#[test]
fn adversarial_245_unknown_orphan_sma_left_operand_yields_unknown_card() {
    let f = cmp(SeriesFactor::Sma(5), CmpOp::Gt, FactorRef::Const(1.0));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

/// Count is deliberately absent from LeafKind (deferred) — must degrade.
#[test]
fn adversarial_245_unknown_count_shape_yields_unknown_card() {
    let f = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::Close,
        op: CmpOp::Gt,
        value: FactorRef::Const(5.0),
        window: 5,
        at_least: 3,
    });
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

/// Close Le Sma(20) is NOT the Ma template (Gt is the declared op) — factor
/// and operand match alone must not be enough to claim a template.
#[test]
fn adversarial_245_unknown_wrong_op_for_ma_shape_yields_unknown_card() {
    let f = cmp(
        SeriesFactor::Close,
        CmpOp::Le,
        FactorRef::Factor(SeriesFactor::Sma(20)),
    );
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

/// DayPct appears in no template — a shape using it must be Unknown.
#[test]
fn adversarial_245_unknown_daypct_factor_yields_unknown_card() {
    let f = cmp(SeriesFactor::DayPct, CmpOp::Gt, FactorRef::Const(5.0));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

/// Not(x) where x itself is unrecognized → Unknown fallback (no panic, no
/// infinite recursion through the Box).
#[test]
fn adversarial_245_unknown_not_wrapping_non_template_yields_unknown_card() {
    let f = Filter::Not(Box::new(cmp(
        SeriesFactor::Close,
        CmpOp::Gt,
        FactorRef::Const(5.0),
    )));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Unknown);
}

// === B. Momentum pair recognition boundaries ===

/// Positive control (direct AST): Ge+Le with the same n must collapse into
/// ONE Momentum card with exact params.
#[test]
fn adversarial_245_momentum_pair_recognized_as_single_card() {
    let f = Filter::And(vec![
        cmp(SeriesFactor::ChangePct(5), CmpOp::Ge, FactorRef::Const(1.0)),
        cmp(
            SeriesFactor::ChangePct(5),
            CmpOp::Le,
            FactorRef::Const(10.0),
        ),
    ]);
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Momentum);
    assert!(matches!(
        leaf.params,
        LeafParams::Momentum {
            days: 5,
            min_pct: 1.0,
            max_pct: 10.0
        }
    ));
}

/// Same shape, different n (5 vs 10) — must NOT collapse into one momentum
/// card; each orphan Cmp becomes an Unknown card.
#[test]
fn adversarial_245_momentum_different_n_does_not_combine() {
    let f = Filter::And(vec![
        cmp(SeriesFactor::ChangePct(5), CmpOp::Ge, FactorRef::Const(1.0)),
        cmp(
            SeriesFactor::ChangePct(10),
            CmpOp::Le,
            FactorRef::Const(10.0),
        ),
    ]);
    let items = filter_to_items(&f);
    let kinds: Vec<LeafKind> = all_leaves(&items).iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![LeafKind::Unknown, LeafKind::Unknown],
        "different-n Cmp pair must not combine into a momentum card"
    );
}

/// Same n but Le before Ge — the template is Ge then Le; a reversed pair
/// must not combine.
#[test]
fn adversarial_245_momentum_reverse_op_order_does_not_combine() {
    let f = Filter::And(vec![
        cmp(
            SeriesFactor::ChangePct(5),
            CmpOp::Le,
            FactorRef::Const(10.0),
        ),
        cmp(SeriesFactor::ChangePct(5), CmpOp::Ge, FactorRef::Const(1.0)),
    ]);
    let items = filter_to_items(&f);
    let kinds: Vec<LeafKind> = all_leaves(&items).iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![LeafKind::Unknown, LeafKind::Unknown],
        "reversed Le→Ge order must not combine into a momentum card"
    );
}

// === C. single-member fold variants ===

/// And(vec![x]) must render the SAME shape as bare x — no extra subgroup.
#[test]
fn adversarial_245_single_member_and_folds_to_bare_shape() {
    let x = vol(5, 2.0);
    let folded = filter_to_items(&Filter::And(vec![x.clone()]));
    let bare = filter_to_items(&x);
    assert_eq!(folded.len(), 1, "single-member And must not vanish");
    assert_eq!(folded, bare, "And([x]) must render identically to bare x");
    assert!(
        matches!(&folded[0], CondItem::Leaf(_)),
        "single-member And must not produce an extra subgroup"
    );
}

/// The Or single-member fold is the same declared rule.
#[test]
fn adversarial_245_single_member_or_folds_to_bare_shape() {
    let x = vol(5, 2.0);
    let folded = filter_to_items(&Filter::Or(vec![x.clone()]));
    let bare = filter_to_items(&x);
    assert_eq!(folded.len(), 1, "single-member Or must not vanish");
    assert_eq!(folded, bare, "Or([x]) must render identically to bare x");
    assert!(matches!(&folded[0], CondItem::Leaf(_)));
}

/// And([And([x])]) — nested single-member wrappers must fold fully.
#[test]
fn adversarial_245_double_wrapped_single_member_folds_to_bare_shape() {
    let x = vol(5, 2.0);
    let double = filter_to_items(&Filter::And(vec![Filter::And(vec![x.clone()])]));
    let bare = filter_to_items(&x);
    assert_eq!(double.len(), 1, "nested single-member And must not vanish");
    assert_eq!(
        double, bare,
        "And([And([x])]) must render identically to bare x"
    );
}

// === D. forward mapping edges (leaf_to_filter / group_to_filter) ===

/// `negated: true` must wrap the *built* template shape in Not.
#[test]
fn adversarial_245_leaf_to_filter_negated_wraps_in_not() {
    let l = card(LeafKind::Ma, LeafParams::Ma(MaKind::AboveMa20), true);
    assert_eq!(
        leaf_to_filter(&l),
        Filter::Not(Box::new(cmp(
            SeriesFactor::Close,
            CmpOp::Gt,
            FactorRef::Factor(SeriesFactor::Sma(20))
        )))
    );
}

/// Unknown cards carry only a JSON summary — forward rebuild must be a
/// no-constraint `And(vec![])`, never a panic.
#[test]
fn adversarial_245_leaf_to_filter_unknown_returns_empty_and() {
    let l = card(
        LeafKind::Unknown,
        LeafParams::Unknown("{\"x\":1}".to_string()),
        false,
    );
    assert_eq!(leaf_to_filter(&l), Filter::And(vec![]));
}

#[test]
fn adversarial_245_group_to_filter_empty_group_returns_empty_and() {
    let g = CondGroup {
        operator: BoolOp::And,
        items: vec![],
    };
    assert_eq!(group_to_filter(&g), Filter::And(vec![]));
}

/// Aligned with `From<ScreenerQuery>`'s `1 => nodes.pop()`: a single member
/// group must emit the bare node, NOT `And([x])`.
#[test]
fn adversarial_245_group_to_filter_single_member_and_emits_bare_node() {
    let g = CondGroup {
        operator: BoolOp::And,
        items: vec![leaf(
            LeafKind::VolumeSurge,
            LeafParams::VolumeSurge {
                days: 5,
                times: 2.0,
            },
            false,
        )],
    };
    assert_eq!(group_to_filter(&g), vol(5, 2.0));
}

/// Same folding for a single-member Or group (plan: "Or 同理").
#[test]
fn adversarial_245_group_to_filter_single_member_or_emits_bare_node() {
    let g = CondGroup {
        operator: BoolOp::Or,
        items: vec![leaf(
            LeafKind::VolumeSurge,
            LeafParams::VolumeSurge {
                days: 5,
                times: 2.0,
            },
            false,
        )],
    };
    assert_eq!(group_to_filter(&g), vol(5, 2.0));
}

#[test]
fn adversarial_245_group_to_filter_multi_member_preserves_order() {
    let g = CondGroup {
        operator: BoolOp::And,
        items: vec![
            leaf(
                LeafKind::Industry,
                LeafParams::MultiSelect(vec!["白酒".to_string()]),
                false,
            ),
            leaf(
                LeafKind::VolumeSurge,
                LeafParams::VolumeSurge {
                    days: 5,
                    times: 2.0,
                },
                false,
            ),
            leaf(LeafKind::Delisted, LeafParams::Delisted(false), false),
        ],
    };
    assert_eq!(
        group_to_filter(&g),
        Filter::And(vec![
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
            vol(5, 2.0),
            Filter::Meta(MetaCond::Delisted(false)),
        ])
    );
}

/// A nested Or group must survive with its operator (the And-root default
/// must not swallow it).
#[test]
fn adversarial_245_group_to_filter_nested_group_preserves_operator() {
    let g = CondGroup {
        operator: BoolOp::And,
        items: vec![
            CondItem::Group(CondGroup {
                operator: BoolOp::Or,
                items: vec![
                    leaf(
                        LeafKind::Industry,
                        LeafParams::MultiSelect(vec!["白酒".to_string()]),
                        false,
                    ),
                    leaf(LeafKind::Delisted, LeafParams::Delisted(false), false),
                ],
            }),
            leaf(
                LeafKind::VolumeSurge,
                LeafParams::VolumeSurge {
                    days: 5,
                    times: 2.0,
                },
                false,
            ),
        ],
    };
    assert_eq!(
        group_to_filter(&g),
        Filter::And(vec![
            Filter::Or(vec![
                Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
                Filter::Meta(MetaCond::Delisted(false)),
            ]),
            vol(5, 2.0),
        ])
    );
}

/// Nested single-member groups fold to the member as a parent item
/// (plan: "And(vec![x]) 子组 → 折叠为 x 作为父级成员").
#[test]
fn adversarial_245_group_to_filter_single_member_nested_group_folds() {
    let g = CondGroup {
        operator: BoolOp::And,
        items: vec![CondItem::Group(CondGroup {
            operator: BoolOp::And,
            items: vec![leaf(
                LeafKind::VolumeSurge,
                LeafParams::VolumeSurge {
                    days: 5,
                    times: 2.0,
                },
                false,
            )],
        })],
    };
    assert_eq!(group_to_filter(&g), vol(5, 2.0));
}

// === E. round-trip adversarial shapes ===

/// A realistic 7-node `ScreenerQuery` compiled by `From` (Industry +
/// MarketCap + Delisted + BullishAlign + Breakout + Momentum + Volume) must
/// survive round-trip with structural equality.
#[test]
fn adversarial_245_round_trip_from_multi_condition_query() {
    let query = ScreenerQuery {
        industries: vec!["白酒".to_string(), "银行".to_string()],
        ma: Some(MaCondition::BullishAlign),
        breakout: Some(BreakoutCondition::new(120)),
        momentum: Some(MomentumCondition::new(5, 1.0, 10.0)),
        volume: Some(VolumeCondition::new(5, 2.5)),
        market_cap_min: Some(100.0),
        ..ScreenerQuery::default()
    };
    let f = Filter::from(query);
    let rebuilt = rebuild(&filter_to_items(&f));
    assert_eq!(rebuilt, f, "multi-condition round-trip must be structural");
}

/// Bare single Ma20 node: leaf-level equivalence (no group wrapping).
#[test]
fn adversarial_245_round_trip_bare_ma20_leaf_level() {
    let f = cmp(
        SeriesFactor::Close,
        CmpOp::Gt,
        FactorRef::Factor(SeriesFactor::Sma(20)),
    );
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf_to_filter(leaf), f);
}

#[test]
fn adversarial_245_round_trip_not_wrapped_delisted() {
    let f = Filter::Not(Box::new(Filter::Meta(MetaCond::Delisted(false))));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert!(leaf.negated, "Not must surface as the negated flag");
    assert_eq!(leaf_to_filter(leaf), f);
}

#[test]
fn adversarial_245_round_trip_momentum_pair_leaf_level() {
    let f = Filter::And(vec![
        cmp(SeriesFactor::ChangePct(5), CmpOp::Ge, FactorRef::Const(1.0)),
        cmp(
            SeriesFactor::ChangePct(5),
            CmpOp::Le,
            FactorRef::Const(10.0),
        ),
    ]);
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Momentum);
    assert_eq!(leaf_to_filter(leaf), f);
}

#[test]
fn adversarial_245_round_trip_bullish_align_leaf_level() {
    let f = Filter::And(vec![
        cmp(
            SeriesFactor::Sma(5),
            CmpOp::Gt,
            FactorRef::Factor(SeriesFactor::Sma(20)),
        ),
        cmp(
            SeriesFactor::Sma(20),
            CmpOp::Gt,
            FactorRef::Factor(SeriesFactor::Sma(60)),
        ),
    ]);
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Ma);
    assert_eq!(leaf.params, LeafParams::Ma(MaKind::BullishAlign));
    assert_eq!(leaf_to_filter(leaf), f);
}

/// And[Or[And[Meta, Series], Meta], Not[Meta]] — mixed And/Or/Not, 4 levels
/// deep, must round-trip with structural equality.
#[test]
fn adversarial_245_round_trip_deep_nesting_preserves_structure() {
    let deep = Filter::And(vec![
        Filter::Or(vec![
            Filter::And(vec![
                Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
                vol(5, 2.0),
            ]),
            Filter::Meta(MetaCond::ListYears(3)),
        ]),
        Filter::Not(Box::new(Filter::Meta(MetaCond::Delisted(false)))),
    ]);
    let rebuilt = rebuild(&filter_to_items(&deep));
    assert_eq!(rebuilt, deep, "deep mixed nesting must round-trip");
}

// === F. boundary / invalid input: f64 extremes, None edges, huge inputs ===

/// Engine contract: NaN must not panic. Exact equality is impossible
/// (NaN != NaN), so assert structural shape + NaN preservation.
#[test]
fn adversarial_245_leaf_to_filter_nan_volume_times_no_panic_preserved() {
    let l = card(
        LeafKind::VolumeSurge,
        LeafParams::VolumeSurge {
            days: 5,
            times: f64::NAN,
        },
        false,
    );
    let out = leaf_to_filter(&l);
    match out {
        Filter::Series(SeriesCond::VolumeSurge { days, times }) => {
            assert_eq!(days, 5);
            assert!(times.is_nan(), "NaN param must be preserved, not corrupted");
        }
        other => panic!("expected VolumeSurge series, got {other:?}"),
    }
}

/// NaN in one momentum bound must survive (not panic, not be rewritten).
#[test]
fn adversarial_245_leaf_to_filter_nan_momentum_bounds_no_panic_preserved() {
    let l = card(
        LeafKind::Momentum,
        LeafParams::Momentum {
            days: 5,
            min_pct: f64::NAN,
            max_pct: 10.0,
        },
        false,
    );
    let out = leaf_to_filter(&l);
    let Filter::And(v) = &out else {
        panic!("expected momentum And pair, got {out:?}");
    };
    assert_eq!(v.len(), 2, "momentum pair must keep both bounds");
    let Filter::Series(SeriesCond::Cmp { factor, op, value }) = &v[0] else {
        panic!("first bound must be a Cmp, got {:?}", v[0]);
    };
    assert_eq!(*factor, SeriesFactor::ChangePct(5));
    assert_eq!(*op, CmpOp::Ge);
    match value {
        FactorRef::Const(min) => assert!(min.is_nan(), "NaN min bound must be preserved"),
        other => panic!("expected Const, got {other:?}"),
    }
}

/// +inf is a total-order value: exact equality is valid here.
#[test]
fn adversarial_245_leaf_to_filter_inf_updays_min_pct_preserved() {
    let l = card(
        LeafKind::UpDays,
        LeafParams::UpDays {
            n: 3,
            min_pct: f64::INFINITY,
        },
        false,
    );
    assert_eq!(
        leaf_to_filter(&l),
        Filter::Series(SeriesCond::UpDays {
            n: 3,
            min_pct: f64::INFINITY
        })
    );
}

/// Each None side is a legitimate "unbounded" bound — must map 1:1.
#[test]
fn adversarial_245_leaf_to_filter_market_cap_single_side_none() {
    let l = card(
        LeafKind::MarketCap,
        LeafParams::MarketCap {
            min: None,
            max: Some(500.0),
        },
        false,
    );
    assert_eq!(
        leaf_to_filter(&l),
        Filter::Meta(MetaCond::MarketCap {
            min: None,
            max: Some(500.0)
        })
    );
}

#[test]
fn adversarial_245_leaf_to_filter_market_cap_both_none() {
    let l = card(
        LeafKind::MarketCap,
        LeafParams::MarketCap {
            min: None,
            max: None,
        },
        false,
    );
    assert_eq!(
        leaf_to_filter(&l),
        Filter::Meta(MetaCond::MarketCap {
            min: None,
            max: None
        })
    );
}

/// Empty multi-select still matches the Industry template — the card must
/// exist with empty params and round-trip exactly.
#[test]
fn adversarial_245_filter_to_items_empty_industry_roundtrip() {
    let f = Filter::Meta(MetaCond::Industry(vec![]));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(leaf.kind, LeafKind::Industry);
    assert_eq!(leaf.params, LeafParams::MultiSelect(vec![]));
    assert_eq!(leaf_to_filter(leaf), f);
}

/// Empty string inside the vector must survive recognition verbatim.
#[test]
fn adversarial_245_filter_to_items_empty_string_multi_select_roundtrip() {
    let f = Filter::Meta(MetaCond::Industry(vec!["".to_string(), "白酒".to_string()]));
    let items = filter_to_items(&f);
    let leaf = sole_leaf(&items);
    assert_eq!(
        leaf.params,
        LeafParams::MultiSelect(vec!["".to_string(), "白酒".to_string()])
    );
    assert_eq!(leaf_to_filter(leaf), f);
}

/// 2000 recognisable nodes: no panic, no dropped node, round-trip intact.
/// Also a crude performance guard — an O(n²) recognizer at this size would
/// be visibly slow.
#[test]
fn adversarial_245_huge_and_no_panic_roundtrip() {
    let nodes: Vec<Filter> = (0..2000u32).map(|i| vol(i % 80 + 1, 1.5)).collect();
    let f = Filter::And(nodes);
    let items = filter_to_items(&f);
    assert_eq!(
        all_leaves(&items).len(),
        2000,
        "no node may be dropped for a large And"
    );
    assert_eq!(rebuild(&items), f);
}

/// 5 levels of alternating And/Or built programmatically — the deep
/// recursion path must not stack-overflow or lose structure.
#[test]
fn adversarial_245_deep_alternating_nesting_no_panic_roundtrip() {
    let mut f = Filter::Meta(MetaCond::Delisted(false));
    for i in 0..5u32 {
        let v = vol(i + 1, 2.0);
        f = if i % 2 == 0 {
            Filter::And(vec![f, v])
        } else {
            Filter::Or(vec![f, v])
        };
    }
    let rebuilt = rebuild(&filter_to_items(&f));
    assert_eq!(rebuilt, f, "deep alternating nesting must round-trip");
}

/// "不限" (`ListYears(None)`) is interface-ambiguous in exact output (either
/// `Meta(ListYears(0))` or a no-constraint `And(vec![])` are defensible).
/// Lock only the guaranteed invariants: no panic and a stable rebuild of
/// whatever was emitted.
#[test]
fn adversarial_245_list_years_none_no_panic_stable() {
    let l = card(LeafKind::ListYears, LeafParams::ListYears(None), false);
    let out = leaf_to_filter(&l); // must not panic
    let items = filter_to_items(&out); // must not panic
    assert_eq!(
        rebuild(&items),
        out,
        "ListYears(None) rebuild must be stable"
    );
}
