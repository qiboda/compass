# Handoff: 同步数据库性能（用时）统计

## 用途 / 对应 issue

- 用户原始需求：**“也加一下同步数据库的性能（用时）统计，方便以后优化方案。”**
- 用户此前指示：“先将 python 迁移到 rust，然后再处理这个问题。”——Python→Rust 采集器迁移已在 master 完成（本地 `master = a8882a7`，PR #327/#333 等合并），**现在开始实现同步用时统计**。
- 本 worktree 对应 GitHub issue：**#334**（`feat: 同步数据库性能用时统计`，A-Data/C-Feature/D-Straightforward/P-Medium）。
- 计划已批准：`.dsh/plans/db-sync-timing-stats.md`。

## 已锁定 grill-me 决策（最终契约，实现不得偏离）

1. **统计范围**：全链路分层——
   - `scripts/update-database.sh` 每个步骤（0~8）及总运行时长；
   - `crates/compass-collectors sync` 内每个采集器/来源的 fetch、import 阶段耗时；
   - `crates/compass-data` 子命令（import/import-compass/check-stock-daily/sepa 等）以 shell 步骤级总耗时记录（不深入各命令内部，已有内部 `elapsed_ms` 日志保留）。
2. **输出**：控制台打印人类可读摘要 + 每次运行生成一个**本地 JSON 文件**（用于后续优化对比），不写入 Dolt、不输出 CSV。
3. **文件格式**：每次运行一个 JSON 文件，包含 run 元信息（日期、开始/结束、总时长、阶段数组）与各层计时数据。
4. **Rust 粒度**：`compass-collectors sync` 负责记录每个来源 fetch/import 耗时；`compass-data` 只记录命令级总耗时（shell 包裹即可）。
5. **汇总方式**：`update-database.sh` 作为协调者统一生成最终单 JSON；Rust 同步通过环境变量指定的临时 JSONL/JSON 路径上报结构化 timing 事件（推荐 `COMPASS_TIMING_FILE`），shell 收集并合并。迁移后已全 Rust，不存在原先 Python/Rust 跨语言合并困难。
6. **文件位置**：`logs/` 已 gitignore；建议 `logs/sync-timings/YYYY-MM-DD-<run_id>.json`（或 `logs/sync-timings-<date>.json`，实现时按 plan 确定）。
7. **失败行为**：计时为附加能力，**不应阻断数据更新主流程**；但写入/上报错误必须可见（输出 warning），不得静默忽略（AGENTS 问题处理闭环）。

## 当前架构事实（已核实）

- `scripts/update-database.sh`（迁移后）：
  - step 0 `sync-investment-data.sh`；step 1 `compass-data import`；step 1b `check-stock-daily`；step 2 `cargo run --bin compass-collectors -- sync`；step 3 Dolt commit collectors；step 4 `import-compass` 11 表（stock_basic/index_basic 全量，其余按 anchor 增量）；step 4b `sepa backfill-dates`；step 5 `sepa temperature` + `sepa score --top 50`；step 6 Dolt commit compute；step 7 打印 TOP50。
  - 现有 `run_step()` 只打印 `>>> Step N`，没有计时；失败即 `exit 1`。
- `crates/compass-collectors/src/orchestrate.rs`：
  - `sync(false)` 是 `compass-collectors sync` 主入口；`fetch()` / `import_target()` 分别负责各来源 fetch/import；`backfill()` 负责 auto-heal 回补；已有 `progress` 模块。
  - 源码各采集器模块（如 `main_flow.rs`, `index_daily.rs`, `block_trade.rs`, `dragon.rs` 等）均有 fetch/import 异步函数。
- `crates/compass-data/src/sepa.rs` 与 `crates/compass-strategy/src/sepa/*` 已有部分 `Instant`/`elapsed_ms` 日志（score/temperature/backtest 等），保留即可。
- 项目无 Python 采集器（已删除）；无统一 timer/performance 模块。
- `logs/` 存在且 gitignore。

## 必须走 PRE-IMPLEMENTATION GATE（worktree 会话内自主完成）

- 第一步：**读取本 handoff**。
- 第二步：**同步原始分支**：`git fetch origin master && git rebase origin/master`（当前 master = a8882a7，若有新提交必须同步）。
- 门禁顺序：
  1. 加载 `skwy-github-workflow`，创建 GitHub issue（A-Data/C-Feature）。
  2. 编写 plan（2+ 模块：shell + `compass-collectors` + 可能 `compass-data`/文档），获用户批准后写入 `.dsh/plans/*.md`（建议 `db-sync-timing-stats.md`）。
  3. 委派 `subagent_skwy_adversarial_test`（第 3.5 步）与 `subagent_skwy_requirement_test`（第 4 步）写 RED 失败测试。
  4. 文档同步（第 5b 步）：至少 `.dsh/kb/user/cli.md`（新计时输出/环境变量/文件位置）、`.dsh/kb/design/architecture.md`（管线计时）、`.dsh/kb/dev/database.md`（如有 run 统计说明）；全仓 grep 相关命令/环境变量引用。
  5. 决策记录（第 5c 步）：检查相关 `.dsh/kb/design/*.md` 是否有 `## 决策记录` 章节，缺失则补齐。
- 实现完成后：真实数据冒烟（跑一次 `bash scripts/update-database.sh` 或至少 `compass-collectors sync` 短路径，确认 JSON 生成、控制台摘要合理、主流程无回归）、`cargo test`/clippy/fmt、commit→review→rebase→reflection，interactive 模式等待用户 push 指令。

## 注意事项

- 不要 export DuckDB（保持原约束）。
- 任何 compass_data Dolt 写库后必须 commit+push。
- 计时上报失败仅 warning，不得阻断数据管线；但不得静默吞错。
- 若实现中发现需要用户决策的契约变化（如 JSON schema、是否记录失败步骤），向用户提出，不自行偏离。
