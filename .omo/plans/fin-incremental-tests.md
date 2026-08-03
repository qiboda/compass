# 测试计划：Issue #160 — 财务四表 merge 增量语义（RED → GREEN）

Ref: https://github.com/qiboda/compass/issues/160 · Worktree: feat/fin-incremental
上游计划: `.omo/plans/fin-incremental-merge.md`（本文件 = 其 Wave 1 的细化执行契约）

**本文件只做规划。不写生产代码、不写测试代码、不改任何源文件。** 经批准后按
RED → GREEN 执行。

---

## 0. 锁定设计（不得偏离）

被测试的修复（GREEN 阶段实现）：

1. `fetch_balance_sheet.py` / `fetch_income.py` / `fetch_cash_flow.py` 的 `import_to_dolt()`
   重构为 `common.import_replace_table(merge=True)` 薄包装：
   - tmp 表 `_tmp_bs` / `_tmp_inc` / `_tmp_cf`，`insert_sql` 为
     `INSERT IGNORE INTO {DOLT_TABLE} (symbol, report_date, {COLS}) SELECT ... FROM {tmp} WHERE <derived-symbol> IN (SELECT symbol FROM stock_basic)`，
     `last_report_expr="MAX(report_date)"`；
   - DDL 首行改 `CREATE TABLE IF NOT EXISTS {DOLT_TABLE}`（merge 流程以 DDL returncode==0
     作为继续标志，plain CREATE TABLE 在表已存在时失败 → 静默返回 0 跳过导入）。
2. `main.py::_import_fin_indicators()` → 同样改为
   `import_replace_table(merge=True, tmp="_tmp_fin", dolt_table="fin_indicators", ...)`，
   新增 `FIN_INDICATORS_DDL`（镜像 live `dolt schema show fin_indicators`，
   `CREATE TABLE IF NOT EXISTS` + `PRIMARY KEY (symbol, report_date)`）。
3. `run()` fetch 逻辑 **不动**：`since = last_report_date(DOLT_TABLE)` 读 data_updates，
   `all_dates = [d for d in all_dates if d >= since]`（`>=` 表示最新报告期会被重抓），
   CSV 每次运行从首个非空写入起以 `mode="w"` 覆盖。

merge 语义：CSV 行 INSERT IGNORE 进**已存在**的表，PK `(symbol, report_date)` 去重；
watermark（data_updates）`row_count = COUNT(*) 全表`、`last_report_date = MAX(report_date)`；
失败路径保留已有行、不写 watermark、无 tmp 残留。

---

## 1. 测试策略：RED / PIN 分类总表

关键事实（已核实，决定 RED 真实性）：

| 事实 | 结论 |
|---|---|
| `common.import_replace_table(merge=True)` 已在 common.py:209-215 实现（dragon/block_trade/main_flow/institution_survey 已在用） | common 级 merge 测试**全部是 PIN**（pre-fix 就绿，锁定 primitive 契约） |
| 三表 `import_to_dolt()` 现为整表替换（RENAME aside → DROP → CREATE → INSERT SELECT） | 追加/历史保留/值保留/全表 watermark 测试**是 RED**（替换语义违反断言） |
| `_import_fin_indicators()` 现为 `DELETE FROM` + INSERT，**无错误处理无回滚** | 历史保留测试 RED；**插入失败测试 RED**（DELETE 已清空、INSERT 失败 → 0 行） |
| `run()` 首写 `append=not first_write` → `mode="w"` 覆盖 | CSV 覆盖测试**是 PIN**（pre-fix 已覆盖陈旧 CSV；答案：历史在 Dolt 不在 CSV） |
| 同 CSV 重跑：replace 与 merge 观测等价（都是 1 行） | 幂等测试 PIN（原 `test_rerun_replaces_table_without_duplicates` 的延续） |
| rerun 插入失败：replace 靠 RENAME 回滚保数据，merge 靠"从未触碰"保数据 | 该失败测试 PIN（两种语义下断言均成立） |

**诚实说明（任务 #7 要求）**：本计划中 ~2/3 的测试 pre-fix 就通过。它们是
**characterization/pin 测试**——把修复后的契约（以及用户 grill 关心的"CSV 会不会被
覆盖"、"历史数据从哪里来"）固化成回归防线；若 GREEN 实现倒退（如有人把 DDL 改回
plain CREATE TABLE、把 INSERT IGNORE 改回 INSERT、把 merge 改回 replace），这些 pin
测试会在未来失败。真正 pre-fix 失败的 **RED 测试**约 10 个，全部集中在 collector
import 层与 fin_indicators（见 §6 证据命令）。

---

## 2. 通用 fixture 约定（沿用现有模式）

所有 Dolt-tempdir 测试沿用 `dolt config --global` + `dolt --data-dir tmp_path init` +
`CREATE TABLE stock_basic` + `CREATE TABLE data_updates` + `monkeypatch.setenv("COMPASS_DATA_DIR", ...)`
模式（见 test_common.py TestImportReplaceTable.dolt_env、test_balance_sheet.py
TestImportToDolt.dolt_env）。**唯一改动**：stock_basic 种子数据增加第二个标的
`('SZ000002')`（merge 追加测试需要新标的能通过过滤；不影响现有只用 SZ000001 的测试）。

辅助方法沿用 `_last()` / `_rows()` / `_write_csv()`（`_write_csv` 写 `_HEADER` + rows）。

---

## 3. 测试用例清单

### Group A — common.py merge 级（test_common.py，新类 `TestImportReplaceTableMerge`）

新增类常量（区别于现有 replace 类的 `_DDL`/`_INSERT`）：

```python
_DDL_MERGE = """\
CREATE TABLE IF NOT EXISTS test_replace (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    value DOUBLE,
    PRIMARY KEY (symbol, trade_date)
)"""

_INSERT_IGNORE = """
    INSERT IGNORE INTO test_replace (symbol, trade_date, value)
    SELECT symbol, trade_date, value
    FROM _tmp_tst
    WHERE symbol IN (SELECT symbol FROM stock_basic)
"""
```

类内 `dolt_env` fixture 复制现有 TestImportReplaceTable.dolt_env（stock_basic 种子
`('SH600519')` 不变）；`_import(csv_path)` 帮助方法：
`import_replace_table(csv_path, "_tmp_tst", _DDL_MERGE, _INSERT_IGNORE, "test_replace",
"test source", "MAX(trade_date)", merge=True)`。

| # | 测试名 | Setup | 断言 | 分类 |
|---|---|---|---|---|
| A1 | `test_merge_first_run_creates_table_and_upserts` | CSV: `[SH600519, 2026-07-31, 1.5]` | `_import == 1`；`COUNT(*) == "1"`；data_updates `row_count=="1"`、`last_report_date=="2026-07-31"`、`source=="test source"`、`last_updated != ""` | PIN |
| A2 | `test_merge_incremental_csv_appends_without_loss` | CSV A: `[SH600519,2026-06-30,2.5], [SH600519,2026-07-31,1.5]` → import；CSV B: `[SH600519,2026-07-31,1.5], [SH600519,2026-08-31,0.5]` → import | 第二次返回 `3`；`COUNT == "3"`；**原行字节级不变**：`SELECT value WHERE symbol='SH600519' AND trade_date='2026-06-30' == "2.5"`、`'2026-07-31' == "1.5"`；新行 `'2026-08-31' == "0.5"` | PIN |
| A3 | `test_merge_same_csv_twice_idempotent` | 同 CSV 导入两次 | 两次均返回 `1`；`COUNT == "1"` | PIN |
| A4 | `test_merge_watermark_full_table_count_and_max` | CSV A: `[SH600519,2026-06-30,2.5]`；CSV B: `[SH600519,2026-07-31,1.5]` | data_updates `row_count == "2"`（**全表**，不是本次 CSV 的 1 行）、`last_report_date == "2026-07-31"` | PIN |
| A5 | `test_merge_insert_failure_preserves_rows_and_watermark` | CSV A 2 行导入 → `DROP TABLE stock_basic` → CSV B 导入 | 返回 `0`；`COUNT == "2"`；值不变；`_tmp_tst` 不存在；watermark 仍是 `row_count=="2"`、`last=="2026-07-31"` | PIN |
| A6 | `test_merge_plain_ddl_silently_skips_import`（隐患 pin） | CSV A 用 `_DDL_MERGE` 导入 1 行；CSV B 用 plain `CREATE TABLE test_replace (...)`（无 IF NOT EXISTS）导入 | 返回 `0`；`COUNT == "1"`；watermark 不变（`row_count=="1"`） | PIN |

RED 命令（本组全绿即达标，属回归基线）：
`uv run pytest tests/test_common.py::TestImportReplaceTableMerge -v`

### Group B — fetch_balance_sheet.py（test_balance_sheet.py）

前置改动：fixture stock_basic 种子加 `('SZ000002')`；`_make_row` 增加关键字参数
`report_date: str = "2024-12-31"` 与 `total_assets: str = "100"`（填入
`_HEADER.index("REPORT_DATE")` / `_HEADER.index("TOTAL_ASSETS")`；现有调用不传参，向后兼容）。

**替换** `TestImportToDolt.test_rerun_replaces_table_without_duplicates`（现 120 行）：

| # | 测试名 | Setup | 断言 | 分类 |
|---|---|---|---|---|
| B1 | `test_incremental_merge_appends_preserving_history`（grill 契约场景） | CSV A: `[_make_row()]` → import（返回 1）；CSV B: `[_make_row(), _make_row(secucode="000002.SZ"), _make_row(report_date="2023-12-31")]` → import | 第二次返回 `3`；`COUNT == "3"`；`TOTAL_ASSETS`（SZ000001, 2024-12-31）`== "100"`；SZ000002 行存在（COUNT==1）；2023-12-31 行存在（COUNT==1）；watermark `row_count=="3"`、`last=="2024-12-31"` | **PIN**（B 全含 A 且同值，replace 下观测等价——诚实标注，见 §1） |
| B2 | `test_incremental_window_preserves_older_history`（**RED 核心**） | CSV A: `[_make_row(), _make_row(report_date="2023-12-31")]` → import（2）；CSV B（增量窗口形态:watermark 重抓 + 新标的）: `[_make_row(), _make_row(secucode="000002.SZ")]` → import | 第二次返回 `3`；`COUNT == "3"`；`COUNT(*) WHERE symbol='SZ000001' AND report_date='2023-12-31' == "1"`（2023 期存活）；watermark `row_count=="3"` | **RED**：pre-fix 替换语义下 CSV B 整表替换 → 2 行、2023-12-31 被抹掉 → `assert 2 == 3` |
| B3 | `test_restated_overlap_value_ignored_on_merge`（**RED**） | CSV A: `[_make_row()]`（TA=100）→ import（1）；CSV B: `[_make_row(total_assets="200"), _make_row(secucode="000002.SZ")]` → import | 返回 `2`；`COUNT == "2"`；`TOTAL_ASSETS`（SZ000001, 2024-12-31）`== "100"`（修订值 200 被 INSERT IGNORE 拒绝，旧值保留） | **RED**：pre-fix 替换 → 值为 `"200"` → `assert "100"` 失败 |
| B4 | `test_same_report_refetch_idempotent` | 同 CSV `[_make_row()]` 导入两次 | 均返回 `1`；`COUNT == "1"` | PIN（原测试的直接延续） |
| B5 | `test_merge_watermark_full_total_and_max_date`（**RED**） | CSV A: `[_make_row(report_date="2023-12-31")]` → import（1）；CSV B: `[_make_row()]`（2024-12-31）→ import | 第二次返回 `2`；data_updates `row_count == "2"`、`last_report_date == "2024-12-31"` | **RED**：pre-fix 替换 → 第二次返回 1、`row_count=="1"` → 断言失败 |

**适配**失败路径测试：

| # | 测试名（现名 → 新名） | Setup | 断言 | 分类 |
|---|---|---|---|---|
| B6 | `test_first_run_insert_failure_leaves_no_table_and_no_error`（143 行）→ `test_first_run_insert_failure_leaves_empty_table` | fixture 中 `DROP TABLE stock_basic`；CSV `[_make_row()]` | 返回 `0`；`SELECT COUNT(*) FROM fin_balance_sheet == "0"`（**表存在且空**——merge 先 CREATE IF NOT EXISTS 再失败）；`_tmp_bs` / `_tmp_bs_old` 均不存在；data_updates 无 fin_balance_sheet 行 | **RED**：pre-fix 替换失败路径 DROP 整表 → 表不存在 → `COUNT(*)` 报错、`_last` 返回 `""`/`"COUNT(*)"` ≠ `"0"` |
| B7 | `test_rerun_insert_failure_rolls_back_previous_data`（167 行）→ `test_rerun_insert_failure_preserves_prior_rows` | 导入 A（1 行）→ `DROP TABLE stock_basic` → 导入 B | 返回 `0`；`COUNT == "1"`；`TOTAL_ASSETS == "100"`；`_tmp_bs`/`_tmp_bs_old` 不存在；watermark 不变（`row_count=="1"`、`last=="2024-12-31"`） | PIN（replace 靠 RENAME 回滚、merge 靠未触碰，观测等价——诚实标注） |

**修复** EdgeCases：

| # | 测试名 | 改动 | 说明 |
|---|---|---|---|
| B8 | `TestImportToDoltEdgeCases.test_dolt_table_import_failure_returns_zero`（351 行） | `monkeypatch.setattr("fetch_balance_sheet.dolt_table_import", ...)` → `monkeypatch.setattr("common.dolt_table_import", lambda _t, _p: False)` | GREEN 重构后 fetch_balance_sheet 不再 import `dolt_table_import`，原 patch 目标不存在 → `monkeypatch.setattr` 默认 raising 抛 AttributeError。改 patch `common` 后 pre-fix / post-fix 均绿 |

**新增** run() 级测试（`TestRun` 类）：

| # | 测试名 | Setup | 断言 | 分类 |
|---|---|---|---|---|
| B9 | `test_run_incremental_overwrites_stale_csv`（**grill 契约：CSV 是否被覆盖**） | `monkeypatch.chdir(tmp_path)`；`COMPASS_DATA_DIR → tmp_path/"no_dolt"`；**预写陈旧 CSV** `RPT_DMSK_FN_BALANCE.csv`（表头 `code,REPORT_DATE` + 行 `000001,2024-12-31`）；`monkeypatch.setattr("fetch_balance_sheet.last_report_date", lambda _t: "2026-06-30")`；stub data=`[{"code":"000001","REPORT_DATE":"2026-06-30"}]` | `run(years=[2026], periods="Q2", page_size=100)`；读 CSV：`csv.DictReader` 行数 == 1 且 `== [{"code":"000001","REPORT_DATE":"2026-06-30"}]`；**不含 2024-12-31 行**；表头仍存在 | **PIN**（pre-fix 首写 `mode="w"` 已覆盖——诚实标注；此测试即对用户"csv 怎么处理/会不会被覆盖"的客观回答：**run() 每次覆盖陈旧 CSV，历史不存 CSV，存在 Dolt，故 Dolt 必须 merge**） |
| B10 | `test_run_incremental_window_starts_at_watermark`（推荐） | `monkeypatch last_report_date → "2026-06-30"`；years=[2026]、periods="Q1,Q2"；stub 计数器闭包（仿 `test_run_fetch_exception_continues`） | `session.get` 恰好调用 **1 次**（2026-03-31 < since 被过滤，仅 06-30 被拉取）；CSV 只含 06-30 行 | PIN（固化 `d >= since` → 最新报告期必被重抓的契约） |

### Group C — fetch_income.py（test_income.py，同构）

与 Group B 相同形状，差异：
- `_make_row` 参数：`report_date` + `parent_netprofit`（默认 `"1000"`，填 `PARENT_NETPROFIT` 列）
- DOLT_TABLE = `fin_income`；CSV 文件名 `inc.csv`；run() CSV = `RPT_DMSK_FN_INCOME.csv`
- 对应现有测试行号：`test_rerun_replaces_table_without_duplicates`（113）、
  `test_first_run_insert_failure_leaves_no_table`（127）、`test_rerun_insert_failure_rolls_back`（146）
- 断言数值按 PARENT_NETPROFIT 调整（B3 用 "200"/"1000"）
- test_income.py **没有** EdgeCases 类（无 B8）

用例：C1=C1B1、C2=B2、C3=B3、C4=B4、C5=B5、C6=B6、C7=B7、C9=B9、C10=B10
（命名同 B 系列，如 `test_incremental_window_preserves_older_history`）。

### Group D — fetch_cash_flow.py（test_cash_flow.py，同构）

同 Group C，差异：
- `_make_row` 参数：`report_date` + `netcash_operate`（默认 `"500"`，填 `NETCASH_OPERATE` 列）
- DOLT_TABLE = `fin_cash_flow`；CSV 文件名 `cf.csv`；run() CSV = `RPT_DMSK_FN_CASHFLOW.csv`
- 现有测试行号：114 / 128 / 147

用例：D1=B1 … D10=B10（断言数值按 NETCASH_OPERATE 调整）。

### Group E — fin_indicators（test_main.py，新类 `TestImportFinIndicatorsMerge`）

新类含 `dolt_env` fixture（stock_basic 种子 `('SZ000001'), ('SZ000002')` + data_updates）；
`monkeypatch.setattr(main_mod, "COLLECTORS_DIR", tmp_path)`；CSV 路径
`tmp_path / "RPT_LICO_FN_CPD.csv"`。

CSV 表头（**必须全列**——`_tmp_fin` 若用 dolt 类型推断建表，INSERT SELECT 引用的列缺一即报错；
全列写法在 GREEN 提供 create_sql 时也健壮）：
`SECUCODE, SECURITY_CODE, REPORTDATE, UPDATE_DATE, NOTICE_DATE, DATATYPE, QDATE, EITIME,
DATAYEAR, DATEMMDD, SECURITY_NAME_ABBR, TRADE_MARKET, TRADE_MARKET_CODE, TRADE_MARKET_ZJG,
SECURITY_TYPE, SECURITY_TYPE_CODE, PUBLISHNAME, BOARD_CODE, BOARD_NAME, ORI_BOARD_CODE,
ORG_CODE, ISNEW, BASIC_EPS, DEDUCT_BASIC_EPS, TOTAL_OPERATE_INCOME, PARENT_NETPROFIT,
WEIGHTAVG_ROE, BPS, MGJYXJJE, XSMLL, YSTZ, SJLTZ, YSHZ, SJLHZ, ZXGXL, ASSIGNDSCRPT, PAYYEAR`
（对应 main.py `_import_fin_indicators` INSERT SELECT 的全部映射列）。

`_make_row(secucode="000001.SZ", report_date="2024-12-31")`：填 SECUCODE / SECURITY_CODE /
REPORTDATE，其余列空串（空 → NULL，要求 FIN_INDICATORS_DDL 各列可空——GREEN 约束）。
`_write_csv` 写全表头 + rows。

**注意**：`_import_fin_indicators()` 现返回 None，重构后返回值未锁定——**测试只断言
Dolt 表状态与 data_updates，不断言返回值**。

| # | 测试名 | Setup | 断言 | 分类 |
|---|---|---|---|---|
| E1 | `test_merge_incremental_appends_preserving_history`（**RED 核心**） | CSV A: `[_make_row(), _make_row(report_date="2023-12-31")]` → import；CSV B: `[_make_row(), _make_row(secucode="000002.SZ")]` → import | `COUNT(*) FROM fin_indicators == "3"`；`COUNT(*) WHERE symbol='SZ000001' AND report_date='2023-12-31' == "1"`；watermark `row_count=="3"`、`last_report_date=="2024-12-31"` | **RED**：pre-fix DELETE+INSERT → 2 行、2023-12-31 被抹 → `assert "3"` 失败（实际 "2"） |
| E2 | `test_merge_same_csv_refetch_idempotent` | 同 CSV `[_make_row()]` 导入两次 | `COUNT == "1"` | PIN（DELETE+INSERT 与 merge 观测等价） |
| E3 | `test_merge_insert_failure_preserves_prior_rows`（**RED**） | CSV A 2 行导入（COUNT=2）→ `DROP TABLE stock_basic` → CSV B `[_make_row()]` 导入 | `COUNT == "2"`（两行全存活）；`_tmp_fin` 不存在；watermark 保持 `row_count=="2"`、`last=="2024-12-31"` | **RED**：pre-fix `DELETE FROM fin_indicators` **成功清空**、INSERT 因 stock_basic 缺失失败（旧代码无回滚）→ 0 行 → `assert "2"` 失败（实际 "0"），watermark 被覆盖为 `row_count=0` |

现有 mock 测试（`TestImportFinIndicators`，595 行起）：
- `test_csv_missing_exits_early` — **保留不动**（CSV 不存在早退逻辑不变）。
- `test_csv_exists_imports_to_dolt` — **先验证后保留**：新流程（经 import_replace_table
  merge）调用序列 = `dolt_table_import` ×1（断言 `assert_called_once` ✓）、`dolt_sql`
  ×4 = DDL/INSERT/DROP tmp/upsert（现有 `>= 3` ✓）、`dolt_sql_csv` ×2 = COUNT/MAX（现有
  `>= 2` ✓）。mock 返回 `returncode=0` / `"Count\n50"`，流程不崩。**现有断言宽松到两种
  流程都满足 → 无需改动**（可选：收紧为精确计数 `==4/==1/==2`，但增加脆弱性，不推荐）。

---

## 4. 现有测试变更清单

| 文件 | 测试 | 动作 | 原因 |
|---|---|---|---|
| test_common.py | `TestImportReplaceTable`（全部 7 个）+ 其余类 | **保留不动** | replace 仍是 primitive 的受支持模式；merge 另立新类 |
| test_common.py | — | **新增** `TestImportReplaceTableMerge`（A1-A6） | 锁定 merge primitive 契约（PIN） |
| test_balance_sheet.py | fixture stock_basic 种子 / `_make_row` | **修改**（加 SZ000002；加 report_date/total_assets 参数） | B2/B3 需要新标的过过滤；向后兼容 |
| test_balance_sheet.py | `test_rerun_replaces_table_without_duplicates`（120） | **删除，替换为** B1-B5 | 语义从 replace 改为 merge（B4 保留其幂等内核） |
| test_balance_sheet.py | `test_first_run_insert_failure_leaves_no_table_and_no_error`（143） | **改名+改断言** → B6 | merge 失败路径：表存在且空，非表消失（RED） |
| test_balance_sheet.py | `test_rerun_insert_failure_rolls_back_previous_data`（167） | **改名+改断言** → B7 | merge 无回滚概念，改为"保留先前行"（PIN） |
| test_balance_sheet.py | `TestImportToDoltEdgeCases.test_dolt_table_import_failure_returns_zero`（351） | **修改 patch 目标** → B8 | 重构后 fetch_balance_sheet 不再持有 `dolt_table_import` |
| test_balance_sheet.py | `TestRun` | **新增** B9、B10 | 固化 run() CSV 覆盖与 since 窗口契约（PIN） |
| test_income.py | 同构：113 替换 / 127 改名 / 146 改名；fixture+`_make_row` 修改；TestRun 新增 C9/C10 | 同 B | 同 |
| test_cash_flow.py | 同构：114 替换 / 128 改名 / 147 改名；TestRun 新增 D9/D10 | 同 B | 同 |
| test_main.py | `TestImportFinIndicators` 两个 mock 测试 | **保留**（第二个先验证后留） | 新流程仍满足 `>=3 / once / >=2` 断言 |
| test_main.py | — | **新增** `TestImportFinIndicatorsMerge`（E1-E3） | fin_indicators 从 DELETE+INSERT 改 merge（RED） |
| 其余所有测试（TestRun 既有 5 个、TestDispatch*、TestDoSync、TestMain、TestImportStockBasic、TestParseYears 等） | **保留不动** | run()/dispatch 逻辑不变 |

---

## 5. 编写顺序（依赖关系）

1. **test_common.py A1-A6**（PIN，最先）：在 tempdir Dolt 中先验证 merge primitive 全流程
   可用（建表/追加/幂等/watermark/失败保留/DDL 隐患），为后续 collector 测试提供信心。
2. **test_balance_sheet.py**（B 组全部 + fixture/_make_row/EdgeCases 改动 + run 级 B9/B10）：
   首个 collector，模式定型。
3. **test_income.py、test_cash_flow.py**（C/D 组）：按 B 组模板机械复制调整。
4. **test_main.py**（E1-E3 + 验证 E4 mock 测试）。
5. **全量回归**：`uv run pytest tests/ -v` → 期望**只有**文档化的 RED 测试失败、其余全绿
   （证明测试隔离、无意外破坏）→ 采集 RED 证据（§6）→ 交 GREEN。

依赖说明：A 组与 B/C/D/E 组相互独立（不同文件），但**先写 A 组**可尽早暴露
merge primitive 在 tempdir Dolt 下的环境问题（dolt 版本差异等），避免污染 B 组调试。
B/C/D 相互独立可并行；E 依赖 B 组建立的 Dolt-tempdir 写法惯例（无代码依赖）。

---

## 6. RED 证据采集（GREEN 之前的门禁证据）

在 `collectors/` 下执行（工作区 venv 未装 pytest 时先 `uv sync` 或 `uv run` 自动解析）：

```bash
cd /data/codes/compass/.worktrees/fin-incremental/collectors

# 三表的 RED 核心（追加保历史 / 修订值保留 / 全表 watermark / 首跑失败留空表）
uv run pytest tests/test_balance_sheet.py -k "test_incremental_window_preserves_older_history or test_restated_overlap_value_ignored_on_merge or test_merge_watermark_full_total_and_max_date or test_first_run_insert_failure_leaves_empty_table" -v 2>&1 | tee /tmp/opencode/red-evidence-bs.log
uv run pytest tests/test_income.py -k "test_incremental_window_preserves_older_history or test_restated_overlap_value_ignored_on_merge or test_merge_watermark_full_total_and_max_date or test_first_run_insert_failure_leaves_empty_table" -v 2>&1 | tee /tmp/opencode/red-evidence-inc.log
uv run pytest tests/test_cash_flow.py -k "test_incremental_window_preserves_older_history or test_restated_overlap_value_ignored_on_merge or test_merge_watermark_full_total_and_max_date or test_first_run_insert_failure_leaves_empty_table" -v 2>&1 | tee /tmp/opencode/red-evidence-cf.log

# fin_indicators 的 RED 核心（追加保历史 / 失败保留先前行）
uv run pytest tests/test_main.py -k "TestImportFinIndicatorsMerge" -v 2>&1 | tee /tmp/opencode/red-evidence-main.log

# 全量回归（证明只有 RED 失败、其余全绿）
uv run pytest tests/ -v 2>&1 | tee /tmp/opencode/red-evidence-full.log
```

**预期失败原因（每个 RED 都对"正确的原因"失败——替换/删除语义违反 merge 断言）**：

| 测试 | 预期失败断言 | pre-fix 实际值 | 根因 |
|---|---|---|---|
| B2/C2/D2 `preserves_older_history` | `assert rows == 3` | `2`（replace 整表替换，2023-12-31 被抹） | 替换语义丢历史 |
| B3/C3/D3 `restated_overlap_value_ignored` | `assert "100"/"1000"/"500"` | `"200"` 等（replace 用 CSV 值覆盖） | 替换语义覆盖修订 |
| B5/C5/D5 `watermark_full_total` | `assert row_count == "2"` | `"1"`（replace 只算本次 CSV） | 替换语义 watermark=CSV 行数非全表 |
| B6/C6/D6 `first_run_insert_failure` | `assert cnt == "0"`（表存在且空） | `""`/`"COUNT(*)"`（表被 DROP） | 替换失败路径删表，merge 留空表 |
| E1 `appends_preserving_history` | `assert "3" == "3"`（COUNT） | `"2"`（DELETE+INSERT 抹 2023-12-31） | DELETE+INSERT 丢历史 |
| E3 `insert_failure_preserves_prior_rows` | `assert "2" == "2"`（COUNT） | `"0"`（DELETE 成功清空、INSERT 失败无回滚） | 旧实现无事务回滚 |

**PIN 测试的"证据"**：同一全量运行中 A1-A6、B1、B4、B7、B8、B9、B10、E2 及全部既有
测试为绿色——输出本身记录 pre-fix 行为基线（诚实归档：这些测试 pre-fix 不失败，
其价值是防止 GREEN 后回归）。

---

## 7. GREEN 后验证

```bash
cd /data/codes/compass/.worktrees/fin-incremental/collectors
uv run pytest tests/ -v            # 全绿
uv run ruff check                  # lint 干净
```

补充冒烟：B9 的 CSV 覆盖断言 + A2 的字节级不变断言在 GREEN 后必须依旧成立
（证明重构未破坏 run()/primitive）。

---

## 8. 风险与注意

| 风险 | 缓解 |
|---|---|
| GREEN 后 B1/B4 等 PIN 测试无法区分 replace/merge（观测等价） | 已由 B2/B3/B5/B6/E1/E3 的 RED 断言兜底；PIN 测试防未来回归 |
| `_tmp_fin` 若用类型推断建表，E 组 CSV 缺列导致 INSERT SELECT 报错 | 测试写全列表头（§3 Group E）；GREEN 若提供 create_sql 亦兼容 |
| FIN_INDICATORS_DDL 列需可空（CSV 空串 → NULL） | 已列为 GREEN 约束 |
| `dolt table import -c` 对已存在 tmp 表的行为依赖 dolt 版本 | 既有 rerun 测试（test_common）已证明可用，测试照抄同一模式 |
| 覆盖率门禁（Python ≥80%） | 重构为删代码 + 新增测试，净增益；T8 全量 pytest 验证 |
