//! Requirement acceptance tests for the Batch-3 Filter AST evaluator
//! (epic #243, issue #246).
//!
//! These tests verify `run_screener(&Filter)` against the issue #246
//! acceptance criteria: the AST is executable end-to-end (rows + total,
//! market-cap sorted, capped), series conditions (连续 N 天每日涨幅 > X%,
//! VolumeSurge) genuinely filter, Or/Not/Const-value Cmps evaluate, and
//! delisted handling survives. They were written RED against the
//! pre-Batch-3 `filter_to_query` accept-grammar (which rejected these shapes
//! with `ScreenerError::UnsupportedFilter`) and now pass through the general
//! evaluator in `screener_eval`.
//!
//! Regression baseline: the 21 semantic tests in `tests/screener.rs` must
//! keep passing unchanged — this file only adds the Batch-3 acceptance
//! surface (issue #246 acceptance criteria).

use chrono::{Datelike, Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::{MAX_RESULTS, run_screener};
use compass_types::{
    CmpOp, FactorRef, Filter, MetaCond, ScreenerQuery, ScreenerRow, SeriesCond, SeriesFactor,
};

// --- Fixtures (adapted from tests/screener.rs) ------------------------------

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

/// 30 bars: 26 flat at 100.0 then 100 → 102 → 104.5 → 107.5. The last three
/// daily gains are ≈ 2.0%, 2.45%, 2.87% (all > 1.5%) and the latest close
/// 107.5 sits above the 20-day SMA (~70.7) — passes `UpDays{3, 1.5}` and
/// `Close > Sma(20)`.
fn streak_series(end: &str, volume: f64) -> Vec<TestBar> {
    let mut closes = vec![100.0; 26];
    closes.extend([100.0, 102.0, 104.5, 107.5]);
    daily_series(end, &closes, volume)
}

/// Same flat base but a down day inside the last three returns
/// (100 → 102 → 99 → 101.5): +2.0%, −2.94%, +2.53% — fails `UpDays{3, 1.5}`.
fn down_day_series(end: &str, volume: f64) -> Vec<TestBar> {
    let mut closes = vec![100.0; 26];
    closes.extend([100.0, 102.0, 99.0, 101.5]);
    daily_series(end, &closes, volume)
}

/// Falling base (200 → 100 over 26 bars) then the same rising tail: the
/// streak passes `UpDays{3, 1.5}` but the latest close 107.5 stays BELOW the
/// 20-day SMA (~124.7) — fails `Close > Sma(20)`.
fn falling_then_streak_series(end: &str, volume: f64) -> Vec<TestBar> {
    let mut closes = Vec::new();
    for i in 0..26 {
        closes.push(200.0 - i as f64 * 4.0);
    }
    closes.extend([100.0, 102.0, 104.5, 107.5]);
    daily_series(end, &closes, volume)
}

/// 60 bars ending at `end`: the last 20 at `recent_vol`, the older 40 at
/// `base_vol` (volume-surge fixture, mirrors tests/screener.rs).
fn volume_series(end: &str, recent_vol: f64, base_vol: f64) -> Vec<TestBar> {
    let mut series = Vec::new();
    let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse");
    for i in 0..60 {
        while matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
            day -= Duration::days(1);
        }
        let vol = if i < 20 { recent_vol } else { base_vol };
        series.push(TestBar {
            date: day.format("%Y-%m-%d").to_string(),
            close: 10.0,
            volume: vol,
        });
        day -= Duration::days(1);
    }
    series.reverse();
    series
}

/// Minimal fixture stock with explicit symbol/industry/delist state.
fn stock(
    symbol: &'static str,
    industry: &'static str,
    delist_date: Option<&'static str>,
    bars: Vec<TestBar>,
) -> TestStock {
    TestStock {
        symbol,
        name: "测试股",
        industry: Some(industry),
        board: Some("主板"),
        list_date: Some("2001-01-01"),
        delist_date,
        total_share: Some(1.0e10),
        bars,
    }
}

/// Result symbols in `rows` order.
fn symbols(rows: &[ScreenerRow]) -> Vec<&str> {
    rows.iter().map(|r| r.symbol.as_str()).collect()
}

// --- Issue #246 acceptance criteria -----------------------------------------

/// #246 "Filter AST 可执行": a mixed Meta+Series+And filter (including the
/// Batch-3 `UpDays` node, which makes the whole shape UnsupportedFilter today)
/// returns the `ScreenerRow` result set with `rows` + `total`, market-cap
/// descending, capped at `MAX_RESULTS`.
#[test]
fn mixed_meta_series_and_filter_returns_sorted_capped_rows() {
    let mut stocks: Vec<TestStock> = Vec::new();
    for i in 0..105 {
        // Distinct total_share → distinct market cap (1075 + 10.75·i 亿), so
        // the sort order is fully determined: highest i first.
        let symbol = format!("SH600{:03}", i).leak();
        stocks.push(TestStock {
            symbol,
            name: "混合测试股",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-01-01"),
            delist_date: None,
            total_share: Some(1.0e9 + i as f64 * 1.0e7),
            bars: streak_series("2026-07-28", 1000.0),
        });
    }
    // A 银行 stock that fails the industry branch must not appear.
    stocks.push(TestStock {
        symbol: "SH600900",
        name: "银行股",
        industry: Some("银行"),
        board: Some("主板"),
        list_date: Some("2001-01-01"),
        delist_date: None,
        total_share: Some(1.0e10),
        bars: streak_series("2026-07-28", 1000.0),
    });
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::And(vec![
        Filter::Meta(MetaCond::Delisted(false)),
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        }),
        Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.5 }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    assert_eq!(res.rows.len(), MAX_RESULTS, "rows capped at 100");
    assert_eq!(res.total, 105, "total counts all matches before the cap");
    for pair in res.rows.windows(2) {
        assert!(
            pair[0].market_cap >= pair[1].market_cap,
            "rows must be market-cap descending: {} ({}) before {} ({})",
            pair[0].symbol,
            pair[0].market_cap,
            pair[1].symbol,
            pair[1].market_cap,
        );
    }
    assert_eq!(res.rows[0].symbol, "SH600104", "highest-cap stock first");
    for row in &res.rows {
        assert_eq!(row.industry, "白酒", "industry branch must filter");
    }
}

/// #246 "连续 N 天每日涨幅 > X% 真实过滤": `UpDays{n: 3, min_pct: 1.5}` through
/// `run_screener` keeps only the stock whose last three daily gains each
/// exceed 1.5%; a down day inside the window disqualifies, and a series
/// shorter than n+1 bars matches nothing (window insufficient → no-match,
/// no crash).
#[test]
fn up_days_three_day_streak_filters_through_run_screener() {
    let stocks = vec![
        stock(
            "SH600001",
            "白酒",
            None,
            streak_series("2026-07-28", 1000.0),
        ),
        stock(
            "SH600002",
            "银行",
            None,
            down_day_series("2026-07-28", 1000.0),
        ),
        // Only 3 bars: fewer than the n+1 = 4 needed for n=3.
        stock(
            "SH600003",
            "医药",
            None,
            daily_series("2026-07-28", &[100.0, 102.0, 104.5], 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.5 });
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    assert_eq!(
        res.rows.len(),
        1,
        "only the 3-day >1.5% streak stock matches; down-day and short-window stocks must be excluded"
    );
    assert_eq!(res.rows[0].symbol, "SH600001");
    assert_eq!(res.total, 1);
}

/// #246 + epic "任意嵌套 AND/OR/NOT": an `Or` node matches a stock when either
/// branch holds (a stock matching neither branch is excluded).
#[test]
fn or_semantics_matches_either_branch() {
    let stocks = vec![
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1_256_197_800.0),
            bars: daily_series("2026-07-28", &[1500.0; 5], 1000.0),
        },
        stock(
            "SZ000001",
            "银行",
            None,
            daily_series("2026-07-28", &[10.0; 5], 1000.0),
        ),
        stock(
            "SZ300001",
            "医药",
            None,
            daily_series("2026-07-28", &[10.0; 5], 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Or(vec![
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        Filter::Meta(MetaCond::Industry(vec!["银行".to_string()])),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    let got = symbols(&res.rows);
    assert_eq!(res.rows.len(), 2, "either branch matches");
    assert!(got.contains(&"SH600519"), "白酒 branch");
    assert!(got.contains(&"SZ000001"), "银行 branch");
    assert!(!got.contains(&"SZ300001"), "neither branch → excluded");
}

/// #246 + epic "任意嵌套 AND/OR/NOT": a `Not` node inverts its sub-filter —
/// `Not(Industry(["银行"]))` excludes the bank while keeping the others.
#[test]
fn not_semantics_excludes_industry() {
    let stocks = vec![
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1_256_197_800.0),
            bars: daily_series("2026-07-28", &[1500.0; 5], 1000.0),
        },
        stock(
            "SZ000001",
            "银行",
            None,
            daily_series("2026-07-28", &[10.0; 5], 1000.0),
        ),
        stock(
            "SZ300001",
            "医药",
            None,
            daily_series("2026-07-28", &[10.0; 5], 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Not(Box::new(Filter::Meta(MetaCond::Industry(vec![
        "银行".to_string(),
    ]))));
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    let got = symbols(&res.rows);
    assert_eq!(res.rows.len(), 2, "bank excluded, others kept");
    assert!(got.contains(&"SH600519"));
    assert!(got.contains(&"SZ300001"));
    assert!(
        !got.contains(&"SZ000001"),
        "Not(银行) must exclude the bank"
    );
}

/// #246 And combination: `Meta(Industry) + Series(Cmp{Close, Gt, Sma(20)}) +
/// UpDays` — each condition individually disqualifies exactly one stock, so
/// only the stock satisfying all three matches. Also asserts the row assembly
/// formula `market_cap = total_share × latest.close / 1e8`.
#[test]
fn and_combination_meta_series_updays_requires_all_conditions() {
    let stocks = vec![
        // A: 白酒, above Sma20, 3-day >1.5% streak → passes everything.
        stock(
            "SH600001",
            "白酒",
            None,
            streak_series("2026-07-28", 1000.0),
        ),
        // B: 银行 with the same series → fails the Industry branch only.
        stock(
            "SH600002",
            "银行",
            None,
            streak_series("2026-07-28", 1000.0),
        ),
        // C: 白酒, streak passes UpDays but latest close below Sma20 → fails
        //    the Close > Sma(20) branch only.
        stock(
            "SH600003",
            "白酒",
            None,
            falling_then_streak_series("2026-07-28", 1000.0),
        ),
        // D: 白酒, above Sma20 but a down day in the window → fails UpDays only.
        stock(
            "SH600004",
            "白酒",
            None,
            down_day_series("2026-07-28", 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::And(vec![
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        Filter::Series(SeriesCond::Cmp {
            factor: SeriesFactor::Close,
            op: CmpOp::Gt,
            value: FactorRef::Factor(SeriesFactor::Sma(20)),
        }),
        Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.5 }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    assert_eq!(res.rows.len(), 1, "only the all-conditions stock matches");
    assert_eq!(res.rows[0].symbol, "SH600001");
    assert_eq!(res.total, 1);
    // Row assembly: total_share 1e10 × latest close 107.5 / 1e8 = 10750 亿.
    assert!(
        (res.rows[0].market_cap - 1.0e10 * 107.5 / 1e8).abs() < 1.0,
        "market_cap = total_share × latest.close / 1e8, got {}",
        res.rows[0].market_cap,
    );
}

/// #246 "连续 N 天每日涨幅 > X% 等序列条件真实过滤": `VolumeSurge` composes with
/// the Batch-3 evaluator — inside an `Or`, the surge branch distinguishes the
/// surging stock (recent 20-bar avg 5000 ≥ 2× the nested 3N=60-bar baseline
/// avg ≈ 2333) from the flat-volume stock (ratio ≈ 1 < 2).
#[test]
fn volume_surge_branch_filters_through_run_screener() {
    let stocks = vec![
        // 白酒: matches via the industry branch (flat volume, irrelevant).
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1_256_197_800.0),
            bars: daily_series("2026-07-28", &[1500.0; 60], 1000.0),
        },
        // 银行 + surge: recent 20 bars at 5000, older 40 at 1000 → ratio
        // 5000/2333 ≈ 2.14 ≥ 2 → matches the VolumeSurge branch.
        stock(
            "SZ000002",
            "银行",
            None,
            volume_series("2026-07-28", 5000.0, 1000.0),
        ),
        // 银行 + flat volume: ratio ≈ 1 < 2 → matches neither branch.
        stock(
            "SZ000003",
            "银行",
            None,
            volume_series("2026-07-28", 1000.0, 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Or(vec![
        Filter::Meta(MetaCond::Industry(vec!["白酒".to_string()])),
        Filter::Series(SeriesCond::VolumeSurge {
            days: 20,
            times: 2.0,
        }),
    ]);
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    let got = symbols(&res.rows);
    assert_eq!(
        res.rows.len(),
        2,
        "白酒 by industry, surging 银行 by volume"
    );
    assert!(got.contains(&"SH600519"));
    assert!(got.contains(&"SZ000002"), "volume-surge stock must match");
    assert!(
        !got.contains(&"SZ000003"),
        "flat-volume 银行 must be excluded (surge is genuine)"
    );
}

/// #246 "序列条件真实过滤": an isolated `Const`-valued `Cmp`
/// (`DayPct > Const(0.0)` — a shape the reverse compiler rejects today) keeps
/// only the stock whose latest daily return is positive; a negative-day stock
/// and a single-bar stock (window insufficient) are excluded.
#[test]
fn day_pct_const_cmp_filters_daily_gainers() {
    let stocks = vec![
        // Latest day +2.19%: (107.5 − 105.2) / 105.2 × 100.
        stock(
            "SH600001",
            "白酒",
            None,
            daily_series("2026-07-28", &[100.0, 105.2, 107.5], 1000.0),
        ),
        // Latest day −2.09%: (103.0 − 105.2) / 105.2 × 100.
        stock(
            "SH600002",
            "银行",
            None,
            daily_series("2026-07-28", &[100.0, 105.2, 103.0], 1000.0),
        ),
        // Single bar: DayPct needs two bars → no-match.
        stock(
            "SH600003",
            "医药",
            None,
            daily_series("2026-07-28", &[100.0], 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let filter = Filter::Series(SeriesCond::Cmp {
        factor: SeriesFactor::DayPct,
        op: CmpOp::Gt,
        value: FactorRef::Const(0.0),
    });
    let res = run_screener(&filter, &reader, date(2026, 7, 28)).expect("run");

    assert_eq!(
        res.rows.len(),
        1,
        "only the positive daily-gain stock matches"
    );
    assert_eq!(res.rows[0].symbol, "SH600001");
}

/// #246 delisted handling (evaluator path): the default query
/// (`Filter::from(ScreenerQuery::default())` → `Meta(Delisted(false))`)
/// excludes the delisted stock, and the evaluator must keep excluding it when
/// `Delisted(false)` is AND-ed with a Batch-3 series node (RED today — UpDays
/// is UnsupportedFilter).
#[test]
fn delisted_excluded_by_default_and_by_delisted_false_node() {
    let stocks = vec![
        stock(
            "SZ000001",
            "银行",
            None,
            streak_series("2026-07-28", 1000.0),
        ),
        // Delisted before `now`; bars end before the delist date.
        stock(
            "SZ000004",
            "医药",
            Some("2026-07-14"),
            streak_series("2026-07-01", 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);
    let now = date(2026, 7, 28);

    // Baseline (regression semantics, GREEN today): default query excludes.
    let default_res =
        run_screener(&Filter::from(ScreenerQuery::default()), &reader, now).expect("run default");
    assert_eq!(default_res.rows.len(), 1, "delisted excluded by default");
    assert_eq!(default_res.rows[0].symbol, "SZ000001");

    // Evaluator path: the delisted stock passes UpDays, so only the
    // Delisted(false) node keeps it out.
    let filter = Filter::And(vec![
        Filter::Meta(MetaCond::Delisted(false)),
        Filter::Series(SeriesCond::UpDays { n: 3, min_pct: 1.5 }),
    ]);
    let res = run_screener(&filter, &reader, now).expect("run");
    assert_eq!(
        res.rows.len(),
        1,
        "delisted must stay excluded under the evaluator"
    );
    assert_eq!(res.rows[0].symbol, "SZ000001");
}

/// #246 delisted handling (evaluator path): `exclude_delisted = false` emits
/// no `Delisted` node (empty `And`), so the delisted stock is included — and
/// the evaluator must preserve that when a Batch-3 series node is the only
/// condition: without a `Delisted(false)` node the delisted stock passes
/// `UpDays` and appears (RED today — UpDays is UnsupportedFilter).
#[test]
fn delisted_included_when_not_excluded_under_evaluator() {
    let stocks = vec![
        // Active stock fails UpDays (down day in the window).
        stock(
            "SZ000001",
            "银行",
            None,
            down_day_series("2026-07-28", 1000.0),
        ),
        // Delisted stock passes UpDays and must be included (no Delisted node).
        stock(
            "SZ000004",
            "医药",
            Some("2026-07-14"),
            streak_series("2026-07-01", 1000.0),
        ),
    ];
    let (_tmp, reader) = build_fixture(&stocks);
    let now = date(2026, 7, 28);

    // Baseline (regression semantics, GREEN today): no Delisted node → include.
    let q = ScreenerQuery {
        exclude_delisted: false,
        ..ScreenerQuery::default()
    };
    let baseline = run_screener(&Filter::from(q), &reader, now).expect("run baseline");
    assert_eq!(
        baseline.rows.len(),
        2,
        "delisted included when exclusion disabled"
    );

    // Evaluator path: And([UpDays]) carries no Delisted node → the delisted
    // stock with a streak is the only match.
    let filter = Filter::And(vec![Filter::Series(SeriesCond::UpDays {
        n: 3,
        min_pct: 1.5,
    })]);
    let res = run_screener(&filter, &reader, now).expect("run");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(
        res.rows[0].symbol, "SZ000004",
        "delisted stock must pass the series condition when not excluded"
    );
}
