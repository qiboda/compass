# B7 Evidence — 切换 update-database.sh 到 Rust + 退役 Python collectors (epic #310 / sub-issue #326)

- Date: 2026-08-29
- Worktree: /data/codes/compass/.worktrees/migrate-collectors-to-rust
- Branch: feat/migrate-collectors-to-rust-b7 (stacked on origin/feat/migrate-collectors-to-rust-b6)
- Base: 9d038b9 (B6 head)
- B7 commits:
  - b790dd9 refactor(collectors): switch update-database.sh and retire Python collectors
  - add9c91 docs(collectors): update KB for Rust collector retirement
  - ac8d117 fix(collectors): address B7 review findings
  - fc4e327 fix(collectors): add missing SQL separator in fin_indicators upsert
  - 426a6fd docs(collectors): add B7 evidence, plan progress and reflection
  - d7a2b09 docs(collectors): record B7 PR #333 in plan
- User decisions confirmed during B7:
  1. Accept JSON-only freeproxy/keepalive; document `--source realtime` as a known Python-only feature not ported (sub-issue #324 scope deviation recorded).
  2. Full live `update-database.sh` smoke was requested, then after discovering a 1990+ `sepa backfill-dates` historical compute backlog the user chose **bounded backfill**: stop the 10h+ full run and validate with `sepa backfill-dates --start 2026-07-31` + temperature + score.

## Implementation

- `scripts/update-database.sh` step 2 now runs:
  `cargo run --bin compass-collectors -- sync` from `$PROJECT_ROOT` (was `uv run python main.py sync`).
- Removed `collectors/` entirely: 72 tracked Python files, `pyproject.toml`, `uv.lock`, `Makefile`, `tests/`.
- Removed migration-era `crates/compass-collectors/scripts/dual_run_*.sh` (8 scripts) — they compared Rust vs Python and are obsolete after Python retirement.
- Moved `collectors/name_en_mapping.csv` -> `crates/compass-collectors/data/name_en_mapping.csv` (100% rename) and fixed `config.rs::name_en_mapping_path()` to resolve `CARGO_MANIFEST_DIR/data/name_en_mapping.csv`.
- CI: removed `python-lint` and `python-test` jobs; pre-commit/pre-push hooks no longer run Python lint/tests; `.gitignore` cleaned.
- GitHub branch protection: removed `Python Lint` / `Python Test` from required status checks (now only `Rust ...` and `Bench (compile)`).
- Docs updated: architecture.md (MIG-1..MIG-5), data-providers.md, database.md, process.md, testing.md, user/cli.md, user/gui.md, user/index.md, design/gui-i18n.md, design/symbols.md, AGENTS.md.
- Real-time proxy source: Rust `freeproxy --source realtime` remains unsupported; `keepalive` cycle treats realtime as skip. Documented as accepted deviation.

## Verification

### Unit / local gates
- `cargo test -p compass-collectors`: 57 passed.
- `cargo clippy -p compass-collectors -- -D warnings`: clean.
- `cargo fmt --check`: clean (pre-commit hook).
- `bash scripts/tests/test-update-database.sh`: ALL TESTS PASSED (17 sections, mock cargo/dolt, Rust sync failure path through `FAKE_CARGO_FAIL_CALL=3`).
- Full workspace `cargo test`: passed before the final fix commit; crate-level suite rerun after review fixes. (Workspace coverage gate not re-run in this batch; see F3 caveats.)

### Real live smoke — attempt 1 (auto-heal ON)
- Ran `scripts/update-database.sh` with default `COMPASS_AUTO_HEAL`.
- Step 0/1 passed, then `compass-collectors sync` started auto-heal and failed at `main_flow` fflow backfill:
  `https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get... client error (SendRequest)`.
- Independent `curl` to the same push2his endpoint also fails with
  `OpenSSL SSL_read: unexpected eof while reading` (all tested secids). This is an external EastMoney push2his connectivity issue, not a Rust regression; Python would hit the same endpoint.
- No compass_data writes were made by the failed attempt.

### Real live smoke — attempt 2 (COMPASS_AUTO_HEAL=0)
- Ran `COMPASS_AUTO_HEAL=0 bash scripts/update-database.sh` to bypass the external push2his outage and validate the Rust collector/import path.
- Passed through:
  - Step 0: investment_data sync (skwy/master == origin/master).
  - Step 1: `compass-data import` exported `stock_daily.parquet` (6138 symbols, 18,368,033 rows).
  - Step 1b: `check-stock-daily` passed.
  - Step 2: `compass-collectors sync` completed:
    - stock_basic 5905 rows
    - fin_indicators fetched+imported
    - balance_sheet 7603 records
    - income 7759 records
    - cash_flow 7583 records
    - dragon_list 99 records (2026-08-28)
    - institution_survey 4054 + 42 records
    - main_flow 5554 items
    - index_daily/index_basic 120 daily + 120 basic rows (partial due push2his outage; THS 90 industries still fetched)
    - data_updates updated
  - Step 3: Dolt collector commit + push (`feat: sepa collectors data ref #139`).
  - Step 4: import-compass 11 tables all ran.
- Step 4b `sepa backfill-dates` then started recomputing every missing date from 1990-12-19 because production `final_score` only covered 2026-07-31+ (existing compute-table gap). Estimated 10+ hours, so it was stopped per user decision.

### Bounded live validation (user-chosen)
- Ran:
  - `cargo run --bin compass-data -- sepa backfill-dates --start 2026-07-31`
  - `cargo run --bin compass-data -- sepa temperature`
  - `cargo run --bin compass-data -- sepa score --top 50`
- Completed successfully; the bounded backfill wrote SEPA compute rows for the available window (e.g. 2026-08-04/05/11/17/18/25/28 etc).
- Dolt commit + push applied for changed compute tables:
  `capital_factor`, `data_updates`, `final_score`, `industry_factor`, `market_temperature`, `technical_factor`
  (commit `fb0tuhnlo2g2aq4fb2cgqbiadbpv196d`, `feat: sepa scores ref #139`).
- `dolt status` clean after push.

## F-wave status

- F1 (commit refs): all B7 commits contain standalone `ref #326`; verified via `git log --format=%B`.
- F2 (review): five parallel reviews (context/goal/quality/security/QA) were run after the initial B7 commits; P0/P1 none; P2 review findings addressed (pre-push comment, realtime wording, gui-i18n path, config test isolation, MIG-5 wording, data-providers note). QA report also flagged no B7 evidence/plan at review time — this file plus plan update closes that.
- F3 (tests/coverage): Rust tests + shell tests pass. Coverage job not re-run locally in this batch; CI will run on the PR. Python coverage gate retired.
- F4 (scope fidelity): B7 did the switch/retire, removed Python collectors, moved mapping data, cleaned CI/hooks/branch protection, updated docs/decision records. Documented unavoidable deviations:
  - `freeproxy --source realtime` not ported (user accepted).
  - Full `update-database.sh` smoke not run to completion due (a) external push2his outage and (b) pre-existing 1990+ SEPA compute backlog; user accepted bounded backfill.
  - Migration-era dual_run scripts removed with Python.

## Known open items after B7

- `push2his.eastmoney.com` connectivity from this environment is failing (TLS unexpected EOF); affects auto-heal `main_flow`/`index_daily` official kline full runs until network recovers. Other EastMoney hosts (datacenter-web/push2) work.
- Independent RED/adversarial/requirement subagent tests for B1-B6 interfaces (plan gate 3.5/4) remain a known deferred item; no new independent test delegation was done in B7.
- SEPA compute tables still have a historical 1990-2026 missing-date backlog; bounded backfill only covered the recent window.
