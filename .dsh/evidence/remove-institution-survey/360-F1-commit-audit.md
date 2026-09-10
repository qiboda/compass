# #360 F1 — 合规审计（compliance audit）

审计对象：worktree `remove-institution-survey`，分支 `feat/remove-institution-survey`。
审计时机：实现 commit 收尾后一次性落盘（ref #181 教训：evidence 不中途写）。

## 1. Issue

| 项 | 值 |
|---|---|
| Issue | #360 `feat: remove institution_survey and reallocate SEPA survey points` |
| URL | https://github.com/qiboda/compass/issues/360 |
| Labels | A-Data / C-Feature / D-Straightforward / P-Medium |
| 审计时状态 | OPEN（未合并，符合"只在 push 后关闭"约束） |

## 2. Plan 落盘

- `.dsh/plans/remove-institution-survey.md`（179 行）在 worktree 内创建，随实现 PR 提交（commit `9777dd6`）。
- 覆盖：TL;DR / Scope / Must NOT have / 数据清理命令 / Todo 依赖矩阵 1-10 / Final verification wave F1-F4。

## 3. Git 事实

| 项 | 值 |
|---|---|
| 基点（origin/master） | `ad850c4` |
| 分支 | `feat/remove-institution-survey` |
| worktree | `/data/codes/compass/.worktrees/remove-institution-survey` |
| 提交台账（本 evidence 更新于反思 commit 之前；3 个 commit 已落盘） | |
| `9777dd6` | `feat: remove institution_survey end-to-end and reallocate survey points to dragon bonus`（24 files, +1531/−657） |
| `824817d` | `docs: add #360 F1/F3/F4 verification evidence`（3 files, +191） |
| `b8255c4` | `fix: address #360 five-angle review findings (docs, tests, evidence)`（15 files, +357/−42） |
| 随后 1 个（docs-only） | 反思 commit：`.dsh/kb/dev/reflections.md` + 本 F1 台账更新——PR 最后一个 commit，与实现同批推送 |
| 区间规模 | `git diff --shortstat ad850c4..HEAD` = **33 files changed, +2043 / −663**（截至 `b8255c4`） |
| `ref #360` 独立成行 | 3 / 3 commit 各自命中 1 行（`git log ad850c4..HEAD --format=%B \| grep -c '^ref #360$'` = 3） |
| pre-commit hook | `cargo fmt --check` 每次通过 |

> 时点说明（质量 review QUAL-P1-1/目标 review P3-3 指出）：本文件首次落盘于 `824817d` 时点，
> 当时区间为 1 commit / 24 files / +1531−657；上表为补齐后的**可复核台账**（含 review 修复 commit），
> PR 最终规模以 issue #360 完成 comment 为准。

## 4. RED → GREEN 证据

门禁 3.5（对抗性）与门禁 4（需求验收）在实现前独立写测试并确认失败：

| 测试 | RED 证据 | GREEN 证据 |
|---|---|---|
| `crates/compass-strategy/tests/institution_survey_removal_adversarial.rs`（7 测试） | 4 FAILED：`institution_dragon_bonus_raised_from_10_to_15`（left 10.0 vs right 15.0）、`survey_rows_do_not_score_without_dragon`（5.0 vs 0.0）、`dragon_bonus_is_per_symbol_not_per_window`、`survey_file_three_states_do_not_change_dragon_score` | `cargo test -p compass-strategy --test institution_survey_removal_adversarial` → **7 passed; 0 failed** |
| `crates/compass-strategy/tests/requirement_institution_survey_removal.rs`（4 测试） | 2 FAILED：`dragon_institution_buy_scores_15_without_survey`（15.0/3.0 vs 10.0/2.0）、`survey_rows_do_not_score_without_dragon`（0.0 vs 5.0） | `cargo test -p compass-strategy --test requirement_institution_survey_removal` → **4 passed; 0 failed** |
| `crates/compass-data/tests/requirement_institution_survey_removal.rs`（2 测试） | 1 FAILED：`institution_survey_parse_rejected_after_removal`（now Ok → ERR） | `cargo test -p compass-data --test requirement_institution_survey_removal` → **2 passed; 0 failed** |
| `scripts/tests/test-update-database.sh` 新增断言块（431-435，`COLLECTOR_TABLES: institution_survey must be removed`） | RED：159 PASS / 1 FAIL（FAIL 即该断言，日志 `/tmp/tudb.log`） | `bash scripts/tests/test-update-database.sh` → **ALL TESTS PASSED**（exit 0，含 section 17 的 `cli.md` 静态断言） |

守卫型用例（实现前后均须通过）：`big_capital_stays_capped_at_30`、`missing_survey_parquet_degrades_gracefully`、
`non_buy_dragon_rows_do_not_contribute`、`run_sepa_succeeds_without_survey_parquet`、
`all_other_table_names_still_parse`（10 表全 Ok）。

## 5. 文档同步（门禁 5b）逐项清单

| 文件 | 更新内容 |
|---|---|
| `.dsh/kb/user/cli.md` | `--table` 列表 11→10；`--since` 日期列说明去 survey_date；新鲜度行情表列表；`institution-survey` 子命令行；`fetch institution_survey` 示例；progress 写入方 6 模块/8 文件→5/7；fetch/import target 集合；增量机制时间序列表 5→4；SEPA 采集器说明 bullet；TRIM 覆盖清单；每日管线 `import-compass 11 张表`→`10 张表` |
| `.dsh/kb/design/data-providers.md` | 删 `fetch_institution_survey` 原语行；append 表清单；TRIM 覆盖列表与分组键 bullet；代理接入面清单；读取原语决策行 5→4；导入语义决策行 5→4；TRIM 范围/gk 键历史行加注；**决策记录新增 #360 行** |
| `.dsh/kb/design/architecture.md` | 采集器模块表；B3 迁移历史行加注；`import-compass` append 表清单；C5 决策行加注（接入者 6→5）；C5 注（progress 写入方 6 模块/8 文件→5/7） |
| `.dsh/kb/dev/database.md` | SEPA 采集表清单；"全部 11 张 compass_data 表"→10；`last_report_date` 语义行删 `institution_survey = MAX(survey_date)` |
| `.dsh/kb/dev/process.md` | 采集器代理接入状态清单去 institution_survey |
| `.dsh/kb/dev/toolchain.md` | 性能卡（7 份→6 份数据）、#298 恢复卡（9→4 append 表）、#343 卡与 09-04 验证卡加注"已随 #360 删除"（历史事实保留） |
| `AGENTS.md` | 无需改动（无 institution_survey 引用，grep 核实） |

决策记录（门禁 5c）：`data-providers.md` 与 `architecture.md` 均含 `## 决策记录` 章节；本次新增
`institution_survey 全链路移除 + SEPA 评分重分配（issue #360）` 行（what / why / why-not 自包含）。

## 6. 结论

F1 通过：issue、plan、RED 证据、docs 清单、决策记录行、`ref #360`、worktree 分支归属均核实。
