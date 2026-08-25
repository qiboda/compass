# Ref #298 — import-compass merge-key/fix/bug smoke

Date: 2026-08-25
Branch: `fix/import-compass-merge-key-mismatch`
Commit: `33488a7` (review-fix commit on top of `bbc0425`; current HEAD)

## Purpose

Verify the #298 code fix on real production data:
- `block_trade` incremental merge must not lose rows.
- Fallback behavior must not overwrite parquet with `--since`-filtered data.

## Commands

```sh
# Before (baseline)
dolt --data-dir /data/compass-data/compass_data sql -q "SELECT COUNT(*) AS c FROM block_trade" -r csv
python3 - <<'PY'
import duckdb
print(duckdb.sql("SELECT COUNT(*) FROM read_parquet('/data/compass-data/parquet_data/block_trade.parquet')").fetchone()[0])
PY
# Both printed 19724.

# After fix (real incremental merge with --since)
cargo run --bin compass-data -- import-compass --table block_trade --since 2026-08-21
```

## Results

- Command returned exit code 0. Log tail:
  - `INFO compass_data::import_compass: Exporting block_trade...`
  - `INFO compass_data::import_compass: Merging incremental data with existing parquet...`
  - `INFO compass_data::import_compass:   → /data/compass-data/parquet_data/block_trade.parquet`
- Post-check:
  - Dolt count: `19724`
  - Parquet count: `19724`
- No `row count mismatch` error.
- Both before/after parquet counts equal the full Dolt count, so the incremental
  merge preserved all rows.

## Related automated verification

- `cargo test -p compass-data --lib`: 104 passed, 0 failed.
- New tests include:
  - `block_trade_merge_preserves_distinct_rows_with_same_narrow_partition_key`
  - `block_trade_incremental_does_not_silently_replace_distinct_full_pk_row`
  - `block_trade_merge_prefer_new_updates_same_full_pk_without_losing_siblings`
  - `block_trade_requirement_acceptance_full_pk_dedup_preserves_all_rows`
  - `financial_table_merge_failure_falls_back_to_full_export_preserves_history`
  - Per-table drift guards for all append/import-compass tables.
