# Data Providers

Compass abstracts all stock data access behind a **trait system**. This lets us
swap backends without changing the code that consumes them — the GUI doesn't
care whether bars come from DuckDB or EastMoney; it just calls
`provider.fetch_bars()`.

## Why traits?

Three problems demanded abstraction:

1. **Multiple data sources**: Compass pulls from EastMoney (HTTP), Dolt (CSV
   export), DuckDB (local cache), and Parquet (main database). Without a shared
   interface, every consumer would need to know which backend it's talking to.

2. **Caching layer**: The GUI needs read-through caching — check local first,
   fetch remote on miss, write back to local. A generic `CachedProvider<R, C>`
   can wrap any reader+cache pair without duplicating the cache logic.

3. **Testability**: Unit tests can provide mock implementations that return
   predefined data, avoiding real HTTP calls and real databases.

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
symbol/timeframe/date-range implements this. So far: EastMoneyProvider,
DuckDbProvider, ParquetReader, CachedProvider, and mock implementations
in tests.

`search_symbols` is secondary — it powers the symbol search box in the GUI.
Returns a list of `SymbolInfo { code, name }` matching the query.

### DataWriter — write-through persistence
Called by CachedProvider after a cache miss to persist freshly fetched bars.
The `overwrite` flag controls behavior: `false` = INSERT OR IGNORE (skip
duplicates), `true` = INSERT OR REPLACE (update existing). DuckDbProvider
is the only implementor.

### NegativeCache — avoid repeated failures
When EastMoney returns no data for a symbol (delisted, doesn't exist, API
error), we don't want to keep hammering the API. NegativeCache marks the
(symbol, timeframe) pair with a timestamp. Future fetches within the TTL
window (7 days) skip the HTTP call entirely.

## The provider hierarchy

```
CachedProvider<R: DataProvider, C: DataProvider + NegativeCache + DataWriter>
    │
    ├── reader: EastMoneyProvider     ← remote HTTP (online)
    └── cache:  DuckDbProvider        ← local SQL (cache + negative cache + write)
```

The GUI always uses `CachedProvider`. The CLI uses `EastMoneyProvider` and
`DuckDbProvider` directly — it doesn't need the caching layer because it's
explicitly managing the download/storage lifecycle.

## CachedProvider: the read-through pattern

`CachedProvider` is the heart of the GUI data pipeline. It sits between the
worker thread and the actual data sources, implementing a three-tier access
strategy:

```
fetch_bars(symbol, timeframe, start, end)
    │
    ├─ 1. NEGATIVE CACHE CHECK
    │     cache.is_no_data(symbol, timeframe, now, TTL_7DAYS)?
    │     → If true: return DataError::NoData (no HTTP call)
    │     → If false: continue
    │
    ├─ 2. INFLIGHT DEDUP
    │     inflight.contains(symbol, timeframe)?
    │     → If true: return NoData (duplicate request, caller retries)
    │     → If false: insert into inflight, continue
    │
    ├─ 3. CACHE READ
    │     cache.fetch_bars(symbol, timeframe, start, end)?
    │     → If non-empty: clear inflight, return bars ✓
    │     → If empty: cache miss → fetch from remote
    │
    ├─ 4. REMOTE FETCH
    │     reader.fetch_bars(symbol, timeframe, start, end)?
    │     → If Ok(bars) and non-empty: cache.save_bars(...) → write-through
    │     → If Err(NoData): cache.mark_no_data(...) → negative cache for 7 days
    │     → If other error: propagate to caller
    │
    └─ 5. CLEANUP
          inflight.remove(symbol, timeframe)
          return result
```

Key behaviors:

- **Cache hit requires non-empty bars.** An empty result from DuckDB means
  "not cached" — we never short-circuit with zero bars back to the caller.
- **Empty results from remote are not cached or marked no-data.** An empty
  OK response is ambiguous (could be a data gap, could be wrong dates). We
  return the empty vec but don't persist it.
- **save_bars uses overwrite=true** inside CachedProvider. Since the cache
  just missed, we know there's nothing to merge — we can safely replace.

## EastMoneyProvider — the online source

EastMoney (东方财富) is China's largest financial data platform. Their public
HTTP APIs provide free access to A-share K-line data without API keys or
authentication.

### Endpoints

| Purpose | Base URL | Path |
|---|---|---|
| K-line (OHLCV) | `push2his.eastmoney.com` | `/api/qt/stock/kline/get` |
| Symbol listing | `push2delay.eastmoney.com` | `/api/qt/clist/get` |
| Real-time quotes | `push2delay.eastmoney.com` | `/api/qt/stock/get` |

### Fetching K-line data

A typical request for 贵州茅台 (600519) daily bars:

```
GET /api/qt/stock/kline/get
  ?secid=1.600519
  &klt=101
  &fqt=1
  &beg=20250101
  &end=20250721
  &lmt=2000
  &fields1=f1,f2,f3,f4,f5,f6
  &fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61
```

Parameters explained:

| Param | Value | Meaning |
|---|---|---|
| `secid` | `1.600519` | Market code (1=SH) + stock code. See `to_secid()` in symbols.md. |
| `klt` | `101` | K-line type. 101=daily, 102=weekly, 103=monthly. See timeframe mapping. |
| `fqt` | `1` | Adjustment mode. Always 1 (前复权 / forward-adjusted). |
| `beg` | `20250101` | Start date. |
| `end` | `20250721` | End date. |
| `lmt` | `2000` | Max bars per request. Hardcoded — the API caps at 2000. |

The response is JSON with klines as comma-separated strings under `data.klines[]`:

```json
{
  "data": {
    "klines": [
      "2025-07-21,12.04,12.01,12.11,11.95,1079027,13053456.00,1.25,1.22,1.25,0.00,..."
    ]
  }
}
```

Field mapping within each kline string:

| Index | Field | Description |
|---|---|---|
| 0 | date | Trade date (YYYY-MM-DD) |
| 1 | open | Opening price |
| 2 | close | Closing price |
| 3 | high | Highest price |
| 4 | low | Lowest price |
| 5 | volume | Trading volume (手, lots) |
| 6 | amount | Trading amount (元, yuan) |
| ... | ... | Additional fields (change%, turnover rate, etc.) — ignored |

Note: indices [1-5] map to `open, close, high, low, volume` — **close comes
before high**, which is EastMoney's convention, not ours. The provider
reorders them into the standard OHLCV shape.

### Symbol search and enumeration

`search_symbols` does a fuzzy keyword search. It's best-effort — parse errors
or network failures return an empty list rather than crashing the GUI.

`search_all_symbols` paginates through every A-share stock using a monolithic
filter string:

```
fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048
```

This covers all major A-share market segments (SH main board, SZ main board,
ChiNext, STAR, B-shares). The API returns 100 symbols per page, with
`data.total` for pagination tracking.

### Stock basic info

`fetch_stock_basic` retrieves a single stock's metadata: name (中文名称),
industry classification, market segment, and listing date. This data is used
to populate the `stock_basic` table in DuckDB and the `stock_basic.parquet`
file.

### Real-time quotes

`fetch_realtime_quote` returns live data: P/E ratio, P/B ratio, total shares,
float shares, and daily price limits (涨停/跌停). This is supplementary — the
GUI could show these as an info panel alongside the chart.

### Rate limiting and resilience

- The API has no documented rate limit but throttles aggressively.
  **Concurrency=2, delay=1s between requests** has been stable.
- Timeout: 10 seconds per request (configurable).
- Retry: up to 3 attempts with exponential backoff for transient failures.
- EastMoney sometimes returns `{"data": null}` for valid stocks — this is
  indistinguishable from genuine no-data. The provider treats it as
  `DataError::NoData`.

## DuckDbProvider — local cache and staging

DuckDbProvider is the Swiss Army knife of data providers. It implements all
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

ParquetReader reads from the Parquet files produced by `compass-data import`
and `merge`. It implements `DataProvider` but NOT `DataWriter` — the Parquet
store is append-only (new symbols are added as new files, existing data is
never modified in-place).

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
| GUI (daily use, cached data available) | DuckDbProvider (cache) |
| GUI (first view, no cache) | CachedProvider → EastMoney → DuckDbProvider |
| CLI: bulk data querying | ParquetReader |
| CLI: incremental download | DuckDbProvider (staging) |
| Test environment | DuckDbProvider (in-memory) |

## Dolt import — the bulk data pipeline

Dolt is a MySQL-compatible database with Git-like versioning. The
`investment_data` Dolt database contains the complete history of A-share
EOD prices — 18.25 million rows across 6,122 stocks, from 1990 to present.

The import pipeline (`compass-data import`) works as follows:

```
dolt sql -r csv -q "SELECT DISTINCT symbol FROM final_a_stock_eod_price"
    │
    ├─ Produces list of 6122 symbols (e.g. "SZ000001", "SH600519")
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

## Provider priority chain

When data is available from multiple sources, this is the priority order:

```
1. Dolt investment_data (local, 18M+ rows, 1990–2026)
     └─ Imported via: compass-data import
     └─ Stored as: parquet_data/stock_daily/*.parquet
     └─ Queried via: ParquetReader
     │
     ▼ (fallback if not imported)
2. EastMoney API (online, real-time)
     └─ Downloaded via: compass-data download, or CachedProvider cache miss
     └─ Cached in: DuckDB stock_daily table
     └─ Queried via: DuckDbProvider
```

In the GUI, CachedProvider automatically follows this chain:
- DuckDB cache hit → return cached bars
- DuckDB cache miss → fetch from EastMoney → write to DuckDB → return bars

If you've run `compass-data import`, the Parquet data should already be in
DuckDB (via `compass-data export`), so the first cache-hit path will find it.

## Configuring providers

Provider configuration flows from `~/.config/compass/config.toml`:

```toml
[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10           # HTTP request timeout
retry_count = 3             # retry attempts on transient failure

[database]
path = "compass.db"         # DuckDB cache file location
```

For the CLI, most settings are command-line arguments (see `compass-data --help`):
- `--base-url`, `--realtime-url`: override EastMoney endpoints
- `--concurrency`, `--delay-ms`: control download rate
- `--db`: staging DuckDB path
