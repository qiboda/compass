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

## Data pipeline

```
UI (CompassApp)
  └─ mpsc::Sender<Cmd>
       └─ Worker thread (tokio runtime)
            └─ CachedProvider<R: DataProvider, W: DataWriter>
                 ├─ 1. SqliteProvider::fetch_bars   (cache read)
                 ├─ 2. EastMoneyProvider::fetch_bars (HTTP, cache miss)
                 └─ 3. SqliteProvider::save_bars     (write-through)
```

## CachedProvider

Read-through cache composition (`src/data/mod.rs`):

```
CachedProvider { reader: EastMoneyProvider, cache: SqliteProvider, writer: SqliteProvider }
    │
    ├── fetch_bars() → 1. cache.fetch_bars()  → if hit AND non-empty, return
    │                  2. reader.fetch_bars()  → call EastMoney HTTP API
    │                  3. writer.save_bars()   → write to SQLite
    │                  4. return bars
```

Key: cache hit requires bars to be **non-empty**. An empty result from SQLite
means "not cached" — we do NOT return zero bars to the caller.

## Source layout

```
src/
├── main.rs              # eframe bootstrap, worker thread spawn, logging, config
├── model.rs             # Cmd enum, CompassState, AppConfig, SymbolInfo
└── data/
    ├── mod.rs           # CachedProvider<R,W>
    ├── provider.rs      # DataProvider + DataWriter traits, DataError enum
    ├── sqlite.rs        # SqliteProvider (cache read + write-through)
    ├── eastmoney.rs     # EastMoneyProvider (HTTP fetch + symbol search)
    └── synthetic.rs     # SyntheticProvider (dead code, kept for future demo use)
```

## SQLite schema

Single `bars` table keyed by `(symbol, timeframe, adj_type, timestamp)`:

```sql
CREATE TABLE IF NOT EXISTS bars (
    symbol      TEXT NOT NULL,
    timeframe   TEXT NOT NULL,
    timestamp   INTEGER NOT NULL,   -- unix epoch
    open        REAL,
    high        REAL,
    low         REAL,
    close       REAL,
    volume      REAL,
    adj_type    TEXT NOT NULL DEFAULT 'none',  -- none, qfq, hfq
    adj_factor  REAL,
    status      TEXT NOT NULL DEFAULT 'normal',
    PRIMARY KEY (symbol, timeframe, adj_type, timestamp)
);
CREATE INDEX IF NOT EXISTS idx_bars_lookup
    ON bars(symbol, timeframe, adj_type, timestamp DESC);
```

### Status values

| Status | Trigger |
|---|---|
| `normal` | Regular trading |
| `suspended` | Stock suspended |
| `limit_up` | Hit daily price ceiling |
| `limit_down` | Hit daily price floor |
| `halted` | Trading halted |

### Adj types

| Type | Description |
|---|---|
| `none` | No adjustment (raw prices) |
| `qfq` | 前复权 (forward-adjusted) |
| `hfq` | 后复权 (backward-adjusted) |

## Commands (UI → Worker)

```rust
enum Cmd {
    FetchBars { symbol, timeframe, range_start, range_end },
    SearchSymbols { query },  // not yet wired in UI
}
```

Lightweight structs only. Data flows back through `CompassState`, not through
the channel response.

## Libraries

| # | Decision | Choice | Rationale |
|---|---|---|---|
| 1 | GUI | egui 0.33 + eframe 0.33 | Pure-Rust immediate-mode GUI |
| 2 | Chart widget | egui-charts 0.2 | Candlestick chart support |
| 3 | Async runtime | tokio (rt-multi-thread) | reqwest requires tokio |
| 4 | HTTP client | reqwest 0.12 (rustls-tls) | No openssl; 10s timeout; retry |
| 5 | Database | rusqlite 0.32 (bundled) | Sync; bundled = no system lib needed |
| 6 | SQLite bridge | tokio::task::spawn_blocking | rusqlite is sync |
| 7 | Serialization | serde + serde_json | EastMoney API returns JSON |
| 8 | Time | chrono 0.4 (serde) | UTC timestamps; JSON parse |
| 9 | Data errors | thiserror 2 | Precise error enum |
| 10 | App errors | anyhow 1 | Context wrapping |
| 11 | Logging | tracing + subscriber + appender | Structured, daily rolling files |
| 12 | Async trait | async-trait 0.1 | Native async trait not stable |
| 13 | Config | toml → ~/.config/compass/config.toml | Fallback defaults |
| 14 | Threading | Main=egui, Worker=tokio | Non-async eframe |
| 15 | State sharing | Arc<Mutex<CompassState>> | Not RefCell; Mutex > RwLock here |
| 16 | Commands | std::sync::mpsc | Lightweight, data via state |
| 17 | Data abstraction | DataProvider + DataWriter traits | SQLite-first, fallback, auto-cache |
