//! Adversarial tests — epic #243 sub-issue #247 (embedded LLM screener,
//! Batch 4): serde-layer deep-nesting guard for `Filter`.
//!
//! ## Why this file exists before the Batch 4 implementation
//!
//! The plan commits two layers of defense against pathological LLM output:
//! 1. serde deserialization (`serde_json::from_str::<Filter>`) must not
//!    recurse to stack overflow on a deep JSON tree — the *serde layer*;
//! 2. the not-yet-implemented `validate_filter` caps nesting at depth 32 —
//!    the *validation layer*, which runs **after** deserialization.
//!
//! Layer 1 is testable *today*: `Filter` and its serde derive already exist.
//! These tests lock the serde-layer guard (serde_json's default recursion
//! limit of 128). They are expected to PASS on the current code — they are
//! regression locks, not RED probes. The RED probes for the not-yet-
//! implemented `validate_filter` / `LlmClient` / `parse_filter_response`
//! interfaces cannot compile until those interfaces land (tracked in the
//! DEFERRED report).

use compass_types::Filter;

/// Build `depth` nested `{"Not": ...}` wrappers around a leaf, in O(n).
fn not_chain(depth: usize) -> String {
    let leaf = r#"{"Meta": {"Delisted": true}}"#;
    let prefix = r#"{"Not": "#.repeat(depth);
    let suffix = "}".repeat(depth);
    format!("{prefix}{leaf}{suffix}")
}

/// Build `depth` alternating `{"And":[{"Or":[...]}]}` wrappers around a leaf,
/// in O(n).
fn alternating_and_or(depth: usize) -> String {
    let s = String::from(r#"{"Meta": {"Delisted": true}}"#);
    let mut prefix = String::new();
    for i in 0..depth {
        if i % 2 == 0 {
            prefix.push_str(r#"{"And": [{"Or": ["#);
        } else {
            prefix.push_str(r#"{"And": ["#);
        }
    }
    let mut suffix = String::new();
    for i in 0..depth {
        if i % 2 == 0 {
            suffix.push_str("]}]}");
        } else {
            suffix.push(']');
        }
    }
    format!("{prefix}{s}{suffix}")
}

// === A. resource exhaustion / stack-overflow: deep nesting ===

/// 10_000 nesting levels is far beyond serde_json's recursion limit: `from_str`
/// must return Err ("recursion limit exceeded"), not crash the test process
/// with a stack overflow.
#[test]
fn serde_rejects_10k_deep_not_chain_without_overflow() {
    let src = not_chain(10_000);
    let res: Result<Filter, _> = serde_json::from_str(&src);
    assert!(
        res.is_err(),
        "10k-deep nesting must be rejected at the serde layer: {res:?}"
    );
}

/// Same guard at a smaller but still over-limit depth.
#[test]
fn serde_rejects_1k_deep_not_chain() {
    let src = not_chain(1_000);
    let res: Result<Filter, _> = serde_json::from_str(&src);
    assert!(res.is_err(), "1k-deep nesting must be rejected: {res:?}");
}

/// Alternating And/Or nesting must hit the same recursion guard — a
/// different tree shape must not slip past the limit.
#[test]
fn serde_rejects_10k_deep_alternating_and_or() {
    let src = alternating_and_or(10_000);
    let res: Result<Filter, _> = serde_json::from_str(&src);
    assert!(
        res.is_err(),
        "10k-deep alternating And/Or must be rejected at the serde layer: {res:?}"
    );
}

/// Depth 32 is the planned `validate_filter` cap for *legal* input. The serde
/// layer (limit 128) must not reject it early — otherwise legal depth-32
/// filters could never reach validation.
#[test]
fn serde_accepts_32_level_nesting() {
    let src = not_chain(32);
    let res: Result<Filter, _> = serde_json::from_str(&src);
    assert!(res.is_ok(), "depth-32 nesting must deserialize: {res:?}");
}

// === B. resource / performance: wide (non-recursive) trees ===

/// A 10_000-member `And` array is iterated, not recursed, by serde — it must
/// deserialize without panic and preserve the member count. Also a crude
/// performance guard: an accidentally recursive array walk at this size would
/// be visibly slow.
#[test]
fn serde_accepts_10k_member_and_vec() {
    let members: Vec<String> = (0..10_000)
        .map(|i| {
            format!(
                r#"{{"Series": {{"VolumeSurge": {{"days": {}, "times": 1.5}}}}}}"#,
                i % 80 + 1
            )
        })
        .collect();
    let src = format!(r#"{{"And": [{}]}}"#, members.join(","));
    let f: Filter = serde_json::from_str(&src).expect("wide And must deserialize");
    match f {
        Filter::And(v) => assert_eq!(v.len(), 10_000, "member count must be preserved"),
        other => panic!("expected And, got {other:?}"),
    }
}

// === C. empty combinators (builder empty-state, plan contract) ===

/// `And([])` / `Or([])` are the builder's empty state — serde must accept
/// them before `validate_filter` (which the plan declares legal) can ever
/// see them.
#[test]
fn serde_accepts_empty_and_or() {
    let and: Filter = serde_json::from_str(r#"{"And": []}"#).expect("empty And deserializes");
    assert_eq!(and, Filter::And(vec![]));
    let or: Filter = serde_json::from_str(r#"{"Or": []}"#).expect("empty Or deserializes");
    assert_eq!(or, Filter::Or(vec![]));
}

// === D. invalid shapes that must be rejected before validation ===

/// A JSON array is not a tagged Filter value — serde must reject it (an LLM
/// hallucinating a bare array must not reach the validation layer).
#[test]
fn serde_rejects_json_array_as_filter() {
    let res: Result<Filter, _> = serde_json::from_str(r#"[]"#);
    assert!(res.is_err(), "bare JSON array must be rejected: {res:?}");
}

/// JSON scalars are not tagged Filter values either.
#[test]
fn serde_rejects_scalar_json_as_filter() {
    assert!(serde_json::from_str::<Filter>(r#"42"#).is_err());
    assert!(serde_json::from_str::<Filter>(r#""x""#).is_err());
    assert!(serde_json::from_str::<Filter>(r#"true"#).is_err());
}

/// Truncated JSON (LLM response cut mid-stream) must be rejected, not panic.
#[test]
fn serde_rejects_truncated_json() {
    let src = r#"{"Series": {"Cmp": {"factor": "Close", "op": "gt", "value": {"Const": 5"#;
    let res: Result<Filter, _> = serde_json::from_str(src);
    assert!(res.is_err(), "truncated JSON must be rejected: {res:?}");
}

/// f64 extremes that DO deserialize today (NaN is rejected by serde_json for
/// `f64` fields by default, but large finite values are accepted) — the
/// finite-ness rule is `validate_filter`'s job, not serde's. This locks the
/// boundary so the validation-layer RED tests (DEFERRED) can build on it.
#[test]
fn serde_accepts_extreme_finite_const() {
    let src = r#"{"Series": {"Cmp": {"factor": "Close", "op": "gt", "value": {"Const": 1e300}}}}"#;
    let f: Filter = serde_json::from_str(src).expect("large finite Const deserializes");
    match f {
        Filter::Series(_) => {}
        other => panic!("expected Series, got {other:?}"),
    }
}
