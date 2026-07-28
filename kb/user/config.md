# Configuration

Compass reads configuration from `~/.config/compass/config.toml` at startup.
All fields are optional — missing keys fall back to sensible defaults.

## Location

```sh
~/.config/compass/config.toml
```

Create the file and directory if they don't exist:

```sh
mkdir -p ~/.config/compass
```

## Full schema

```toml
[parquet]
# Directory containing stock_basic.parquet and stock_daily/.
# Used by the GUI and CLI (import, export, backup).
# Default: "/data/compass-data/parquet_data"
dir = "/data/compass-data/parquet_data"


[dolt]
# Dolt repository directories used by the data pipeline CLI.
# Default: "/data/compass-data/investment_data"
investment_data_dir = "/data/compass-data/investment_data"
# Default: "/data/compass-data/compass_data"
compass_data_dir = "/data/compass-data/compass_data"

[app]
# Stock code displayed when the app starts.
# Default: "000001"
default_symbol = "000001"

# Timeframe displayed when the app starts.
# Default: "1d"
default_timeframe = "1d"
```

## Defaults

If the config file doesn't exist or can't be parsed, these defaults apply:

| Section | Key | Default |
|---|---|---|
│ `parquet` | `dir` | `/data/compass-data/parquet_data` |
| `dolt` | `investment_data_dir` | `/data/compass-data/investment_data` |
| `dolt` | `compass_data_dir` | `/data/compass-data/compass_data` |
| `app` | `default_symbol` | `000001` |
| `app` | `default_timeframe` | `1d` |

## Examples

### Change default stock to 贵州茅台

```toml
[app]
default_symbol = "600519"
```

A partial config works — only the keys you specify are overridden:

```toml
[app]
default_symbol = "600519"
# default_timeframe stays "1d" (default)
```

### Custom data directory locations

```toml
[parquet]
dir = "/mnt/data/compass/parquet_data"

[dolt]
investment_data_dir = "/mnt/data/compass/investment_data"
compass_data_dir = "/mnt/data/compass/compass_data"
```

## Validation

The config is validated at startup. If parsing fails, a warning is logged and
all defaults are used. Check the logs for details:

```sh
RUST_LOG=info cargo run 2>&1 | grep config
```
