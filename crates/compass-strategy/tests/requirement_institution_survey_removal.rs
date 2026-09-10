//! 需求验收测试（issue #360）—— SEPA capital 模块移除 institution_survey
//! 信号，dragon 机构买入加分 +10 → +15，cap 30 不变。
//!
//! 契约（`.dsh/plans/remove-institution-survey.md` 验收标准）：
//! - `score_capital` 无 survey 输入：dragon 机构净买入（institution_flag=1
//!   且 net_amount>0）贡献 `15.0`（当前 10.0 → RED）；
//! - 只有调研行、无机构 dragon 时 capital 分数必须为 `0.0`（当前 +5.0 → RED）；
//! - `big_capital` cap 30 保持不变（当前/实现后均 30 → 守卫）；
//! - 完全无 institution_survey.parquet 时 `run_sepa` 正常返回（守卫）。
//!
//! 自包含约束（RED→GREEN 无需修改）：
//! - 仅走公开 API `compass_strategy::sepa::run_sepa` +
//!   `compass_core::data::parquet::ParquetReader`；
//! - 不引用将被删除的类型/方法：`InstitutionSurveyRow`、
//!   `fetch_institution_survey`、`TestSurvey`、`with_surveys`、
//!   `SepaWindow.surveys`；
//! - institution_survey.parquet 经 DuckDB 直建（表名/列名是纯数据字符串，
//!   实现移除后该文件只是不再被读取，测试依旧编译、通过）。
//!
//! 数学（沿袭 `tests/sepa.rs` 既有 fixture，对抗性套件已独立验证）：
//! 30 根 close=10.0 平价日线 → volume_price=0 / chip=0 →
//! capital = CAPITAL_WEIGHT × big_capital；main_flow 单值 →
//! rank_percentile=1.0 → 20.0；block premium_rate < -2 → +5.0。
//!
//! 可制造性：全部场景由公开 API + tempdir parquet 构造，无内部注入需求。

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::sepa::run_sepa;
use compass_types::SepaQuery;
use tempfile::TempDir;

const NOW: &str = "2026-07-31";
const CAPITAL_WEIGHT: f64 = 0.20;

/// One fixture stock: `bar_count` flat weekday bars ending at `NOW`.
#[derive(Clone)]
struct StockSpec {
    symbol: &'static str,
    name: &'static str,
    bar_count: usize,
    close: f64,
    volume: f64,
    amount: f64,
}

impl StockSpec {
    /// 30 flat bars at 10.0, 5 亿 amount (liquidity pass), listed 2010.
    /// 30 bars → up_day_volume_ratio = 0, chip_compliance = 0 →
    /// capital = 0.20 × big_capital exactly.
    fn flat30(symbol: &'static str, name: &'static str) -> Self {
        StockSpec {
            symbol,
            name,
            bar_count: 30,
            close: 10.0,
            volume: 1.0e6,
            amount: 5.0e8,
        }
    }
}

/// One capital_main_flow row. Single value → rank_percentile = 1.0 → 20.0.
#[derive(Clone)]
struct FlowRow {
    symbol: &'static str,
    trade_date: &'static str,
    main_net_inflow: f64,
}

/// One dragon_list row. `institution_flag: Some(1)` = 机构专用 seat (TINYINT 1).
#[derive(Clone)]
struct DragonRow {
    symbol: &'static str,
    trade_date: &'static str,
    net_amount: f64,
    institution_flag: Option<i8>,
}

/// One block_trade row; `premium_rate < -2.0` → discount → +5.0 adjustment.
#[derive(Clone)]
struct BlockRow {
    symbol: &'static str,
    trade_date: &'static str,
    premium_rate: f64,
}

/// One institution_survey row (built via DuckDB only; never via core types).
#[derive(Clone)]
struct SurveyRow {
    symbol: &'static str,
    survey_date: &'static str,
    org_name: &'static str,
    survey_type: &'static str,
}

#[derive(Clone, Default)]
struct Seed {
    stocks: Vec<StockSpec>,
    flows: Vec<FlowRow>,
    dragons: Vec<DragonRow>,
    blocks: Vec<BlockRow>,
    surveys: Vec<SurveyRow>,
}

/// Weekday-only date strings, `n` days, ending at `end` (inclusive).
fn weekdays_before(end: &str, n: usize) -> Vec<String> {
    let mut day = NaiveDate::parse_from_str(end, "%Y-%m-%d").expect("parse end");
    let mut out = Vec::new();
    for _ in 0..n {
        while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            day -= Duration::days(1);
        }
        out.push(day.format("%Y-%m-%d").to_string());
        day -= Duration::days(1);
    }
    out.reverse();
    out
}

/// Build a tempdir parquet dataset from `seed` and open a `ParquetReader`.
fn build(seed: &Seed) -> (TempDir, ParquetReader) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let conn = duckdb::Connection::open_in_memory().expect("duckdb");

    // stock_daily.parquet：平价日线（flat30）
    conn.execute_batch(
        "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
    )
    .expect("create daily");
    for s in &seed.stocks {
        for date in weekdays_before(NOW, s.bar_count) {
            conn.execute(
                "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    s.symbol,
                    date.as_str(),
                    s.close - 1.0,
                    s.close + 0.1,
                    s.close - 0.1,
                    s.close,
                    s.close,
                    s.volume,
                    s.amount
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

    // stock_basic.parquet（industry_en 列缺失 → reader 走 legacy 分支）
    conn.execute_batch(
        "CREATE TABLE basic (symbol VARCHAR, name VARCHAR, list_date DATE, delist_date DATE, board VARCHAR, full_name VARCHAR, total_share DOUBLE, industry VARCHAR, region VARCHAR);",
    )
    .expect("create basic");
    for s in &seed.stocks {
        conn.execute(
            "INSERT INTO basic VALUES (?, ?, ?, NULL, ?, ?, ?, ?, NULL)",
            duckdb::params![
                s.symbol,
                s.name,
                "2010-01-01",
                "主板",
                s.name,
                1.0e9,
                "测试",
            ],
        )
        .expect("insert basic");
    }
    conn.execute_batch(&format!(
        "COPY basic TO '{}' (FORMAT PARQUET)",
        tmp.path().join("stock_basic.parquet").display()
    ))
    .expect("copy basic");

    // capital_main_flow.parquet
    if !seed.flows.is_empty() {
        conn.execute_batch(
            "CREATE TABLE capital_main_flow (symbol VARCHAR, trade_date DATE, main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE, super_large_net DOUBLE, large_net DOUBLE, medium_net DOUBLE, small_net DOUBLE, update_date DATE);",
        )
        .expect("create capital_main_flow");
        for f in &seed.flows {
            conn.execute(
                "INSERT INTO capital_main_flow VALUES (?, ?, ?, 2.0, 1.2e7, 8.0e6, 0.0, 0.0, ?)",
                duckdb::params![f.symbol, f.trade_date, f.main_net_inflow, f.trade_date],
            )
            .expect("insert capital_main_flow");
        }
        conn.execute_batch(&format!(
            "COPY capital_main_flow TO '{}' (FORMAT PARQUET)",
            tmp.path().join("capital_main_flow.parquet").display()
        ))
        .expect("copy capital_main_flow");
    }

    // dragon_list.parquet（institution_flag 直写 TINYINT；可选 NULL）
    if !seed.dragons.is_empty() {
        conn.execute_batch(
            "CREATE TABLE dragon_list (symbol VARCHAR, trade_date DATE, seat_type VARCHAR, buy_amount DOUBLE, sell_amount DOUBLE, net_amount DOUBLE, institution_flag TINYINT, update_date DATE);",
        )
        .expect("create dragon_list");
        for d in &seed.dragons {
            match d.institution_flag {
                Some(flag) => {
                    conn.execute(
                        "INSERT INTO dragon_list VALUES (?, ?, '机构专用', ?, 1.0e8, ?, ?, ?)",
                        duckdb::params![
                            d.symbol,
                            d.trade_date,
                            d.net_amount,
                            d.net_amount,
                            flag,
                            d.trade_date
                        ],
                    )
                    .expect("insert dragon_list");
                }
                None => {
                    conn.execute(
                        "INSERT INTO dragon_list VALUES (?, ?, '机构专用', ?, 1.0e8, ?, NULL, ?)",
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
            }
        }
        conn.execute_batch(&format!(
            "COPY dragon_list TO '{}' (FORMAT PARQUET)",
            tmp.path().join("dragon_list.parquet").display()
        ))
        .expect("copy dragon_list");
    }

    // block_trade.parquet
    if !seed.blocks.is_empty() {
        conn.execute_batch(
            "CREATE TABLE block_trade (symbol VARCHAR, trade_date DATE, price DOUBLE, volume DOUBLE, amount DOUBLE, buyer VARCHAR, seller VARCHAR, premium_rate DOUBLE, update_date DATE);",
        )
        .expect("create block_trade");
        for b in &seed.blocks {
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

    // institution_survey.parquet：DuckDB 直建（纯数据；实现移除后不再被读取）
    if !seed.surveys.is_empty() {
        conn.execute_batch(
            "CREATE TABLE institution_survey (symbol VARCHAR, survey_date DATE, org_name VARCHAR, survey_type VARCHAR, update_date DATE);",
        )
        .expect("create institution_survey");
        for s in &seed.surveys {
            conn.execute(
                "INSERT INTO institution_survey VALUES (?, ?, ?, ?, ?)",
                duckdb::params![
                    s.symbol,
                    s.survey_date,
                    s.org_name,
                    s.survey_type,
                    s.survey_date
                ],
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

fn query() -> SepaQuery {
    SepaQuery { top_n: 50 }
}

fn now() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 31).expect("valid date")
}

/// 单一 flat30 股票种子的缺省种子。
fn seed(symbol: &'static str, name: &'static str) -> Seed {
    Seed {
        stocks: vec![StockSpec::flat30(symbol, name)],
        ..Seed::default()
    }
}

/// RED-1（happy path）：机构净买入 dragon + 无 institution_survey.parquet →
/// big_capital_inflow 分数必须为 15.0（当前 10.0 → RED）。
///
/// fixture：SZ000001 机构净买入（flag=1, net=+4e8）、无 flow/block/survey 文件。
/// big_capital = (0 + dragon + 0).clamp(0,30)；capital = 0.20 × big_capital。
///
/// 可制造性：公开 API + tempdir parquet 构造，无内部注入。
#[test]
fn dragon_institution_buy_scores_15_without_survey() {
    let seed = Seed {
        dragons: vec![DragonRow {
            symbol: "SZ000001",
            trade_date: NOW,
            net_amount: 4.0e8,
            institution_flag: Some(1),
        }],
        ..seed("SZ000001", "平安银行")
    };
    let (_tmp, reader) = build(&seed);

    let data = run_sepa(&query(), &reader, now()).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    let row = &data.rows[0];

    // 断言目标值 vs 当前值：score 15.0（当前 10.0 → RED）
    assert_eq!(
        row.details.capital[2].score, 15.0,
        "机构净买入 dragon 加分必须为 15.0（旧 survey+5 重分配后）"
    );
    // capital = 0.20 × 15.0 = 3.0（当前 0.20 × 10.0 = 2.0 → RED）
    assert!(
        (row.capital - CAPITAL_WEIGHT * 15.0).abs() < 1e-9,
        "capital module = 0.20 × 15.0，实际 {}",
        row.capital
    );
}

/// RED-2（基本错误路径变体）：无机构 dragon、但 institution_survey.parquet
/// 含目标行时，capital 分数必须为 0.0（当前 +5.0 → RED）。
///
/// fixture：SZ000001 仅有两行调研记录（2026-07-31 / 2026-07-15），
/// 无 dragon/flow/block 文件。
///
/// 可制造性：公开 API + tempdir parquet 构造，无内部注入。
#[test]
fn survey_rows_do_not_score_without_dragon() {
    let seed = Seed {
        surveys: vec![
            SurveyRow {
                symbol: "SZ000001",
                survey_date: NOW,
                org_name: "长信基金",
                survey_type: "电话会议",
            },
            SurveyRow {
                symbol: "SZ000001",
                survey_date: "2026-07-15",
                org_name: "国泰基金",
                survey_type: "现场调研",
            },
        ],
        ..seed("SZ000001", "平安银行")
    };
    let (_tmp, reader) = build(&seed);

    let data = run_sepa(&query(), &reader, now()).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    let row = &data.rows[0];

    // 断言目标值 vs 当前值：score 0.0（当前 5.0 → RED）
    assert_eq!(
        row.details.capital[2].score, 0.0,
        "无机构 dragon 时调研行不得贡献任何加分（旧 +5 已移除）"
    );
    // capital = 0.0（当前 0.20 × 5.0 = 1.0 → RED）
    assert!(
        (row.capital - 0.0).abs() < 1e-9,
        "capital module 必须为 0，实际 {}",
        row.capital
    );
}

/// 守卫（cap 不变）：main 20 + dragon 15 + block ±5 → clamp 后恰好 30.0。
///
/// fixture：SZ000001 五日内主力净流入唯一（rank_percentile=1.0 → 20.0）、
/// 机构净买入 dragon、大宗折价 -3% → +5.0；当前 20+10+5=35→30，
/// 实现后 20+15+5=40→30，均为 30 → 守卫。
///
/// 可制造性：公开 API + tempdir parquet 构造，无内部注入。
#[test]
fn big_capital_cap_stays_30() {
    let seed = Seed {
        flows: vec![FlowRow {
            symbol: "SZ000001",
            trade_date: NOW,
            main_net_inflow: 2.0e7,
        }],
        dragons: vec![DragonRow {
            symbol: "SZ000001",
            trade_date: NOW,
            net_amount: 4.0e8,
            institution_flag: Some(1),
        }],
        blocks: vec![BlockRow {
            symbol: "SZ000001",
            trade_date: NOW,
            premium_rate: -3.0,
        }],
        ..seed("SZ000001", "平安银行")
    };
    let (_tmp, reader) = build(&seed);

    let data = run_sepa(&query(), &reader, now()).expect("run sepa");
    assert_eq!(data.rows.len(), 1);
    let row = &data.rows[0];

    // 断言目标值 vs 当前值：score 30.0（当前 30.0，守卫）
    assert_eq!(
        row.details.capital[2].score, 30.0,
        "main 20 + dragon + block ±5 必须被 clamp 到恰好 30.0，不得超限"
    );
    assert!(
        (row.capital - CAPITAL_WEIGHT * 30.0).abs() < 1e-9,
        "capital module = 0.20 × 30.0，实际 {}",
        row.capital
    );
}

/// 守卫（降级）：完全无 institution_survey.parquet 时 run_sepa 正常返回。
///
/// fixture：仅单一 flat30 股票，无任何附加表文件。
///
/// 可制造性：公开 API + tempdir parquet 构造，无内部注入。
#[test]
fn run_sepa_succeeds_without_survey_parquet() {
    let seed = seed("SZ000001", "平安银行");
    let (_tmp, reader) = build(&seed);

    // 断言目标值 vs 当前值：Ok 且含目标行（当前与实现后均成立，守卫）
    let data = run_sepa(&query(), &reader, now()).expect("run sepa: 无 survey 文件必须正常返回");
    assert_eq!(data.rows.len(), 1, "无 survey 文件时仍应正常产出该股票行");
}
