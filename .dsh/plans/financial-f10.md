# financial-f10 - Work Plan

## TL;DR (For humans)

**What you'll get:** 财务三表（利润表/资产负债表/现金流量表）数据从东财"主干版"换成"F10 完整版"报表，字段数从 46/57/48 暴增到 203/319/254——补齐研发费用、商誉、租赁负债、EPS、少数股东损益等此前缺失的关键科目。数据全量重抓 2020 至今，重新入库并刷新图表所用的 Parquet 文件。

**Why this approach:** F10 报表 API `columns=ALL` 全量返回且无数据丢失，纯选型问题；Parquet 列式存储让几百个 NULL 字段几乎不占空间，所以干脆全字段保留、一次到位。三张表在 Rust 侧走 `SELECT *` 导出，schema 扩展自动生效，配套只需要同步测试与文档。

**What it will NOT do:** 不改财务指标表（fin_indicators）、不动 Rust 业务代码（CompassTable 枚举/表名/PK 不变）、不做新旧数据增量迁移（旧 DMSK 数据直接丢弃全量重建）、不新增 GUI 财务展示功能。

**Effort:** Large
**Risk:** Medium - 全量重抓 6000+ 标的 × 2020 至今 × 200-300 字段，API 限流与耗时是主要风险；新 schema 与旧 Dolt 表不兼容需重建
**Decisions to sanity-check:** ① F10 字段全部保留（含 _YOY 列）② 本次重建用 replace 语义、未来增量恢复 merge ③ 字段命名保留 F10 原生列名 ④ Dolt 表名/PK/symbol 生成不变

Your next move: approve and run `$start-work` to execute. Full execution detail follows below.

---

> TL;DR (machine): Large effort, Medium risk - 三采集器切 F10 报表 + 全量重抓重建 Dolt 三表 + 刷新 Parquet + Rust 测试/文档配套同步

## Scope
### Must have
- fetch_income/fetch_balance_sheet/fetch_cash_flow 三采集器 REPORT_NAME 切到 RPT_F10_FINANCE_GINCOME/GBALANCE/GCASHFLOW，DDL/COLS 全字段（203/319/254）
- 三采集器测试 _HEADER/DDL/单位断言更新（TDD：先失败后通过），茅台 2024 单位断言（TOTAL_OPERATE_INCOME≈1.7414e11，BASIC_EPS≈68.64）
- Dolt 三表重建（drop 旧 schema + 新 DDL）+ 全量重抓 2020 至今 + data_updates 水位验证 + dolt commit/push
- Parquet 刷新（compass-data import-compass）+ 真实数据冒烟（行数/日期范围/数值单位）
- import_compass.rs 测试夹具/断言同步 F10 新字段（setup_financial_table、run_fin_*_exports_parquet）
- kb 文档：data-providers.md（补财务三表 schema 章节 + 决策记录追加）、architecture.md（字段数 46/57/48→203/319/254）
- 全程 ref #202

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不动 fin_indicators（显式列名 SELECT 存量告警，独立范围）
- 不动 fix/sepa-unit worktree 的文件
- 不改 CompassTable 枚举 / Dolt 表名 / PK (symbol, report_date) / symbol 生成 / START_YEAR=2020
- 不引入 GUI 消费财务数据的新功能
- 不做增量迁移（历史 DMSK 数据直接丢弃，TRUNCATE + 全量重建）
- 不新增其他数据源（保持东财 datacenter API）
- 不在生产代码使用 unwrap() / as 类型转换压制错误

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（先写失败测试再改采集器；Python pytest + Rust cargo test）
- Evidence: `.omo/evidence/financial-f10/<task>-<N>.<ext>`（所有断言/冒烟输出落盘）

## Execution strategy
### Parallel execution waves
- Wave 1: 任务 1（F10 API 字段全集实测，产出列名 JSON）
- Wave 2: 任务 2/3/4 并行（三采集器独立改造，均依赖任务 1 的字段 JSON）+ 任务 7 并行（Rust 测试夹具同步，仅依赖任务 1）
- Wave 3: 任务 5（Dolt 重建 + 全量重抓，依赖 2/3/4）
- Wave 4: 任务 6（Parquet 刷新 + 冒烟，依赖 5）+ 任务 8（kb 文档，依赖 2/3/4 字段数确定）
- Final: F1-F4 验证波并行

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | — | 2,3,4,7 | — |
| 2 | 1 | 5 | 3,4,7 |
| 3 | 1 | 5 | 2,4,7 |
| 4 | 1 | 5 | 2,3,7 |
| 5 | 2,3,4 | 6 | — |
| 6 | 5 | — | 8 |
| 7 | 1 | — | 2,3,4 |
| 8 | 2,3,4 | — | 6 |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. F10 报表 API 字段全集实测：抓取 RPT_F10_FINANCE_GINCOME/GBALANCE/GCASHFLOW 的完整列名清单，产出 `.omo/evidence/financial-f10/f10_columns.json`
  What to do: 用 curl 调用东财 datacenter API 抓三张 F10 报表各一页（茅台 600519 或任一标的，2024-12-31），filter 语法 `(SECURITY_CODE="600519")(REPORT_DATE='2024-12-31')` URL 编码；从返回 JSON 的 columns/字段提取**完整列名清单**（含 _YOY 列），保存为 `.omo/evidence/financial-f10/f10_columns.json`（每表一个数组，含字段名+示例值+类型推断）。同时验证 filter 用 REPORT_DATE（underscore 版）可用。若 API 返回 203/319/254 与 handoff 预期不符，**停下向用户报告偏差**，不擅自调整范围。
  Must NOT do: 不写任何采集器代码；不修改任何文件；不猜测列名——以 API 实际返回为准
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2,3,4,7
  References: .omo/handoff.md:30-31（API 用法+实测值）；collectors/common.py:295-378（fetch_paginated 参数结构）；kb/dev/toolchain.md（如遇 API 异常先查排查卡）
  Acceptance criteria: `.omo/evidence/financial-f10/f10_columns.json` 存在且三表字段数 ≥ 200/310/250（接近 handoff 的 203/319/254，±容差）；含 TOTAL_OPERATE_INCOME、RESEARCH_EXPENSE、BASIC_EPS、MINORITY_INTEREST 等 handoff 实测字段名
  QA scenarios: happy: `python3 -c "import json; d=json.load(open('.omo/evidence/financial-f10/f10_columns.json')); print({k: len(v) for k,v in d.items()})"` 输出三表字段数。failure: API 429/超时——记录到 kb/dev/toolchain.md 排查卡并重试（指数退避）；字段数与 handoff 不符→报用户。Evidence .omo/evidence/financial-f10/task-1-financial-f10.json
  Commit: N（纯证据收集，随任务 2 一起提交）

- [ ] 2. fetch_income.py 切换 RPT_F10_FINANCE_GINCOME：REPORT_NAME/DDL/COLS 全字段 + 测试同步（TDD）
  What to do: 先改 `collectors/tests/test_income.py` 的 `_HEADER` 为 F10 字段清单（从 f10_columns.json 的 GINCOME 生成）、`_make_row` 辅助函数同步、DDL 断言同步——**跑 pytest 确认失败（RED）**；再改 `collectors/fetch_income.py`：REPORT_NAME="RPT_F10_FINANCE_GINCOME"、FILTER_COLUMN 保持 "REPORT_DATE"（若任务 1 验证需改则用实测值）、DDL=全字段 CREATE TABLE（从 JSON 生成，数值列 DOUBLE、symbol/report_date PK）、COLS=全字段逗号串、tmp_name="_tmp_inc" 不变、START_YEAR=2020 不变、merge=True→**本次改为 merge=False（replace 语义全量重建）**、insert_sql 的 symbol 生成（CONCAT(UPPER(交易所后缀), SECURITY_CODE)）与 WHERE stock_basic 过滤保持不变——跑 pytest 确认通过（GREEN）。DDL 生成方式：用 python 脚本从 f10_columns.json 生成 DDL 文本（数值→DOUBLE，日期→DATE，其余→VARCHAR/TEXT），避免手抄 203 个字段。
  Must NOT do: 不改 fetch_paginated/common.py（除非任务 1 证明 filter 列名不兼容）；不手动改 DOLT_TABLE 表名；不在测试里硬编码除 F10 字段外的幻数；不删除既有测试用例
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 5
  References: collectors/fetch_income.py:23-90（常量+run+import_to_dolt）；collectors/common.py:179-277（import_replace_table merge/replace 语义）；collectors/tests/test_income.py（_HEADER 46 字段 需全换）；.omo/evidence/financial-f10/f10_columns.json
  Acceptance criteria: `uv run pytest collectors/tests/test_income.py -x -q` 全绿；RED 阶段先观察到 _HEADER 与 DDL 不匹配的失败输出（保存到 evidence）；茅台单位断言 TOTAL_OPERATE_INCOME≈1.7414e11（±1%）、BASIC_EPS≈68.64（±1%）通过
  QA scenarios: happy: `uv run pytest collectors/tests/test_income.py -v -q` 全过。failure: 断言失败→检查 _HEADER/DDL 生成与 JSON 一致性；fetch stub 返回字段与 _HEADER 不匹配→同步 conftest 的 StubSession 响应字段。Evidence .omo/evidence/financial-f10/task-2-financial-f10.txt
  Commit: Y | feat(data): switch fin_income collector to F10 GINCOME full schema\n\nref #202

- [ ] 3. fetch_balance_sheet.py 切换 RPT_F10_FINANCE_GBALANCE：REPORT_NAME/DDL/COLS 全字段 + 测试同步（TDD）
  What to do: 同任务 2 模式：先改 `collectors/tests/test_balance_sheet.py`（_HEADER 57→319 字段、_make_row、DDL 断言）确认 RED；再改 `collectors/fetch_balance_sheet.py`（REPORT_NAME="RPT_F10_FINANCE_GBALANCE"、FILTER_COLUMN 保持 "REPORT_DATE"、DDL/COLS 从 f10_columns.json 的 GBALANCE 生成、merge=False、tmp_name="_tmp_bs" 不变、symbol 生成/stock_basic 过滤不变）确认 GREEN。DDL 用脚本从 JSON 生成。
  Must NOT do: 同任务 2 约束；特别是不改 FILTER_COLUMN 除非任务 1 实测证明需改
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 5
  References: collectors/fetch_balance_sheet.py:24-100；collectors/tests/test_balance_sheet.py（_HEADER 57 字段需全换）；.omo/evidence/financial-f10/f10_columns.json
  Acceptance criteria: `uv run pytest collectors/tests/test_balance_sheet.py -x -q` 全绿；RED 失败输出已保存；资产负债表无 TOTAL_OPERATE_INCOME（用 total_assets 类字段断言单位，如 TOTAL_ASSETS 实测值待任务 1 确认，至少断言表导入行数与 PK 幂等）
  QA scenarios: happy: pytest 全过。failure: 同上模式。Evidence .omo/evidence/financial-f10/task-3-financial-f10.txt
  Commit: Y | feat(data): switch fin_balance_sheet collector to F10 GBALANCE full schema\n\nref #202

- [ ] 4. fetch_cash_flow.py 切换 RPT_F10_FINANCE_GCASHFLOW：REPORT_NAME/DDL/COLS 全字段 + 测试同步（TDD）
  What to do: 同任务 2 模式：先改 `collectors/tests/test_cash_flow.py`（_HEADER 48→254 字段、_make_row、DDL 断言）确认 RED；再改 `collectors/fetch_cash_flow.py`（REPORT_NAME="RPT_F10_FINANCE_GCASHFLOW"、FILTER_COLUMN 保持 "REPORT_DATE"、DDL/COLS 从 f10_columns.json 的 GCASHFLOW 生成、merge=False、tmp_name="_tmp_cf" 不变、symbol 生成/过滤不变）确认 GREEN。DDL 用脚本从 JSON 生成。
  Must NOT do: 同任务 2 约束
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 5
  References: collectors/fetch_cash_flow.py:22-88；collectors/tests/test_cash_flow.py（_HEADER 48 字段需全换）；.omo/evidence/financial-f10/f10_columns.json
  Acceptance criteria: `uv run pytest collectors/tests/test_cash_flow.py -x -q` 全绿；RED 失败输出已保存
  QA scenarios: happy: pytest 全过。failure: 同上模式。Evidence .omo/evidence/financial-f10/task-4-financial-f10.txt
  Commit: Y | feat(data): switch fin_cash_flow collector to F10 GCASHFLOW full schema\n\nref #202

- [ ] 5. Dolt 三表重建 + 全量重抓 2020 至今：drop 旧表 → 新 DDL → 采集 → import → dolt commit/push
  What to do: 在 `/data/compass-data/compass_data`（Dolt 仓库，remote doltremoteapi.dolthub.com/skwy/compass_data）执行：`dolt sql -q "DROP TABLE IF EXISTS fin_income, fin_balance_sheet, fin_cash_flow"`（旧 DMSK schema 与新 F10 字段集不兼容，必须 drop 重建）；运行三采集器全量重抓（`uv run python main.py fetch income/balance_sheet/cash_flow` 或直接 `python collectors/fetch_xxx.py`，2020 至今，page_size 视 API 限流调整）；`import_replace_table` 以 merge=False（replace 语义）导入建新表；验证 data_updates 表 last_report_date 正确更新；**立即 dolt add + dolt commit + dolt push origin main + dolt status 确认干净**（ref #190 规则：写库后必须同 session 收尾）。Dolt 仓库操作路径见 kb/dev/database.md。
  Must NOT do: 不 TRUNCATE 后再用旧 schema 建表；不手动编辑 Dolt 数据；不 skip dolt commit/push；不删除 data_updates 里其他表的水位记录
  Parallelization: Wave 3 | Blocked by: 2,3,4 | Blocks: 6
  References: kb/dev/database.md（Dolt 查询/同步/提交）；collectors/common.py:179-277（import_replace_table）；AGENTS.md「compass_data Dolt 仓库 — 每次数据变更后 commit & push」
  Acceptance criteria: `dolt status` 干净且与 origin 同步；三表存在且 schema 含 F10 字段（`dolt schema show fin_income | wc -l` 反映 200+ 字段）；行数 > 0；data_updates 有正确 last_report_date（≥2020 年）
  QA scenarios: happy: `dolt sql -q "SELECT COUNT(*), MIN(report_date), MAX(report_date) FROM fin_income"` 行数>0 且日期覆盖 2020-2026。failure: 导入失败→查 kb/dev/toolchain.md 排查卡；字段数不符→对照 f10_columns.json 检查 DDL 生成。Evidence .omo/evidence/financial-f10/task-5-financial-f10.txt（含 dolt status/行数/日期范围输出）
  Commit: N（Dolt 数据提交是 dolt commit，代码无变更）

- [ ] 6. Parquet 刷新 + 真实数据冒烟：compass-data import-compass 导出三表 + 行数/日期范围/数值单位断言
  What to do: 运行 `cargo run --bin compass-data -- import-compass --table fin_income/fin_balance_sheet/fin_cash_flow --overwrite`（或等价命令，确认 CLI 用法后执行）导出三张 parquet；用 duckdb CLI 或 Rust 查询验证：行数一致、日期范围 2020-2026、**茅台 2024 年报数值单位断言**（TOTAL_OPERATE_INCOME≈1.7414e11 元 ±1%、BASIC_EPS≈68.64）——这是「数据终态证据」（ref #154 教训：冒烟必须落库验证，不能只看 exit 0）。输出全部断言到 evidence。
  Must NOT do: 不在无真实数据时用 fixture 冒充冒烟；不跳过数值断言只看行数；不修改 import_compass.rs 业务代码（SELECT * 自动带新列）
  Parallelization: Wave 4 | Blocked by: 5 | Blocks: —
  References: crates/compass-data/src/import_compass.rs:234-342（import_financial_table/import_append_table SELECT * 路径）；kb/design/data-providers.md（Parquet 布局）；AGENTS.md「compass-data CLI 速查」
  Acceptance criteria: 三张 parquet 存在；行数与 Dolt 一致；日期范围覆盖 2020 至今；茅台断言通过；冒烟输出落盘 evidence
  QA scenarios: happy: duckdb 查询断言全过。failure: 数值偏差>1%→查单位口径（F10 元 vs DMSK 元已 handoff 实测一致，若不一致停下报用户）。Evidence .omo/evidence/financial-f10/task-6-financial-f10.txt
  Commit: N（数据产物，无代码变更）

- [ ] 7. import_compass.rs Rust 配套同步：测试夹具 F10 新字段 + SELECT * 导出新列断言 + cargo test/clippy/fmt/doc
  What to do: 修改 `crates/compass-data/src/import_compass.rs` 测试模块：`setup_financial_table`（853-882 行）的 CREATE TABLE 改用 F10 新字段（从 f10_columns.json 取代表性字段：TOTAL_OPERATE_INCOME、RESEARCH_EXPENSE、BASIC_EPS 等）+ 保留 symbol/report_date PK；`run_fin_balance_sheet/income/cash_flow_exports_parquet`（884-945 行）增强断言：导出 parquet 的 DESCRIBE 含 F10 新列名（SELECT * 自动带新列的行为被测试锁定）。业务代码（import_financial_table/import_append_table）**不改**。跑 `cargo test -p compass-data && cargo clippy -p compass-data -- -D warnings && cargo fmt --check && RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p compass-data`。
  Must NOT do: 不改 import_financial_table/import_append_table 业务逻辑；不改 CompassTable 枚举；不引入 unwrap()/as 压制类型；不改 fix/sepa-unit 相关文件
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: —
  References: crates/compass-data/src/import_compass.rs:853-945（测试夹具与导出测试）；kb/dev/testing.md（Rust 测试模式）；.omo/evidence/financial-f10/f10_columns.json
  Acceptance criteria: `cargo test -p compass-data` 全绿；`cargo clippy -p compass-data -- -D warnings` 零警告；`cargo fmt --check` 干净；`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p compass-data` 无警告
  QA scenarios: happy: 四命令全过。failure: 测试断言失败→检查 DESCRIBE 列名与 JSON 一致性；clippy 警告→修复后重跑。Evidence .omo/evidence/financial-f10/task-7-financial-f10.txt
  Commit: Y | test(data): lock F10 schema expansion in import_compass tests\n\nref #202

- [ ] 8. kb 文档同步：data-providers.md 补财务三表 schema 章节 + architecture.md 字段数 + 决策记录追加
  What to do: `kb/design/data-providers.md`：在 Schema 章节（89-129 行后）补「财务三表 Parquet schema」小节——表名、PK、字段数（203/319/254）、单位（元）、来源（RPT_F10_FINANCE_G*）、SELECT * 导出路径说明；追加决策记录条目（F10 选型 + 全字段保留 + 元单位）。`kb/design/architecture.md`：322-325 行采集器字段数 46/57/48 → 203/319/254。检查决策记录章节格式符合 AGENTS.md「## 决策记录」规范（表格：决策/选项/选择/理由/排除原因）。
  Must NOT do: 不改 AGENTS.md（除非规则变更需要）；不写与实现不符的字段数；不删除既有文档内容
  Parallelization: Wave 4 | Blocked by: 2,3,4 | Blocks: —
  References: kb/design/data-providers.md:89-129（Schema 章节）:333（决策记录）；kb/design/architecture.md:322-325（字段数）；.omo/evidence/financial-f10/f10_columns.json
  Acceptance criteria: 两个文件更新完成；data-providers.md 含财务三表 schema 小节与决策记录表格；architecture.md 字段数为 203/319/254
  QA scenarios: happy: grep 验证字段数 203/319/254 出现于两文件。failure: 决策记录格式不符→按 AGENTS.md 模板修正。Evidence .omo/evidence/financial-f10/task-8-financial-f10.txt
  Commit: Y | docs: document F10 financial schema in kb\n\nref #202

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- 每个代码变更 commit 引用 `ref #202`（issue #202 保持 OPEN 直到 push 后关闭）
- commit 顺序：任务 2/3/4（采集器，可独立 commit）→ 任务 7（Rust 测试）→ 任务 8（文档）
- Dolt 数据变更用 `dolt commit`（任务 5 内完成），不混入 git commit
- 每个 commit 后运行 `/review-work` 审查（compass-workflow 规则），发现问题修复后重新 commit（最多 2 轮）
- 用户确认 push 后：先 `/reflect` 写反思 commit（含 ref #202）再 push——反思与实现同批推送（ref #119）
- push 成功到 origin/master 后：`gh issue comment 202` 追加完成 comment（实现摘要+验收状态+commit 列表+方案偏差），然后 `gh issue close 202`

## Success criteria
- 三采集器 REPORT_NAME 为 F10 报表，DDL/COLS 全字段（203/319/254），pytest 全绿（含覆盖率 ≥95%）
- Dolt 三表重建成功、全量数据 2020-2026、dolt 仓库与 origin 同步
- Parquet 刷新，茅台 2024 断言通过（TOTAL_OPERATE_INCOME≈1.7414e11 元、BASIC_EPS≈68.64）
- cargo test/clippy/fmt/doc 全绿（compass-data）
- kb 文档已同步，决策记录完整
- issue #202 收尾（完成 comment + close）

