# Chart App (GUI)

## Launching

```sh
cargo run
```

The app opens a 1280×720 dark-themed window titled "Compass — Stock Chart".

## Interface

### Top bar

| Control | What it does |
|---|---|
| **Symbol** | Text field — type a 6-digit stock code |
| **Timeframe** | Dropdown — currently only `1d` (daily) |
| **Fetch** | Button — load chart data for the symbol |

### Status indicators

| Indicator | Meaning |
|---|---|
| **Loading...** | Data is being fetched from local cache or EastMoney |
| **Red text** | An error occurred (network, no data, etc.) |

### Chart area

The chart displays candlestick bars with:

- **Pan**: click and drag horizontally
- **Zoom**: scroll wheel
- **Crosshair**: hover over a candle for OHLCV details
- **Visible bars**: 100 bars shown by default

## How data flows

When you click "Fetch":

1. **Check cache** — if this stock was viewed before, bars load instantly from local DuckDB
2. **Fetch online** — if not cached, calls EastMoney API (requires internet)
3. **Save to cache** — downloaded bars are saved for next time
4. **Display chart** — bars appear as candlesticks

First view of a stock requires a network call (~1–3 seconds). Subsequent
views are instant (no network).

## Stock codes

Enter a 6-digit A-share code:

| Code | Stock |
|---|---|
| `000001` | 平安银行 (Shenzhen) |
| `600519` | 贵州茅台 (Shanghai) |
| `688001` | 华兴源创 (科创板, Shanghai) |
| `300750` | 宁德时代 (ChiNext, Shenzhen) |
| `830799` | 艾融软件 (北交所, Beijing) |

For Shanghai indices, prefix with `sh.`:

| Input | What you get |
|---|---|
| `000001` | 平安银行 (SZ stock — default) |
| `sh.000001` | 上证指数 (SH index) |

See [kb/design/symbols.md](../design/symbols.md) for the complete code table.

## Configuring defaults

Create `~/.config/compass/config.toml` to set the startup symbol and timeframe:

```toml
[app]
default_symbol = "600519"
default_timeframe = "1d"
```

See [Config](config.md) for all options.

## Data prerequisites

The chart app reads from the local DuckDB cache (`compass.db`). Before first use,
you need data available:

```sh
# Option A: Import from Dolt (complete history)
cargo run --bin compass-data -- import
cargo run --bin compass-data -- export   # → compass.db

# Option B: Download from EastMoney (specific stocks)
cargo run --bin compass-data -- download --symbols 000001,600519
```

If no local data exists, the app falls back to fetching from EastMoney online
on each "Fetch" click.
