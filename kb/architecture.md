# Architecture

## Threading model

Two-thread design eliminates blocking issues between UI and I/O.

```
┌─────────────────────┐     std::sync::mpsc      ┌───────────────────────┐
│   MAIN THREAD (egui) │ ──── Cmd::Fetch ──────► │   WORKER THREAD       │
│                      │                          │   tokio Runtime       │
│  eframe::App::ui()   │                          │                       │
│    lock state        │ ◄─── Arc<Mutex> ──────── │   loop {              │
│    read bars         │     (shared state)       │     cmd = rx.recv()   │
│    draw chart        │                          │     bars = fetch()    │
│    send Cmd on click │                          │     update state      │
│                      │     ctx.request_repaint()│     ctx.request_repaint()
└─────────────────────┘                          └───────────────────────┘
```

- Main thread: eframe event loop, owns `CompassApp` widget. Polls `CompassState`
  via `Arc<Mutex<>>` every frame, rebuilds chart data when `bars_version` changes.
- Worker thread: `std::thread::spawn` → `tokio::runtime::Runtime::new()`
  → `block_on` loop that receives `Cmd` via `mpsc::channel`.
- State sharing: `Arc<Mutex<CompassState>>`. NOT `RefCell` (not `Send`).
  Mutex chosen over `RwLock` — no write starvation risk here.
- All DuckDB I/O goes through `tokio::task::spawn_blocking` since DuckDB
  operations are synchronous. The connection is wrapped in `Arc<Mutex<Connection>>`.

## Data pipeline (GUI)

```
UI (CompassApp)
  └─ mpsc::Sender<Cmd>
       └─ Worker thread (tokio runtime)
            └─ CachedProvider<R: DataProvider, C: DataProvider+NegativeCache+DataWriter>
                 ├─ 1. DuckDbProvider::fetch_bars      (cache read from stock_daily)
                 ├─ 2. EastMoneyProvider::fetch_bars    (HTTP, cache miss)
                 └─ 3. DuckDbProvider::save_bars        (write-through to stock_daily)
```

## Data pipeline (CLI — compass-data)

```
compass-data download    EastMoney API → staging.duckdb (staging)
compass-data import      Dolt investment_data → parquet_data/ (main DB)
compass-data merge       staging.duckdb → parquet_data/ (incremental merge)
compass-data export      parquet_data/ → duckdb/csv (format conversion)
```

### Download pipeline

```
tokio runtime (current_thread)
  ├─ 1. EastMoneyProvider::search_all_symbols  → enumerate stocks
  ├─ 2. EastMoneyProvider::fetch_stock_basic   → stock_basic table
  ├─ 3. DuckDbProvider::get_stored_range        → gap detection
  ├─ 4. EastMoneyProvider::fetch_bars            → OHLCV chunks
  └─ 5. DuckDbProvider::save_stock_daily         → staging.duckdb
```

### Import pipeline (Dolt → Parquet)

```
dolt CLI (CSV export)
  ├─ 1. SELECT DISTINCT symbol → symbol list
  ├─ 2. Per-symbol: SELECT → CSV → DuckDB → Parquet
  └─ 3. Stock basic → stock_basic.parquet
```

### Merge pipeline (staging → Parquet)

```
DuckDB staging
  ├─ 1. List symbols in staging DuckDB
  ├─ 2. For each symbol NOT already in Parquet:
  └─ 3. COPY staging → parquet_data/stock_daily/{symbol}.parquet
```

## CachedProvider

Read-through cache composition (`crates/compass-core/src/data/mod.rs`):

```
CachedProvider { reader: EastMoneyProvider, cache: DuckDbProvider }
    │
    ├── fetch_bars() → 1. cache.fetch_bars()       → if hit AND non-empty, return
    │                  2. reader.fetch_bars()       → call EastMoney HTTP API
    │                  3. cache.save_bars()         → write to DuckDB (stock_daily)
    │                  4. return bars
    ├── NegativeCache  → marks (symbol, timeframe) pairs as no-data (TTL 7d)
    └── Inflight dedup → prevents duplicate concurrent fetches
```

Cache hit requires bars to be **non-empty**. An empty result from DuckDB
means "not cached" — we do NOT return zero bars to the caller.

## Source layout

```
crates/
├── compass-core/               # Library (compass-core)
│   ├── src/
│   │   ├── lib.rs              # pub mod data; pub mod model;
│   │   ├── model.rs            # Cmd, CompassState, AppConfig, SymbolInfo, RealtimeQuote, AdjFactor, StockBasic
│   │   └── data/
│   │       ├── mod.rs          # CachedProvider<R, C> (read-through cache)
│   │       ├── provider.rs     # DataProvider + DataWriter + NegativeCache traits, DataError
│   │       ├── duckdb.rs       # DuckDbProvider (4-table schema)
│   │       ├── eastmoney.rs    # EastMoneyProvider (HTTP fetch, search, realtime)
│   │       ├── parquet.rs      # ParquetReader (DuckDB read_parquet)
│   │       ├── symbol.rs       # to_exchange(), to_ts_code()
│   │       └── synthetic.rs    # Synthetic test data generator
│   └── tests/
│       └── integration_test.rs
├── compass/                    # GUI binary
│   └── src/main.rs             # eframe bootstrap, worker thread, logging, config
└── compass-data/               # CLI binary
    └── src/
        ├── main.rs             # clap subcommand dispatch
        ├── download.rs         # download: EastMoney → staging DuckDB
        ├── import_dolt.rs      # import: Dolt CSV → Parquet
        ├── merge.rs            # merge: staging DuckDB → Parquet (incremental)
        ├── export.rs           # export: Parquet → DuckDB / other formats
        ├── baostock.rs         # Baostock Python subprocess for adj_factor
        ├── chunk.rs            # Date range chunk splitting (max 2000 days)
        ├── progress.rs         # indicatif progress bar
        └── retry.rs            # fetch_with_retry (exponential backoff)
Cargo.toml                      # workspace root — shared dependencies
```

## DuckDB schema (staging, 4 tables + negative cache)

Core OHLCV table — keyed by `(symbol, trade_date)`:

```sql
CREATE TABLE IF NOT EXISTS stock_daily (
    symbol      VARCHAR NOT NULL,      -- 6-digit stock code: '000001', '600519'
    trade_date  DATE NOT NULL,
    open        DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    close       DOUBLE,
    adjclose    DOUBLE,                -- adjustment close from Dolt
    volume      DOUBLE,                -- 成交量 (手)
    amount      DOUBLE,                -- 成交额 (元)
    PRIMARY KEY (symbol, trade_date)
);
```

Adjustment factors:

```sql
CREATE TABLE IF NOT EXISTS stock_adj_factor (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    adj_factor  DOUBLE NOT NULL,
    PRIMARY KEY (symbol, trade_date)
);
```

Stock basic info (name, exchange, listing date):

```sql
CREATE TABLE IF NOT EXISTS stock_basic (
    symbol      VARCHAR PRIMARY KEY,
    name        VARCHAR,
    area        VARCHAR,
    industry    VARCHAR,
    market      VARCHAR,
    exchange    VARCHAR,
    list_date   DATE,
    delist_date DATE
);
```

Price limits (涨停/跌停):

```sql
CREATE TABLE IF NOT EXISTS stock_limit (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    up_limit    DOUBLE,
    down_limit  DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);
```

Negative cache (TTL-managed no-data marks):

```sql
CREATE TABLE IF NOT EXISTS no_data_marks (
    symbol TEXT NOT NULL, timeframe TEXT NOT NULL,
    last_checked BIGINT NOT NULL,
    PRIMARY KEY (symbol, timeframe)
);
```

## Parquet schema (main database)

```
parquet_data/
├── stock_basic.parquet        # symbol, name, exchange, list_date, delist_date
└── stock_daily/
    ├── 000001.parquet         # One file per symbol
    ├── 600519.parquet         # Columns: tradedate, open, high, low, close, adjclose, volume, amount
    └── ...
```

Parquet files are the source of truth. DuckDB staging is a temporary buffer
for EastMoney downloads before merging into Parquet.

## symbol convention

- Primary key is `symbol` — a 6-digit bare code: `"000001"`, `"600519"`, `"836149"`
- Exchange is inferred from code ranges (`to_exchange()` in `crates/compass-core/src/data/symbol.rs`):
  - `6xxxxx` → SH, `000xxx–004xxx` → SZ, `300xxx` → SZ, `8xxxxx` → BJ
- `ts_code` format (`"000001.SZ"`) has been retired; the `to_ts_code()` helper
  still exists for backward compatibility but is no longer used as a primary key

## Commands (UI → Worker)

```rust
enum Cmd {
    FetchBars { symbol, timeframe, range_start, range_end },
    SearchSymbols { query },
}
```

Lightweight structs only. Data flows back through `CompassState`, not through
the channel response.

## Libraries

| # | Decision | Choice | Rationale |
|---|---|---|---|
| 1 | GUI | egui 0.33 + eframe 0.33 | Pure-Rust immediate-mode GUI |
| 2 | Chart widget | egui-charts 0.2 | Candlestick chart support |
| 3 | Async runtime | tokio (rt-multi-thread for GUI, current_thread for CLI) | reqwest requires tokio |
| 4 | HTTP client | reqwest 0.12 (rustls-tls) | No openssl; 10s timeout; retry |
| 5 | Database | duckdb 1.0 (bundled + parquet) | Columnar OLAP; Parquet read/write; no system lib needed |
| 6 | DB threading | tokio::task::spawn_blocking + Arc<Mutex<>> | duckdb is sync; Mutex for exclusive access |
| 7 | Serialization | serde + serde_json | EastMoney API returns JSON |
| 8 | Time | chrono 0.4 (serde) | UTC timestamps; JSON parse |
| 9 | Data errors | thiserror 2 | Precise error enum |
| 10 | App errors | anyhow 1 | Context wrapping |
| 11 | Logging | tracing + subscriber + appender | Structured, daily rolling files |
| 12 | Async trait | async-trait 0.1 | Native async trait not stable |
| 13 | Config | toml → ~/.config/compass/config.toml | Fallback defaults |
| 14 | CLI args | clap 4 (derive) | compass-data binary |
| 15 | Progress | indicatif 0.17 | Spinner + progress bar for CLI |
| 16 | Concurrency | futures 0.3 (Semaphore + buffer_unordered) | Bounded parallelism for CLI |
| 17 | Threading | Main=egui, Worker=tokio | Non-async eframe |
| 18 | State sharing | Arc<Mutex<CompassState>> | Not RefCell; Mutex > RwLock here |
| 19 | Commands | std::sync::mpsc | Lightweight, data via state |
| 20 | Data abstraction | DataProvider + DataWriter + NegativeCache traits | DuckDB-first, fallback, auto-cache |
| 21 | Parquet storage | ParquetReader + COPY TO PARQUET | Columnar, partitioned by symbol, queryable by DuckDB |
| 22 | Dolt import | dolt CLI CSV → DuckDB → Parquet | Offline bulk import of investment_data |
