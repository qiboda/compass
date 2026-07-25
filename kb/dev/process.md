# Development Process

## Issue-driven workflow

The complete development cycle for features and bugs:

```
User raises requirement
  →  OpenCode grills (/grill-me) to clarify scope and decisions
  →  Shared understanding reached → summarize locked-in decisions
  →  OpenCode creates GitHub issue (feature_request or bug_report template)
  →  OpenCode shows issue with gh issue view <N>
  →  git checkout -b feat/desc              # create feature branch
  →  /ulw-plan (if multi-step) → implement
  →  cargo nextest + clippy + fmt → commit with ref #N → push branch
  →  gh pr create --body "Closes #N"        # create PR
  →  CI passes → manual squash merge → issue auto-closes via Closes #N
  →  git checkout master && git pull && git branch -d feat/desc  # cleanup
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
3. Fix it — commit with `ref #N`

### Commit → issue linking

| Commit type | Issue reference |
|---|---|
| feat / fix | `ref #N` in commit body |

`ref #N` goes in the commit body (enforced by commit-msg hook).
`Closes #N` goes in the PR description body — GitHub auto-closes when the PR is merged.

Never put `fixes #N` or `closes #N` in a commit message — that would
auto-close the issue when the commit merges to master, bypassing PR review.

### Commit-msg hook

A git hook (`.githooks/commit-msg`) enforces issue references:

```
feat/fix commits → must include "ref #N"
test, refactor, docs, chore → no issue reference required
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

The pre-push hook (`.githooks/pre-push`) enforces these checks in order:

1. **cargo fmt --check**
2. **cargo clippy -- -D warnings**
3. **cargo doc --no-deps** (must be warning-free)
4. **Issue references**: `ref #N` must point to open issues

Never merge a PR with failing CI — check CI status at the PR page or
with `gh pr checks <branch>`

Manual pre-push checklist:
cargo doc --no-deps         # must be warning-free
```

All quality gates must pass before `git push`. Never push broken code.
`cargo doc --no-deps` verifies that `#![warn(missing_docs)]` in
`compass-core` is clean — every public item must have a `///` doc comment.

### PR rhythm

Create a PR immediately after completing each issue. Do not batch.
Keep PRs small and focused on a single issue.

### Commit discipline

- Each commit = one logical unit. Never mix bugfix + feature + refactor.
- Conventional commits: `feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`.
- All feat/fix commits must include `ref #N` in the commit body.
- Never use `fixes #N` or `closes #N` in commit messages — those go in PR body.
- Template: `git config commit.template .gitmessage` is already set.

## Git branching

**Feature-branch + PR.** Each feature/fix gets its own branch off `master`.

```
master  ──●────────●────────●──  (PR squash merge)
          \        /        /
  feat/xxx  ●──●──●        /
  fix/yyy         ●──●──●─
```

- Branch naming: `feat/<short-description>` for features, `fix/<short-description>` for fixes
- Push branch → create PR → CI passes → manual squash merge → branch deleted
- Never push directly to `master`

## Version control

```sh
git checkout -b feat/short-desc          # create feature branch
git add <files>                           # stage only intended changes
git commit                                 # uses .gitmessage template
git push -u origin feat/short-desc        # push branch (not master), set upstream
gh pr create --base master --title "..." --body "Closes #N"  # create PR
# Wait for CI to pass, then manually squash-merge via GitHub UI
git checkout master && git pull            # sync local master
git branch -d feat/short-desc             # delete local branch
git push origin --delete feat/short-desc  # clean up remote branch
```

## Quickstart

```sh
cargo run                       # launch the GUI app (needs X11/Wayland)
RUST_LOG=debug cargo run        # verbose logging
```

### CLI (compass-data)

```sh
# Download from EastMoney to staging DuckDB
cargo run --bin compass-data -- download --symbols "000001,600519"
cargo run --bin compass-data -- download --symbols all --concurrency 2 --delay-ms 2000
cargo run --bin compass-data -- download --symbols all --overwrite  # force overwrite

# Import from Dolt into Parquet main database
cargo run --bin compass-data -- import
cargo run --bin compass-data -- import --limit 100
cargo run --bin compass-data -- import --overwrite  # full replace (skip merge)

# Merge staging DuckDB into Parquet
cargo run --bin compass-data -- merge
cargo run --bin compass-data -- merge --overwrite   # staging wins on conflict

# Export Parquet to DuckDB
cargo run --bin compass-data -- export
cargo run --bin compass-data -- export --overwrite  # force overwrite

# Full help
cargo run --bin compass-data -- --help
cargo run --bin compass-data -- download --help
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

### Inspect the staging DuckDB

```sh
# DuckDB CLI not installed by default. Use the export command instead:
cargo run --bin compass-data -- export

# Or query via Python duckdb package if available
```

### Inspect Parquet files

```sh
ls -lh parquet_data/stock_daily/ | head -20
wc -l parquet_data/stock_daily/     # file count = symbol count
```

### Query Parquet with DuckDB

```rust
use duckdb::Connection;
let conn = Connection::open_in_memory()?;
conn.execute_batch("SELECT * FROM read_parquet('parquet_data/stock_daily/SH600519.parquet') LIMIT 5")?;
```

### Dolt database queries

```sh
dolt --data-dir=investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
dolt --data-dir=investment_data sql -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001' ORDER BY tradedate DESC LIMIT 5"
dolt --data-dir=investment_data sql -q "SELECT * FROM ts_a_stock_list LIMIT 5"
```

### Reset everything

```sh
rm data/compass.duckdb logs/compass.log         # GUI cache
rm -rf data/                                 # staging cache
rm -rf parquet_data/                        # main Parquet data
```
