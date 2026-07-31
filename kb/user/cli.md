# Data Pipeline (CLI)

## Overview

`compass-data` manages A-share OHLCV data through four subcommands:

```
Dolt investment_data ──import─────────► parquet_data/
Dolt compass_data ────import-compass──► parquet_data/
parquet_data/ ────────export──────────► duckdb / csv / parquet-dir
parquet_data/ ────────backup──────────► Baidu Cloud (zip)
```

EastMoney data is fetched by the Python collectors (`collectors/`) into Dolt
`compass_data`, then imported via `import-compass`. The Rust CLI itself never
talks to EastMoney.

## Common options

- **`--overwrite`** (on `import-compass` and `export`): replace existing data
  with new values. Default behavior is merge/skip — existing data is preserved,
  only new data is added.
- `import` (Dolt investment_data) always writes the full dataset directly —
  there is no `--overwrite` flag.

---

## `import` — Dolt investment_data → Parquet (Primary)

Imports complete history from the local Dolt `investment_data` database into
the Parquet main database.

```sh
cargo run --bin compass-data -- import [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--dolt-dir` | from config `[dolt].investment_data_dir` | Dolt database directory |
| `--output` | from config `[parquet].dir` | Output directory for Parquet files |
| `--symbols` | (all) | Comma-separated 6-digit codes (e.g. `000001,600519`) |
| `--limit` | `0` (all) | Max number of symbols to import |
| `--start-date` | (earliest) | Filter by start date (YYYYMMDD) |
| `--end-date` | (latest) | Filter by end date (YYYYMMDD) |
| `--since` | (none) | Incremental: only import data with tradedate >= since (YYYYMMDD) |

The import reads each symbol's rows via `dolt sql -r parquet` (direct binary
Parquet) and writes them into the single `stock_daily.parquet` file. Running it
again re-imports the full dataset.

### Output structure

```
parquet_data/
├── stock_basic.parquet             # Stock metadata (one file)
├── stock_daily.parquet             # OHLCV data (single file with symbol column)
└── stock_daily.symbols.txt         # Symbol index (one per line)
```

The `symbol` column in `stock_daily.parquet` stores Dolt's native symbol format
(e.g. `SZ000001`, `SH600519`). A stock (SZ) and an index (SH) sharing the same
6-digit code are disambiguated by the exchange prefix.

### Examples

```sh
# Full import (all 6000+ stocks, ~1 hour)
cargo run --bin compass-data -- import

# Import specific stocks
cargo run --bin compass-data -- import --symbols 000001,600519

# Import with date filter
cargo run --bin compass-data -- import --start-date 20200101 --end-date 20250721

# Import first 100 stocks (testing)
cargo run --bin compass-data -- import --limit 100

# Incremental: only data since 2026-07-25
cargo run --bin compass-data -- import --since 20260725
```

---

## `import-compass` — Dolt compass_data → Parquet

Imports tables from our own `compass_data` Dolt repository (company profiles,
financial indicators, balance sheet, income, cash flow) into Parquet.

```sh
cargo run --bin compass-data -- import-compass --table <table> [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--table` | (required) | `stock_basic`, `fin_indicators`, `fin_balance_sheet`, `fin_income`, `fin_cash_flow` |
| `--dolt-dir` | from config `[dolt].compass_data_dir` | Dolt database directory |
| `--output` | from config `[parquet].dir` | Output directory for Parquet files |
| `--overwrite` | `false` | Replace existing data instead of merging |
| `--since` | (none) | Incremental: only import data with report_date >= since (YYYYMMDD) |

### Examples

```sh
# Import company profiles
cargo run --bin compass-data -- import-compass --table stock_basic

# Import financial indicators (incremental)
cargo run --bin compass-data -- import-compass --table fin_indicators --since 20260101

# Force overwrite
cargo run --bin compass-data -- import-compass --table stock_basic --overwrite
```

---

## `export` — Parquet → Other Formats

Exports the Parquet main database to DuckDB, CSV, or another Parquet directory.

```sh
cargo run --bin compass-data -- export [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--input` | from config `[parquet].dir` | Parquet data directory |
| `--format` | `duckdb` | Output format: `duckdb`, `csv`, `parquet-dir` |
| `--output` | `/data/compass-data/compass.duckdb` | Output path |
| `--overwrite` | `false` | Replace existing data instead of skipping |

### Examples

```sh
# Export to DuckDB
cargo run --bin compass-data -- export

# Export to CSV
cargo run --bin compass-data -- export --format csv --output data.csv

# Force overwrite
cargo run --bin compass-data -- export --overwrite
```

---

## `backup` — Parquet → Baidu Cloud

Zips `parquet_data/` and uploads to Baidu Cloud via `baidupcs` (BaiduPCS-Go).

```sh
cargo run --bin compass-data -- backup [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--input` | from config `[parquet].dir` | Parquet data directory to backup |
| `--keep-zip` | `false` | Keep local zip file after upload |

- Timestamped filenames: `parquet_data-YYYYMMDD-HHMMSS.zip`
- Target folder: `/compass/` on Baidu Cloud
- Standalone: `scripts/upload-parquet.sh [--keep-zip]`

---

## Python collectors (EastMoney → Dolt)

The `collectors/` directory contains Python scripts (uv + curl_cffi) that fetch
data from EastMoney APIs into CSV, then import into Dolt `compass_data`:

```sh
cd collectors/
uv sync                                # first time: install dependencies

uv run python main.py fetch stock_basic
uv run python main.py sync             # fetch + import all
uv run python main.py sync-investment --restart
```

Key concepts:
- **curl_cffi** for TLS impersonation (EastMoney anti-crawler)
- **CSV as intermediary** between API and Dolt
- **`.state.json`** files track last fetch for incremental updates
- **`--resume`** flag to continue interrupted fetches

See `kb/design/architecture.md` for the full collectors pipeline description.

---

## Typical workflows

### First-time setup (from Dolt)

```sh
# 1. Import all data from Dolt investment_data
cargo run --bin compass-data -- import

# 2. Import company profiles from Dolt compass_data
cargo run --bin compass-data -- import-compass --table stock_basic

# 3. Launch the chart app
cargo run
```

### Fetch new data from EastMoney

```sh
# 1. Fetch latest data into Dolt compass_data (Python collectors)
cd collectors/
uv run python main.py sync

# 2. Import the new tables into Parquet
cargo run --bin compass-data -- import-compass --table fin_indicators --since 20260101
```

### Backup to Baidu Cloud

```sh
cargo run --bin compass-data -- backup            # upload zip
cargo run --bin compass-data -- backup --keep-zip # keep local zip after upload
```

---

## Troubleshooting

### Rate limiting (collectors)

EastMoney throttles aggressive requests. In `collectors/`, reduce concurrency
and increase delay:

```sh
uv run python main.py sync --concurrency 1 --delay-ms 3000
```

### Dolt not found

```sh
# Verify Dolt is installed and investment_data/ exists
dolt --data-dir=investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
```

### Import is slow

Import queries Dolt 6000+ times — each query takes ~0.5s. Total time is determined
by Dolt query speed, not file I/O. Date filtering speeds it up:

```sh
cargo run --bin compass-data -- import --start-date 20240101
```

### Logs

Set `RUST_LOG` for verbose output:

```sh
RUST_LOG=debug cargo run --bin compass-data -- import
```

Logs appear on stderr and in `logs/compass.log` (daily rolling).
