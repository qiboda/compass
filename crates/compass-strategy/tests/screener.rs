//! Integration tests for the screening engine.
//!
//! Fixtures build a tempdir parquet dataset (stock_daily + stock_basic) and
//! run the engine against it, mirroring the compass-core test pattern.

use chrono::{Datelike, Duration, NaiveDate};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::{MAX_RESULTS, run_screener};
use compass_types::{
    BreakoutCondition, Filter, MaCondition, MomentumCondition, ScreenerQuery, VolumeCondition,
};

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

/// Series with a clear 20-day momentum: closes rise linearly 10→20.
fn rising_series(end: &str, volume: f64) -> Vec<TestBar> {
    let mut closes = Vec::new();
    for i in 0..40 {
        closes.push(10.0 + i as f64 * 10.0 / 39.0);
    }
    daily_series(end, &closes, volume)
}

fn stock_000001(bars: Vec<TestBar>) -> TestStock {
    TestStock {
        symbol: "SZ000001",
        name: "平安银行",
        industry: Some("银行"),
        board: Some("主板"),
        list_date: Some("1991-04-03"),
        delist_date: None,
        total_share: Some(1.0e10),
        bars,
    }
}

#[test]
fn empty_query_returns_market_sorted_by_cap() {
    let stocks = vec![
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1_256_197_800.0), // ~12.56亿股
            bars: daily_series("2026-07-28", &[1500.0; 5], 1000.0),
        },
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let res = run_screener(
        &Filter::from(ScreenerQuery::default()),
        &reader,
        date(2026, 7, 28),
    )
    .expect("run");
    // 平安 194亿×10 = 1940亿；茅台 12.56亿×1500 = 18840亿 → 茅台第一
    assert_eq!(res.rows.len(), 2);
    assert_eq!(res.rows[0].symbol, "SH600519");
    assert_eq!(res.rows[1].symbol, "SZ000001");
    assert_eq!(res.total, 2);
}

#[test]
fn delisted_stock_excluded_by_default_and_included_when_disabled() {
    let stocks = vec![
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
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

    let res = run_screener(
        &Filter::from(ScreenerQuery::default()),
        &reader,
        date(2026, 7, 28),
    )
    .expect("run");
    assert_eq!(res.rows.len(), 1, "delisted excluded by default");
    assert_eq!(res.rows[0].symbol, "SZ000001");

    let q = ScreenerQuery {
        exclude_delisted: false,
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 2, "delisted included when disabled");
}

#[test]
fn basics_without_bars_is_excluded() {
    let stocks = vec![
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
        TestStock {
            symbol: "SZ301677",
            name: "C欣兴工具",
            industry: Some("机械"),
            board: Some("创业板"),
            list_date: Some("2025-06-01"),
            delist_date: None,
            total_share: Some(1.0e8),
            bars: Vec::new(), // 无任何 bar
        },
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let res = run_screener(
        &Filter::from(ScreenerQuery::default()),
        &reader,
        date(2026, 7, 28),
    )
    .expect("run");
    assert_eq!(res.rows.len(), 1, "basics-without-bars must be excluded");
    assert_eq!(res.rows[0].symbol, "SZ000001");
    assert_eq!(res.total, 1, "excluded symbols must not count toward total");
}

#[test]
fn industry_filter_or_semantics() {
    let stocks = vec![
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1.0e10),
            bars: daily_series("2026-07-28", &[1500.0; 5], 1000.0),
        },
        TestStock {
            symbol: "SH600000",
            name: "浦发银行",
            industry: Some("银行"),
            board: Some("主板"),
            list_date: Some("1999-11-10"),
            delist_date: None,
            total_share: Some(1.0e10),
            bars: daily_series("2026-07-28", &[8.0; 5], 1000.0),
        },
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        industries: vec!["银行".to_string()],
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 2, "both banks match (OR within industries)");
    for row in &res.rows {
        assert_eq!(row.industry, "银行");
    }
}

#[test]
fn exchange_filter_92_prefix_is_bj() {
    let stocks = vec![
        TestStock {
            symbol: "BJ920992",
            name: "中科美菱",
            industry: Some("医疗"),
            board: Some("北交所"),
            list_date: Some("2023-01-01"),
            delist_date: None,
            total_share: Some(1.0e8),
            bars: daily_series("2026-07-28", &[10.0; 5], 1000.0),
        },
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        exchanges: vec!["BJ".to_string()],
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(
        res.rows[0].symbol, "BJ920992",
        "BJ-prefixed must be classified BJ"
    );
}

#[test]
fn exchange_filter_lowercase_prefix_is_case_insensitive() {
    // parse_explicit_prefix accepts lowercase prefixes; the strategy layer
    // must derive SH from "sh600519" instead of classifying it SZ.
    let stocks = vec![
        TestStock {
            symbol: "sh600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1.0e10),
            bars: daily_series("2026-07-28", &[10.0; 5], 1000.0),
        },
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        exchanges: vec!["SH".to_string()],
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(
        res.rows[0].symbol, "sh600519",
        "lowercase-prefixed symbol must be classified SH"
    );
}

#[test]
fn exchange_filter_bare_code_falls_back_to_shape_heuristic() {
    // Pre-migration bare parquet: 6xxxxx derives SH, matching the GUI's
    // legacy fallback (F2: strategy layer previously dropped it and
    // classified 600519 as SZ, dropping it from the SH filter).
    let stocks = vec![
        TestStock {
            symbol: "600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1.0e10),
            bars: daily_series("2026-07-28", &[10.0; 5], 1000.0),
        },
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        exchanges: vec!["SH".to_string()],
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(
        res.rows[0].symbol, "600519",
        "bare 6xxxxx code must derive SH via the legacy shape heuristic"
    );
}

#[test]
fn list_years_filter_and_missing_list_date_excluded() {
    let stocks = vec![
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1.0e10),
            bars: daily_series("2026-07-28", &[1500.0; 5], 1000.0),
        },
        TestStock {
            symbol: "SZ000001",
            name: "平安银行",
            industry: Some("银行"),
            board: Some("主板"),
            list_date: None,
            delist_date: None,
            total_share: Some(1.0e10),
            bars: daily_series("2026-07-28", &[10.0; 5], 1000.0),
        },
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        list_years: Some(10),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "≥10y passes; NULL list_date excluded");
    assert_eq!(res.rows[0].symbol, "SH600519");
}

#[test]
fn market_cap_filter_uses_yi_units_and_missing_total_share_excluded() {
    let stocks = vec![
        TestStock {
            symbol: "SH600519",
            name: "贵州茅台",
            industry: Some("白酒"),
            board: Some("主板"),
            list_date: Some("2001-08-27"),
            delist_date: None,
            total_share: Some(1_256_197_800.0), // 12.56亿股 × 1500 = 18840亿
            bars: daily_series("2026-07-28", &[1500.0; 5], 1000.0),
        },
        stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0)),
    ];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        market_cap_min: Some(10_000.0),
        market_cap_max: Some(20_000.0),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "only 茅台 cap 18840亿 in range");
    assert_eq!(res.rows[0].symbol, "SH600519");
    assert!(
        (res.rows[0].market_cap - 18_842.97).abs() < 1.0,
        "cap in 亿: {}",
        res.rows[0].market_cap
    );
}

#[test]
fn ma_above_ma20_matches_rising_trend() {
    let stocks = vec![stock_000001(rising_series("2026-07-28", 1000.0))];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        ma: Some(MaCondition::AboveMa20),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "rising stock is above MA20");
}

#[test]
fn ma_above_ma20_rejects_falling_trend() {
    let mut closes = Vec::new();
    for i in 0..40 {
        closes.push(20.0 - i as f64 * 10.0 / 39.0);
    }
    let stocks = vec![stock_000001(daily_series("2026-07-28", &closes, 1000.0))];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        ma: Some(MaCondition::AboveMa20),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert!(res.rows.is_empty(), "falling stock is below MA20");
}

#[test]
fn breakout_requires_strict_new_high() {
    // Latest close equal to previous max must NOT match (strict >).
    let mut closes = vec![10.0; 61];
    closes.push(10.0);
    let stocks = vec![stock_000001(daily_series("2026-07-28", &closes, 1000.0))];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        breakout: Some(BreakoutCondition::new(60)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert!(res.rows.is_empty(), "equality is not a breakout");
}

#[test]
fn breakout_matches_true_new_high() {
    let mut closes = vec![10.0; 61];
    closes.push(11.0);
    let stocks = vec![stock_000001(daily_series("2026-07-28", &closes, 1000.0))];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        breakout: Some(BreakoutCondition::new(60)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "true new high matches");
}

#[test]
fn momentum_filter_bounds() {
    // rising_series: closes rise linearly 10→20 over 40 bars; the 20-day
    // return at the end is ≈ 34.5%.
    let stocks = vec![stock_000001(rising_series("2026-07-28", 1000.0))];
    let (_tmp, reader) = build_fixture(&stocks);

    // Within bounds: [30, 40]
    let q = ScreenerQuery {
        momentum: Some(MomentumCondition::new(20, 30.0, 40.0)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "34.5% return within [30,40]");

    // Out of bounds: [0, 10]
    let q = ScreenerQuery {
        momentum: Some(MomentumCondition::new(20, 0.0, 10.0)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert!(res.rows.is_empty(), "34.5% return not in [0,10]");
}

#[test]
fn volume_filter_matches_surge() {
    // Bars are built newest-first then reversed, so index 0 is the NEWEST.
    // Recent 20 bars volume 5000 (indices 0..20), older 40 bars volume 1000
    // (indices 20..60) → baseline 60-bar average = (20×5000+40×1000)/60
    // ≈ 2333, recent average 5000, ratio ≈ 2.14 ≥ 2.
    let mut series: Vec<TestBar> = Vec::new();
    let mut day = NaiveDate::parse_from_str("2026-07-28", "%Y-%m-%d").expect("parse");
    for i in 0..60 {
        while matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
            day -= Duration::days(1);
        }
        let vol = if i < 20 { 5000.0 } else { 1000.0 };
        series.push(TestBar {
            date: day.format("%Y-%m-%d").to_string(),
            close: 10.0,
            volume: vol,
        });
        day -= Duration::days(1);
    }
    series.reverse();

    let stocks = vec![stock_000001(series)];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        volume: Some(VolumeCondition::new(20, 2.0)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "volume surge matches");
}

#[test]
fn volume_filter_rejects_flat_volume() {
    let stocks = vec![stock_000001(daily_series(
        "2026-07-28",
        &[10.0; 60],
        1000.0,
    ))];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        volume: Some(VolumeCondition::new(20, 2.0)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert!(res.rows.is_empty(), "flat volume ratio ~1 < 2");
}

#[test]
fn window_insufficient_skips_condition_not_crash() {
    let stocks = vec![stock_000001(daily_series(
        "2026-07-28",
        &[10.0; 30],
        1000.0,
    ))];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        ma: Some(MaCondition::AboveMa60),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert!(res.rows.is_empty(), "insufficient window skips condition");
}

#[test]
fn volume_boundary_exactly_3n_bars_computes() {
    let mut series: Vec<TestBar> = Vec::new();
    let mut day = NaiveDate::parse_from_str("2026-07-28", "%Y-%m-%d").expect("parse");
    for _ in 0..60 {
        while matches!(day.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
            day -= Duration::days(1);
        }
        series.push(TestBar {
            date: day.format("%Y-%m-%d").to_string(),
            close: 10.0,
            volume: 3000.0,
        });
        day -= Duration::days(1);
    }
    series.reverse();

    let stocks = vec![stock_000001(series)];
    let (_tmp, reader) = build_fixture(&stocks);

    let q = ScreenerQuery {
        volume: Some(VolumeCondition::new(20, 1.0)),
        ..ScreenerQuery::default()
    };
    let res = run_screener(&Filter::from(q.clone()), &reader, date(2026, 7, 28)).expect("run");
    assert_eq!(res.rows.len(), 1, "exactly 3N bars computes (ratio 1 ≥ 1)");
}

#[test]
fn total_capped_at_100_rows() {
    let mut stocks: Vec<TestStock> = Vec::new();
    for i in 0..120 {
        let symbol = format!("SZ{:06}", 300000 + i).leak();
        stocks.push(TestStock {
            symbol,
            name: "测试股",
            industry: Some("测试"),
            board: Some("创业板"),
            list_date: Some("2020-01-01"),
            delist_date: None,
            total_share: Some(1.0e9),
            bars: daily_series("2026-07-28", &[10.0; 5], 1000.0),
        });
    }
    let (_tmp, reader) = build_fixture(&stocks);

    let res = run_screener(
        &Filter::from(ScreenerQuery::default()),
        &reader,
        date(2026, 7, 28),
    )
    .expect("run");
    assert_eq!(res.rows.len(), MAX_RESULTS, "rows capped at 100");
    assert_eq!(res.total, 120, "total counts all matches before cap");
}

#[test]
fn empty_market_returns_empty_result() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let reader = ParquetReader::new(tmp.path()).expect("create reader");
    let res = run_screener(
        &Filter::from(ScreenerQuery::default()),
        &reader,
        date(2026, 7, 28),
    )
    .expect("run");
    assert!(res.rows.is_empty());
    assert_eq!(res.total, 0);
}

/// Capture tracing output to prove the engine emits its completion log.
#[test]
fn run_screener_emits_completion_log() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    struct TestWriter(Arc<Mutex<String>>);
    impl Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("lock")
                .push_str(&String::from_utf8_lossy(buf));
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TestWriter {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            TestWriter(self.0.clone())
        }
    }

    let stocks = vec![stock_000001(daily_series("2026-07-28", &[10.0; 5], 1000.0))];
    let (_tmp, reader) = build_fixture(&stocks);

    // set_global_default installs the capture buffer as the process-wide
    // dispatcher: run_screener's events reach it via the get_global fast path,
    // immune to thread-local dispatch state. Scoped set_default/with_default
    // were flaky (#138): under parallel test threads the library's debug!
    // occasionally resolved to the thread-local none-dispatch and the buffer
    // stayed empty. Err is ignored: another test may have already installed a
    // global default (capture still works for this test's own events).
    let buf = Arc::new(Mutex::new(String::new()));
    let writer = TestWriter(buf.clone());
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(writer)
            .with_max_level(tracing::Level::DEBUG)
            .finish(),
    );
    run_screener(
        &Filter::from(ScreenerQuery::default()),
        &reader,
        date(2026, 7, 28),
    )
    .expect("run");

    let log = buf.lock().expect("lock");
    assert!(
        log.contains("screener run completed"),
        "engine must emit completion log, got: {log}"
    );
    assert!(log.contains("matched"), "log carries match stats: {log}");
}
