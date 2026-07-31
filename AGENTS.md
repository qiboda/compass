# AGENTS.md — compass

A-share 股票图表桌面应用（egui）。数据管线以本地 Dolt `investment_data` 为**主数据源**
（18M+ 行，6000+ 标的）。GUI 只读本地 Parquet 文件（DuckDB 查询），**无在线回退**。
Python collectors 从 EastMoney API 抓取数据写入 Dolt。

**项目书** = 本项目所有规则与知识文件的统称，包括 `AGENTS.md` 和 `kb/` 目录下所有文件。

**默认对话语言：中文。** 所有回答、解释、讨论默认使用中文，代码注释和提交信息按惯例使用英文。

---

## 品质准则

精益求精，追求完美。每一行代码、每一次提交、每一个决策，都应以最高标准衡量。容不得将就、凑合、差不多。

- 代码不行就重构，不要留着凑合
- 设计不对就推翻，不要叠加补丁
- 流程有漏洞就堵，不要绕过去

---

## ⚡ GRILL-ME FIRST (ALWAYS)

**On EVERY user message in this repo, you MUST load `/grill-me` before responding.**
This is NON-NEGOTIABLE. No exceptions.

The grill-me interview must complete with "shared understanding reached" before
you proceed to any other action — including reading files, classifying the
request, creating todos, or writing code.

**Grill-me completes → must enter PRE-IMPLEMENTATION GATE (below) for any
feature or bugfix work. Grill-me is step 0; the gate is steps 1-4.
Do NOT skip the gate just because grill-me reached shared understanding.**

---

## 🛑 PRE-IMPLEMENTATION GATE (READ BEFORE ANY CODE CHANGE)

**This gate applies to ALL code changes.** The only exceptions are:
- Documentation-only changes (typos, formatting, adding explanations)
- Cargo fmt / clippy fixes (already handled by CI)
- Trivial typo fixes in comments or strings

**Everything else — features, bugfixes, refactors, new commands, CI changes, hooks,
scripts, dependency updates — MUST go through the gate.**

Before you touch a single file, verbalize EACH step to the user and confirm completion:

| Step | Action | Evidence Required |
|---|---|---|
| **1. Issue** | Invoke `/issue-workflow` to create/manage issues | Issue URL(s) shown to user |
| **2. Plan** | If 2+ modules involved: run `/ulw-plan` agent until approval | `.omo/plans/*.md` file created + user approved |
| **3. Tests** | Invoke `/test` (qa skill) to write failing tests | Test output showing failure |
| **4a. Rustdoc** | Invoke `/rustdoc` to verify `#![warn(missing_docs)]` compliance | `cargo doc --no-deps` is warning-free |
| **4b. Docs** | Invoke `/docs` to identify which `kb/` files need updating | List of files to user |

**If ANY step is incomplete, STOP. Do NOT implement. Do NOT create todos. Do NOT edit files.**

### SELF-CHECK (MANDATORY — ask yourself these 4 questions before every code edit)

1. **"Is there a GitHub issue for this work?"** — If not, create one NOW.
2. **"Does my commit message include `ref #N`?"** — If not, add it before committing.
3. **"Have I written a failing test first?"** — If not, write one NOW before the implementation.
4. **"Have I updated the relevant kb/ file?"** — If not, identify the file and update it.

These 4 questions are NOT optional. They are the minimum standard. If you skip any,
you are violating the workflow.

**Test-first is non-negotiable**: any bugfix or feature change MUST start with a
failing test that reproduces the problem (RED), then the fix that makes it pass
(GREEN). This applies to Python (`collectors/tests/`), Rust (`#[cfg(test)]`),
and every language in this repo. Writing the fix before the failing test is an
anti-pattern — see `kb/dev/friction.md`.

### HARD BLOCK

This gate is NON-NEGOTIABLE. The `compass-workflow` skill, when loaded, will
remind you of this gate. If you find yourself writing code without completing
these steps, you are violating the workflow — stop immediately, `git stash` or revert, and go back to step 0.

**Workflow violations are themselves a bug.** If the gate was skipped, the work
is incomplete regardless of code quality. Record the violation in reflections.

### After implementation: Reflection Record

After EVERY feature/bugfix, invoke `/reflect` (reflect skill) to write a
post-implementation reflection and append it to `kb/dev/reflections.md`.

This is MANDATORY — commit it with the implementation or immediately after.

---

## Workflow (MANDATORY)

For all **feature** and **bugfix** work, the `compass-workflow` skill MUST be loaded.
This enforces: issue-driven development, doc-sync, test-first, per-step-verify,
and commit discipline.

**After loading the skill**: immediately run through the PRE-IMPLEMENTATION GATE
checklist above. Do not skip any step.

### Available Skills

| Skill | Slash Command | Purpose |
|---|---|---|
| `compass-workflow` | `/compass-workflow` | Enforces issue-driven dev, doc-sync, test-first, per-step-verify, commit discipline |
| `issue-workflow` | `/issue-workflow` | Creates and manages issues (single + epic/sub-issue decomposition and batch close) |
| `worktree` | `/worktree` | Manage git worktrees for PR development |
| `open-worktrees` | `//open-worktrees` | Launch all worktree zones in separate kitty windows |
| `qa` (test) | `/test` | Write unit/integration tests (TDD/BDD), test coverage |
| `rustdoc` | `/rustdoc` | Verify `#![warn(missing_docs)]` compliance |
| `docs` | `/docs` | Identify and update `kb/` files based on code changes |
| `reflect` | `/reflect` | Write post-implementation reflections with trend analysis |
| `friction` | `/friction` | Record AI behavior corrections to `kb/dev/friction.md` |
| `product` | `/product` | Sprint candidate analysis (read-only, milestone proposals) |

All skills are located under `.opencode/skills/<name>/SKILL.md`. OpenCode
auto-discovers skills from the filesystem — no registration needed.

### Epic & Sub-Issue Workflow

Large requirements spanning multiple modules are decomposed into an **epic**
(parent issue) with **sub-issues** (child issues) via GitHub native sub-issues.
Key rules: one epic = one PR (each sub-issue one commit with `ref #<sub-N>`),
one worktree, batch processing by dependency DAG with manual batch switch,
batch close after merge. Plan files (`.omo/plans/<epic>.md`) track status.

See `.opencode/skills/issue-workflow/SKILL.md` for the full sub-issue lifecycle.

### Issue-Driven Commits

**Every commit must reference a GitHub issue.** No exceptions — not even for
chores, docs, or scripts. The pre-push hook rejects commits without `ref #N`.

For epic work, each commit references its sub-issue (`ref #<sub-N>`).

```
feat: add thing

ref #26
```

### Commit → Review (MANDATORY)

After every commit, always run review. No exceptions.

1. **Commit**: stage changes, write a descriptive message with `ref #N`, commit.
2. **Review**: run review on the committed changes.
3. **Fix**: if review finds issues, fix them and recommit (max 2 rounds).

See `kb/dev/process.md` for full review workflow.

### Commit & Push

Commit and push are **separate operations**. Do not chain them with `&&`.

**Commit**: 直接执行，不需要向用户申请确认。提交是 agent 的职责，按流程 commit 后自动 review。

**HARD BLOCK: Never auto-push.** Wait for the user to explicitly say "push" / "推送".
**Follow the user's exact words.** "commit" means only commit; "push" means only push.

See `kb/dev/process.md` for the full push gate checklist.

### Issue Lifecycle

**HARD BLOCK: Close issues only AFTER push.** An issue is not "done" until the fix is on
`origin/master`. Do not close an issue after commit — wait for successful push.

**Epic close**: after the PR is merged to master, close all sub-issues first, then
close the epic. Record a summary comment on the epic listing all completed sub-issues.

See `kb/dev/process.md` for the full issue lifecycle and `kb/github/labels.md`
for the Bevy-style A-/C-/D-/P-/S- taxonomy. Minimum: one A- and one C- label.

### Scope Discipline

**Never silently change a planned approach.** If an external constraint
(library bug, API incompatibility, missing crate) blocks the agreed-upon
implementation, do NOT work around it by altering the feature design.
Flag the issue to the user and ask for a decision.

The grill-me decisions and the approved plan define the contract. Any
deviation — even a pragmatic workaround — requires user approval first.

---

## Sprint 规划

使用 GitHub Milestones 进行每周 sprint 管理（周一规划 / 周日回顾，周末为核心开发窗口）。
`product` skill 每周一扫描代码库和 open issues，提出 3-5 个候选需求；`/product brainstorm`
可随时手动触发。Sprint 节奏由 `compass-workflow` skill 的 Sprint Rhythm 规则强制执行。

## 摩擦记录

任何「AI 行为偏差被用户纠正」的场合（grill-me 分歧、执行方向偏离、意图误解、约束遗漏等），
都应记录到 `kb/dev/friction.md`：自动检测（用户纠正时提示）或手动 `/friction` 命令。
与 reflections 区分：friction 记录决策过程中的卡点和纠正；reflections 记录实施后的教训。

## 决策记录

所有 `kb/design/` 下的设计文档 MUST 包含 `## 决策记录` 章节，自包含地记录
关键设计决策的 **what + why + why-not**。

- **格式**: 表格 `| 决策 | 选项 | 选择 | 理由 | 排除原因 |`
- **保障**: `compass-workflow` PRE-IMPLEMENTATION GATE Step 4c 检查是否存在
- **自包含**: 决策记录不依赖外部引用（如 friction.md），所有理由直接写在设计文档内

---

## Worktrees

PR 开发使用 git worktrees，位于 `.worktrees/<name>/`（gitignored），每个 worktree 对应
一个 PR/epic，合并后清理。创建后必须执行 `/handoff` + 解绑当前 opencode session。
**加载 `worktree` skill 获取完整流程**（含 post-creation MANDATORY 步骤与清理）。

## Knowledge base

详细文档在 `kb/` 下，按四部分组织。**AGENTS.md 是索引，不是重复** — 细节只在 kb/ 中，绝不在此复制。

| 文件 | 内容 |
|---|---|
| `kb/design/architecture.md` | 系统总览、crate 关系、线程模型、数据管线、存储策略、库选型 |
| `kb/design/data-providers.md` | Provider trait 体系、DuckDbProvider/ParquetReader、错误处理、DDL |
| `kb/design/symbols.md` | A 股市场分段、符号约定、交换所推断、timeframe 映射 |
| `kb/design/roadmap.md` | 产品路线图 — 愿景、已完成、规划中 |
| `kb/dev/testing.md` | rstest + tokio::test 模式、内存 DuckDB、Dolt 测试库、benchmark/Tracy |
| `kb/dev/process.md` | 开发流程、命令、配置、调试、Dolt 操作、重置 |
| `kb/dev/reflections.md` | 事后反思 — 做了什么、哪里出错、教训 |
| `kb/dev/friction.md` | 摩擦记录 — AI 行为偏差与纠正 |
| `kb/user/index.md` | 用户总览 — Compass 是什么、快速开始、前置条件 |
| `kb/user/gui.md` | 图表应用 — 界面、控件、数据流、股票代码 |
| `kb/user/cli.md` | 数据管线 — import/import-compass/export/backup、工作流、排障 |
| `kb/user/config.md` | 配置参考 — 全部选项、默认值、示例 |
| `kb/github/labels.md` | Issue/PR 标签分类 — Bevy 风格 C/A/D/P/S 前缀 |
| `kb/github/comments.md` | 评论规范 — 永远追加，绝不修改 |
| `kb/github/ask.md` | GitHub bot 角色 — /ask 只读问答（工作流按路径加载，勿改） |
| `kb/github/fix.md` | GitHub bot 角色 — /fix 修 bug（工作流按路径加载，勿改） |
| `kb/github/impl.md` | GitHub bot 角色 — /impl 实现功能（工作流按路径加载，勿改） |
| `kb/github/pr-review.md` | GitHub bot 角色 — /review 代码审查（工作流按路径加载，勿改） |
| `kb/github/ci-fix.md` | GitHub bot 角色 — CI 失败诊断（工作流按路径加载，勿改） |

## Setup

- **Rust edition 2024** — requires Rust ≥1.85. Current: 1.96.
- **GUI app** — needs a display server (X11/Wayland). `cargo run` opens a window.
- Logs written to `logs/compass.log` (daily rolling).
- Config at `~/.config/compass/config.toml` (falls back to defaults). 见 `kb/user/config.md`.

## Commands

```sh
cargo build
cargo run                    # GUI chart window
cargo run --bin compass-data -- <subcommand>  # data pipeline CLI
cargo test                   # unit + integration tests
cargo fmt
cargo clippy
RUST_LOG=debug cargo run     # verbose logging
```

### compass-data CLI 速查

```sh
cargo run --bin compass-data -- import                    # Dolt investment_data → Parquet（全量）
cargo run --bin compass-data -- import --since 20260725   # 增量
cargo run --bin compass-data -- import-compass --table stock_basic  # Dolt compass_data → Parquet
cargo run --bin compass-data -- export                    # Parquet → DuckDB
cargo run --bin compass-data -- backup                    # Parquet → 百度云
```

`import-compass`/`export` 默认 merge/skip，`--overwrite` 覆盖；`import` 总是全量直写。
完整选项见 `kb/user/cli.md`。

## Architecture & Data providers

- **架构**: `kb/design/architecture.md` — 线程模型、数据管线、schema、源码布局、库选型
- **数据提供者**: `kb/design/data-providers.md` — DuckDB、Dolt、ParquetReader、DataError
- **符号约定**: `kb/design/symbols.md` — 市场分段、交换所推断、timeframe 映射

**Priority**: Dolt `investment_data` (local) 是主数据源。GUI 数据访问全部本地 — 无在线回退。

### compass_data Dolt 仓库 — 每次数据变更后 commit & push

`/data/compass-data/compass_data` 是 Dolt 仓库（remote:
`doltremoteapi.dolthub.com/skwy/compass_data`）。**每次数据修改**（import、re-import、
schema 变更、data_updates 更新）都必须提交并推送到 remote：

```sh
cd /data/compass-data/compass_data
dolt add <table>...        # or `dolt add .`
dolt commit -m "feat: ..." # describe the data change
dolt push origin main
```

完整 Dolt 操作指南（含跨库查询示例）见 `kb/dev/process.md#dolt-database-queries`。

## Parquet schema & Config

- **Parquet 主数据库结构** 与 **DuckDB DDL**: `kb/design/data-providers.md`（Schema 章节）
- **配置参考**: `kb/user/config.md`（全部选项 + 默认值 + 示例）

## Testing

见 `kb/dev/testing.md` — rstest + tokio::test 模式、内存 DuckDB、Dolt 测试库、benchmark、Tracy 分析。

## API reference

类型级 API 参考见 `cargo doc --open`（`#![warn(missing_docs)]` 强制所有 pub 项带 `///` 注释）。
egui-charts 用法示例见 `kb/user/gui.md` 与 `cargo doc`。
