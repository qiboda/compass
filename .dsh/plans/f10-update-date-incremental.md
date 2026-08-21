# Plan: 财务三表按 UPDATE_DATE 增量抓取（issue #299）

## 目标

实现 GitHub issue #299：`balance_sheet` / `income` / `cash_flow` 三张财务 F10 表
从按 `REPORT_DATE` 报告期窗口增量，改为按 `UPDATE_DATE` 时间锚点增量抓取，
捕获同一 `(symbol, report_date)` 的历史修订，并减少重复全量拉取。

## 已锁定决策（grill-me / handoff）

- 无 anchor（首次运行 / 无 state.json / 无 data_updates 行）时：
  **不回退**到 `REPORT_DATE` 全量枚举 / 全量 replace；
  直接使用固定起始日 `2020-01-01`，走 `UPDATE_DATE>='2020-01-01'` 增量路径
  （相当于一次拉取全部历史更新）。
- 导入统一改为 `import_replace_table(..., merge=True)` + `INSERT ... ON DUPLICATE KEY UPDATE`
  （参照 `main.py::_import_fin_indicators` 的 Dolt 2.2.3 兼容写法：SELECT 侧唯一别名 +
  ODKU 无前缀别名引用，不用 `VALUES()`）。
- state.json 记录 `last_update_date` + `last_report_date`。
- 本次为 feature 工作，完整走 gate：worktree → plan → RED tests → docs → implement → commit/review/PR。

## 接口契约

### `collectors/common.py`（共享增量基建）

- 新增 `fetch_by_update_date(session, throttle, report_name, anchor, page_size=EM_PAGE_SIZE, *, pool=None)`
  —— 从 `fetch_fin_indicators.py` 平移同一逻辑：`filter=(UPDATE_DATE>='{anchor}')`、
  `sortColumns=UPDATE_DATE`、`sortTypes=1`、pages cap 500。
- 新增/泛化 `update_date_anchor(report_name, state_path, dolt_table=None)`：
  - 保持现 `_update_anchor` 语义：`min(data_updates.last_updated, state.json.last_update_date)`；
  - 新增 `dolt_table` 参数：传入时 data_updates 查该表；缺省保持旧映射
    （`RPT_LICO_FN_CPD` → `fin_indicators`，其他 report_name 原样作为表名）。
  - 返回空串表示两源皆无；调用方按锁定决策替换为 `2020-01-01`。
- 新增 `normalize_update_date(value)`：从 `fetch_fin_indicators.py::_normalize_update_date`
  平移（YYYY-MM-DD 或 None）。
- `dedupe_csv(path, date_col="REPORTDATE")`：增加可配置日期列名；
  F10 三表调用 `date_col="REPORT_DATE"`，fin_indicators 保持默认 `REPORTDATE` 不变。
- `fetch_fin_indicators.py` 改为从 `common` 导入/别名这些函数（保持 `fetch_fin_indicators._update_anchor`
  等现有测试兼容）。

### `collectors/fetch_balance_sheet.py` / `fetch_income.py` / `fetch_cash_flow.py`

每个模块：

- `run(years=None, periods="Q1,Q2,Q3,FY", page_size=100, incremental=False)`
  - `incremental=False`：保留现有 `REPORT_DATE` 枚举路径（显式全量/按报告期窗口）。
  - `incremental=True`：
    - `state_path = Path(f"{REPORT_NAME}.state.json")`
    - `anchor = update_date_anchor(REPORT_NAME, state_path, dolt_table=DOLT_TABLE)`
    - `anchor = anchor or "2020-01-01"`（锁定决策）
    - `records = await fetch_by_update_date(...)`
    - 写 CSV（覆盖旧 CSV）；`total_records > 0` 时写 state.json：
      `last_report_date` = 本批最大 `REPORT_DATE`，`last_update_date` =
      本批最大规范化 `UPDATE_DATE`（无值保留旧 anchor；未来日期 clamp 到今天），
      另含 `total_rows` / `last_run`。
- 独立 CLI 增加 `--incremental` flag。
- `import_to_dolt()`：
  - 改为 `import_replace_table(..., merge=True)`
  - `insert_sql` 改为 `INSERT ... SELECT ... ON DUPLICATE KEY UPDATE`：
    SELECT 侧对 `COLS` 每列生成唯一别名（`_<COL>`，文本列 `TRIM(col) AS _<COL>`），
    ODKU 子句 `col=_<COL>`。
  - 仍过滤 `stock_basic` 内 symbol。

### `collectors/main.py`

- `dispatch_fetch(target, years=None, incremental=False)`：
  `balance_sheet` / `income` / `cash_flow` 透传 `incremental`。
- `fetch` 子命令新增 `--incremental` flag。
- `do_sync()` 三表调用 `run(incremental=True)`；导入仍走各模块 `import_to_dolt()`（已是 merge）。
- 更新模块 docstring / `--help` 文案。

## 测试计划（RED，实现前由子代理写）

- `subagent_skwy_adversarial_test`（3.5）：
  - 无 anchor → anchor 必须是 `2020-01-01` 且走 UPDATE_DATE 路径（不 fallback 枚举）。
  - state.json 缺 `last_update_date` / data_updates `last_updated` NULL / 未来日期 clamp。
  - 空结果不推进 state；全部 UPDATE_DATE 缺失时保留旧 anchor。
  - 重复 PK 修订：同一 `(SECURITY_CODE, REPORT_DATE)` 新 UPDATE_DATE/新值在 CSV 去重 keep-last、
    Dolt ODKU 覆盖。
  - `--incremental` 与 `--years/--periods` 的忽略关系；`dispatch_fetch`/`do_sync` 接线。
- `subagent_skwy_requirement_test`（4）：
  - 三表 incremental fetch 构造 `(UPDATE_DATE>='anchor')` filter、写 CSV/state。
  - 三表 import merge 首建表 / 历史保留 / 修订覆盖 / data_updates 行更新。
  - main.py `fetch balance_sheet --incremental`、`sync` 调用路径。

## 文档同步（5b）

| 文件 | 原因 |
|---|---|
| `.dsh/kb/design/architecture.md` | 数据管线章节：财务三表增量方式描述 |
| `.dsh/kb/design/data-providers.md` | 决策记录新增 #299 行（UPDATE_DATE 增量 + merge/ODKU） |
| `.dsh/kb/user/cli.md` | CLI/同步行为：三表 `--incremental`、`sync` 默认增量 |
| `.dsh/kb/dev/database.md`（如涉及） | data_updates 锚点/state.json 说明（按实际需要） |

## 实现后验证

- `cd collectors && uv run pytest tests/test_fin_indicators.py tests/test_balance_sheet.py tests/test_income.py tests/test_cash_flow.py -q`
- 全量 Python 测试 + 覆盖率门禁 ≥95%（如 CI 命令允许）。
- 真实数据冒烟：在可控环境跑一次三表 `--incremental`（可用小范围/短 anchor 或 dry-run 验证
  API filter、CSV 列、state 写入），落库行数与 data_updates 检查。

## 提交计划

- 按逻辑单元 commit：common 基建 → 三表 fetch/import → main.py 接线 → docs。
- 每个 commit 独立成行 `ref #299`。
- 不自动 push；commit 后跑 review，再等用户 push 指令。
