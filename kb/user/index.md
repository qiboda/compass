# Compass User Guide

Compass is a **local-first A-share stock chart application** with a data
pipeline for managing historical market data.

## What you can do

| Tool | What it does |
|---|---|
| **Chart app** (`cargo run`) | View interactive candlestick charts for any A-share stock |
| **Data pipeline** (`compass-data`) | Import, export, and back up market data |

## How data works

Compass stores all market data **locally on your machine**. Once imported,
charts render instantly — no internet, no API keys, no rate limits.

```
Data sources → Import → Parquet files → Chart app
```

There are two ways data gets into the local Parquet database:

| Source | What | When to use |
|---|---|---|
| **Dolt** (`investment_data`) | Complete A-share EOD history (1990–present, 18M+ rows) | Bulk import via `compass-data import` |
| **EastMoney** (online) | Real-time and historical data via Python collectors | Fetch data not yet in Dolt, then import |

Dolt is the **primary** data source — complete, offline, fast. EastMoney data
is fetched by the Python collectors (`collectors/`) and flows into Dolt
`compass_data`, then into Parquet via import. The GUI itself is **local-only** —
it never calls EastMoney directly.

## Quickstart

```sh
# 1. Import all A-share history from Dolt (one-time, ~1 hour)
cargo run --bin compass-data -- import

# 2. Launch the chart app
cargo run

# Type a stock code (e.g. 600519) and click Fetch
```

## Prerequisites

- **Rust** ≥ 1.85 (edition 2024)
- **Display server** (X11 or Wayland) for the GUI
- **Dolt CLI** for `compass-data import`
- **Dolt database** `investment_data/` for the import source

## Documentation map

| Document | Covers |
|---|---|
| [GUI](gui.md) | Chart app — symbol input, timeframe, controls |
| [CLI](cli.md) | Data pipeline — download, import, merge, export |
| [Config](config.md) | `config.toml` — all options and defaults |

For developers: [kb/design/](../design/architecture.md) covers system design and architecture.
