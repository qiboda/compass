# remove-institution-survey — Work Plan

> 状态（2026-09-10）：实现 + 数据清理 + evidence 完成，五角度 review 已跑（findings 已修）；
> 待反思 commit 与 push/PR/issue 收尾。台账见下 Todos，完成定义以 Final verification wave（F1-F4）为准。

## TL;DR (For humans)

**What you'll get:** `institution_survey` 表从全链路彻底移除——采集器模块、CLI 子命令、
Dolt/Parquet 数据、`CompassTable`/`InstitutionSurveyRow`/ParquetReader 方法、SEPA 评分 survey
信号（+5 重分配给 dragon_list 机构买入：+10 → +15，cap 30 不变）、`update-database.sh`、
全部文档。存量 Dolt 表与 parquet 文件直接删除，不留备份。

**Why this approach:** 用户 grill-me 已锁定（2026-09-09）：表不值得保留——每轮日常采集触发
`#343` 历史一致性守卫降级全量导出（根因：按 NOTICE_DATE 抓取、按 survey_date 锚定的晚发布
数据落入 Dolt 但不满足 `survey_date >= since` 增量切片）。与其修增量列，不如整表移除；
旧 survey +5 信号重分配给 dragon 机构买入（+10→+15），五模块公式其余不动。

**What it will NOT do:** 不改其他任何评分公式（dragon cap 30、main-flow 百分位、block-trade ±5、
四模块权重全部不变）；不动其他表/采集器；不为机构调研数据保留任何备份或归档。

**Effort:** Medium — 6 个 crate/文件组 + 文档，纯删除为主，公式改动仅 1 处（dragon 10→15）。
**Risk:** Medium — 跨 4 个 crate，测试须全程绿；覆盖率门槛 93% 不得下降。

**Decision anchors:** 用户决策 1-4（见 `.dsh/plans/handoff.md`）为契约，不可偏离。

---

> TL;DR (machine): Delete institution_survey across collectors/CLI/Dolt/Parquet/CompassTable/
> model/ParquetReader/SEPA scoring; reallocate survey +5 to dragon institution buy (+10→+15,
> cap 30 fixed); drop Dolt table + parquet, no backup; update docs (cli/data-providers/
> architecture/database/process/toolchain) + i18n zh/en note template + GUI note renderer.

## Scope

### Must have (repository code)

1. **`crates/compass-collectors`**
   - 删除整文件 `src/institution_survey.rs`；`src/lib.rs:38` 移除 `pub mod institution_survey`
   - `src/main.rs`：line 5 移除 import；删除 270-292 `institution-survey`/`institution_survey`
     CLI 分支；line 720 help 文本移除该子命令
   - `src/orchestrate.rs`：line 19 移除 import；line 147 fetch 分支；187-189 import 分支；
     590-603 sync 中 fetching/import 分支（`require_nonzero`
     app/collector 集合各减一项）

2. **`crates/compass-data/src/import_compass.rs`**
   - `CompassTable` 移除 `InstitutionSurvey` 变体（line 41）；parse 分支 line 65 删除
   - import match 159-176 删除（含 `warn_if_stale`）
   - line 234 文档注释中的表清单移除 `institution_survey`
   - `DDL` 763-769 删除 institution_survey 建表段
   - 测试：985-986 parse 用例、1631-1643 fixture、3156-3198 drift-guard 测试删除；
     相关测试辅助/常量（如表清单常量、fixture 行）同步清理

3. **`crates/compass-core`**
   - `src/model.rs` 233-249 删除 `InstitutionSurveyRow`（含 doc 注释；确认无其他引用）
   - `src/data/parquet.rs`：line 19 import 移除；删除 `fetch_institution_survey` 方法
     （860-882）；删除测试 1406-1470（含空文件/多行 fixture 用例）与相关 fixture 辅助

4. **`crates/compass-strategy`**
   - `src/sepa/scoring.rs`：
     - 文件头 doc（line 14 附近）：`dragon-list institution 10 + survey 5 + block-trade ±5`
       → `dragon-list institution 15 + block-trade ±5`
     - `SepaWindow` 删除 `surveys` 字段（line 91）；`fetch_sepa_window` 删除
       `fetch_institution_survey` 调用与 tracking::debug（141-143）及构造项
     - `score_sepa` 删除 surveys 切片（208-211）与 `surveyed` 集合构建（316-321）
     - `MarketContext` 删除 `surveyed` 字段与构造项（378）；`research`/`score_symbol`:
       `CapitalInputs` 删除 `has_survey`（675）；`score_symbol` 删除 `has_survey` 赋值（970）
     - `score_capital`：dragon `10.0` → `15.0`；删除 `survey = if has_survey {5.0} else {0.0}`；
       `big_capital` 计算移除 survey；`note_args` 变 `vec![main_flow, dragon, inputs.block_adj]`
     - 新增常量（如 `DRAGON_INSTITUTION_BONUS: f64 = 15.0`）——按现有常量风格，命名自定
   - `tests/sepa.rs`：删除 `TestSurvey` 结构/字段/`with_surveys`/表创建与 COPY 逻辑
     （61-82、101-102、239-255）；405-407 调用处及相应测试断言更新；
     检查 fixture 汇总行数/断言（`with_surveys(...)` 相关）
   - `src/sepa/` 其余文件确认无 survey 引用（已 grep：无）

5. **`crates/compass-i18n`（GUI 注记模板）**
   - `locales/en.yml:267`：`big_capital: "main %{main} + dragon %{dragon} + survey %{survey} + block %{block}"`
     → `"main %{main} + dragon %{dragon} + block %{block}"`
   - `locales/zh.yml:265`：`"主力%{main}+龙虎%{dragon}+调研%{survey}+大宗%{block}"`
     → `"主力%{main}+龙虎%{dragon}+大宗%{block}"`

6. **`crates/compass`（GUI note 渲染，display-only）**
   - `src/citizens/sepa.rs`：line 104 doc 注释 `main/dragon/survey/block` → `main/dragon/block`；
     `factor_note_text` big_capital 分支移除 `survey = ...`（arg(2) 改为 block）；
     line 845/849 测试 note_args 变 3 元素 `&[75.0, 10.0, 5.0]` / `&[75.0, 10.0, -5.0]`

7. **`scripts/update-database.sh` + `scripts/tests/test-update-database.sh`**
   - `update-database.sh:53` `COLLECTOR_TABLES` 移除 `institution_survey`
   - `test-update-database.sh`：全部 institution_survey 相关断言/桩数据/调用记录期望更新或删除
     （lines 87, 236, 259-261, 434, 519, 559-560, 601, 673, 709-710, 768, 780）——
     含 `COLLECTOR_TABLES` 精确断言、dolt add 行、import-compass 调用、共享日期桩
     （`program_name`/date 编排）、modified 行输出断言

### Must have (docs — 门禁 5b)

| 文件 | 变更 |
|---|---|
| `.dsh/kb/user/cli.md` | 128 `--table` 列表、132 `--since` 日期列说明、169 新鲜度列表、269/293/302/321 子命令与示例、345 增量机制、355 采集器清单 — 全部移除 institution_survey |
| `.dsh/kb/design/data-providers.md` | 377 `fetch_institution_survey` API 行、423 append 表清单、435/441/459 相关行；537（ParquetReader 原语 5→4）、555/560/572/574 决策记录行按「决策记录」约定处理（历史记录保留或标注移除 + 新增移除决策行） |
| `.dsh/kb/design/architecture.md` | 448 表清单、476 EastMoney 采集器清单、524 merge 分区表清单、719 C5 决策行、723 相关行 |
| `.dsh/kb/dev/database.md` | 39 SEPA 采集表清单、75 data_updates 锚点说明 |
| `.dsh/kb/dev/process.md` | 568 采集器清单相关行（按上下文） |
| `.dsh/kb/dev/toolchain.md` | 444/738/757/785/874 排查卡中的当前状态表述（历史卡按需标注；不篡改历史事实） |
| `.dsh/kb/design/*.md` 决策记录章节 | 确认存在 `## 决策记录`；新增一行：institution_survey 移除 + survey +5 重分配决策（ref #360） |

### Must NOT have (guardrails)

- 不改除 dragon 10→15 之外的任何评分数值/公式（cap 30、权重、main-flow、block-trade ±5 全部不动）
- 不为 institution_survey 数据做备份/归档/迁移（用户决策 3：直接删除）
- 不动其他表（dragon_list/block_trade/main_flow/fin_*/index_*）的采集、DDL、schema、数据
- 不改 CompassTable 其他变体、不改 import_compass 其他表路径/条件
- 不触碰 `.dsh/kb/` 中历史反思/归档记录（reflections*.md 只读）
- 不删除历史决策记录行的「记录事实」——只更新当前状态表述/清单，历史行加注或在行内保留
- 不绕过测试（RED 先行）、不跳过 `just check`、不手动降低覆盖率门槛

### Data cleanup（代码 GREEN 后，本任务专有步骤）

```sh
cd /data/compass-data/compass_data
dolt sql -q "DROP TABLE IF EXISTS institution_survey"
dolt sql -q "DELETE FROM data_updates WHERE table_name='institution_survey'"
dolt add institution_survey data_updates
dolt commit -m "feat: drop institution_survey table"
dolt push origin main
rm -f /data/compass-data/parquet_data/institution_survey.parquet
```
- 无 DuckDB export（不属于本次全量刷新）
- 语义：Dolt remote `dolthub.com/skwy/compass_data`（主库），写库动作必须 commit+push（ref #190）
- 验证：`dolt sql -q "SHOW TABLES"` 无该表、`dolt status` 干净、`ls parquet_data` 无文件

## Verification strategy

- RED 先行（门禁 3.5/4）：对抗性 + 需求验收两批新测试在实现前写并确认失败
- 实施后：`just check`（fmt + clippy + test + 覆盖率脚本）、`cargo llvm-cov` 阈值不降
- 真实数据冒烟：`sepa score`/`sepa backtest` 用真实 parquet 跑通，验证 dragon +15 生效、
  无 survey 相关 panic；`update-database.sh --dry-run`（或重放 calls 断言脚本）验证
  COLLECTOR_TABLES 无该表
- 全仓 grep 验证零残留：`grep -R institution_survey /data/codes/compass --exclude-dir=.git
  --exclude-dir=.worktrees --exclude-dir=target`（允许的历史记录例外需逐条说明）
- review：五角度并行（review/review_goal/review_quality/review_security/review_qa）

## Execution strategy

### Dependency matrix

| Todo | Depends on | Blocks |
| --- | --- | --- |
| 1. RED tests（adversarial + requirement） | plan | 2 |
| 2. collectors 删除 | 1 | 4, 6 |
| 3. compass-core 删除（model + parquet reader） | 1 | 4 |
| 4. compass-data 删除（enum/DDL/tests） | 1, 3 | 5 |
| 5. compass-strategy 删除 + 分值重分配 + 测试 | 1, 3, 4 | 6 |
| 6. scripts（update-database.sh + tests）+ CLI/orchestrate | 2, 5 | 7 |
| 7. i18n + GUI note 渲染 | 5 | 8 |
| 8. docs 同步 | 5 | 9 |
| 9. 数据清理（Dolt drop + parquet rm + push） | 6 | 10 |
| 10. 全量验证（RED→GREEN、just check、冒烟、grep、覆盖率） | 2-9 | — |

### Todos

- [x] 1. 委派对抗性 + 需求验收测试（RED）
  What to do: 门禁 3.5/4——两个测试 subagent 独立写失败测试/测试清单
  Acceptance: 新测试当前失败（RED 证据）；列出需删除/更新的既有测试清单
- [x] 2. collectors：删除模块 + CLI + orchestrate 分支
- [x] 3. compass-core：删除 InstitutionSurveyRow + fetch_institution_survey + 测试
- [x] 4. compass-data：删除枚举/parse/import match/DDL/测试
- [x] 5. compass-strategy：评分删除 survey 信号 + dragon 10→15 + 测试/fixture 更新
- [x] 6. scripts：update-database.sh + test-update-database.sh 断言更新
- [x] 7. i18n 模板 + GUI `factor_note_text` 更新（display-only）
- [x] 8. docs 同步（上述清单全部文件 + 决策记录行）
- [x] 9. 数据清理（Dolt drop + commit/push + parquet 删除 + 验证）
- [x] 10. 全量验证 —— 完成定义见下节 Final verification wave（F1-F4），本行不再重复列举

## Final verification wave

- **F1 合规审计**：issue #360 创建、plan 落盘、RED 证据、docs 清单逐项、决策记录行、
  commit 均 `ref #360`、分支在 worktree
- **F2 审查**：五角度 review 无 P0/P1；问题修复后复审
- **F3 测试+覆盖率**：`just check` 全绿 + `cargo llvm-cov nextest --json --summary-only` +
  `scripts/check-coverage.sh`（总 93% / 核心 crate 95% / compass 90% 不降）
- **F4 scope fidelity**：dragon 15/cap 30 唯一数值变更；无备份、无其他表改动、无公式漂移；
  grep 零残留（历史记录例外逐条列出）
- evidence 落盘 `.dsh/evidence/remove-institution-survey/`（实现收尾后一次性写）
