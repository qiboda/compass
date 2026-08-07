//! Local concept-board daily aggregation (epic #139 decision 8/20).
//!
//! Aggregates per-concept-board daily statistics from the raw membership
//! snapshot ([`ConceptMember`]) plus whole-market cross-section bars: the
//! equal-weighted mean day-over-day change of the members, the summed latest
//! amount, the fraction of members that rose, and the member count. This is
//! the local replacement for EastMoney board indexes — no online calls, no
//! board-quote interface.
//!
//! Symbol keys are exchange-prefixed (`SH600519`); membership rows and
//! cross-section bars both carry prefixed symbols, so joins use the keys
//! as-is (no normalization, issue #181).

use std::collections::HashMap;

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
/// `bars_by_symbol` must be keyed by **exchange-prefixed** symbols with
/// series in ascending `trade_date` order (as returned by
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
    // Latest two bars per symbol (prefixed symbol keys).
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
        let Some(&pct) = pct_by_symbol.get(member.symbol.as_str()) else {
            continue;
        };
        let amount = amount_by_symbol
            .get(member.symbol.as_str())
            .copied()
            .unwrap_or(0.0);
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
    /// `amount`. `symbol` is the exchange-prefixed code used as the map key.
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

    /// Member row; `symbol` must be exchange-prefixed (SH/SZ/BJ).
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
            ("SH600000", two_bar_series("SH600000", 100.0, 3.0, 1.0e9)),
            ("SZ000001", two_bar_series("SZ000001", 100.0, 5.0, 2.0e9)),
        ]);
        let members = vec![member("BK1000", "SH600000"), member("BK1000", "SZ000001")];
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
    fn prefixed_members_join_prefixed_bars_without_normalization() {
        // Membership and bar keys are both exchange-prefixed and join
        // directly; a bare-code member does NOT match a prefixed key (the
        // old bare-code normalization is gone, issue #181).
        let bars = group(vec![
            ("SH600519", two_bar_series("SH600519", 100.0, 10.0, 1.0e8)),
            ("SZ000001", two_bar_series("SZ000001", 100.0, 3.0, 2.0e8)),
            ("SZ000002", two_bar_series("SZ000002", 100.0, -2.0, 3.0e8)),
        ]);
        let members = vec![
            member("BK1001", "SH600519"),
            member("BK1001", "SZ000001"),
            member("BK1001", "000001"), // bare → no key match, skipped
        ];
        let out = aggregate_concept_daily(&members, &bars);
        let daily = out.get("BK1001").expect("concept present");
        // Only the two prefixed members join: (10 + 3) / 2.
        assert!(
            (daily.pct_change - 6.5).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
        assert_eq!(daily.up_ratio, 1.0);
        assert_eq!(daily.member_count, 2);
        assert!((daily.amount - 3.0e8).abs() < 1e-9, "got {}", daily.amount);
    }

    #[test]
    fn members_without_latest_bar_or_prev_are_skipped() {
        // Member "SH600888" has no bars at all; "SH600777" has a single bar
        // (missing the previous close) — both skipped.
        let bars = group(vec![
            ("SH600000", two_bar_series("SH600000", 100.0, 3.0, 1.0e9)),
            (
                "SH600777",
                two_bar_series("SH600777", 100.0, 5.0, 1.0e9)
                    .into_iter()
                    .take(1)
                    .collect(),
            ),
        ]);
        let members = vec![
            member("BK1002", "SH600000"),
            member("BK1002", "SH600888"),
            member("BK1002", "SH600777"),
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
            ("SH600000", two_bar_series("SH600000", 0.0, 5.0, 1.0e9)),
            ("SH600001", two_bar_series("SH600001", 100.0, 5.0, 1.0e9)),
        ]);
        let members = vec![member("BK1003", "SH600000"), member("BK1003", "SH600001")];
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
        let members = vec![member("BK1004", "SH600000"), member("BK1004", "SH600001")];
        let out = aggregate_concept_daily(&members, &bars);
        assert!(!out.contains_key("BK1004"));
    }

    #[test]
    fn empty_members_yield_empty_map() {
        let bars = group(vec![(
            "SH600000",
            two_bar_series("SH600000", 100.0, 3.0, 1.0e9),
        )]);
        let out = aggregate_concept_daily(&[], &bars);
        assert!(out.is_empty());
    }

    #[test]
    fn single_member_concept_uses_that_member_only() {
        let bars = group(vec![(
            "SH600000",
            two_bar_series("SH600000", 100.0, 7.5, 4.0e9),
        )]);
        let members = vec![member("BK1005", "SH600000")];
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
