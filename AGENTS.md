# AGENTS.md — compass

A-share stock chart desktop application built with egui. Fetches real
OHLCV data from EastMoney (东方财富), caches locally in SQLite.

## Knowledge base

Detailed docs under `kb/`:

| File | Content |
|---|---|
| `kb/architecture.md` | Threading model, data pipeline, CachedProvider, SQLite schema, source layout, library decisions |
| `kb/symbols.md` | A-share market segments, `to_secid()` prefix/fallback logic, timeframe mapping |
| `kb/data-providers.md` | EastMoney HTTP API (params, JSON paths), SqliteProvider read/write, DataError |
| `kb/testing.md` | rstest + tokio::test patterns, in-memory SQLite, httpmock setup |
| `kb/process.md` | Dev workflow, commands, config, debugging, reset |

## Setup

- **Rust edition 2024** — requires Rust ≥1.85. Current: 1.96.
- **GUI app** — needs a display server (X11/Wayland). `cargo run` opens a window.
- Logs written to `logs/compass.log` (daily rolling).
- Config at `~/.config/compass/config.toml` (falls back to defaults).

## Commands

```sh
cargo build
cargo run           # opens stock chart window
cargo test          # unit tests (rstest + tokio-test)
cargo fmt
cargo clippy
RUST_LOG=debug cargo run   # verbose logging
```

## Architecture

- Single binary crate (`src/main.rs`). `Cargo.lock` committed.
- `CompassApp` owns a `Chart` widget and shared `CompassState` (Arc<Mutex<>>).
- Worker thread (`std::thread`) runs a `tokio` runtime, listens for `Cmd` via mpsc,
  dispatches to `CachedProvider`, and updates `CompassState`.
- UI thread polls state each frame, rebuilds chart data on `bars_version` change.

### Data pipeline

```
UI (CompassApp)
  └─ mpsc::Sender<Cmd>
       └─ Worker thread (tokio runtime)
            └─ CachedProvider<R: DataProvider, W: DataWriter>
                 ├─ 1. SqliteProvider::fetch_bars  (cache read)
                 ├─ 2. EastMoneyProvider::fetch_bars  (HTTP, cache miss)
                 └─ 3. SqliteProvider::save_bars  (write-through)
```

## Data providers

### EastMoneyProvider (`src/data/eastmoney.rs`)

Fetches K-line data from `push2his.eastmoney.com`. Symbol → secid conversion
via `to_secid()`:

| Input | secid | Description |
|---|---|---|
| `000001` | `0.000001` | 平安银行 (SZ, heuristic default) |
| `600519` | `1.600519` | 贵州茅台 (SH, heuristic: 6xxxxx) |
| `688001` | `1.688001` | 华兴源创 (科创板) |
| `300750` | `0.300750` | 宁德时代 (创业板) |
| `sh.000001` | `1.000001` | 上证指数 (explicit SH prefix) |
| `sz.000001` | `0.000001` | 显式深圳 |
| `bj.8xxxxx` | `0.8xxxxx` | 北交所 |

Explicit prefixes `sh.` / `sz.` / `bj.` disambiguate the `000xxx–004xxx`
range (SZ stocks vs SH indices). Prefixes are case-insensitive.

### SqliteProvider (`src/data/sqlite.rs`)

Local persistent cache. One table `bars` keyed by `(symbol, timeframe, adj_type, timestamp)`.
Used as both `DataProvider` (read) and `DataWriter` (write-through).

### CachedProvider (`src/data/mod.rs`)

Read-through cache: cache hit → return, cache miss → fetch remote → write cache.

## Config

`~/.config/compass/config.toml` (all fields optional):

```toml
[database]
path = "compass.db"

[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10
retry_count = 3

[app]
default_symbol = "000001"
default_timeframe = "1d"
```

## Testing

- Framework: `rstest` (parameterized + fixtures) + `#[tokio::test]` for async
- HTTP mock: `httpmock` (dev-dependency, not yet wired)
- SQLite tests use `":memory:"` for isolated in-memory databases
- Run: `cargo test` or `cargo nextest run` (recommended for large suites)

```toml
[dev-dependencies]
rstest = "0.25"
httpmock = "0.8"
```

## egui-charts API

- `Bar::new(time, open, high, low, close, volume)` — OHLCV bar
- `BarData::from_bars(bars)` — dataset wrapper
- `Chart::new(data)` — interactive chart widget (pan, zoom, crosshair)
- `chart.set_chart_type(ChartType::Candles)` — candlestick display
- `chart.show(ui)` — render inside any `egui::Ui`
