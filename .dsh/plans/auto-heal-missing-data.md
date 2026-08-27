# Plan: 自动回补缺失数据机制（auto-heal missing data）

- Issue: #308 — [feat: 自动回补缺失数据机制（auto-heal missing data）](https://github.com/qiboda/compass/issues/308)
- Worktree: `feat-auto-heal-missing-data`
- Type: data pipeline feature (Python collectors + Rust CLI + shell script + docs)
- 已锁定 grill-me 决策：见 `.dsh/plans/handoff.md`（本 plan 为执行契约，不得偏离）
- 当前状态：Batch 1-3 实现与自动化测试已完成；F4 真实数据冒烟仍在待办（需用户/环境允许时跑 `update-database.sh`）。

## 目标

让数据管线具备**自动检测并回补缺失数据**的能力：即使不是每天运行 `update-database.sh`
（原 `scripts/sepa_daily.sh`），也能在下次运行时自动补齐中间/尾部缺失交易日，并补算
全部 SEPA 派生表；非日频表按 `data_updates.last_updated` + row_count 检查，避免逐日误报。

## 任务批次

### Batch 1 — Python collectors 自动检测/回补源数据

| Status | Task | 说明 |
|--------|------|------|
| completed | 交易日历/缺口检测工具 | 从 investment_data `ts_trade_day_calendar`（`exchange='SSE'` 且 `is_open=1`）读取交易日；对日频表按 Dolt 实际 `trade_date` 对比日历，输出缺失日期集合 |
| completed | `fetch_main_flow` 历史回补 | 缺口时走 EastMoney `push2his.eastmoney.com/api/qt/stock/fflow/daykline/get` 逐股历史 API；字段 f52-f56/f57 映射到 `capital_main_flow` 列；`INSERT IGNORE` 幂等 |
| completed | `fetch_index_daily` 指定范围回补 | 按 symbol 显式拉取缺失日期范围（中间缺口），维护 `index_basic` 伴生表 |
| completed | `fetch_dragon` / `fetch_block_trade` 范围回补 | dragon 已支持 `--start/--end`；block_trade 增加等价范围参数或复用日期过滤；回补中间缺口 |
| completed | `main.py sync` 集成自动扫描 | sync 开头检测日频表缺口 → 回补 → 再正常增量采集；严格失败：任一重试后仍失败则整个 sync abort；非日频表做新鲜度/row_count 检查并记录 |
| completed | Python 测试 | 上述能力的需求测试 + 对抗性测试（RED 先行） |

### Batch 2 — Rust `sepa` 派生表补算

| Status | Task | 说明 |
|--------|------|------|
| completed | `sepa temperature --date` | 现有 `SepaCmd::Temperature` 无 `--date`，补齐与 `score --date` 一致 |
| completed | `sepa backfill-dates [--start --end]` | 从 Parquet stock_daily 交易日集合与 Dolt 计算表已存在日期比对；对每个缺失交易日调用 score + temperature 写回 5 张计算表 + data_updates |
| completed | 严格失败 | 任一日期计算/写回失败立即退出，不做部分成功继续 |
| completed | Rust 测试 | 需求测试 + 对抗性测试（RED 先行） |

### Batch 3 — 脚本改名与全链路集成

| Status | Task | 说明 |
|--------|------|------|
| completed | 改名 `scripts/sepa_daily.sh` → `scripts/update-database.sh` | 彻底改名，不保留旧名兼容入口；全仓 grep 更新所有引用 |
| completed | 新管线步骤 | step 0 `scripts/sync-investment-data.sh` → step 1 `cargo import` → step 2 `collectors/main.py sync`（含 auto-heal）→ step 3 Dolt commit collector → step 4 import-compass 11 表 → step 5 `sepa backfill-dates` + `sepa temperature` + `sepa score --top 50` → step 6 Dolt commit compute → step 7 TOP50 |
| completed | stock_daily 缺口检查 | import 后对 Parquet/Dolt 按交易日历核对；缺失则硬失败（不静默、不降级） |
| completed | 文档同步 | `.dsh/kb/user/cli.md`（每日流水线/命令名）、`.dsh/kb/design/data-providers.md`（回补/增量语义）、`.dsh/kb/design/architecture.md`（管线变更）、`.dsh/kb/dev/database.md`（锚点与回补）、`.dsh/kb/dev/process.md`（如工作流变更）；全仓 grep `sepa_daily` 旧名 |
| completed | Shell 测试 | `scripts/tests/test-sepa-daily.sh` 改名并更新 mock 断言；必要时补 backfill/检测步骤断言 |

## 接口契约（测试依据）

### Python

- `collectors/common.py`（或 `main.py` 内新增模块）：
  - `trade_calendar(start: str, end: str) -> list[str]`：从 investment_data Dolt 读取 SSE 开市日。
  - `missing_dates(table: str, date_col: str, start: str, end: str) -> list[str]`：从 compass_data Dolt 读取某表已有日期，对比日历返回缺失日。
  - `set_last_report_date(table: str, date: str) -> None`：用于回补后恢复/推进锚点；实现细节以测试为准。
- `fetch_main_flow`：
  - `backfill(start: str, end: str) -> Path`：逐股拉取历史资金流并写 CSV。
  - 字段映射：`f52..f56` → `main_net_inflow/small_net/medium_net/large_net/super_large_net`（按已实测顺序），`f57` → `main_net_inflow_rate`；`trade_date` 来自 API 行首日期。
  - `import_to_dolt` 保持 merge/`INSERT IGNORE`，重复回补不产生重复行。
- `fetch_index_daily`：
  - `backfill(start: str, end: str) -> Path`（或等价 `run(..., start=..., end=...)`）：对全部 symbols 拉取指定日期范围。
- `fetch_block_trade`：
  - `run(..., start: str | None, end: str | None)`：在现有 years 过滤基础上支持范围。

### Rust

- `SepaCmd::Temperature` 增加 `--date <YYYY-MM-DD>`。
- `SepaCmd::BackfillDates { start: Option<String>, end: Option<String> }`，命令名 `backfill-dates`。
- 函数签名（以现有 sepa.rs 风格）：
  - `pub fn run_temperature(reader: &ParquetReader, dolt_dir: &Path, date: Option<NaiveDate>) -> Result<(), Box<dyn Error>>`
  - `pub fn run_backfill_dates(start: Option<NaiveDate>, end: Option<NaiveDate>, reader: &ParquetReader, dolt_dir: &Path) -> Result<(), Box<dyn Error>>`
- 缺失日期判定：Parquet stock_daily 中存在的交易日 `D`，若 Dolt `final_score`（或 `technical_factor`）该日无行则视为缺失；`--start/--end` 时仅处理窗内日期。
- 每个缺失日：`run_score`（4 表）+ `run_temperature`（market_temperature）写回；严格失败。

## 文档同步（gate 5b）

基于变更文件：`collectors/main.py`、`collectors/fetch_main_flow.py`、`collectors/fetch_index_daily.py`、`collectors/fetch_block_trade.py`、`collectors/fetch_dragon.py`、`collectors/common.py`、`crates/compass-data/src/main.rs`、`crates/compass-data/src/sepa.rs`、`scripts/update-database.sh`、`scripts/tests/*`。

| 文档 | 原因 |
|---|---|
| `.dsh/kb/user/cli.md` | 每日流水线入口改名 `update-database.sh`；auto-heal 行为说明 |
| `.dsh/kb/design/data-providers.md` | `capital_main_flow` 历史回补数据源、`sepa backfill-dates`、增量/回补语义、决策记录 |
| `.dsh/kb/design/architecture.md` | 管线步骤变化（sync-investment 前置、backfill、stock_daily 检查） |
| `.dsh/kb/dev/database.md` | 锚点/回补、Dolt 表 check/commit 说明 |
| `.dsh/kb/dev/process.md` | 如工作流/命令变化 |
| `AGENTS.md` | 仅索引，若项目级命令/规则变化则一句话更新 |

另全仓 grep：`sepa_daily.sh`、`sepa_daily`、`每日一键流水线`。

## 验证门禁（F-wave）

- F1：合规审计——每个实现 commit 含独立行 `ref #308`（或后续子 issue ref）；无未引用文件；gate 各步骤 evidence 落盘 `.dsh/evidence/`。
- F2：审查——每个 commit 后 `subagent_review` 五角度；PR 前完整 diff 再审查一轮。
- F3：测试+覆盖率——`cargo test` / `cargo clippy -D warnings` / `cargo fmt --check` 全绿；`collectors` pytest 全绿；覆盖率不低于现有门槛（Rust workspace 93%，collectors 95%）。
- F4：scope fidelity——实现与本文档/issue 验收一致；真实数据冒烟：
  - 跑 `scripts/update-database.sh` 一次；
  - 验证 `capital_main_flow` 补齐 08-13/08-14/08-24/08-25；
  - 验证 5 张派生表补齐缺失交易日；
  - 验证 Dolt/Parquet 行数、日期范围、`data_updates` 锚点；
  - 验证无缺口日不重复拉取/不产生重复行。

## 禁止/边界

- 不 export DuckDB（原约束）。
- 不保留 `sepa_daily.sh` 兼容入口。
- 数据异常禁止静默绕过；按 AGENTS.md 问题处理闭环记录到 `.dsh/kb/dev/toolchain.md`。
- Dolt compass_data 写库后必须 commit+push（由脚本/流程保证）。
