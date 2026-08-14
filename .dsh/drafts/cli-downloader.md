---
slug: cli-downloader
status: approved
intent: clear
pending-action: rewrite .omo/plans/cli-downloader.md (DuckDB migration added)
approach: >
  Replace rusqlite+SqliteProvider with duckdb-rs+DuckDbProvider across the
  entire project (GUI + CLI). DuckDB database file (compass.duckdb) replaces
  SQLite (compass.db). CachedProvider cache field changed from hardcoded
  SqliteProvider to generic C: DataProvider. CLI writes data via DuckDbProvider
  + optional Parquet export. Schema adapted for DuckDB (native TIMESTAMP type
  available, keep epoch integers for compatibility).
---

# Draft: cli-downloader (revised — DuckDB/Parquet migration)

## Components (topology ledger)
| id | outcome | status | evidence path |
| -- | ------- | ------ | ------------- |
| C0 | Replace SqliteProvider → DuckDbProvider; delete sqlite.rs, create duckdb.rs | active | src/data/sqlite.rs, src/data/mod.rs:23-28 |
| C1 | Cargo.toml: remove rusqlite, add duckdb; add [lib] + [[bin]] | active | Cargo.toml |
| C2 | Extract src/lib.rs; update main.rs imports; make CachedProvider cache generic | active | src/main.rs:12-13, src/data/mod.rs:23 |
| C3 | Symbol enumeration — search_all_symbols() on EastMoneyProvider | active | src/data/eastmoney.rs:168-213 |
| C4 | CLI binary — src/bin/downloader.rs with clap, tokio, DuckDB pipeline | active | new |
| C5 | Incremental fetch + retry + indicatif progress | active | new |
| C6 | Parquet export support in CLI | active | new |
| C7 | KB sync + integration tests | active | kb/*.md |

## Key decisions (revised)

1. **DuckDB replaces SQLite everywhere**: GUI and CLI both use DuckDB. Delete `src/data/sqlite.rs`, create `src/data/duckdb.rs`. DuckDbProvider implements both `DataProvider` and `DataWriter` traits (same pattern as SqliteProvider).

2. **CachedProvider cache made generic**: Currently `cache: sqlite::SqliteProvider` is hardcoded (`src/data/mod.rs:25`). Change to `cache: C` where `C: DataProvider`. This is a one-line type parameter change.

3. **Schema ported to DuckDB**: Same `bars` table, `no_data_marks` table. DuckDB supports `INSERT OR REPLACE` (same syntax). Epoch integers kept for timestamp (not DuckDB TIMESTAMP) to maintain compatibility with existing code using `DateTime::from_timestamp()`.

4. **DuckDB connection**: `duckdb::Connection` is thread-safe (unlike rusqlite which needs `Arc<Mutex<>>`). DuckDbProvider can use `Arc<Connection>` directly without Mutex. This simplifies `spawn_blocking` closures.

5. **Database file**: `compass.duckdb` replaces `compass.db`. Default path unchanged in config (just extension changes).

6. **Parquet export**: CLI adds `--export-parquet <dir>` flag. After download completes, runs `COPY bars TO '{dir}/bars.parquet' (FORMAT PARQUET)` via DuckDB. Parquet is columnar compressed — ideal for stock time series analysis.

7. **duckdb crate**: `duckdb = { version = "1", features = ["bundled"] }` — bundled DuckDB C library, same pattern as rusqlite bundled.

## Metis gap findings (from previous pass, still applicable)
| Gap# | Finding | Resolution |
|------|---------|------------|
| G1 | Cargo.toml changes | Added: remove rusqlite, add duckdb, [lib] + [[bin]] |
| G2 | adj_type labeling | Pre-existing; document; duckdb schema mirrors sqlite |
| G4 | Chunking algorithm | Date-based with timeframe-aware windows |
| G5 | search_symbols trait | search_all_symbols on EastMoneyProvider only |
| G6 | Retry strategy | 3-retry exponential backoff |
| G7 | Exit codes | 0=all success, 1=any failure |
| G11 | CLI flag semantics | Specified |
| G12 | Runtime type | current_thread for CLI |
| G17 | KB sync | Exact updates per file specified |

## Scope IN
- Delete `src/data/sqlite.rs`; create `src/data/duckdb.rs` with DuckDbProvider
- CachedProvider cache field → generic `C: DataProvider`
- Cargo.toml: remove `rusqlite`, add `duckdb` (bundled), add `clap`, `indicatif`, `futures`
- src/lib.rs extraction; main.rs update
- EastMoneyProvider::search_all_symbols()
- CLI binary with DuckDB pipeline + Parquet export
- 3-retry exponential backoff + indicatif progress
- Unit + integration tests
- KB docs sync

## Scope OUT
- No SqliteProvider code remains (deleted)
- No data migration tool (users start fresh with `compass.duckdb` or re-download)
- No changes to EastMoney provider or DataProvider trait signatures
- No daemon/scheduling
- No data export beyond Parquet

## Dependencies delta
| Remove | Add |
|--------|-----|
| `rusqlite = { version = "0.32", features = ["bundled"] }` | `duckdb = { version = "1", features = ["bundled"] }` |
| — | `clap = { version = "4", features = ["derive"] }` |
| — | `indicatif = "0.17"` |
| — | `futures = "0.3"` |

## Open questions
(None)

## Approval gate
status: approved (scope change — DuckDB migration folded in)
