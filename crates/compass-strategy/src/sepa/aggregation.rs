//! Local industry-board daily aggregation (epic #139 decision 8/20,
//! issue #283 D5: theme scoring switched from concept memberships to
//! `stock_basic.industry` grouping).
//!
//! Aggregates per-industry daily statistics from the per-symbol industry
//! classification (stock_basic.industry) plus whole-market cross-section
//! bars: the equal-weighted mean day-over-day change of the members, the
//! summed latest amount, the fraction of members that rose, and the member
//! count. This is the local replacement for EastMoney board indexes — no
//! online calls, no board-quote interface.
//!
//! Symbol keys are exchange-prefixed (`SH600519`); the industry map and
//! cross-section bars both carry prefixed symbols, so joins use the keys
//! as-is (no normalization, issue #181).

use std::collections::HashMap;

use compass_core::model::CrossSectionBar;

/// One industry board's daily aggregate, consumed by the theme scoring
/// module (todo 10). Internal structure — not part of the GUI contract.
#[derive(Debug, Clone, PartialEq)]
pub struct IndustryDaily {
    /// Equal-weighted mean day-over-day change of the members (%).
    pub pct_change: f64,
    /// Sum of the members' latest-bar trading amounts (yuan).
    pub amount: f64,
    /// Fraction of aggregated members that rose (`pct > 0`), in 0..1.
    pub up_ratio: f64,
    /// Number of members with computable data (after skipping).
    pub member_count: usize,
}

/// Aggregate industry boards from the per-symbol industry classification and
/// per-symbol bar series.
///
/// `industry_of` maps each exchange-prefixed symbol to its industry name
/// (from `stock_basic.industry`; symbols without a classification are simply
/// not aggregated). `bars_by_symbol` must be keyed by **exchange-prefixed**
/// symbols with series in ascending `trade_date` order (as returned by
/// [`ParquetReader::fetch_cross_section`](compass_core::data::parquet::ParquetReader::fetch_cross_section)).
///
/// Per member the day-over-day change is
/// `(close_t - close_{t-1}) / close_{t-1} * 100` from the latest two bars.
/// A member is skipped when it has fewer than two bars (missing latest bar),
/// when the relevant closes are non-finite, or when the previous close is
/// zero. An industry whose members are all skipped has no entry in the result
/// map. `up_ratio` and the equal-weight mean are computed over the surviving
/// members only.
pub fn aggregate_industry_daily(
    industry_of: &HashMap<String, String>,
    bars_by_symbol: &HashMap<String, Vec<&CrossSectionBar>>,
) -> HashMap<String, IndustryDaily> {
    // Latest two bars per symbol (prefixed symbol keys).
    let mut pct_by_symbol: HashMap<&str, f64> = HashMap::new();
    let mut amount_by_symbol: HashMap<&str, f64> = HashMap::new();
    for (symbol, series) in bars_by_symbol {
        if series.len() < 2 {
            continue;
        }
        let latest = series[series.len() - 1];
        let prev = series[series.len() - 2];
        if !latest.close.is_finite() || !prev.close.is_finite() || prev.close == 0.0 {
            continue;
        }
        pct_by_symbol.insert(
            symbol.as_str(),
            (latest.close - prev.close) / prev.close * 100.0,
        );
        amount_by_symbol.insert(
            symbol.as_str(),
            if latest.amount.is_finite() {
                latest.amount
            } else {
                0.0
            },
        );
    }

    let mut out: HashMap<String, IndustryDaily> = HashMap::new();
    for (symbol, industry) in industry_of {
        let Some(&pct) = pct_by_symbol.get(symbol.as_str()) else {
            continue;
        };
        let amount = amount_by_symbol
            .get(symbol.as_str())
            .copied()
            .unwrap_or(0.0);
        let daily = out.entry(industry.clone()).or_insert(IndustryDaily {
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
        let industry_of = HashMap::from([
            ("SH600000".to_string(), "半导体".to_string()),
            ("SZ000001".to_string(), "半导体".to_string()),
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        let daily = out.get("半导体").expect("industry present");
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
    fn prefixed_symbols_join_prefixed_bars_without_normalization() {
        // Industry and bar keys are both exchange-prefixed and join
        // directly; a bare-code symbol does NOT match a prefixed key (the
        // old bare-code normalization is gone, issue #181).
        let bars = group(vec![
            ("SH600519", two_bar_series("SH600519", 100.0, 10.0, 1.0e8)),
            ("SZ000001", two_bar_series("SZ000001", 100.0, 3.0, 2.0e8)),
            ("SZ000002", two_bar_series("SZ000002", 100.0, -2.0, 3.0e8)),
        ]);
        let industry_of = HashMap::from([
            ("SH600519".to_string(), "白酒".to_string()),
            ("SZ000001".to_string(), "白酒".to_string()),
            ("000001".to_string(), "白酒".to_string()), // bare → no key match, skipped
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        let daily = out.get("白酒").expect("industry present");
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
    fn symbols_without_latest_bar_or_prev_are_skipped() {
        // "SH600888" has no bars at all; "SH600777" has a single bar
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
        let industry_of = HashMap::from([
            ("SH600000".to_string(), "银行".to_string()),
            ("SH600888".to_string(), "银行".to_string()),
            ("SH600777".to_string(), "银行".to_string()),
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        let daily = out.get("银行").expect("industry present");
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
        let industry_of = HashMap::from([
            ("SH600000".to_string(), "银行".to_string()),
            ("SH600001".to_string(), "银行".to_string()),
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        let daily = out.get("银行").expect("industry present");
        assert_eq!(daily.member_count, 1);
        assert!(
            (daily.pct_change - 5.0).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
    }

    #[test]
    fn all_members_skipped_omits_industry() {
        let bars = group(vec![]);
        let industry_of = HashMap::from([
            ("SH600000".to_string(), "银行".to_string()),
            ("SH600001".to_string(), "银行".to_string()),
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        assert!(!out.contains_key("银行"));
    }

    #[test]
    fn empty_industry_map_yields_empty_result() {
        let bars = group(vec![(
            "SH600000",
            two_bar_series("SH600000", 100.0, 3.0, 1.0e9),
        )]);
        let out = aggregate_industry_daily(&HashMap::new(), &bars);
        assert!(out.is_empty());
    }

    #[test]
    fn single_member_industry_uses_that_member_only() {
        let bars = group(vec![(
            "SH600000",
            two_bar_series("SH600000", 100.0, 7.5, 4.0e9),
        )]);
        let industry_of = HashMap::from([("SH600000".to_string(), "白酒".to_string())]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        let daily = out.get("白酒").expect("industry present");
        assert!(
            (daily.pct_change - 7.5).abs() < 1e-9,
            "got {}",
            daily.pct_change
        );
        assert_eq!(daily.member_count, 1);
        assert_eq!(daily.up_ratio, 1.0);
    }

    #[test]
    fn symbols_without_industry_are_not_aggregated() {
        let bars = group(vec![(
            "SH600000",
            two_bar_series("SH600000", 100.0, 7.5, 4.0e9),
        )]);
        // Only the bar series exists; no industry classification.
        let out = aggregate_industry_daily(&HashMap::new(), &bars);
        assert!(out.is_empty());
    }

    // ── Adversarial: industry-name special characters (issue #283 D5) ─────────

    #[test]
    fn blank_industry_name_is_a_distinct_group() {
        // stock_basic.industry may carry an empty/whitespace string for symbols
        // with no classification. It is a valid HashMap key and must aggregate
        // into its own group — never panic, never fold into a real industry.
        let bars = group(vec![
            ("SH600000", two_bar_series("SH600000", 100.0, 3.0, 1.0e9)),
            ("SH600001", two_bar_series("SH600001", 100.0, 4.0, 2.0e9)),
        ]);
        let industry_of = HashMap::from([
            ("SH600000".to_string(), String::new()),     // empty string
            ("SH600001".to_string(), "   ".to_string()), // whitespace-only
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        assert_eq!(
            out.len(),
            2,
            "each distinct key (incl. blank) is its own group"
        );
        let empty = out.get("").expect("the empty-string industry aggregates");
        assert_eq!(empty.member_count, 1);
        assert!((empty.pct_change - 3.0).abs() < 1e-9);
        let ws = out
            .get("   ")
            .expect("the whitespace-only industry aggregates");
        assert_eq!(ws.member_count, 1);
    }

    #[test]
    fn unicode_and_punctuation_industry_names_do_not_collide() {
        // Names rich in leading/trailing space + unicode punctuation must stay
        // distinct — trimming or folding them would merge distinct branches.
        let bars = group(vec![
            ("SH600000", two_bar_series("SH600000", 100.0, 1.0, 1.0)),
            ("SH600001", two_bar_series("SH600001", 100.0, 2.0, 1.0)),
            ("SH600002", two_bar_series("SH600002", 100.0, 3.0, 1.0)),
        ]);
        let names = [
            " 半导体 ", // trimmed-matching a distinct name
            "半导体",   // the "clean" twin — must NOT merge above
            "银行 /【金融】",
        ];
        let industry_of = HashMap::from([
            ("SH600000".to_string(), names[0].to_string()),
            ("SH600001".to_string(), names[1].to_string()),
            ("SH600002".to_string(), names[2].to_string()),
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        assert_eq!(
            out.len(),
            3,
            "every distinct industry name keeps its own group"
        );
        assert_eq!(out.get(" 半导体 ").unwrap().member_count, 1);
        assert_eq!(out.get("半导体").unwrap().member_count, 1);
        assert_eq!(out.get("银行 /【金融】").unwrap().member_count, 1);
    }

    #[test]
    fn skipped_industry_omitted_while_sibling_industry_kept() {
        // One industry's members all lack bars (skipped) — it must be OMITTED
        // from the result (no zero-member ghost entry), while a sibling industry
        // with valid members still lands. Otherwise a per-industry loop over the
        // result would divide by a phantom member_count == 0.
        let bars = group(vec![(
            "SH600000",
            two_bar_series("SH600000", 100.0, 5.0, 1.0e9),
        )]);
        let industry_of = HashMap::from([
            ("SH600000".to_string(), "银行".to_string()),
            ("SH600777".to_string(), "银行".to_string()), // no bars → skipped
            ("SZ000001".to_string(), "医药".to_string()), // no bars → skipped entirely
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        assert!(
            !out.contains_key("医药"),
            "a fully-barless industry must be omitted"
        );
        let bank = out
            .get("银行")
            .expect("the sibling with a valid member stays");
        assert_eq!(
            bank.member_count, 1,
            "the barless member is skipped, not counted"
        );
        assert!((bank.pct_change - 5.0).abs() < 1e-9);
    }

    #[test]
    fn up_ratio_excludes_zero_and_negative_changes() {
        // up_ratio counts only pct > 0.
        let bars = group(vec![
            ("SH600000", two_bar_series("SH600000", 100.0, 0.0, 1.0)), // flat
            ("SH600001", two_bar_series("SH600001", 100.0, -2.0, 1.0)), // down
            ("SH600002", two_bar_series("SH600002", 100.0, 4.0, 1.0)), // up
        ]);
        let industry_of = HashMap::from([
            ("SH600000".to_string(), "综合".to_string()),
            ("SH600001".to_string(), "综合".to_string()),
            ("SH600002".to_string(), "综合".to_string()),
        ]);
        let out = aggregate_industry_daily(&industry_of, &bars);
        let d = out.get("综合").unwrap();
        assert_eq!(d.member_count, 3);
        assert!((d.up_ratio - 1.0 / 3.0).abs() < 1e-9, "got {}", d.up_ratio);
    }

    // ── Adversarial: scale — the aggregation must stay ~O(n), no O(n²) ────────

    #[test]
    fn aggregates_many_industries_without_quadratic_blowup() {
        // 5_000 symbols spread across 1_000 industries: a per-member scan for
        // each industry (O(n·k)) would be catastrophic here; a single pass over
        // symbols (O(n)) completes instantly. Correctness is asserted exactly so
        // an O(n²) regression can neither hide behind wrong counts nor crash on
        // real-scale input. The wall-time guard is deliberately loose (flaky-CI
        // safe) — the point is the algorithm shape, not a tight timer.
        let n = 5_000usize;
        let names: Vec<String> = (0..n).map(|i| format!("industry{i}")).collect();
        // Owned symbol strings live for the whole test; the ``group`` helper is
        // fed ``&str`` borrows and leaks the vec backing the ref-map.
        let symbols: Vec<String> = (0..n).map(|i| format!("SH{i:06}")).collect();
        let mut series: Vec<(&str, Vec<CrossSectionBar>)> = Vec::with_capacity(n);
        let mut industry_of = HashMap::with_capacity(n);
        for i in 0..n {
            series.push((&symbols[i], two_bar_series(&symbols[i], 100.0, 1.0, 1.0e9)));
            industry_of.insert(symbols[i].clone(), names[i % 1_000].clone());
        }
        let bars = group(series);

        let start = std::time::Instant::now();
        let out = aggregate_industry_daily(&industry_of, &bars);
        let elapsed_secs = start.elapsed().as_secs_f64();

        // Every industry has exactly 5 members → pct == 1.0 and up_ratio == 1.0.
        assert_eq!(out.len(), 1_000, "all 1_000 industries present");
        for (name, daily) in &out {
            assert_eq!(daily.member_count, 5, "industry {name}");
            assert!((daily.pct_change - 1.0).abs() < 1e-9, "industry {name}");
            assert_eq!(daily.up_ratio, 1.0, "industry {name}");
        }
        // 5k×2-bar aggregation on this hot path must complete fast even on a
        // slow CI box; an O(n²) implementation would blow past the 3s bound.
        assert!(
            elapsed_secs < 3.0,
            "5k members across 1k industries took {elapsed_secs:.3}s — O(n²) regression?"
        );
    }
}
