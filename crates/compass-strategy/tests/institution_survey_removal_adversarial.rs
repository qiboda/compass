//! 对抗性集成测试（RED 阶段）—— issue #360：移除 institution_survey 表并
//! 重分配 SEPA 资本模块评分。
//!
//! Plan 契约（`.dsh/plans/remove-institution-survey.md`）：
//! - `score_capital` 删除 survey 信号（旧 has_survey → +5）；
//! - dragon 机构净买入加分 `10.0` → `15.0`；
//! - `big_capital` 的 cap 30 保持不变；无其他公式变更；
//! - `note_args` 从 `vec![main_flow, dragon, survey, block_adj]`（4 元素）
//!   变为 `vec![main_flow, dragon, block_adj]`（3 元素，GUI 注记模板同步改）。
//!
//! 攻击维度（六）：
//! 1. 边界/分值重分配 —— 机构净买入 dragon 的加分必须为 15（RED：现为 10）；
//! 2. survey 残余不得计分 —— 只有调研行、无机构买入 → 0（RED：现为 5）；
//! 3. cap 30 不变量 —— main_flow 20 + dragon + block ±，clamp 后恰好 30（守卫）；
//! 4. 缺失 parquet 降级 —— 无 institution_survey.parquet 正常返回（守卫）；
//! 5. 无 dragon 不贡献 —— 非机构/非净买入/零净额 dragon 行均不加分（守卫）；
//! 6. 混合场景组合 —— dragon 买入 + survey 文件以 不建/空/多行 三态存在，
//!    结果必须一致且与 [1] 的 15.0 相同（RED：当前不建/空文件=10、多行=15）。
//!
//! 自包含约束（RED→GREEN 无需修改）：
//! - 只走公开 API `compass_strategy::sepa::run_sepa` +
//!   `compass_core::data::parquet::ParquetReader`（Cargo 自动发现本文件，
//!   无需改 Cargo.toml）；
//! - 不引用将被删除的类型/方法：`TestSurvey`、`with_surveys`、
//!   `InstitutionSurveyRow`、`fetch_institution_survey`、`SepaWindow.surveys`；
//! - institution_survey.parquet 全部经 DuckDB 直建（表名/列名是纯数据字符串，
//!   实现移除后该文件只是不再被读取，测试依旧编译、通过）。
//!
//! Fixture 模式沿用 `tests/sepa.rs`：内存 DuckDB → COPY PARQUET →
//! `ParquetReader`；adjclose == close；日期切片窗口为
//! [2026-07-31 − SEPA_WINDOW_DAYS(550), 2026-07-31]。

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use compass_core::data::parquet::ParquetReader;
use compass_strategy::sepa::run_sepa;
use compass_types::SepaQuery;
use tempfile::TempDir;

const NOW: &str = "2026-07-31";
const CAPITAL_WEIGHT: f64 = 0.20;

/// One flat bar (close constant over the whole series).
struct Bar {
    date: String,
    close: f64,
    volume: f64,
    amount: f64,
}

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
    /// 30 bars → `up_day_volume_ratio` = 0 (all deltas zero → Some(0.0)),
    /// `chip_compliance` = 0 (< 61 bars) → capital = 0.20 × big_capital exactly.
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

/// One capital_main_flow row. `main_net_inflow` 2.0e7 → 5-day cumulative 2.0e7.
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
    /// Write an empty (0-row, schema-only) institution_survey.parquet.
    empty_survey_file: bool,
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

fn flat_bars(spec: &StockSpec) -> Vec<Bar> {
    weekdays_before(NOW, spec.bar_count)
        .into_iter()
        .map(|date| Bar {
            date,
            close: spec.close,
            volume: spec.volume,
            amount: spec.amount,
        })
        .collect()
}

/// Build a tempdir parquet dataset from `seed` and open a `ParquetReader`.
///
/// 可制造性：全部场景均通过公开 API `run_sepa` + `ParquetReader` 构造，
/// 表格文件由本函数用 DuckDB 写入 tempdir；无任何内部注入需求。
fn build(seed: &Seed) -> (TempDir, ParquetReader) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let conn = duckdb::Connection::open_in_memory().expect("duckdb");

    // stock_daily.parquet
    conn.execute_batch(
        "CREATE TABLE daily (symbol VARCHAR, tradedate DATE, open DOUBLE, high DOUBLE, low DOUBLE, close DOUBLE, adjclose DOUBLE, volume DOUBLE, amount DOUBLE);",
    )
    .expect("create daily");
    for s in &seed.stocks {
        for b in flat_bars(s) {
            conn.execute(
                "INSERT INTO daily VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                duckdb::params![
                    s.symbol,
                    b.date.as_str(),
                    b.close - 1.0,
                    b.close + 0.1,
                    b.close - 0.1,
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

    // stock_basic.parquet (industry_en 列缺失 → reader 走 legacy 分支)
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

    // institution_survey.parquet：仅当有行或要求空文件时写出
    if !seed.surveys.is_empty() || seed.empty_survey_file {
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

/// 机构净买入 dragon（flag=1、net>0）：唯一触发 +15 的输入。
fn buy_dragon(symbol: &'static str) -> DragonRow {
    DragonRow {
        symbol,
        trade_date: NOW,
        net_amount: 4.0e8,
        institution_flag: Some(1),
    }
}

/// 单一 flat30 股票 + 可选附加表的种子工厂，避免逐测试手写。
fn seed(symbol: &'static str, name: &'static str) -> Seed {
    Seed {
        stocks: vec![StockSpec::flat30(symbol, name)],
        ..Seed::default()
    }
}

/// 维度 1：边界/分值重分配 —— 机构净买入 dragon 的加分必须从 10 提升到 15。
///
/// fixture：SZ000001 机构净买入（flag=1, net=+4e8）、无 flow/block/survey、
/// 无 capital_main_flow.parquet → main_flow=0、block_adj=0（map 缺省 0.0）。
/// big_capital = (0 + dragon + 0).clamp(0,30)；capital = 0.20 × big_capital
/// （flat30 系列 volume_price=0、chip=0）。
///
/// 断言目标值 vs 当前值：
/// - details.capital[2].score == 15.0（当前 10.0 → RED）
/// - rows[0].capital == 3.0（当前 2.0 → RED）
/// - note_args == [0.0, 15.0, 0.0]（当前 [0.0, 10.0, 0.0, 0.0] → RED）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
#[test]
fn institution_dragon_bonus_raised_from_10_to_15() {
    let seed = Seed {
        dragons: vec![buy_dragon("SZ000001")],
        ..seed("SZ000001", "平安银行")
    };
    let (_tmp, reader) = build(&seed);

    let data = run_sepa(&query(), &reader, now()).expect("run sepa");
    assert_eq!(data.date, NOW);
    assert_eq!(data.rows.len(), 1);
    let row = &data.rows[0];
    let big_cap = &row.details.capital[2];

    assert_eq!(
        big_cap.score, 15.0,
        "机构净买入 dragon 加分必须为 15.0（旧 survey+5 重分配后）"
    );
    assert!(
        (row.capital - CAPITAL_WEIGHT * 15.0).abs() < 1e-9,
        "capital module = 0.20 × big_capital: {}",
        row.capital
    );
    assert_eq!(
        big_cap.note_args,
        Some(vec![0.0, 15.0, 0.0]),
        "注记参数须为 [main_flow, dragon, block_adj] 三元素（GUI 模板同步改）"
    );
}

/// 维度 2：survey 残余不得计分 —— 无机构 dragon、但 institution_survey.parquet
/// 含目标行时，资本模块必须为 0（旧实现会给 +5，是本次移除的目标毒点）。
///
/// fixture：SZ000001 仅有两行调研记录（2026-07-31 / 2026-07-15），无 dragon/
/// flow/block 文件。
///
/// 断言目标值 vs 当前值：
/// - details.capital[2].score == 0.0（当前 5.0 → RED）
/// - rows[0].capital == 0.0（当前 1.0 → RED）
/// - note_args == [0.0, 0.0, 0.0]（当前 [0.0, 0.0, 5.0, 0.0] → RED）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
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
    let big_cap = &row.details.capital[2];

    assert_eq!(
        big_cap.score, 0.0,
        "无机构 dragon 时调研行不得贡献任何加分（旧 +5 已移除）"
    );
    assert!(
        (row.capital - 0.0).abs() < 1e-9,
        "capital module 必须为 0: {}",
        row.capital
    );
    assert_eq!(
        big_cap.note_args,
        Some(vec![0.0, 0.0, 0.0]),
        "注记参数不得残留 survey 项"
    );
}

/// 维度 3：cap 30 不变量 —— main_flow 20 + dragon 15 + block ±5 → clamp 后恰好 30，
/// 不超 30。当前与实现后均为 30 → 守卫断言。
///
/// fixture：SZ000001 五日内主力净流入唯一（rank_percentile 单值=1.0 → 20.0）、
/// 机构净买入 dragon、大宗折价 -3% → +5.0；当前 20+10+5=35→30，
/// 实现后 20+15+5=40→30。
///
/// 断言目标值 vs 当前值：
/// - details.capital[2].score == 30.0（当前 30.0，守卫）
/// - rows[0].capital == 6.0（当前 6.0，守卫）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
#[test]
fn big_capital_stays_capped_at_30() {
    let seed = Seed {
        flows: vec![FlowRow {
            symbol: "SZ000001",
            trade_date: NOW,
            main_net_inflow: 2.0e7,
        }],
        dragons: vec![buy_dragon("SZ000001")],
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
    let big_cap = &row.details.capital[2];

    assert_eq!(
        big_cap.score, 30.0,
        "20(main) + dragon + 5(block 折价) 必须被 clamp 到恰好 30.0，不得超限"
    );
    assert!(
        (row.capital - CAPITAL_WEIGHT * 30.0).abs() < 1e-9,
        "capital module = 0.20 × 30.0: {}",
        row.capital
    );
}

/// 维度 4：缺失 parquet 降级 —— 完全无 institution_survey.parquet 时
/// run_sepa 正常返回，且与「空 schema 文件」结果逐字段一致（守卫）。
///
/// fixture：与维度 3 相同（cap 命中，能区分是否错误列数/异常）。
///
/// 断言目标值 vs 当前值：
/// - 两次运行均 Ok、rows 同长、SepaData 全等（当前/实现后均成立，守卫）
/// - rows[0].details.capital[2].score == 30.0（守卫）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
#[test]
fn missing_survey_parquet_degrades_gracefully() {
    let base = Seed {
        flows: vec![FlowRow {
            symbol: "SZ000001",
            trade_date: NOW,
            main_net_inflow: 2.0e7,
        }],
        dragons: vec![buy_dragon("SZ000001")],
        blocks: vec![BlockRow {
            symbol: "SZ000001",
            trade_date: NOW,
            premium_rate: -3.0,
        }],
        ..seed("SZ000001", "平安银行")
    };
    let absent = Seed {
        empty_survey_file: false,
        ..base.clone()
    };
    let empty_file = Seed {
        empty_survey_file: true,
        ..absent.clone()
    };

    let (_t1, r1) = build(&absent);
    let data_absent = run_sepa(&query(), &r1, now()).expect("run sepa: 无 survey 文件");
    let (_t2, r2) = build(&empty_file);
    let data_empty = run_sepa(&query(), &r2, now()).expect("run sepa: 空 schema 文件");

    assert_eq!(data_absent, data_empty, "缺失/空文件不得影响任何得分");
    assert_eq!(
        data_absent.rows[0].details.capital[2].score, 30.0,
        "cap 在缺失降级路径下保持 30"
    );
}

/// 维度 5：无 dragon 不贡献 —— 三类「非净买入」dragon 行均不得加分
/// （flag=1 但净流出 / flag=0 但净流入 / flag=NULL / 净额恰为 0）：
/// 这是分值重分配后最容易引入的隐性 bug（条件放宽成只看 flag）。
///
/// fixture：4 只 flat30 股票，各自带一条非买入 dragon 行，无 flow/block/survey。
///
/// 断言目标值 vs 当前值：
/// - 四只股票 details.capital[2].score == 0.0（当前均为 0.0，守卫）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
#[test]
fn non_buy_dragon_rows_do_not_contribute() {
    let seed = Seed {
        stocks: vec![
            StockSpec::flat30("SZ000001", "净流出甲"),
            StockSpec::flat30("SH600001", "非机构乙"),
            StockSpec::flat30("SH600002", "无标志丙"),
            StockSpec::flat30("SH600003", "零净额丁"),
        ],
        dragons: vec![
            DragonRow {
                symbol: "SZ000001",
                trade_date: NOW,
                net_amount: -5.0e8, // 机构席位但净流出
                institution_flag: Some(1),
            },
            DragonRow {
                symbol: "SH600001",
                trade_date: NOW,
                net_amount: 4.0e8, // 非机构席位（flag=0）
                institution_flag: Some(0),
            },
            DragonRow {
                symbol: "SH600002",
                trade_date: NOW,
                net_amount: 4.0e8, // flag 为 NULL
                institution_flag: None,
            },
            DragonRow {
                symbol: "SH600003",
                trade_date: NOW,
                net_amount: 0.0, // 净额恰为 0（严格 >0 边界）
                institution_flag: Some(1),
            },
        ],
        ..Seed::default()
    };
    let (_tmp, reader) = build(&seed);

    let data = run_sepa(&query(), &reader, now()).expect("run sepa");
    assert_eq!(data.rows.len(), 4);
    for row in &data.rows {
        let big_cap = &row.details.capital[2];
        assert_eq!(
            big_cap.score, 0.0,
            "非净买入 dragon 行不得贡献加分: {} (score={})",
            row.symbol, big_cap.score
        );
        assert!(
            (row.capital - 0.0).abs() < 1e-9,
            "{} capital 必须为 0: {}",
            row.symbol,
            row.capital
        );
    }
}

/// 维度 6a：dragon 加分按符号隔离 —— 只有「自身」有机构净买入才加分，
/// 别的股票买入不得跨符号泄漏（防整窗布尔化重构）。
///
/// fixture：X=SZ000001 有买入 dragon；Y=SH600001 无任何 dragon 行。
///
/// 断言目标值 vs 当前值：
/// - X: details.capital[2].score == 15.0（当前 10.0 → RED）
/// - Y: details.capital[2].score == 0.0（当前 0.0，守卫）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
#[test]
fn dragon_bonus_is_per_symbol_not_per_window() {
    let seed = Seed {
        stocks: vec![
            StockSpec::flat30("SZ000001", "买入股"),
            StockSpec::flat30("SH600001", "旁观股"),
        ],
        dragons: vec![buy_dragon("SZ000001")],
        ..Seed::default()
    };
    let (_tmp, reader) = build(&seed);

    let data = run_sepa(&query(), &reader, now()).expect("run sepa");
    assert_eq!(data.rows.len(), 2);
    let buyer = data
        .rows
        .iter()
        .find(|r| r.symbol == "SZ000001")
        .expect("buyer row");
    let bystander = data
        .rows
        .iter()
        .find(|r| r.symbol == "SH600001")
        .expect("bystander row");

    assert_eq!(
        buyer.details.capital[2].score, 15.0,
        "买入方自身必须得 15 分"
    );
    assert_eq!(
        bystander.details.capital[2].score, 0.0,
        "其他股票的买入不得跨符号泄漏给旁观股"
    );
}

/// 维度 6b：混合场景组合 —— 机构 dragon + survey 文件三态（不建 / 空文件 /
/// 多行含本股与它股）时，结果必须彼此一致且等于 [1] 的 15.0。
///
/// 攻击点：实现可能只删「计分」却仍读文件、或按「行数非空」误判——三态
/// 对比能同时暴露残余读取与残留计分。
///
/// 断言目标值 vs 当前值：
/// - 三态 details.capital[2].score 均 == 15.0（当前 不建=10.0、空=10.0、
///   多行=15.0 → RED）
/// - data_a == data_b == data_c（当前 a/b=10 vs c=15 → RED）
///
/// 可制造性：通过公开 API run_sepa + ParquetReader + tempdir parquet 构造；
/// 无内部注入需求。
#[test]
fn survey_file_three_states_do_not_change_dragon_score() {
    let base = Seed {
        dragons: vec![buy_dragon("SZ000001")],
        ..seed("SZ000001", "平安银行")
    };
    let absent = Seed {
        empty_survey_file: false,
        ..base.clone()
    };
    let empty_file = Seed {
        empty_survey_file: true,
        ..absent.clone()
    };
    let multi_row = Seed {
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
            SurveyRow {
                symbol: "SH600999", // 无 basics 行，不得影响任何计分
                survey_date: NOW,
                org_name: "南方基金",
                survey_type: "电话会议",
            },
        ],
        ..base.clone()
    };

    let (_t1, r1) = build(&absent);
    let data_a = run_sepa(&query(), &r1, now()).expect("run sepa: 无 survey 文件");
    let (_t2, r2) = build(&empty_file);
    let data_b = run_sepa(&query(), &r2, now()).expect("run sepa: 空 survey 文件");
    let (_t3, r3) = build(&multi_row);
    let data_c = run_sepa(&query(), &r3, now()).expect("run sepa: 多行 survey 文件");

    assert_eq!(
        data_a.rows[0].details.capital[2].score, 15.0,
        "无 survey 文件：dragon 买入 = 15.0"
    );
    assert_eq!(
        data_b.rows[0].details.capital[2].score, 15.0,
        "空 survey 文件：dragon 买入 = 15.0"
    );
    assert_eq!(
        data_c.rows[0].details.capital[2].score, 15.0,
        "多行 survey 文件：dragon 买入 = 15.0"
    );
    assert_eq!(data_a, data_b, "空文件与缺失必须逐字段一致");
    assert_eq!(
        data_a, data_c,
        "多行文件与缺失必须逐字段一致（survey 零影响）"
    );
}
