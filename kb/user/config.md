# 配置文件

Compass 启动时从 `~/.config/compass/config.toml` 读取配置。所有字段均为可选 — 缺失的键回退到合理的默认值。

## 文件位置

```sh
~/.config/compass/config.toml
```

如果文件和目录不存在，请创建：

```sh
mkdir -p ~/.config/compass
```

## 完整配置项

```toml
# 主题预设（顶层键，不属于任何节）。有效值："compass_dark" | "compass_light"。
# 默认值："compass_dark"
theme = "compass_dark"

[parquet]
# 包含 stock_basic.parquet 和 stock_daily.parquet 的文件夹。
# 默认值："/data/compass-data/parquet_data"
dir = "/data/compass-data/parquet_data"

[dolt]
# Dolt investment_data 仓库目录（主要 OHLCV 数据源）。
# 默认值："/data/compass-data/investment_data"
investment_data_dir = "/data/compass-data/investment_data"

# Dolt compass_data 仓库目录（基本面、自定义数据）。
# 默认值："/data/compass-data/compass_data"
compass_data_dir = "/data/compass-data/compass_data"

[app]
# 应用启动时显示的股票代码。
# 默认值："000001"
default_symbol = "000001"

# 应用启动时显示的时间周期。
# 默认值："1d"
default_timeframe = "1d"
```

## 默认值

如果配置文件不存在或无法解析，将应用以下默认值：

| 节 | 键 | 默认值 |
|---|---|---|
| （顶层） | `theme` | `compass_dark` |
| `parquet` | `dir` | `/data/compass-data/parquet_data` |
| `dolt` | `investment_data_dir` | `/data/compass-data/investment_data` |
| `dolt` | `compass_data_dir` | `/data/compass-data/compass_data` |
| `app` | `default_symbol` | `000001` |
| `app` | `default_timeframe` | `1d` |

## 配置示例

### 修改默认股票为 贵州茅台

```toml
[app]
default_symbol = "600519"
```

部分配置也能工作 — 仅覆盖你指定的键：

```toml
[app]
default_symbol = "600519"
# default_timeframe 保持 "1d"（默认值）
```

### 主题预设

```toml
theme = "compass_light"
```

从两种内置视觉主题中选择：`compass_dark`（默认）或 `compass_light`。

### 自定义数据目录

```toml
[parquet]
dir = "/mnt/data/parquet_data"

[dolt]
investment_data_dir = "/mnt/data/investment_data"
compass_data_dir = "/mnt/data/compass_data"
```

## 验证

配置文件在启动时被验证。如果解析失败，将记录一条警告，并使用全部默认值。无效的主题值（两种有效预设之外的任何值）回退到 `compass_dark`。查看日志获取详情：

```sh
RUST_LOG=info cargo run 2>&1 | grep config
```
