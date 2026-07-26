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
[database]
# Path to the parquet_data directory containing OHLCV data.
# Default: "parquet_data"
parquet_dir = "parquet_data"

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
| `database` | `parquet_dir` | `parquet_data` |
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

### Increase API timeout for slow connections

```toml
[api]
timeout_secs = 30
```

### Custom database location

```toml
[database]
path = "/data/compass/cache.duckdb"
```

## Validation

The config is validated at startup. If parsing fails, a warning is logged and
all defaults are used. Check the logs for details:

```sh
RUST_LOG=info cargo run 2>&1 | grep config
```
