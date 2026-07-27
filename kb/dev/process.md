# Development Process

## Issue-driven workflow

The complete development cycle for features and bugs:

```
User raises requirement
  →  User comments /ask or /fix or /impl on issue/PR
  →  OpenCode GitHub Action triggers, loads role-specific kb/github/*.md
  →  /ask: answers only. /fix: evaluates complexity, fixes or reports.
      /impl: implements with test-first + ref #N
  →  (If working locally) OpenCode grills (/grill-me) to clarify scope
  →  Shared understanding reached → summarize locked-in decisions
  →  OpenCode creates GitHub issue (feature_request or bug_report template)
  →  OpenCode shows issue with gh issue view <N>
  →  /ulw-plan (if multi-step)  →  implement
  →  cargo nextest (tests must pass)
  →  commit with ref #N
  →  ai-review (/review-work)
  →  push master
  →  CI passes  →  close issue with gh issue close N
```

Docs, lint fixes, and typos skip the grill-me + issue cycle — implement directly.

| Work type | Issue required? |
|---|---|
| Feature | ✅ Required |
| Bug fix | ✅ Required |
| Refactor | ✅ Required |
| Docs update | ❌ Skip |
| Lint / typo | ❌ Skip |

### When OpenCode discovers a new bug

1. Create issue using `.github/ISSUE_TEMPLATE/bug_report.md` template
2. Read it back (`gh issue view <N>`) to confirm it exists
3. Fix it — commit with `fixes #N`

### Commit → issue linking

| Commit type | Issue reference |
|---|---|
| feat / fix | `ref #N` |

Issues are closed **manually** via `gh issue close N` after verification.
Do NOT use `fixes #N` or `closes #N` — these auto-close the issue on push.

### Commit-msg hook

A git hook (`.githooks/commit-msg`) enforces issue references:

```
feat/fix commits → must include "ref #N"
test, refactor, docs, chore → no issue reference required
```

The hook is activated via `git config core.hooksPath .githooks` (already configured).

## OpenCode workflow

### GitHub Action commands

OpenCode runs as a GitHub Action bot. Use these commands in issue/PR comments:

| Command | Usage | Behavior |
|---|---|---|
| `/ask` | Issue or PR comment | Read-only Q&A — answers questions, explains code |
| `/fix` | Issue or PR comment | Bug fix: simple → fix+commit; complex → analyze+suggest PR |
| `/review` | PR comment | Code review — checks correctness, conventions, Rust best practices |
| `/impl` | Issue or PR comment | Feature implementation — follows full compass workflow |
| (auto) | CI workflow failure | Diagnoses CI failure, reports root cause |

Each command maps to a workflow file in `.github/workflows/opencode-*.yml` and a
role instruction file in `kb/github/*.md`. GitHub roles layer on top of AGENTS.md
(Common Baseline + Role Overlay) — AGENTS.md provides project conventions, role
files add role-specific constraints.

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
cargo doc --no-deps         # must be warning-free
```

All four must pass before `git push`. Never push broken code.
`cargo doc --no-deps` verifies that `#![warn(missing_docs)]` in
`compass-core` is clean — every public item must have a `///` doc comment.

### Quality review

After committing and before pushing, run `/review-work` — launches 5 parallel
agents (goal verification, code quality, security, QA execution, context mining).

Skippable for: docs, lint fixes, typos, trivial chores.

### Push rhythm

Push immediately after completing each issue. Do not batch.

### Commit discipline

- Each commit = one logical unit. Never mix bugfix + feature + refactor.
- Conventional commits: `feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`.
- Bugfix commits use `fixes #N`, feature commits use `closes #N`.
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

## Worktrees (functional zone isolation)

Worktrees divide the project into persistent functional zones. Each worktree
hosts multiple features in its domain — it is NOT deleted after a single
feature ships. The `/worktree` skill provides conventions and commands.

**Convention**: worktrees live at `.worktrees/<name>/` (gitignored),
one per functional area (e.g. `custom-dolt`, `egui-mobius`).

**Why not plugins**: The `opencode-worktree` plugin (kdco/worktree via OCX)
was evaluated and found to have blocking issues (no idempotent re-open,
unreliable terminal spawn, no session reopen). Manual worktrees +
the `/worktree` skill give full control without those issues.

**Post-creation**: after `git worktree add`, the worktree skill requires:
1. Symlink data dirs from `/data/compass-data/` if available
2. `/handoff` → writes `.worktrees/<name>/.omo/handoff.md` with current context
3. Tell user: `cd .worktrees/<name> && opencode` (new session reads handoff)
4. Stay in master — don't switch session into the worktree directory

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

```sh
# Import from Dolt investment_data into Parquet main database
cargo run --bin compass-data -- import
cargo run --bin compass-data -- import --limit 100
cargo run --bin compass-data -- import --symbols 000001,600519
cargo run --bin compass-data -- import --overwrite  # full replace (skip merge)

# Import custom data from compass_data Dolt
cargo run --bin compass-data -- import-compass --table stock_basic
cargo run --bin compass-data -- import-compass --table fin_indicators

# Export Parquet to DuckDB (default: /data/compass-data/compass.duckdb)
cargo run --bin compass-data -- export
cargo run --bin compass-data -- export --overwrite  # force overwrite

# Custom paths via CLI flags (override config.toml defaults)
cargo run --bin compass-data -- import --dolt-dir /custom/investment_data --output /custom/parquet

# Full help
cargo run --bin compass-data -- --help
cargo run --bin compass-data -- import --help
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

| Change type | kb/ file to update |
|---|---|
| New data source, API call, schema change | `kb/design/data-providers.md` |
| Threading, pipeline, library changes | `kb/design/architecture.md` |
| Symbol format, timeframe mapping | `kb/design/symbols.md` |
| Test framework, patterns | `kb/dev/testing.md` |
| Workflow, hooks, conventions | `kb/dev/process.md` |
| Project-level conventions | `AGENTS.md` |

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

## Running tests

```sh
cargo nextest run                       # recommended
cargo test                              # standard runner
cargo test duckdb                       # filter by name
cargo test --test integration_test      # integration tests only
```

## Checking code quality

```sh
cargo fmt --check           # verify formatting
cargo clippy -- -D warnings # strict lint
```

## Config

Create `~/.config/compass/config.toml` to override defaults:

```toml
[app]
default_symbol = "600519"
```

Missing keys fall back to defaults defined in `crates/compass-core/src/model.rs`.

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
ls -lh /data/compass-data/parquet_data/stock_daily/ | head -20
wc -l /data/compass-data/parquet_data/stock_daily/     # file count = symbol count
```

### Query Parquet with DuckDB

```rust
use duckdb::Connection;
let conn = Connection::open_in_memory()?;
conn.execute_batch("SELECT * FROM read_parquet('/data/compass-data/parquet_data/stock_daily/SH600519.parquet') LIMIT 5")?;
```

### collectors (Python data pipeline)

Fetch data from EastMoney APIs into CSV, then import into `compass_data` Dolt.

```sh
cd collectors/
uv sync                           # first time: install dependencies

# Unified CLI
uv run python main.py fetch stock_basic
uv run python main.py sync       # fetch + import all
uv run python main.py sync-investment --restart
```

Key concepts:
- **curl_cffi** for TLS impersonation (EastMoney anti-crawler)
- **CSV as intermediary** between API and Dolt
- **`.state.json`** files track last fetch for incremental updates
- **`--resume`** flag to continue interrupted fetches

### compass-data CLI (Rust)

```sh
# Dolt → Parquet
compass-data import-compass --table stock_basic
compass-data import-compass --table fin_indicators

# investment_data incremental import
compass-data import --since 20260725

# Backup to Baidu Cloud
compass-data backup
compass-data backup --keep-zip
```

### Baidu Cloud backup

Sync `parquet_data/` snapshot to Baidu Cloud via `baidupcs` (BaiduPCS-Go):

- Target: `/compass/` folder
- Format: timestamped zip (`parquet_data-YYYYMMDD-HHMMSS.zip`)
- Standalone: `scripts/upload-parquet.sh [--keep-zip]`

### Dolt database queries

```sh
# investment_data (read-only, primary data source)
dolt --data-dir=/data/compass-data/investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
dolt --data-dir=/data/compass-data/investment_data sql -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001' ORDER BY tradedate DESC LIMIT 5"
dolt --data-dir=/data/compass-data/investment_data sql -q "SELECT * FROM ts_a_stock_list LIMIT 5"
```

### compass_data (custom mutable database)

`compass_data` is our own Dolt repository for custom data — company profiles,
financial indicators, watchlists, etc. It lives at `/data/compass-data/compass_data`.

```sh
# Run `dolt sql` from the parent directory to enable cross-database queries
cd /data/compass-data
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

### Reset everything

```sh
rm /data/compass-data/compass.duckdb logs/compass.log  # GUI cache
rm -rf /data/compass-data/parquet_data/                 # main Parquet data (re-import to restore)
```
