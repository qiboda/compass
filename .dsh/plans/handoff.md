# Handoff — fix-index-daily-in-daily-pipeline

## Purpose
把 `index_daily`（指数日线）纳入每日数据收集/标准刷新流程，避免数据库更新时指数数据停留在旧交易日。

## User observation (quote)
- 用户说：「指数的数据没有更新？」（2026-08-25 22:28）
- 用户确认要补跑指数日线采集链路，并确认「每日的数据收集也是需要的」「按推荐」。

## Locked decisions
1. 现在补跑指数日线采集链路（数据操作，主 session 正在执行）：
   - `collectors/main.py fetch index_daily`
   - `collectors/main.py import index_daily`
   - commit/push `compass_data` Dolt
   - `import-compass --table index_daily` 更新 Parquet
   - 不 export DuckDB
2. 代码变更：把 `index_daily` 纳入标准刷新流程（用户选择“按推荐”，认为是每日数据收集必需）。
3. 本修复使用独立 worktree：`fix-index-daily-in-daily-pipeline`
   - 与已有 block_trade worktree（`fix/import-compass-merge-key-mismatch`）隔离。
4. 具体代码方向（worktree agent 可细化并走 PRE-IMPLEMENTATION GATE）：
   - 大概率修改 `scripts/sepa_daily.sh`：
     - step 2 增加 `index_daily` 的 fetch + import
     - step 3 collector tables allowlist 加入 `index_daily`
     - step 4 增量锚点查询和数据表列表加入 `index_daily`
     - 更新脚本头部注释（4 sources → 5 sources）
   - 同步修改 `scripts/tests/test-sepa-daily.sh` 的断言（4 sources → 5 sources）
   - 按映射表同步相关 `.dsh/kb/` 文档。

## Newly discovered issue during main-session data operation (2026-08-25)
- 补跑 `collectors fetch/import index_daily` 后，运行 `import-compass --table index_daily --since 2026-08-21` 时第一次出现：
  `DuckDB merge failed: Binder Error: Set operations can only apply to expressions with the same number of result columns, falling back to full export`
- 该 fallback 用 **since 过滤后的数据** 直接覆盖了 `index_daily.parquet`，导致临时只剩 360 行（08-21/08-24/08-25 各 120 行）。
- 已用全量 `import-compass --table index_daily` 恢复为 528,874 行；随后重试增量 `--since 2026-08-21` 成功，未再触发 fallback。当前 Parquet 数据健康。
- 根因初步判断：`import_compass.rs` 的 merge fallback 本身是危险路径——一旦 merge 失败，会以 `--since` 过滤集覆盖全量 Parquet，丢弃历史。即使本次已恢复，这个 fallback 仍是数据丢失隐患。
- 重要：该问题已有已知 issue **#298**（open）：“import-compass incremental merge fallback overwrites parquet with since-filtered data, losing history”。现有 block_trade worktree `fix/import-compass-merge-key-mismatch` 的 commit `bbc0425` 可能已包含 fallback 修复（“widen block_trade merge key and fallback full export”），但尚未合入 master；本 worktree 应避免重复开 issue，并与 #298 / block_trade PR 协调。
- 本 worktree 至少应记录到 `.dsh/kb/dev/toolchain.md`；是否把 fallback 修复也纳入本 PR 由 worktree agent 判断/与用户确认。

## Process reminders (AGENTS.md)
- 这是 feature/bugfix/代码变更，必须走 PRE-IMPLEMENTATION GATE：
  - 创建 GitHub issue（含 A- 与 C- 标签）
  - 失败测试（RED，委派 skwy-requirement-test / skwy-adversarial-test）
  - 文档同步（第 5b 步）
  - 决策记录（第 5c 步：检查 `.dsh/kb/design/` 相关文档是否有 `## 决策记录`）
- worktree 会话启动后第一步读取本 handoff；然后 `git fetch origin master && git rebase origin/master` 同步原始分支再开始。
- 提交信息必须含独立成行的 `ref #N`；不要 push 除非用户明确说"push"。

## Current state notes (as of handoff)
- master at `accee7f`; new branch `fix/index-daily-in-daily-pipeline` based on master.
- 数据现状：Dolt `index_daily` max `trade_date = 2026-08-25`（528,874 行），Parquet 已同步到 08-25（528,874 行）；市场 `stock_daily.parquet` 已到 08-25。
- 根因：本轮“更新数据库”只做了 Dolt→Parquet 导入，没有跑 Python 采集器；`sepa_daily.sh` 原本只采集 4 个 SEPA 源，不包含 `index_daily`。
- 本轮主 session 已在后台补跑 `collectors/main.py fetch index_daily` + `import index_daily`，并已 commit/push Dolt 与更新 Parquet。

## Note
本 worktree 的 `.dsh/plans/handoff.md` 在 master 上是被 git 跟踪的旧文件（内容原为 issue #299 财务三表 UPDATE_DATE 增量）；本次覆盖为本修复上下文；worktree 初始 sync/rebase 可能把该跟踪文件重置回旧内容，本次再次覆盖，请以本文件为准。
