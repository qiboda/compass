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

[watchlist]
# 自选股（Sidebar 左侧栏）。带交易所前缀的代码列表（如 "SH600519"）。
# 由 GUI 在添加/移除自选时自动写回，重启后恢复。
# symbols = ["SH600519", "SZ000002"]

[screener]
# 选股器条件（Screener tab）。全部可选——缺省键用默认值。
# 由 GUI 在每次点击"筛选"时自动写回，重启后恢复。
# industries = ["白酒", "银行"]          # 行业多选（OR），空 = 不限
# exchanges = ["SH", "SZ"]               # 交易所多选，空 = 不限
# boards = ["主板"]                      # 板块多选，空 = 不限
# list_years = 3                        # 上市时长下限（年），缺省 = 不限
# market_cap_min = 100.0                # 市值下限（亿元）
# market_cap_max = 5000.0               # 市值上限（亿元）
# exclude_delisted = true               # 排除退市（默认 true）
# ma = "bullish_align"                  # 均线：above_ma20 / above_ma60 / bullish_align
# breakout = { days = 60 }              # N 日新高
# momentum = { days = 20, min_pct = 0.0, max_pct = 100.0 }  # 动量区间
# volume = { days = 20, times = 2.0 }   # 量能：近 N 日均量 ≥ 倍数 × 近 3N 日均量

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
# 应用启动时显示的股票代码（带交易所前缀）。
# 默认值："SZ000001"
default_symbol = "SZ000001"

# 应用启动时显示的时间周期。
# 默认值："1d"
default_timeframe = "1d"
```

## 默认值

如果配置文件不存在或无法解析，将应用以下默认值：

| 节 | 键 | 默认值 |
|---|---|---|
| （顶层） | `theme` | `compass_dark` |
| `watchlist` | `symbols` | `[]`（空自选） |
| `parquet` | `dir` | `/data/compass-data/parquet_data` |
| `dolt` | `investment_data_dir` | `/data/compass-data/investment_data` |
| `dolt` | `compass_data_dir` | `/data/compass-data/compass_data` |
| `app` | `default_symbol` | `SZ000001` |
| `app` | `default_timeframe` | `1d` |

## 配置示例

### 修改默认股票为 贵州茅台

```toml
[app]
default_symbol = "SH600519"
```

部分配置也能工作 — 仅覆盖你指定的键：

```toml
[app]
default_symbol = "SH600519"
# default_timeframe 保持 "1d"（默认值）
```

### 主题预设

```toml
theme = "compass_light"
```

从两种内置视觉主题中选择：`compass_dark`（默认）或 `compass_light`。

### 自选股（watchlist）

```toml
[watchlist]
symbols = ["SH600519", "SZ000002"]
```

左侧自选栏的股票列表，按代码升序。GUI 在侧边栏点 ＋ 添加、点 × 并确认移除时
自动写回；也可手动编辑。缺失该节 = 空自选。

### 旧格式自动迁移（D10，issue #181）

符号前缀规范化（issue #181）之前，配置中可以写裸 6 位码。加载配置时，
文件中的裸码值会被**自动迁移**为带前缀形式并回写文件（回写失败仅警告，
不阻断启动）：

- 6 开头 → `SH`（如 `600519` → `SH600519`）
- 8 开头、43 开头、92 开头 → `BJ`（如 `830799` → `BJ830799`、`430047` → `BJ430047`、`920001` → `BJ920001`）
- 其余 → `SZ`（如 `000001` → `SZ000001`）
- dot 形式直接规范化（如 `sh.000001` → `SH000001`）

仅迁移**文件中的值**；内存默认值（`SZ000001`）不迁移。新配置建议直接写
带前缀形式。

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
RUST_LOG=info scripts/run.sh 2>&1 | grep config
```
