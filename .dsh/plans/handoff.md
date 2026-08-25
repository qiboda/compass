# Handoff — fix import-compass merge-key mismatch

## 用途
修复 `crates/compass-data/src/import_compass.rs` 中 append-table `partition_cols` 与生产 Dolt 主键不一致导致的增量 merge 丢行问题；同时对全部 append/import-compass 表做一致性防漂移测试。

## 已锁定决策（grill-me）
- 用户原话：先“更新数据库”（已完成），随后说“修复”（block_trade bug）；grill 选择：**同时全面审计所有表**。
- 提交位置：**新建 worktree + PR**（用户明确选择）。
- 回归测试范围：**所有表加一致性防漂移测试**（不只 block_trade）。
- 修复后验证：**运行真实增量导入验证**（`import-compass --table block_trade --since 2026-08-21` 确认不再丢行）。
- 不 export duckdb（本轮只修 bug，不出 DuckDB）。

## 已知事实（2026-08-25 记录）
- 仓库 `/data/codes/compass`；Dolt 生产库 `/data/compass-data/compass_data`。
- `block_trade` bug 现象：增量 `import-compass --table block_trade --since 2026-08-21` 报错
  `row count mismatch: merge lost rows old=19724 parquet=8872 (table block_trade)`。
- 根因：`import_compass.rs` `BlockTrade` 的 `AppendTableSpec { partition_cols: "symbol, trade_date, price" }` 太窄；
  生产 Dolt `block_trade` 主键为 `PRIMARY KEY (symbol, trade_date, price, volume, amount, buyer, seller)`。
- 临时恢复：已用全量 `import-compass --table block_trade` 恢复 Parquet 19724 行；代码 bug 未修。
- 审计结果（代码 partition_cols vs 生产 Dolt PK）：
  - `fin_indicators` / `fin_income` / `fin_cash_flow` / `fin_balance_sheet` PK = (symbol, report_date) — 一致
  - `capital_main_flow` PK = (symbol, trade_date) — 一致
  - `dragon_list` PK = (symbol, trade_date, seat_type) — 一致
  - `institution_survey` PK = (symbol, survey_date, org_name) — 一致
  - `index_daily` PK = (symbol, trade_date)（代码 parquet 侧用 tradedate）— 一致
  - `index_basic` 全量覆盖无 merge — 无 partition 键
  - `block_trade` PK = (symbol, trade_date, price, volume, amount, buyer, seller) — **不一致，需修**
- 相关测试常量：`BLOCK_TRADE_SCHEMA`（import_compass.rs 测试内）原为窄 PK `PRIMARY KEY (symbol, trade_date, price)`；已随修复同步为生产完整 PK。

## 流程提醒
- 本 worktree 从 `master` 创建；开始工作前先 `git fetch origin master && git rebase origin/master` 同步基点。
- 按 AGENTS.md 预实现门禁：创建 GitHub issue → 委派 adversarial/requirement test 写 RED → 实现 → 文档同步 → 验证 → commit → review。
- `ref #<issue>` 必须独立成行；`Never auto-push`，push 前用户确认。
