# AGENTS.md — compass

A-share stock chart desktop application built with egui. Data pipeline uses
local Dolt `investment_data` as the **primary data source** (18M+ rows, 6000+ stocks).
The GUI reads exclusively from local Parquet files via DuckDB — no online fallback.
Python collectors use EastMoney API to fetch data into Dolt.

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
| **4a. Rustdoc** | Invoke `/rustdoc` to verify `#[deny(missing_docs)]` compliance | `cargo doc --no-deps` is warning-free |
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
| `rustdoc` | `/rustdoc` | Verify `#[deny(missing_docs)]` compliance |
| `docs` | `/docs` | Identify and update `kb/` files based on code changes |
| `reflect` | `/reflect` | Write post-implementation reflections with trend analysis |
| `friction` | `/friction` | Record AI behavior corrections to `kb/dev/friction.md` |

All skills are located under `.opencode/skills/<name>/SKILL.md`. OpenCode
auto-discovers skills from the filesystem — no registration needed.

### Epic & Sub-Issue Workflow

Large requirements that span multiple modules or independent deliverables are
decomposed into an **epic** (parent issue) with **sub-issues** (child issues) using
GitHub native sub-issue support. See `.opencode/skills/issue-workflow/SKILL.md`
for the full sub-issue lifecycle.

**Key rules for epic work:**

| Rule | Description |
|---|---|
| **Decomposition** | `/ulw-plan` identifies sub-issues during planning; all created upfront via `gh issue create --parent <epic-N>` |
| **Batch processing** | Sub-issues ordered by dependency DAG; independent ones run in parallel, dependent ones serialize |
| **PR strategy** | All sub-issues in one epic → one PR (prevents half-finished features on master) |
| **Commits** | Each sub-issue = one commit with `ref #<sub-N>`; multiple commits in one PR, regular merge |
| **GATE** | Each sub-issue independently walks the full PRE-IMPLEMENTATION GATE |
| **Worktree** | One epic = one worktree (`.worktrees/<name>/`) |
| **Batch switch** | Manual confirmation — agent reports batch completion, user confirms before next batch |
| **Close** | All sub-issues + epic closed via `gh issue close` after PR merges to master |

**Plan file format** (`.omo/plans/<epic-name>.md`):

```markdown
### Batch 1
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #12 | Implement XYZ | — |
| in_progress | #13 | Implement ABC | #12 |
```

Status: `pending` | `in_progress` | `done`. The plan file is the canonical tracking document.

**Sub-issue body template:**

```markdown
> **Parent**: #<epic-N>
> **Plan**: .omo/plans/<epic-name>.md
> **Batch**: <N>
> **Depends on**: #<sub-X> (or "—" if none)

## 描述
...

## 验收标准
...
```

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

## Sprint 规划

使用 GitHub Milestones 进行每周 sprint 管理（周一～周日，周末为核心开发窗口），
以产品视角驱动敏捷开发。

- **周一**：规划 milestone — product agent 自动扫描代码库和 open issues，提出 3-5 个候选需求
- **周日**：回顾完成情况，close 已完成的 milestone
- **手动触发**：`/product brainstorm` 随时获取候补需求

Sprint 节奏由 `compass-workflow` skill 的 Sprint Rhythm 规则强制执行。

## 摩擦记录

任何「AI 行为偏差被用户纠正」的场合，都应记录到 `kb/dev/friction.md`。

- **触发方式**: 自动检测（用户纠正 AI 时提示）或手动 `/friction` 命令
- **范围**: 所有纠正型交互 — grill-me 分歧、执行方向偏离、意图误解、约束遗漏等
- **格式**: `[日期] [关联会话] [我的偏差] [你的纠正] [教训]`
- **与 reflections 区分**: friction 记录决策过程中的卡点和纠正；reflections 记录实施后的教训

摩擦记录由 `compass-workflow` skill 的 Friction Record 规则触发，`/friction` skill 执行写入。

## 决策记录

所有 `kb/design/` 下的设计文档 MUST 包含 `## 决策记录` 章节，自包含地记录
关键设计决策的 **what + why + why-not**。

- **格式**: 表格 `| 决策 | 选项 | 选择 | 理由 | 排除原因 |`
- **保障**: `compass-workflow` PRE-IMPLEMENTATION GATE Step 4c 检查是否存在
- **自包含**: 决策记录不依赖外部引用（如 friction.md），所有理由直接写在设计文档内

## Worktrees

For PR development, load the `worktree` skill. Worktrees live at
`.worktrees/<name>/` — each is a **transient PR workspace**, created for
a single PR and cleaned up after merge. Branch naming: `feat/<short-description>` or `fix/<short-description>`.

After creating a worktree, the skill enforces MANDATORY post-creation steps:
1. `/handoff` → saves context to `.worktrees/<name>/.omo/handoff.md`
2. Tell user to open a new opencode session: `cd .worktrees/<name> && opencode`
3. Current session stays in master — do NOT cd into the worktree.

After PR merge, the skill enforces MANDATORY cleanup:
1. Remove worktree: `git worktree remove .worktrees/<name> --force`
2. Delete PR branch: `git branch -D feat/<name>`

## Knowledge base

Detailed docs under `kb/` — organized into four sections:

| Section | Purpose |
|---|---|---|
| `kb/design/` | Project design — architecture, data providers, symbols |
| `kb/dev/` | Development aids — workflow, process, testing, reflections |
| `kb/user/` | User reference — installation, GUI, CLI, config |
| `kb/github/` | GitHub Action bot role instructions (/ask, /fix, /review, /impl, ci-fix), label conventions, and comment rules |

| File | Content |
|---|---|
| `kb/design/architecture.md` | System overview, crate relationships, threading rationale, data pipeline flows, storage strategy, library decisions |
| `kb/design/data-providers.md` | Trait system design, DuckDbProvider read-through pattern, Parquet/DuckDB/Dolt providers, error handling |
| `kb/design/symbols.md` | A-share market segments, symbol convention rationale, exchange inference, secid mapping, timeframe handling |
| `kb/design/roadmap.md` | Product roadmap — vision, completed, and planned milestones |
| `kb/dev/testing.md` | rstest + tokio::test patterns, in-memory DuckDB, httpmock setup |
| `kb/dev/process.md` | Dev workflow, commands, config, debugging, reset |
| `kb/dev/reflections.md` | Post-implementation reflections — what went wrong, lessons learned |
| `kb/dev/friction.md` | Friction records — AI behavior corrections and lessons |
| `kb/user/index.md` | User overview — what Compass is, quickstart, prereqs |
| `kb/user/gui.md` | Chart app — interface, controls, data flow, stock codes |
| `kb/user/cli.md` | Data pipeline — import, export, workflows, troubleshooting |
| `kb/user/config.md` | Config reference — all options, defaults, examples |
| `kb/github/labels.md` | Issue/PR label taxonomy — Bevy-style C/A/D/P/S prefixes |
| `kb/github/comments.md` | Comment convention — always append, never edit existing |

## Setup

- **Rust edition 2024** — requires Rust ≥1.85. Current: 1.96.
- **GUI app** — needs a display server (X11/Wayland). `cargo run` opens a window.
- Logs written to `logs/compass.log` (daily rolling).
- Config at `~/.config/compass/config.toml` (falls back to defaults).

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

### compass-data CLI

```sh
# Import from Dolt investment_data (primary) into Parquet main database
cargo run --bin compass-data -- import                    # all 6000+ stocks (merge mode)
cargo run --bin compass-data -- import --symbols 000001,600519  # specific stocks
cargo run --bin compass-data -- import --overwrite        # full overwrite (ignore merge)

# Export Parquet to DuckDB
cargo run --bin compass-data -- export
cargo run --bin compass-data -- export --overwrite        # force overwrite
```

All commands default to **merge/skip** behavior (migration-style):
existing unique keys are preserved, only new data is added. Pass `--overwrite`
to replace existing data. Applies to `import` and `export`.

## Architecture

See `kb/design/architecture.md` — threading model, data pipeline, schema, source layout, libraries.

## Data providers

See `kb/design/data-providers.md` — DuckDB, Dolt, ParquetReader, DataError.

**Priority**: Dolt `investment_data` (local) is the **primary** data source.
All GUI data access is local-only — no online fallback.

### Dolt import (`crates/compass-data/src/import_dolt.rs`)

Reads from Dolt `investment_data` (`final_a_stock_eod_price` and `ts_a_stock_list`
tables) via `dolt sql -r parquet` (direct binary Parquet) and `dolt sql -r csv`
(for symbol enumeration), partitioned by Dolt symbol.

### Python collectors

EastMoney data is fetched by Python scripts in `collectors/` using `curl_cffi`
to bypass TLS fingerprinting. Data flows: EastMoney API → CSV → Dolt `compass_data` →
`compass-data import` → Parquet. For secid mapping details, see `kb/design/symbols.md`.

### compass_data Dolt repo — commit & push after every data change

`/data/compass-data/compass_data` is a Dolt repo (remote:
`doltremoteapi.dolthub.com/skwy/compass_data`). **Every data modification**
(import, re-import, schema change, data_updates update) must be committed and
pushed to the remote:

```sh
cd /data/compass-data/compass_data
dolt add <table>...        # or `dolt add .`
dolt commit -m "feat: ..." # describe the data change
dolt push origin main
```

This keeps the remote Dolt database in sync with local data. Check status with
`dolt status`; the working tree should be clean after each push.

## Parquet schema (main database)

```
parquet_data/
├── stock_basic.parquet        # symbol, name, exchange, list_date, delist_date
├── stock_daily.parquet        # symbol, tradedate, open, high, low, close, adjclose, volume, amount
└── stock_daily.symbols.txt    # one symbol per line (fast listing)
```

DuckDB schema (staging):
```sql
CREATE TABLE stock_daily (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    open, high, low, close, adjclose DOUBLE,
    volume, amount DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_basic (
    symbol      VARCHAR PRIMARY KEY,
    name, industry, market, exchange VARCHAR,
    list_date, delist_date DATE
);
CREATE TABLE stock_adj_factor (
    symbol, trade_date, adj_factor, PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_limit (
    symbol, trade_date, up_limit, down_limit, PRIMARY KEY (symbol, trade_date)
);
```

## Config

`~/.config/compass/config.toml` (all fields optional):

```toml
[parquet]
dir = "/data/compass-data/parquet_data"

[dolt]
investment_data_dir = "/data/compass-data/investment_data"
compass_data_dir = "/data/compass-data/compass_data"

[app]
default_symbol = "000001"
default_timeframe = "1d"
```

## Testing

See `kb/dev/testing.md` — rstest + tokio::test patterns, in-memory DuckDB, httpmock setup.

## egui-charts API

- `Bar::new(time, open, high, low, close, volume)` — OHLCV bar
- `BarData::from_bars(bars)` — dataset wrapper
- `Chart::new(data)` — interactive chart widget (pan, zoom, crosshair)
- `chart.set_chart_type(ChartType::Candles)` — candlestick display
- `chart.show(ui)` — render inside any `egui::Ui`


