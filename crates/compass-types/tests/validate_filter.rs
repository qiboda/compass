//! Requirement acceptance tests for `validate_filter` (epic #243 Batch 4,
//! ref #247, plan Todo 1).
//!
//! RED phase: `validate_filter` is not implemented yet — this file fails to
//! compile until Todo 1 lands (missing symbol `compass_types::validate_filter`).
//! Contract (plan): `pub fn validate_filter(&Filter) -> Result<(), String>`;
//! every Err message names the offending field. Rules: ① all window/count
//! params > 0; ② `Count.at_least <= Count.window`; ③ `MarketCap.min <= max`
//! when both Some; ④ all f64 fields finite (no NaN/Inf); ⑤ nesting depth cap
//! 32. Empty `And(vec![])`/`Or(vec![])` are legal (builder empty state).

use compass_types::{
    CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor, validate_filter,
};

/// Composite filter exercising every valid Meta/Series variant with normal
/// parameters — happy-path baseline.
fn valid_composite() -> Filter {
    Filter::And(vec![
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        Filter::Meta(MetaCond::Exchange(vec!["SH".to_string()])),
        Filter::Meta(MetaCond::Board(vec!["主板".to_string()])),
        Filter::Meta(MetaCond::ListYears(3)),
        Filter::Meta(MetaCond::Delisted(false)),
        Filter::Meta(MetaCond::MarketCap {
            min: Some(100.0),
            max: Some(5000.0),
        }),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Sma(20),
            op: CmpOp::Gt,
            value: FactorRef::Const(5.0),
        }),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        }),
        Filter::Series(SeriesCond::UpDays { n: 5, min_pct: 3.0 }),
        Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 10,
            at_least: 5,
        }),
        Filter::Series(SeriesCond::VolumeSurge {
            days: 20,
            times: 2.0,
        }),
    ])
}

/// Chain of `depth` nested `Not` wrappers around a bare leaf.
fn deep_not(depth: u32) -> Filter {
    let mut f = Filter::Meta(MetaCond::Delisted(true));
    for _ in 0..depth {
        f = f.negate();
    }
    f
}

fn cmp_with_factor(factor: SeriesFactor) -> Filter {
    Filter::Series(SeriesCond::Cmp {
        factor,
        op: CmpOp::Gt,
        value: FactorRef::Const(1.0),
    })
}

#[test]
fn validate_filter_accepts_all_variants_with_normal_params() {
    assert_eq!(validate_filter(&valid_composite()), Ok(()));
}

#[test]
fn validate_filter_accepts_minimal_window_of_one() {
    let f = Filter::And(vec![
        cmp_with_factor(SeriesFactor::Sma(1)),
        cmp_with_factor(SeriesFactor::ChangePct(1)),
        cmp_with_factor(SeriesFactor::AvgVolume(1)),
        cmp_with_factor(SeriesFactor::NDayHigh(1)),
        Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 1.0 }),
        Filter::Series(SeriesCond::VolumeSurge {
            days: 1,
            times: 1.0,
        }),
        Filter::Series(SeriesCond::Count {
            factor: SeriesFactor::DayPct,
            op: CmpOp::Gt,
            value: FactorRef::Const(0.0),
            window: 1,
            at_least: 1,
        }),
    ]);
    assert_eq!(validate_filter(&f), Ok(()));
}

#[test]
fn validate_filter_accepts_empty_and_or() {
    assert_eq!(validate_filter(&Filter::And(vec![])), Ok(()));
    assert_eq!(validate_filter(&Filter::Or(vec![])), Ok(()));
}

#[test]
fn validate_filter_accepts_single_sided_market_cap() {
    let f = Filter::Meta(MetaCond::MarketCap {
        min: Some(100.0),
        max: None,
    });
    assert_eq!(validate_filter(&f), Ok(()));
}

#[test]
fn validate_filter_accepts_depth_32_not_chain() {
    assert_eq!(validate_filter(&deep_not(32)), Ok(()));
}

#[test]
fn validate_filter_rejects_depth_33_not_chain() {
    let err = validate_filter(&deep_not(33)).expect_err("depth 33 must be rejected");
    assert!(
        err.contains("nesting too deep"),
        "depth error must be recognizable, got: {err}"
    );
}

#[test]
fn validate_filter_rejects_clearly_over_limit_nesting() {
    let err = validate_filter(&deep_not(64)).expect_err("depth 64 must be rejected");
    assert!(err.contains("nesting too deep"));
}

#[test]
fn validate_filter_rejects_sma_zero_window() {
    let err = validate_filter(&cmp_with_factor(SeriesFactor::Sma(0)))
        .expect_err("Sma(0) must be rejected");
    assert!(err.contains("Sma"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_change_pct_zero_window() {
    let err = validate_filter(&cmp_with_factor(SeriesFactor::ChangePct(0)))
        .expect_err("ChangePct(0) must be rejected");
    assert!(
        err.contains("ChangePct"),
        "message must name the field: {err}"
    );
}

#[test]
fn validate_filter_rejects_avg_volume_zero_window() {
    let err = validate_filter(&cmp_with_factor(SeriesFactor::AvgVolume(0)))
        .expect_err("AvgVolume(0) must be rejected");
    assert!(
        err.contains("AvgVolume"),
        "message must name the field: {err}"
    );
}

#[test]
fn validate_filter_rejects_n_day_high_zero_window() {
    let f = Filter::Series(SeriesCond::Cmp {
        factor: SeriesFactor::Close,
        op: CmpOp::Gt,
        value: FactorRef::Factor(SeriesFactor::NDayHigh(0)),
    });
    let err = validate_filter(&f).expect_err("NDayHigh(0) must be rejected");
    assert!(
        err.contains("NDayHigh"),
        "message must name the field: {err}"
    );
}

#[test]
fn validate_filter_rejects_up_days_zero_n() {
    let f = Filter::Series(SeriesCond::UpDays { n: 0, min_pct: 3.0 });
    let err = validate_filter(&f).expect_err("UpDays n=0 must be rejected");
    assert!(err.contains("UpDays"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_volume_surge_zero_days() {
    let f = Filter::Series(SeriesCond::VolumeSurge {
        days: 0,
        times: 2.0,
    });
    let err = validate_filter(&f).expect_err("VolumeSurge days=0 must be rejected");
    assert!(err.contains("days"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_count_zero_window() {
    let f = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 0,
        at_least: 0,
    });
    let err = validate_filter(&f).expect_err("Count window=0 must be rejected");
    assert!(err.contains("window"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_count_zero_at_least() {
    let f = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 10,
        at_least: 0,
    });
    let err = validate_filter(&f).expect_err("Count at_least=0 must be rejected");
    assert!(
        err.contains("at_least"),
        "message must name the field: {err}"
    );
}

#[test]
fn validate_filter_rejects_count_at_least_exceeding_window() {
    let f = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 5,
        at_least: 6,
    });
    let err = validate_filter(&f).expect_err("at_least > window must be rejected");
    assert!(
        err.contains("at_least"),
        "message must name the field: {err}"
    );
}

#[test]
fn validate_filter_rejects_market_cap_min_above_max() {
    let f = Filter::Meta(MetaCond::MarketCap {
        min: Some(200.0),
        max: Some(100.0),
    });
    let err = validate_filter(&f).expect_err("min > max must be rejected");
    assert!(
        err.contains("min") && err.contains("max"),
        "message must name both bounds: {err}"
    );
}

#[test]
fn validate_filter_rejects_nan_const_value() {
    let f = Filter::Series(SeriesCond::Cmp {
        factor: SeriesFactor::Close,
        op: CmpOp::Gt,
        value: FactorRef::Const(f64::NAN),
    });
    let err = validate_filter(&f).expect_err("NaN const must be rejected");
    assert!(err.contains("Const"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_nan_up_days_min_pct() {
    let f = Filter::Series(SeriesCond::UpDays {
        n: 5,
        min_pct: f64::NAN,
    });
    let err = validate_filter(&f).expect_err("NaN min_pct must be rejected");
    assert!(
        err.contains("min_pct"),
        "message must name the field: {err}"
    );
}

#[test]
fn validate_filter_rejects_nan_volume_surge_times() {
    let f = Filter::Series(SeriesCond::VolumeSurge {
        days: 20,
        times: f64::NAN,
    });
    let err = validate_filter(&f).expect_err("NaN times must be rejected");
    assert!(err.contains("times"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_nan_market_cap_min() {
    let f = Filter::Meta(MetaCond::MarketCap {
        min: Some(f64::NAN),
        max: None,
    });
    let err = validate_filter(&f).expect_err("NaN market-cap min must be rejected");
    assert!(err.contains("min"), "message must name the field: {err}");
}

#[test]
fn validate_filter_rejects_infinite_market_cap_max() {
    let f = Filter::Meta(MetaCond::MarketCap {
        min: None,
        max: Some(f64::INFINITY),
    });
    let err = validate_filter(&f).expect_err("infinite market-cap max must be rejected");
    assert!(err.contains("max"), "message must name the field: {err}");
}
