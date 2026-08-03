# Plan: Issue #160 — Financial Collectors Incremental Merge + fin_balance_sheet Rebuild

Ref: https://github.com/qiboda/compass/issues/160 · Worktree: feat/fin-incremental

## 问题（已核实）

- `fetch_income.py` / `fetch_balance_sheet.py` / `fetch_cash_flow.py` 自实现 `import_to_dolt()` = **整表替换**
  （RENAME aside → DROP → CREATE → INSERT SELECT），而 `run()` 无条件套用增量窗口
  （`since = data_updates.last_report_date`）→ 增量抓取 + 整表替换 = 历史丢失。
- `main.py::_import_fin_indicators()` = `DELETE FROM` + INSERT（同样替换语义）。
- `common.import_replace_table(merge=True)`（INSERT IGNORE + PK 去重）已存在，财务四表未使用。
- Dolt 实测：fin_balance_sheet=1 行垃圾（SZ000001/2024-12-31/TOTAL_ASSETS=100）；
  fin_income=126880 / fin_cash_flow=126701 / fin_indicators=126447，watermark 均 2026-06-30。

## 锁定决策（grill-me 契约，见 .omo/handoff.md）

1. 范围：修复 fin_balance_sheet 重建 + 财务三表增量 + sepa 补跑；c/w/yahoo 停更表不处理。
2. fin_balance_sheet 全量重建（重置锚点 → 抓 2020-2026 全报告期 → 恢复 ≥13 万行；一次性，后续走增量）。
3. 财务四表导入改 **merge 语义**（INSERT IGNORE + PK `(symbol, report_date)` 去重，复用
   `common.import_replace_table(merge=True)`）；fetch 保持报告期级增量（最新报告期重抓补新披露公司）。
4. 数据操作序列：fin_balance_sheet 全量重建 → 三表增量补采 → 重生成 fin_* parquet → sepa 补跑 08-03。
5. 提交：Dolt compass_data commit+push + GitHub 代码/测试/kb 文档 commit+push（ref #160）。
6. 工作空间：本 worktree。

## 执行波次

### Wave 1 — RED 测试（并行，独立）
- T1: test_common.py 加 5 个 merge=True 测试（首次建表/增量追加不丢历史/同键重抓幂等/
  watermark=总数+MAX/插入失败保留旧行）。DDL 用 `CREATE TABLE IF NOT EXISTS` + `INSERT IGNORE`。
- T2: test_balance_sheet/income/cash_flow.py — 替换 `test_rerun_replaces_table_without_duplicates`
  为合并语义测试（追加保历史/同报告期幂等/watermark）；适配失败路径测试；修
  `test_dolt_table_import_failure_returns_zero`（改 patch `common.dolt_table_import`）；
  加 run() 级"最新报告期被重抓"测试。
- T3: test_main.py 加 `_import_fin_indicators` 的 Dolt-tempdir merge 测试；适配 mock 计数断言。

### Wave 2 — GREEN 实现（并行，依赖 Wave 1）
- T4/T5/T6: fetch_balance_sheet / fetch_income / fetch_cash_flow —
  DDL 改 `CREATE TABLE IF NOT EXISTS`；`import_to_dolt()` 重构为
  `import_replace_table(merge=True)` 薄包装（tmp `_tmp_bs/_tmp_inc/_tmp_cf`、
  `INSERT IGNORE INTO {DOLT_TABLE}` + stock_basic 过滤、`last_report_expr="MAX(report_date)"`）；
  清理未用 import。
- T7: main.py `_import_fin_indicators()` → 加 `FIN_INDICATORS_DDL`（镜像 live
  `dolt schema show fin_indicators`，含 eitime datetime / ori_board_code varchar(10) /
  `CREATE TABLE IF NOT EXISTS` / PK (symbol, report_date)），重构为
  `import_replace_table(merge=True)`；保留原列映射。
- T9(draft): kb 文档草稿（最终文本 Wave 3 定稿）。

### Wave 3 — 全量验证 + 原子提交
- T8: `cd collectors && uv run pytest tests/ -v` 全绿 + `uv run ruff check`；提交：
  - c1 `test: financial collectors merge-incremental semantics (RED) ref #160`
  - c2 `fix: financial 4-table import uses merge semantics (INSERT IGNORE + PK dedup) ref #160`
- T9: kb 文档修正，提交：
  - c3 `docs: correct collector import semantics in data-providers.md and cli.md ref #160`

### Wave 4 — 数据操作（顺序执行；独立于 git，Dolt repo /data/compass-data/compass_data）
- T10: fin_balance_sheet 全量重建 — 核实 COUNT==1 → `DELETE FROM fin_balance_sheet`；
  `DELETE FROM data_updates WHERE table_name='fin_balance_sheet'`（重置锚点）→
  `uv run python main.py fetch balance_sheet`（26 报告期；任何 FAILED 期按问题处理闭环诊断）
  → `uv run python main.py import balance_sheet` → 验证 ≥130927 行、SZ000001/2024-12-31
  TOTAL_ASSETS != 100、watermark 2026-06-30。
- T11: 三表增量补采 — 并行 fetch（income / cash_flow / `fetch_fin_indicators.py --years 2026`），
  顺序 import；验证各表 2026-06-30 ≥102 家、总数增长。
- T12: Dolt commit D1 + push origin main（`dolt add fin_balance_sheet fin_income fin_cash_flow
  fin_indicators data_updates` → `fix(data): ... (ref #160)`）。
- T13: 重生成 fin_* parquet（`import-compass --table fin_balance_sheet/income/cash_flow/indicators`）；
  duckdb 验证行数 == Dolt、MAX report_date == 2026-06-30。
- T14: sepa 补跑（`sepa temperature` + `sepa score --top 50`）；验证 final_score/market_temperature
  trade_date=2026-08-03；Dolt commit D2 + push（`feat: sepa scores 2026-08-03 after financial
  backfill (ref #160)`）。

### Wave 5 — 工作流收尾（主 agent，不委派）
- T15: /review-work（≤2 轮修复）→ 呈报用户 → 用户批准 push → rebase master → /reflect 反思
  commit → push → PR → #160 完成 comment（逐项核实证据）+ 关闭 issue。

## 关键风险

| 风险 | 缓解 |
|---|---|
| merge 流程若 DDL 非 `CREATE TABLE IF NOT EXISTS` 会静默跳过导入 | T4-T7 必改 DDL；T10/T11 验证 Done 行数 |
| 垃圾行经 INSERT IGNORE 存活（PK 撞真实数据） | T10 重建前先 DELETE |
| API 429 / 单期抓取失败 | 内置重试 + run() 逐期继续；FAILED 期按闭环诊断重抓 |
| fin_indicators `--incremental` 解析不存在的 worktree Dolt 路径 | 用 `--years 2026` 有界补采 |
| 覆盖率门禁（Python ≥80%） | 重构为删代码，净增益；T8 跑全量 pytest |

## 验收标准（issue #160）

- [ ] 财务四表 import 后增量重跑不丢历史（同报告期重抓幂等）
- [ ] fin_balance_sheet Dolt ≥ 13 万行（2020-2026 全报告期）
- [ ] fin_income/cash_flow/indicators 2026-06-30 ≥ 102 家
- [ ] fin_* parquet 重新生成且数据一致
- [ ] sepa 衍生表 trade_date = 2026-08-03
- [ ] 测试覆盖 merge 增量语义（RED → GREEN）
- [ ] Dolt compass_data commit + push；GitHub 代码 commit + push
