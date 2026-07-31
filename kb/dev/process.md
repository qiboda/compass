# Development Process

## Issue-driven workflow

The complete development cycle for features and bugs:

```
User raises requirement
  →  OpenCode grills (/grill-me) to clarify scope and decisions
  →  Shared understanding reached → summarize locked-in decisions
  →  OpenCode creates GitHub issue (feature_request or bug_report template)
  →  OpenCode shows issue with gh issue view <N>
  →  /ulw-plan (if multi-step)  →  plan may identify sub-issues for epic decomposition
  →  For epics: /issue-workflow creates epic + sub-issues upfront, batches by DAG
  →  implement (each sub-issue walks GATE independently)
  →  cargo nextest (tests must pass)
  →  commit with ref #<sub-N> (epic) or ref #N (single issue)
  →  ai-review (/review-work) — per sub-issue + pre-PR
  →  push master (one PR with all sub-issue commits)
  →  CI passes  →  batch close sub-issues + epic with gh issue close
```

Docs, lint fixes, and typos skip the grill-me + issue cycle — implement directly.

| Work type | Issue required? |
|---|---|
| Feature | ✅ Required |
| Bug fix | ✅ Required |
| Refactor | ✅ Required |
| Docs update | ❌ Skip |
| Lint / typo | ❌ Skip |

### Epic & Sub-Issue Workflow

Large requirements are decomposed into an **epic** (parent issue) with **sub-issues**
(child issues) using GitHub native sub-issue support (`gh issue create --parent <epic-N>`).

#### Epic creation flow

1. `/ulw-plan` identifies sub-issues during planning — all created upfront
2. `/issue-workflow` creates the epic + all sub-issues in one batch
3. Each sub-issue body includes: Parent, Plan, Batch, Depends on metadata
4. `.omo/plans/<epic>.md` tracks status via `pending | in_progress | done` table

#### Batch execution

- Sub-issues ordered by dependency DAG (topological sort)
- Independent sub-issues in the same batch run in parallel (multiple subagents in the same worktree)
- Dependent sub-issues serialize — blocked items wait for their dependencies to complete
- Batch switching is **manual** — agent reports completion, user confirms before next batch
- New sub-issues discovered during execution: allowed; update plan file and re-evaluate DAG

#### One PR, multiple commits

All sub-issues in one epic ship in a **single PR** to prevent half-finished features on master.
Each sub-issue is one commit (`ref #<sub-N>`). Merge strategy: regular merge (not squash) —
preserves commit history and issue traceability.

#### Review

- Per sub-issue: review after each sub-issue commit (`/review-work`)
- Pre-PR: review full PR diff after all sub-issues complete

#### Close

After PR merges to `master`:
1. Close all sub-issues: `gh issue close <sub-N1> <sub-N2> ...`
2. Close the epic: `gh issue close <epic-N>`
3. Record summary on epic listing all completed sub-issues and PR

### When OpenCode discovers a new bug

1. Create issue using `.github/ISSUE_TEMPLATE/bug_report.md` template
2. Read it back (`gh issue view <N>`) to confirm it exists
3. Fix it — commit with `ref #N`

### Commit → issue linking

| Commit type | Issue reference |
|---|---|
| feat / fix (single issue) | `ref #N` |
| feat / fix (epic sub-issue) | `ref #<sub-N>` |

Issues are closed **manually** via `gh issue close N` after verification.
Do NOT use `fixes #N` or `closes #N` — these auto-close the issue on push.
For epic work, batch-close all sub-issues first, then the epic.

### Commit-msg hook

A git hook (`.githooks/commit-msg`) enforces issue references:

```
Every commit must include "ref #N" — no exceptions.
feat, fix, test, refactor, docs, chore — all included.
```

The hook is activated via `git config core.hooksPath .githooks` (already configured).

## OpenCode workflow

### When to plan first

Use `/ulw-plan` (plan → approve → execute) for:
- Changes touching 2+ modules
- Architecture changes (threading, data pipeline, new provider traits)
- New data sources or external API integration
- Symbol format or config schema changes

Skip planning for:
- Single-file bugfixes
- Adding tests
- Documentation updates
- Typo / lint fixes

### Before implementing

Read the GitHub issue with `gh issue view <N>` to catch corner cases,
reproduction steps, and expected behavior not captured in the plan.

### Per-step verification

After every code change:
```sh
cargo nextest run           # all tests must pass
```
Ensure `lsp_diagnostics` is clean on changed files.

**Doc comment discipline**: every `pub` item added or modified in
`compass-core` MUST have a `///` doc comment. This is enforced by
`#![warn(missing_docs)]` — `cargo doc --no-deps` must be warning-free.
Doc comments are part of the code, not an afterthought. Write them
as you write the implementation, not after.

### Before pushing

**Push gate**: `git push` requires explicit user instruction. The agent may
commit freely but must NOT push unless the user explicitly says "push",
"提交push", or equivalent. No implicit pushing — even after passing all
quality gates.

The pre-push hook (`.githooks/pre-push`) enforces these checks in order:

1. **CI health**: latest CI run on `master` must be passing. If it's failing,
   create an issue for the failure, fix it, then push. Never push on top of
   a broken CI.
2. **cargo fmt --check**
3. **cargo clippy -- -D warnings**
4. **cargo doc --no-deps** (must be warning-free)
5. **Issue references**: `ref #N` must point to open issues

Manual pre-push checklist:

1. `cargo fmt --check` — code must be formatted
2. `cargo clippy -- -D warnings` — no warnings
3. `cargo doc --no-deps` — must be warning-free
4. Issue references: `ref #N` must point to open issues

All four must pass before `git push`. Never push broken code.
`cargo doc --no-deps` verifies that `#![warn(missing_docs)]` in
`compass-core` is clean — every public item must have a `///` doc comment.

### Quality review

After committing and before pushing, run `/review-work` — launches 5 parallel
agents (goal verification, code quality, security, QA execution, context mining).

Skippable for: docs, lint fixes, typos, trivial chores.

### Push rhythm

For single issues: push immediately after completing the issue. Do not batch.

For epic work: push after all sub-issues in the PR are complete. One push per PR.

### Commit discipline

- Each commit = one logical unit. Never mix bugfix + feature + refactor.
- Conventional commits: `feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`.
- Issue linking: `feat`/`fix` commits use `ref #N` (issues closed manually after push).
- Template: `git config commit.template .gitmessage` is already set.

## Git branching

**Feature-branch workflow.** Most work happens on feature branches, merged via PR.
Trivial fixes (typo, config, one-line change) can go directly to master.

```
master  ●──●──●──●────────●  (trunk)
              \          /
feat/xxx       ●──●──●──┘   (feature branch, PR, merge)
```

**Merge strategy**: Use regular merge (not squash). Preserves all commit
history — each commit maps to an issue reference (`ref #N`), and losing that
granularity would break traceability.

### PR merge workflow

After a PR is merged but before closing the related issue, add a comment on the
issue noting any deviations between the actual changes and the PR description:

- What was implemented differently from the PR description
- What was omitted or deferred
- Any unplanned changes that were included

This keeps issues as an accurate record of what was actually shipped.

```
gh issue comment <N> --body "PR #M 已合并。与 PR 描述不一致之处：
- ..."
```

## Worktrees (transient PR workspace)

Worktrees live at `.worktrees/<name>/` (gitignored). Each worktree is a
**transient PR workspace**, created for a single PR or epic and cleaned up
after merge. Branch naming: `feat/<short-description>` or `fix/<short-description>`.

**加载 `/worktree` skill 获取完整流程**（创建、post-creation MANDATORY 步骤、
`/handoff`、解绑 opencode session、合并后清理）。

**Why not plugins**: The `opencode-worktree` plugin (kdco/worktree via OCX)
was evaluated and found to have blocking issues (no idempotent re-open,
unreliable terminal spawn, no session reopen). Manual worktrees +
the `/worktree` skill give full control without those issues.

## Version control

```sh
git add <files>              # stage only intended changes
git commit                    # uses .gitmessage template
git push origin main          # triggers CI
```

## Quickstart

```sh
cargo run                       # launch the GUI app (needs X11/Wayland)
RUST_LOG=debug cargo run        # verbose logging
```

### CLI (compass-data)

完整子命令参考见 `kb/user/cli.md`。速查：

```sh
cargo run --bin compass-data -- import                    # Dolt investment_data → Parquet（全量）
cargo run --bin compass-data -- import --since 20260725   # 增量
cargo run --bin compass-data -- import-compass --table stock_basic  # Dolt compass_data → Parquet
cargo run --bin compass-data -- export                    # Parquet → DuckDB
cargo run --bin compass-data -- backup                    # Parquet → 百度云
```

## Adding a feature (manual)

If working without OpenCode:

1. **Explore** the relevant source files (`kb/design/architecture.md` for layout).
2. **Test first**: Write a failing test in `#[cfg(test)] mod tests`.
3. **Implement** in the source file.
4. **Verify**: `cargo nextest run` + `lsp_diagnostics`.
5. **Update docs** if the change affects architecture, symbol format, or config.

### Knowledge base sync

Every code change that affects behavior, APIs, data structures, config,
workflows, or conventions must update the relevant `kb/` file in the
same commit. AGENTS.md must be updated if the architecture overview changes.

权威的「变更类型 → kb/ 文件」映射表见
`.opencode/skills/docs/SKILL.md` § Change → kb/ Mapping Table。

### Documentation conventions

**kb/design/ files must use narrative, developer-onboarding style.**
A reader new to the project should understand not just _what_ but _why_.
Every design decision must be accompanied by its rationale: the problem
it solves, the alternatives considered, the trade-offs accepted.

**API reference belongs in `cargo doc`, not kb/.**
Use `///` doc comments on public types, traits, and functions.
`kb/design/` explains design intent and architecture; `cargo doc`
handles the precise API surface. The two complement each other —
kb/ tells the story, rustdoc provides the reference.

**Never hardcode version numbers in kb/.** `Cargo.toml` is the single
source of truth for dependency versions. kb/ docs may mention crate
names and their purpose, but not `= "0.25"`.

**AGENTS.md is an index, not a duplicate.** It points at kb/ files
with one-line summaries. Full explanation lives in kb/, never repeated
in AGENTS.md.

## TDD workflow

Feature and bugfix work follows TDD (Test-Driven Development):

```
DESIGN TESTS → RED → GREEN → REFACTOR
```

0. **DESIGN TESTS**: Write a **test case document** (inline comment block in the test
   module or a separate `#[doc]` block) listing every scenario the tests must cover:

   ```
   // Test cases:
   // 1. Normal input — returns expected result
   // 2. Empty input — returns empty/default
   // 3. Boundary values — min/max handled correctly
   // 4. Error paths — invalid input produces proper error
   // 5. Edge cases — null/missing fields, very large values, etc.
   ```

   This ensures test coverage is comprehensive and prevents blind-spot bugs.
   The test case list serves as a checklist — every item must have at least one
   corresponding `#[test]` or `#[case]` before the implementation is considered done.

1. **RED**: Write a failing test that documents the expected behavior.
   - Test must fail before any implementation code exists.
   - If it passes immediately, delete or rewrite — it's testing nothing.
   - Verify each scenario from the test case document is covered.
2. **GREEN**: Write the minimal implementation to make the test pass.
3. **REFACTOR**: Clean up the code while keeping tests green.

Exploratory changes (new API integration, architecture experiments) may
write tests after implementation to lock in behavior.

## Running tests & code quality

测试运行、benchmark、Tracy profiling 见 `kb/dev/testing.md`。速查：

```sh
cargo nextest run                       # 推荐
cargo fmt --check                       # verify formatting
cargo clippy -- -D warnings             # strict lint
```

## Config

Config 位于 `~/.config/compass/config.toml`，全部字段可选，缺省回退到
`crates/compass-core/src/model.rs` 中的默认值。完整选项见 `kb/user/config.md`。

## Logs

- Stderr: always. `RUST_LOG` controls level (`error`, `warn`, `info`, `debug`, `trace`).
- File: `logs/compass.log` (daily rolling).

## Debugging tips

### Check what the EastMoney API returns

```sh
# K-line API
curl "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=0.000001&klt=101&fqt=1&beg=20250101&end=20250721&lmt=10&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61"

# Symbol listing API
curl "https://push2delay.eastmoney.com/api/qt/clist/get?pn=1&pz=3&fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048&fields=f12,f14&ut=bd1d9ddb04089700cf9c27f6f7426281"
```

### Inspect Parquet files

```sh
ls -lh parquet_data/stock_daily.parquet
wc -l parquet_data/stock_daily.symbols.txt    # symbol count
```

### Query Parquet with DuckDB

```rust
use duckdb::Connection;
let conn = Connection::open_in_memory()?;
conn.execute_batch("SELECT * FROM read_parquet('parquet_data/stock_daily.parquet') WHERE symbol = 'SH600519' LIMIT 5")?;
```

### collectors (Python data pipeline)

Fetch data from EastMoney APIs into CSV, then import into `compass_data` Dolt.
命令与工作流见 `kb/user/cli.md` § Python collectors 与 `kb/design/architecture.md` § collectors。

Key concepts:
- **curl_cffi** for TLS impersonation (EastMoney anti-crawler)
- **CSV as intermediary** between API and Dolt
- **`.state.json`** files track last fetch for incremental updates
- **`--resume`** flag to continue interrupted fetches

### Baidu Cloud backup

`compass-data backup` zips `parquet_data/` and uploads to Baidu Cloud via
`baidupcs` (BaiduPCS-Go):

- Target: `/compass/` folder
- Format: timestamped zip (`parquet_data-YYYYMMDD-HHMMSS.zip`)
- Standalone: `scripts/upload-parquet.sh [--keep-zip]`

### Dolt database queries

```sh
# investment_data (read-only, third-party)
dolt --data-dir=investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
dolt --data-dir=investment_data sql -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001' ORDER BY tradedate DESC LIMIT 5"
dolt --data-dir=investment_data sql -q "SELECT * FROM ts_a_stock_list LIMIT 5"
```

### compass_data (custom mutable database)

`compass_data` is our own Dolt repository for custom data — company profiles,
financial indicators, watchlists, etc. It lives alongside `investment_data`.

```sh
# Run `dolt sql` from the parent directory to enable cross-database queries
cd /path/to/compass
dolt sql -q "SELECT * FROM compass_data.stock_basic LIMIT 5"
dolt sql -q "SELECT * FROM compass_data.fin_indicators WHERE symbol='SH600519' ORDER BY report_date DESC"

# Cross-database JOINs
dolt sql -q "
SELECT sb.name, sb.industry_l1, ts.list_date
FROM compass_data.stock_basic sb
JOIN investment_data.ts_a_stock_list ts ON sb.ts_code = ts.ts_code
"

dolt sql -q "
SELECT sb.name, fi.report_date, fi.revenue / 1e8 AS rev_yi, fi.eps
FROM compass_data.stock_basic sb
JOIN compass_data.fin_indicators fi ON sb.symbol = fi.symbol
JOIN investment_data.final_a_stock_eod_price e ON sb.symbol = e.symbol
WHERE sb.symbol = 'SH600519'
ORDER BY e.tradedate DESC
LIMIT 3
"
```

Key tables:
| Table | Purpose | Key |
|---|---|---|
| `stock_basic` | Company profiles | `symbol` (`SZ000001`) + `ts_code` (`000001.SZ`) |
| `fin_indicators` | Financial indicators per report period | `(symbol, report_date)` |
| `fin_balance_sheet` | Balance sheet (资产负债表) | `(symbol, report_date)` |
| `fin_income` | Income statement (利润表) | `(symbol, report_date)` |
| `fin_cash_flow` | Cash flow statement (现金流量表) | `(symbol, report_date)` |

```sh
# Query financial statements
dolt sql -q "
SELECT * FROM compass_data.fin_balance_sheet
WHERE symbol='SH600519' ORDER BY report_date DESC LIMIT 3"

dolt sql -q "
SELECT * FROM compass_data.fin_income
WHERE symbol='SH600519' ORDER BY report_date DESC LIMIT 3"

dolt sql -q "
SELECT * FROM compass_data.fin_cash_flow
WHERE symbol='SH600519' ORDER BY report_date DESC LIMIT 3"

# Cross-table financial analysis
dolt sql -q "
SELECT sb.name, bs.report_date,
  bs.TOTAL_ASSETS / 1e8 AS total_assets_yi,
  inc.TOTAL_OPERATE_INCOME / 1e8 AS revenue_yi,
  cf.NETCASH_OPERATE / 1e8 AS operating_cf_yi
FROM compass_data.stock_basic sb
JOIN compass_data.fin_balance_sheet bs ON sb.symbol = bs.symbol
JOIN compass_data.fin_income inc ON bs.symbol = inc.symbol AND bs.report_date = inc.report_date
JOIN compass_data.fin_cash_flow cf ON bs.symbol = cf.symbol AND bs.report_date = cf.report_date
WHERE sb.symbol = 'SH600519'
ORDER BY bs.report_date DESC
LIMIT 3"
```

### Reset everything

```sh
rm -rf /data/compass-data/parquet_data/   # main Parquet data
rm logs/compass.log                        # logs
```
