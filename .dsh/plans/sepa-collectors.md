# sepa-collectors - Work Plan（东方SEPA · 数据就绪层）

> 执行计划 1/3 — 覆盖 Batch 1+2（epic #139 子 issue #140-#146）
> 依赖：无上游。产出被 sepa-engine（plan 2）与 sepa-delivery（plan 3）消费。
> 配套：`.omo/plans/sepa-engine.md`（引擎）、`.omo/plans/sepa-delivery.md`（交付）、`.omo/plans/sepa.md`（生命周期跟踪表）、`.omo/designs/sepa-gui.md`（GUI 设计）

## TL;DR (For humans)

**What you'll get:** SEPA 系统的数据地基——① 5 个 Python 采集器每天从东财拉取主力资金流/龙虎榜/大宗交易/机构调研/概念成分股，写入 Dolt compass_data 仓库；② Rust 数据层扩展（日线横截面新增开高低收/成交额 4 字段 + 5 张新表的读取原语）；③ import-compass 命令行支持 5 张新表的 Parquet 增量导入（成分表全量覆盖）。数据从"有"变"齐"。

**Why this approach:** 采集器全部照抄现有 fetch_income.py 重构范本（common.py 复用、data_updates 增量、失败回滚），零新机制；东财 5 个接口 4 个同构（datacenter-web），概念成分独立处理；成分表用全量覆盖导入避免删除不传播（审查修订）；数据读取原语只扩展 ParquetReader 自身（审查修订授权），不动 DuckDbProvider。

**What it will NOT do:** 不采集东财官方概念板块行情指数（板块行情由引擎本地等权聚合，plan 2）；不做历史批量回算；不新增外部 Python 依赖（若东财接口确需新依赖，暂停并请示）；不修改 DuckDbProvider/现有 screener 行为。

**Effort:** Large
**Risk:** Medium - 5 个东财接口字段可能随源站变动；Dolt 导入失败回滚路径需测全
**Decisions to sanity-check:** concept_member 全量覆盖导入（非增量 merge）、ParquetReader 读取原语授权范围（仅自身）、symbol 带交易所前缀约定（SH/SZ/BJ）

Your next move: 批准后在 worktree 内按 Wave 1→2 执行；每子 issue 一个 commit（ref #N）。

---

> TL;DR (machine): Large effort, 7 todos in 2 waves, 5 Python collectors (EastMoney datacenter) + Rust data layer (CrossSectionBar +4 fields, 5 read primitives) + import-compass 5 tables, zero new deps, all → Dolt compass_data.

## Scope
### Must have
- 5 个 Python collector 脚本（fetch_main_flow / fetch_dragon / fetch_block_trade / fetch_institution_survey / fetch_concept_member），全部照抄 `collectors/fetch_income.py` 重构范本
- Dolt compass_data 5 张新表（DDL 见各 todo；symbol 带交易所前缀 SH/SZ/BJ，复合主键，update_date，登记 data_updates）
- main.py 注册 4 触点（dispatch_fetch elif / dispatch_import elif / do_sync / argparse choices×2）
- Python 测试：每 collector 1 个测试文件（TestRun stub session + TestImportToDolt 真实 temp Dolt），覆盖率 ≥80%
- CrossSectionBar 扩展 open/high/low/amount 4 字段 + fetch_cross_section SQL 更新 + 5 张新表读取原语（fetch_concept_member 等，仿 fetch_cross_section 模式）
- import-compass CompassTable 枚举 + 5 变体 + FromStr + run() 分发；4 资金表增量合并、concept_member 全量覆盖
- 文档：kb/design/data-providers.md 决策记录（CrossSectionBar 字段集 + 读取原语取舍）
- 既有测试兼容：compass-strategy screener 集成测试、compass-core 测试全绿

### Must NOT have (guardrails, anti-slop, scope boundaries)
- **不采集概念板块行情**（concept_daily 由引擎本地聚合，plan 2 todo 9）
- **不新增外部 Python 依赖**；不复制 common.py 函数（一律 import 复用）；不用 .state.json（走 data_updates 增量）
- 不改 DuckDbProvider（仅扩展 ParquetReader 自身读取方法）；不改现有 run_screener 行为
- 不做历史批量回算；不做 GUI/CLI/引擎任何实现（plan 2/3 范围）
- 每子 issue 一个 commit（ref #N）；master 不直推实现

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（先写失败测试再实现）
  - Python: pytest + StubSession（conftest.py 既有 fixture）+ 真实 temp Dolt（dolt_env fixture 既有）
  - Rust: rstest / tokio::test + tempdir DuckDB COPY parquet fixture（tests/screener.rs 既有模式）
- Evidence: `.omo/evidence/sepa-collectors/task-<N>-sepa-collectors.<ext>`（attemptDir = .omo/evidence/）
- 质量门：`cd collectors && uv run pytest --cov=. --cov-fail-under=80 tests/` + `ruff check`；`cargo test -p compass-core -p compass-strategy -p compass-data` + `cargo clippy` + `cargo fmt --check` + `cargo doc --no-deps`（#![warn(missing_docs)]）
- **前置风险登记**：master 基线 `run_screener_emits_completion_log` 为 flaky（顺序敏感，open issue #138）——执行前先修 #138 或登记豁免，否则 todo 6 验收 `cargo test -p compass-strategy` 会被卡住

## Execution strategy
### Parallel execution waves
- Wave 1: todos 1-5（5 collectors 相互独立，并行）
- Wave 2: todos 6-7（数据层两任务独立，可在 Wave 1 后期并行启动）

### Dependency matrix（本 plan 内部 + 跨 plan）
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1-5 (collectors) | — | 7 | 互相 |
| 6 (CrossSectionBar+读取原语) | — | plan2 todo 8/9/10, plan3 todo 13 | 1-5, 7 |
| 7 (import-compass) | 1-5（Dolt 表已建） | plan2 todo 9/10 | 6 |

跨 plan 依赖：**plan 2（sepa-engine）全部 3 个 todo 依赖本 plan todo 6**（读取原语）与 todo 7（表就绪）；**plan 3 GUI todo 13 依赖 todo 6**。

## Todos

- [ ] 1. collector: 主力资金流采集（capital_main_flow）— issue #140
  What to do / Must NOT do:
  新建 `collectors/fetch_main_flow.py`，逐行照抄 `fetch_income.py` 重构范本结构：
  - 模块常量：`REPORT_NAME = "RPT_MAIN_MONEY_FLOW"`、`FILTER_COLUMN = "TRADE_DATE"`（注意：主力资金流按交易日过滤，非财报期）、`DOLT_TABLE = "capital_main_flow"`
  - DDL（内嵌 CREATE TABLE 字符串，末尾主键）：
    ```sql
    CREATE TABLE capital_main_flow (
      symbol VARCHAR(20) NOT NULL,
      trade_date DATE NOT NULL,
      main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE,
      super_large_net DOUBLE, large_net DOUBLE,
      medium_net DOUBLE, small_net DOUBLE,
      update_date DATE,
      PRIMARY KEY (symbol, trade_date)
    );
    ```
  - COLS 列表（INSERT 用，不含 symbol/trade_date）：main_net_inflow, main_net_inflow_rate, super_large_net, large_net, medium_net, small_net, update_date
  - `async def run(years=None, page_size=100) -> Path`：复用 common.py 的 `AsyncSession(impersonate="chrome142")` / `fetch_paginated(session, throttle, REPORT_NAME, FILTER_COLUMN, trade_date, page_size)` / `write_csv` / `last_report_date(DOLT_TABLE)` 增量（只拉上次之后的日子）；逐交易日循环（日期从数据源返回的 TRADE_DATE 值推进，不自行生成交易日历）；stderr 进度；单日失败 catch + continue
  - `def import_to_dolt(csv_path=None) -> int`：`dolt_table_import("_tmp_mf", csv_path)` → 旧表 RENAME `_tmp_mf_old` → `dolt_sql(DDL)` 建新表 → `INSERT INTO capital_main_flow (symbol, trade_date, ...) SELECT CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE,'.',-1)), SECURITY_CODE), TRADE_DATE, ... FROM _tmp_mf WHERE symbol IN (SELECT symbol FROM stock_basic)`（symbol 拼接产生 `SH600519` 前缀格式）→ 失败回滚（DROP 新表 + RENAME 还原）→ 成功 DROP 临时表 → data_updates 5 列 upsert（source = `'EastMoney datacenter RPT_MAIN_MONEY_FLOW'`，last_report_date = MAX(TRADE_DATE)）
  - `__main__`：argparse（--years/--page-size）+ asyncio.run
  - main.py 注册 4 触点：dispatch_fetch elif（L186-216 链内加 `fetch_main_flow`）、dispatch_import elif（L221-237）、do_sync 步骤（L242 后）、choices 两处（L308 fetch、L315 import）
  测试文件 `collectors/tests/test_main_flow.py`：TestRun（patch AsyncSession 为 stub，monkeypatch asyncio.sleep + chdir tmp + COMPASS_DATA_DIR 指向不存在目录触发增量短路）+ TestImportToDolt（dolt_env fixture 真实 Dolt：建 stock_basic/data_updates → 跑 import_to_dolt → 断言表行数/symbol 前缀/data_updates 5 列/失败回滚）
  Must NOT: 不复制 common.py 函数（import 复用）；不用 .state.json；不自行生成交易日历（用接口返回的 TRADE_DATE 推进）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7
  References (executor has NO interview context - be exhaustive):
  - 范本: `collectors/fetch_income.py:27-237`（常量 L27-30 / DDL L32-82 / COLS L84-96 / run L99-155 / import_to_dolt L158-221 / __main__ L224-237）
  - common.py: `collectors/common.py:67-136`（dolt_dir L67 / dolt_sql L94 / dolt_sql_csv L100 / dolt_table_import L107 / last_report_date L122）、`collectors/common.py:38-61`（EM_BASE/EM_HEADERS/EM_* 常量）、`collectors/common.py:154-237`（fetch_paginated）
  - 注册触点: `collectors/main.py:186-239`（dispatch_fetch/import）、`collectors/main.py:242-296`（do_sync）、`collectors/main.py:306-316`（choices）
  - 测试设施: `collectors/tests/conftest.py:13-99`（StubResponse/StubSession/make_stub_session）、`collectors/tests/test_income.py:46-171`（TestImportToDolt + dolt_env）、`collectors/tests/test_income.py:177-273`（TestRun 三件套）
  - 文档: `kb/user/cli.md:152-176`（采集器章节）
  - 表结构约定: epic #139 body 决策 13（symbol 带前缀/复合主键/update_date/data_updates）
  Acceptance criteria (agent-executable):
  - `cd collectors && uv run pytest tests/test_main_flow.py -q` 全绿（TestRun + TestImportToDolt 两组）
  - `cd collectors && uv run pytest --cov=. --cov-fail-under=80 tests/ -q` 通过（全量回归，防 main.py 触点破坏其他 collector）
  - `cd collectors && uv run ruff check fetch_main_flow.py tests/test_main_flow.py`
  - main.py 三处注册点存在（grep dispatch_fetch/main_flow、dispatch_import/main_flow、choices/main_flow）
  QA scenarios:
  - happy: `uv run pytest tests/test_main_flow.py::TestRun -q`（stub session 验证 CSV 生成与增量短路）；TestImportToDolt 验证 Dolt 表行数 + symbol 前缀 `SH600519` 格式 + data_updates 5 列 upsert
  - failure: TestImportToDolt 注入 DDL 失败 → 断言回滚（`_tmp_mf_old` 还原、新表不存在、无残留）
  - 幂等: 同日期 CSV 重跑 import_to_dolt → 行数不增（DELETE+重写语义）
  - Evidence: `.omo/evidence/sepa-collectors/task-1-sepa-collectors.txt`（记录测试输出 + 断言结果）
  Commit: Y | feat(collectors): add main flow collector

- [ ] 2. collector: 龙虎榜采集（dragon_list）— issue #141
  What to do / Must NOT do:
  新建 `collectors/fetch_dragon.py`，同范本结构：
  - 常量：`REPORT_NAME = "RPT_DAILYBILLBOARD_DETAILSNEW"`、`FILTER_COLUMN = "TRADE_DATE"`、`DOLT_TABLE = "dragon_list"`
  - DDL：
    ```sql
    CREATE TABLE dragon_list (
      symbol VARCHAR(20) NOT NULL,
      trade_date DATE NOT NULL,
      seat_type VARCHAR(10) NOT NULL,
      buy_amount DOUBLE, sell_amount DOUBLE, net_amount DOUBLE,
      institution_flag TINYINT,
      update_date DATE,
      PRIMARY KEY (symbol, trade_date, seat_type)
    );
    ```
    （三主键：一股一天可多个席位类型；institution_flag=1 表示机构席位）
  - run/import_to_dolt/__main__ 同 todo 1 模式（symbol 拼接、stock_basic 过滤、回滚、data_updates 5 列 upsert，source=`'EastMoney datacenter RPT_DAILYBILLBOARD_DETAILSNEW'`）
  - main.py 4 触点注册 + `tests/test_dragon.py`（TestRun + TestImportToDolt）
  Must NOT: 同 todo 1（不复制函数/不用 state.json/不自行生成交易日历）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7
  References: 同 todo 1 全部 + 注意 seat_type 取接口字段（如 '机构专用'/'营业部'，实现时以实际返回为准并保留原始字符串）
  Acceptance criteria (agent-executable): `uv run pytest tests/test_dragon.py -q` 全绿；覆盖率门槛通过；ruff 干净
  QA scenarios: 同 todo 1 三件套（happy/failure 回滚/幂等）。Evidence: `.omo/evidence/sepa-collectors/task-2-sepa-collectors.txt`
  Commit: Y | feat(collectors): add dragon list collector

- [ ] 3. collector: 大宗交易采集（block_trade）— issue #142
  What to do / Must NOT do:
  新建 `collectors/fetch_block_trade.py`，同范本结构：
  - 常量：`REPORT_NAME = "RPT_BLOCKTRADE_DETAILS"`、`FILTER_COLUMN = "TRADE_DATE"`、`DOLT_TABLE = "block_trade"`
  - DDL：
    ```sql
    CREATE TABLE block_trade (
      symbol VARCHAR(20) NOT NULL,
      trade_date DATE NOT NULL,
      price DOUBLE NOT NULL,
      volume DOUBLE, amount DOUBLE,
      buyer VARCHAR(100), seller VARCHAR(100),
      premium_rate DOUBLE,
      update_date DATE,
      PRIMARY KEY (symbol, trade_date, price)
    );
    ```
    （三主键：一股一天可多笔不同价格；buyer/seller 为席位名称字符串）
  - run/import_to_dolt/__main__ 同 todo 1；main.py 4 触点 + `tests/test_block_trade.py`
  Must NOT: 同 todo 1。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7
  References: 同 todo 1
  Acceptance criteria (agent-executable): `uv run pytest tests/test_block_trade.py -q` 全绿；覆盖率门槛；ruff 干净
  QA scenarios: 同 todo 1 三件套。Evidence: `.omo/evidence/sepa-collectors/task-3-sepa-collectors.txt`
  Commit: Y | feat(collectors): add block trade collector

- [ ] 4. collector: 机构调研采集（institution_survey）— issue #143
  What to do / Must NOT do:
  新建 `collectors/fetch_institution_survey.py`，同范本结构：
  - 常量：`REPORT_NAME = "RPT_ORG_SURVEYNEW"`、`FILTER_COLUMN = "NOTICE_DATE"`（机构调研按公告日过滤，以接口实际支持字段为准，若为 SURVEY_DATE 则用之）、`DOLT_TABLE = "institution_survey"`
  - DDL：
    ```sql
    CREATE TABLE institution_survey (
      symbol VARCHAR(20) NOT NULL,
      survey_date DATE NOT NULL,
      org_name VARCHAR(100) NOT NULL,
      survey_type VARCHAR(20),
      update_date DATE,
      PRIMARY KEY (symbol, survey_date, org_name)
    );
    ```
    （三主键：一股一天可多家机构调研）
  - run/import_to_dolt/__main__ 同 todo 1；main.py 4 触点 + `tests/test_institution_survey.py`
  Must NOT: 同 todo 1。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7
  References: 同 todo 1
  Acceptance criteria (agent-executable): `uv run pytest tests/test_institution_survey.py -q` 全绿；覆盖率门槛；ruff 干净
  QA scenarios: 同 todo 1 三件套。Evidence: `.omo/evidence/sepa-collectors/task-4-sepa-collectors.txt`
  Commit: Y | feat(collectors): add institution survey collector

- [ ] 5. collector: 概念板块成分采集（concept_member）— issue #144
  What to do / Must NOT do:
  新建 `collectors/fetch_concept_member.py`（**独立于其他 4 个：版本跟踪语义，非日频增量**）：
  - 常量：`REPORT_NAME = "RPT_F10_CORETHEME_BOARDTYPE"`（概念板块成分接口）、`DOLT_TABLE = "concept_member"`
  - 板块列表先取（板块代码 ↔ 名称映射；以接口实际返回为准，如 BOARD_CODE/BOARD_NAME 字段），再逐板块拉成分股
  - DDL：
    ```sql
    CREATE TABLE concept_member (
      concept_code VARCHAR(20) NOT NULL,
      symbol VARCHAR(20) NOT NULL,
      concept_name VARCHAR(50),
      update_date DATE,
      PRIMARY KEY (concept_code, symbol)
    );
    ```
  - 写入语义：**版本跟踪非每日快照**——每次运行 `DELETE FROM concept_member`（清空旧版本）→ 全量 INSERT 当前成分（更新 update_date = CURDATE()）；**不做**按交易日追加
  - data_updates 5 列 upsert（last_report_date = CURDATE()）
  - main.py 4 触点 + `tests/test_concept_member.py`（重点：版本更新幂等——重跑后行数不变、被移除成分不复存在）
  Must NOT: **不采集概念板块行情**（concept_daily 由引擎本地聚合，plan 2 todo 9）；不逐板块 K 线抓取；不按交易日版本快照（只保留当前版本）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7
  References: 同 todo 1 + epic #139 body 决策 8/20（概念板块口径、版本跟踪非每日快照）
  Acceptance criteria (agent-executable): `uv run pytest tests/test_concept_member.py -q` 全绿；覆盖率门槛；ruff 干净
  QA scenarios: happy: TestImportToDolt 断言行数与 PK 唯一；幂等: 重跑 import_to_dolt → 行数不变；**版本更新: 先插 50 成分再以 45 成分重跑 → 断言被移除 5 只不复存在**（删除传播）。Evidence: `.omo/evidence/sepa-collectors/task-5-sepa-collectors.txt`
  Commit: Y | feat(collectors): add concept member collector

- [ ] 6. data: CrossSectionBar 扩展 open/high/low/amount + 新表读取原语 — issue #145
  What to do / Must NOT do:
  **A. 字段扩展**：
  - `crates/compass-core/src/model.rs:137-148` CrossSectionBar 增加 4 字段：`pub open: f64, pub high: f64, pub low: f64, pub amount: f64`（现有 5 字段 symbol/trade_date/adjclose/close/volume 不变）
  - `crates/compass-core/src/data/parquet.rs:373-428` fetch_cross_section 的 SELECT 增加 `open, high, low, amount`（列序与 parquet 文件一致：symbol, tradedate, open, high, low, close, adjclose, volume, amount——已验证 stock_daily.parquet 实际含 9 列）
  **B. 新表读取原语（审查修订授权）**：
  - ParquetReader 新增 5 个方法（仿 fetch_cross_section 模式：`read_parquet('...')` + 内存 DuckDB 查询，返回 Vec 结构体，无 WHERE 则全量）：
    ```rust
    pub fn fetch_concept_member(&self) -> Result<Vec<ConceptMember>, DataError>;
    pub fn fetch_capital_main_flow(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<CapitalMainFlow>, DataError>;
    pub fn fetch_dragon_list(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<DragonListRow>, DataError>;
    pub fn fetch_block_trade(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<BlockTradeRow>, DataError>;
    pub fn fetch_institution_survey(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<InstitutionSurveyRow>, DataError>;
    ```
  - model.rs 新增 5 个结构体（字段对齐 Dolt DDL：ConceptMember{concept_code, symbol, concept_name, update_date}、CapitalMainFlow{symbol, trade_date, main_net_inflow, main_net_inflow_rate, super_large_net, large_net, medium_net, small_net, update_date}、DragonListRow{...seat_type, buy_amount, sell_amount, net_amount, institution_flag...}、BlockTradeRow{...price, volume, amount, buyer, seller, premium_rate...}、InstitutionSurveyRow{...survey_date, org_name, survey_type...}）
  - 文件路径约定：与 stock_daily.parquet 同目录（ParquetReader 已知 parquet_dir），文件名 = 表名 + `.parquet`
  **C. fixture 同步**：
  - `crates/compass-strategy/tests/screener.rs:14-97`：build_fixture 已写全 9 列（open=close-1 等派生）——确认 TestBar 结构体与 INSERT 参数覆盖新字段；若测试需要断言新字段值，扩展 TestBar 加 open/high/low/amount 字段
  - `crates/compass-core` 内部测试（如有构造 CrossSectionBar 处）同步
  **D. 文档**：`kb/design/data-providers.md` 决策记录新增两行：① CrossSectionBar 字段集从 5 → 9（open/high/low/amount 加入的理由：SEPA 形态/ATR/成交额因子需要）；② 读取原语扩展取舍（新增 ParquetReader 自身方法而非 DuckDbProvider，避免三处 impl 牵连）
  Must NOT: **不改 DuckDbProvider**（仅允许扩展 ParquetReader 自身——引擎读表路径的唯一授权）；不加新依赖；不改 DuckDbProvider DDL/SCHEMA_SQL；不破坏现有 screener 行为（字段增加向后兼容）。
  Parallelization: Wave 2 | Blocked by: — | Blocks: plan2 todo 8/9/10, plan3 todo 13
  References: `crates/compass-core/src/model.rs:137-148`（CrossSectionBar）、`crates/compass-core/src/data/parquet.rs:373-428`（fetch_cross_section SQL）、`crates/compass-core/src/data/parquet.rs:314-359`（load_all_stock_basics 读取模式参考）、`crates/compass-strategy/tests/screener.rs:14-97`（build_fixture）、`crates/compass-strategy/tests/screener.rs:14-19`（TestBar）、`kb/design/data-providers.md`（决策记录章节）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-core -p compass-strategy` 全绿（含新增字段断言测试 + 既有 screener 回归）
  - `cargo clippy -p compass-core` 干净；`cargo fmt --check` 通过
  - 新测试：tempdir parquet fixture → fetch_cross_section 返回 9 字段且 open/high/low/amount 值正确；5 个新读取原语各自返回结构体行数正确
  QA scenarios:
  - happy: 新增 `#[cfg(test)]` 测试——tempdir 写含 9 列的 parquet → fetch_cross_section 断言 9 字段值；5 原语各写最小 fixture 断言行数与字段
  - regression: 既有 screener 集成测试全绿（字段扩展不破坏）
  - boundary: 新表 parquet 缺失 → **锁定返回空 Vec**（与 fetch_cross_section 缺文件行为 parquet.rs:378-380 一致，审查修订；**禁止**返回 DataError——否则 run_sepa 的 `?` 会在 GUI 表未导入时直接失败，无法优雅降级）
  - Evidence: `.omo/evidence/sepa-collectors/task-6-sepa-collectors.txt`
  Commit: Y | feat(core): extend CrossSectionBar and add SEPA table readers

- [ ] 7. data: import-compass 支持 5 张 SEPA 新表 — issue #146
  What to do / Must NOT do:
  扩展 `crates/compass-data/src/import_compass.rs`：
  - `CompassTable` 枚举（L13-20）加 5 变体：`ConceptMember` / `MainFlow` / `DragonList` / `BlockTrade` / `InstitutionSurvey`
  - `FromStr`（L22-34）加 5 映射：`"concept_member"` / `"capital_main_flow"` / `"dragon_list"` / `"block_trade"` / `"institution_survey"`
  - `run()` match（L37-57）加 5 分发
  - **导入策略分两类（审查修订）**：
    - (a) 4 张资金表照抄 `import_financial_table` 增量合并模式（L152-208）：tiny-data 守卫 `new_data.len() < 500` → warn 跳过；`since.is_some() && !overwrite && path.exists()` → DuckDB `ROW_NUMBER() OVER (PARTITION BY symbol, trade_date ORDER BY priority)` + UNION ALL（旧=1 新=2）→ `WHERE rn=1` → COPY TO parquet；DuckDB 失败 warn + 回退全量；since 过滤列 = trade_date
    - (b) **concept_member 必须全量覆盖导入（不增量 merge）**——成分表是 DELETE+重写语义，ROW_NUMBER 合并会让被移除的成分股残留 parquet（删除不传播 → 题材评分用过时成分）；实现为始终全量直写（等价 --overwrite；数据量 ~1.5 万行全量成本可忽略）
  - 测试：`crates/compass-data/src/import_compass.rs` 内嵌 `#[cfg(test)]`（既有模式 L209-774）：temp Dolt 建 5 表 → 跑 run() → DuckDB read_parquet 断言；重点新增 **concept_member 删除传播测试**（旧 parquet 含 50 成分 → 新 Dolt 45 成分 → import 后 parquet 断言 45 行、被移除 5 只不存在）
  Must NOT: 不动采集层 data_updates（collectors 负责登记，import-compass 只读 Dolt）；不添加 concept_daily（引擎聚合产物不走 import-compass）；不改 4 张资金表的增量语义。
  Parallelization: Wave 2 | Blocked by: 1-5 | Blocks: plan2 todo 9/10
  References: `crates/compass-data/src/import_compass.rs:13-57`（枚举/FromStr/run）、`crates/compass-data/src/import_compass.rs:83-150`（import_fin_indicators）、`crates/compass-data/src/import_compass.rs:152-208`（import_financial_table 增量合并）、`crates/compass-data/src/import_compass.rs:229-256`（setup_dolt 测试模式）、`crates/compass-data/src/import_dolt.rs:40`（run_dolt_sql_parquet）
  Acceptance criteria (agent-executable):
  - `cargo test -p compass-data import_compass` 全绿（含 5 新表导入 + concept_member 删除传播 + 4 资金表增量合并测试）
  - `cargo test -p compass-data` 覆盖率门槛（llvm-cov ≥80%）
  - `cargo clippy -p compass-data` 干净
  QA scenarios:
  - happy: temp Dolt 建表插数 → run() → read_parquet 断言行数与字段
  - incremental: 资金表 since 合并——旧 parquet + 新 Dolt 同 PK → 断言新值覆盖旧值（priority 2 胜）
  - **删除传播: concept_member 50→45 成分 → 断言 parquet 45 行、无残留**
  - failure: 空数据守卫（len<500 → warn 跳过不崩）
  - Evidence: `.omo/evidence/sepa-collectors/task-7-sepa-collectors.txt`
  Commit: Y | feat(data): import-compass support SEPA tables

## Final verification wave（本 plan）
> 并行运行，全部 APPROVE 后进入 plan 2。Surface results 并等用户确认。
- [ ] F1. 合规审计: 5 collector 照抄范本（grep 确认 import common.py 而非复制）；4 资金表增量 + concept_member 全量（代码走查）；读取原语只在 ParquetReader 自身（无 DuckDbProvider 改动）；无新增 Python 依赖（pyproject.toml diff）
- [ ] F2. 质量门: Python `uv run pytest --cov=. --cov-fail-under=80 tests/` + `ruff check`；Rust `cargo test -p compass-core -p compass-strategy -p compass-data` + `clippy` + `fmt --check` + `doc --no-deps`
- [ ] F3. 真实数据冒烟: 真实 Dolt compass_data 上运行 1 个 collector（如 fetch_main_flow）拉最近 3 日数据 → dolt 表可查 → data_updates 更新（客观验证：`dolt sql -q "SELECT * FROM capital_main_flow LIMIT 5"` + data_updates 行）；真实 Parquet 上跑 import-compass 1 张表
- [ ] F4. 范围保真: 无 concept_daily 采集、无 state.json、无 DuckDbProvider 改动、无历史回算

## Commit strategy（本 plan）
- todo 1-5 各 1 commit（`feat(collectors): ...` + `ref #140`~`#144`）、todo 6（`feat(core): ...` + `ref #145`）、todo 7（`feat(data): ...` + `ref #146`）
- 每 commit 后 /review-work（5 agent）；发现问题修复重 commit（≤2 轮）
- 全部 7 commit 在一个 worktree（.worktrees/sepa/）、一个 PR；push 前 rebase origin/master
- Dolt 数据 commit（采集结果）由 sepa_daily.sh（plan 3 todo 12）统一处理，本 plan 开发期手动 dolt add/commit/push 验证即可（AGENTS.md 规范）

## Success criteria（本 plan）
- 5 collector + 5 测试文件全绿，覆盖率 ≥80%；Dolt 5 表可查（真实数据冒烟通过）
- CrossSectionBar 9 字段 + 5 读取原语 + 既有测试兼容
- import-compass 5 表导入正确（含 concept_member 删除传播验证）
- kb/design/data-providers.md 决策记录更新
- F1-F4 全部 APPROVE → 解锁 plan 2（sepa-engine）
