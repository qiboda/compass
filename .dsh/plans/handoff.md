# Handoff: fix #342 + #343 (backfill retry + incremental import history loss)

## Purpose
Fix two related data-pipeline bugs found during the 2026-08-30 database update:
- #342: `main_flow.rs::backfill()` has no per-symbol retry, so transient Sina HTTP failures abort the whole `update-database.sh`.
- #343: `import-compass --since` incremental merge cannot see auto-healed historical rows older than `--since`, causing Dolt/Parquet divergence; merge success also leaks `priority`/`rn` internal columns into the final Parquet.

User instruction: “这两个bug去修复。” One worktree/PR for both bugs.

## Locked grill decisions (contract)
1. **One worktree/PR** covering #342 + #343.
2. **#342 failure policy**: per-symbol retry 3 times (consistent with daily `fetch_symbol_window`); if still failing after retries, **abort the whole batch (strict failure)** with the failed symbol in the error; no skip-and-continue.
3. **#343 fix approach**: in `import_append_table`, before merging with `--since`, do a safety check comparing Dolt vs old Parquet for historical rows (`date_col < since`). If historical rows are missing/stale, **fall back to full export without `--since`**; otherwise keep the fast incremental merge.
4. **Internal columns**: also clean `priority`/`rn` before writing the merged Parquet, so the merge success path never leaves internal columns in the production file.
5. **Validation**: unit/integration tests RED→GREEN + real-data targeted smoke (small backfill/re-import scenario); do NOT run the ~1h full `update-database.sh` unless needed.

## Issues
- #342 — `main_flow.rs::backfill()` 无单股重试
- #343 — `import-compass --since` 无法同步 auto-heal 回补的早于锚点历史

## Key code locations
- `crates/compass-collectors/src/main_flow.rs`
  - `SINA_URL` = `https://money.finance.sina.com.cn/quotes_service/api/json_v2.php/MoneyFlow.ssl_qsfx_lscjfb`
  - `SINA_DAILY_NUM=20`, `SINA_BACKFILL_NUM=1000`, `SINA_MIN_INTERVAL=100ms`
  - `fetch_symbol_window()` (~187-227): per-symbol 3 retries with `2u64 << attempt` seconds (2s/4s), then Err.
  - `backfill()` (~336-401): currently loops `client.get_json_with_headers_and_proxy(...).await?` with **no per-symbol retry** — this is #342.
- `crates/compass-data/src/import_compass.rs`
  - `import_append_table()` (~395-510): `effective_since = if path.exists() { since } else { None }`; date filter `WHERE {date_col} >= '{s}'`; DuckDB merge uses `ROW_NUMBER() OVER (PARTITION BY ...)` and adds `priority`/`rn`; success branch `std::fs::copy(&tmp_path, &path)` writes internal columns into the production Parquet (#343).
  - Known fallback already does true full export + backup + full row-count validation (from #298).
- `crates/compass-collectors/src/orchestrate.rs`: `DAILY_AUTO_HEAL_TABLES` = capital_main_flow/index_daily/dragon_list/block_trade; `auto_heal()` calls `backfill()` then `require_nonzero` (0 rows fails).
- `scripts/update-database.sh`: step 2 `compass-collectors sync`; step 4 `import-compass` tables; no SEPA compute steps currently.

## Repo state at creation
- Base: local `master` at `46a324b` (includes un-pushed docs commit `46a324b` recording #342/#343 in `.dsh/kb/dev/toolchain.md`; upstream origin/master is `c80fe93`).
- Branch: `fix/backfill-retry-import-history`.
- Two Dolt repos are clean/pushed; Parquet files were fully rebuilt and Dolt==Parquet as of the 2026-08-30 manual repair.
- Other old worktrees exist but are unrelated.

## Process reminders
- First step after opening worktree: read this handoff, then `git fetch origin && git rebase origin/master` before starting.
- Follow PRE-IMPLEMENTATION GATE: issues already exist (#342/#343); create `.dsh/plans/` plan in this worktree, adversarial + requirement tests RED first, docs sync, decision records.
- All `.dsh` outputs must be created inside this worktree and committed with the PR.
- Do not modify the main `/data/codes/compass` checkout; this worktree owns the implementation.
- Dolt write-back requires commit+push; this task should not need Dolt writes except for the real-data smoke if it modifies Dolt (clean up/skip if possible).
