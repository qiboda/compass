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

## Data pipeline (CLI downloader)

```
compass-downloader
  └─ tokio runtime (current_thread)
       ├─ 1. EastMoneyProvider::search_all_symbols  → enumerate stocks
       ├─ 2. EastMoneyProvider::fetch_stock_basic   → stock_basic table
       ├─ 3. DuckDbProvider::get_stored_range        → gap detection
       ├─ 4. EastMoneyProvider::fetch_bars            → OHLCV chunks
       ├─ 5. DuckDbProvider::save_stock_daily         → stock_daily table
       └─ 6. Baostock (Python subprocess)             → stock_adj_factor table
```

## CachedProvider

Read-through cache composition (`src/data/mod.rs`):

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
src/
├── lib.rs                     # pub mod data; pub mod model;
├── main.rs                    # eframe bootstrap, worker thread spawn, logging, config (binary: compass)
├── model.rs                   # Cmd enum, CompassState, AppConfig, SymbolInfo, RealtimeQuote, AdjFactor
├── data/
│   ├── mod.rs                 # CachedProvider<R, C> (read-through cache composition)
│   ├── provider.rs            # DataProvider + DataWriter + NegativeCache traits, DataError enum
│   ├── duckdb.rs              # DuckDbProvider (7-table schema, cache read + write-through)
│   ├── eastmoney.rs           # EastMoneyProvider (HTTP fetch, symbol search, realtime, stock info)
│   └── symbol.rs              # to_exchange(), to_ts_code() — bare code → ts_code conversion
└── bin/
    ├── downloader/
    │   ├── main.rs            # CLI binary (compass-downloader): enumerate → download → save
    │   ├── baostock.rs        # Baostock Python subprocess for adj_factor
    │   ├── chunk.rs           # Date range chunk splitting (max 2000 days per chunk)
    │   ├── progress.rs        # indicatif MultiProgress spinner + bar
    │   └── retry.rs           # fetch_with_retry (exponential backoff: 1s/2s/4s)
    └── downloader.rs          # Legacy single-file stub (redirects to downloader/ module)
```

## DuckDB schema (7 tables + negative cache)

Core OHLCV table — keyed by `(ts_code, trade_date)`:

```sql
CREATE TABLE IF NOT EXISTS stock_daily (
    ts_code     VARCHAR NOT NULL,      -- e.g. '000001.SZ'
    trade_date  DATE NOT NULL,
    open        DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    close       DOUBLE,
    pre_close   DOUBLE,                -- computed from previous close in chunk
    change      DOUBLE,                -- 涨跌额
    pct_chg     DOUBLE,                -- 涨跌幅 (%)
    vol         DOUBLE,                -- 成交量 (手)
    amount      DOUBLE,                -- 成交额 (元)
    PRIMARY KEY (ts_code, trade_date)
);
```

Adjustment factors from Baostock:

```sql
CREATE TABLE IF NOT EXISTS stock_adj_factor (
    ts_code     VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    adj_factor  DOUBLE NOT NULL,
    PRIMARY KEY (ts_code, trade_date)
);
```

Stock basic info (name, industry, exchange, listing date):

```sql
CREATE TABLE IF NOT EXISTS stock_basic (
    ts_code     VARCHAR PRIMARY KEY,
    symbol      VARCHAR,
    name        VARCHAR,
    area        VARCHAR,
    industry    VARCHAR,
    market      VARCHAR,
    exchange    VARCHAR,
    list_date   DATE,
    delist_date DATE
);
```

Auxiliary tables:

```sql
-- Trading status (is_open per day)
CREATE TABLE IF NOT EXISTS stock_status (
    ts_code VARCHAR NOT NULL, trade_date DATE NOT NULL,
    is_open BOOLEAN DEFAULT TRUE,
    PRIMARY KEY (ts_code, trade_date)
);

-- Price limits (涨停/跌停)
CREATE TABLE IF NOT EXISTS stock_limit (
    ts_code VARCHAR NOT NULL, trade_date DATE NOT NULL,
    up_limit DOUBLE, down_limit DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
);

-- Daily indicators (PE/PB/PS/turnover)
CREATE TABLE IF NOT EXISTS daily_indicator (
    ts_code VARCHAR NOT NULL, trade_date DATE NOT NULL,
    turnover_rate DOUBLE, turnover_rate_f DOUBLE, volume_ratio DOUBLE,
    pe DOUBLE, pe_ttm DOUBLE, pb DOUBLE, ps DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
);

-- Share capital
CREATE TABLE IF NOT EXISTS stock_share (
    ts_code VARCHAR NOT NULL, trade_date DATE NOT NULL,
    total_share DOUBLE, float_share DOUBLE, free_share DOUBLE,
    total_mv DOUBLE, circ_mv DOUBLE,
    PRIMARY KEY (ts_code, trade_date)
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

## ts_code convention

- Format: `{code}.{exchange}` — e.g. `000001.SZ`, `600519.SH`, `836149.BJ`
- `to_ts_code(symbol)` in `src/data/symbol.rs` converts bare codes (with optional
  `sh.`/`sz.`/`bj.` prefixes) to `ts_code`
- `to_exchange(code)` returns `"SH"`, `"SZ"`, or `"BJ"`

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
| 5 | Database | duckdb 1.0 (bundled + parquet) | Columnar OLAP; Parquet export; no system lib needed |
| 6 | DB threading | tokio::task::spawn_blocking + Arc<Mutex<>> | duckdb is sync; Mutex for exclusive access |
| 7 | Serialization | serde + serde_json | EastMoney API returns JSON |
| 8 | Time | chrono 0.4 (serde) | UTC timestamps; JSON parse |
| 9 | Data errors | thiserror 2 | Precise error enum |
| 10 | App errors | anyhow 1 | Context wrapping |
| 11 | Logging | tracing + subscriber + appender | Structured, daily rolling files |
| 12 | Async trait | async-trait 0.1 | Native async trait not stable |
| 13 | Config | toml → ~/.config/compass/config.toml | Fallback defaults |
| 14 | CLI args | clap 4 (derive) | compass-downloader binary |
| 15 | Progress | indicatif 0.17 | Spinner + progress bar for CLI |
| 16 | Concurrency | futures 0.3 (Semaphore + buffer_unordered) | Bounded parallelism for CLI |
| 17 | Threading | Main=egui, Worker=tokio | Non-async eframe |
| 18 | State sharing | Arc<Mutex<CompassState>> | Not RefCell; Mutex > RwLock here |
| 19 | Commands | std::sync::mpsc | Lightweight, data via state |
| 20 | Data abstraction | DataProvider + DataWriter + NegativeCache traits | DuckDB-first, fallback, auto-cache |
| 21 | Adjustment | Baostock via Python subprocess | adj_factor from `query_adjust_factor` |
