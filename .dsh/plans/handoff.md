# Handoff — Fix complete Compass data daily refresh

## 用途
把 `scripts/sepa_daily.sh` 从 SEPA-only 扩成**完整 compass_data 每日刷新入口**，覆盖
`stock_basic` + 财务四表（`fin_indicators`, `fin_balance_sheet`, `fin_income`,
`fin_cash_flow`）+ SEPA 表（`capital_main_flow`, `dragon_list`, `block_trade`,
`institution_survey`）+ 指数（`index_daily`/`index_basic`）。同时修正文档中对
「每日一键流水线」的不准确描述。

## Issue URL
https://github.com/qiboda/compass/issues/306

## 已锁定 grill-me 决策
- Q1 选 **1**：扩展现有 `scripts/sepa_daily.sh`，不新建 update-all.sh。
- Q2 选 **1**：包含 `stock_basic`（全部 compass_data 表）。
- Q3 选 **1**：step 2 改用 `uv run python main.py sync`，避免 shell 重复维护源列表；
  同步扩展 Dolt allowlist 和 import-compass 表清单。
- 用户指令：「修复一下，然后再更新数据。」 → 先修脚本（走 PRE-IMPLEMENTATION GATE），
  再跑修复后的脚本更新全部数据。
- 不执行 `export`（DuckDB）。

## 根因（已诊断）
- `scripts/sepa_daily.sh` step 2 只 fetch/import `main_flow dragon block_trade
  institution_survey index_daily`；`COLLECTOR_TABLES` 只有 6 表；step 4 只
  import-compass 这 6 表。财务四表与 `stock_basic` 没有进每日管线。
- `collectors/main.py sync` 已是全量刷新入口（`fetch all + import all`），
  并非能力缺失，是流程用错脚本。
- `.dsh/kb/user/cli.md` 把 sepa_daily.sh 称为“每日一键流水线”，但未说明不含财务表。

## 后续步骤（worktree 会话自主完成）
1. `git fetch origin master && git rebase origin/master`（同步原始分支）。
2. 按 `skwy-workflow` 门禁完成：issue → plan → 对抗性测试/需求测试（RED）→ 实现 →
   文档同步（`.dsh/kb/user/cli.md` 等）→ 决策记录 → 提交/review/PR。
3. 真实数据冒烟：跑修复后的 `scripts/sepa_daily.sh`，验证 Dolt/Parquet 全部表
   （含财务表）更新、行数与 data_updates 锚点一致。
4. 完成后关闭 worktree 并通知主 session。

## 关键路径
- 主 repo `/data/codes/compass`（master `dd83939`）
- Dolt compass_data `/data/compass-data/compass_data`；investment_data
  `/data/compass-data/investment_data`
- `scripts/sepa_daily.sh` 需修改；`collectors/main.py sync` 现有全量能力
- `cargo run --bin compass-data -- import-compass --table <table> [--since ...]`
