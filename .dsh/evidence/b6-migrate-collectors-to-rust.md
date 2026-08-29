# B6 — Orchestration CLI migration evidence

- Epic: #310
- Sub-issue: #325
- Branch: `feat/migrate-collectors-to-rust-b6`
- Date: 2026-08-29

## Scope

Ported `collectors/main.py` orchestration to Rust:

- `crates/compass-collectors/src/orchestrate.rs` — `fetch`, `import_target`,
  `sync`, `progress`, `backfill`, `auto_heal`, `sync_investment`.
- `crates/compass-collectors/src/stock_basic_official.rs` — added
  `import_to_dolt` (replace-by-rename + name-en mapping join, mirrors
  `main.py::_import_stock_basic`).
- `crates/compass-collectors/src/main.rs` — unified CLI subcommands:
  `fetch`, `import`, `sync`, `sync-investment`, `progress`, `backfill`,
  in addition to the existing flat collector commands.
- Docs: `.dsh/kb/design/architecture.md`, `.dsh/kb/user/cli.md`.

## Verification

- `cargo test -p compass-collectors`: **56 passed** (includes 3 new
  orchestrate unit tests).
- `cargo clippy -p compass-collectors -- -D warnings`: clean.
- `cargo fmt --check`: clean (pre-commit/pre-push also enforce).
- CLI smoke:
  - `compass-collectors progress` reads real `*.progress.json` files and
    prints human-readable statuses (block_trade/dragon/index_daily/
    institution_survey/main_flow seen).
  - `compass-collectors import stock_basic --foo` -> exit 1 with
    `unknown import argument`.
  - `compass-collectors sync --foo` -> exit 1 with `unknown sync argument`.
  - `compass-collectors fetch balance_sheet --years abc` -> exit 1 with
    `invalid year in --years: abc`.

## Review

Two review rounds:

1. First `subagent_review` found:
   - P1: `stock_basic_official` error path could drop the only backup after a
     failed restore (fixed).
   - P2/P3: CLI year parsing, trailing-arg acceptance, sync-investment
     nohup/timeout, count-parse fallbacks, usage. Most fixed.
2. Second `subagent_review` found:
   - P1: timeout on `run_dolt_investment` did not kill the spawned dolt child
     (`kill_on_drop` not set) (fixed).
   - P2: remaining old flat CLI parsers still silently filter invalid years;
     accepted as pre-existing batch scope.

Commits on B6 branch:
- `56b0e5a` feat: unified orchestration CLI
- `0ca27d5` fix: harden B6 orchestration review findings
- `5cd7b5f` fix: kill dolt child on sync-investment timeout

## Known gaps (carried forward, same as prior batches)

- Independent RED/adversarial/requirement subagent tests (gate 3.5/4) still
  not delegated.
- No Dolt-level dual-run/round-trip for B6; CLI smoke only.
- Full `sync` against production Dolt/network not run during this batch
  (would write production data; left for B7 stabilization).
- Python `collectors/` and `scripts/update-database.sh` remain unchanged;
  B7 will switch and retire them.
