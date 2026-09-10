# #360 F3 — 测试与覆盖率（tests + coverage + real-data smoke）

审计对象：worktree `remove-institution-survey`，实现 commit `9777dd6`（基点 `ad850c4`）。

## 1. 提交前门禁

| 命令 | 结果 |
|---|---|
| `cargo fmt --all`（含测试 agent 产出的 3 个新测试文件格式化） | 通过（pre-commit hook `cargo fmt --check` 亦通过） |
| `cargo test --workspace` | exit 0；全部测试 binary ok |
| `just check`（fmt --check + clippy + test） | **exit 0**（全绿） |
| `bash scripts/tests/test-update-database.sh` | **ALL TESTS PASSED**（exit 0） |

新增测试（GREEN）：

- `crates/compass-strategy/tests/institution_survey_removal_adversarial.rs`：7 passed / 0 failed
- `crates/compass-strategy/tests/requirement_institution_survey_removal.rs`：4 passed / 0 failed
- `crates/compass-data/tests/requirement_institution_survey_removal.rs`：2 passed / 0 failed

## 2. 覆盖率（llvm-cov nextest + 阈值脚本）

采集：`cargo llvm-cov nextest --json --summary-only > cov.json` → `1703 tests run: 1703 passed, 2 skipped`（exit 0）。
校验：`bash scripts/check-coverage.sh cov.json`：

| 目标 | 阈值 | 实测 | 结果 |
|---|---|---|---|
| workspace | 93% | 95.59% | OK |
| compass-core | 95% | 97.17% | OK |
| compass-data | 95% | 96.55% | OK |
| compass-i18n | 95% | 99.39% | OK |
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

冒烟数据已提交并推送 Dolt（见 F4 的数据面记录）。

## 4. 已知边界（非本 issue 范围）

- 历史 `final_score` 行的分值仍是旧公式产物（survey +5 / dragon +10）；本次只重算 2026-09-09。
  如需全历史口径统一，可另跑 `compass-data sepa backfill-dates`（不在 #360 scope）。
- `market_temperature.total_amount` 重跑存在浮点末位差异（1.9036236812701262e+12 vs …218e+12，
  DuckDB 并行求和顺序所致），与 #360 无关，已单独 commit 记录。

## 5. 结论

F3 通过：`just check` 全绿、覆盖率 9 个目标全部高于阈值、真实数据 score/backtest 冒烟通过且清理后行为不变。
