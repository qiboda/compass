# Real-data smoke — db-sync-timing-stats (issue #334)

Date: 2026-08-29
Worktree: `.worktrees/db-sync-timing-stats`

## Command

```sh
COMPASS_AUTO_HEAL=0 \
COMPASS_TIMING_FILE=/tmp/compass-sync-smoke.jsonl \
SYNC_TIMING_DIR=/tmp/compass-sync-smoke-timings \
cargo run --bin compass-collectors -- sync
```

## Result

- Rust `compass-collectors sync` ran against real Dolt/network data and wrote
  12 structured `collector` JSONL events to `COMPASS_TIMING_FILE`:
  - `stock_basic` fetch/import
  - `fin_indicators` fetch/import
  - `balance_sheet` fetch/import
  - `income` fetch/import
  - `cash_flow` fetch/import
  - `dragon` fetch/import
- Sync stopped at `dragon_list` import because the source returned 0 records
  (2026-08-29 is not a trading day / no dragon-list data). This is the existing
  data-pipeline nonzero-rows guard, **not** a timing failure: timing events
  continued to be emitted and no timing warning blocked the pipeline.
- Partial collector imports wrote to `compass_data` and were committed/pushed:
  Dolt commit `ennng72ctrkmenrq2f7vp9scq7knuoq3` (`feat: sync timing smoke data update ref #334`).
- Investment_data remained clean; no timing JSON was written to the repo.
- Shell final-JSON merge is not exercised by `compass-collectors sync` alone; it
  is covered by the mock shell test suites:
  - `scripts/tests/test-timing-requirements.sh`
  - `scripts/tests/test-timing-adversarial.sh`
