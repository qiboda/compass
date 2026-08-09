# data-quality-monitor - Work Plan

## TL;DR (For humans)

**What you'll get:** `import` 与 `import-compass` 两个数据导入命令会在写盘后自动核查「源 Dolt 数据」与「写入的 Parquet 文件」是否一致——行数精确对比、日期范围对比、数据新鲜度检查。发现不一致（如查询条件笔误导致数据截断）立即报错退出，绝不静默产出坏数据；发现数据过期（采集停滞）只告警、不中断正常流程。

**Why this approach:** 现在 import 只把源行数打印进日志、从不验证写盘结果——一次静默截断会直接污染下游 Parquet 而无感知。校验完全复用现有的 `dolt sql` 查询与 duckdb 读取通道，零新依赖、改动集中在两个命令文件。merge 增量路径因语义（新旧合并）无法精确对比，退而求其次保证"不丢数据"；新鲜度阈值按表分类（财务报告 120 天 / 行情数据 7 天）。

**What it will NOT do:** 不比较新旧 Parquet 文件（二期再做）；不改 Python 采集器；不加邮件/通知等外部告警（报错退出即告警）；不新增配置文件项（阈值写死为常量）。

**Effort:** Medium
**Risk:** Medium - 给两个 import 命令新增了报错退出路径，需小心不破坏既有的 merge 回退、空数据跳过等分支
**Decisions to sanity-check:** 新鲜度阈值（财务 120 天 / 行情 7 天）；`--limit` 下预期行数 = 源行数与 limit 的较小值；merge 路径只保证不丢数据、不做精确对比

Your next move: approve, or run a high-accuracy review. Full execution detail follows below.

---

> TL;DR (machine): Medium effort, Medium risk - row-count/date-range/freshness validation for import + import-compass, fail-loudly on mismatch, warn on stale; 9 todos + 4 final-verification tasks, compass-data 95% coverage gate.

## Scope
### Must have
- `import`（import_dolt::run）：写盘后校验 ①源 Dolt COUNT(同 WHERE) vs 写入 parquet COUNT（limit>0 时预期 = min(源COUNT, limit)，即"最多 N 行"语义）；②源 vs 目标 tradedate MIN/MAX 精确对比（limit>0 时跳过）。任一不一致 → 返回 Err，命令 exit(1)（fail loudly）
- `import-compass`（import_compass::run）：
  - 全量路径（无 --since / --overwrite / 首次导入）：源 Dolt COUNT(含 date_filter + 表专属 WHERE) vs 写入 parquet COUNT 精确对比，不一致 → Err
  - merge 路径（--since + 已有 parquet + 非 overwrite）：校验 merge 后行数 ≥ 旧 parquet 行数（不丢数据）；**merge 失败 fallback 路径跳过此校验**（fallback 是修复损坏文件的恢复机制，其预期 = 源 COUNT(含 date_filter)，视同全量导出）
  - 新鲜度校验：读 compass_data Dolt 的 `data_updates.last_report_date`，按表阈值——fin_indicators/fin_balance_sheet/fin_income/fin_cash_flow = 120 天；capital_main_flow/dragon_list/block_trade/institution_survey/concept_member = 7 天；stock_basic 跳过（last_report_date 为 NULL）。过期 → warn 不退出
- 生产代码行数 helper：把测试中的 `read_parquet_row_count`（import_compass.rs:1016-1024）提升为共享生产函数
- 文档同步：kb/user/cli.md（两命令行为变更）、kb/dev/database.md（data_updates 消费方）、kb/design/data-providers.md（如涉 schema 说明）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 与旧 Parquet 文件对比（Q2 留二期，grill-me 锁定）
- Python collectors 侧任何改动（不碰 common.py / main.py / fetch_*.py / 不新增 Python 测试）
- 新增告警通道（邮件/通知/Webhook）——error 退出 + exit(1) 即告警机制
- `import` 命令的新鲜度校验（investment_data 无 data_updates 表，handoff 确认）
- 修改 import_compass 的 DuckDB merge SQL 语义（ROW_NUMBER 去重逻辑不动）
- 新增配置项（阈值硬编码常量，不进 config.toml，不做 CLI flag）
- 不引入任何新 crate 依赖（duckdb/chrono 已在 compass-data 依赖中）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD - RED（skwy-requirement-test 委派写失败测试）→ 实现 GREEN → 独立 QA 复核（skwy-requirement-test 再委派）。框架：Rust 内置 #[cfg(test)] + 临时 Dolt 仓库（setup_dolt 模式）+ 内存 duckdb（现有测试模式），遵循 kb/dev/testing.md
- 覆盖率门槛：compass-data per-crate 95%（scripts/check-coverage.sh:23 强制，CI 失败）——新增分支必须有对应测试
- Evidence: .omo/evidence/task-<N>-data-quality-monitor.<ext>
- 测试命令：`cargo test -p compass-data`（RED 阶段应失败）、`cargo llvm-cov --json` + `scripts/check-coverage.sh`（覆盖率门槛）

## Execution strategy
### Parallel execution waves
> 目标每 wave 5-8 todos。Wave 1 是 RED 测试（可并行），Wave 2-3 是实现（依赖 RED），Wave 4 是对抗测试 + QA + 文档。

**Wave 1（RED 测试，可并行）**
- 1. 委派 skwy-requirement-test 写需求验收失败测试（RED）
- 2. 委派 skwy-adversarial-test（预期返回 DEFERRED——无接口契约，记录后等待首个接口 commit）

**Wave 2（import 实现）**
- 3. 生产行数/日期 helper 模块（validate.rs）：read_parquet_row_count 提升 + 严格 Dolt COUNT 解析 + 日期规范化
- 4. import_dolt::run 行数校验 + 日期范围校验实现（GREEN，让 Wave 1 测试通过）

**Wave 3（import-compass 实现）**
- 5. import_compass::run 全量行数校验 + merge 不丢数据校验 + fallback 语义（GREEN）
- 6. import_compass::run 新鲜度校验（data_updates 读取 + 按表阈值 + NULL/缺失处理）（GREEN）

**Wave 4（对抗测试 + 独立 QA + 文档）**
- 7. 重新委派 skwy-adversarial-test（携带首个可编译接口 commit SHA）
- 8. 委派 skwy-requirement-test 独立 QA 复核（验证者与实现者分离）
- 9. 文档同步 + 决策记录检查

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 (RED requirement tests) | — | 4, 5, 6 (GREEN 需 RED 先行) | 2, 3 |
| 2 (RED adversarial, DEFERRED) | — | 7 (重委派) | 1, 3 |
| 3 (validate.rs helpers) | — | 4, 5, 6 | 1, 2 |
| 4 (import_dolt 校验) | 1, 3 | 7 | 5, 6 |
| 5 (import_compass 行数) | 1, 3 | 7 | 4, 6 |
| 6 (import_compass 新鲜度) | 1, 3 | 7 | 4, 5 |
| 7 (adversarial 重委派) | 4, 5, 6 (首个接口 commit) | 8 | — |
| 8 (独立 QA 复核) | 7 | 9 | — |
| 9 (文档同步) | 8 | F1-F4 | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

### Wave 1: RED tests（门禁 3.5 + 4）

- [x] 1. 委派 skwy-requirement-test 写需求验收失败测试（RED）
  What to do / Must NOT do: 委派 `task(subagent_type="skwy-requirement-test", load_skills=["skwy-requirement-test"], run_in_background=false)`，让独立 QA agent 读 plan 与 import_dolt.rs / import_compass.rs / sepa.rs，为下述契约各写失败测试（此时实现不存在 → 测试编译/运行失败或断言失败 = RED）：
  - import：行数不一致 → run() 返回 Err；日期范围不一致 → Err；limit>0 时预期 min(COUNT, limit)；一致时 Ok
  - import-compass 全量路径：行数不一致 → Err；一致 → Ok
  - import-compass merge 路径：merge 后行数 < 旧 parquet 行数 → Err（不丢数据）；正常 merge → Ok
  - import-compass 新鲜度：last_report_date 过期（> 阈值）→ 仅 warn 不 Err；新鲜 → 无 warn
  - 测试模式：临时 Dolt 仓库（setup_dolt，见 import_dolt.rs:339-366）+ 内存 duckdb（read_parquet COUNT）；RED 阶段断言目标函数/行为尚未实现
  Must NOT do: 不得写生产实现代码；不得触碰 collectors/；不得修改 plan；bash 禁 cargo run / git 写操作
  Parallelization: Wave 1 | Blocked by: — | Blocks: 4, 5, 6
  References: .omo/plans/data-quality-monitor.md（本 plan 契约）；crates/compass-data/src/import_dolt.rs:127-321（run 结构）、:300-318（现有 COUNT）；crates/compass-data/src/import_compass.rs:53-122（run dispatch）、:124-145（stock_basic WHERE symbol LIKE）、:166-232（fin_indicators）、:276-342（append_table + merge）、:1016-1024（read_parquet_row_count 测试 helper）；crates/compass-data/src/sepa.rs:73-79（UPDATES_SCHEMA）；kb/dev/testing.md（rstest + tokio::test 模式、覆盖率门槛 95%）
  Acceptance criteria: `cargo test -p compass-data` 有新失败测试（RED），失败断言明确指向缺失的校验行为；无测试编译错误（函数签名已存在或测试隔离）
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-data <new-test-name>` 输出 RED 断言失败；failure: `cargo test -p compass-data` 全量运行确认失败仅来自新测试（不破坏既有测试）。Evidence .omo/evidence/task-1-data-quality-monitor.txt
  Commit: Y | test(compass-data): add failing acceptance tests for data quality validation (RED) (ref #136)

- [x] 2. 委派 skwy-adversarial-test（预期 DEFERRED）
  What to do / Must NOT do: 委派 `task(subagent_type="skwy-adversarial-test", load_skills=["skwy-adversarial-test"], run_in_background=false)`。因本 plan 无预先定义的接口契约（校验函数签名尚未设计/提交），agent 应返回 DEFERRED 并记录：等首个可编译接口 commit（todo 3/4/5/6 完成后）携带 SHA 重新委派（todo 7）
  Must NOT do: 若 agent 判定 DEFERRED，不得强行让其在无接口下写测试；不得写生产代码
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7
  References: .omo/handoff.md（gate 3.5 规则）；AGENTS.md「门禁第 3.5 步 ADVERSARIAL TESTS」：plan 无接口契约时返回 DEFERRED，首个可编译接口 commit 后携带 SHA 重新委派
  Acceptance criteria: 返回 DEFERRED 记录（含原因：无接口契约）；若 agent 判定有可测契约则写对抗性测试（RED）
  QA scenarios: happy: 收到 DEFERRED 记录；failure: 未返回明确 DEFERRED/测试结论。Evidence .omo/evidence/task-2-data-quality-monitor.txt
  Commit: N（DEFERRED 无代码产出）

### Wave 2: import validation（GREEN）

- [x] 3. 新建 crates/compass-data/src/validate.rs：行数/日期 helper 模块
  What to do / Must NOT do: 新建模块并在 lib.rs 注册（lib.rs 现仅 3 行 pub mod）。提供生产函数（全部 pub，供 import_dolt/import_compass 调用）：
  - `parquet_row_count(path: &Path) -> Result<usize, Box<dyn Error>>`：内存 duckdb `SELECT COUNT(*) FROM read_parquet('{path}')`，从 import_compass.rs:1016-1024 测试 helper 提升；错误传播（不 unwrap）
  - `parquet_date_range(path: &Path, col: &str) -> Result<(Option<String>, Option<String>), Box<dyn Error>>`：`SELECT MIN(CAST({col} AS DATE)), MAX(CAST({col} AS DATE)) FROM read_parquet(...)` → YYYY-MM-DD 字符串（TIMESTAMP→DATE 规范化，kb/design/data-providers.md:249）
  - `dolt_count(dolt_dir: &Path, table: &str, where_clause: &str) -> Result<usize, Box<dyn Error>>`：经 run_dolt_sql_csv（import_dolt.rs:43-61）执行 `SELECT COUNT(*) AS cnt FROM {table} {where_clause}`；**严格解析**——CSV 第二行必须 parse::<usize>() 成功，失败返回 Err（禁用 import_dolt.rs:306-310 的 `.unwrap_or(0)` 静默降级，Metis BLOCKING#6）
  - `dolt_date_range(dolt_dir: &Path, table: &str, where_clause: &str, col: &str) -> Result<(Option<String>, Option<String>), Box<dyn Error>>`：`SELECT MIN({col}), MAX({col}) FROM {table} {where_clause}`；CSV 解析；NULL → None
  - `data_updates_last_report_date(dolt_dir: &Path, table: &str) -> Result<Option<String>, Box<dyn Error>>`：`SELECT last_report_date FROM data_updates WHERE table_name = '{table}'`（同 sepa.rs:819 测试模式）；无行 / NULL → Ok(None)（Metis BLOCKING#4：不止 stock_basic，fin_* 首次 do_sync 也可能 NULL）
  - `today_cn() -> NaiveDate`：Asia/Shanghai 今日 = `Utc::now().date_naive() + Duration::hours(8)`（中国无 DST 固定 +8；Metis BLOCKING#5）
  - `verify_row_count(dolt_count: usize, parquet_count: usize, table: &str) -> Result<(), Box<dyn Error>>` + `verify_date_range(dolt: (Option<String>, Option<String>), parquet: (Option<String>, Option<String>), table: &str) -> Result<(), Box<dyn Error>>`：纯逻辑对比函数，返回统一格式错误消息供测试断言——`"row count mismatch: dolt={src} parquet={dst} (table {table})"`、`"date range mismatch: dolt={min}..{max} parquet={min}..{max} (table {table})"`
  - `freshness_days(dolt_dir, table, today) -> Result<Option<i64>, Box<dyn Error>>`：读 last_report_date，与 today 差值天数（负数/未来 → 0，不 warn；Metis 边界）
  每个函数带 `#[cfg(test)]` 单测（compass-data 95% 覆盖率门槛，scripts/check-coverage.sh:23）。错误消息必须包含 table 名与双方数值。
  Must NOT do: 不改 import_dolt.rs / import_compass.rs 现有逻辑（本 todo 只新增模块）；不引入新 crate（duckdb/chrono 已在依赖）；不用 unwrap_or(0)/expect
  Parallelization: Wave 2 | Blocked by: — | Blocks: 4, 5, 6
  References: import_compass.rs:1016-1024（helper 来源）；import_dolt.rs:43-61（run_dolt_sql_csv）、:300-310（现有解析模式与陷阱）；sepa.rs:73-79（UPDATES_SCHEMA）、:819（data_updates 查询模式）；kb/design/data-providers.md:249（tradedate TIMESTAMP）；crates/compass-data/src/lib.rs（模块注册）；.omo/drafts/data-quality-monitor.md（Metis BLOCKING#4/5/6）
  Acceptance criteria: `cargo test -p compass-data validate` 全绿；`cargo clippy -p compass-data` 无新警告；`cargo check -p compass-data` 通过
  QA scenarios: happy: 临时 Dolt 建表插数，dolt_count/parquet_row_count 返回正确值，verify_row_count 一致时 Ok；failure: 篡改 parquet 文件行数（或 COUNT 查询 CSV 非数字）→ 对应函数返回 Err 且消息含 table 名。Evidence .omo/evidence/task-3-data-quality-monitor.txt
  Commit: Y | feat(compass-data): add validate module with row-count/date-range helpers (ref #136)

- [x] 4. import_dolt::run 行数校验 + 日期范围校验实现（GREEN）
  What to do / Must NOT do: 修改 import_dolt.rs run()——在步骤 5（现有 COUNT 查询，:300-318）之后、return Ok 之前插入校验（保留现有 info 日志）：
  1. 行数校验：源 `dolt_count(dolt_dir, "final_a_stock_eod_price", &where_clause)` vs `parquet_row_count(&final_path)`。**预期值**：limit==0 时 = 源 COUNT；limit>0 时 = `min(源 COUNT, limit)`（D3 决策——limit 语义"最多 N 行"，COUNT 查询无 LIMIT 是设计使然，Metis 已确认）。实际 parquet 行数必须 == 预期值，否则 `verify_row_count` 返回 Err 向上传播 → main.rs:252 "Import failed: {e}" → exit(1)
  2. 日期范围校验（**limit>0 时跳过**，D3 决策——MIN/MAX 语义被 LIMIT 破坏）：源 `dolt_date_range(dolt_dir, "final_a_stock_eod_price", &where_clause, "tradedate")` vs 目标 `parquet_date_range(&final_path, "tradedate")`。不一致 → Err。空结果处理：源 0 行 && parquet 0 行 → Ok（0==0 一致，Metis 边界#1）
  3. `--symbols` 过滤：where_clause 已含 symbol IN(...)（:224-233），COUNT/MIN/MAX 查询复用同一 where_clause → 天然一致，无需额外处理（Metis 边界#2）
  Must NOT do: 不改动数据查询/写入逻辑（:271-287 原样）；不改 --limit 的查询构造；不删现有 info 日志；不动 symbols.txt 生成
  Parallelization: Wave 2 | Blocked by: 1, 3 | Blocks: 7
  References: import_dolt.rs:127-321（run 结构）、:265-269（limit_clause）、:271-278（数据查询）、:300-318（现有 COUNT）；crates/compass-data/src/validate.rs（todo 3 产出）；kb/design/data-providers.md:249（tradedate TIMESTAMP 需 CAST）
  Acceptance criteria: `cargo test -p compass-data` 中 todo 1 写的 import 相关 RED 测试全部转 GREEN；既有 import 测试（filter_symbols/limit/since/date 系列）不回归
  QA scenarios: happy: 插入 N 行数据 → import 成功 Ok，parquet 行数 == N；failure: 构造不一致场景（如篡改 where_clause 或 limit<COUNT）→ run() 返回 Err，err 含 "row count mismatch" 或 "date range mismatch"。Evidence .omo/evidence/task-4-data-quality-monitor.txt
  Commit: Y | feat(compass-data): validate row count and date range after import (ref #136)

### Wave 3: import-compass validation（GREEN）

- [x] 5. import_compass::run 全量行数校验 + merge 不丢数据校验 + fallback 语义（GREEN）
  What to do / Must NOT do: 修改 import_compass.rs，在每张表写入路径的写盘点之后插入行数校验：
  1. **全量路径**（stock_basic :142；fin_indicators 无 since/overwrite 分支 :227；append_table 无 since/overwrite 分支 :337；concept_member :358）：写盘后 `parquet_row_count(&path)` vs `dolt_count(dolt_dir, table, date_filter)`。**COUNT 查询必须镜像完整 WHERE**（Metis BLOCKING#2）：stock_basic 的查询带 `WHERE symbol LIKE 'SH%' OR symbol LIKE 'SZ%' OR symbol LIKE 'BJ%'`（:128-139），COUNT 查询必须带同样的 symbol-prefix 条件（date_filter 为空）；fin/append 表带 `WHERE {date_col} >= '{since}'`（date_filter 非空时）。不一致 → Err
  2. **merge 路径**（fin_indicators :199-228；append_table :305-338，条件 since.is_some() && !overwrite && path.exists()）：在 merge 前记录旧 parquet 行数 `old_count = parquet_row_count(&path)`（merge 会覆盖文件，必须先读）；merge 后校验 `parquet_row_count(&path) >= old_count`（不丢数据，D1 决策）。**merge 失败 fallback 路径**（:218-220/328-330，warn 后 `std::fs::write(&path, &new_data)` 覆盖）：**跳过不丢数据校验**（Metis BLOCKING#1——fallback 是修复损坏文件的恢复机制，写的是 since 过滤数据，行数必然 < 旧 parquet；现有测试 fin_indicators_merge_failure_falls_back_to_full_export :752-795 断言 fallback 后行数 = since 窗口行数）。fallback 后改按全量口径校验：`parquet_row_count == dolt_count(含 date_filter)`
  3. **tiny-skip 路径**（new_data.len() < 500 → warn + return Ok，:194-197/300-303）：不触发行数校验（A5 决策——无文件可校验；Metis 张力#7：全量导出 0 行时源 COUNT=0 vs 无 parquet，天然一致，无需额外处理）
  Must NOT do: 不改 DuckDB merge SQL（:209-217/319-327 的 ROW_NUMBER 去重语义不动）；不改 tiny-skip 的 500 字节阈值；不改 fallback 逻辑本身（只在其后加校验）；不触碰 stock_basic 的查询列
  Parallelization: Wave 3 | Blocked by: 1, 3 | Blocks: 7
  References: import_compass.rs:124-145（stock_basic）、:166-232（fin_indicators）、:276-342（append_table + merge + fallback）、:351-361（concept_member）；crates/compass-data/src/validate.rs（todo 3 产出）；.omo/drafts/data-quality-monitor.md（Metis BLOCKING#1/2）
  Acceptance criteria: `cargo test -p compass-data` 中 todo 1 写的 import-compass 行数相关 RED 测试全部转 GREEN；既有 merge/fallback/tiny-skip 测试（:641-1606）不回归
  QA scenarios: happy: 全量导入 N 行 → Ok 且行数一致；merge 导入新数据 → Ok 且合并后行数 ≥ 旧；failure: 构造行数不一致（如 Dolt 表在导出后被清空）→ Err；merge 后篡改使行数 < 旧 → Err。Evidence .omo/evidence/task-5-data-quality-monitor.txt
  Commit: Y | feat(compass-data): validate row counts on full and merge import-compass paths (ref #136)

- [x] 6. import_compass::run 新鲜度校验（data_updates 读取 + 按表阈值 + NULL/缺失处理）（GREEN）
  What to do / Must NOT do: 在 import_compass.rs 各表导入函数写盘成功后（行数校验之后）调用新鲜度校验：
  1. 阈值表（硬编码常量，A2 决策）：`FIN_FRESHNESS_DAYS: i64 = 120`（fin_indicators/fin_balance_sheet/fin_income/fin_cash_flow）；`MARKET_FRESHNESS_DAYS: i64 = 7`（capital_main_flow/dragon_list/block_trade/institution_survey/concept_member）；`stock_basic` 跳过（A3 决策——last_report_date 为 NULL，main.py:79-85 仅写 4 列）
  2. 每个表：`data_updates_last_report_date(dolt_dir, table)` → Ok(None)（无记录 / NULL）时**跳过新鲜度校验**（Metis BLOCKING#4——首次 do_sync 或采集失败窗口）；Ok(Some(date)) 时 `freshness_days` 算差值，`days > threshold` → `warn!("freshness: {table} last_report_date {date} is {days} days old (threshold {threshold})")` 不退出（Q5 决策——新鲜度过期仅 warn）
  3. "今天" 用 `today_cn()`（Asia/Shanghai，Metis BLOCKING#5）；last_report_date 未来日期（测试 fixtures 常用 2099-12-31）→ freshness_days 返回 0，不 warn
  Must NOT do: 不做成 error（Q5 锁定 warn）；不改 data_updates 表本身；不校验 last_updated（Q4 锁定 last_report_date 口径）；不触碰 collectors
  Parallelization: Wave 3 | Blocked by: 1, 3 | Blocks: 7
  References: sepa.rs:73-79（UPDATES_SCHEMA）；collectors/common.py:284-289 + main.py:79-85/396-401（写方，各表 last_report_date 语义）；collectors/fetch_concept_member.py:289（concept_member = CURDATE()）；crates/compass-data/src/validate.rs（todo 3 产出）；.omo/drafts/data-quality-monitor.md（Metis BLOCKING#4/5）
  Acceptance criteria: `cargo test -p compass-data` 中 todo 1 写的新鲜度 RED 测试全部转 GREEN；无 warn 场景（新鲜数据）断言无 freshness warn 输出
  QA scenarios: happy: data_updates 无记录 → 无 warn 无 Err；last_report_date = today → 无 warn；failure: 构造 last_report_date = 200 天前（fin 表）→ warn 输出含 "freshness" 且 run 仍 Ok。Evidence .omo/evidence/task-6-data-quality-monitor.txt
  Commit: Y | feat(compass-data): warn on stale data_updates last_report_date during import-compass (ref #136)

### Wave 4: adversarial tests + independent QA + docs

- [x] 7. 重新委派 skwy-adversarial-test（携带首个可编译接口 commit SHA）
  What to do / Must NOT do: 在 todo 4/5/6 的实现 commit 全部落地后，取首个可编译接口 commit 的 SHA（`git log --oneline` 中首个包含 validate.rs 或 import 校验的 commit），重新委派 `task(subagent_type="skwy-adversarial-test", load_skills=["skwy-adversarial-test"], run_in_background=false)`，prompt 中携带该 SHA 与 plan 契约，要求攻击边界场景：
  - 行数校验：空表 0 行；COUNT CSV 解析失败（非数字输出）；parquet 文件缺失/损坏（duckdb COUNT 失败）；limit 恰好 == 源 COUNT（min 边界）；--symbols 过滤后 0 行
  - 日期校验：空结果 MIN/MAX 为 NULL；limit>0 跳过逻辑；tradedate TIMESTAMP 时区边界（当日 00:00）
  - merge/fallback：merge 后行数 == 旧行数（恰相等）；fallback 写入后校验口径
  - 新鲜度：阈值恰好 == 120/7（边界，不 warn）；未来日期（2099-12-31）；data_updates 表不存在（无 UPDATES_SCHEMA）；NULL last_report_date
  - 并发/资源：parallel 测试进程 unique_work_path 竞争（ref #184 教训）；大表 COUNT 性能
  Must NOT do: 若 agent 判定已有测试覆盖充分，可返回 SKIP 但必须列出已覆盖项；不得写生产代码；不得改既有测试（只能新增）
  Parallelization: Wave 4 | Blocked by: 4, 5, 6（首个接口 commit） | Blocks: 8
  References: 首个接口 commit SHA（实现后获取）；.omo/plans/data-quality-monitor.md（契约）；.omo/evidence/task-2-data-quality-monitor.txt（首轮 DEFERRED 记录）；AGENTS.md「门禁第 3.5 步」
  Acceptance criteria: 对抗性测试 RED（实现已存在 → 应全部通过）或 SKIP 记录；新增测试 `cargo test -p compass-data` 全绿
  QA scenarios: happy: 对抗性测试全部 GREEN；failure: 发现真实 bug → 记入 issue 修复后重跑。Evidence .omo/evidence/task-7-data-quality-monitor.txt
  Commit: Y（若产出测试）| test(compass-data): add adversarial edge-case tests for data validation (ref #136)

- [x] 8. 委派 skwy-requirement-test 独立 QA 复核（实现后独立验证）
  What to do / Must NOT do: 实现全部完成（todo 4/5/6 GREEN + todo 7 对抗测试落地）后，重新委派 `task(subagent_type="skwy-requirement-test", load_skills=["skwy-requirement-test"], run_in_background=false)` 做独立 QA 复核——验证者与实现者分离原则（AGENTS.md）：读最终代码 + 跑全部测试 + 独立判断覆盖缺口（对照 plan 契约逐条核对：行数/日期/新鲜度/merge/fallback/limit 全部覆盖）；独立跑 `cargo test -p compass-data` + `cargo llvm-cov --json` 检查 95% 门槛
  Must NOT do: 不写生产实现代码；不改既有测试逻辑（可报告缺口）；bash 禁 cargo run / git 写操作
  Parallelization: Wave 4 | Blocked by: 7 | Blocks: 9
  References: crates/compass-data/src/validate.rs + import_dolt.rs + import_compass.rs（最终代码）；.omo/plans/data-quality-monitor.md（契约）；kb/dev/testing.md（覆盖率门槛 compass-data 95%）
  Acceptance criteria: QA 报告列出：逐条契约核对结果（通过/缺口）+ 覆盖率检查结果；无未覆盖的 plan 契约项
  QA scenarios: happy: 全部契约项通过 + 覆盖率 ≥95%；failure: 发现缺口 → 记入 todo 修复。Evidence .omo/evidence/task-8-data-quality-monitor.txt
  Commit: N（QA 复核报告，无代码产出；缺口修复另行 commit）

- [x] 9. 文档同步 + 决策记录检查（门禁 5b + 5c）
  What to do / Must NOT do: 按 AGENTS.md「变更类型 → kb/ 文件映射表」同步文档：
  - kb/user/cli.md：import 章节新增「数据质量校验」说明（写盘后行数/日期范围校验，不一致 error 退出，limit>0 时行数预期 = min(源COUNT, limit) 且日期校验跳过）；import-compass 章节新增「全量路径行数校验 / merge 不丢数据 / data_updates 新鲜度 warn」说明（含阈值 120/7 天）
  - kb/dev/database.md：data_updates 消费方新增 import-compass 新鲜度校验（原有：collectors 锚点 + sepa_daily.sh）
  - kb/design/data-providers.md：如 schema 说明有出入（tradedate TIMESTAMP 校验需 CAST）补一句
  - 检查相关 kb/design/ 文件（data-providers.md）含 `## 决策记录` 章节（门禁 5c）——缺失则补齐
  Must NOT do: 不修改 AGENTS.md（除非流程级教训，本期不需要）；不写超出上述文件的文档
  Parallelization: Wave 4 | Blocked by: 8 | Blocks: F1-F4
  References: kb/user/cli.md:28-111（import/import-compass 章节）；kb/dev/database.md:40/88（data_updates）；kb/design/data-providers.md:247-249（tradedate TIMESTAMP）；AGENTS.md「变更类型 → kb/ 文件映射表」+ 门禁 5b/5c
  Acceptance criteria: 三个 kb 文件更新完成；data-providers.md 含 `## 决策记录` 章节（无则补齐）；`grep -c "数据质量\|新鲜度" kb/user/cli.md` > 0
  QA scenarios: happy: 逐文件 diff 检查文档与实现行为一致；failure: 文档与实现不符 → 修正。Evidence .omo/evidence/task-9-data-quality-monitor.txt
  Commit: Y | docs: sync cli/database/data-providers for data quality validation (ref #136)

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- 每 todo 独立 commit，消息含 `ref #136`（独立成行，AGENTS.md 规则）；类型前缀：test / feat / docs
- commit → review 循环（/review-work，最多 2 轮修复）
- 全部 commit 后 rebase origin/master（worktree 基线已确认 behind 0），等用户确认 push 才 push
- push 前 /skwy-reflect 写反思 commit（ref #119 教训），随 PR 同批推送
- push 成功后追加完成 comment + 关闭 issue #136（HARD BLOCK：只在 push 后关闭）

## Success criteria
- `import` 与 `import-compass` 写盘后自动校验，不一致时 error 退出（exit 1），新鲜度过期仅 warn
- 全部既有测试不回归；新测试全绿；compass-data 覆盖率 ≥95%
- 三个 kb 文档已同步；issue #136 收尾（完成 comment + 关闭）
