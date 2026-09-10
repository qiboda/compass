# #360 F3 — 测试与覆盖率（tests + coverage + real-data smoke）

审计对象：worktree `remove-institution-survey`，实现 commit `9777dd6`（基点 `ad850c4`），
门禁于**修复 commit 前的最终工作树状态**复测（数据清理 + 五角度 review 修复后）。

## 1. 提交前门禁（最终态复测）

| 命令 | 结果 |
|---|---|
| `cargo fmt --all`（含测试 agent 产出的 3 个新测试文件 + review 修复） | 通过（pre-commit hook `cargo fmt --check` 亦通过） |
| `cargo test --workspace` | exit 0；全部测试 binary ok |
| `just check`（fmt --check + clippy -D warnings + test） | **exit 0**（全绿）；输出落盘 `.dsh/evidence/remove-institution-survey/360-F3-just-check.txt`（精简版：fmt/clippy 结论 + 46 个 test-result 行） |
| `bash scripts/tests/test-update-database.sh` | **ALL TESTS PASSED**（exit 0，含 section 17 对 `cli.md` 10 表口径的静态断言） |

新增测试（GREEN）：

- `crates/compass-strategy/tests/institution_survey_removal_adversarial.rs`：7 passed / 0 failed
- `crates/compass-strategy/tests/requirement_institution_survey_removal.rs`：4 passed / 0 failed
- `crates/compass-data/tests/requirement_institution_survey_removal.rs`：2 passed / 0 failed
- review 追加：`crates/compass-collectors/src/orchestrate.rs::removed_and_unknown_targets_are_rejected`（fetch/import 分派对已移除 target 与未知名均拒绝）

## 2. 覆盖率（llvm-cov nextest + 阈值脚本）

采集：`cargo llvm-cov nextest --json --summary-only > cov.json` →
`1704 tests run: 1704 passed, 2 skipped`（exit 0）。校验：`bash scripts/check-coverage.sh cov.json`
（输出落盘 `.dsh/evidence/remove-institution-survey/360-F3-coverage.txt`）：

| 目标 | 阈值 | 实测 | 结果 |
|---|---|---|---|
| workspace | 93% | 95.60% | OK |
| compass-core | 95% | 97.19% | OK |
| compass-data | 95% | 96.52% | OK |
| compass-i18n | 95% | 99.39% | OK |
| compass（GUI） | 90% | 92.85% | OK |
| compass-strategy | 95% | 96.86% | OK |
| compass-types | 95% | 99.44% | OK |
| compass-ui | 95% | 97.54% | OK |
| compass-collectors | 20% | 43.30% | OK |

### 2.1 首次 llvm-cov 运行失败及修复（问题闭环记录）

首跑 exit 100：`compass-data::bin/compass-data sepa::tests::run_backfill_dates_stage_csv_temp_files_cleaned_after_success`
断言失败——`stage_csv temp files were not cleaned: ["/tmp/compass_sepa_writeback/2026-08-14_market_temperature.csv_685732_11.csv"]`。
根因：该断言扫描**共享**临时目录（`std::env::temp_dir()/compass_sepa_writeback`），where 并行 nextest 进程正在写入
同日期前缀的 stage 文件；`cargo test`（`just check`）因线程/进程交错不同未复现。属 #357 反思记录过的同目录 flaky
（第二次出现）。
修复：`staged_files_containing` 按**本进程 PID** 过滤（`stage_csv` 文件名含 PID）+ 测试运行前置快照，
只对「本进程本次运行新增且未清理」的文件断言。修复后单测 PASS、全量 llvm-cov `1704 passed`。
| compass（GUI） | 90% | 92.81% | OK |
| compass-strategy | 95% | 96.86% | OK |
| compass-types | 95% | 99.44% | OK |
| compass-ui | 95% | 97.55% | OK |
| compass-collectors | 20% | 42.65% | OK |

未降门槛，无 crate 低于阈值。

## 3. 真实数据冒烟（plan Verification strategy）

| 步骤 | 命令 | 结果 |
|---|---|---|
| SEPA 评分（数据清理前） | `compass-data sepa score --top 5` | exit 0；`matched=4721 returned=5 date=2026-09-09`；写回 technical_factor 4721 / industry_factor 37 / capital_factor 4721 / final_score 4721 / market_temperature 1 |
| 历史回测 | `compass-data sepa backtest --start 2026-07-01 --csv /tmp/smoke-bt.csv` | exit 0；`days=52 scoring_ms=74093`；窗口 2026-07-01..2026-09-09、10 次换仓、策略累计 −30.78%、最大回撤 31.80%、基准 −7.84%；权益曲线落盘（1517 B） |
| 机构买入股抽样 | `SELECT d.symbol, c.big_capital_score FROM dragon_list d JOIN capital_factor c ... WHERE institution_flag=1` | 2026-09-09 机构买入股 capital 分含新 +15 效应（多只封顶 30：SH600108 / SH603900 / SZ000020 …） |
| 数据清理后二次冒烟 | `compass-data sepa score --top 3` | exit 0；`matched=4721`；TOP3 与清理前完全一致 → 最终状态不依赖已删除的表/parquet |
| 全窗口回测（修复 SEC-P1-1） | `compass-data sepa backtest --csv /tmp/backtest-full.csv` | exit 0；窗口 2025-01-02..2026-09-09、`days=411 scoring_ms=511846`、82 次换仓、策略累计 +15.77%／年化 9.44%／胜率 58.5%／最大回撤 32.87%（基准 +115.68%）；曲线落盘 → 恢复 `backtest_result` 完整历史快照（410 行；见 F4 §2 副作用披露） |

冒烟数据已提交并推送 Dolt（见 F4 的数据面记录）。

## 4. 已知边界（非本 issue 范围）

- 历史 `final_score` 行的分值仍是旧公式产物（survey +5 / dragon +10）；本次只重算 2026-09-09。
  「仅有调研、无机构龙虎买入」的标的在新口径下净失 5 分资本分（cap 30 无法吸收）。
  如需全历史口径统一，可另跑 `compass-data sepa backfill-dates`（不在 #360 scope，已建 issue #361 跟踪）。
- `market_temperature.total_amount` 重跑存在浮点末位差异（1.9036236812701262e+12 vs …218e+12，
  DuckDB 并行求和顺序所致），与 #360 无关，已单独 commit 记录。
- `backtest_result` 的无条件全表 DELETE（冒烟副作用）已修复数据面、代码层加固列入 issue #361。

## 5. 结论

F3 通过：`just check` 全绿（日志落盘）、覆盖率 9 个目标全部高于阈值（1704 tests 全过）、
真实数据 score/backtest 冒烟通过且数据清理后行为不变；首跑暴露的共享临时目录 flaky 已按问题闭环修复并复测。
