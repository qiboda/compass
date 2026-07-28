# AGENTS.md — compass

A-share stock chart desktop application built with egui. Data pipeline uses
local Dolt `investment_data` as the **primary data source** (18M+ rows, 6000+ stocks),
with EastMoney (online) as a fallback. Parquet-based storage with DuckDB for querying.

**项目书** = 本项目所有规则与知识文件的统称，包括 `AGENTS.md` 和 `kb/` 目录下所有文件。

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
| **1. Issue** | Verify `gh issue view <N>` exists, or create one | Issue URL shown to user |
| **2. Plan** | If 2+ modules involved: run `/ulw-plan` agent until approval | `.omo/plans/*.md` file created + user approved |
| **3. Tests** | Write failing test(s) FIRST, confirm they fail | Test output showing failure |
| **4. Docs** | Identify which `kb/` files need updating | List of files to user |

**If ANY step is incomplete, STOP. Do NOT implement. Do NOT create todos. Do NOT edit files.**

### SELF-CHECK (MANDATORY — ask yourself these 4 questions before every code edit)

1. **"Is there a GitHub issue for this work?"** — If not, create one NOW.
2. **"Does my commit message include `ref #N`?"** — If not, add it before committing.
3. **"Have I written a failing test first?"** — If not, write one NOW before the implementation.
4. **"Have I updated the relevant kb/ file?"** — If not, identify the file and update it.

These 4 questions are NOT optional. They are the minimum standard. If you skip any,
you are violating the workflow.

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
| `worktree` | `/worktree` | Manage git worktrees for PR development |
| `open-worktrees` | `//open-worktrees` | Launch all worktree zones in separate kitty windows |
| `qa` (test) | `/test` | Write unit/integration tests (TDD/BDD), test coverage |
| `rustdoc` | `/rustdoc` | Verify `#[deny(missing_docs)]` compliance |
| `docs` | `/docs` | Identify and update `kb/` files based on code changes |
| `reflect` | `/reflect` | Write post-implementation reflections with trend analysis |

All skills are located under `.opencode/skills/<name>/SKILL.md`. OpenCode
auto-discovers skills from the filesystem — no registration needed.

### Issue-Driven Commits

**Every commit must reference a GitHub issue.** No exceptions — not even for
chores, docs, or scripts. The pre-push hook rejects commits without `ref #N`.

```
feat: add thing

ref #26
```

### Commit → Review (MANDATORY)

After every commit, always run review. No exceptions.

1. **Commit**: stage changes, write a descriptive message with `ref #N`, commit.
2. **Review**: run review on the committed changes.
3. **Fix**: if review finds issues, fix them and recommit.
4. **Repeat**: review again after the fix commit. Max 2 rounds; remaining
   issues → create GitHub issues and note in commit message.

### Commit & Push

Commit and push are **separate operations**. Do not chain them with `&&`.

1. **Commit first**: stage changes, write a descriptive message, commit.
2. **Verify locally**: ensure `cargo test`, `cargo clippy`, and `cargo fmt` pass.
3. **Push separately**: only after local verification is clean.

**HARD BLOCK: Never auto-push.** Wait for the user to explicitly say "push" / "推送".
Even if verification passes, do not push without user command. If you pushed without
the user's permission, you violated the workflow — revert and apologize.

Never `git push` in the same command as `git commit`. The pre-push hook runs
formatting, clippy, and doc checks — if they fail, the commit should remain
local until fixed, not be amended mid-push.

**Follow the user's exact words.** If the user says "commit" / "提交", only
commit — do not push. If the user says "push" / "推送", only push — do not
amend and re-commit. Never assume one implies the other.

### Issue Lifecycle

**HARD BLOCK: Close issues only AFTER push.** An issue is not "done" until the fix is on
`origin/master`. Do not close an issue after commit — wait for successful push.

- commit → issue stays OPEN
- push succeeds → close issue with `gh issue close`
- When closing, record the PR that implemented it: `gh issue comment <N> --body "Fixed by #<PR>"`

Every issue and PR must include labels at creation time. See `kb/github/labels.md`
for the Bevy-style A-/C-/D-/P-/S- taxonomy. Minimum: one A- and one C- label.

### Scope Discipline

**Never silently change a planned approach.** If an external constraint
(library bug, API incompatibility, missing crate) blocks the agreed-upon
implementation, do NOT work around it by altering the feature design.
Flag the issue to the user and ask for a decision.

The grill-me decisions and the approved plan define the contract. Any
deviation — even a pragmatic workaround — requires user approval first.

## Worktrees

For PR development, load the `worktree` skill. Worktrees live at
`.worktrees/<name>/` — each is a **transient PR workspace**, created for
a single PR and cleaned up after merge. Branch naming: `pr/<short-description>`.

After creating a worktree, the skill enforces MANDATORY post-creation steps:
1. `/handoff` → saves context to `.worktrees/<name>/.omo/handoff.md`
2. Tell user to open a new opencode session: `cd .worktrees/<name> && opencode`
3. Current session stays in master — do NOT cd into the worktree.

After PR merge, the skill enforces MANDATORY cleanup:
1. Remove worktree: `git worktree remove .worktrees/<name> --force`
2. Delete PR branch: `git branch -D pr/<name>`

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
| `kb/design/data-providers.md` | Trait system design, CachedProvider read-through pattern, EastMoney/DuckDB/Parquet providers, error handling |
| `kb/design/symbols.md` | A-share market segments, symbol convention rationale, exchange inference, secid mapping, timeframe handling |
| `kb/dev/testing.md` | rstest + tokio::test patterns, in-memory DuckDB, httpmock setup |
| `kb/dev/process.md` | Dev workflow, commands, config, debugging, reset |
| `kb/dev/reflections.md` | Post-implementation reflections — what went wrong, lessons learned |
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

See `kb/design/architecture.md` — threading model, data pipeline, CachedProvider, schema, source layout, libraries.

## Data providers

See `kb/design/data-providers.md` — EastMoney, DuckDB, Dolt, ParquetReader, DataError.

**Priority**: Dolt `investment_data` (local) is the **primary** data source.
EastMoney is a fallback for data not available locally.

### Dolt import (`crates/compass-data/src/import_dolt.rs`)

Reads from Dolt `investment_data` (`final_a_stock_eod_price` and `ts_a_stock_list`
tables) via `dolt sql -r parquet` (direct binary Parquet) and `dolt sql -r csv`
(for symbol enumeration), partitioned by Dolt symbol.

### EastMoneyProvider (`crates/compass-core/src/data/eastmoney.rs`)

Fetches K-line data from `push2his.eastmoney.com`. Symbol → secid conversion via `to_secid()`:

| Input | secid | Description |
|---|---|---|
| `000001` | `0.000001` | 平安银行 (SZ, heuristic default) |
| `600519` | `1.600519` | 贵州茅台 (SH, heuristic: 6xxxxx) |
| `688001` | `1.688001` | 华兴源创 (科创板) |
| `300750` | `0.300750` | 宁德时代 (创业板) |
| `sh.000001` | `1.000001` | 上证指数 (explicit SH prefix) |
| `sz.000001` | `0.000001` | 显式深圳 |
| `bj.8xxxxx` | `0.8xxxxx` | 北交所 |

## Parquet schema (main database)

```
parquet_data/
├── stock_basic.parquet        # symbol, name, exchange, list_date, delist_date
└── stock_daily/
    ├── SZ000001.parquet      # tradedate, open, high, low, close, adjclose, volume, amount
    ├── SH600519.parquet
    └── ...
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


