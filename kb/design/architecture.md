# Architecture

## What is Compass?

Compass is a **local-first A-share stock chart application**. Unlike web-based
stock viewers that depend on a remote server for every interaction, Compass
downloads and caches all OHLCV data locally. Once data is imported, chart
rendering is instant — no network calls, no rate limiting, no API keys.

It has two faces:

| Face | Binary | Purpose |
|---|---|---|
| **Chart app** | `compass` | Interactive candlestick chart with symbol search, timeframe selection, crosshair, zoom, pan. Runs as a native desktop window via egui. |
| **Data pipeline** | `compass-data` | Offline data management — download from EastMoney, import from Dolt, merge staging into production, export to other formats. |

Both share the same library crate (`compass-core`), which defines the data
model, provider traits, and all I/O logic.

## How the crates fit together

```
compass (GUI binary)
  │
  ├── compass-core (library)
  │     ├── model.rs      ─ shared types: Cmd, CompassState, AppConfig, Bar
  │     ├── data/mod.rs   ─ CachedProvider (read-through cache + negative cache)
  │     ├── data/provider.rs ─ DataProvider, DataWriter, NegativeCache traits
  │     ├── data/duckdb.rs   ─ DuckDbProvider (local cache)
  │     ├── data/eastmoney.rs ─ EastMoneyProvider (HTTP API)
  │     ├── data/parquet.rs   ─ ParquetReader (main database)
  │     ├── data/symbol.rs    ─ Exchange inference, code conversion
  │     └── data/synthetic.rs ─ Test data generator
  │
  └── compass-data (CLI binary)
        └── download / import / merge / export subcommands
```

`compass-core` contains zero UI code. It provides traits and implementations
for fetching, storing, and querying stock data. The GUI and CLI are thin
orchestrators that wire up providers and dispatch work.

## The threading model: why two threads?

The core architectural challenge: **egui runs synchronously on the main thread,
but all data I/O (HTTP, DuckDB) requires async tokio.** If we block the main
thread on I/O, the UI freezes. If we use async on the main thread, egui breaks.

The solution: separate the UI thread from the I/O thread.

```
┌──────────────────────────┐     mpsc::channel      ┌──────────────────────────┐
│   MAIN THREAD (eframe)    │ ──── Cmd::Fetch ─────► │   WORKER THREAD           │
│                            │                        │   tokio Runtime           │
│  eframe::App::update()    │                        │                            │
│    lock state (read)      │ ◄── Arc<Mutex<State>> ─ │   loop {                   │
│    read bars               │     (shared state)      │     cmd = rx.recv()       │
│    draw chart              │                        │     bars = fetch()        │
│    send Cmd on click       │                        │     update state           │
│                            │   ctx.request_repaint()│     ctx.request_repaint()  │
└──────────────────────────┘                        └──────────────────────────┘
```

Key design decisions:

- **mpsc channel** for commands: lightweight, unidirectional. The UI sends
  `Cmd::FetchBars`, the worker processes it and writes results to shared state.
  Data never flows back through the channel — no need for oneshot replies.

- **Arc<Mutex<CompassState>>** for shared state: both threads read/write the
  same struct. Mutex was chosen over RwLock because the critical sections are
  tiny (copying bars, setting a loading flag) — RwLock's overhead isn't
  justified here.

- **No RefCell**: `RefCell` is `!Send`, so it can't cross thread boundaries.
  Mutex is the only viable interior-mutability primitive for this use case.

- **request_repaint()**: after every state update, the worker tells egui to
  redraw. The main thread checks `bars_version` on each frame — if it changed,
  it rebuilds the chart data from shared state.

- **spawn_blocking for DuckDB**: DuckDB's C API is synchronous. All DuckDB
  queries run inside `tokio::task::spawn_blocking`, which moves the blocking
  work to a dedicated thread pool. This keeps the tokio runtime responsive
  for other async tasks (HTTP fetches, timers).

The worker thread owns the tokio runtime. It's created with `std::thread::spawn`,
then immediately calls `rt.block_on(async { ... })` to enter an async event
loop. This is a common pattern when the outer application framework (eframe) is
not async-aware.

## Data pipeline: from user click to chart

When you type `600519`, select `1d`, and click "Fetch", here's what happens:

```
UI (CompassApp::update)
  │  user clicks "Fetch"
  │  cmd_tx.send(Cmd::FetchBars { symbol: "600519", timeframe: "1d", ... })
  │
  ▼
Worker thread (start_worker_thread)
  │  cmd_rx.recv() → Cmd::FetchBars
  │  state.loading = true
  │
  ▼
CachedProvider::fetch_bars("600519", "1d", start, end)
  │
  ├─ 1. Check negative cache → is this symbol known to have no data? (TTL 7d)
  │     If yes → return DataError::NoData immediately (no HTTP call)
  │
  ├─ 2. Check inflight dedup → is another fetch for this same symbol already running?
  │     If yes → return NoData (caller will retry or ignore)
  │
  ├─ 3. Try DuckDB cache → SELECT FROM stock_daily WHERE symbol='600519'
  │     If non-empty result → cache hit! Return bars. No remote call.
  │
  ├─ 4. Cache miss → EastMoneyProvider::fetch_bars()
  │     HTTP GET to push2his.eastmoney.com → parse JSON → Vec<Bar>
  │
  ├─ 5. If successful → DuckDbProvider::save_bars() (write-through to cache)
  │     If NoData → mark negative cache (skip this symbol for 7 days)
  │
  └─ 6. Clear inflight, return bars to worker
  │
  ▼
Worker updates CompassState
  │  state.set_bars("600519", "1d", bars)
  │  bars_version++  (UI detects this change next frame)
  │  state.loading = false
  │  ctx.request_repaint()
  │
  ▼
UI (next frame)
  │  detects bars_version changed
  │  rebuilds BarData from state.bars
  │  draws candlestick chart
```

### Why read-through cache?

The CachedProvider pattern is "check local first, fetch remote on miss, write
back to local." This gives three benefits:

1. **Instant replay**: after the first fetch, repeated views of the same stock
   load from DuckDB — no network, no rate limiting.
2. **Offline capability**: once cached, you can view charts without internet.
3. **API respect**: fewer HTTP calls to EastMoney means fewer chances of being
   rate-limited or blocked.

### Why negative cache?

EastMoney returns `{"data": null}` for stocks that don't exist or delisted
stocks. Without negative caching, every "Fetch" click would hit the API, get
null, and waste a request. With negative caching (TTL 7 days), the first miss
marks the symbol; subsequent fetches within 7 days return NoData instantly.

### Why inflight dedup?

If the user clicks "Fetch" twice before the first request completes, two
identical HTTP calls would race. Inflight dedup tracks which
(symbol, timeframe) pairs currently have an active request. The second click
returns NoData instead of starting a duplicate fetch.

## Data pipeline: CLI (compass-data)

The CLI manages data offline, before the GUI ever runs. It has four subcommands
that form a pipeline:

```
EastMoney API ──download──► staging.duckdb ──merge──► parquet_data/ ──export──► compass.duckdb
Dolt DB ───────import─────► parquet_data/
```

### download: EastMoney → staging
- Enumerates all A-share symbols via EastMoney search API
- Fetches stock basic info (name, industry, list date)
- Detects gaps (compares existing data range with requested range)
- Downloads OHLCV bars in chunks (max 2000 bars per request)
- Rate-limited: configurable concurrency (default 2) and delay between requests (default 1s)
- Writes to staging DuckDB via INSERT OR IGNORE (skip duplicates by default)

### import: Dolt → Parquet
- Queries Dolt `investment_data` database via `dolt sql -r parquet`
- Extracts 6000+ stocks from `final_a_stock_eod_price` table (18M+ rows)
- Strips exchange prefixes (SZ000001 → 000001)
- Writes Parquet bytes directly — no CSV or DuckDB intermediary
- One Parquet file per Dolt symbol: `parquet_data/stock_daily/SZ000001.parquet`
- Merge mode (default): uses DuckDB `read_parquet` to merge existing + new
- Overwrite mode: bytes written directly to target file

### merge: staging → Parquet
- Lists symbols in staging DuckDB not yet in Parquet
- For each new symbol: COPY staging → Parquet file
- Incremental: only moves data for symbols that don't already exist

### export: Parquet → other formats
- Reads parquet_data/ directory
- Exports to DuckDB, CSV, or parquet-dir format
- Used to create the final database the GUI reads from

**Default behavior everywhere**: merge/skip. Existing data is preserved; only
new data is added. Pass `--overwrite` to replace. This migration-style behavior
prevents accidental data loss.

## Storage strategy: why both DuckDB and Parquet?

```
Compass uses two database formats for different purposes:

  Parquet files (parquet_data/)
    ├─ Source of truth — the canonical data store
    ├─ Stock basic: stock_basic.parquet (one file for all symbols)
    └─ Stock daily: stock_daily/{symbol}.parquet (one file per symbol)

  DuckDB (compass.db / staging.duckdb)
    ├─ GUI cache — stores fetched bars for instant replay
    ├─ CLI staging — temporary buffer during download
    └─ Negative cache — tracks no-data symbols with TTL
```

### Why Parquet as source of truth?

- **Columnar**: DuckDB queries only read the columns they need (e.g., `SELECT
  close` reads only the close column). Much faster than row-based formats for
  analytical queries across thousands of bars.
- **Partitioned by symbol**: each stock is one file. Adding a new stock is a
  new file — no table rebuilds. Deleting is `rm`.
- **Queryable**: DuckDB's `read_parquet()` function lets us query Parquet files
  directly with SQL, without loading them into tables.
- **Portable**: Parquet is an open standard. You can open it with Python
  (pandas, polars), R, or any DuckDB instance. No vendor lock-in.
- **Compact**: columnar compression reduces storage. 6000+ stocks × 30 years ≈
  manageable disk footprint.

### Why DuckDB for caching and staging?

- **Write-friendly**: INSERT OR REPLACE/IGNORE semantics; automatic primary key
  conflict handling. Parquet is append-only and harder to update.
- **In-memory mode**: tests use `DuckDbProvider::new_in_memory()` for fully
  isolated databases with zero cleanup.
- **Bundled**: the `duckdb` crate bundles the C library — no system dependency.
- **OLAP-optimized**: DuckDB is built for analytical workloads (aggregations,
  window functions, time-series queries), which maps perfectly to stock data.

### The read path

The GUI reads from Parquet via `ParquetReader` when available (after import),
and uses DuckDB as a read-through cache. The CLI writes to DuckDB staging first,
then merges into Parquet. This two-tier design separates the concerns of "fast
writes and caching" (DuckDB) from "durable, queryable storage" (Parquet).

## Symbol convention: why bare 6-digit codes?

Every stock in Compass is identified by a bare 6-digit code: `"000001"`,
`"600519"`, `"836149"`. No exchange suffix, no prefix.

### Why not ts_code format?

The older format `"000001.SZ"` mixes identity (the code) with metadata (the
exchange). This causes problems:

- A single stock can have different formats depending on context
- Parsing requires splitting on `.` and handling edge cases
- Exchange can be **inferred from the code itself** — the suffix is redundant

We retired `ts_code` from the schema. The `to_ts_code()` function still exists
for backward compatibility but is no longer used as a primary key.

### Exchange inference

Since codes are unique across exchanges in practice, we use heuristic rules:

| Code starts with | Exchange |
|---|---|
| `6` | Shanghai (SH) |
| `8` | Beijing (BJ) |
| Anything else | Shenzhen (SZ) |

For the rare ambiguous cases (e.g., `000001` could be 平安银行 SZ or 上证指数 SH),
use explicit prefixes: `sh.000001` for the index, plain `000001` for the stock
(which defaults to SZ — stocks are the common case).

See `kb/design/symbols.md` for the complete market segment breakdown and
EastMoney secid mapping.

## Config system

Compass loads config from `~/.config/compass/config.toml` on startup. All
fields are optional — missing keys fall back to sensible defaults defined in
`AppConfig::default()`.

```toml
[database]
path = "compass.db"           # where to store the cache

[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10
retry_count = 3

[app]
default_symbol = "000001"     # what to show on startup
default_timeframe = "1d"
```

The config path is `$HOME/.config/compass/config.toml`. If the file doesn't
exist or can't be parsed, the app starts with all defaults — no manual setup
required.

## Logging

Logs go to **two sinks** simultaneously:

1. **stderr** — always; level controlled by `RUST_LOG` env var
2. **`logs/compass.log`** — daily rolling file with ANSI stripped

```sh
RUST_LOG=debug cargo run    # verbose: see every HTTP request, DuckDB query
RUST_LOG=info cargo run     # normal: state transitions, fetch counts, errors
RUST_LOG=warn cargo run     # quiet: only problems
```

The file appender uses `tracing-appender`'s daily rotation — each day gets a
new file (`compass.log.2025-07-23`), and the current day is always
`compass.log`.

## Library decisions

Every library choice in Compass was deliberate. Here's why each one was chosen:

| # | Decision | Choice | Why |
|---|---|---|---|
| 1 | GUI framework | egui 0.33 + eframe | Pure-Rust immediate-mode GUI. No HTML/CSS/JS, no webview dependency. Compiles to a single native binary. |
| 2 | Chart widget | egui-charts 0.2 | Candlestick chart with built-in pan, zoom, crosshair. Matches the egui ecosystem. |
| 3 | Async runtime | tokio (rt-multi-thread) | reqwest requires tokio. Multi-thread runtime lets the worker handle concurrent fetches. CLI uses current_thread for simplicity. |
| 4 | HTTP client | reqwest 0.12 (rustls-tls) | No OpenSSL dependency (rustls). Configurable timeout + retry. JSON deserialization built in. |
| 5 | Database | duckdb 1.0 (bundled) | OLAP-optimized columnar engine. Reads/writes Parquet natively. The `bundled` feature ships the C library — no system duckdb required. |
| 6 | DB threading | spawn_blocking + Arc<Mutex<>> | DuckDB is synchronous C. `spawn_blocking` moves queries to a thread pool so they don't block the async runtime. Mutex ensures exclusive connection access. |
| 7 | Serialization | serde + serde_json | EastMoney API returns JSON. serde derives on all data types. |
| 8 | Time handling | chrono 0.4 | UTC timestamps, date arithmetic (range_start/end calculation), JSON parse support. |
| 9 | Error types | thiserror 2 (library), anyhow 1 (binaries) | Precise `DataError` enum with `From` impls for `?` propagation in the library. `anyhow` for context-wrapping in binary entry points. |
| 10 | Logging | tracing + subscriber + appender | Structured, async, level-filtered. Daily rolling files via tracing-appender. |
| 11 | Async traits | async-trait 0.1 | Native async traits in Rust are still unstable. This macro is the standard workaround. |
| 12 | Config | toml → Deserialize | Simple, readable format. `#[serde(default)]` on every field means partial configs work. |
| 13 | CLI args | clap 4 (derive) | Derive macro generates the CLI parser from a struct. Type-safe, self-documenting. |
| 14 | Progress bars | indicatif 0.17 | Spinner + progress bar for long-running CLI operations (download, import). |
| 15 | Concurrency | futures Semaphore + buffer_unordered | Bounded parallelism for CLI download. Semaphore caps concurrent requests; buffer_unordered preserves order while processing results as they arrive. |
| 16 | State sharing | Arc<Mutex<CompassState>> | Shared between UI and worker threads. Mutex chosen over RwLock — critical sections are short, no write starvation risk. |
| 17 | Command channel | std::sync::mpsc | Simple, well-understood. Lightweight commands flow one way; results flow back through shared state, not the channel. |
| 18 | Provider traits | DataProvider + DataWriter + NegativeCache | Trait-based abstraction lets us swap backends: DuckDB, EastMoney, Parquet — all behind the same interface. Testable with mock implementations. |
| 19 | Parquet storage | DuckDB read_parquet + COPY TO | Columnar format partitioned by symbol. Queryable without loading into tables. |
| 20 | Dolt import | dolt CLI → Parquet (direct) | Offline bulk import of 18M+ rows. Dolt `sql -r parquet` writes binary Parquet directly, skipping the CSV intermediary. |

## Where to go next

- **Data providers**: `kb/design/data-providers.md` — the trait system and each
  provider implementation in depth
- **Symbols**: `kb/design/symbols.md` — market segments, code conversion,
  timeframe mapping
- **API reference**: `cargo doc --open` — full type-level documentation for
  all public APIs
