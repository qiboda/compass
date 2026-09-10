//! Requirement-acceptance tests for issue #360: remove institution_survey.
//!
//! Plan contract (`.dsh/plans/remove-institution-survey.md`, acceptance
//! criteria): `CompassTable` no longer has an `InstitutionSurvey` variant and
//! `"institution_survey".parse::<CompassTable>()` returns `Err`; the other 10
//! table names keep parsing `Ok`.
//!
//! RED vs current code: `"institution_survey"` still parses
//! `Ok(CompassTable::InstitutionSurvey)` today, so `assert!(is_err())` fails.
//! The tests compile today (the `InstitutionSurvey` variant is never
//! referenced; `CompassTable` has no `Debug`, so only boolean asserts are
//! used) and turn GREEN once the variant + parse arm are removed.
//!
//! `CompassTable` exposes no other pub methods (only `FromStr`), so no
//! `as_str`/`table_name` assertions apply here.

use compass_data::import_compass::CompassTable;

#[test]
fn institution_survey_parse_rejected_after_removal() {
    // Given 旧表名 "institution_survey"（采集器/管线已不再支持的表）
    let parsed = "institution_survey".parse::<CompassTable>();
    // When 通过 CompassTable::from_str 解析
    //   Then 必须返回 Err——当前返回 Ok(CompassTable::InstitutionSurvey) → RED；
    //        实现移除变体与 parse 分支后返回 Err → GREEN。
    assert!(
        parsed.is_err(),
        "institution_survey 必须不再可解析为 CompassTable"
    );
}

#[test]
fn all_other_table_names_still_parse() {
    // Given 其余 10 个公开表名（issue #360 范围外，均不得受影响）
    let cases: &[(&str, CompassTable)] = &[
        ("stock_basic", CompassTable::StockBasic),
        ("fin_indicators", CompassTable::FinIndicators),
        ("fin_balance_sheet", CompassTable::FinBalanceSheet),
        ("fin_income", CompassTable::FinIncome),
        ("fin_cash_flow", CompassTable::FinCashFlow),
        ("capital_main_flow", CompassTable::MainFlow),
        ("dragon_list", CompassTable::DragonList),
        ("block_trade", CompassTable::BlockTrade),
        ("index_daily", CompassTable::IndexDaily),
        ("index_basic", CompassTable::IndexBasic),
    ];
    for (name, expected) in cases {
        // When 解析每个表名
        //   Then 必须仍为 Ok 且映射到不变的正确变体（当前与实现后均成立 → 守卫）。
        let parsed = (*name).parse::<CompassTable>().expect("parse must succeed");
        assert!(
            parsed == *expected,
            "表 {name} 必须仍映射到同一 CompassTable 变体"
        );
    }
}
