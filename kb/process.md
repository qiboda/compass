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
  →  cargo nextest + clippy + fmt  →  commit with fixes/closes #N  →  push main
  →  CI passes  →  GitHub auto-closes issue
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

| Issue type | Commit trailer |
|---|---|
| Bug fix | `fixes #N` |
| Feature | `closes #N` |

GitHub auto-closes the issue when the commit reaches `main`.

### Commit-msg hook

A git hook (`.githooks/commit-msg`) enforces issue references:

```
feat: commits → must include "closes #N"
fix:  commits → must include "fixes #N"
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
cargo run                       # launch the app (needs X11/Wayland)
RUST_LOG=debug cargo run        # verbose logging
```

## Adding a feature (manual)

If working without OpenCode:

1. **Explore** the relevant source files (`kb/architecture.md` for layout).
2. **Test first**: Write a failing test in `#[cfg(test)] mod tests`.
3. **Implement** in the source file.
4. **Verify**: `cargo nextest run` + `lsp_diagnostics`.
5. **Update docs** if the change affects architecture, symbol format, or config.

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
cargo test sqlite                       # filter by name
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

### Inspect the SQLite cache

```sh
sqlite3 compass.db "SELECT symbol, timeframe, count(*) FROM bars GROUP BY 1, 2;"
sqlite3 compass.db "SELECT * FROM bars WHERE symbol='000001' AND timeframe='1d' ORDER BY timestamp DESC LIMIT 5;"
```

### Reset everything

```sh
rm compass.db logs/compass.log
```
