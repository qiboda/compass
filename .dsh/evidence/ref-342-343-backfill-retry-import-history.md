# Ref #342/#343 — backfill per-symbol retry + incremental --since history verification

Date: 2026-08-31
Branch: `fix/backfill-retry-import-history` (worktree
`/data/codes/compass/.worktrees/fix-backfill-retry-import-history`)
HEAD at evidence time: `82e8f2a` (round-2 defensive commit; implementation set complete)

## Commits (origin/master..HEAD)

| Commit | Message |
|---|---|
| 46a324b | docs: record main_flow backfill retry and incremental import backfill history loss (pre-fix toolchain.md records) |
| dfb6174 | docs: rewrite worktree handoff for backfill retry and incremental import history fix |
| 64ef76c | refactor: add backfill per-symbol retry interface skeleton (first compilable interface commit; RED tests delegation target) |
| 8218c93 | fix: retry backfill fetch per symbol with strict batch abort (ref #342) |
| c755024 | fix: verify history before --since merge and drop internal columns (ref #343) |
| 588b71a | docs: land approved plan with placement amendments |
| 1000998 | fix: address review findings on retry and merge history check (round-1, two reviews) |
| 82e8f2a | test: lock zero-attempt and rate-limit invariants, poison-safe stem locks (round-2 reviewers' P2/P3 items) |

## F1 — Gate compliance audit

- **0.5 Worktree**: `.worktrees/fix-backfill-retry-import-history`, handoff written and rewritten (dfb6174) to the correct #342/#343 contract.
- **1 Design**: skipped (pure data-pipeline logic, no UI) — recorded in plan.
- **2 Issue**: #342/#343 both OPEN (A-Data, C-Bug), created before implementation; tracked via gh CLI (GitHub MCP disabled).
- **3 Plan**: `.dsh/plans/fix-backfill-retry-import-history.md` (221+ lines) approved by user via ask_user_question (no plan mode in session); landed in 588b71a.
- **3.5 Adversarial tests**: subagent 37b20975 — #342 entry test with HTTPS_PROXY dead-port injection (wreq default auto_sys_proxy) RED; #343 10 tests RED; 4 interface tests RED after skeleton SHA 64ef76c.
- **4 Requirement tests**: subagent f7bb3357 — #343 empty-after-since-slice repair RED + fast-path anti-regression GREEN; extract_backfill_window 5 tests RED.
- **5b Docs synced**: `.dsh/kb/user/cli.md` (--since incremental verification), `.dsh/kb/design/data-providers.md` (+2 decision-record rows), plan file correction log; `.dsh/kb/dev/toolchain.md` PR number backfill deferred until PR created.
- **5c Decision records**: data-providers.md `## 决策记录` pre-existed; rows appended.

## F2 — Review rounds (Commit → Review)

- **Round 1** (parallel): #342 review 0a9ad431 — passed, P0/P1 = 0 (P2×2, P3×6); #343 review c25d61a0 — passed after fixes (P1×1: create_dir_all for history staging dir; P2×2; P3×6). All fixed in 1000998, including a parallel-test race fix (stem-level static Mutexes).
- **Round 2** (re-review of 1000998): #342 2905da88 — passed, P0/P1/P2 = 0 (P3×3 optional); #343 8ec02954 — passed, P0/P1 = 0 (P2×1 poison-safety, P3×2). All adopted and fixed in 82e8f2a (tests/annotations only, one `debug_assert` in production code).
- No blocker at any round; 2-round limit respected.

## F3 — Test evidence

- `cargo test -p compass-collectors --lib`: **98 passed, 0 failed** (incl. main_flow 29; new `retry_sina_backfill_rejects_zero_attempts`, constant/rate-limit guards).
- `cargo test -p compass-data`: lib **113 passed**; bin units **108 passed**; integration: data_quality_adversarial 19, index_import_compass 8, name_en_data_layer_adversarial 3, requirement_index_import 4, requirement_name_en_data 3 — all 0 failed.
- `cargo fmt --check`: clean. `cargo clippy -p compass-data -p compass-collectors --all-targets`: exit 0, no warnings.
- **Workspace `just check`** (fmt --check + `cargo clippy -D warnings` + `cargo test` across all crates incl. doc-tests): **exit 0** (2026-08-31, post 82e8f2a).
- RED evidence (pre-implementation, archived in agent reports): #342 entry test failed with bare `HTTP error ... client error (ProxyConnect)` (no symbol/attempts); #343 9/10 merge tests failed (leaked priority/rn; missing/orphaned/stale history rows; QueryReturnedNoRows for tradedate; silent fallback poisoning).

## F4 — Scope fidelity vs approved plan

**#342 (main_flow backfill retry):**
- Retry 3 per symbol — `SINA_BACKFILL_RETRIES = 3`, test `sina_backfill_retries_constant_is_three` ✓
- Backoff 2s/4s — `SINA_BACKFILL_BACKOFF = 2s`, `backoff * (1u32 << attempt)`; backoff-sequence test locks ≥30ms at 10ms base; constant test locks `BACKOFF >= SINA_MIN_INTERVAL` ✓
- Strict batch abort, error names symbol — `BackfillSymbolFailed { symbol, attempts: 3, reason }`; entry test asserts symbol + "3 attempts" + no partial CSV ✓
- `extract_backfill_window` pure fn (parse_sina_row + inclusive range) — 5 requirement tests ✓
- Daily path untouched — `fetch_symbol_window`/`run()` daily branch unchanged ✓

**#343 (incremental merge history verification):**
- Consistent history → fast merge, no internal columns — `incremental_merge_fast_path_no_internal_columns` (exact 9-column set), `..._fast_path_keeps_new_rows_and_matches_dolt_values`, `..._fast_path_no_fallback` (index_daily) ✓
- Auto-healed history older than anchor → full export alignment — `repairs_missing_history_before_since`, `repairs_stale_history_values`, `large_history_single_missing_row`, `empty_after_since_slice_still_repairs_auto_healed_history`, `removes_orphaned_parquet_rows` ✓
- Same-key stale values repaired — value-compare tests (non-prefer-new fin_indicators too) ✓
- Unreadable old parquet → recovery — `corrupt_parquet_falls_back_with_backup` (anti-regression) ✓
- No priority/rn on either path — column-set asserts + `second_run_no_fallback_no_leak` (no new pre_merge_backup) ✓
- index_daily tradedate mapping — `incremental_merge_index_daily_tradedate_detects_history_divergence` ✓
- Real-data smoke (2026-08-30): `import-compass --table capital_main_flow --since 2026-08-28` exit 0, fast merge (no divergence log), rows 118097 == Dolt, 9 columns, no priority/rn.

## Related automated verification

- Full RED→GREEN test record: adversarial 37b20975 + requirement f7bb3357 reports (12+2 merge tests, 4+1 retry tests, 5 extract tests).
- Test helper additions: `parquet_columns`, `this_process_pre_merge_backup_exists/files`, stem-level static `Mutex` serde for /tmp/compass_parquet_work backup races.
