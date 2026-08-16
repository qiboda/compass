# industry-ths 完成报告 — issue #283 板块数据源战略调整

日期：2026-08-16 · 分支：feat/industry-ths · PR: https://github.com/qiboda/compass/pull/285 · Issue: https://github.com/qiboda/compass/issues/283（已关闭）

## Commit 清单（12 个，base 3fd7248 → rebased onto a908ca5）

| SHA | 内容 |
|---|---|
| f59df12 | feat: THS 行业采集器（fetch_ths_industry_list/fetch_ths_kline/run 两段重构），移除 concept 采集 |
| 6150a93 | feat: Rust 侧 concept 全链路移除 + industry 聚合 + BK 4-6 + theme_score 删 |
| 5661e89 | docs: KB 同步（data-providers/symbols/ui/gui/toolchain）+ plan + cleanup 脚本 |
| d257cf3 / b47807b | docs: D3 两次修正定稿（先保留→后删除东财 BK 行） |
| fd08242 | fix: review 修复（THS 日期归一化/年循环失败不截断/fetch 重试/列表空告警/除零 guard/sepa_daily.sh/新对抗测试 23 个） |
| f24e8bb | docs: P3 注释清理 |
| 6c1666b | fix: sepa theme 断言修复（fixture amount 5e8→9e8 + margin 断言） |
| 639c864 | test: 删 importorskip 死测试 |
| be31455 | fix: basic CSV 每次 run 重建（CSV 复活事故根因修复 + 测试契约反转） |
| fd382ae | fix: import timeout 600→3600 + cleanup 脚本 CSV 镜像同步删除 |
| 378c002 | docs: post-implementation reflection |

## 验收状态（8/8 达成，goal 审查逐项核实）

1. ✅ 采集器：THS 源 + concept 移除 + 快速失败（#277 机制）
2. ✅ BK 4-6 位符号（validate_symbol/parse_explicit_prefix + 测试）
3. ✅ 数据清理：Dolt commit pvah87l0（concept 3263 + BK 496 + concept_member + theme_score 列）
4. ✅ SEPA 行业聚合 / theme_score 删 / themes 移除 / 题材列保留
5. ✅ GUI 概念段移除 + i18n key 删
6. ✅ 官方指数腾讯源不变（回归绿）
7. ✅ 测试：对抗/需求/收集对抗 + 全套件绿
8. ✅ 全套件绿：pytest 602 passed（cov 95.67%）；cargo test 38 套 0 失败；fmt/clippy/ruff 干净

## 数据落地（Dolt compass_data，已 commit + push）

- 清理：`pvah87l0`（concept 3263 行 + 东财 BK 496 行 + concept_member 表 + theme_score 列 + _tmp_name_en）
- 采集导入：`aq7b7sk` — index_basic = 90 industry（BK881xxx THS）+ 30 official；index_daily = 367,780 industry + 145,215 official = 512,995 行（1990-12-19 ~ 2026-08-14）
- 列名迁移：`l9b2ft8`（final_score industry_factor 列 concept_name→industry_name）

## 方案偏差与修复

1. **CSV 复活事故**（真实运行暴露）：`_persist_outputs` 增量门禁不重建 basic CSV → B1 清理后旧镜像残留 → import 复活 1,000 已删行。修复：每次非短路 run 全量重建（merge 永不丢行）+ cleanup 脚本同步删 CSV + 测试契约反转（be31455/fd382ae）。教训入 reflections（378c002）。
2. **Dolt 大表导入性能**：INSERT IGNORE 512,995 行 41min 未完成 → dolt table import 批量路径 18s 完成 + 原子 RENAME 交换；timeout 600→3600（fd382ae）。

## 审查记录（五角度全过）

- quality：P1-1 年循环（fd08242 修）
- general：P1-1 sepa_daily.sh（fd08242 修）、P1-2 年循环（fd08242 修）
- security：无 blocking（P2 _dolt_close 已加固）
- goal：9/9 决策、8/8 验收全达成；遗留 Dolt 工作区残留 → 本次采集收尾已清理
- qa：P0-1 theme=16.67（6c1666b 修）、P0-2 Dolt 无 THS 数据（aq7b7sk 已入）、P1-3 industry_factor 列名（l9b2ft8 已改）

## 后续建议（proposed，未建 issue）

- Dolt 大表导入性能：merge 模式 INSERT IGNORE → dolt table import 路径（已实测 100 倍+）
- import 后自动断言 index_type 分布/行数，当场拦截复活类数据事故
