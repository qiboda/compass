# F3 测试 + 覆盖率 — issue #357

证据时点：2026-09-06（阶段 0-5 + review 修复轮全部落地后）

## 测试基线（各阶段终点一致漂移）

| 套件 | 结果 | 备注 |
|---|---|---|
| cargo test -p compass --bin compass | 392 passed / 0 failed / 0 ignored | 含编辑器/jit 单测 + kittest（各编辑器 header/sidebar/body、N 键、快捷键、layout 持久化、workspace 切换、chart_editor_active 双判定、watchlist_leaf_open、Screener ⋮ 切换） |
| requirement_editor_architecture | 33 passed / 0 failed | 阶段 0 RED → 阶段 5 全 GREEN（i18n editor keys/toggle_sidebar、n_key_routed、timeframe scoping、layout 4 契约 etc.） |
| adversarial_357_editor_architecture | 7 passed / 0 failed | phase0/2f×3/3×2 + tabkind guard vacate |
| requirement_index_market | 5 passed / 0 failed | 含 41923b0 supersede 双形态 |
| compass-i18n lib | 9 passed / 0 failed | KEY_TREE 对称（zh/en）+ 插值 |
| compass-ui | 7 passed / 0 failed | Sidebar 拆分后行为不变 |
| cargo clippy -p compass --tests | 0 warning | sepa master 债务已在 cd4fddf 修复 |

## 覆盖率（compass ≥90% / workspace ≥93% 门槛）

- `cargo llvm-cov nextest --json --summary-only`（1690 tests run: 1690 passed, 1 slow, 2 skipped；`scripts/check-coverage.sh cov.json` **EXIT=0 全部 PASS**）：
  - workspace（core 合计）**95.60%**（≥93 PASS）；compass **92.78%**（≥90 PASS）；compass-core 97.19 / compass-data 96.58 / compass-i18n 99.39 / compass-strategy 96.86 / compass-types 99.44 / compass-ui 97.54（各 ≥95 PASS）；compass-collectors 41.93%（≥20 PASS）
  - 关键编辑器文件：editor/mod.rs 96.997% / tabs.rs 99.545% / main.rs 90.876%（lines）/ screener.rs 91.439% / chart.rs 88.235% / sepa.rs 96.448% / market.rs 96.269% / logger.rs 93.431% / ui_fixes_218.rs 99.471%
  - 前置 flaky 排除：/tmp/compass_sepa_writeback 遗留清理后全绿（run_backfill_dates_stage_csv_temp_files_cleaned_after_success 单测隔离 PASS 确认环境残留）
  - 工作区留存 cov.json（llvm-cov 导出，gitignored）
