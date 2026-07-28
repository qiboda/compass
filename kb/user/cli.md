# Data Pipeline (CLI)

## Overview

`compass-data` manages A-share OHLCV data through four subcommands:

```
EastMoney API ──download──► staging.duckdb ──merge──► parquet_data/ ──export──► data/compass.duckdb
Dolt DB ───────import─────► parquet_data/
```

## Common options

Every subcommand follows the same data integrity rule:

- **Default**: merge/skip — existing data is preserved, only new data is added
- **`--overwrite`**: replace existing data with new values

Pass `--overwrite` when you want a clean slate. Omit it for incremental updates.

---

## `download` — EastMoney → Staging DuckDB

Downloads OHLCV bars from EastMoney's public API into a staging DuckDB database.

```sh
cargo run --bin compass-data -- download [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--symbols` | `all` | Comma-separated stock codes or `all` for every A-share |
| `--db` | `data/staging.duckdb` | Staging database path |
| `--concurrency` | `2` | Max simultaneous downloads |
| `--delay-ms` | `1000` | Delay between requests (ms) |
| `--start-date` | `19900101` | Earliest date (YYYYMMDD) |
| `--end-date` | yesterday | Latest date (YYYYMMDD) |
| `--base-url` | `https://push2his.eastmoney.com` | EastMoney K-line endpoint |
| `--realtime-url` | `https://push2delay.eastmoney.com` | EastMoney realtime endpoint |
| `--overwrite` | `false` | Replace existing data instead of skipping |

### Examples

```sh
# Download all A-shares (slow — hours)
cargo run --bin compass-data -- download --symbols all

# Download specific stocks
cargo run --bin compass-data -- download --symbols 000001,600519,300750

# Download with rate limiting (more conservative)
cargo run --bin compass-data -- download --symbols all --concurrency 1 --delay-ms 2000

# Download recent data only (faster)
cargo run --bin compass-data -- download --symbols all --start-date 20250101

# Force overwrite
cargo run --bin compass-data -- download --symbols all --overwrite
```

---

## `import` — Dolt → Parquet (Primary)

Imports complete history from the local Dolt `investment_data` database into
partitioned Parquet files.

```sh
cargo run --bin compass-data -- import [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--dolt-dir` | `investment_data` | Dolt database directory |
| `--output` | `parquet_data` | Output directory for Parquet files |
| `--symbols` | (all) | Comma-separated 6-digit codes (e.g. `000001,600519`) |
| `--limit` | `0` (all) | Max number of symbols to import |
| `--start-date` | (earliest) | Filter by start date (YYYYMMDD) |
| `--end-date` | (latest) | Filter by end date (YYYYMMDD) |
| `--overwrite` | `false` | Replace existing data instead of merging |

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

# Full overwrite (delete existing, start fresh)
cargo run --bin compass-data -- import --overwrite
```

---

## `merge` — Staging DuckDB → Parquet

Moves data from the staging DuckDB into the Parquet main database for symbols
that don't already exist there.

```sh
cargo run --bin compass-data -- merge [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--db` | `data/staging.duckdb` | Staging database path |
| `--output` | `parquet_data` | Parquet data directory |
| `--overwrite` | `false` | Replace existing data instead of merging |

### Example

```sh
# After download, merge new symbols into Parquet
cargo run --bin compass-data -- download --symbols all
cargo run --bin compass-data -- merge
```

---

## `export` — Parquet → Other Formats

Exports the Parquet main database to DuckDB, CSV, or another Parquet directory.

```sh
cargo run --bin compass-data -- export [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `--input` | `parquet_data` | Parquet data directory |
| `--format` | `duckdb` | Output format: `duckdb`, `csv`, `parquet-dir` |
| `--output` | `data/compass.duckdb` | Output path |
| `--overwrite` | `false` | Replace existing data instead of skipping |

### Examples

```sh
# Export to DuckDB (for the GUI to use)
cargo run --bin compass-data -- export

# Export to CSV
cargo run --bin compass-data -- export --format csv --output data.csv

# Force overwrite
cargo run --bin compass-data -- export --overwrite
```

---

## Typical workflows

### First-time setup (from Dolt)

```sh
# 1. Import all data from Dolt
cargo run --bin compass-data -- import

# 2. Export to DuckDB for the GUI
cargo run --bin compass-data -- export

# 3. Launch the chart app
cargo run
```

### Incremental update (downloading new data)

```sh
# 1. Download latest data from EastMoney
cargo run --bin compass-data -- download --symbols all

# 2. Merge new data into Parquet
cargo run --bin compass-data -- merge

# 3. Re-export to DuckDB (includes new data)
cargo run --bin compass-data -- export --overwrite
```

### Download a single stock for charting

```sh
cargo run --bin compass-data -- download --symbols 600519
cargo run
# Type "600519", click Fetch — data loads from compressed cache
```

---

## Troubleshooting

### Rate limiting

EastMoney throttles aggressive requests. If you see `RateLimited` errors:

```sh
# Reduce concurrency and increase delay
cargo run --bin compass-data -- download --concurrency 1 --delay-ms 3000
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
