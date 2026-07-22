# Data Providers

## Provider stack

```
CachedProvider<R: DataProvider, C: DataProvider+NegativeCache+DataWriter>
    ├── reader: EastMoneyProvider  (remote HTTP)
    └── cache:  DuckDbProvider     (local read + write-through + negative cache)
```

The GUI uses `CachedProvider`. The CLI (`compass-data download`) uses
`EastMoneyProvider` and `DuckDbProvider` directly without CachedProvider.

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

Source: `https://push2his.eastmoney.com` (K-line) and
`https://push2delay.eastmoney.com` (symbol listing, stock info, realtime).

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

HTTP GET to `{realtime_base_url}/api/qt/clist/get`. Uses `keyword` parameter for
fuzzy matching. Best-effort: any parse or network error returns empty `Vec`.

### Search all symbols (`search_all_symbols`)

Paginated symbol listing. Correct fs filter for all A-shares:

```
fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81+s:2048
```

Parameters include `ut=bd1d9ddb04089700cf9c27f6f7426281` (static token).
Page size capped at 100 by the API. Uses `data.total` field for pagination tracking.

### Fetch stock basic (`fetch_stock_basic`)

HTTP GET to `{realtime_base_url}/api/qt/clist/get`. Paginates through
A-share stocks to find the target code. Returns `StockBasic` with:

| Field | JSON key | Description |
|---|---|---|
| `symbol` | `f12` | Stock code (e.g. `"600519"`) |
| `name` | `f14` | Stock display name (e.g. `"贵州茅台"`) |
| `industry` | `f100` | Industry classification |
| `market` | `f102` | Market segment (e.g. `"沪主板"`) |
| `list_date` | `f124` | Unix timestamp → `NaiveDate`; `-1` means unknown |

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

## DuckDbProvider (`src/data/duckdb.rs`)

Implements `DataProvider`, `DataWriter`, and `NegativeCache`. Uses
`Arc<Mutex<Connection>>` internally. Serves as staging database for
`compass-data download` and cache for GUI.

### Schema (4 tables + no_data_marks)

| Table | Purpose | Key |
|---|---|---|
| `stock_daily` | Core OHLCV bars | `(symbol, trade_date)` |
| `stock_adj_factor` | Adjustment factors from Baostock | `(symbol, trade_date)` |
| `stock_basic` | Stock name, industry, exchange, list date | `symbol` |
| `stock_limit` | Daily price limits (涨停/跌停) | `(symbol, trade_date)` |
| `no_data_marks` | Negative cache entries with TTL | `(symbol, timeframe)` |

### Read path (`DataProvider::fetch_bars`)

```sql
SELECT CAST(trade_date AS VARCHAR), open, high, low, close, volume
FROM stock_daily
WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
ORDER BY trade_date ASC
```

### Write path (`DataWriter::save_bars`)

```sql
INSERT OR REPLACE INTO stock_daily
    (symbol, trade_date, open, high, low, close, volume)
VALUES (?, ?, ?, ?, ?, ?, ?)
```

### Per-table methods (non-trait, for CLI downloader)

| Method | Operation |
|---|---|
| `save_stock_daily(symbol, records)` | INSERT OR REPLACE into stock_daily |
| `get_stored_range(symbol)` | MIN/MAX trade_date for gap detection |
| `save_adj_factors(symbol, factors)` | INSERT OR REPLACE into stock_adj_factor |
| `get_adj_factor_range(symbol)` | MIN/MAX trade_date for adj_factor |
| `upsert_stock_basic(info)` | INSERT OR REPLACE into stock_basic |
| `get_stock_basic(symbol)` | Read single stock_basic record |
| `save_limits(symbol, records)` | INSERT OR REPLACE into stock_limit |

### Record types

```rust
pub struct DailyRecord { trade_date, open, high, low, close, adjclose, volume, amount }
pub struct AdjFactorRecord { trade_date, adj_factor }
pub struct LimitRecord { trade_date, up_limit, down_limit }
pub struct StockBasic { symbol, name, area, industry, market, exchange, list_date, delist_date }
```

## ParquetReader (`src/data/parquet.rs`)

Reads Parquet files directly via DuckDB `read_parquet()`. Implements `DataProvider`.
This is the primary data source once Dolt import is complete — no native DuckDB
tables needed.

### Directory layout

```
parquet_data/
├── stock_basic.parquet
└── stock_daily/
    ├── 000001.parquet     # One file per symbol
    ├── 600519.parquet
    └── ...
```

### Methods

| Method | Implementation |
|---|---|
| `fetch_bars(symbol, start, end)` | `SELECT ... FROM read_parquet('{symbol}.parquet') WHERE ...` |
| `search_symbols(query)` | Client-side filter over filesystem scan |
| `list_symbols()` | `std::fs::read_dir(parquet_data/stock_daily/)` |
| `get_stored_range(symbol)` | `SELECT MIN/MAX(tradedate) FROM read_parquet(...)` |
| `get_stock_basic(symbol)` | `SELECT ... FROM read_parquet('stock_basic.parquet') WHERE symbol = ?` |

### Threading

`fetch_bars` uses `tokio::task::spawn_blocking` with a clone of `Arc<Mutex<Connection>>`.
Other methods are synchronous (filesystem scanning is fast).

## Dolt → Parquet import (`src/bin/data/import_dolt.rs`)

Reads from Dolt `investment_data` database via the `dolt` CLI:

1. `dolt sql -r csv -q "SELECT DISTINCT symbol FROM final_a_stock_eod_price"`
2. Per symbol: `dolt sql -r csv` → temp CSV → DuckDB `read_csv` → `COPY ... TO '{symbol}.parquet'`
3. Strips SH/SZ/BJ prefix from Dolt symbols (e.g. `SZ000001` → `000001`)

Source table: `final_a_stock_eod_price` (18.25M rows, 6122 stocks, 1990–2026).

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
path = "compass.duckdb"
```
