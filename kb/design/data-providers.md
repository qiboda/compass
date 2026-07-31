# 数据提供者

Compass 将所有股票数据访问抽象在 **trait 体系**之后。这样可以在不修改消费者代码的情况下切换后端 — GUI 直接使用 `DuckDbProvider`，只需调用 `provider.fetch_bars()`。

## 为什么使用 trait？

两个问题要求抽象：

1. **多数据源**：Compass 从 Parquet（主数据库）和 DuckDB（内存缓存）读取数据。如果没有统一的接口，每个消费者都需要知道自己在跟哪个后端打交道。

2. **可测试性**：单元测试可以提供返回预定义数据的 mock 实现，避免依赖真实数据库。

## 三个 trait

```rust
#[async_trait]
pub trait DataProvider: Send + Sync {
    async fn fetch_bars(&self, symbol, timeframe, range_start, range_end)
        -> Result<Vec<Bar>, DataError>;
    async fn search_symbols(&self, query: &str)
        -> Result<Vec<SymbolInfo>, DataError>;
}

#[async_trait]
pub trait DataWriter: Send + Sync {
    async fn save_bars(&self, symbol, timeframe, bars: &[Bar], overwrite: bool)
        -> Result<(), DataError>;
}

#[async_trait]
pub trait NegativeCache: Send + Sync {
    async fn mark_no_data(&self, symbol: &str, timeframe: &str)
        -> Result<(), DataError>;
    async fn is_no_data(&self, symbol, timeframe, now_ts, ttl_secs)
        -> Result<bool, DataError>;
}
```

### DataProvider — 只读访问
核心获取接口。任何能为给定的标的/时间周期/日期范围生成 `Vec<Bar>` 的类型都可以实现此 trait。目前已实现的有：DuckDbProvider、ParquetReader，以及测试中的 mock 实现。

`search_symbols` 是辅助功能 — 为 GUI 中的标的搜索框提供数据。返回匹配查询的 `SymbolInfo { code, name }` 列表。

### DataWriter — 直写持久化
在获取数据后调用，将 bar 持久化到本地。
`overwrite` 标志控制行为：`false` = INSERT OR IGNORE（跳过重复键），`true` = INSERT OR REPLACE（更新已有行）。只有 DuckDbProvider 实现了此 trait。

### NegativeCache — 避免重复失败
此 trait 由 DuckDbProvider 实现，存储带 TTL 时间戳的 `no_data_marks` 条目。在当前纯本地架构中，GUI 不使用它 — 它的存在是为了完整性和 CLI 分阶段工作流。

## Provider 层级

GUI 直接使用 `DuckDbProvider` — 通过 `read_parquet()` 读取 `parquet_data/stock_daily.parquet`，并使用内存 DuckDB 连接缓存最近获取的数据。所有数据均为本地数据，无在线回退。

```rust
// backend.rs: DuckDbProvider 是唯一的数据提供者
let provider = DuckDbProvider::new(parquet_dir.exists().then_some(parquet_dir))?;
provider.fetch_bars(symbol, timeframe, start, end).await
```

## DuckDbProvider — 本地缓存与导出目标

DuckDbProvider 是数据提供者中的瑞士军刀。它实现了三个 trait，同时充当 GUI 缓存和 CLI 导出目标。

### 为什么用 DuckDB 做缓存？

- **写入是核心场景**：我们需要 INSERT OR REPLACE/IGNORE 来实现幂等缓存。DuckDB 通过其 SQL 方言自然地处理这一需求。
- **读取对分析查询很快**：`SELECT ... WHERE symbol=? AND trade_date BETWEEN ? AND ? ORDER BY trade_date` — 一个教科书式的 OLAP 查询。
- **零配置**：数据库文件在首次打开时自动创建。无需 schema 迁移，无需配置。
- **测试用内存模式**：`DuckDbProvider::new_in_memory()` 为每个测试提供完全隔离的数据库。无需清理，无干扰。

### 线程模型

DuckDB 的 Rust 绑定封装了一个同步 C 库。每个数据库操作都通过 `tokio::task::spawn_blocking`，将调用转移到专门的线程池：

```rust
// 在 DuckDbProvider 内部
let conn = self.conn.clone();
tokio::task::spawn_blocking(move || {
    let conn = conn.lock().unwrap();
    conn.execute("SELECT ...", params![])
}).await.unwrap()
```

连接本身是 `Arc<Mutex<Connection>>`。每个数据库同一时间只有一个查询 — 但由于查询很快（缓存读取 <1ms），争用可以忽略不计。

### Schema

五张表，均在首次使用时自动创建：

| 表 | 键 | 用途 |
|---|---|---|
| `stock_daily` | `(symbol, trade_date)` | 核心 OHLCV bar — 主缓存表 |
| `stock_basic` | `symbol` | 股票名称、行业、交易所、上市日期 |
| `stock_adj_factor` | `(symbol, trade_date)` | 价格复权因子 |
| `stock_limit` | `(symbol, trade_date)` | 每日涨跌停价格 |
| `no_data_marks` | `(symbol, timeframe)` | 带 TTL 时间戳的负缓存条目 |

DuckDB DDL（首次使用时自动创建）：

```sql
CREATE TABLE stock_daily (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    open, high, low, close, adjclose DOUBLE,
    volume, amount DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_basic (
    symbol      VARCHAR PRIMARY KEY,
    name, industry, market, exchange VARCHAR,
    list_date, delist_date DATE
);
CREATE TABLE stock_adj_factor (
    symbol, trade_date, adj_factor, PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_limit (
    symbol, trade_date, up_limit, down_limit, PRIMARY KEY (symbol, trade_date)
);
```

Parquet 主数据库布局（由 `compass-data import` 生成）：

```
parquet_data/
├── stock_basic.parquet        # symbol, name, exchange, list_date, delist_date
├── stock_daily.parquet        # symbol, tradedate, open, high, low, close, adjclose, volume, amount
└── stock_daily.symbols.txt    # 每行一个标的（快速列表）
```

完整 DDL 见 `AGENTS.md` 和 `kb/design/architecture.md`。

### 缺口检测

`get_stored_range(symbol)` 返回某只股票的 `(MIN(trade_date), MAX(trade_date))`。增量导入使用此信息来确定哪些日期范围已经被覆盖：

```
stored range:   2020-01-02 ──────────── 2024-12-31
requested:      2019-01-01 ──────────────────────── 2025-07-21
                                     ^^^^^^^^^^^^^^
                                     仅此缺口需要导入
```

这跳过了对已覆盖日期的重复导入，加速增量更新。

### 写入语义

所有写方法接受 `overwrite: bool` 参数：

| `overwrite` | SQL | 行为 |
|---|---|---|
| `false`（默认） | `INSERT OR IGNORE` | 键已存在的行跳过 |
| `true` | `INSERT OR REPLACE` | 用新值替换已有行 |

CLI 子命令（`import-compass`、`export`）使用相同的语义 — `--overwrite` 控制现有数据是保留还是替换。

### 时间周期聚合 (ref #46)

`DuckDbProvider::fetch_bars()` 支持三种时间周期：

| 时间周期 | 行为 |
|---|---|
| `"1d"` | 返回原始日线 bar — 无聚合 |
| `"1w"` | 日线 → 周线聚合：`open` = 周一开盘价，`high` = 周最高价，`low` = 周最低价，`close` = 周五收盘价，`volume` = 周成交量合计 |
| `"1M"` | 日线 → 月线聚合：相同 OHLCV 聚合，使用 `date_trunc('month', ...)` |

聚合在日线数据加载到内存 `stock_daily` 表后进行 DuckDB SQL 重新查询（包含 parquet 回退缓存预热）：

```sql
SELECT DATE_TRUNC('week', trade_date) as grp_date,
       FIRST(open) as open,
       MAX(high) as high,
       MIN(low) as low,
       LAST(close) as close,
       SUM(volume) as volume
FROM (
    SELECT * FROM stock_daily
    WHERE symbol = ? AND trade_date >= ? AND trade_date <= ?
    ORDER BY trade_date ASC
)
GROUP BY grp_date
ORDER BY grp_date
```

子查询中的 `ORDER BY trade_date ASC` 保证 `FIRST`/`LAST` 按时间顺序返回每个时间桶中最早/最晚的值。只有 DuckDB 的 `stock_daily` 路径执行聚合；`ParquetReader`（直接 parquet 读取）始终返回日线数据。

## ParquetReader — 主数据库

ParquetReader 从 `compass-data import` 生成的 Parquet 文件中读取数据。它实现了 `DataProvider` 但不实现 `DataWriter` — Parquet 存储是只追加的（数据合并到单个文件中，已有数据永远不会原地修改）。

### 工作原理

`ParquetReader` 封装了一个 DuckDB 内存连接，使用 `read_parquet()` 查询单个 Parquet 文件，通过 `WHERE symbol = ?` 过滤：

```sql
SELECT CAST(tradedate AS VARCHAR), open, high, low, close, volume
FROM read_parquet('parquet_data/stock_daily.parquet')
WHERE symbol = ? AND tradedate >= ? AND tradedate <= ?
ORDER BY tradedate ASC
```

标的行为 DuckDB 参数 (`?`) 绑定，不拼接到 SQL 字符串中。数据不加载到表中。DuckDB 每次查询时读取 Parquet 文件，利用列式投影和谓词下推提高效率。

### 标的发现

`list_symbols()` 首先检查 `stock_daily.symbols.txt`（每行一个标的，已排序），该文件由导入管线与 `stock_daily.parquet` 一起生成。这是快速路径 — 一次简单的文件读取。

如果 `symbols.txt` 不存在，则回退到 `SELECT DISTINCT symbol FROM read_parquet('stock_daily.parquet') ORDER BY symbol`。如果两个来源都不存在，返回空 vec。

`search_symbols()` 将 `stock_basic.parquet` 加载到 DuckDB 中，对 `name` 列执行 LIKE 查询。首次查询较慢（全表扫描），但内存中的 DuckDB 会在后续搜索中保持快速。

### 何时使用 ParquetReader vs DuckDbProvider

| 场景 | 使用 |
|---|---|
| GUI（日常使用，缓存数据可用） | DuckDbProvider（缓存） |
| CLI：批量数据查询 | ParquetReader |
| 测试环境 | DuckDbProvider（内存模式） |

## Dolt 导入 — 批量数据管线

Dolt 是一个兼容 MySQL 的数据库，具有类 Git 的版本控制。`investment_data` Dolt 数据库包含 A 股 EOD 价格的完整历史 — 1825 万行，6122 只股票，从 1990 年至今。

导入管线（`compass-data import`）工作方式如下：

```
dolt sql -r csv -q "SELECT DISTINCT symbol FROM final_a_stock_eod_price"
    │
    ├─ 生成 6123 个标的的列表（例如 "SZ000001"、"SH600519"）
    │     [注：标的列表查询仍使用 CSV 格式以便简单的文本解析]
    │
    ├─ 对每个标的：
    │     dolt sql -r parquet -q "SELECT * FROM final_a_stock_eod_price WHERE symbol='SZ000001'"
    │       → 二进制 Parquet 字节（无 CSV 中间层）
    │       → 直接写入 parquet_data/stock_daily.parquet（带 symbol 列的单个文件）
    │
    │       → 标的列表写入 parquet_data/stock_daily.symbols.txt
    └─ 股票基本信息 → parquet_data/stock_basic.parquet
```

导入直接写入完整数据集 — 没有合并模式，也没有 `--overwrite` 标志。重新运行会用 Dolt 的新导出替换文件。使用 `--since` 进行增量导入更新数据。

## 错误处理

### DataError 枚举

```rust
pub enum DataError {
    Network(reqwest::Error),     // HTTP 失败（超时、DNS、连接拒绝）
    Database(duckdb::Error),     // DuckDB 失败（文件损坏、磁盘满、锁）
    Parse(String),               // JSON 反序列化、日期解析、Mutex 中毒
    RateLimited(u64),            // EastMoney 限流，含重试等待秒数
    NoData { symbol: String },   // 标的无数据（已退市、无效、API 返回 null）
}
```

### 设计哲学

- **库层面精确定义**：`DataError` 变体让调用方能够区分"这个标的不存在"和"网络断了"。GUI 可以展示不同的提示信息；CLI 可以决定是否重试。
- **符合人体工学的传播**：实现了 `From<reqwest::Error>` 和 `From<duckdb::Error>`，因此 `?` 可以在 provider 方法中直接使用。
- **Parse 错误携带上下文**：`DataError::Parse(String)` 包含解析失败的原始字符串，而不仅仅是通用消息。这使得排查 API 响应变更变得直接了当。
- **生产代码不使用 unwrap**：库代码使用 `?` 或带描述性消息的 `.expect(msg)`。不允许裸 `.unwrap()` — 错误消息必须解释发生了什么以及在哪里发生。

## Provider 配置

Provider 配置从 `~/.config/compass/config.toml` 读取。`[parquet].dir` 键设置 Parquet 数据目录；`[dolt]` 键设置 Dolt 仓库路径：

```toml
[parquet]
dir = "/data/compass-data/parquet_data"

[dolt]
investment_data_dir = "/data/compass-data/investment_data"
compass_data_dir = "/data/compass-data/compass_data"
```

对于 CLI，大部分设置都是命令行参数（参见 `compass-data --help`）：
- `--dolt-dir`、`--output`：覆盖 Dolt 目录和输出路径
- `--input`、`--format`、`--output`：导出选项
- `--since`：增量导入截止日期

完整配置参考见 `kb/user/config.md`。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 数据访问抽象：Provider 层设计 | 各后端直接调用 / trait 统一接口 | 三 trait 体系：DataProvider + DataWriter + NegativeCache | 多数据源（Dolt/DuckDB/Parquet）需统一接口；trait 支持 mock 实现用于测试，消费者与后端解耦 | 直接调用导致每个消费者需知道后端类型，无法替换或测试 |
| GUI 数据来源 | 在线 API 直连 / 多层读穿缓存 / 纯本地直读 Parquet | DuckDbProvider 直读 `stock_daily.parquet`（`read_parquet()` 回退） | 本地读取零延迟、无网络依赖、无 API 限流；重构后消除 cache miss 与负缓存复杂度 | 在线直连增加延迟和失败点；读穿缓存需维护 CachedProvider、负缓存、inflight 去重等多层状态 |
| 错误处理：错误类型设计 | anyhow 通用错误 / 精确枚举 | DataError 枚举：Network / Database / Parse / RateLimited / NoData，含 From 实现 | 调用方可区分错误类型（如 NoData 表示标的不存在 vs Network 表示网络中断），GUI 可据此展示不同提示；From 实现支持 `?` 传播 | anyhow 丢失错误分类信息，调用方无法做差异化处理；Parse 携带原始字符串便于排查 API 响应变更 |

