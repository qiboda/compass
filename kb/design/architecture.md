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
  ├── main.rs        ─ CompassApp (eframe::App), entry point, wiring
  ├── state.rs       ─ SharedState with Dynamic<T> reactive fields
  ├── messages.rs    ─ AppMessage, FetchRequest, FetchResponse
  ├── tabs.rs        ─ Tab/TabKind/TabViewer (egui_dock bridge)
  ├── backend.rs     ─ wire_backend, BackendHandle, AsyncDispatcher wiring
  ├── dispatcher.rs  ─ register_citizens, lifecycle draining, message routing
  ├── citizens/
  │   ├── chart.rs   ─ ChartCitizen: OHLCV candlestick chart
  │   └── logger.rs  ─ LoggerPanel: scrollable log viewer
  ├── widgets/
  │   └── searchable_dropdown.rs ─ StockPicker widget, filter_stocks()
  │
  ├── compass-core (library)
  │     ├── model.rs      ─ shared types: AppConfig, Exchange, StockBasic, Bar
  │     ├── data/mod.rs   ─ Module declarations
  │     ├── data/provider.rs ─ DataProvider, DataWriter, NegativeCache traits
  │     ├── data/duckdb.rs   ─ DuckDbProvider (in-memory + Parquet-backed)
  │     ├── data/parquet.rs   ─ ParquetReader (main database)
  │     ├── data/symbol.rs    ─ Exchange inference, code conversion
  │     └── data/synthetic.rs ─ Test data generator
  │
  └── compass-data (CLI binary)
        └── import / import-compass / export / backup subcommands
```

`compass-core` contains zero UI code. It provides traits and implementations
for fetching, storing, and querying stock data. The GUI and CLI are thin
orchestrators that wire up providers and dispatch work.

The GUI binary (`compass`) uses the **egui-mobius citizen pattern** — a
reactive architecture where UI panels are modeled as `Citizen` structs with
outbox-based event dispatch, shared state is managed via `Dynamic<T>` reactive
fields, and async work is routed through `Signal`/`Slot` typed channels to an
`AsyncDispatcher` running on a dedicated tokio runtime.

## Citizen pattern architecture

The core architectural challenge: **egui runs synchronously on the main thread,
but all data I/O (HTTP, DuckDB) requires async tokio.** If we block the main
thread on I/O, the UI freezes. If we use async on the main thread, egui breaks.

The solution uses the **egui-mobius citizen pattern**, a Level 3 reactive
architecture inspired by Elm and Flux. Three layers handle the separation:

| Layer | Name | Responsibility |
|---|---|---|
| **1. Presentation** | `Citizen` panels + `egui_dock` | Render UI, emit outbox messages |
| **2. Reactive state** | `SharedState` with `Dynamic<T>` | Hold application state; auto-notify on change |
| **3. Async backend** | `Signal`/`Slot` + `AsyncDispatcher` | Execute I/O on a tokio runtime; write results back to state |

### Layer 1: Citizens and the DockArea

The UI is split into two `Citizen` panels inside an `egui_dock::DockArea`, with a
toolbar rendered above:

| Citizen | File | Role |
|---|---|---|
| **ChartCitizen** | `citizens/chart.rs` | OHLCV candlestick chart via `egui-charts`. Reads `bars` from shared state reactively and re-renders when the data changes. |
| **LoggerPanel** | `citizens/logger.rs` | Scrollable log view. Reads log entries from shared state. |

Panels are arranged inside an `egui_dock::DockArea`, giving the user a
tabbed interface they can rearrange and resize. A toolbar at the top
provides symbol search, exchange selection, and the Fetch button.

```
┌──────────────────────────────────────────────┐
│  Toolbar                                     │
│  [Symbol ▾] [Exchange ▾] [TF: 1d] [Fetch]   │
├──────────────────────────────────────────────┤
│  egui_dock::DockArea                         │
│  ┌──────────────────────────────────────────┐│
│  │  Chart (candlestick)                     ││
│  │  ┌───┬───┬───┬───┬───┬───┐              ││
│  │  │   │   │   │   │   │   │              ││
│  │  │   │   │   │   │   │   │              ││
│  │  └───┴───┴───┴───┴───┴───┘              ││
│  ├──────────────────────────────────────────┤│
│  │  Logger (scrollable)                     ││
│  └──────────────────────────────────────────┘│
└──────────────────────────────────────────────┘
```

The toolbar uses `CompassApp` local state (exchange index, stock picker)
and directly calls `dispatcher::handle()` on Fetch. It replaces the
outbox pattern previously used by ControlCitizen.

```
CompassApp::ui() each frame:
  1. Render toolbar (symbol picker, exchange combo, Fetch)
  2. Render DockArea → Chart and Logger citizens
  3. Drain citizen lifecycle messages from dispatcher
  4. request_repaint_after(200ms) for continuous update
```

### Layer 2: Reactive state with Dynamic\<T\>

State lives in `SharedState` (defined in `state.rs`), a struct where every
field is a `Dynamic<T>` from `egui_mobius_reactive`:

```rust
pub struct SharedState {
    pub symbol:    Dynamic<String>,         // current symbol
    pub timeframe: Dynamic<String>,         // current timeframe
    pub bars:      Dynamic<Vec<Bar>>,        // OHLCV bars
    pub loading:   Dynamic<bool>,            // fetch in-flight
    pub error:     Dynamic<Option<String>>,  // last error
    pub log:       Dynamic<Vec<String>>,     // log entries
}
```

`Dynamic<T>` wraps the value behind an `Arc<RwLock<T>>` and provides
`get()`, `set()`, and `subscribe()`. Multiple readers share the same
underlying storage — no separate `Arc<Mutex<CompassState>>` wrapper is
needed.

Key differences from the old `CompassState` + `Arc<Mutex<>>` approach:

- **No manual version counter**: `bars_version` is gone. The chart citizen
  compares `bars.len()` on each frame; a difference triggers data rebuild.
  The reactive runtime could also notify subscribers automatically.

- **No Mutex contention**: `Dynamic<T>` uses `RwLock` internally, but each
  field is independent. Writing `bars` doesn't lock `loading`, so reads
  from different fields never contend.

- **Clone-free reads**: citizens read via `Dynamic::get()` which returns a
  cloned value. For `Vec<Bar>` this is an O(n) clone — acceptable because
  bar counts are small (under 10k per stock). The chart only re-renders
  when the count changes.

### Layer 3: Async backend via Signal/Slot

Instead of a manual `mpsc` channel + worker thread loop, the app uses
egui-mobius's Level 3 async dispatch:

```
┌─ UI THREAD (eframe) ─────────────────────┐
│                                           │
│  ControlCitizen::show()                   │
│    user clicks Fetch                      │
│    outbox.push(AppMessage::FetchBars)     │
│         │                                 │
│         ▼                                 │
│  dispatcher::handle()                     │
│    state.loading.set(true)                │
│    work_signal.send(FetchRequest) ───┐    │
│                                     │    │
└─────────────────────────────────────│────┘
                                     │
                              ┌──────▼─────────────────────┐
                              │  AsyncDispatcher (tokio)   │
                              │                             │
                              │  attach_async(work_slot,   │
                              │    result_signal,          │
                              │    |req| async {           │
                              │      reader.fetch(req)     │
                              │      → FetchResponse       │
                              │    })                      │
                              │                             │
                              └──────┬─────────────────────┘
                                     │
                              ┌──────▼─────────────────────┐
                              │  result_slot.start()       │
                              │    |resp| {                │
                              │      state.bars.set(bars)  │
                              │      state.loading.set(false)
                              │      egui_ctx.request_repaint()
                              │    }                       │
                              └────────────────────────────┘
```

The wiring happens once at startup in `backend.rs`:

1. **`factory::create_signal_slot::<FetchRequest>()`** — creates a
   `Signal<FetchRequest>` (sender) and `Slot<FetchRequest>` (receiver).

2. **`AsyncDispatcher::new()`** — owns the tokio runtime. Its
   `attach_async()` method connects a `Slot<FetchRequest>` (input),
   a `Signal<FetchResponse>` (output), and an async worker function.

3. **`result_slot::start()`** — a closure that runs on the UI thread
   whenever a `FetchResponse` arrives. It writes results into the
   `Dynamic<T>` fields and calls `request_repaint()`.

The `BackendHandle` struct owns the `AsyncDispatcher`. As long as it's
alive (stored on `CompassApp`), the tokio runtime keeps running. Dropping
it shuts everything down cleanly.

### Threading summary

| Thread | Role | Code |
|---|---|---|
| **Main (UI)** | egui rendering, citizen outbox drain, event routing, result slot handler | `CompassApp::ui()` |
| **AsyncDispatcher** | Tokio multi-thread runtime, receives `FetchRequest`, runs `DuckDbProvider`, sends `FetchResponse` | `AsyncDispatcher` via `backend.rs` |

The old pattern used a manual `std::thread::spawn` + `mpsc::channel` +
`Arc<Mutex<CompassState>>`. The new pattern replaces all three with
framework-managed primitives: citizens own presentation, `Dynamic<T>`
owns state, and `Signal`/`Slot` + `AsyncDispatcher` own async I/O.

### spawn_blocking for DuckDB

DuckDB's C API is synchronous. All DuckDB queries run inside
`tokio::task::spawn_blocking`, which moves the blocking work to a dedicated
thread pool. This keeps the tokio runtime responsive for other async tasks
(HTTP fetches, timers). This part hasn't changed from the previous architecture.

## Data pipeline: from user click to chart

When you type `600519`, select `1d`, and click "Fetch", here's what happens:

```
UI (CompassApp::ui)
  │  user clicks "Fetch" button
  │  state.symbol.set("600519")
  │  state.timeframe.set("1d")
  │  outbox.push(AppMessage::FetchBars)
  │
  ▼ (same frame, outbox drain loop)
  dispatcher::handle(AppMessage::FetchBars, state, work_signal)
  │  state.loading.set(true)
  │  work_signal.send(FetchRequest { symbol:"600519", timeframe:"1d", ... })
  │
  ▼
AsyncDispatcher (tokio runtime)
  │  work_slot receives FetchRequest
  │
  ▼
DuckDbProvider::fetch_bars("600519", "1d", start, end)
  │
  ├─ 1. Query in-memory stock_daily table → cache hit? Return bars.
  │
  ├─ 2. Cache miss → read parquet_data/stock_daily.parquet via read_parquet()
  │     with WHERE symbol = ? filtering
  │
  ├─ 3. Cache-warm: INSERT OR IGNORE parquet data into in-memory table
  │     Subsequent queries hit memory, not disk
  │
  └─ 4. Return FetchResponse to result_signal
  │
  ▼
result_slot handler (called on UI thread)
  │  state.bars.set(resp.bars)
  │  state.loading.set(false)
  │  state.error.set(None or error)
  │  egui_ctx.request_repaint()
  │
  ▼
UI (next frame)
  │  ChartCitizen::show() reads state.bars.get()
  │  bars.len() differs from previous → rebuilds BarData
  │  chart.show(ui) renders candlestick chart
```

### Why local-only?

With #31 and #32, the GUI reads all data from local Parquet files. No remote
fallback, no negative cache, no inflight dedup. The data pipeline (import from
Dolt, collectors from EastMoney) runs offline; the GUI only queries what's
already on disk.

## Data pipeline: CLI (compass-data)

The CLI manages data offline, before the GUI ever runs. It has three subcommands
that form a pipeline:

```
Dolt DB ───────import─────► parquet_data/
staging.duckdb ──merge───► parquet_data/
parquet_data/ ──export───► compass.duckdb
```

The project also maintains its own Dolt repository `compass_data/` for
custom mutable data (company profiles, financial indicators, watchlists),
stored alongside the read-only `investment_data`. Queries join across both
databases: `compass_data.stock_basic JOIN investment_data.final_a_stock_eod_price`.
See `kb/dev/process.md#dolt-database-queries` for usage examples.

### collectors: Python data pipeline

```
EastMoney API ──collectors──► CSV ──import──► compass_data (Dolt)
```

The `collectors/` directory contains Python scripts (uv + curl_cffi) for
fetching data from EastMoney public APIs and importing into Dolt:

| Script | Purpose | Data |
|---|---|---|
| `main.py` | 统一 CLI: fetch/import/sync/sync-investment | — |
| `fetch_stock_basic.py` | 公司基本信息 | 12,388 stocks, 13 fields |
| `fetch_fin_indicators.py` | 财务指标 | 473K rows, 37 fields, 2000-2026 |
| `fetch_balance_sheet.py` | 资产负债表 | 57 fields, quarterly, RPT_DMSK_FN_BALANCE |
| `fetch_income.py` | 利润表 | 46 fields, quarterly, RPT_DMSK_FN_INCOME |
| `fetch_cash_flow.py` | 现金流量表 | 48 fields, quarterly, RPT_DMSK_FN_CASHFLOW |

Toolchain: `uv` (Python dependency manager) + `ruff` (lint/formatter) +
`pytest` (16 tests) + `mypy` (type checking). CI via GitHub Actions,
pre-commit/pre-push hooks enforce lint + test on every change.

Key design decisions:
- **curl_cffi** over httpx/aiohttp: EastMoney checks TLS fingerprints (JA3/JA4);
  curl_cffi impersonates Chrome to bypass detection
- **CSV as intermediate**: eastmoney → CSV → Dolt, not direct
- **Incremental mode**: state files (`.state.json`) track last fetch date;
  `--incremental` flag fetches only new report periods
- **Known limitation**: REPORTDATE-based increments cannot detect revisions to
  already-fetched periods (e.g. 五粮液 2025Q1 revision). A periodic `--refresh N`
  flag is planned (see issue #27).

### import: Dolt → Parquet
- Queries Dolt `investment_data` database via `dolt sql -r parquet`
- Extracts 6000+ stocks from `final_a_stock_eod_price` table (18M+ rows)
- Writes to a single `parquet_data/stock_daily.parquet` with a `symbol` column
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

### backup: Parquet → Baidu Cloud
- Zips `parquet_data/` using Python zipfile (no system `zip` dependency)
- Uploads to Baidu Cloud via `baidupcs` CLI (`BaiduPCS-Go`)
- Timestamped filenames: `parquet_data-YYYYMMDD-HHMMSS.zip`
- Target folder: `/compass/` on Baidu Cloud
- `--keep-zip` flag preserves local zip after upload

**Default behavior everywhere**: merge/skip. Existing data is preserved; only
new data is added. Pass `--overwrite` to replace. This migration-style behavior
prevents accidental data loss.

## Storage strategy: why both DuckDB and Parquet?

```
Compass uses two database formats for different purposes:

  Parquet files (parquet_data/)
    ├─ Source of truth — the canonical data store
    ├─ Stock basic: stock_basic.parquet (one file for all symbols)
    ├─ Stock daily: stock_daily.parquet (single file with symbol column)

  DuckDB (in-memory for GUI, file-backed for CLI staging)
    ├─ GUI — in-memory with Parquet fallback (reads parquet_data/ on cache miss)
    ├─ CLI staging — temporary buffer during download (data/staging.duckdb)
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

## Symbol convention: Dolt-native prefixed codes

Every stock in Compass is identified by its Dolt-native symbol with exchange
prefix: `"SZ000001"`, `"SH600519"`, `"BJ836149"`. The 2-letter prefix
(SZ/SH/BJ) is part of the canonical identifier — it's in the Parquet
filename, in the database column, and in the API. Bare 6-digit input is
accepted as a convenience and resolved via exchange inference.

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
parquet_dir = "parquet_data"           # parquet data directory
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
| 3 | Async runtime | tokio (rt-multi-thread) | DuckDbProvider uses tokio::spawn_blocking for synchronous DuckDB queries. CLI uses current_thread for simplicity. |
| 4 | HTTP client | reqwest 0.12 (rustls-tls) | Used only by compass-data CLI (kept for potential future use). GUI has no HTTP dependency. |
| 5 | Database | duckdb 1.0 (bundled) | OLAP-optimized columnar engine. Reads/writes Parquet natively. The `bundled` feature ships the C library — no system duckdb required. |
| 6 | DB threading | spawn_blocking + Mutex | DuckDB is synchronous C. `spawn_blocking` moves queries to a thread pool so they don't block the async runtime. Mutex on the DuckDB connection ensures exclusive access. |
| 7 | Serialization | serde + serde_json | Config parsing and test data. serde derives on all data types. |
| 8 | Time handling | chrono 0.4 | UTC timestamps, date arithmetic (range_start/end calculation), JSON parse support. |
| 9 | Error types | thiserror 2 (library), anyhow 1 (binaries) | Precise `DataError` enum with `From` impls for `?` propagation in the library. `anyhow` for context-wrapping in binary entry points. |
| 10 | Logging | tracing + subscriber + appender | Structured, async, level-filtered. Daily rolling files via tracing-appender. |
| 11 | Async traits | async-trait 0.1 | Native async traits in Rust are still unstable. This macro is the standard workaround. |
| 12 | Config | toml → Deserialize | Simple, readable format. `#[serde(default)]` on every field means partial configs work. |
| 13 | CLI args | clap 4 (derive) | Derive macro generates the CLI parser from a struct. Type-safe, self-documenting. |
| 14 | Progress bars | indicatif 0.17 | Spinner + progress bar for long-running CLI operations (import). |
| 15 | Concurrency | futures Semaphore + buffer_unordered | Bounded parallelism for CLI download. Semaphore caps concurrent requests; buffer_unordered preserves order while processing results as they arrive. |
| 16 | Reactive state | egui_mobius_reactive `Dynamic<T>` | Per-field `Dynamic<T>` replaces monolithic `Arc<Mutex<CompassState>>`. No manual version counter, no cross-field lock contention. Each field is independently readable/writable. |
| 17 | Citizen pattern | egui_citizen (Citizen trait) | Frameworks citizen lifecycle (register, activate, deactivate, drain) and eliminates manual thread wiring. Citizens use outbox pattern — no direct backend coupling. |
| 18 | Dock layout | egui_dock 0.20 | Tabbed dockable panels with resize and rearrange. Bridges to citizen activation via TabViewer. Replaces manual panel layout. |
| 19 | Async dispatch | egui_mobius `Signal`/`Slot` + `AsyncDispatcher` | Typed channels replace `mpsc::channel` for command dispatch. `AsyncDispatcher` manages its own tokio runtime — no `std::thread::spawn` + `rt.block_on` boilerplate. |
| 20 | Provider traits | DataProvider + DataWriter + NegativeCache | Trait-based abstraction for data backends: DuckDB, Parquet — all behind the same interface. Testable with mock implementations. |
| 21 | Parquet storage | DuckDB read_parquet + COPY TO | Columnar format partitioned by symbol. Queryable without loading into tables. |
| 22 | Dolt import | dolt CLI → Parquet (direct) | Offline bulk import of 18M+ rows. Dolt `sql -r parquet` writes binary Parquet directly, skipping the CSV intermediary. |

## Where to go next

- **Data providers**: `kb/design/data-providers.md` — the trait system and each
  provider implementation in depth
- **Symbols**: `kb/design/symbols.md` — market segments, code conversion,
  timeframe mapping
- **API reference**: `cargo doc --open` — full type-level documentation for
  all public APIs

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 数据访问策略：GUI 读取数据的来源 | 在线 API 直接请求 / 本地文件缓存 / 纯本地无回退 | 纯本地 Parquet 文件，无在线回退 | 本地读取零延迟、无网络依赖、无 API 限流；数据管线（import/collector）离线运行，GUI 只查询已落盘数据 | 在线 API 增加延迟和失败点；缓存策略需处理过期和同步问题，增加复杂度 |
| 异步架构：UI 线程与 I/O 分离方案 | 手动 std::thread + mpsc / 框架托管的 citizen 模式 | egui-mobius citizen 模式：Citizen trait + Dynamic\<T\> + Signal/Slot + AsyncDispatcher | 消除手动线程布线、Arc\<Mutex\> 竞争和版本计数器；Citizen 通过 outbox 解耦，AsyncDispatcher 自管 tokio runtime | 手动线程方案代码量大、易出错；Dynamic\<T\> 提供字段级独立读写，无跨字段锁竞争 |
| 规范存储格式：Parquet 单文件 vs 其他方案 | 每标的单独文件 / 单文件含 symbol 列 / DuckDB 做主存储 | 单个 `stock_daily.parquet`，symbol 列分区查询 | 列式存储、谓词下推、开放标准、工具链兼容（Python/R/DuckDB）；单文件管理简单，无需处理数千个文件 | 单文件追加困难（写入需重写整个文件），但通过 DuckDB staging + merge 管线解决；每标的单独文件增加文件管理开销 |
| 符号约定：规范标识符格式 | ts_code 格式（`000001.SZ`）/ Dolt-native 前缀格式（`SZ000001`） | Dolt-native 前缀格式 | 前缀即交换所见即所得，无歧义（`SZ000852` 和 `SH000852` 可共存）；交换所可从代码推断，ts_code 的后缀冗余 | ts_code 将身份与元数据混合，`.SZ` 后缀冗余且格式不一致（需解析 `.` 分隔符） |
