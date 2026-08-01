# 数据管线（CLI）

## 概述

`compass-data` 通过四个子命令管理 A 股 OHLCV 数据：

```
Dolt investment_data ──import─────────► parquet_data/
Dolt compass_data ────import-compass──► parquet_data/
parquet_data/ ────────export──────────► duckdb / csv / parquet-dir
parquet_data/ ────────backup──────────► 百度云（zip）
```

东方财富数据由 Python 采集器（`collectors/`）获取，存入 Dolt `compass_data`，再通过 `import-compass` 导入。Rust CLI 本身从不与东方财富通信。

## 通用选项

- **`--overwrite`**（用于 `import-compass` 和 `export`）：用新值替换已有数据。默认行为是合并/跳过 — 已有数据保留，仅添加新数据。
- `import`（Dolt investment_data）始终直接写入完整数据集 — 没有 `--overwrite` 标志。

---

## `import` — Dolt investment_data → Parquet（主要）

从本地 Dolt `investment_data` 数据库导入完整历史数据到 Parquet 主数据库。

```sh
cargo run --bin compass-data -- import [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--dolt-dir` | 来自配置 `[dolt].investment_data_dir` | Dolt 数据库目录 |
| `--output` | 来自配置 `[parquet].dir` | Parquet 文件输出目录 |
| `--symbols` | （全部） | 逗号分隔的 6 位代码（如 `000001,600519`） |
| `--limit` | `0`（全部） | 最大导入股票数量 |
| `--start-date` | （最早） | 按起始日期过滤（YYYYMMDD） |
| `--end-date` | （最晚） | 按截止日期过滤（YYYYMMDD） |
| `--since` | （无） | 增量导入：仅导入 tradedate >= since 的数据（YYYYMMDD） |

导入过程通过 `dolt sql -r parquet`（直接二进制 Parquet）读取每只股票的行数据，写入单一的 `stock_daily.parquet` 文件。再次运行会重新导入完整数据集。

### 输出结构

```
parquet_data/
├── stock_basic.parquet             # 股票元数据（由 import-compass --table stock_basic 生成）
├── stock_daily.parquet             # OHLCV 数据（单文件，含 symbol 列）
└── stock_daily.symbols.txt         # 股票索引（每行一个）
```

`stock_daily.parquet` 中的 `symbol` 列存储 Dolt 原生的股票代码格式（如 `SZ000001`、`SH600519`）。共享同一 6 位代码的股票（SZ）和指数（SH）通过交易所前缀区分。

### 示例

```sh
# 全量导入（全部 6000+ 只股票，约 1 小时）
cargo run --bin compass-data -- import

# 导入指定股票
cargo run --bin compass-data -- import --symbols 000001,600519

# 带日期过滤的导入
cargo run --bin compass-data -- import --start-date 20200101 --end-date 20250721

# 导入前 100 只股票（测试用）
cargo run --bin compass-data -- import --limit 100

# 增量导入：仅导入 2026-07-25 以来的数据
cargo run --bin compass-data -- import --since 20260725
```

---

## `import-compass` — Dolt compass_data → Parquet

从我们自己的 `compass_data` Dolt 仓库导入表（公司概况、财务指标、资产负债表、利润表、现金流量表）到 Parquet。

```sh
cargo run --bin compass-data -- import-compass --table <table> [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--table` | （必填） | `stock_basic`、`fin_indicators`、`fin_balance_sheet`、`fin_income`、`fin_cash_flow` |
| `--dolt-dir` | 来自配置 `[dolt].compass_data_dir` | Dolt 数据库目录 |
| `--output` | 来自配置 `[parquet].dir` | Parquet 文件输出目录 |
| `--overwrite` | `false` | 替换已有数据而非合并 |
| `--since` | （无） | 增量导入：仅导入 report_date >= since 的数据（YYYYMMDD） |

### 示例

```sh
# 导入公司概况
cargo run --bin compass-data -- import-compass --table stock_basic

# 导入财务指标（增量）
cargo run --bin compass-data -- import-compass --table fin_indicators --since 20260101

# 强制覆盖
cargo run --bin compass-data -- import-compass --table stock_basic --overwrite
```

---

## `export` — Parquet → DuckDB

将 Parquet 主数据库导出为 DuckDB 文件（GUI 的读库路径）。

```sh
cargo run --bin compass-data -- export [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--input` | 来自配置 `[parquet].dir` | Parquet 数据目录 |
| `--format` | `duckdb` | 输出格式（当前仅实现 `duckdb`；其他值会警告并跳过） |
| `--output` | `/data/compass-data/compass.duckdb` | 输出路径 |
| `--overwrite` | `false` | 替换已有数据而非跳过 |

### 示例

```sh
# 导出到 DuckDB
cargo run --bin compass-data -- export

# 强制覆盖
cargo run --bin compass-data -- export --overwrite
```

---

## `backup` — Parquet → 百度云

将 `parquet_data/` 打包为 zip 并通过 `baidupcs`（BaiduPCS-Go）上传到百度云。

```sh
cargo run --bin compass-data -- backup [OPTIONS]
```

| 选项 | 默认值 | 说明 |
|---|---|---|
| `--input` | 来自配置 `[parquet].dir` | 要备份的 Parquet 数据目录 |
| `--keep-zip` | `false` | 上传后保留本地 zip 文件 |

- 文件名带时间戳：`parquet_data-YYYYMMDD-HHMMSS.zip`
- 目标文件夹：百度云上的 `/compass/`
- 独立脚本：`scripts/upload-parquet.sh [--keep-zip]`

---

## Python 采集器（数据源 → Dolt）

`collectors/` 目录包含 Python 脚本（uv + curl_cffi），从各数据源获取数据并存入 CSV，再导入 Dolt `compass_data`。财务表仍来自东方财富；`stock_basic` 已切换到三大交易所官网：

```sh
cd collectors/
uv sync                                # 首次：安装依赖

uv run python main.py fetch stock_basic   # 上交所/深交所/北交所官网
uv run python main.py fetch fin_indicators
uv run python main.py sync             # 获取 + 导入全部
uv run python main.py sync-investment --restart
```

关键概念：
- **curl_cffi** 用于 TLS 伪装（东方财富反爬虫；BSE 官网需要携带会话 cookie）
- **CSV 作为中间格式**，连接 API 与 Dolt
- **`.state.json`** 文件跟踪上次获取时间，用于增量更新

`fetch stock_basic` 现在运行 `fetch_stock_basic_official.py`，从三大交易所官网
（SSE/SZSE/BSE）抓取股票基本信息，输出 `stock_basic_official.csv`。旧的东财采集器
`fetch_stock_basic.py` 仍保留但不再用于 stock_basic——其 EM_FS m:0+t:81 段混入
6841 只新三板/老三板股票。原先为东财分页设计的 `--resume` / `--max-pages` 标志已移除。

采集器管线的完整描述见 `kb/design/architecture.md`。

---

## 典型工作流

### 首次设置（从 Dolt）

```sh
# 1. 从 Dolt investment_data 导入全部数据
cargo run --bin compass-data -- import

# 2. 从 Dolt compass_data 导入公司概况
cargo run --bin compass-data -- import-compass --table stock_basic

# 3. 启动图表应用
scripts/run.sh
```

### 从东方财富获取新数据

```sh
# 1. 将最新数据获取到 Dolt compass_data（Python 采集器）
cd collectors/
uv run python main.py sync

# 2. 将新表导入到 Parquet
cargo run --bin compass-data -- import-compass --table fin_indicators --since 20260101
```

### 备份到百度云

```sh
cargo run --bin compass-data -- backup            # 上传 zip
cargo run --bin compass-data -- backup --keep-zip # 上传后保留本地 zip
```

---

## 排障

### 速率限制（采集器）

东方财富会限制激进请求。在 `collectors/` 中，降低并发并增加延迟：

```sh
uv run python main.py sync --concurrency 1 --delay-ms 3000
```

### Dolt 未找到

```sh
# 验证 Dolt 已安装且 investment_data/ 存在
dolt --data-dir=investment_data sql -q "SELECT COUNT(*) FROM final_a_stock_eod_price"
```

### 导入速度慢

导入过程查询 Dolt 6000 多次 — 每次查询约 0.5 秒。总时间由 Dolt 查询速度决定，而非文件 I/O。日期过滤可加速：

```sh
cargo run --bin compass-data -- import --start-date 20240101
```

### 日志

设置 `RUST_LOG` 以获取详细输出：

```sh
RUST_LOG=debug cargo run --bin compass-data -- import
```

日志输出到 stderr 和 `logs/compass.log`（按日滚动）。
