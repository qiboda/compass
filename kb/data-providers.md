# Data Providers

## Provider stack

```
CachedProvider<R: DataProvider, C: DataProvider+NegativeCache+DataWriter>
    ├── reader: EastMoneyProvider  (remote HTTP)
    └── cache:  DuckDbProvider     (local read + write-through + negative cache)
```

The CLI downloader (`compass-downloader`) uses `EastMoneyProvider` and `DuckDbProvider`
directly — it does NOT use `CachedProvider`.

## Traits (`src/data/provider.rs`)

```rust
#[async_trait]
trait DataProvider: Send + Sync {
    async fn fetch_bars(&self, symbol, timeframe, range_start, range_end) -> Result<Vec<Bar>, DataError>;
    async fn search_symbols(&self, query: &str) -> Result<Vec<SymbolInfo>, DataError>;
}

#[async_trait]
trait DataWriter: Send + Sync {
    async fn save_bars(&self, symbol, timeframe, bars: &[Bar]) -> Result<(), DataError>;
}

#[async_trait]
trait NegativeCache: Send + Sync {
    async fn mark_no_data(&self, symbol: &str, timeframe: &str) -> Result<(), DataError>;
    async fn is_no_data(&self, symbol, timeframe, now_ts, ttl_secs) -> Result<bool, DataError>;
}
```

## EastMoneyProvider (`src/data/eastmoney.rs`)

Source: `https://push2his.eastmoney.com` (K-line, symbols, stock info) and
`https://push2.eastmoney.com` (realtime quotes).

### Fetch bars (`DataProvider::fetch_bars`)

HTTP GET to `{base_url}/api/qt/stock/kline/get`.

Parameters:

| Param | Source | Example |
|---|---|---|
| `secid` | `to_secid(symbol)` | `0.000001`, `1.600519` |
| `klt` | `timeframe_to_klt(tf)` | `101` (daily), `102` (weekly) |
| `fqt` | Hardcoded | `1` (前复权) |
| `beg` | `range_start` formatted `%Y%m%d` | `20250101` |
| `end` | `range_end` formatted `%Y%m%d` | `20250721` |
| `lmt` | Hardcoded | `2000` (max bars) |
| `fields1` | Hardcoded | `f1,f2,f3,f4,f5,f6` |
| `fields2` | Hardcoded | `f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61` |

Response JSON path: `data.klines[]` — each element is a comma-separated string:
```
"2025-07-21,12.04,12.01,12.11,11.95,1079027,..."
           [1]    [2]   [3]   [4]   [5]
          open  close high   low  volume
```

Parsed fields: `[0]=date, [1]=open, [2]=close, [3]=high, [4]=low, [5]=volume`.

### Search symbols (`DataProvider::search_symbols`)

HTTP GET to `{base_url}/api/qt/clist/get`.

Parameters:

| Param | Value |
|---|---|
| `pn` | `1` |
| `pz` | `20` |
| `po` | `1` |
| `np` | `1` |
| `fltt` | `2` |
| `invt` | `2` |
| `fid` | `f3` |
| `fs` | `b:DLMK014` |
| `keyword` | User query |

Response JSON path: `data.diff[]` with `f12` (code) and `f14` (name).

Best-effort: any parse or network error returns empty `Vec`, never propagates.

### Search all symbols (`search_all_symbols`)

Paginated version of search_symbols. Parameters:

| Param | Value |
|---|---|
| `pn` | Incrementing page number (1..100) |
| `pz` | Page size (caller-supplied) |
| `po` | `1` |
| `np` | `1` |
| `fltt` | `2` |
| `invt` | `2` |
| `fid` | `f3` |
| `fs` | Caller-supplied filter (e.g. `"b:DLMK014"`) |
| `fields` | `"f12,f14"` |

Auto-pagination: stops when the response contains fewer items than `pz` (partial
page) or 0 items (empty page). Maximum 100 pages.

### Fetch stock basic (`fetch_stock_basic`)

HTTP GET to `{base_url}/api/qt/clist/get`.

Filters by `fs=m:{code}` and returns a single `StockBasic` record:

| Field | JSON key | Description |
|---|---|---|
| `symbol` | `f12` | Stock code (e.g. `"600519"`) |
| `name` | `f14` | Stock display name (e.g. `"贵州茅台"`) |
| `industry` | `f100` | Industry classification |
| `market` | `f102` | Market segment (e.g. `"沪主板"`) |
| `list_date` | `f124` | Unix timestamp → `NaiveDate`; `-1` means unknown |

Exchange is inferred via `to_exchange(code)`. `ts_code` is generated as
`"{code}.{exchange}"` via `to_ts_code(code)`.

### Fetch realtime quote (`fetch_realtime_quote`)

HTTP GET to `{realtime_base_url}/api/qt/stock/get?secid={secid}&fields=f9,f167,f84,f85,f51,f52`.

Returns `RealtimeQuote`:

| Field | JSON key | Description |
|---|---|---|
| `pe` | `f9` | P/E ratio |
| `pb` | `f167` | P/B ratio |
| `total_share` | `f84` | Total share capital (万股) |
| `float_share` | `f85` | Floating share capital (万股) |
| `up_limit` | `f51` | Daily price ceiling (涨停价) |
| `down_limit` | `f52` | Daily price floor (跌停价) |

All fields are `Option<f64>`. String values like `"-"` or `"-3.14"` are parsed
via `parse_opt_f64`; unparseable values yield `None`. A null `data` key returns
`DataError::NoData`.

## DuckDbProvider (`src/data/duckdb.rs`)

Implements `DataProvider`, `DataWriter`, and `NegativeCache`. Uses
`Arc<Mutex<Connection>>` internally — one connection, one writer at a time.

### Schema (7 tables + no_data_marks)

| Table | Purpose | Key |
|---|---|---|
| `stock_daily` | Core OHLCV bars | `(ts_code, trade_date)` |
| `stock_adj_factor` | Adjustment factors from Baostock | `(ts_code, trade_date)` |
| `stock_basic` | Stock name, industry, exchange, list date | `ts_code` |
| `stock_status` | Per-day trading status | `(ts_code, trade_date)` |
| `stock_limit` | Daily price limits (涨停/跌停) | `(ts_code, trade_date)` |
| `daily_indicator` | PE/PB/PS/turnover indicators | `(ts_code, trade_date)` |
| `stock_share` | Share capital and market cap | `(ts_code, trade_date)` |
| `no_data_marks` | Negative cache entries with TTL | `(symbol, timeframe)` |

### Read path (`DataProvider::fetch_bars`)

Uses `tokio::task::spawn_blocking`. Converts bare symbol → `ts_code` via
`symbol::to_ts_code()`. Query:

```sql
SELECT CAST(trade_date AS VARCHAR), open, high, low, close, vol
FROM stock_daily
WHERE ts_code = ? AND trade_date >= ? AND trade_date <= ?
ORDER BY trade_date ASC
```

### Write path (`DataWriter::save_bars`)

Uses `spawn_blocking` + `INSERT OR REPLACE` with ts_code conversion:

```sql
INSERT OR REPLACE INTO stock_daily
    (ts_code, trade_date, open, high, low, close, vol)
VALUES (?, ?, ?, ?, ?, ?, ?)
```

### Per-table methods (non-trait, for CLI downloader)

| Method | Operation |
|---|---|
| `save_stock_daily(ts_code, records)` | INSERT OR REPLACE into stock_daily; computes pre_close |
| `get_stored_range(ts_code)` | MIN/MAX trade_date for gap detection |
| `save_adj_factors(ts_code, factors)` | INSERT OR REPLACE into stock_adj_factor |
| `get_adj_factor_range(ts_code)` | MIN/MAX trade_date for adj_factor |
| `upsert_stock_basic(info)` | INSERT OR REPLACE into stock_basic |
| `get_stock_basic(ts_code)` | Read single stock_basic record |
| `save_status(ts_code, records)` | INSERT OR REPLACE into stock_status |
| `save_limits(ts_code, records)` | INSERT OR REPLACE into stock_limit |
| `save_indicators(ts_code, records)` | INSERT OR REPLACE into daily_indicator |
| `save_shares(ts_code, records)` | INSERT OR REPLACE into stock_share |

### Record types

```rust
pub struct DailyRecord { trade_date, open, high, low, close, change, pct_chg, vol, amount }
pub struct AdjFactorRecord { trade_date, adj_factor }
pub struct StatusRecord { trade_date, is_open }
pub struct LimitRecord { trade_date, up_limit, down_limit }
pub struct IndicatorRecord { trade_date, turnover_rate, turnover_rate_f, volume_ratio, pe, pe_ttm, pb, ps }
pub struct ShareRecord { trade_date, total_share, float_share, free_share, total_mv, circ_mv }
pub struct StockBasic { ts_code, symbol, name, area, industry, market, exchange, list_date, delist_date }
```

### NegativeCache

| Method | Query |
|---|---|
| `mark_no_data(symbol, timeframe)` | `INSERT OR REPLACE INTO no_data_marks` with current timestamp |
| `is_no_data(symbol, timeframe, now_ts, ttl_secs)` | `SELECT 1 FROM no_data_marks WHERE … AND last_checked >= ?` |

TTL defaults to 7 days. Stale entries (>TTL) are considered expired (returns `false`).

## Baostock integration (`src/bin/downloader/baostock.rs`)

Python subprocess for fetching adjustment factors.

### Script: `scripts/fetch_adj_factor.py`

```python
import baostock as bs, json, sys
bs.login()
rs = bs.query_adjust_factor(code, start_date, end_date)
# outputs JSON array of [{trade_date, adj_factor}] to stdout
```

### Rust integration

```rust
pub async fn fetch_adj_factors(code, start_date, end_date) -> Result<Vec<AdjFactor>>
```

- Spawns `python3 scripts/fetch_adj_factor.py <code> <start> <end>`
- Reads stdout, parses JSON array
- Returns `Vec<AdjFactor { trade_date: String, adj_factor: f64 }>`
- Writes to `stock_adj_factor` via `DuckDbProvider::save_adj_factors()`

## Error type

```rust
enum DataError {
    Network(reqwest::Error),        // HTTP failures
    Database(duckdb::Error),        // DuckDB failures
    Parse(String),                  // JSON/date parsing, mutex poison
    RateLimited(u64),               // EastMoney rate limit
    NoData { symbol: String },      // API returned no klines
}
```

`DataError` implements `From<reqwest::Error>` and `From<duckdb::Error>` for
ergonomic `?` propagation.

## Config

```toml
[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10
retry_count = 3

[database]
path = "compass.db"     # DuckDB database file (not SQLite — the config key remains unchanged)
```
