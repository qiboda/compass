//! Adversarial integration tests for the general Filter AST evaluator
//! (epic #243, Batch 3, issue #246).
//!
//! These tests pin the evaluator's edge semantics: UpDays window boundaries,
//! Count per-day sliding evaluation, deep And/Or/Not nesting, Delisted(true),
//! MarketCap missing-share gating, NDayHigh breakout strictness and
//! factor-vs-factor comparisons. Every test asserts the correct
//! post-Batch-3 behavior (they were written RED against the pre-Batch-3
//! `filter_to_query` reverse-compilation, which rejected these shapes with
//! `ScreenerError::UnsupportedFilter`; the general evaluator
//! [`screener_eval::evaluate`] now makes them pass).
//!
//! Fixture mirrors `tests/screener.rs`: in-memory DuckDB → COPY PARQUET →
//! `ParquetReader`. Adjusted close == raw close in fixtures.

use chrono::{Datelike, Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::run_screener;
use compass_types::{CmpOp, FactorRef, Filter, MetaCond, SeriesCond, SeriesFactor};

/// One daily bar's values; only adjclose/close/volume are used.
#[derive(Clone)]
struct TestBar {
    date: String,
    close: f64,
    volume: f64,
}

/// One fixture stock.
struct TestStock {
    symbol: &'static str,
    name: &'static str,
    industry: Option<&'static str>,
    board: Option<&'static str>,
    list_date: Option<&'static str>,
    delist_date: Option<&'static str>,
    total_share: Option<f64>,
    bars: Vec<TestBar>,
}

/// Build a tempdir with stock_daily.parquet + stock_basic.parquet from fixtures.
fn build_fixture(stocks: &[TestStock]) -> (tempfile::TempDir, ParquetReader) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let conn = duckdb::Connection::open_in_memory().expect("duckdb");

    conn.execute_batch(
        "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
    )
    .expect("create daily");
    for s in stocks {
        for b in &s.bars {
            conn.execute(
                "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    s.symbol,
                    b.date.as_str(),
                    b.close - 1.0,
                    b.close + 1.0,
                    b.close - 0.5,
                    b.close,
                    b.close,
                    b.volume,
                    0.0
                ],
            )
            .expect("insert daily");
        }
    }
    conn.execute_batch(&format!(
        "COPY daily TO '{}' (FORMAT PARQUET)",
        tmp.path().join("stock_daily.parquet").display()
    ))
    .expect("copy daily");

    conn.execute_batch(
        "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
    )
    .expect("create basic");
    for s in stocks {
        conn.execute(
            "INSERT INTO basic VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)",
            duckdb::params![
                s.symbol,
                s.name,
                s.list_date,
                s.delist_date,
                s.board,
                s.name,
                s.total_share,
                s.industry,
            ],
        )
        .expect("insert basic");
    }
    conn.execute_batch(&format!(
        "COPY basic TO '{}' (FORMAT PARQUET)",
        tmp.path().join("stock_basic.parquet").display()
    ))
    .expect("copy basic");

    let reader = ParquetReader::new(tmp.path()).expect("create reader");
    (tmp, reader)
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

/// Weekday-only daily bars ending at `end` (inclusive), values from `closes`.
fn daily_series(end: &str, closes: &[f64], volume: f64) -> Vec<TestBar> {
    let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
    let mut out = Vec::new();
    for close in closes.iter().rev() {
        while matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
            day -= Duration::days(1);
        }
        out.push(TestBar {
            date: day.format("%Y-%m-%d").to_string(),
            close: *close,
            volume,
        });
        day -= Duration::days(1);
    }
    out.reverse();
    out
}

/// A listed main-board stock with the given industry / share count.
fn test_stock(
    symbol: &'static str,
    name: &'static str,
    industry: &'static str,
    total_share: Option<f64>,
    bars: Vec<TestBar>,
) -> TestStock {
    TestStock {
        symbol,
        name,
        industry: Some(industry),
        board: Some("主板"),
        list_date: Some("1991-04-03"),
        delist_date: None,
        total_share,
        bars,
    }
}

/// Closes with daily returns ≈ +1.0%, +1.02%, +1.97% (all > 0.5%).
const RISING_4: [f64; 4] = [100.0, 101.0, 102.03, 104.04];

// ===========================================================================
// UpDays — edge cases
// ===========================================================================

/// UpDays must actually evaluate (currently UnsupportedFilter → Err): 3
/// consecutive days each rising > 0.5% must return the symbol.
#[test]
fn up_days_three_consecutive_rising_days_matches() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &RISING_4, 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 0.5 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "3 consecutive >0.5% rises must match");
    assert_eq!(res.rows[0].symbol, "SZ000001");
}

/// n=0 is vacuously true — even a single-bar series matches.
#[test]
fn up_days_n_zero_is_vacuous_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0], 1.0e6), // single bar
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 0, min_pct: 5.0 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "n=0 is vacuously true");
}

/// Window boundary: n=2 needs exactly 3 bars (1 base + 2 returns).
#[test]
fn up_days_exactly_n_plus_one_bars_matches() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[100.0, 101.0, 102.03], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 2, min_pct: 0.5 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert_eq!(
        res.rows.len(),
        1,
        "exactly n+1 bars must evaluate the streak"
    );
}

/// min_pct is EXCLUSIVE: a return exactly equal to the threshold must NOT
/// count. `(201-200)/200*100 = 0.5` exactly; min_pct 0.5 → streak broken.
#[test]
fn up_days_min_pct_boundary_is_exclusive_no_match() {
    // Boundary: second return is exactly 0.5%.
    let boundary = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[100.0, 200.0, 201.0], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&boundary);
    let filter = Filter::Series(SeriesCond::UpDays { n: 2, min_pct: 0.5 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "return exactly equal to min_pct is NOT an up day"
    );

    // Just above the boundary: 1.05/200*100 = 0.525 > 0.5 → matches.
    let above = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[100.0, 200.0, 201.05], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&above);
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "return just above min_pct IS an up day");
}

/// A zero base price inside the return window makes the streak undefined —
/// must be no-match, not a panic.
#[test]
fn up_days_zero_base_price_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[5.0, 0.0, 2.0], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 2, min_pct: 0.5 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "zero base price → undefined → no match"
    );
}

/// A down day at the FIRST bar of the window breaks the whole streak.
#[test]
fn up_days_streak_broken_at_first_day_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[100.0, 99.0, 100.5, 101.5], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 0.5 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert!(res.rows.is_empty(), "-1% first day kills the n=3 streak");
}

/// min_pct = 0.0 with flat closes: return 0.0 is NOT strictly above 0.0.
#[test]
fn up_days_flat_closes_with_zero_min_pct_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0, 10.0, 10.0, 10.0], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 2, min_pct: 0.0 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "0.0 <= 0.0 → flat days are not up days"
    );
}

/// Non-finite min_pct → undefined → no-match (not an error).
#[test]
fn up_days_nan_min_pct_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &RISING_4, 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays {
        n: 2,
        min_pct: f64::NAN,
    });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert!(res.rows.is_empty(), "NaN threshold → no match");
}

// ===========================================================================
// Count — sliding-window day counting
// ===========================================================================

/// at_least=0: count >= 0 always holds once the window itself is satisfied,
/// even when no single day qualifies.
#[test]
fn count_at_least_zero_matches_even_with_no_qualifying_day() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0; 5], 1.0e6), // flat → DayPct 0 > 0 false
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 5,
        at_least: 0,
    });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Count must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "count 0 >= at_least 0 → match");
}

/// Degenerate window=0: the empty day window yields count 0, and at_least=0
/// still matches (the plan's index-loop formula: len < 0 never holds).
#[test]
fn count_zero_window_with_zero_at_least_matches() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0; 5], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 0,
        at_least: 0,
    });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Count must evaluate, not error");
    assert_eq!(
        res.rows.len(),
        1,
        "empty day window with at_least 0 → match"
    );
}

/// Overall window insufficient: 5 bars < window 10 → no match regardless of
/// at_least.
#[test]
fn count_insufficient_total_window_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0; 5], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::Close,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 10,
        at_least: 1,
    });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Count must evaluate, not error");
    assert!(res.rows.is_empty(), "fewer bars than window → no match");
}

/// at_least above the maximum possible count (window) can never match.
#[test]
fn count_at_least_exceeding_window_never_matches() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[100.0, 101.0, 102.03, 104.04, 105.5], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
        window: 5,
        at_least: 6, // > window, impossible
    });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Count must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "at_least > window can never be reached"
    );
}

/// Per-day factor windows inside the loop: on 10 bars, Sma(5) is only
/// computable from day index 4 — the first 4 days must NOT be counted.
/// Naive all-bars counting would reach 10 ≥ 7; the correct count is 6.
#[test]
fn count_sma_window_insufficient_days_inside_loop_not_counted() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0; 10], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    // Sma(5) > 1.0 is true for every computable day (price 10), but only
    // days 4..=9 (6 days) are computable → count 6.
    let at_least_7 = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::Sma(5),
        op: CmpOp::Gt,
        value: FactorRef::Const(1.0),
        window: 10,
        at_least: 7,
    });
    let res = run_screener(&at_least_7, &reader, date(2026, 7, 28))
        .expect("Count must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "window-insufficient days inside the loop must not be counted"
    );

    // The same filter with at_least 6 must match (exactly the 6 computable
    // days qualify).
    let at_least_6 = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::Sma(5),
        op: CmpOp::Gt,
        value: FactorRef::Const(1.0),
        window: 10,
        at_least: 6,
    });
    let res = run_screener(&at_least_6, &reader, date(2026, 7, 28))
        .expect("Count must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "exactly the 6 computable days qualify");
}

/// Count with value = Factor(another factor): the reference is itself
/// evaluated per-day. On a ~+1%/day series, ChangePct(2) ≈ 2.01% > DayPct
/// ≈ 1.0% for days 2..=9 (8 days) → at_least 8 matches.
#[test]
fn count_factor_value_factor_evaluated_per_day() {
    let closes = [
        100.0, 101.0, 102.01, 103.03, 104.06, 105.10, 106.15, 107.21, 108.29, 109.37,
    ];
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &closes, 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Count {
        factor: SeriesFactor::ChangePct(2),
        op: CmpOp::Gt,
        value: FactorRef::Factor(SeriesFactor::DayPct),
        window: 10,
        at_least: 8,
    });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Count must evaluate, not error");
    assert_eq!(
        res.rows.len(),
        1,
        "ChangePct(2) > DayPct on all computable days"
    );
}

// ===========================================================================
// Or / Not — deep nesting
// ===========================================================================

/// Top-level Or: one branch matching suffices.
#[test]
fn or_any_branch_matches() {
    let stocks = vec![test_stock(
        "SH600519",
        "贵州茅台",
        "白酒",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0; 5], 1000.0),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Or(vec![
        Filter::Meta(MetaCond::Industry(vec!["银行".to_string()])),
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
    ]);
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Or must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "second Or branch matches");
}

/// Not flips a leaf condition: 白酒 stock is not in 银行 → matches.
#[test]
fn not_negates_leaf_condition() {
    let stocks = vec![test_stock(
        "SH600519",
        "贵州茅台",
        "白酒",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0; 5], 1000.0),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Not(Box::new(Filter::Meta(MetaCond::Industry(vec![
        "银行".to_string(),
    ]))));
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("Not must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "Not(银行) matches the 白酒 stock");
}

/// Mixed And/Or/Not tree at combinator depth 8 must evaluate without
/// stack overflow and with correct boolean algebra (even Not count = idempotent).
///
/// Tree: Not^6( And([ Or([银行, 白酒]), UpDays{n:1, min_pct:0.5} ]) ).
///   SZ000001 (银行, rising +1.97%):  Or true, UpDays true → inner true  → match.
///   SH600519 (白酒, flat):           Or true, UpDays false → inner false → no match.
#[test]
fn deeply_nested_and_or_not_tree_evaluates_without_overflow() {
    let stocks = vec![
        test_stock(
            "SZ000001",
            "平安银行",
            "银行",
            Some(1.0e10),
            daily_series("2026-07-28", &RISING_4, 1.0e6),
        ),
        test_stock(
            "SH600519",
            "贵州茅台",
            "白酒",
            Some(1.0e10),
            daily_series("2026-07-28", &[10.0; 4], 1.0e6),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let inner = Filter::And(vec![
        Filter::Or(vec![
            Filter::Meta(MetaCond::Industry(vec!["银行".to_string()])),
            Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        ]),
        Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 0.5 }),
    ]);
    let filter = (0..6).fold(inner, |f, _| Filter::Not(Box::new(f)));

    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("deeply nested evaluator must not error");
    assert_eq!(res.rows.len(), 1, "only the rising 银行 stock matches");
    assert_eq!(res.rows[0].symbol, "SZ000001");
}

// ===========================================================================
// Delisted — D5: Delisted(true) must evaluate
// ===========================================================================

/// Delisted(true) is currently rejected by filter_to_query; the evaluator
/// must match ONLY stocks with a delist_date.
#[test]
fn delisted_true_matches_only_delisted() {
    let stocks = vec![
        test_stock(
            "SZ000001",
            "平安银行",
            "银行",
            Some(1.0e10),
            daily_series("2026-07-28", &[10.0; 5], 1000.0),
        ),
        TestStock {
            symbol: "SZ000004",
            name: "国华退",
            industry: Some("医药"),
            board: Some("主板"),
            list_date: Some("1991-01-01"),
            delist_date: Some("2026-07-14"),
            total_share: Some(1.0e9),
            bars: daily_series("2026-07-01", &[3.0; 5], 1000.0),
        },
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Meta(MetaCond::Delisted(true));
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("Delisted(true) must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "only the delisted stock matches");
    assert_eq!(res.rows[0].symbol, "SZ000004");
}

// ===========================================================================
// MarketCap — missing total_share gating (regression guard for the GUI
// default MarketCap{None,None} card)
// ===========================================================================

/// MarketCap{None,None} (the GUI default card) must NOT drop stocks with a
/// missing total_share — the pre-Batch-3 engine treats them as cap 0.0 and
/// passes the (absent) bound check. Wrapped with a matching UpDays so this is
/// RED today (UpDays → UnsupportedFilter) and pins the guard after Batch 3.
#[test]
fn market_cap_none_none_missing_share_matches_regression_guard() {
    let stocks = vec![
        test_stock(
            "SZ000001",
            "平安银行",
            "银行",
            None, // missing total_share
            daily_series("2026-07-28", &RISING_4, 1.0e6),
        ),
        test_stock(
            "SH600519",
            "贵州茅台",
            "白酒",
            Some(1.0e10),
            daily_series("2026-07-28", &RISING_4, 1.0e6),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::And(vec![
        Filter::Meta(MetaCond::MarketCap {
            min: None,
            max: None,
        }),
        Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 0.1 }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("MarketCap{None,None} + UpDays must evaluate, not error");
    assert_eq!(
        res.rows.len(),
        2,
        "missing total_share must NOT be dropped when no cap bound is active"
    );
    assert!(
        res.rows.iter().any(|r| r.symbol == "SZ000001"),
        "missing-share stock must be present"
    );
    assert!(res.rows.iter().any(|r| r.symbol == "SH600519"));
}

/// Missing total_share + an ACTIVE cap bound → excluded (0.0 cannot satisfy
/// a minimum). Wrapped with UpDays to stay RED today.
#[test]
fn market_cap_bounded_missing_share_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        None, // missing total_share
        daily_series("2026-07-28", &RISING_4, 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::And(vec![
        Filter::Meta(MetaCond::MarketCap {
            min: Some(1.0),
            max: None,
        }),
        Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 0.1 }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("MarketCap bound + UpDays must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "missing share with active cap bound → excluded"
    );
}

// ===========================================================================
// NDayHigh — breakout strictness (previous n bars, EXCLUDING latest)
// ===========================================================================

/// Close > NDayHigh(3): 13 > max(10,11,12) → breakout matches. Wrapped with
/// UpDays{n:0} (vacuously true) so this is RED today.
#[test]
fn nday_high_breakout_positive_matches() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0, 11.0, 12.0, 13.0], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::And(vec![
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(3)),
        }),
        Filter::Series(SeriesCond::UpDays { n: 0, min_pct: 0.0 }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("breakout must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "13 > previous-3 max 12 → breakout");
}

/// Strictness: latest close EQUAL to the previous-3 max is NOT a breakout.
#[test]
fn nday_high_breakout_equal_previous_high_is_strict_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0, 11.0, 12.0, 12.0], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::And(vec![
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::NDayHigh(3)),
        }),
        Filter::Series(SeriesCond::UpDays { n: 0, min_pct: 0.0 }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("breakout must evaluate, not error");
    assert!(
        res.rows.is_empty(),
        "12 > max(10,11,12) is false → no breakout"
    );
}

/// NDayHigh as a LEFT-hand factor (NDayHigh(3) > 11.5) — currently rejected
/// as outside the accept-grammar; the evaluator must support it.
#[test]
fn nday_high_as_left_operand_matches() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0, 11.0, 12.0, 13.0], 1.0e6),
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Cmp {
        factor: SeriesFactor::NDayHigh(3),
        op: CmpOp::Gt,
        value: FactorRef::Const(11.5),
    });
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("NDayHigh left operand must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "previous-3 max 12 > 11.5 → match");
}

// ===========================================================================
// Series Cmp — Factor vs Factor on both sides
// ===========================================================================

/// A SINGLE Cmp{Sma(5), Gt, Factor(Sma(20))} (not the BullishAlign pair) is
/// rejected by the current reverse-compiler. The evaluator must compare the
/// two factors: rising trend → Sma5 > Sma20 → match; falling → no match.
#[test]
fn sma_factor_vs_factor_single_cmp_matches_rising_only() {
    let mut rising = Vec::new();
    for i in 0..30 {
        rising.push(10.0 + i as f64 * 3.0 / 29.0); // 10 → 13, uptrend
    }
    let mut falling = Vec::new();
    for i in 0..30 {
        falling.push(13.0 - i as f64 * 3.0 / 29.0); // 13 → 10, downtrend
    }

    let stocks = vec![
        test_stock(
            "SZ000001",
            "平安银行",
            "银行",
            Some(1.0e10),
            daily_series("2026-07-28", &rising, 1.0e6),
        ),
        test_stock(
            "SH600519",
            "贵州茅台",
            "白酒",
            Some(1.0e10),
            daily_series("2026-07-28", &falling, 1.0e6),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Cmp {
        factor: SeriesFactor::Sma(5),
        op: CmpOp::Gt,
        value: FactorRef::Factor(SeriesFactor::Sma(20)),
    });
    let res = run_screener(&filter, &reader, date(2026, 7, 28))
        .expect("Sma vs Sma comparison must evaluate, not error");
    assert_eq!(res.rows.len(), 1, "only the uptrend stock has Sma5 > Sma20");
    assert_eq!(res.rows[0].symbol, "SZ000001");
}

// ===========================================================================
// Empty / single-bar series — no panic, no-match when window insufficient
// ===========================================================================

/// Single-bar series: UpDays{n:1} needs 2 bars → no match, and must not
/// panic. (run_screener drops zero-bar symbols before evaluation, so the
/// empty-series API boundary cannot be reached through this entry point.)
#[test]
fn single_bar_series_window_insufficient_no_match() {
    let stocks = vec![test_stock(
        "SZ000001",
        "平安银行",
        "银行",
        Some(1.0e10),
        daily_series("2026-07-28", &[10.0], 1.0e6), // single bar
    )];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 1, min_pct: 0.1 });
    let res =
        run_screener(&filter, &reader, date(2026, 7, 28)).expect("UpDays must evaluate, not error");
    assert!(res.rows.is_empty(), "1 bar < n+1=2 → no match, no panic");
}
