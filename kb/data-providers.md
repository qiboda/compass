# Data Providers

## Provider stack

```
CachedProvider<R: DataProvider, W: DataWriter>
    ├── reader: EastMoneyProvider  (remote HTTP)
    ├── cache:  SqliteProvider     (local read)
    └── writer: SqliteProvider     (local write-through)
```

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
```

## EastMoneyProvider (`src/data/eastmoney.rs`)

### Fetch bars

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

### Search symbols

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

## SqliteProvider (`src/data/sqlite.rs`)

Implements both `DataProvider` and `DataWriter` on the same struct.
Uses `Arc<Mutex<Connection>>` internally — one connection, one writer at a time.

### Read path (DataProvider::fetch_bars)

Uses `tokio::task::spawn_blocking` to avoid blocking the async worker.
Query:

```sql
SELECT timestamp, open, high, low, close, volume
FROM bars
WHERE symbol = ?1 AND timeframe = ?2
  AND timestamp >= ?3 AND timestamp <= ?4
ORDER BY timestamp ASC
```

### Write path (DataWriter::save_bars)

Also uses `spawn_blocking`. Uses `INSERT OR REPLACE` (upsert):

```sql
INSERT OR REPLACE INTO bars
    (symbol, timeframe, timestamp, open, high, low, close, volume, adj_type, adj_factor, status)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'none', 0.0, 'normal')
```

## Error type

```rust
enum DataError {
    Network(reqwest::Error),      // HTTP failures
    Database(rusqlite::Error),    // SQLite failures
    Parse(String),                // JSON/date parsing, mutex poison
    RateLimited(u64),             // EastMoney rate limit (unused)
    NoData { symbol: String },    // API returned no klines
}
```

`DataError` implements `From<reqwest::Error>` and `From<rusqlite::Error>` for
ergonomic `?` propagation.

## Config

```toml
[api]
base_url = "https://push2his.eastmoney.com"
timeout_secs = 10
retry_count = 3        # not yet wired into retry logic

[database]
path = "compass.db"
```
