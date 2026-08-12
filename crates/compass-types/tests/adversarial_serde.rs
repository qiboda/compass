//! 对抗性测试：serde 默认值函数全路径覆盖（issue #250）。
//!
//! 背景：compass-types 覆盖率门槛从 80% 提到 95%，实测缺口 15 行全部是
//! serde `#[serde(default = "...")]` 默认值函数（default_momentum_days /
//! default_momentum_min_pct / default_momentum_max_pct / default_volume_days /
//! default_volume_times）。根因：此前仅 `breakout = {}` 空表反序列化路径被
//! 测试覆盖，momentum/volume 空表或部分字段缺省的表从未反序列化，导致这
//! 五个函数从未被调用。
//!
//! 本文件用空表 / 部分字段缺省 / 非法输入 / 边界值 / round-trip 反序列化
//! 强制触发全部默认值函数，并断言 serde 契约（默认值正确、类型错误拒绝、
//! 越界拒绝、无业务校验）。
//!
//! 注：按本会话权限约束（仅允许写 `**/tests/**`），测试放在集成测试目录
//! 而非 lib.rs 的 mod tests 内；覆盖目标一致——默认值函数由 serde 反序列化
//! 路径触发，与测试文件位置无关。

use compass_types::{BreakoutCondition, MomentumCondition, ScreenerQuery, VolumeCondition};

/// `momentum = {}` 空表触发 default_momentum_days / min_pct / max_pct。
#[test]
fn empty_momentum_table_uses_per_field_defaults() {
    let src = "momentum = {}\n";
    let q: ScreenerQuery = toml::from_str(src).expect("empty momentum table parses");
    assert_eq!(
        q.momentum,
        Some(MomentumCondition {
            days: 20,
            min_pct: 0.0,
            max_pct: 100.0,
        })
    );
}

/// `volume = {}` 空表触发 default_volume_days / times。
#[test]
fn empty_volume_table_uses_per_field_defaults() {
    let src = "volume = {}\n";
    let q: ScreenerQuery = toml::from_str(src).expect("empty volume table parses");
    assert_eq!(
        q.volume,
        Some(VolumeCondition {
            days: 20,
            times: 2.0
        })
    );
}

/// 三个条件子结构同时为空表：各自走 per-field 默认值函数。
#[test]
fn all_three_empty_condition_tables_use_defaults() {
    let src = "breakout = {}\nmomentum = {}\nvolume = {}\n";
    let q: ScreenerQuery = toml::from_str(src).expect("empty tables parse");
    assert_eq!(q.breakout, Some(BreakoutCondition { days: 60 }));
    assert_eq!(
        q.momentum,
        Some(MomentumCondition {
            days: 20,
            min_pct: 0.0,
            max_pct: 100.0,
        })
    );
    assert_eq!(
        q.volume,
        Some(VolumeCondition {
            days: 20,
            times: 2.0
        })
    );
}

/// 部分字段缺省：只给 days，min_pct/max_pct 走默认值函数。
#[test]
fn partial_momentum_only_days_fills_rest_with_defaults() {
    let src = "momentum = { days = 30 }\n";
    let q: ScreenerQuery = toml::from_str(src).expect("partial momentum parses");
    assert_eq!(
        q.momentum,
        Some(MomentumCondition {
            days: 30,
            min_pct: 0.0,
            max_pct: 100.0,
        })
    );
}

/// 部分字段缺省：只给 min_pct/max_pct，days 走默认值函数。
#[test]
fn partial_momentum_only_bounds_days_uses_default() {
    let src = "momentum = { min_pct = 5.0, max_pct = 50.0 }\n";
    let q: ScreenerQuery = toml::from_str(src).expect("partial momentum parses");
    assert_eq!(
        q.momentum,
        Some(MomentumCondition {
            days: 20,
            min_pct: 5.0,
            max_pct: 50.0,
        })
    );
}

/// 部分字段缺省：days 与 times 各自独立走默认值函数。
#[test]
fn partial_volume_each_field_fills_missing_with_default() {
    let by_days: ScreenerQuery =
        toml::from_str("volume = { days = 5 }\n").expect("volume days parses");
    assert_eq!(
        by_days.volume,
        Some(VolumeCondition {
            days: 5,
            times: 2.0
        })
    );

    let by_times: ScreenerQuery =
        toml::from_str("volume = { times = 3.0 }\n").expect("volume times parses");
    assert_eq!(
        by_times.volume,
        Some(VolumeCondition {
            days: 20,
            times: 3.0
        })
    );
}

/// 边界值：days = 0 与 days = u32::MAX 都能反序列化并 round-trip 一致。
#[test]
fn zero_and_u32_max_days_roundtrip() {
    let src = "momentum = { days = 0 }\nvolume = { days = 4294967295 }\n";
    let q: ScreenerQuery = toml::from_str(src).expect("boundary days parse");
    assert_eq!(q.momentum.unwrap().days, 0);
    assert_eq!(q.volume.unwrap().days, u32::MAX);

    let back: ScreenerQuery =
        toml::from_str(&toml::to_string(&q).expect("serialize")).expect("roundtrip");
    assert_eq!(back, q);
}

/// 错误路径：u32 越界（溢出 / 负数）必须反序列化失败。
#[test]
fn days_outside_u32_range_rejected() {
    let overflow = "momentum = { days = 4294967296 }\n";
    assert!(
        toml::from_str::<ScreenerQuery>(overflow).is_err(),
        "days above u32::MAX must be rejected"
    );

    let negative = "volume = { days = -1 }\n";
    assert!(
        toml::from_str::<ScreenerQuery>(negative).is_err(),
        "negative days must be rejected"
    );
}

/// 错误路径：字段类型不匹配（字符串给 u32/f64）必须反序列化失败。
#[test]
fn type_mismatch_inside_condition_table_rejected() {
    let bad_days = "momentum = { days = \"thirty\" }\n";
    assert!(toml::from_str::<ScreenerQuery>(bad_days).is_err());

    let bad_pct = "momentum = { min_pct = \"5\" }\n";
    assert!(toml::from_str::<ScreenerQuery>(bad_pct).is_err());

    let bad_times = "volume = { times = \"x\" }\n";
    assert!(toml::from_str::<ScreenerQuery>(bad_times).is_err());
}

/// 注意：serde 不做业务校验——min_pct > max_pct 当前**不会**报错，而是
/// 原样保留。此测试锁定当前行为（若生产代码未来加校验需同步更新）。
#[test]
fn inverted_bounds_deserialize_as_is_no_business_validation() {
    let src = "momentum = { min_pct = 100.0, max_pct = 0.0 }\n";
    let q: ScreenerQuery = toml::from_str(src).expect("inverted bounds parse");
    assert_eq!(
        q.momentum,
        Some(MomentumCondition {
            days: 20,
            min_pct: 100.0,
            max_pct: 0.0,
        })
    );
}

/// 非法输入：条件表内未知键被静默忽略，已知字段照常反序列化。
#[test]
fn unknown_key_inside_condition_table_ignored() {
    let src = "momentum = { days = 30, bogus = 1 }\n";
    let q: ScreenerQuery = toml::from_str(src).expect("unknown key ignored");
    assert_eq!(
        q.momentum,
        Some(MomentumCondition {
            days: 30,
            min_pct: 0.0,
            max_pct: 100.0,
        })
    );
}

/// 非法输入：camelCase 键（minPct）与 snake_case 字段名不匹配，serde 不会
/// 映射，min_pct 静默使用默认值 0.0。
#[test]
fn camel_case_key_not_mapped_falls_back_to_default() {
    let src = "momentum = { minPct = 5.0 }\n";
    let q: ScreenerQuery = toml::from_str(src).expect("camelCase key ignored");
    assert_eq!(q.momentum.unwrap().min_pct, 0.0);
}

/// ScreenerQuery 整体 round-trip：空条件表反序列化后序列化，默认值函数
/// 产出的字段必须显式写出，再反序列化保持一致。
#[test]
fn query_roundtrip_injects_defaults_into_serialized_toml() {
    let src = "breakout = {}\nmomentum = {}\nvolume = {}\n";
    let q: ScreenerQuery = toml::from_str(src).expect("empty tables parse");
    let toml_str = toml::to_string(&q).expect("serialize");
    assert!(
        toml_str.contains("days = 20"),
        "days serialized: {toml_str}"
    );
    assert!(
        toml_str.contains("min_pct = 0.0"),
        "min_pct serialized: {toml_str}"
    );
    assert!(
        toml_str.contains("max_pct = 100.0"),
        "max_pct serialized: {toml_str}"
    );
    assert!(
        toml_str.contains("times = 2.0"),
        "times serialized: {toml_str}"
    );
    let back: ScreenerQuery = toml::from_str(&toml_str).expect("roundtrip");
    assert_eq!(back, q);
}

/// 缺省语义：momentum 字段缺失 → None（不启用）；volume 空表 → Some(默认)。
#[test]
fn momentum_absent_volume_present_keeps_none_vs_some() {
    let src = "volume = {}\n";
    let q: ScreenerQuery = toml::from_str(src).expect("parses");
    assert_eq!(q.momentum, None);
    assert_eq!(
        q.volume,
        Some(VolumeCondition {
            days: 20,
            times: 2.0
        })
    );
}
