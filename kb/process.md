# Development Process

## Issue-driven workflow

The complete development cycle for features and bugs:

```
User raises requirement
  →  OpenCode grills (/grill-me) to clarify scope and decisions
  →  Shared understanding reached → summarize locked-in decisions
  →  OpenCode creates GitHub issue (feature_request or bug_report template)
  →  OpenCode shows issue with gh issue view <N>
  →  /ulw-plan (if multi-step)  →  implement
  →  cargo nextest + clippy + fmt  →  commit with ref #N  →  push master
  →  CI passes  →  manually close issue with gh issue close N
```

Refactors, docs, lint fixes, and typos skip the grill-me + issue cycle — implement directly.

| Work type | Issue required? |
|---|---|
| Feature | ✅ Required |
| Bug fix | ✅ Required |
| Refactor | ❌ Skip |
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

### Before pushing

```sh
cargo nextest run
cargo clippy -- -D warnings
cargo fmt --check
```

All three must pass before `git push`. Never push broken code.

### Push rhythm

Push immediately after completing each issue. Do not batch.

### Commit discipline

- Each commit = one logical unit. Never mix bugfix + feature + refactor.
- Conventional commits: `feat:`, `fix:`, `test:`, `refactor:`, `docs:`, `chore:`.
- Bugfix commits use `fixes #N`, feature commits use `closes #N`.
- Template: `git config commit.template .gitmessage` is already set.

## Git branching

**Trunk-based development.** Push directly to `main`.

```
main  ●──●──●──●──●  (trunk)
```

Solo project — no feature branches, no PRs. CI runs on push to `main`.
If the project grows to multiple contributors, switch to feature branches + PRs.

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

### CLI downloader

```sh
# Download all A-share stocks
cargo run --bin compass-downloader -- --symbols all

# Download specific symbols
cargo run --bin compass-downloader -- --symbols "000001,600519"

# Custom database path, start date, concurrency
cargo run --bin compass-downloader -- \
    --symbols all \
    --db /path/to/compass.duckdb \
    --start-date 19900101 \
    --concurrency 5 \
    --delay-ms 200

# Parquet export after download
cargo run --bin compass-downloader -- --symbols all --export-parquet /tmp/exports/

# Full help
cargo run --bin compass-downloader -- --help
```

## Adding a feature (manual)

If working without OpenCode:

1. **Explore** the relevant source files (`kb/architecture.md` for layout).
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
| New data source, API call, schema change | `data-providers.md` |
| Threading, pipeline, library changes | `architecture.md` |
| Symbol format, timeframe mapping | `symbols.md` |
| Test framework, patterns | `testing.md` |
| Workflow, hooks, conventions | `process.md` |
| Project-level conventions | `AGENTS.md` |

## TDD workflow

Feature and bugfix work follows TDD (Test-Driven Development):

```
RED → GREEN → REFACTOR
```

1. **RED**: Write a failing test that documents the expected behavior.
   - Test must fail before any implementation code exists.
   - If it passes immediately, delete or rewrite — it's testing nothing.
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

Missing keys fall back to defaults defined in `src/model.rs`.

## Logs

- Stderr: always. `RUST_LOG` controls level (`error`, `warn`, `info`, `debug`, `trace`).
- File: `logs/compass.log` (daily rolling).

## Debugging tips

### Check what the API returns

```sh
curl "https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=0.000001&klt=101&fqt=1&beg=20250101&end=20250721&lmt=10&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61"
```

### Inspect the DuckDB database

```sh
# Install duckdb CLI: https://duckdb.org/docs/installation/
duckdb compass.db

# Inside duckdb shell:
.tables
SELECT ts_code, COUNT(*) FROM stock_daily GROUP BY ts_code;
SELECT * FROM stock_daily WHERE ts_code='000001.SZ' ORDER BY trade_date DESC LIMIT 5;
SELECT * FROM stock_basic;
SELECT * FROM stock_adj_factor WHERE ts_code='000001.SZ' ORDER BY trade_date DESC LIMIT 5;

# Or from command line:
duckdb compass.db -c "SELECT ts_code, COUNT(*) FROM stock_daily GROUP BY ts_code;"
duckdb compass.db -c "SELECT * FROM stock_basic;"
```

### Baostock setup

Baostock requires Python 3 and the `baostock` package:

```sh
pip install baostock
```

The CLI downloader invokes Baostock via `python3 scripts/fetch_adj_factor.py`.
If `python3` is not on PATH, use a symlink or set up a wrapper.

### Reset everything

```sh
rm compass.db logs/compass.log
```
