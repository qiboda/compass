# Handoff — fix/mainflow-null-rows-filter

## 用途
修复 `capital_main_flow` 历史回补把退市/无数据股票写成 NULL 空行的 bug，并清理现有脏数据。

- GitHub issue: https://github.com/qiboda/compass/issues/348
- 基础分支: master @ `1ea7dbe`（创建时即 origin/master，开始工作前仍应 `git fetch origin && git rebase origin/master`）

## 背景与证据
2026-09-02 更新数据库后抽查 2026-08-19 ~ 2026-09-02（11 个交易日）发现：
- `capital_main_flow` 全表 134,755 行，其中 3,042 行 `main_net_inflow IS NULL`（占 2.26%）；最近两周窗口内 2,027 行。
- 3,042 行的 `main_net_inflow / main_net_inflow_rate / super_large_net / large_net / medium_net / small_net` 全部为 NULL。
- 其中 2,997 行是已退市股票在退市日期之后仍被写入（如 SH600001 邯郸钢铁 2009 退市、SH600005 武钢股份 2017 退市在 2026-08-19 有 NULL 行）；仅 45 行为上市中但当日未成交/无数据。
- NULL 行集中在 2026-07-31、08-17~21、08-26~28，每天约 336~340 行。Dolt 与 Parquet 都包含这些空行（不是 Dolt↔Parquet 不一致）。

## 根因
`crates/compass-collectors/src/main_flow.rs` 的 `backfill_symbols()`（约 line 397-414）直接执行
`SELECT symbol FROM stock_basic ORDER BY symbol`，未按 `list_date` / `delist_date` 过滤。
`run()`（每日采集，约 line 252）和 `backfill(start,end,symbols)`（历史回补，约 line 420）都使用该函数，
导致退市/无数据股票也被拉取并写入 Dolt。

## 已锁定决策（grill 确认）
1. **代码修 + 清理现有脏数据**：修 backfill 过滤逻辑，并删除 Dolt/Parquet 中已有 NULL 空行。
2. **每日采集也一起过滤**：daily fetch 和 historical backfill 共用 active-symbol 过滤。
3. **手动指定 symbols 也统一过滤**：显式传 `--symbols` 时同样按活跃区间过滤。
4. 预期过滤条件：
   - `list_date` 为空或 `CAST(list_date AS DATE) <= end`
   - `delist_date` 为空或 `delist_date >= start`
   - 即只拉取目标区间内至少有一天处于上市状态的股票。
5. 防御性导入守卫：禁止把 `main_net_inflow` 为空/NULL 的行写入 `capital_main_flow`。

## 关键代码位置
- `crates/compass-collectors/src/main_flow.rs`
  - `SINA_URL`, `SINA_DAILY_NUM=20`, `SINA_BACKFILL_NUM=1000`, `SINA_BACKFILL_RETRIES=3`, `SINA_BACKFILL_BACKOFF=2s`
  - `run()`：每日窗口；`backfill_symbols()` 取全部 stock_basic
  - `backfill(start,end,symbols: Option<&[String]>)`：`symbols=None` 时调 `backfill_symbols()`
  - `import_to_dolt()`：`INSERT IGNORE INTO capital_main_flow ... WHERE symbol IN (SELECT symbol FROM stock_basic)`
- `crates/compass-collectors/src/orchestrate.rs`
  - `DAILY_AUTO_HEAL_TABLES`、`auto_heal()`、`backfill()` 对 capital 调 `main_flow::backfill(start,end,None)` 后 `require_nonzero`
- `stock_basic` schema：`symbol`, `list_date varchar(20)`（形如 `2020-12-23 00:00:00`），`delist_date date`（可空）；Dolt `CAST(list_date AS DATE)` 可用。

## 实施建议
- 建议把 active-symbol 查询抽成纯函数/独立 helper（可测试）：如 `active_symbols(start, end)` 或 `filter_active_symbols(symbols, start, end)`。
- `run()` 日常用 `active_symbols(today, today)` 或等价；`backfill()` 用 `active_symbols(start, end)`。
- 显式 `symbols` 也要过过滤；过滤后为空不应误报“无 symbols”导致整个 sync 失败（按现有语义决定，参考 no-op 语义）。
- 导入守卫可在 `import_to_dolt` 插入 SQL 加 `AND main_net_inflow IS NOT NULL`（或等价格式），避免任何路径写入空行。
- 清理现有数据：
  1. `DELETE FROM capital_main_flow WHERE main_net_inflow IS NULL`（当前 3,042 行，所有 flow 列均为 NULL）
  2. Dolt commit + push
  3. 无 `--since` 全量 `cargo run --bin compass-data -- import-compass --table capital_main_flow` 重建 Parquet
- 不 export DuckDB。

## 流程提醒
- 走 PRE-IMPLEMENTATION GATE：plan（.dsh/plans/*.md）→ 对抗性测试 RED → 需求测试 RED → 实现 GREEN → 文档同步 → 决策记录 → 真实数据冒烟 → commit + review → PR。
- 每个 commit 必须含独立行 `ref #348`；push 前先 rebase origin/master；push 后追加完成 comment 并关闭 issue #348。
- Dolt 任何写库后必须 commit/push，工作区必须 clean。
- 遇到异常禁止静默绕过，按问题处理闭环记录到 `.dsh/kb/dev/toolchain.md` 或 reflections。
