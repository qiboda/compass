# Evidence — feat: 自动回补缺失数据机制 (#308)

- Branch: `feat/auto-heal-missing-data`
- Worktree: `.worktrees/feat-auto-heal-missing-data`
- Issue: #308 (A-Data/C-Feature/D-Complex/P-High)
- Base: `0306b8a` (master after PR #307)

## Implementation commits

| Commit | Type | Summary |
|---|---|---|
| `e143814` | feat | Core implementation: Python collectors/backfill, Rust sepa backfill/check, script rename, tests/docs |
| `148e095` | fix | Review round 1: backfill imports to Dolt, strict symbol resolution, dragon range, production autostrict shim, print cap |
| `2141afa` | fix | Review round 2: `dolt_sql_csv_strict`, empty-source backfill no-op, tests |
| `fea4f6e` | fix | Real-data validation: per-table auto-heal range + deterministic test gating |

## Verification

### Automated

- Python collectors full suite: **962 passed, 1 warning** (`uv run pytest -q`)
- Rust `compass-core` + `compass-data` full tests: **pass**
  - includes new `trade_dates`, `check_stock_daily_gaps`, `sepa backfill-dates`, `sepa temperature --date` coverage
- Shell: `scripts/tests/test-update-database.sh` → **ALL TESTS PASSED**
- Shell adversarial: `test-update-database-adversarial.sh` → **ALL TESTS PASSED**
- `ruff check .` → clean
- `cargo clippy --workspace -- -D warnings` (CI-style) → clean
- `cargo fmt --check` → clean

### Real-data read-only smoke

- `cargo run --bin compass-data -- check-stock-daily` → exit 0 (no stock_daily calendar gap in real data)
- Per-table gap detection on real local Dolt:
  - `capital_main_flow`: range `2026-07-31..2026-08-28`, **13 missing** (bounded; no full-history flood)
  - `index_daily`: **1 missing** (`2026-08-28`)
  - `dragon_list`: **1 missing** (`2026-08-28`)
  - `block_trade`: **1 missing** (`2026-08-28`)
- Worktree local `investment_data` symlink was absent; created `investment_data -> /data/compass-data/investment_data` (gitignored, not part of commit) to run the read-only smoke in this worktree.

### F4 status

Full `scripts/update-database.sh` end-to-end smoke (network fetch + Dolt write/commit) was **not run** in this session; the read-only detection and gap-check paths were validated. Full script remains the final manual validation before/among user push approval.

## Scope notes

- No DuckDB export added (locked constraint preserved).
- `scripts/sepa_daily.sh` fully renamed to `scripts/update-database.sh`; no old-name compatibility entry.
- Per-table scanning uses each table's own earliest trade date, preventing a long-history table (index_daily from 1990) from triggering full-history backfill on shorter-history tables.
- Legacy `do_sync` tests are deterministic via `COMPASS_AUTO_HEAL=0` in conftest; the dedicated auto-heal suites keep auto-heal enabled.
