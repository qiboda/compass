# Data Providers

Compass abstracts all stock data access behind a **trait system**. This lets us
swap backends without changing the code that consumes them — the GUI uses
`DuckDbProvider` directly; it just calls `provider.fetch_bars()`.

## Why traits?

Two problems demanded abstraction:

1. **Multiple data sources**: Compass reads from Parquet (main database) and
   DuckDB (in-memory cache). Without a shared interface, every consumer would
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
symbol/timeframe/date-range implements this. So far: DuckDbProvider,
ParquetReader, and mock implementations in tests.

`search_symbols` is secondary — it powers the symbol search box in the GUI.
Returns a list of `SymbolInfo { code, name }` matching the query.

### DataWriter — write-through persistence
Called after a fetch to persist bars locally.
The `overwrite` flag controls behavior: `false` = INSERT OR IGNORE (skip
duplicates), `true` = INSERT OR REPLACE (update existing). DuckDbProvider
is the only implementor.

### NegativeCache — avoid repeated failures
The trait is implemented by DuckDbProvider and stores `no_data_marks` entries
with TTL timestamps. In the current local-only architecture the GUI does not
use it — it exists for completeness and CLI staging workflows.

## The provider hierarchy

The GUI uses `DuckDbProvider` directly — it reads from `parquet_data/stock_daily.parquet`
via `read_parquet()` with an in-memory DuckDB connection for caching recently fetched data.
All data is local, no online fallback.

```rust
// backend.rs: DuckDbProvider is the sole data provider
let provider = DuckDbProvider::new(parquet_dir.exists().then_some(parquet_dir))?;
provider.fetch_bars(symbol, timeframe, start, end).await
```

## DuckDbProvider — local cache and export target

DuckDbProvider is the Swiss Army knife of data providers. It implements all
three traits and serves as both the GUI cache and the CLI export target.

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
| `stock_adj_factor` | `(symbol, trade_date)` | Price adjustment factors |
| `stock_limit` | `(symbol, trade_date)` | Daily price ceiling/floor |
| `no_data_marks` | `(symbol, timeframe)` | Negative cache entries with TTL timestamps |

The full DDL is in `AGENTS.md` and `kb/design/architecture.md`.

### Gap detection

`get_stored_range(symbol)` returns `(MIN(trade_date), MAX(trade_date))` for a
stock. Incremental imports use this to determine which date ranges are already
covered:

```
stored range:   2020-01-02 ──────────── 2024-12-31
requested:      2019-01-01 ──────────────────────── 2025-07-21
                                     ^^^^^^^^^^^^^^
                                     only this gap needs importing
```

This skips re-importing already-covered dates, speeding up incremental updates.

### Write semantics

All write methods accept an `overwrite: bool`:

| `overwrite` | SQL | Behavior |
|---|---|---|
| `false` (default) | `INSERT OR IGNORE` | Skip rows where the key already exists |
| `true` | `INSERT OR REPLACE` | Replace existing rows with new values |

The same semantic is used by the CLI subcommands (`import-compass`, `export`) —
`--overwrite` controls whether existing data is preserved or replaced.

### Timeframe aggregation (ref #46)

`DuckDbProvider::fetch_bars()` supports three timeframes:

| Timeframe | Behavior |
|---|---|
| `"1d"` | Returns raw daily bars — no aggregation |
| `"1w"` | Aggregates daily → weekly: `open` = Monday's open, `high` = week max, `low` = week min, `close` = Friday's close, `volume` = week sum |
| `"1M"` | Aggregates daily → monthly: same OHLCV aggregation, `date_trunc('month', ...)` |

Aggregation runs as a DuckDB SQL re-query after daily data is loaded into the
in-memory `stock_daily` table (including parquet fallback cache-warm):

```sql
SELECT DATE_TRUNC('week', trade_date) as grp_date,
       FIRST(open) as open,
       MAX(high) as high,
       MIN(low) as low,
       LAST(close) as close,
       SUM(volume) as volume
FROM (
    SELECT * FROM stock_daily
    WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
    ORDER BY trade_date ASC
)
GROUP BY grp_date
ORDER BY grp_date
```

The subquery's `ORDER BY trade_date ASC` guarantees `FIRST`/`LAST` return the
chronologically earliest/latest values per time bucket. Only DuckDB's
`stock_daily` path performs aggregation; the `ParquetReader` (direct parquet
read) always returns daily data.

## ParquetReader — the main database

ParquetReader reads from the Parquet files produced by `compass-data import`.
It implements `DataProvider` but NOT `DataWriter` — the Parquet store is
append-only (data is merged into the single file, existing data is never
modified in-place).

### How it works

`ParquetReader` wraps a DuckDB in-memory connection and uses `read_parquet()`
to query the single Parquet file with `WHERE symbol = ?` filtering:

```sql
SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume
FROM read_parquet('parquet_data/stock_daily.parquet')
WHERE symbol = ? AND tradedate >= ? AND tradedate <= ?
ORDER BY tradedate ASC
```

Symbols are bound as DuckDB parameters (`?`), not interpolated into the SQL string.
No data is loaded into tables. DuckDB reads the Parquet file on each query,
exploiting columnar projection and predicate pushdown for efficiency.

### Symbol discovery

`list_symbols()` first checks for `stock_daily.symbols.txt` (one symbol per
line, sorted), which is generated alongside `stock_daily.parquet` by the import
pipeline. This is the fast path — a simple file read.

If `symbols.txt` doesn't exist, it falls back to `SELECT DISTINCT symbol FROM
read_parquet('stock_daily.parquet') ORDER BY symbol`. If neither source exists,
returns an empty vec.

`search_symbols()` loads `stock_basic.parquet` into DuckDB and runs a LIKE
query against the `name` column. First time is slow (full scan), but the
in-memory DuckDB keeps it warm for subsequent searches.

### When to use ParquetReader vs DuckDbProvider

| Scenario | Use |
|---|---|
| GUI (daily use, cached data available) | DuckDbProvider (cache) |
| CLI: bulk data querying | ParquetReader |
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
    │       → written directly to parquet_data/stock_daily.parquet (single file with symbol column)
    │
    │       → symbols list written to parquet_data/stock_daily.symbols.txt
    └─ Stock basic info → parquet_data/stock_basic.parquet
```

The import writes the full dataset directly — there is no merge mode and no
`--overwrite` flag. Re-running it replaces the files with a fresh export from
Dolt. Use `--since` for incremental imports of newer data.

## Error handling

### The DataError enum

```rust
pub enum DataError {
    Network(reqwest::Error),     // HTTP failures (timeout, DNS, connection refused)
    Database(duckdb::Error),     // DuckDB failures (corrupt file, disk full, lock)
    Parse(String),               // JSON deserialization, date parsing, mutex poison
    RateLimited(u64),            // EastMoney rate limit with retry-after seconds
    NoData { symbol: String },   // Symbol has no data (delisted, invalid, API returns null)
}
```

### Design philosophy

- **Precise in the library**: `DataError` variants let callers distinguish
  between "this symbol doesn't exist" and "the network is down." The GUI can
  show different messages; the CLI can decide whether to retry.
- **Ergonomic propagation**: `From<reqwest::Error>` and `From<duckdb::Error>`
  are implemented, so `?` works directly in provider methods.
- **Parse errors carry context**: `DataError::Parse(String)` includes the raw
  string that failed to parse, not just a generic message. This makes debugging
  API response changes straightforward.
- **No unwrap in production**: library code uses `?` or `.expect(msg)` with
  descriptive messages. No bare `.unwrap()` — the error message must explain
  what went wrong and where.

## Configuring providers

Provider configuration flows from `~/.config/compass/config.toml`. The
`[parquet].dir` key sets the Parquet data directory; `[dolt]` keys set the Dolt
repository paths:

```toml
[parquet]
dir = "/data/compass-data/parquet_data"

[dolt]
investment_data_dir = "/data/compass-data/investment_data"
compass_data_dir = "/data/compass-data/compass_data"
```

For the CLI, most settings are command-line arguments (see `compass-data --help`):
- `--dolt-dir`, `--output`: override Dolt directory and output path
- `--input`, `--format`, `--output`: export options
- `--since`: incremental import cutoff

See `kb/user/config.md` for the full config reference.

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 数据访问抽象：Provider 层设计 | 各后端直接调用 / trait 统一接口 | 三 trait 体系：DataProvider + DataWriter + NegativeCache | 多数据源（Dolt/DuckDB/Parquet）需统一接口；trait 支持 mock 实现用于测试，消费者与后端解耦 | 直接调用导致每个消费者需知道后端类型，无法替换或测试 |
| GUI 数据来源 | 在线 API 直连 / 多层读穿缓存 / 纯本地直读 Parquet | DuckDbProvider 直读 `stock_daily.parquet`（`read_parquet()` 回退） | 本地读取零延迟、无网络依赖、无 API 限流；重构后消除 cache miss 与负缓存复杂度 | 在线直连增加延迟和失败点；读穿缓存需维护 CachedProvider、负缓存、inflight 去重等多层状态 |
| 错误处理：错误类型设计 | anyhow 通用错误 / 精确枚举 | DataError 枚举：Network / Database / Parse / RateLimited / NoData，含 From 实现 | 调用方可区分错误类型（如 NoData 表示标的不存在 vs Network 表示网络中断），GUI 可据此展示不同提示；From 实现支持 `?` 传播 | anyhow 丢失错误分类信息，调用方无法做差异化处理；Parse 携带原始字符串便于排查 API 响应变更 |
