# AGENTS.md — compass

A-share stock chart desktop application built with egui. Data pipeline uses
local Dolt `investment_data` as the **primary data source** (18M+ rows, 6000+ stocks),
with EastMoney (online) as a fallback. Parquet-based storage with DuckDB for querying.

---

## ⚡ GRILL-ME FIRST (ALWAYS)

**On EVERY user message in this repo, you MUST load `/grill-me` before responding.**
This is NON-NEGOTIABLE. No exceptions.

The grill-me interview must complete with "shared understanding reached" before
you proceed to any other action — including reading files, classifying the
request, creating todos, or writing code.

---

## 🛑 PRE-IMPLEMENTATION GATE (READ BEFORE ANY CODE CHANGE)

**For ALL feature and bugfix work, you MUST complete this gate BEFORE writing any code.**

Before you touch a single file, verbalize EACH step to the user and confirm completion:

| Step | Action | Evidence Required |
|---|---|---|
| **1. Issue** | Verify `gh issue view <N>` exists, or create one | Issue URL shown to user |
| **2. Plan** | If 2+ modules involved: run `/ulw-plan` agent until approval | `.omo/plans/*.md` file created + user approved |
| **3. Tests** | Write failing test(s) FIRST, confirm they fail | Test output showing failure |
| **4. Docs** | Identify which `kb/` files need updating | List of files to user |

**If ANY step is incomplete, STOP. Do NOT implement. Do NOT create todos. Do NOT edit files.**

### HARD BLOCK

This gate is NON-NEGOTIABLE. The `compass-workflow` skill, when loaded, will
remind you of this gate. If you find yourself writing code without completing
these steps, you are violating the workflow — stop immediately, `git stash` or revert, and go back to step 0.

Exceptions (skip the gate): documentation-only changes, lint fixes, typo fixes.

### After implementation: Reflection Record

After EVERY feature/bugfix, append a brief reflection to `kb/dev/reflections.md`:

```markdown
## [date] — ref #[N] [title]

**What was done**: [1-2 sentences]
**What went wrong**: [process failures, if any]
**Lessons learned**: [what to do differently]
```

This is MANDATORY — commit it with the implementation or immediately after.

---

## Workflow (MANDATORY)

For all **feature** and **bugfix** work, the `compass-workflow` skill MUST be loaded.
This enforces: issue-driven development, doc-sync, test-first, per-step-verify,
and commit discipline.

**After loading the skill**: immediately run through the PRE-IMPLEMENTATION GATE
checklist above. Do not skip any step.

### Commit & Push

Commit and push are **separate operations**. Do not chain them with `&&`.

1. **Commit first**: stage changes, write a descriptive message, commit.
2. **Verify locally**: ensure `cargo test`, `cargo clippy`, and `cargo fmt` pass.
3. **Push separately**: only after local verification is clean.

Never `git push` in the same command as `git commit`. The pre-push hook runs
formatting, clippy, and doc checks — if they fail, the commit should remain
local until fixed, not be amended mid-push.

**Follow the user's exact words.** If the user says "commit" / "提交", only
commit — do not push. If the user says "push" / "推送", only push — do not
amend and re-commit. Never assume one implies the other.

### Scope Discipline

**Never silently change a planned approach.** If an external constraint
(library bug, API incompatibility, missing crate) blocks the agreed-upon
implementation, do NOT work around it by altering the feature design.
Flag the issue to the user and ask for a decision.

The grill-me decisions and the approved plan define the contract. Any
deviation — even a pragmatic workaround — requires user approval first.

## Worktrees

For isolated development (experiments, library migrations, multi-day features),
load the `worktree` skill. Worktrees live at `.worktrees/<name>/` and map to
`feature/<name>` branches. See `kb/dev/process.md#worktrees` for policy.

## Knowledge base

Detailed docs under `kb/` — organized into three sections:

| Section | Purpose |
|---|---|
| `kb/design/` | Project design — architecture, data providers, symbols |
| `kb/dev/` | Development aids — workflow, process, testing, reflections |
| `kb/user/` | User reference — installation, GUI, CLI, config |

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
| `kb/user/cli.md` | Data pipeline — download, import, merge, export, workflows, troubleshooting |
| `kb/user/config.md` | Config reference — all options, defaults, examples |

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

# Download from EastMoney (fallback) into staging DuckDB
cargo run --bin compass-data -- download --symbols 600519
cargo run --bin compass-data -- download --symbols all --overwrite  # force overwrite

# Merge staging DuckDB into Parquet main database
cargo run --bin compass-data -- merge
cargo run --bin compass-data -- merge --overwrite         # staging wins on conflict

# Export Parquet to DuckDB
cargo run --bin compass-data -- export
cargo run --bin compass-data -- export --overwrite        # force overwrite
```

All commands default to **merge/skip** behavior (migration-style):
existing unique keys are preserved, only new data is added. Pass `--overwrite`
to replace existing data. Applies to `import`, `download`, `merge`, `export`.

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
[database]
path = "data/compass.duckdb"

[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10

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


