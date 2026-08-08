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

`search_symbols` 是辅助接口 — 按查询词返回匹配的 `SymbolInfo { code, name }` 列表。
注：GUI 的搜索框不依赖此接口（使用自定义 `StockPicker` 组件直接过滤股票列表）；
DuckDbProvider 的实现返回空列表，只有 ParquetReader 提供实际实现。

### DataWriter — 直写持久化
在获取数据后调用，将 bar 持久化到本地。
`overwrite` 标志控制行为：`false` = INSERT OR IGNORE（跳过重复键），`true` = INSERT OR REPLACE（更新已有行）。只有 DuckDbProvider 实现了此 trait。

### NegativeCache — 避免重复失败
此 trait 由 DuckDbProvider 实现，存储带 TTL 时间戳的 `no_data_marks` 条目。当前纯本地架构中没有任何消费者调用它（GUI 与 CLI 均不使用）— 保留实现与测试，但为未启用能力。

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

四张表，均在首次使用时自动创建：

| 表 | 键 | 用途 |
|---|---|---|
| `stock_daily` | `(symbol, trade_date)` | 核心 OHLCV bar — 主缓存表 |
| `stock_adj_factor` | `(symbol, trade_date)` | 价格复权因子 |
| `stock_limit` | `(symbol, trade_date)` | 每日涨跌停价格 |
| `no_data_marks` | `(symbol, timeframe)` | 带 TTL 时间戳的负缓存条目 |

> `stock_basic` 不再由 DuckDB 管理——元数据只走 Parquet（`import-compass --table stock_basic` 生成）。

DuckDB DDL（首次使用时自动创建）：

```sql
CREATE TABLE stock_daily (
    symbol      VARCHAR NOT NULL,
    trade_date  DATE NOT NULL,
    open, high, low, close, adjclose DOUBLE,
    volume, amount DOUBLE,
    PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_adj_factor (
    symbol, trade_date, adj_factor, PRIMARY KEY (symbol, trade_date)
);
CREATE TABLE stock_limit (
    symbol, trade_date, up_limit, down_limit, PRIMARY KEY (symbol, trade_date)
);
```

Parquet 主数据库布局（`stock_daily.parquet` 由 `compass-data import` 生成，`stock_basic.parquet` 由 `import-compass --table stock_basic` 生成）：

```
parquet_data/
├── stock_basic.parquet        # symbol, name, list_date, delist_date, board, full_name, total_share, industry, region
├── stock_daily.parquet        # symbol, tradedate, open, high, low, close, adjclose, volume, amount
└── stock_daily.symbols.txt    # 每行一个标的（快速列表）
```

完整 DDL 见上文（DuckDB DDL 代码块）。Parquet 主数据库布局见本文件的 Schema 章节。

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

### 前复权（ref #176）

`fetch_bars()` 三条路径（1d 内存表 / parquet 回退 / 1w·1M 聚合）**均返回前复权价**：
`factor_i = adjclose_i / close_i`，`open/high/low/close × factor_i` 后写入
`Bar`（volume 原样）。最新日 `adjclose == close` → factor=1.0，价格与现价一致。

- **1w/1M 先缩放后聚合**：内层 SELECT 按日 factor 缩放 OHLC，外层再
  `FIRST(open)/MAX(high)/MIN(low)/LAST(close)/SUM(volume)`——保证除权日的
  周/月线高低点准确（聚合后再缩放会失真）。
- close≤0 或 adjclose 非有限时 factor 回落 1.0（不产生 inf/NaN）。
- 指标（MA/BOLL）在缩放后的 adjusted 序列上实时计算；渲染层无感知。

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

### 横截面读取（fetch_cross_section）

`fetch_cross_section(range_start, range_end)` 是**全市场扫描**原语：不像
`fetch_bars_blocking` 那样按 `WHERE symbol = ?` 过滤，而是按日期范围一次返回
**所有标的**的 bar，供选股器等横截面分析使用：

```sql
SELECT symbol, CAST(tradedate AS VARCHAR) AS tradedate, open, high, low, adjclose, close, volume, amount
FROM read_parquet('parquet_data/stock_daily.parquet')
WHERE tradedate >= ? AND tradedate <= ?
ORDER BY symbol, tradedate ASC
```

返回 `Vec<CrossSectionBar>`（`symbol`、`trade_date: NaiveDate`、`open`/`high`/`low`/`adjclose`/`close`/`volume`/`amount`）—
这是代码库中**首个把 `adjclose` 带出读取层**的路径（`fetch_bars` 的 fallback 查询虽 SELECT 了 adjclose 但映射时丢弃）。
SEPA 扩展（epic #139）加入 `open`/`high`/`low`/`amount`：形态（VCP）与 ATR 需要 OHLC，成交额因子需要 `amount`。
列顺序与 `stock_daily.parquet` 实际 9 列布局一致：symbol, tradedate, open, high, low, close, adjclose, volume, amount。

注意：parquet 的 `tradedate` 列实际类型是 **TIMESTAMP**（非 DATE），
`CAST AS VARCHAR` 产出 `"1991-04-04 00:00:00"` 带时间分量。解析必须用
`date_str_to_utc`（兼容 DATE 与 TIMESTAMP 两种格式），不能只用 `%F`。

### SEPA 数据表读取原语（epic #139）

5 个只读原语，模式与 `fetch_cross_section` 一致（`read_parquet()` + 内存 DuckDB 查询），
文件名 = Dolt 表名 + `.parquet`，与 `stock_daily.parquet` 同目录（由 `import-compass --table ...` 生成）：

| 方法 | 文件 | 日期过滤 | 说明 |
|---|---|---|---|
| `fetch_concept_member()` | `concept_member.parquet` | 无（全量快照） | 概念成分映射，版本化非每日快照 |
| `fetch_capital_main_flow(start, end)` | `capital_main_flow.parquet` | `trade_date` | 主力资金流；NULL 金额 COALESCE 为 0.0，`small_net` 保持 Option |
| `fetch_dragon_list(start, end)` | `dragon_list.parquet` | `trade_date` | 龙虎榜席位；`institution_flag` 为 TINYINT → `Option<i8>` |
| `fetch_block_trade(start, end)` | `block_trade.parquet` | `trade_date` | 大宗交易；price/volume/amount NULL 时 COALESCE 为 0.0 |
| `fetch_institution_survey(start, end)` | `institution_survey.parquet` | `survey_date` | 机构调研 |

**缺文件行为（审查修订锁定）**：表未导入时返回**空 Vec**（与 `fetch_cross_section`
缺 `stock_daily.parquet` 行为一致），**不返回 DataError**——否则 `run_sepa` 的 `?`
会在 GUI 表未导入时直接失败，无法优雅降级。

### 标的发现

`list_symbols()` 首先检查 `stock_daily.symbols.txt`（每行一个标的，已排序），该文件由导入管线与 `stock_daily.parquet` 一起生成。这是快速路径 — 一次简单的文件读取。

如果 `symbols.txt` 不存在，则回退到 `SELECT DISTINCT symbol FROM read_parquet('stock_daily.parquet') ORDER BY symbol`。如果两个来源都不存在，返回空 vec。

`search_symbols()` 基于 `list_symbols()` 的结果做大小写不敏感的子串过滤（仅匹配 code，不匹配 name）。
返回过滤后的 `SymbolInfo` 列表。

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
    └─ 标的列表写入 parquet_data/stock_daily.symbols.txt
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
| stock_basic 数据源 | 东财 push2 (EM_FS) / investment_data ts_a_stock_list / 三大交易所官网 | 官网 | 数据权威含退市日期、无新三板污染（东财 t:81 段混入 6841 只新三板/老三板）、ts_a_stock_list 过时 4 年 | 东财段位不可靠且无退市日期；ts_a_stock_list max list_date 2022-07-18 无法覆盖新股 |
| stock_basic 元数据存储 | DuckDB 表 + Parquet 双轨 / 仅 Parquet | 仅 Parquet（`import-compass --table stock_basic` 生成，ParquetReader 直读） | duckdb.rs 的旧 stock_basic 路径（8 列旧 schema + upsert/get）零生产调用者；`import` 写 5 列占位文件会覆盖新 9 列 parquet（issue #181 起无 exchange 列）；单一数据源避免双 schema 维护 | DuckDB 双轨徒增第二份 schema 定义与同步成本；`import` 保留导出会持续制造错误列文件（ref #80） |
| 横截面原语位置 | DataProvider trait 方法 / DuckDbProvider 方法 / ParquetReader 固有方法 | ParquetReader 固有方法 `fetch_cross_section` | 与 `load_all_stock_basics` 同源同模式；避免 trait 三 impl（duckdb/parquet/synthetic）牵连；符合"直读 parquet"契约 | trait 扩展需同时改三处 impl 且 synthetic 为私有 mod；DuckDbProvider 依赖内存表缓存模型，与全表扫描不兼容 |
| CrossSectionBar 字段集 | 全 OHLCV / 仅 adjclose / adjclose+close+volume | `symbol, trade_date, adjclose, close, volume` | 足以支撑选股器全部条件（均线/动量/突破用 adjclose，量能用 volume，最新价/市值用 close）；DuckDB 列裁剪最小化 I/O | 全字段浪费内存（约 1.6M 行）；仅 adjclose 无法算市值/最新价 |
| CrossSectionBar 字段集（SEPA 扩展，ref #145） | 保持 5 字段 / 扩展 9 字段 | 9 字段（+`open`/`high`/`low`/`amount`） | SEPA 形态模块（VCP 需 high/low 通道）与 ATR（需 high/low/close）依赖 OHLC，成交额（20 日均额）过滤需要 `amount`；字段追加向后兼容，选股器仍只用原 5 字段，内存代价可接受（全市场单日 ~6000 行） | 保持 5 字段无法支撑形态/ATR/成交额因子；只加 amount 则形态与 ATR 仍缺数据 |
| SEPA 新表读取原语位置（ref #145） | ParquetReader 固有方法 / DuckDbProvider 方法 / DataProvider trait 扩展 | ParquetReader 固有方法（5 个：concept_member/capital_main_flow/dragon_list/block_trade/institution_survey） | 与 `fetch_cross_section`/`load_all_stock_basics` 同源同模式（`read_parquet()` 直读）；新表只读 parquet 文件，无 DuckDB 表缓存模型；避免 trait 三处 impl（duckdb/parquet/synthetic）牵连 | trait 扩展需同时改三处 impl 且 synthetic 为私有 mod；DuckDbProvider 依赖内存表缓存模型，与全表扫描不兼容 |
| `tradedate` 列类型解析 | `%F` 严格解析 / `date_str_to_utc` 双格式 | `date_str_to_utc`（兼容 DATE 与 TIMESTAMP） | 真实 parquet 列为 TIMESTAMP，`CAST AS VARCHAR` 带时间分量；`%F` 会静默丢弃全部行（测试 DATE fixture 不暴露） | `%F` 在生产环境静默空结果，是真实数据核验发现的陷阱 |
| 选股市值计算 | total_share × 最新 adjclose / × 最新 close ÷ 1e8 | `total_share × 最新 close ÷ 1e8`（亿元） | 最新日 adjclose == close（前复权锚点）；市值是现实世界值，用原始价 | adjclose 复权价会失真；单位显式 ÷1e8 与 GUI 亿元输入一致 |
| SEPA 写回方式（ref #150） | REPLACE INTO / 两段式 DELETE + `dolt table import -a` | 两段式：先 `DELETE FROM <table> WHERE trade_date='<date>'` 清当日，再 append CSV | 幂等重跑核心——同日期重跑行数不增；无需 SQL 转义整行值；与 `dolt table import` 封装风格一致 | REPLACE INTO 需转义且与 import 管线不一致；破坏性最小（只清当日，保留其他日期） |
| fetch_bars 前复权（ref #176） | 返回前复权价 / 返回原始价由 GUI 缩放 | 返回前复权价（`factor_i = adjclose_i / close_i`，1w/1M 先缩放后聚合） | 渲染层无感知、单点缩放避免多路径逻辑复制；最新日 factor 恒 1.0 与现价一致 | GUI 侧缩放需每调用方重复逻辑且周/月聚合难以正确处理除权日（先聚合后缩放会失真） |
| SEPA 计算表列级 DDL（ref #150） | plan 模板列（ma60/ma120/ma250/atr20、return20/return60、volume_ratio_score/institution_score、hs300_trend 等）/ 按 SepaData 可得字段自定义 | 按 SepaData 字段自定义（见下） | `SepaRow` 只暴露五模块加权分 + `details` 子项分；MA/ATR/板块动量等原始值不进入 SepaData（不加 serde、不改 compass-strategy 的约束下不可得），列必须对齐实际可写值 | plan 模板列含不可得字段，强行写入只能填 NULL/占位，违背"不写表面表" |
| technical_factor 列集（ref #150） | plan 模板（ma60/ma120/ma250/atr20/rs_score/vcp_score）/ 子项分 | `symbol, trade_date, structure_score, position_score, rs_score, vcp_score, breakout_score, update_date`，PK(symbol, trade_date) | 均线结构/价格位置/相对强度/VCP质量/突破确认为 `details.trend`/`details.pattern` 子项分，直接可得 | MA/ATR 原始值仅在 compass-strategy 内部，暴露需改引擎代码（本 todo 禁止） |
| industry_factor 列集（ref #150） | plan 模板（concept_code/return20/return60/concept_amount）/ 概念名聚合 | `concept_name, trade_date, stock_count, gain_score, amount_score, diffusion_score, heat_score, news_score, update_date`，PK(concept_name, trade_date) | SepaData 仅暴露每股票 `themes`（概念名）与 theme 子项分，按概念名聚合可得板块热度汇总；concept_code 不进入 SepaData | 模板需 concept_code 与板块动量原始值，均不可得；聚合免二次计算（复用 run_sepa 输出） |
| capital_factor 列集（ref #150） | plan 模板（volume_ratio_score/chip_score/main_flow_score/institution_score）/ 子项分 | `symbol, trade_date, volume_price_score, chip_score, big_capital_score, update_date`，PK(symbol, trade_date) | 量价配合/筹码集中/大资金流入为 `details.capital` 子项分，直接可得 | institution_score 独立值只存在于 note 字符串（"主力+龙虎+调研+大宗"），解析脆弱 |
| final_score 列集（ref #150） | — | plan 模板原样：`symbol, trade_date, trend_score, theme_score, money_score, pattern_score, risk_score, total_score, rank, update_date`，PK(symbol, trade_date)；`rank` 反引号转义（Dolt 保留字） | `SepaRow` 五模块加权分 + total + rank 全部直接可得（money_score = `SepaRow.capital`） | — |
| market_temperature 列集（ref #150） | 原始值直存 / 从 indicators value_text 解析 | plan 模板列：`trade_date, score, hs300_trend, zz1000_trend, limit_up_count, total_amount, breadth, position_suggestion, update_date`，PK(trade_date)；数值从 5 个 `SepaIndicator.value_text` 解析 | 原始值（ratio/涨停数/成交额/上涨比例）只在引擎内部计算，`MarketThermometer` 仅暴露 value_text；格式由 temperature.rs 常量锁定（`{:.1}%`/`{n} 家`/`{:.2}万亿`）且有既有测试断言 | 改引擎暴露原始值违反"不改 compass-strategy"；value_text 解析失败回退 0，绝不 panic |
| SEPA symbol 前缀来源（ref #150，issue #181 修订） | 裸码直写 / 从 stock_basic.parquet exchange 列拼前缀 / **透传引擎前缀 symbol** | 写回 CSV 直接透传 `SepaRow.symbol`（前缀形式，如 `SZ000001`），`exchange_prefixes` 查找已删除 | D6 删除 stock_basic.parquet 的 exchange 列后无从查找；引擎 key 已全前缀（C4），透传零转换零回退，杜绝 `SHSZ000001` 垃圾前缀 | 拼前缀依赖已删 exchange 列且回退 `SH` 会误标一切；裸码直写与采集表外键语义不一致 |
| run_temperature 实现（ref #150） | 独立重算温度计 / 复用 run_sepa 输出 | `run_sepa(SepaQuery{top_n:1})` 取 `SepaData.thermometer` | run_sepa 内部已算温度计，复用免重复 fetch 与分组逻辑；全市场打分一次可接受（CLI 非热路径） | 独立重算需复制 fetch/分组管线，双份维护 |
| data_updates 登记（ref #150） | 仅 Dolt 表 / 同步登记 | 每张计算表 import 后 upsert `data_updates`（source=`'compass-data sepa'`，last_report_date=计算日期，last_updated=运行日，row_count=导入行数） | 与采集表同款可观测性；脚本/用户可查最近计算状态 | 不登记则计算历史无从追踪 |
| 写回内容与 `--top` 解耦（ref #150，PR 评审 P0-1） | 写回 top-N 截断集 / 写回全量计算集 | `run_score` 引擎 `top_n: usize::MAX` 全量计算，`--top` 仅控制终端表格打印；write_back 持久化全量通过过滤的排序结果 | PR 评审实证：`sepa score --top 3` 重跑会 DELETE 当日已存全部行再只写 3 行（SZ000852/SZ000906 被删）；`--top` 语义是"输出条数上限"，不该决定持久化内容；与 GUI "TOP N 截断仅作用于本地副本" 原则一致 | 写回 top-N 子集导致不同 --top 值产生不同持久状态、非幂等 |
| run_temperature 写回范围（ref #150，PR 评审 P0-2） | 复用 write_back 全 5 表 / 仅写 market_temperature | `write_back` 增加 `tables: &[&str]` 参数；`run_temperature` 只传 `["market_temperature"]`，DELETE 与 import 均限定范围 | PR 评审实证：temperature 复用 `run_sepa(top_n:1)` 后 write_back 全 5 表 → final_score/technical_factor/capital_factor 被清空至 1 行；温度计命令只应写温度计行 | 全 5 表写回使独立运行 temperature 破坏当日评分数据 |
| 默认计算日期（ref #150，决策 22 修正） | `Utc::now()` / 数据内最新交易日 | `reader.latest_trade_date()`（stock_daily.parquet MAX(tradedate)）回退 `Utc::now()` | PR 评审实证：周日运行无 --date 写出 trade_date=2026-08-02 非交易日行且基于 07-28 数据却标 08-02；决策 22 原文"只算最新交易日" | 墙钟日期在周末/节假日非交易日，产生伪日期行 |
| 采集器导入语义（ref #139，F3 修复） | 整表替换 / merge 追加 | 4 个时间序列表（main_flow/dragon/block_trade/institution_survey）`import_replace_table(merge=True)`：CREATE IF NOT EXISTS + INSERT IGNORE 按 PK 去重，增量窗口 CSV 追加进已有表；concept_member 保持全量重写（版本快照） | F3 实证：增量窗口 CSV（last_report_date 之后）+ 整表替换 → institution_survey 40096 行被覆盖成 29 行（历史丢失）；merge 使同日期重跑行数不增、历史完整（293769 行稳定） | 整表替换在增量窗口下破坏完整历史；concept_member 是版本快照语义，重写为预期行为 |
| 财务采集器导入语义（ref #160） | 整表替换 / merge 追加 | 财务四表（fin_balance_sheet/fin_income/fin_cash_flow/fin_indicators）`import_replace_table(merge=True)`：DDL 改 `CREATE TABLE IF NOT EXISTS`、`INSERT IGNORE` 按 PK `(symbol, report_date)` 去重；fetch 保持报告期级增量（`since` 锚点读 data_updates，最新报告期会重抓以捕获期内新披露公司） | 增量窗口 + 整表替换 = 历史丢失（fin_balance_sheet 130927 行被覆盖成 1 行测试样例行）；merge 使同报告期重跑幂等、历史永不丢失 | 整表替换在报告期级增量窗口下破坏完整历史；保留替换语义的旧实现（RENAME aside → DROP → CREATE → INSERT SELECT）是 #160 数据丢失事故根因 |
| 采集器长文本导入（ref #139，F3 修复） | `dolt table import -c` 类型推断 / 显式宽 schema 建表 | `dolt_table_import(create_sql=...)` 先 CREATE 宽临时表（如 RECEIVE_OBJECT VARCHAR(1000)）再 `-u` 导入 | F3 实证：dolt `-c` 推断字符串固定 varchar(200)，长 UTF-8 值按字节截断产生畸形字节（org_name 900 字节 → 198 字节），post-import ALTER 无法修复已截断数据 | `-c` 推断在长文本表上静默截断，破坏 utf8mb4 插入 |
| institution_survey 去重分组键（ref #139，F3 修复） | HEX(org_name) 仅按机构 / 完整复合键 | `GROUP BY s, d, gk`（s/d 为已派生 symbol/date，gk=HEX(org_name)），列用 MAX() 重新派生 | F3 实证：仅按 org 分组把同机构不同 symbol/date 的事件坍缩成一行（长信基金 484 事件 → 1 行，全表 293916 行 → 40115 行，丢失 86%）；复合键在保留去重同时保留每个 (symbol, survey_date, org) 事件；实测 Dolt 2.2.3 对中文列 GROUP BY 无 bug（无需纯 ASCII 键） | 仅按 org 分组破坏事件粒度，是静默数据丢失 |

| backtest_result 表结构（ref #154） | 存每日持仓明细 / 只存每日净值 | PK(trade_date) + strategy_nav/benchmark_nav/update_date | 净值曲线足以支撑绩效复盘；明细体积大且可重算 | 明细持久化无查询需求；全表 DELETE + `dolt table import -a` 单快照替换幂等 |
