# Data Providers

Compass abstracts all stock data access behind a **trait system**. The GUI uses
`DuckDbProvider` as its sole provider — all data is read from local Parquet files
via `read_parquet()`. No online API calls, no HTTP dependencies in the GUI.

## Why traits?

Two problems demanded abstraction:

1. **Multiple backends**: The data pipeline uses DuckDB (in-memory cache) and
   Parquet (main database). Without a shared interface, every consumer would
   need to know which backend it's talking to.

2. **Testability**: Unit tests can provide mock implementations that return
   predefined data, avoiding real databases.

## The three traits

```rust
#[async_trait]
pub trait DataProvider: Send + Sync {
    async fn fetch_bars(&self, symbol, timeframe, range_start, range_end)
        -> Result<Vec<Bar>, DataError>;
    async fn search_symbols(&self, query: &str)
        -> Result<Vec<SymbolInfo>, DataError>;
}

#[async_trait]
pub trait DataWriter: Send + Sync {
    async fn save_bars(&self, symbol, timeframe, bars: &[Bar], overwrite: bool)
        -> Result<(), DataError>;
}

#[async_trait]
pub trait NegativeCache: Send + Sync {
    async fn mark_no_data(&self, symbol: &str, timeframe: &str)
        -> Result<(), DataError>;
    async fn is_no_data(&self, symbol, timeframe, now_ts, ttl_secs)
        -> Result<bool, DataError>;
}
```

### DataProvider — read-only access

The core fetch interface. Anything that can produce `Vec<Bar>` for a given
symbol/timeframe/date-range implements this. Implementors: DuckDbProvider,
ParquetReader, and mock implementations in tests.

`search_symbols` powers the symbol search box in the GUI. Returns a list of
`SymbolInfo { code, name }` matching the query.

### DataWriter — write-through persistence

Called after a fetch to persist bars locally. The `overwrite` flag controls
behavior: `false` = INSERT OR IGNORE (skip duplicates), `true` = INSERT OR
REPLACE (update existing). DuckDbProvider is the only implementor.

### NegativeCache — avoid repeated failures

When no data exists for a symbol (delisted, doesn't exist, never imported),
the negative cache marks the (symbol, timeframe) pair with a timestamp.
Subsequent fetches within the TTL window skip the read attempt entirely,
avoiding wasted Parquet file I/O and DuckDB queries.

## The provider hierarchy

```
Compass GUI (backend.rs)
    │
    └── DuckDbProvider  ←  sole data provider, reads parquet_data/ directly
            │
            ├─ In-memory DuckDB (cache layer)
            │     stock_daily, stock_basic, stock_adj_factor,
            │     stock_limit, no_data_marks tables
            │
            └─ Parquet fallback
                  On cache miss: read_parquet('parquet_data/stock_daily/{symbol}.parquet')
                  Cache-warm: INSERT OR IGNORE into in-memory tables
```

The GUI uses `DuckDbProvider` directly — it reads from `parquet_data/` via
`read_parquet()` with an in-memory DuckDB connection for caching recently
fetched data. All data is local, no online fallback.

```rust
// backend.rs: DuckDbProvider is the sole data provider
let provider = DuckDbProvider::new(parquet_dir.exists().then_some(parquet_dir))?;
provider.fetch_bars(symbol, timeframe, start, end).await
```

### Read path

```
fetch_bars(symbol, timeframe, start, end)
    │
    ├─ 1. NEGATIVE CACHE CHECK
    │     cache.is_no_data(symbol, timeframe, now, TTL_7DAYS)?
    │     → If true: return DataError::NoData (skip I/O)
    │     → If false: continue
    │
    ├─ 2. IN-MEMORY CACHE READ
    │     SELECT * FROM stock_daily WHERE symbol=? AND trade_date BETWEEN ? AND ?
    │     → If non-empty: return bars ✓
    │     → If empty: cache miss → read from Parquet
    │
    ├─ 3. PARQUET FALLBACK
    │     read_parquet('parquet_data/stock_daily/{symbol}.parquet')
    │     → If data exists: cache-warm into in-memory tables, return bars
    │     → If no data: mark_no_data() → return DataError::NoData
    │
    └─ 4. RETURN
          result to caller
```

Key behaviors:

- **Cache hit requires non-empty bars.** An empty result means "not cached" —
  we never short-circuit with zero bars back to the caller.
- **Empty results from Parquet are marked no-data.** A missing Parquet file or
  empty result means the symbol was never imported — we mark it in the negative
  cache to avoid repeated disk I/O.
- **save_bars uses overwrite=true** when cache-warming. We just confirmed the
  cache is empty, so there's nothing to merge — we can safely replace.

## EastMoney data pipeline (Python collectors)

EastMoney (东方财富) is used **only by the Python collectors**, not the Rust
GUI. The collectors fetch data from EastMoney's public HTTP API and write it
to CSV files, which are then imported into Dolt `compass_data`. The Rust side
only reads local Parquet/DuckDB data — it has no HTTP client dependency.

For EastMoney API details (secid mapping, K-line endpoints, timeframes),
see `kb/design/symbols.md`.

See `collectors/` for the Python scripts and `kb/dev/process.md` for the
import workflow.

## DuckDbProvider — local cache and staging

DuckDbProvider is the primary data provider for the GUI. It implements all
three traits and serves as both the GUI cache and the CLI staging database.

### Why DuckDB for caching?

- **Writes are the use case**: we need INSERT OR REPLACE/IGNORE for idempotent
  caching. DuckDB handles this naturally with its SQL dialect.
- **Reads are fast for analytical queries**: `SELECT ... WHERE symbol=? AND
  trade_date BETWEEN ? AND ? ORDER BY trade_date` — a textbook OLAP query.
- **Zero setup**: the database file is created on first open. No schema
  migrations, no config.
- **In-memory mode for tests**: `DuckDbProvider::new_in_memory()` gives each
  test a fully isolated database. No cleanup, no interference.

### Threading

DuckDB's Rust bindings wrap a synchronous C library. Every database operation
goes through `tokio::task::spawn_blocking`, which moves the call to a dedicated
thread pool:

```rust
// Inside DuckDbProvider
let conn = self.conn.clone();
tokio::task::spawn_blocking(move || {
    let conn = conn.lock().unwrap();
    conn.execute("SELECT ...", params![])
}).await.unwrap()
```

The connection itself is `Arc<Mutex<Connection>>`. Only one query at a time
per database — but since queries are fast (<1ms for cached reads), contention
is negligible.

### Schema

Five tables, all created automatically on first use:

| Table | Key | Purpose |
|---|---|---|
| `stock_daily` | `(symbol, trade_date)` | Core OHLCV bars — the main cache table |
| `stock_basic` | `symbol` | Stock name, industry, exchange, listing dates |
| `stock_adj_factor` | `(symbol, trade_date)` | Price adjustment factors (from Baostock) |
| `stock_limit` | `(symbol, trade_date)` | Daily price ceiling/floor |
| `no_data_marks` | `(symbol, timeframe)` | Negative cache entries with TTL timestamps |

The full DDL is in `architecture.md` and `AGENTS.md`.

### Gap detection

`get_stored_range(symbol)` returns `(MIN(trade_date), MAX(trade_date))` for a
stock. The CLI downloader uses this to determine which date ranges need fetching:

```
stored range:   2020-01-02 ──────────── 2024-12-31
requested:      2019-01-01 ──────────────────────── 2025-07-21
                                     ^^^^^^^^^^^^^^
                                     only this gap needs downloading
```

This skips re-downloading already-cached dates, reducing API calls and speeding
up incremental updates.

### Write semantics

All write methods accept an `overwrite: bool`:

| `overwrite` | SQL | Behavior |
|---|---|---|
| `false` (default) | `INSERT OR IGNORE` | Skip rows where the key already exists |
| `true` | `INSERT OR REPLACE` | Replace existing rows with new values |

This is the same semantic used by the CLI subcommands — `--overwrite` controls
whether existing data is preserved or replaced.

## ParquetReader — the main database

ParquetReader reads from the Parquet files produced by `compass-data import`.
It implements `DataProvider` but NOT `DataWriter` — the Parquet store is
append-only (new symbols are added as new files, existing data is never
modified in-place).

### How it works

`ParquetReader` wraps a DuckDB in-memory connection and uses `read_parquet()`
to query Parquet files directly:

```sql
SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume
FROM read_parquet('parquet_data/stock_daily/SH600519.parquet')
WHERE tradedate >= '2025-01-01' AND tradedate <= '2025-07-21'
ORDER BY tradedate ASC
```

No data is loaded into tables. DuckDB reads the Parquet file on each query,
exploiting columnar projection and predicate pushdown for efficiency.

### Symbol discovery

`list_symbols()` scans the filesystem: `std::fs::read_dir("parquet_data/stock_daily/")`.
Each `.parquet` filename (minus extension) is a symbol. This is fast because
there's no database to query — it's just a directory listing.

`search_symbols()` loads `stock_basic.parquet` into DuckDB and runs a LIKE
query against the `name` column. First time is slow (full scan), but the
in-memory DuckDB keeps it warm for subsequent searches.

### When to use ParquetReader vs DuckDbProvider

| Scenario | Use |
|---|---|
| GUI (daily use) | DuckDbProvider (in-memory cache + Parquet fallback) |
| CLI: bulk data querying | ParquetReader directly |
| Test environment | DuckDbProvider (in-memory) |

## Dolt import — the bulk data pipeline

Dolt is a MySQL-compatible database with Git-like versioning. The
`investment_data` Dolt database contains the complete history of A-share
EOD prices — 18.25 million rows across 6,122 stocks, from 1990 to present.

The import pipeline (`compass-data import`) works as follows:

```
dolt sql -r csv -q "SELECT DISTINCT symbol FROM final_a_stock_eod_price"
    │
    ├─ Produces list of 6123 symbols (e.g. "SZ000001", "SH600519")
    │     [Note: symbol list query still uses CSV for simple text parsing]
    │
    ├─ For each symbol:
    │     dolt sql -r parquet -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001'"
    │       → binary Parquet bytes (no CSV intermediation)
    │       → written directly to parquet_data/stock_daily/SZ000001.parquet
    │
    └─ Stock basic info → parquet_data/stock_basic.parquet
```

**Merge vs overwrite**: by default (`--overwrite` not set), the import is
migration-style:
1. Read existing Parquet data (if any) via `read_parquet`
2. Write Dolt Parquet data to temp file
3. Use `ROW_NUMBER() OVER (PARTITION BY tradedate ORDER BY priority)` to
   deduplicate: existing data gets priority 1, Dolt data gets priority 2
4. Write merged result as new Parquet file via DuckDB `read_parquet` + `COPY TO`

This means you can run `import` repeatedly without losing any data you've
already imported from other sources. Only genuinely new dates are added.

With `--overwrite`: the Parquet file is rewritten entirely from Dolt data.
Use this when Dolt is the canonical source and you want a clean slate.

## Error handling

### The DataError enum

```rust
pub enum DataError {
    Network(reqwest::Error),     // HTTP failures (legacy — not used by GUI)
    Database(duckdb::Error),     // DuckDB failures (corrupt file, disk full, lock)
    Parse(String),               // JSON deserialization, date parsing, mutex poison
    RateLimited(u64),            // Rate limit (legacy — not used by GUI)
    NoData { symbol: String },   // Symbol has no data (not imported, delisted, invalid)
}
```

Note: `Network` and `RateLimited` are legacy variants from the removed
EastMoneyProvider. They are retained for backward compatibility with the
CLI pipeline but are never raised by the GUI's data path.

### Design philosophy

- **Precise in the library**: `DataError` variants let callers distinguish
  between "this symbol doesn't exist" and "the database is corrupt." The GUI
  can show different messages; the CLI can decide whether to retry.
- **Ergonomic propagation**: `From<reqwest::Error>` and `From<duckdb::Error>`
  are implemented, so `?` works directly in provider methods.
- **Parse errors carry context**: `DataError::Parse(String)` includes the raw
  string that failed to parse, not just a generic message. This makes debugging
  straightforward.
- **No unwrap in production**: library code uses `?` or `.expect(msg)` with
  descriptive messages. No bare `.unwrap()` — the error message must explain
  what went wrong and where.

## Provider priority chain

```
1. Dolt investment_data (local, 18M+ rows, 1990–2026)
     └─ Imported via: compass-data import
     └─ Stored as: parquet_data/stock_daily/*.parquet
     │
     ▼
2. Parquet main database
     └─ Queried via: ParquetReader or DuckDbProvider (with Parquet fallback)
     │
     ▼
3. DuckDB in-memory cache
     └─ Read-through cache: Parquet data loaded on first access
     └─ Subsequent reads hit memory (cache-warm)
```

All data access is local. The GUI has no online fallback — if data isn't in
the Parquet store, the symbol returns `DataError::NoData`. Users run
`compass-data import` offline to populate the Parquet store before using the GUI.

## Configuring providers

Provider configuration flows from `~/.config/compass/config.toml`:

```toml
[parquet]
dir = "/data/compass-data/parquet_data"  # Parquet data directory

[dolt]
investment_data_dir = "/data/compass-data/investment_data"
compass_data_dir = "/data/compass-data/compass_data"
```

For the CLI, most settings are command-line arguments (see `compass-data --help`):
- `--dolt-dir`, `--output`: override data directories
- `--limit`, `--symbols`: control import scope
- `--overwrite`: replace existing data instead of merging
