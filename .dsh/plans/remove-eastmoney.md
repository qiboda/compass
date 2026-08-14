# remove-eastmoney - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST. -->

**What you'll get:** Rust GUI only reads local Parquet data via DuckDbProvider — no EastMoney HTTP, no CachedProvider read-through, no compass.db. CLI loses `download` subcommand (data collection now in Python). About 2000 lines deleted across 9 files.

**Why this approach:** #31 already made DuckDbProvider self-sufficient via parquet fallback. EastMoneyProvider and CachedProvider are now dead weight — removing them eliminates ~2000 lines of code, drops the reqwest dependency from the GUI crate, and simplifies the data access path to a single provider.

**What it will NOT do:** Break compass-data import/merge/export CLI. Touch ParquetReader or DuckDbProvider internals. Affect provider traits (DataProvider/DataWriter/NegativeCache remain).

**Effort:** Medium
**Risk:** Medium — multiple crate changes, test suite must stay green throughout
**Decisions to sanity-check:** CachedProvider removal means no negative cache in GUI (was EastMoney-specific anyway). ApiConfig removal means config.toml `[api]` section is dead for GUI.

Your next move: approve the plan, then run with `$start-work`. Full execution detail follows below.

---

> TL;DR (machine): Medium effort, delete EastMoneyProvider + CachedProvider + download CLI + refactor backend.rs to direct DuckDbProvider. ~2000 lines deleted, 4 crates touched.

## Scope
### Must have
- Delete `crates/compass-core/src/data/eastmoney.rs` entirely
- Delete `CachedProvider` struct, impl, and tests from `crates/compass-core/src/data/mod.rs`
- Delete `pub mod eastmoney` from `crates/compass-core/src/data/mod.rs`
- Delete `crates/compass-data/src/download.rs`, `retry.rs`, `chunk.rs`, `progress.rs`
- Remove download subcommand from `crates/compass-data/src/main.rs`
- Replace CachedProvider+EastMoneyProvider with direct DuckDbProvider in `crates/compass/src/backend.rs`
- Remove `reqwest` from `crates/compass/Cargo.toml`
- Remove `ApiConfig` struct, `Default impl`, default functions from `crates/compass-core/src/model.rs`
- Delete `crates/compass-core/benches/eastmoney_bench.rs`
- Update EastMoney-dependent integration tests in `crates/compass-core/tests/integration_test.rs`
- Update `kb/` docs (AGENTS.md, architecture, data-providers, config)

### Must NOT have (guardrails, anti-slop, scope boundaries)
- Must NOT touch `crates/compass-data/src/import_dolt.rs`, `import_compass.rs`, `merge.rs`, `export.rs`
- Must NOT touch `crates/compass-core/src/data/parquet.rs` (ParquetReader)
- Must NOT touch `crates/compass-core/src/data/duckdb.rs` (DuckDbProvider)
- Must NOT touch `crates/compass-core/src/data/provider.rs` (trait definitions)
- Must NOT touch `crates/compass-core/src/data/symbol.rs`
- Must NOT touch `crates/compass-data/Cargo.toml` or `crates/compass-core/Cargo.toml`
- Must NOT break import/merge/export CLI functionality

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD for backend.rs refactor; tests-after for deletions (characterization via existing tests)
- Evidence: .omo/evidence/remove-eastmoney/

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. Delete eastmoney.rs | — | 4 | 2, 3 |
| 2. Delete download modules | — | 5 | 1, 3 |
| 3. Delete eastmoney_bench.rs | — | — | 1, 2 |
| 4. Remove CachedProvider + pub mod eastmoney | 1 | 6, 7 | 5 |
| 5. Remove download from main.rs | 2 | — | 4 |
| 6. Refactor backend.rs | 4 | — | 7 |
| 7. Remove reqwest + ApiConfig | 6 | — | — |
| 8. Update integration tests | 4, 6 | — | 9, 10 |
| 9. Update kb/ docs | — | — | 8 |
| 10. Final verify (test+clippy+fmt) | 1-9 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.

- [ ] 1. Delete `crates/compass-core/src/data/eastmoney.rs` (entire file)
  What to do: `rm crates/compass-core/src/data/eastmoney.rs`. Must NOT touch any other file in this todo.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 4
  References: crates/compass-core/src/data/eastmoney.rs (1422 lines)
  Acceptance criteria: File no longer exists. `cargo check -p compass-core` should fail (other code still references it).
  QA scenarios: Verify file deleted, verify no references broken in next wave.
  Commit: N (combined with todo 2)

- [ ] 2. Delete download subcommand modules from compass-data
  What to do: Delete these 4 files: `crates/compass-data/src/download.rs`, `retry.rs`, `chunk.rs`, `progress.rs`. Must NOT touch main.rs registration — todo 5 handles that.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 5
  References: crates/compass-data/src/download.rs, retry.rs, chunk.rs, progress.rs
  Acceptance criteria: Files deleted. `cargo check -p compass-data` fails (main.rs still references them).
  QA: verify files deleted.
  Commit: N

- [ ] 3. Delete `crates/compass-core/benches/eastmoney_bench.rs`
  What to do: Delete the file. Must NOT touch any other bench.
  Parallelization: Wave 1 | Blocked by: —
  References: crates/compass-core/benches/eastmoney_bench.rs
  Acceptance: file deleted.
  Commit: N

- [ ] 4. Remove `CachedProvider` and `pub mod eastmoney` from `crates/compass-core/src/data/mod.rs`
  What to do: Delete `pub mod eastmoney;` line, delete the entire `CachedProvider` struct + impl block + tests. Keep `DataProvider`, `DataWriter`, `NegativeCache` imports from `provider.rs`. Must NOT touch `pub mod duckdb`, `pub mod parquet`, `pub mod symbol`, `pub mod synthetic`.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 6, 7
  References: crates/compass-core/src/data/mod.rs (entire file, ~457 lines, CachedProvider at lines 44-139, tests at 141-457)
  Acceptance: `pub mod eastmoney` removed, no CachedProvider symbol in file. `cargo check -p compass-core` fails (backend.rs still uses CachedProvider).
  QA: grep for CachedProvider in mod.rs — should return 0 hits.
  Commit: N

- [ ] 5. Remove download subcommand from `crates/compass-data/src/main.rs`
  What to do: Remove the `Download` variant from the CLI enum, remove its clap derive attributes, remove the match arm that handles it. Must NOT break Import/Merge/Export/Baostock/Backup variants.
  Parallelization: Wave 2 | Blocked by: 2 | Blocks: —
  References: crates/compass-data/src/main.rs (enum Command, Download variant ~lines 31-68, match arm)
  Acceptance: `cargo check -p compass-data` passes with no download references. `cargo run --bin compass-data -- --help` no longer shows download.
  QA: run `cargo run --bin compass-data -- --help` — verify no "download" in output.
  Commit: N

- [ ] 6. Refactor `crates/compass/src/backend.rs` — replace CachedProvider+EastMoneyProvider with direct DuckDbProvider
  What to do — TDD approach:
    a. Write a RED test in backend (or verify existing test fails before changes)
    b. Remove `use ... eastmoney::EastMoneyProvider` import
    c. Remove `use ... CachedProvider` import  
    d. Remove `let base_url = config.api.base_url.clone();` line
    e. Remove `let client = reqwest::Client::builder()...` block
    f. Remove `let reader = EastMoneyProvider::new(...)` block
    g. Replace `let provider = CachedProvider::new(reader, cache);` with `let provider = cache;` (keep DuckDbProvider as the DataProvider)
    h. Remove `base_url` and `client` captures from the outer closure
    i. Verify: `cargo check -p compass` passes, `cargo test -p compass` passes
  Must NOT: change DuckDbProvider construction, touch result slot startup, or change FetchRequest/FetchResponse handling.
  Parallelization: Wave 3 | Blocked by: 4 | Blocks: 7
  References: crates/compass/src/backend.rs (full file ~124 lines), crates/compass-core/src/data/duckdb.rs (DuckDbProvider APIs)
  Acceptance: `cargo check -p compass`, `cargo test -p compass-core` all pass. No EastMoney/CachedProvider imports in backend.rs.
  QA: `cargo test -p compass-core` — all existing tests pass. `cargo test -p compass` — all tests pass.
  Commit: Y | refactor(compass): replace CachedProvider with direct DuckDbProvider, remove EastMoney wiring

- [ ] 7. Remove `reqwest` from `compass/Cargo.toml` + delete `ApiConfig` from `compass-core/src/model.rs`
  What to do:
    a. `compass/Cargo.toml`: remove `reqwest` from `[dependencies]`
    b. `compass-core/src/model.rs`: delete `ApiConfig` struct (lines 133-142), its `Default impl` (163-170), `default_base_url()` fn, `default_timeout_secs()` fn, remove `api: ApiConfig` field from `AppConfig`, remove `#[serde(default)] pub api: ApiConfig` line, update `AppConfig` doc comment
  Must NOT: touch `AppSection`, `DatabaseConfig`, `CompassState`, or any other model type.
  Parallelization: Wave 3 | Blocked by: 6 | Blocks: —
  References: crates/compass/Cargo.toml, crates/compass-core/src/model.rs (lines 113-195)
  Acceptance: `cargo check --workspace` passes. No reqwest in compass deps. No ApiConfig in model.rs.
  Commit: Y | chore: remove reqwest from compass, delete ApiConfig

- [ ] 8. Update EastMoney-dependent integration tests
  What to do: Remove EastMoneyProvider-dependent tests from `crates/compass-core/tests/integration_test.rs`:
    - `e2e_fetch_bars_real_api_returns_data` (already #[ignore])
    - `e2e_fetch_stock_basic_real_api_returns_valid_data` (already #[ignore])
    - `e2e_search_all_symbols_real_api_returns_stocks` (already #[ignore])
    - `e2e_two_symbols_kline_fetch_and_save_to_duckdb` (uses EastMoneyProvider)
    - `e2e_empty_search_all_symbols_handled_gracefully` (uses EastMoneyProvider)
    - Remove `use ... eastmoney::EastMoneyProvider` import
    Keep: `duckdb_in_memory_has_required_tables`, `parquet_reader_loads_exported_data`
  Must NOT: break remaining tests.
  Parallelization: Wave 4 | Blocked by: 4, 6 | Blocks: —
  References: crates/compass-core/tests/integration_test.rs
  Acceptance: `cargo test -p compass-core` all remaining tests pass (expect ~136 tests after deletions).
  Commit: Y | test: remove EastMoney-dependent integration tests

- [ ] 9. Update kb/ docs — remove EastMoney/CachedProvider/download references
  What to do:
    - `kb/design/architecture.md`: update crate diagram — remove EastMoneyProvider, update data path description
    - `kb/design/data-providers.md`: remove EastMoneyProvider from provider hierarchy, update CachedProvider description
    - `kb/user/config.md`: remove `[api]` section and `ApiConfig` references
    - `kb/user/cli.md`: remove download subcommand documentation (if exists)
    - `AGENTS.md`: update compass-data CLI commands (remove download)
  Must NOT: change anything about Parquet, DuckDB, import/merge/export docs.
  Parallelization: Wave 4 | Blocked by: — | Blocks: —
  Acceptance: grep for "EastMoney" and "CachedProvider" in kb/ — only historical references in reflections.md remain.
  Commit: Y | docs: update kb/ for EastMoney/CachedProvider removal

- [ ] 10. Final verification — full workspace test + clippy + fmt
  What to do:
    `cargo test --workspace`
    `cargo clippy --workspace -- -D warnings`
    `cargo fmt --check`
  Must NOT: skip any crate.
  Parallelization: Wave 4 | Blocked by: 1-9
  Acceptance: ALL tests pass, clippy clean, fmt clean. No compile warnings.
  Commit: N (verification only)

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE.

- [ ] F1. Plan compliance audit — verify all 10 todos completed, no scope creep
- [ ] F2. Code quality review — verify no leftover EastMoney/CachedProvider references, imports clean
- [ ] F3. Real manual QA — `cargo test --workspace` all green, `cargo run --bin compass-data -- --help` no download
- [ ] F4. Scope fidelity — verify Must NOT have items: import/merge/export untouched, ParquetReader/DuckDbProvider/traits intact

## Commit strategy
- Combine todos 1-5 into single commit (deletion wave, no behavioral change)
- Todo 6 standalone (backend.rs refactor)
- Todo 7 standalone (Cargo.toml + model.rs)
- Todo 8 standalone (tests)
- Todo 9 standalone (docs)

## Success criteria
1. `cargo test --workspace` — all tests pass
2. `cargo clippy --workspace -- -D warnings` — clean
3. `grep -r "EastMoney" crates/compass/src/` — zero results
4. `grep -r "CachedProvider" crates/compass/src/` — zero results
5. `cargo run --bin compass-data -- --help` — download not shown
6. `cargo build --bin compass` — compiles without reqwest dependency
