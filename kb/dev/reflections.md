# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

旧的条目若已驱动具体流程改进（gate 硬阻、pre-push hook、自举测试等），则退役 — 教训已融入流程。

---

## 2026-07-26 — ref #31 fix: DuckDbProvider 直读 parquet_data，消除 cache miss

**What was done**: 将 DuckDbProvider 从文件型 DuckDB (`compass.db`) 改为内存型 + Parquet 回退。
`fetch_bars` 先查内存表（`save_bars` 的 EastMoney 数据），miss 时通过 `read_parquet()` 直接读取
`parquet_data/stock_daily/{EXCHANGE}{code}.parquet`。GUI 启动即可命中本地数据，不再每次走 EastMoney HTTP。

**What went well**: RED→GREEN 严格 TDD，3 个新测试精准覆盖首次查询、日期过滤、save_bars 优先级。

**What went wrong**: 初期方案考虑了 glob VIEW 方案，但用户明确指向直读 parquet 文件，避免了过度设计。

**Lessons learned**:
1. 直读 parquet 文件比 glob VIEW 更简洁，DuckDB 的 `read_parquet()` 对单文件查询已足够高效
2. 列名映射 (`tradedate` → `trade_date`) 和符号映射 (`000001` → `SZ000001.parquet`) 是跨存储格式的核心细节
3. CLI 工具仍需文件型 DuckDB，保留 `new_file()` 构造函数是必要的分离

## 2026-07-26 — ref #24 refactor: integrate egui-mobius Level 3 citizen pattern

**What was done**: Replaced manual mpsc + Arc<Mutex<CompassState>> architecture with
egui-mobius Level 3 (AsyncDispatcher + typed signal/slot + Dynamic<T>). Converted
single-panel CentralPanel layout to 3-citizen DockArea (Control, Chart, Logger).
Removed dead code: bars_version, search_results, Cmd::SearchSymbols, retry_count.
Upgraded egui from 0.33 to 0.35, switched egui-charts to qiboda fork.

**What went well**: Grill-me locked all architectural decisions before implementation.
Plan-first approach produced a structured 14-task plan with wave-based parallelism.
Zero compilation errors on first build after each wave.

**What went wrong**: egui_citizen not published on crates.io — git dependency required.
egui_lens 0.5.0 panic bug from `ReactiveEventLoggerState::default()` (max_logs=0) —
fixed by using `new()` instead.

**Lessons learned**:
1. Grill-me as step 0 is effective — 9 decisions locked before plan creation.
2. Wave-based plan decomposition enables true parallelism.
3. Never silently change a planned approach — added Scope Discipline rule to AGENTS.md.

## 2026-07-25 — ref #16 fix: pre-push hook new-branch range scans only branch commits

**What was done**: Changed issue-reference validation in `.beads/hooks/pre-push` to use
`git merge-base origin/master` for new branches instead of scanning all reachable history.

**What went wrong**: `$local_oid` scanned entire commit history, flagging closed issues from old master commits.

**Lessons learned**: `git log $sha` without range prefix scans all ancestors — use `merge-base..$sha`.

---

## 2026-07-25 — chore: add worktree management skill

**What was done**: Created `worktree` skill, standardized conventions, cleaned orphan worktree.

**Lessons learned**: Process docs reference skills, don't duplicate them. Worktrees need cleanup discipline.

---

## 2026-07-26 — feat: 创建 compass_data Dolt 仓库 #25

**What was done**: 在 `investment_data` 同级创建 `compass_data` Dolt 仓库，含 `stock_basic` 和
`fin_indicators` 两张表，验证跨库 JOIN（ts_code 直连 ts_a_stock_list，symbol 直连
final_a_stock_eod_price）。

**What went wrong**: Dolt SQL 里 `||` 是逻辑 OR 而非字符串拼接 — 转用 `CONCAT()` 解决。
investment_data 的 exchange 值并非文档中的 `SHSE/BSE` 而是 `SSE/BSE`，跨库映射需以实际值为准。

**Lessons learned**: Dolt 方言和 MySQL 一致，`||` 不等于拼接；跨库查询需从父目录运行 `dolt sql` 无 `--data-dir`。

---

## 2026-07-26 — 流程违规: import-compass/backup 等多项 feature 工作跳过 PRE-IMPLEMENTATION GATE

**What went wrong**: 当天完成了 import-compass 命令、Backup 上传、fin_indicators 增量、
CI 修复等多项 feature 工作，全部跳过了 PRE-IMPLEMENTATION GATE（无 GitHub issue、
无 plan、无 test-first）。

**Lessons learned**:
1. GATE 对 "所有代码变更" 的约束力不足 — 已补充 SELF-CHECK 4 问硬性规则
2. "feature 工作" 的边界太模糊，容易自欺"这不是 feature" — 改为白名单例外（仅 docs/lint/typofix）
3. 流程违规本身即是 bug — 记录到 reflections 并修复流程

---

## 2026-07-26 — ref #43 feat: GUI layout rework + SH/SZ/BJ exchange selection

**What was done**: Replaced the 3-citizen DockArea layout with a toolbar + 2-citizen
DockArea (Chart, Logger). Added Exchange enum (SH/SZ/BJ/All), searchable symbol dropdown
loaded from `stock_basic.parquet`, exchange-filtered symbol list, and exchange prefix
auto-prepending. Removed ControlCitizen and the outbox pattern from the main loop.

**What went well**: All gate steps completed (issue #43, ulw-plan, RED phase tests, docs).
Momus review passed with no blocking issues. TDD approach caught sort order bug in
filter_stocks() before manual testing.

**Lessons learned**:
1. egui 0.35 doesn't have `TopBottomPanel` — used inline horizontal toolbar instead
2. Module visibility: `mod widgets` declared in main.rs makes `crate::widgets` accessible from test modules
3. The `#![warn(missing_docs)]` lint is aggressive — every public item needs a doc comment, even enum variants

## 2026-07-27 — ref #54 feat: OpenCode GitHub 多工作流架构

**What was done**: 将单一 `opencode.yml` 拆分为 5 个专用工作流（/ask、/fix、/review、
/impl、CI-fix），每个工作流有独立的触发条件、权限和角色指令。AGENTS.md 采用 Common
Baseline + Role Overlay 策略——AGENTS.md 保持不变作为项目约定基线，GitHub 角色专用指令
放在 `kb/github/*.md`，workflow prompt 仅做文件路由。Momus 审议通过，actionlint 零错误。

**What went wrong**: 源码审查发现 opencode GitHub Action 的 `assertPayloadKeyword()`
硬编码了 `/opencode` 和 `/oc` 关键词检查，自定义命令（/ask、/fix 等）可能在 action
内部被拒绝。待部署后实测验证 `mentions` 输入是否生效。

**Lessons learned**:
1. GitHub Actions 的 `workflow_run` 事件仅从默认分支触发 workflow 定义，但可以响应
   任意分支的 workflow 完成事件——只要 workflow 文件在默认分支上。
2. prompt 输入的行为应实测确认（覆盖 vs 追加），计划中采用保守策略：prompt 只做文件路由。
3. YAML workflow 的 actionlint 验证可替代传统 TDD 的 RED 阶段（config 文件无编译/运行测试）。



**What was done**: Moved parquet_data, investment_data, compass_data out of project to
`/data/compass-data/`, made all paths configurable via `[parquet]` / `[dolt]` in
config.toml, removed dead `DatabaseConfig` and `merge` command, updated all kb/
docs and AGENTS.md, added `scripts/link-data-dirs.sh` for worktree data access.

**What went wrong**: `dolt clone chenditc/investment_data` is too large for tool
timeouts (~4GB, 16M chunks). Had to background it with `nohup`. Push to
skwy/investment_data and `import --overwrite` still pending.

**Lessons learned**:
1. Large data operations should always go through background/nohup, never inline
2. `cargo check` is fast but doesn't catch missing deps in Cargo.toml — compass-data
   needed `toml` and `serde` added for config loading
3. Removing structs from AppConfig is safe as long as no production code consumes them
   (confirmed via grep before deleting DatabaseConfig)

## 2026-07-28 — ref #55 worktree 改为 PR-only + 删除 symlink 脚本 + issue 关闭记录 PR

**What was done**: Changed worktree from persistent functional zones to transient
PR-only workspaces (create per PR, cleanup on merge). Deleted `scripts/link-data-dirs.sh`
(no longer needed — paths are config-driven). Added PR recording step to issue
close flow. Fixed branching rule in compass-workflow skill (was "trunk-based, no
feature branches" — corrected to feature-branch workflow with `pr/` naming).
Standardized branch naming to `pr/<short-description>` across all docs.

**What went wrong**: Compass-workflow SKILL.md still had stale "no feature branches"
rule conflicting with the new PR-only workflow. Caught during cross-file review.

**Lessons learned**: When changing a convention that spans multiple files, grep
ALL markdown files for the old pattern — not just the ones you're editing.

## 2026-07-28 — ref #56 自动 review + 修复 + 提 issue + 提交策略全局化

**What was done**: Replaced manual POST-IMPLEMENTATION SELF-AUDIT checklist
with an automated 5-step review flow: commit strategy decision → `/review-work`
→ auto-fix (≤3 files, related) or create issue (unrelated or >3 files) →
re-review (max 2 rounds) → finalize. Added global commit strategy rule to
AGENTS.md (large changes commit-first, small changes fix-first).

**What went wrong**: Nothing. Grill-me decision tree resolved cleanly.

**Lessons learned**: A single `>3 files` threshold is clear enough — no need
to define "large" vs "small" change through subjective criteria.

## 2026-07-28 — ref #57 docs: /fix /impl role instructions mandate PR-based commits

**What was done**: Modified `kb/github/fix.md` and `kb/github/impl.md` to
mandate PR-based commit workflow — all code changes from OpenCode bots must
go through PR branches, never direct push to main. Review caught a `Fixes #N`
vs `Addresses #N` auto-close contradiction, and fix.md quality gate asymmetry
with impl.md.

**What went wrong**: Skipped the post-implementation review before first commit.
Correctly identified that PRE-IMPLEMENTATION GATE has a "doc-only" exception,
but incorrectly assumed the POST-IMPLEMENTATION REVIEW also has one. The two
are separate processes — the gate is for starting work, the review is for
finishing work. Doc-only only skips the gate, not the review.

**Lessons learned**:
1. PRE-IMPLEMENTATION GATE exceptions ≠ POST-IMPLEMENTATION REVIEW exceptions.
   If you skip the gate because of an exception, the review still applies.
2. Added explicit warning to compass-workflow SKILL.md gate exceptions section
   to prevent this mental shortcut from recurring.
