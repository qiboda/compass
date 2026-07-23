# AGENTS.md — compass

A-share stock chart desktop application built with egui. Data pipeline supports
EastMoney (online), Dolt investment_data (local), and Parquet-based storage
with DuckDB for querying.

## Knowledge base

Detailed docs under `kb/`:

| File | Content |
|---|---|
| `kb/architecture.md` | Threading model, data pipeline, CachedProvider, schema, source layout, libraries |
| `kb/symbols.md` | A-share market segments, `to_secid()` prefix/fallback logic, timeframe mapping |
| `kb/data-providers.md` | EastMoney HTTP, DuckDB, Dolt, ParquetReader, DataError |
| `kb/testing.md` | rstest + tokio::test patterns, in-memory DuckDB, httpmock setup |
| `kb/process.md` | Dev workflow, commands, config, debugging, reset |

## Setup

- **Rust edition 2024** — requires Rust ≥1.85. Current: 1.96.
- **GUI app** — needs a display server (X11/Wayland). `cargo run` opens a window.
- Logs written to `logs/compass.log` (daily rolling).
- Config at `~/.config/compass/config.toml` (falls back to defaults).

## Commands

```sh
cargo build
cargo run                    # GUI chart window
cargo run --bin compass-data -- <subcommand>  # data pipeline CLI
cargo test                   # unit + integration tests
cargo fmt
cargo clippy
RUST_LOG=debug cargo run     # verbose logging
```

### compass-data CLI

```sh
# Download from EastMoney into staging DuckDB
cargo run --bin compass-data -- download --symbols 600519

# Import from Dolt investment_data into Parquet main database
cargo run --bin compass-data -- import --limit 100

# Merge staging DuckDB into Parquet main database
cargo run --bin compass-data -- merge

# Export Parquet to DuckDB
cargo run --bin compass-data -- export
```

## Architecture

See `kb/architecture.md` — threading model, data pipeline, CachedProvider, schema, source layout, libraries.

## Data providers

See `kb/data-providers.md` — EastMoney, DuckDB, Dolt, ParquetReader, DataError.

### EastMoneyProvider (`crates/compass-core/src/data/eastmoney.rs`)

Fetches K-line data from `push2his.eastmoney.com`. Symbol → secid conversion via `to_secid()`:

| Input | secid | Description |
|---|---|---|
| `000001` | `0.000001` | 平安银行 (SZ, heuristic default) |
| `600519` | `1.600519` | 贵州茅台 (SH, heuristic: 6xxxxx) |
| `688001` | `1.688001` | 华兴源创 (科创板) |
| `300750` | `0.300750` | 宁德时代 (创业板) |
| `sh.000001` | `1.000001` | 上证指数 (explicit SH prefix) |
| `sz.000001` | `0.000001` | 显式深圳 |
| `bj.8xxxxx` | `0.8xxxxx` | 北交所 |

### Dolt import (`crates/compass-data/src/import_dolt.rs`)

Reads from Dolt `investment_data` (`final_a_stock_eod_price` table) via `dolt sql`
CSV export, converts to Parquet files partitioned by symbol.

## Parquet schema (main database)

```
parquet_data/
├── stock_basic.parquet        # symbol, name, exchange, list_date, delist_date
└── stock_daily/
    ├── 000001.parquet         # tradedate, open, high, low, close, adjclose, volume, amount
    ├── 600519.parquet
    └── ...
```

DuckDB schema (staging):
```sql
CREATE TABLE stock_daily (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    open, high, low, close, adjclose DOUBLE,
    volume, amount DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_basic (
    symbol      VARCHAR PRIMARY KEY,
    name, industry, market, exchange VARCHAR,
    list_date, delist_date DATE
);
CREATE TABLE stock_adj_factor (
    symbol, trade_date, adj_factor, PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_limit (
    symbol, trade_date, up_limit, down_limit, PRIMARY KEY (symbol, trade_date)
);
```

## Config

`~/.config/compass/config.toml` (all fields optional):

```toml
[database]
path = "compass.duckdb"

[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10
retry_count = 3

[app]
default_symbol = "000001"
default_timeframe = "1d"
```

## Testing

See `kb/testing.md` — rstest + tokio::test patterns, in-memory DuckDB, httpmock setup.

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
