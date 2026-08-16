//! Integration tests for the SEPA five-module scoring engine (epic #139,
//! sub-issue #149).
//!
//! Fixtures build a tempdir parquet dataset (stock_daily + stock_basic +
//! the five SEPA tables) and run [`run_sepa`] against it, mirroring the
//! `screener.rs` pattern. Each table's parquet is written only when the
//! fixture carries rows — an absent table exercises the locked "missing
//! parquet → empty vec" degradation of the readers.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::sepa::run_sepa;
use compass_types::SepaQuery;

/// One daily bar's values; `adjclose == close` (no adjustments in fixtures).
#[derive(Clone)]
struct TestBar {
    date: String,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    amount: f64,
}

/// One fixture stock. `in_basic` controls whether a stock_basic row is
/// written (false = bar-only symbol such as an index code, which the engine
/// must exclude via the basics join).
struct TestStock {
    symbol: &'static str,
    name: &'static str,
    board: &'static str,
    industry: &'static str,
    list_date: &'static str,
    total_share: f64,
    in_basic: bool,
    bars: Vec<TestBar>,
}

/// One capital-main-flow row (capital_main_flow.parquet).
struct TestFlow {
    symbol: &'static str,
    trade_date: &'static str,
    main_net_inflow: f64,
}

/// One dragon-list row (dragon_list.parquet).
struct TestDragon {
    symbol: &'static str,
    trade_date: &'static str,
    net_amount: f64,
}

/// One block-trade row (block_trade.parquet).
struct TestBlock {
    symbol: &'static str,
    trade_date: &'static str,
    premium_rate: f64,
}

/// One institution-survey row (institution_survey.parquet).
struct TestSurvey {
    symbol: &'static str,
    survey_date: &'static str,
}

struct Fixture {
    stocks: Vec<TestStock>,
    flows: Vec<TestFlow>,
    dragons: Vec<TestDragon>,
    blocks: Vec<TestBlock>,
    surveys: Vec<TestSurvey>,
}

impl Fixture {
    fn new(stocks: Vec<TestStock>) -> Self {
        Fixture {
            stocks,
            flows: Vec::new(),
            dragons: Vec::new(),
            blocks: Vec::new(),
            surveys: Vec::new(),
        }
    }

    fn with_flows(mut self, flows: Vec<TestFlow>) -> Self {
        self.flows = flows;
        self
    }

    fn with_dragons(mut self, dragons: Vec<TestDragon>) -> Self {
        self.dragons = dragons;
        self
    }

    fn with_blocks(mut self, blocks: Vec<TestBlock>) -> Self {
        self.blocks = blocks;
        self
    }

    fn with_surveys(mut self, surveys: Vec<TestSurvey>) -> Self {
        self.surveys = surveys;
        self
    }

    /// Write all fixture tables to a tempdir parquet dataset and open a
    /// reader over it. Each table is written only when it has rows.
    fn build(&self) -> (tempfile::TempDir, ParquetReader) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");

        conn.execute_batch(
            "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
        )
        .expect("create daily");
        for s in &self.stocks {
            for b in &s.bars {
                conn.execute(
                    "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    duckdb::params![
                        s.symbol,
                        b.date.as_str(),
                        b.close - 1.0,
                        b.high,
                        b.low,
                        b.close,
                        b.close,
                        b.volume,
                        b.amount
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
        for s in &self.stocks {
            if !s.in_basic {
                continue;
            }
            conn.execute(
                "INSERT INTO basic VALUES (?, ?, ?, NULL, ?, ?, ?, ?, NULL)",
                duckdb::params![
                    s.symbol,
                    s.name,
                    s.list_date,
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

        if !self.flows.is_empty() {
            conn.execute_batch(
                "CREATE TABLE capital_main_flow (symbol VARCHAR, trade_date DATE, main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE, super_large_net DOUBLE, large_net DOUBLE, medium_net DOUBLE, small_net DOUBLE, update_date DATE);",
            )
            .expect("create capital_main_flow");
            for f in &self.flows {
                conn.execute(
                    "INSERT INTO capital_main_flow VALUES (?, ?, ?, 2.0, ?, ?, 0.0, 0.0, ?)",
                    duckdb::params![
                        f.symbol,
                        f.trade_date,
                        f.main_net_inflow,
                        f.main_net_inflow * 0.6,
                        f.main_net_inflow * 0.4,
                        f.trade_date,
                    ],
                )
                .expect("insert capital_main_flow");
            }
            conn.execute_batch(&format!(
                "COPY capital_main_flow TO '{}' (FORMAT PARQUET)",
                tmp.path().join("capital_main_flow.parquet").display()
            ))
            .expect("copy capital_main_flow");
        }

        if !self.dragons.is_empty() {
            conn.execute_batch(
                "CREATE TABLE dragon_list (symbol VARCHAR, trade_date DATE, seat_type VARCHAR, buy_amount DOUBLE, sell_amount DOUBLE, net_amount DOUBLE, institution_flag TINYINT, update_date DATE);",
            )
            .expect("create dragon_list");
            for d in &self.dragons {
                conn.execute(
                    "INSERT INTO dragon_list VALUES (?, ?, '机构专用', ?, 1.0e8, ?, 1, ?)",
                    duckdb::params![
                        d.symbol,
                        d.trade_date,
                        d.net_amount,
                        d.net_amount,
                        d.trade_date
                    ],
                )
                .expect("insert dragon_list");
            }
            conn.execute_batch(&format!(
                "COPY dragon_list TO '{}' (FORMAT PARQUET)",
                tmp.path().join("dragon_list.parquet").display()
            ))
            .expect("copy dragon_list");
        }

        if !self.blocks.is_empty() {
            conn.execute_batch(
                "CREATE TABLE block_trade (symbol VARCHAR, trade_date DATE, price DOUBLE, volume DOUBLE, amount DOUBLE, buyer VARCHAR, seller VARCHAR, premium_rate DOUBLE, update_date DATE);",
            )
            .expect("create block_trade");
            for b in &self.blocks {
                conn.execute(
                    "INSERT INTO block_trade VALUES (?, ?, 10.0, 100000.0, 1.0e6, '中信证券', '机构专用', ?, ?)",
                    duckdb::params![b.symbol, b.trade_date, b.premium_rate, b.trade_date],
                )
                .expect("insert block_trade");
            }
            conn.execute_batch(&format!(
                "COPY block_trade TO '{}' (FORMAT PARQUET)",
                tmp.path().join("block_trade.parquet").display()
            ))
            .expect("copy block_trade");
        }

        if !self.surveys.is_empty() {
            conn.execute_batch(
                "CREATE TABLE institution_survey (symbol VARCHAR, survey_date DATE, org_name VARCHAR, survey_type VARCHAR, update_date DATE);",
            )
            .expect("create institution_survey");
            for s in &self.surveys {
                conn.execute(
                    "INSERT INTO institution_survey VALUES (?, ?, '长信基金', '电话会议', ?)",
                    duckdb::params![s.symbol, s.survey_date, s.survey_date],
                )
                .expect("insert institution_survey");
            }
            conn.execute_batch(&format!(
                "COPY institution_survey TO '{}' (FORMAT PARQUET)",
                tmp.path().join("institution_survey.parquet").display()
            ))
            .expect("copy institution_survey");
        }

        let reader = ParquetReader::new(tmp.path()).expect("create reader");
        (tmp, reader)
    }
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

/// Weekday-only daily bars ending at `end` (inclusive), closes from `closes`,
/// high/low spread `(up, down)` around each close, uniform volume/amount.
fn bars(end: &str, closes: &[f64], up: f64, down: f64, volume: f64, amount: f64) -> Vec<TestBar> {
    let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
    let mut out = Vec::new();
    for close in closes.iter().rev() {
        while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            day -= Duration::days(1);
        }
        out.push(TestBar {
            date: day.format("%Y-%m-%d").to_string(),
            high: close + up,
            low: close - down,
            close: *close,
            volume,
            amount,
        });
        day -= Duration::days(1);
    }
    out.reverse();
    out
}

fn stock(
    symbol: &'static str,
    name: &'static str,
    list_date: &'static str,
    bars: Vec<TestBar>,
) -> TestStock {
    TestStock {
        symbol,
        name,
        board: "主板",
        industry: "测试",
        list_date,
        total_share: 1.0e9,
        in_basic: true,
        bars,
    }
}

fn bar_only_stock(symbol: &'static str, bars: Vec<TestBar>) -> TestStock {
    TestStock {
        symbol,
        name: "指数",
        board: "主板",
        industry: "指数",
        list_date: "2005-01-01",
        total_share: 0.0,
        in_basic: false,
        bars,
    }
}

/// The fixture "strong" stock: 300 bars rising 10 → 22.73 with a final +10%
/// limit-up bar to 25.0, tight ranges (ATR ≈ 1.2% of close → no risk ATR
/// deduction), volume doubling over the last 20 bars (量比 2 → breakout full
/// base, no 放量滞涨), 5 亿 amount (liquidity pass).
fn strong_series() -> Vec<TestBar> {
    let mut closes: Vec<f64> = (0..300).map(|i| 10.0 + i as f64 * 12.73 / 299.0).collect();
    closes[299] = 25.0;
    let mut s = bars("2026-07-31", &closes, 0.1, 0.1, 1.0e6, 9.0e8);
    for b in s.iter_mut().skip(280) {
        b.volume = 2.0e6;
    }
    s
}

/// Second member of the hot industry: same rise but a +7% final bar (keeps
/// the board's 领涨带动 condition satisfied with ≥2 leaders > 5%).
fn strong_series_b() -> Vec<TestBar> {
    let mut closes: Vec<f64> = (0..300).map(|i| 10.0 + i as f64 * 12.73 / 299.0).collect();
    closes[299] = 24.3;
    let mut s = bars("2026-07-31", &closes, 0.1, 0.1, 1.0e6, 9.0e8);
    for b in s.iter_mut().skip(280) {
        b.volume = 2.0e6;
    }
    s
}

/// The fixture "junk" stock: 200 bars falling 25 → 10 with a small +0.2%
/// bounce on the final bar (keeps market breadth at 100% rising). The steep
/// 15% last-20-bar amplitude keeps it out of the 筹码集中 sideways branch.
fn junk_series() -> Vec<TestBar> {
    let mut closes: Vec<f64> = (0..200).map(|i| 25.0 - i as f64 * 15.0 / 199.0).collect();
    closes[199] = 10.10; // above closes[198] ≈ 10.075 → an up day
    let mut s = bars("2026-07-31", &closes, 0.1, 0.1, 1.0e6, 5.0e8);
    s.last_mut().expect("non-empty").volume = 1.0e5; // up day on negligible volume
    s
}

/// Flat filler stock with a +2% final bar (above MA250, modest score).
fn filler_series() -> Vec<TestBar> {
    let mut closes = vec![15.0; 300];
    closes[299] = 15.3;
    bars("2026-07-31", &closes, 0.1, 0.1, 1.0e6, 5.0e8)
}

fn ranking_fixture() -> Fixture {
    // The strong pair shares an industry so the theme module (issue #283 D5)
    // aggregates them into one hot board; the rest stay in separate
    // industries that cannot outrank it.
    let mut strong_a = stock("SZ000001", "平安银行", "2010-01-01", strong_series());
    strong_a.industry = "半导体";
    let mut strong_b = stock("SH600003", "测试科技", "2015-01-01", strong_series_b());
    strong_b.industry = "半导体";
    Fixture::new(vec![
        strong_a,
        strong_b,
        stock("SH600000", "工商银行", "2005-01-01", junk_series()),
        stock("SH600001", "测试甲", "2005-01-01", filler_series()),
        stock("SH600002", "测试乙", "2005-01-01", filler_series()),
    ])
    .with_flows(
        (0..5)
            .map(|k| TestFlow {
                symbol: "SZ000001",
                trade_date: [
                    "2026-07-27",
                    "2026-07-28",
                    "2026-07-29",
                    "2026-07-30",
                    "2026-07-31",
                ][k],
                main_net_inflow: 2.0e7,
            })
            .collect(),
    )
    .with_dragons(vec![TestDragon {
        symbol: "SZ000001",
        trade_date: "2026-07-31",
        net_amount: 4.0e8,
    }])
    .with_blocks(vec![TestBlock {
        symbol: "SZ000001",
        trade_date: "2026-07-31",
        premium_rate: -3.0,
    }])
    .with_surveys(vec![TestSurvey {
        symbol: "SZ000001",
        survey_date: "2026-07-31",
    }])
}

fn default_query() -> SepaQuery {
    SepaQuery { top_n: 50 }
}

/// 评分排序: 强趋势+热门题材+资金流入股 must outrank the junk stock, rows in
/// official descending order, thermometer computed, theme capped at 25 even
/// with the news default of 10, risk 0 for the clean stock.
#[test]
fn strong_stock_outranks_junk_stock() {
    let (_tmp, reader) = ranking_fixture().build();

    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.date, "2026-07-31");
    assert_eq!(data.rows.len(), 5);

    // Official order: strong > industry peer > fillers > junk.
    assert_eq!(data.rows[0].symbol, "SZ000001");
    assert_eq!(data.rows[4].symbol, "SH600000");
    let strong = &data.rows[0];
    let junk = &data.rows[4];
    assert!(
        strong.total_score > junk.total_score + 40.0,
        "strong {} must outrank junk {} by a wide margin",
        strong.total_score,
        junk.total_score
    );
    assert!((strong.trend - 30.0).abs() < 0.01, "trend {}", strong.trend);
    assert!(
        (strong.theme - 25.0).abs() < 1e-9,
        "theme must cap at 25 (news default 10): {}",
        strong.theme
    );
    assert!(
        (strong.capital - 16.0).abs() < 0.01,
        "capital {}",
        strong.capital
    );
    assert!((strong.risk - 0.0).abs() < 1e-9, "risk {}", strong.risk);
    assert!(strong.total_score <= 100.0);
    // Issue #283 D5: every classified stock now has an industry theme score
    // (the "测试" industry's diffusion + news components), so junk's total is
    // small but no longer zero — the strong-margin assertion above is the
    // real contract.
    assert!(
        junk.total_score < strong.total_score - 40.0,
        "junk {} must trail strong {} by a wide margin",
        junk.total_score,
        strong.total_score
    );
    assert_eq!(junk.risk, -1.5, "junk risk = -(30 deductions) × 0.05");

    // Details carry the five modules with locked sub-item maxima.
    assert_eq!(
        strong.details.trend.iter().map(|f| f.max).sum::<f64>(),
        100.0
    );
    assert_eq!(strong.details.theme[3].max, 20.0, "news max 20");
    assert_eq!(strong.details.theme[3].score, 10.0, "news default 10");
    let breakout = &strong.details.pattern[1];
    assert!(
        (breakout.score - 2.5).abs() < 1e-9,
        "breakout = 5 base × 0.5 (40-60 band): {}",
        breakout.score
    );
    let big_capital = &strong.details.capital[2];
    assert_eq!(big_capital.score, 30.0, "big-capital capped at 30");

    // Thermometer: 5-stock market ≈ 40.2 → middle-low band, 40-60.
    let tm = &data.thermometer;
    assert!(
        (40.0..60.0).contains(&tm.score),
        "thermometer score {}",
        tm.score
    );
    assert_eq!(tm.indicators.len(), 5);
}

/// 风险方向: a stock triggering every deduction (ATR>5%, 120-day drawdown
/// of >30%, a 20-day surge of >30% on 量比 >3) lands at exactly −3.75 —
/// never −5 — while the clean stock stays at 0.
#[test]
fn risk_deductions_floor_at_minus_3_75() {
    // 300 bars: rise 10→30 (bars 0..200), crash 30→10 (200..220), flat 10
    // (220..280), surge 10→13.5 over the last 20 bars on 4× volume. Wide
    // 1.5/1.5 ranges → ATR ≈ 22% of close. The 30 peak sits inside the
    // 120-day window → drawdown 55%.
    let mut closes: Vec<f64> = Vec::new();
    closes.extend((0..200).map(|i| 10.0 + i as f64 * 0.1));
    closes.extend((0..20).map(|i| 30.0 - i as f64));
    closes.extend((0..60).map(|_| 10.0));
    closes.extend((1..=20).map(|j| 10.0 + j as f64 * 3.5 / 20.0));
    let mut risky = bars("2026-07-31", &closes, 1.5, 1.5, 1.0e6, 5.0e8);
    for b in risky.iter_mut().skip(280) {
        b.volume = 4.0e6;
    }

    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", strong_series()),
        stock("SH600004", "高危测试", "2008-01-01", risky),
    ]);
    let (_tmp, reader) = fixture.build();

    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 2);
    let strong = data
        .rows
        .iter()
        .find(|r| r.symbol == "SZ000001")
        .expect("strong");
    let risky = data
        .rows
        .iter()
        .find(|r| r.symbol == "SH600004")
        .expect("risky");
    assert_eq!(strong.risk, 0.0, "clean stock has zero risk contribution");
    assert!(
        (risky.risk - -3.75).abs() < 1e-9,
        "all-deduction stock must hit exactly -3.75, got {}",
        risky.risk
    );
    assert!(risky.risk >= -3.75, "risk must never go below -3.75");
    assert_eq!(risky.details.risk.len(), 3);
    assert_eq!(risky.details.risk[0].score, -20.0, "ATR deduction");
    assert_eq!(risky.details.risk[1].score, -30.0, "drawdown deduction");
    assert_eq!(risky.details.risk[2].score, -25.0, "surge deduction");
}

/// A stock whose name contains "ST" is hard-filtered.
#[test]
fn filter_st_name() {
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock("SH600010", "ST中安", "2000-01-01", filler_series()),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0].symbol, "SZ000001");
}

/// A stock whose name contains "退" is hard-filtered.
#[test]
fn filter_delisting_name() {
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock("SH600011", "国华退", "2000-01-01", filler_series()),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0].symbol, "SZ000001");
}

/// A stock listed within the last 90 calendar days (~60 trading days) is
/// hard-filtered as 次新.
#[test]
fn filter_short_listing() {
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock("SH601001", "次新股", "2026-07-01", filler_series()),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0].symbol, "SZ000001");
}

/// A stock whose 20-day average amount is below 3000 万 is hard-filtered.
#[test]
fn filter_low_liquidity() {
    let mut low_amount = filler_series();
    for b in low_amount.iter_mut() {
        b.amount = 1.0e6; // avg 100 万 ≪ 3000 万
    }
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock("SH601002", "冷门股", "2000-01-01", low_amount),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0].symbol, "SZ000001");
}

/// A stock with no bar in the last ~5 trading days (last bar 11 calendar days
/// before the market's latest bar) is hard-filtered as suspended.
#[test]
fn filter_suspended() {
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock(
            "SH601003",
            "停牌股",
            "2000-01-01",
            bars("2026-07-20", &[10.0; 60], 0.1, 0.1, 1.0e6, 5.0e8),
        ),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0].symbol, "SZ000001");
}

/// A BJ (北交所) stock is hard-filtered via its exchange prefix.
#[test]
fn filter_bj_exchange() {
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock("BJ920001", "北交测试", "2000-01-01", filler_series()),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    assert_eq!(data.rows[0].symbol, "SZ000001");
}

/// An index code (SH000905) with bars but no stock_basic row is excluded by
/// the basics join, while the same numeric code as a stock (SZ000905, has a
/// basics row) is scored (issue #181: SH/SZ same-code collision).
#[test]
fn index_row_without_basics_is_excluded_from_results() {
    let fixture = Fixture::new(vec![
        stock("SZ000905", "厦门港务", "2010-01-01", filler_series()),
        bar_only_stock("SH000905", filler_series()),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert!(
        data.rows.iter().any(|r| r.symbol == "SZ000905"),
        "stock with basics row must be scored"
    );
    assert!(
        !data.rows.iter().any(|r| r.symbol == "SH000905"),
        "index row without basics must not leak into results"
    );
}

/// Every hard filter fires at once → empty rows, no crash.
#[test]
fn all_filtered_returns_empty_rows() {
    let mut low_amount = filler_series();
    for b in low_amount.iter_mut() {
        b.amount = 1.0e6;
    }
    let fixture = Fixture::new(vec![stock("SH600010", "ST冷门", "2026-07-01", low_amount)]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert!(data.rows.is_empty());
    assert_eq!(data.date, "2026-07-31");
}

/// A bare tempdir (no parquet at all) degrades to an empty result set: every
/// reader returns an empty vec, the thermometer scores 0, no panic.
#[test]
fn empty_market_returns_empty_sepa_data() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let reader = ParquetReader::new(tmp.path()).expect("create reader");
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert!(data.rows.is_empty());
    assert_eq!(data.thermometer.score, 0.0);
    assert_eq!(data.thermometer.position_key, "sepa.position.low");
    assert_eq!(data.date, "2026-07-31");
}

/// Boundary: a stock with an insufficient window (5 bars) and a stock with a
/// zero adjusted/raw close inside its history must both be scored without a
/// panic, and every returned score must be finite. Missing SEPA tables
/// (concept/flow/…) degrade to empty vecs in the same run.
#[test]
fn short_window_and_zero_close_do_not_panic() {
    let mut broken: Vec<f64> = (0..100).map(|i| 10.0 + i as f64 * 0.1).collect();
    broken[79] = 0.0; // zero base for the 20-day momentum window
    let fixture = Fixture::new(vec![
        stock("SZ000001", "平安银行", "2010-01-01", filler_series()),
        stock(
            "SH600020",
            "短窗股",
            "2020-01-01",
            bars("2026-07-31", &[10.0; 5], 0.1, 0.1, 1.0e6, 5.0e8),
        ),
        stock(
            "SH600021",
            "坏数据股",
            "2020-01-01",
            bars("2026-07-31", &broken, 0.1, 0.1, 1.0e6, 5.0e8),
        ),
    ]);
    let (_tmp, reader) = fixture.build();
    let data = run_sepa(&default_query(), &reader, date(2026, 7, 31)).expect("run sepa");
    assert_eq!(
        data.rows.len(),
        3,
        "none of the three stocks hits a hard filter"
    );
    for row in &data.rows {
        assert!(
            row.total_score.is_finite(),
            "{} total must be finite",
            row.symbol
        );
        assert!(
            (0.0..=100.0).contains(&row.total_score),
            "{} total {} out of range",
            row.symbol,
            row.total_score
        );
    }
    let short = data
        .rows
        .iter()
        .find(|r| r.symbol == "SH600020")
        .expect("short");
    assert!(
        short.total_score < 30.0,
        "5-bar window scores low: {}",
        short.total_score
    );
}
