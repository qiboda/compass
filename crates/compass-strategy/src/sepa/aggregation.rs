//! Local concept-board daily aggregation (epic #139 decision 8/20).
//!
//! Aggregates per-concept-board daily statistics from the raw membership
//! snapshot ([`ConceptMember`]) plus whole-market cross-section bars: the
//! equal-weighted mean day-over-day change of the members, the summed latest
//! amount, the fraction of members that rose, and the member count. This is
//! the local replacement for EastMoney board indexes — no online calls, no
//! board-quote interface.
//!
//! Symbol keys are normalized to the bare 6-digit code (exchange prefix
//! stripped via [`parse_explicit_prefix`]); membership rows carry prefixed
//! symbols (`SH600519`) while cross-section bars carry bare codes
//! (`600519`), so both sides go through the same normalization.

use std::collections::HashMap;

use compass_core::data::symbol::parse_explicit_prefix;
use compass_core::model::{ConceptMember, CrossSectionBar};

/// One concept board's daily aggregate, consumed by the theme scoring module
/// (todo 10). Internal structure — not part of the GUI contract.
#[derive(Debug, Clone, PartialEq)]
pub struct ConceptDaily {
    /// Equal-weighted mean day-over-day change of the members (%).
    pub pct_change: f64,
    /// Sum of the members' latest-bar trading amounts (yuan).
    pub amount: f64,
    /// Fraction of aggregated members that rose (`pct > 0`), in 0..1.
    pub up_ratio: f64,
    /// Number of members with computable data (after skipping).
    pub member_count: usize,
}

/// Aggregate concept boards from membership rows and per-symbol bar series.
///
/// `bars_by_symbol` must be keyed by **bare** 6-digit codes with series in
/// ascending `trade_date` order (as returned by
/// [`ParquetReader::fetch_cross_section`](compass_core::data::parquet::ParquetReader::fetch_cross_section)).
///
/// Per member the day-over-day change is
/// `(close_t - close_{t-1}) / close_{t-1} * 100` from the latest two bars.
/// A member is skipped when it has fewer than two bars (missing latest bar),
/// when the relevant closes are non-finite, or when the previous close is
/// zero. A concept whose members are all skipped has no entry in the result
/// map. `up_ratio` and the equal-weight mean are computed over the surviving
/// members only.
pub fn aggregate_concept_daily(
    members: &[ConceptMember],
    bars_by_symbol: &HashMap<String, Vec<&CrossSectionBar>>,
) -> HashMap<String, ConceptDaily> {
    // Latest two bars per symbol (normalized bare code).
    let mut pct_by_symbol: HashMap<&str, f64> = HashMap::new();
    let mut amount_by_symbol: HashMap<&str, f64> = HashMap::new();
    for (bare, series) in bars_by_symbol {
        if series.len() < 2 {
            continue;
        }
        let latest = series[series.len() - 1];
        let prev = series[series.len() - 2];
        if !latest.close.is_finite() || !prev.close.is_finite() || prev.close == 0.0 {
            continue;
        }
        pct_by_symbol.insert(
            bare.as_str(),
            (latest.close - prev.close) / prev.close * 100.0,
        );
        amount_by_symbol.insert(
            bare.as_str(),
            if latest.amount.is_finite() {
                latest.amount
            } else {
                0.0
            },
        );
    }

    let mut out: HashMap<String, ConceptDaily> = HashMap::new();
    for member in members {
        let bare = parse_explicit_prefix(&member.symbol).1;
        let Some(&pct) = pct_by_symbol.get(bare) else {
            continue;
        };
        let amount = amount_by_symbol.get(bare).copied().unwrap_or(0.0);
        let daily = out
            .entry(member.concept_code.clone())
            .or_insert(ConceptDaily {
                pct_change: 0.0,
                amount: 0.0,
                up_ratio: 0.0,
                member_count: 0,
            });
        daily.pct_change += pct;
        daily.amount += amount;
        if pct > 0.0 {
            daily.up_ratio += 1.0;
        }
        daily.member_count += 1;
    }
    for daily in out.values_mut() {
        let n = daily.member_count as f64;
        daily.pct_change /= n;
        daily.up_ratio /= n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    /// Two-bar series: `[base, base * (1 + pct/100)]`, latest bar carries
    /// `amount`. `symbol` is the bare code used as the map key.
    fn two_bar_series(symbol: &str, base: f64, pct: f64, amount: f64) -> Vec<CrossSectionBar> {
        let end = NaiveDate::from_ymd_opt(2026, 7, 31).expect("valid date");
        let closes = [base, base * (1.0 + pct / 100.0)];
        closes
            .iter()
            .enumerate()
            .map(|(i, close)| CrossSectionBar {
                symbol: symbol.to_string(),
                trade_date: end - Duration::days(1 - i as i64),
                open: *close,
                high: *close,
                low: *close,
                adjclose: *close,
                close: *close,
                volume: 0.0,
                amount: if i == 1 { amount } else { 0.0 },
            })
            .collect()
    }

    /// Member row; `symbol` may be prefixed (SH/SZ/BJ) or bare.
    fn member(concept: &str, symbol: &str) -> ConceptMember {
        ConceptMember {
            concept_code: concept.to_string(),
            symbol: symbol.to_string(),
            concept_name: None,
            update_date: None,
        }
    }

    /// Group per-symbol series into a ref map. Test fixtures are tiny and
    /// the map borrows its backing data, so the helper leaks the owned vecs
    /// (reclaimed at process exit) to sidestep a self-referential return.
    fn group(
        series: Vec<(&str, Vec<CrossSectionBar>)>,
    ) -> HashMap<String, Vec<&'static CrossSectionBar>> {
        series
            .into_iter()
            .map(|(s, bars)| {
                let bars: &'static Vec<CrossSectionBar> = Box::leak(Box::new(bars));
                (s.to_string(), bars.iter().collect())
            })
            .collect()
    }

    #[test]
    fn equal_weighted_mean_of_member_changes() {
        // Two members +3% and +5% → 4.0%.
        let bars = group(vec![
            ("600000", two_bar_series("600000", 100.0, 3.0, 1.0e9)),
            ("000001", two_bar_series("000001", 100.0, 5.0, 2.0e9)),
        ]);
        let members = vec![member("BK1000", "600000"), member("BK1000", "000001")];
        let out = aggregate_concept_daily(&members, &bars);
        let daily = out.get("BK1000").expect("concept present");
        assert!(
            (daily.pct_change - 4.0).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
        assert!((daily.amount - 3.0e9).abs() < 1e-9, "got {}", daily.amount);
        assert_eq!(daily.up_ratio, 1.0);
        assert_eq!(daily.member_count, 2);
    }

    #[test]
    fn prefixed_bare_and_dolt_native_symbols_normalize_to_bare_code() {
        // Mixed membership formats: prefixed (SH/SZ, with and without dot)
        // and bare codes all resolve against bare-code bar keys.
        let bars = group(vec![
            ("600519", two_bar_series("600519", 100.0, 10.0, 1.0e8)),
            ("000001", two_bar_series("000001", 100.0, 3.0, 2.0e8)),
            ("000002", two_bar_series("000002", 100.0, -2.0, 3.0e8)),
        ]);
        let members = vec![
            member("BK1001", "SH600519"),
            member("BK1001", "000001"),
            member("BK1001", "SZ.000002"),
        ];
        let out = aggregate_concept_daily(&members, &bars);
        let daily = out.get("BK1001").expect("concept present");
        // (10 + 3 - 2) / 3.
        assert!(
            (daily.pct_change - 11.0 / 3.0).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
        assert!(
            (daily.up_ratio - 2.0 / 3.0).abs() < 1e-9,
            "got {}",
            daily.up_ratio
        );
        assert_eq!(daily.member_count, 3);
        assert!((daily.amount - 6.0e8).abs() < 1e-9, "got {}", daily.amount);
    }

    #[test]
    fn members_without_latest_bar_or_prev_are_skipped() {
        // Member "600888" has no bars at all; "600777" has a single bar
        // (missing the previous close) — both skipped.
        let bars = group(vec![
            ("600000", two_bar_series("600000", 100.0, 3.0, 1.0e9)),
            (
                "600777",
                two_bar_series("600777", 100.0, 5.0, 1.0e9)
                    .into_iter()
                    .take(1)
                    .collect(),
            ),
        ]);
        let members = vec![
            member("BK1002", "600000"),
            member("BK1002", "600888"),
            member("BK1002", "600777"),
        ];
        let out = aggregate_concept_daily(&members, &bars);
        let daily = out.get("BK1002").expect("concept present");
        assert_eq!(daily.member_count, 1);
        assert!(
            (daily.pct_change - 3.0).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
        assert_eq!(daily.up_ratio, 1.0);
    }

    #[test]
    fn zero_previous_close_skips_member_without_panic() {
        // prev close == 0.0 → division by zero guard, member skipped.
        let bars = group(vec![
            ("600000", two_bar_series("600000", 0.0, 5.0, 1.0e9)),
            ("600001", two_bar_series("600001", 100.0, 5.0, 1.0e9)),
        ]);
        let members = vec![member("BK1003", "600000"), member("BK1003", "600001")];
        let out = aggregate_concept_daily(&members, &bars);
        let daily = out.get("BK1003").expect("concept present");
        assert_eq!(daily.member_count, 1);
        assert!(
            (daily.pct_change - 5.0).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
    }

    #[test]
    fn all_members_skipped_omits_concept() {
        let bars = group(vec![]);
        let members = vec![member("BK1004", "600000"), member("BK1004", "600001")];
        let out = aggregate_concept_daily(&members, &bars);
        assert!(!out.contains_key("BK1004"));
    }

    #[test]
    fn empty_members_yield_empty_map() {
        let bars = group(vec![(
            "600000",
            two_bar_series("600000", 100.0, 3.0, 1.0e9),
        )]);
        let out = aggregate_concept_daily(&[], &bars);
        assert!(out.is_empty());
    }

    #[test]
    fn single_member_concept_uses_that_member_only() {
        let bars = group(vec![(
            "600000",
            two_bar_series("600000", 100.0, 7.5, 4.0e9),
        )]);
        let members = vec![member("BK1005", "600000")];
        let out = aggregate_concept_daily(&members, &bars);
        let daily = out.get("BK1005").expect("concept present");
        assert!(
            (daily.pct_change - 7.5).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
        assert_eq!(daily.member_count, 1);
        assert_eq!(daily.up_ratio, 1.0);
    }
}
