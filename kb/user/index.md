# Compass User Guide

Compass is a **local-first A-share stock chart application** with a data
pipeline for managing historical market data.

## What you can do

| Tool | What it does |
|---|---|
| **Chart app** (`cargo run`) | View interactive candlestick charts for any A-share stock |
| **Data pipeline** (`compass-data`) | Download, import, merge, and export market data |

## How data works

Compass stores all market data **locally on your machine**. Once imported,
charts render instantly — no internet, no API keys, no rate limits.

```
Data sources → Import → Parquet files → Chart app
           ↘ Download → DuckDB cache ↗
```

There are two data sources:

| Source | What | When to use |
|---|---|---|
| **Dolt** (`investment_data`) | Complete A-share EOD history (1990–present, 18M+ rows) | Bulk import via `compass-data import` |
| **EastMoney** (online) | Real-time and historical K-line data | Live download via `compass-data download` |

Dolt is the **primary** data source — complete, offline, fast. EastMoney is
a fallback for data not yet imported locally.

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
