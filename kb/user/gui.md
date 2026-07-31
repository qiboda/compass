# Chart App (GUI)

## Launching

```sh
cargo run
```

The app opens a 1280×720 dark-themed window titled "Compass — Stock Chart".

## Interface

### Toolbar

The top toolbar provides all controls in a single row:

| Control | Icon | What it does |
|---|---|---|---|
| **Symbol** | 🔍 | Searchable dropdown — type a code prefix (e.g. `600`) or name substring (e.g. `平安`) to filter the list. Displays `EXCHANGE \| CODE \| NAME` format. Click to select. |
| **Exchange** | 🏛 | Dropdown — filter by `全部`/`SH`/`SZ`/`BJ`. Narrows the symbol list to the selected exchange. |
| **TF** | ⏱ | ComboBox — select `1d` (daily), `1w` (weekly), or `1M` (monthly). Controls OHLCV bar aggregation. |
| **Fetch** | ⬇ | Button — load chart data for the selected symbol from the local database. |
| **Theme** | 🎨 | Button — opens a dropdown to switch between `compass_dark` and `compass_light` presets. Applies globally to all UI elements. |

### Status indicators

Status messages appear as **toast notifications** in the top-right corner of the window.

| Type | Icon | Meaning | Auto-dismiss |
|---|---|---|---|
| **Loading** | ⏳ | Data is being loaded from the local database | — (persists until complete) |
| **Success** | ✅ | Operation completed (fetch, import, export) | 3s |
| **Warning** | ⚠ | Non-critical issue (e.g., stale data) | 5s |
| **Error** | ❌ | An error occurred (network, no data, invalid symbol) | 8s |

Toasts use Phosphor icon glyphs. Messages stack vertically; older toasts fade out as new ones appear.

### Chart area

The chart displays candlestick bars with:

- **Pan**: click and drag horizontally
- **Zoom**: scroll wheel
- **Crosshair**: hover over a candle for OHLCV details
- **Visible bars**: 100 bars shown by default

### Logger

A scrollable log panel shows fetch status, errors, and citizen lifecycle events.

### Theme switching

Three built-in visual themes are available:

| Preset | Description |
|---|---|
| `compass_dark` | Default dark theme (TradingView-style) |
| `compass_light` | Light theme for daytime use |
| `compass_blue` | Blue-tinted theme |

Click the **🎨 (PALETTE)** button in the toolbar to open a dropdown and select a
theme. The change applies instantly to all UI elements — chart background,
toolbar, panels, buttons, and text colors. No restart required.

The active theme is persisted to `~/.config/compass/config.toml` under
`[app].theme` and restored on next launch.

### Toast notifications

Status feedback appears as transient toast notifications anchored to the
**top-right** corner of the window. Each toast shows a Phosphor icon glyph,
a brief message, and auto-dismisses after a preset duration.

| Type | Icon | Dismiss | Example |
|---|---|---|---|
| Success | ✅ | 3 seconds | "Data loaded: sh.600519 (100 bars)" |
| Warning | ⚠ | 3 seconds | "No data available for this date range" |
| Error | ❌ | 8 seconds | "Network error: connection timeout" |
| Info | ℹ | 3 seconds | "Import complete: 2,430 records" |

Toasts stack vertically; up to 5 are visible at once. Older notifications slide
up and fade out to make room for new ones. Clicking a toast dismisses it
immediately.

### Modal dialogs

Modal dialogs are used for actions that require user confirmation:

- **Fullscreen overlay** — dark semi-transparent backdrop prevents interaction with the rest of the UI
- **Centered panel** — white/theme-colored dialog box with title, message, and action buttons
- **OK / Cancel** — standard confirmation pattern; Escape key equals Cancel
- **Focus trapping** — keyboard and mouse input is confined to the dialog while open

Modals appear for destructive actions (e.g., overwriting data, clearing cache)
and for import/export confirmations.

### File dialog

File operations use `egui-file-dialog` for native-feeling file selection:

- **Import** — choose a `.parquet` or `.csv` file to import into the local database
- **Export** — choose a destination directory and filename for exporting chart data
- **Navigation** — browse the filesystem with standard directory tree, file list, and path breadcrumbs
- **Filters** — file type filters show only relevant files (Parquet, CSV) by default

## How data flows

When you click "Fetch":

1. The selected exchange prefixes the symbol (e.g., `sh.600519`)
2. **Query local Parquet** — `DuckDbProvider` reads `stock_daily.parquet` directly via DuckDB's `read_parquet()`
3. **Display chart** — bars appear as candlesticks

The GUI is **local-only**: it never calls EastMoney or any online API. All data
comes from the local Parquet main database. If a symbol isn't in the database,
the GUI shows a "no data" message — use the data pipeline (`compass-data import`
or the Python collectors) to bring data in first.

## Stock codes

Select from the dropdown or type to search:

| Code | Stock | Exchange |
|---|---|---|
| `000001` | 平安银行 | SZ |
| `600519` | 贵州茅台 | SH |
| `688001` | 华兴源创 | SH |
| `300750` | 宁德时代 | SZ |
| `830799` | 艾融软件 | BJ |

The exchange dropdown filters the symbol list. When an exchange is selected
(SH/SZ/BJ), the symbol code is auto-prefixed (e.g., `sh.600519`) before
fetching. With "全部" selected, no prefix is added.

## Configuring defaults

Create `~/.config/compass/config.toml` to set startup preferences:

```toml
[app]
default_symbol = "600519"
default_timeframe = "1d"

theme = "compass_light"
```

See [Config](config.md) for all options.

## Data prerequisites

The chart app reads OHLCV data directly from `parquet_data/stock_daily.parquet`
via DuckDB's `read_parquet()` (in-memory, no persistent DuckDB file needed).
Before first use, ensure data is available:

```sh
# Import from Dolt (complete history)
cargo run --bin compass-data -- import
# Data is ready — parquet_data/stock_daily.parquet is the source of truth
```

If a symbol isn't in the local database, the GUI shows a "no data" message —
there is no online fallback. Import the missing data first.

For the symbol dropdown to be populated, `stock_basic.parquet` must exist in the
parquet data directory (default: `parquet_data/`). This file is created by
`compass-data import` (or `import-compass --table stock_basic`).
