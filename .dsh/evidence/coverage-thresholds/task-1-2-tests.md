# Task 1-2 Evidence — 补测测试证据（issue #250）

## Task 1: compass-types 对抗性测试（skwy-adversarial-test 产出）

**文件**: `crates/compass-types/tests/adversarial_serde.rs`（248 行，14 个 #[test]）

**验证输出**（`cargo nextest run -p compass-types`，提交前实测）:
```
Starting 23 tests across 2 binaries
    PASS (1-9)  compass-types tests::*          （9 个既有单测）
    PASS (10-23) compass-types::adversarial_serde *  （14 个新增对抗性测试）
Summary: 23 tests run: 23 passed, 0 skipped
```

**覆盖效果**（`cargo llvm-cov -p compass-types --json`，提交后实测）:
- 未覆盖行 63-73 / 108-114（default_momentum_days/min_pct/max_pct、default_volume_days/times）→ 全部触发
- compass-types 行覆盖 89.58% → **100%**（144/144）

## Task 2: compass-i18n 白名单分支补测（skwy-requirement-test 产出，主 agent 落盘）

**文件**: `crates/compass-i18n/src/lib.rs`（+104 行：`is_allowed_zh_token` 提取 + 2 表驱动测试）

**验证输出**（`cargo nextest run -p compass-i18n`，提交前实测）:
```
Starting 6 tests across 1 binary
    PASS tests::zh_whitelist_prefixes_allow_cjk_free_values   （16 正向用例）
    PASS tests::zh_whitelist_rejects_non_whitelisted_keys     （6 负面 key）
    PASS tests::all_key_constants_resolve_in_zh_and_en
    PASS tests::zh_values_are_chinese
    PASS tests::en_values_contain_no_cjk_characters
    PASS tests::locale_files_are_key_symmetric
Summary: 6 tests run: 6 passed, 0 skipped
```

**覆盖效果**（`cargo llvm-cov -p compass-i18n --json`，提交后实测）:
- 未覆盖行 413-415（sepa.unit / screener.ma / screener.years 白名单分支，真实 zh.yml 数据下被短路）→ 表驱动用例逐一真实求值
- compass-i18n 行覆盖 93.94% → **99.14%**（115/116；L13 宏行为不可测，接受）

## 主证据

- 全量验证: `.omo/evidence/coverage-thresholds/task-5-verify.json`（`cargo llvm-cov nextest --json --summary-only` 原始输出，8 项门槛全绿）
- 流程概述: `.omo/evidence/coverage-thresholds/task-evidence.md`
