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
├── stock_basic.parquet        # symbol, name, list_date, delist_date, board, full_name, total_share, industry, industry_en, region（epic #266 加 industry_en）
├── stock_daily.parquet        # symbol, tradedate, open, high, low, close, adjclose, volume, amount
├── index_basic.parquet        # symbol, name, name_en, index_type（指数/板块名称表，epic #255；epic #266 加 name_en）
├── index_daily.parquet        # symbol, index_type, tradedate, open, high, low, close, volume, amount, adjclose（指数/板块日线，epic #255）
├── fin_income.parquet         # 利润表 — F10 完整版（203 字段），PK (symbol, report_date)
├── fin_balance_sheet.parquet  # 资产负债表 — F10 完整版（319 字段），PK (symbol, report_date)
├── fin_cash_flow.parquet      # 现金流量表 — F10 完整版（254 字段），PK (symbol, report_date)
└── stock_daily.symbols.txt    # 每行一个标的（快速列表）
```

### 指数/板块表（epic #255）

指数与板块行情**独立于股票**建表/落盘——尊重 ref #201「股票数据剔除指数」决策，
不污染选股/评分。两张 Dolt 表建在 `compass_data` 库（`collectors/fetch_index_daily.py`
创建），经 `import-compass --table index_daily|index_basic` 导出 Parquet：

**index_daily**（指数/板块日线，`index_type` 区分三类标的）：

```sql
CREATE TABLE IF NOT EXISTS index_daily (
    symbol      VARCHAR(20) NOT NULL,
    trade_date  DATE NOT NULL,
    index_type  VARCHAR(20) NOT NULL,
    open        DOUBLE,
    close       DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    volume      DOUBLE,
    amount      DOUBLE,
    update_date DATE,
    PRIMARY KEY (symbol, trade_date)
)
```

**index_basic**（名称表，picker 与板块列表的名称/类型来源）：

```sql
CREATE TABLE IF NOT EXISTS index_basic (
    symbol      VARCHAR(20) NOT NULL PRIMARY KEY,
    name        VARCHAR(100),
    index_type  VARCHAR(20),
    name_en     VARCHAR(100)
)
```

- **`index_type` 取值**：`official`（交易所官方指数）/ `industry`（行业板块）。
  官方指数为**硬编码白名单**（约 30 只主流指数，`fetch_index_daily.py::OFFICIAL_INDICES`，
  akshare index_zh_em 同款做法：SH000001 上证指数 / SZ399001 深证成指 / SZ399006 创业板指 /
  SH000300 沪深300 / SH000905 中证500 / SH000852 中证1000 等）；行业板块为**同花顺
  90 个申万一级行业**（issue #283 D1：列表实时抓 `q.10jqka.com.cn/thshy/` GBK 页面
  href 提取 `881xxx` + 名称，140 行去重 → 90；symbol 为 `BK` + 6 位，如 `BK881101`）。
  **东财板块 1000 个采集目标已废弃**（issue #281，2026-08-20 关闭）：不再补采东财
  概念/行业 1000 板块，板块数据以标准行业分类（同花顺 90 个申万一级行业）为准。
  **概念板块（`concept`）已彻底移除**（issue #283 D4）：不再采集/发现/入库，Dolt
  存量概念行与 `concept_member` 表已清理，GUI 概念段与 SEPA 概念主题标签同步删除。
- **同花顺行业 K 线（issue #283 D2）**：按年分页 `d.10jqka.com.cn/v4/line/bk_881xxx/01/{year}.js`
  （JSONP 包装，`data` 字段 `;` 分隔），年循环 2007→当前年、空年提前终止；
  7 字段列序为 `日期,开,高,低,收,量,额`（与东财 `开,收,高,低` **不同**，解析时重排为
  东财序再复用 `_kline_records`）。同花顺段受 #277 连续失败快速终止保护。
- **腾讯回退（issue #278/#286）**：官方指数优先东财 push2his；东财失败/empty 时自动切腾讯
  `web.ifzq.gtimg.cn/appstock/app/newfqkline/get`（count≤2000，end 日期反向分页拉全历史；
  day 行 11 字段，index 8 为成交额（万元），采集时 ×10000 转为元；缺失/畸形金额降级为 0；
  受 #277 连续失败快速终止保护）。行业板块只走同花顺。
- **增量**：盘后 `data_updates.last_report_date` 短路跳过；K 线按 symbol 的
  `MAX(trade_date)` 真增量（issue #292）——THS 行业只拉 MAX 年份→今年（MAX 为
  12-31 时从次年启动）并过滤 `<= MAX` 旧行；官方指数东财 `beg=MAX+1`、腾讯增量
  翻页遇 `<= MAX` 行即停；新 symbol 自动补全量历史；周末/停牌无新行按成功 no-op
  处理，不触发 fast-fail。
- **Parquet 布局**：
  - `index_daily.parquet`：`symbol, index_type, tradedate, open, high, low, close, volume, amount, adjclose`
    ——导出时 Dolt `trade_date` 重命名为 `tradedate`（对齐 `stock_daily.parquet` 列名），
    **`adjclose = close` 占位**（指数无复权概念，东财 `fqt=0` 不复权拉取；占位列使
    `DuckDbProvider` 既有 7 列查询 / 前复权缩放（factor=1.0 恒等）/ 1w·1M 聚合零改动复用）。
  - `index_basic.parquet`：`symbol, name, name_en, index_type`。
  - **`name_en` / `stock_basic.industry_en`（epic #266）**：collectors 静态映射表
    `collectors/name_en_mapping.csv`（`section,key,value`：index 按 symbol /
    industry 按行业中文；concept 段已随 #283 删除）import 时 LEFT JOIN 写入；
    未收录 → NULL → GUI 回退中文。读取侧（`ParquetReader`）对旧 parquet（无新列）
    优雅降级 `None`（`is_missing_column` binder 错误匹配后重试无列查询）。
- **导出语义**：`import-compass --table index_daily` 走**增量 merge**
  （parquet 侧 PK (symbol, tradedate)，`prefer_new` 即 Dolt 新值优先，`--since` 支持，
  与 capital_main_flow 同款 `import_append_table`）；`--table index_basic` **全量覆盖**
  （仿 concept_member 版本快照语义——上游删板块即从 parquet 消失）。两者新鲜度阈值 7 天
  （行情表档）。

**单位约定（ref #201）**：`stock_daily.parquet` 的 `volume` 为**股**（Dolt 源为"手"，import 时 ×100）、
`amount` 为**元**（Dolt 源为"千元"，import 时 ×1000）。SEPA 引擎的 `MIN_AVG_AMOUNT=3000万`
以元为准；其他消费者（GUI 图表柱高、screener 量能相对倍数）不受换算影响。
另外 import 无条件剔除 6 个指数代码（见 `symbols.md` 指数剔除约定），parquet 与
`symbols.txt` 均不含指数（指数行情自 epic #255 起独立入库 `index_daily.parquet`，
见上文「指数/板块表」章节）。

财务三表（fin_income/fin_balance_sheet/fin_cash_flow，ref #202）：

- **来源**：东财 F10 完整版报表 `RPT_F10_FINANCE_GINCOME/GBALANCE/GCASHFLOW`（`columns=ALL` 全量返回），替代此前 DMSK 主干版（46/57/48 字段）
- **schema**：`symbol` + `report_date`（PK）+ 全部 F10 字段（203/319/254 列，含 `_YOY` 同比列）；字段名保持 F10 原生大写名，数值列 DOUBLE、文本列 VARCHAR(100)
- **单位**：元（与 DMSK 口径一致；茅台 2024 实测 TOTAL_OPERATE_INCOME=174144069958.25、BASIC_EPS=68.64）
- **范围**：2020 至今（START_YEAR=2020），report_date 按季度（Q1/Q2/Q3/FY）
- **导出**：`import-compass --table fin_income|fin_balance_sheet|fin_cash_flow`（`SELECT *` 通配符，新列自动带出）；采集器导入用 replace 语义（全表原子重建）+ 显式宽 schema 临时表（`create_sql`，203-319 列超 Dolt `-c` 推断 65504 字节行尺寸上限）

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

### 指数/板块数据读取：双 parquet 路由（epic #255）

`DuckDbProvider::fetch_bars` 在内存表 miss 时**按序回退两个 parquet 文件**：先查
`stock_daily.parquet`（issue #31 既有回退路径），结果为空再查 `index_daily.parquet`
（同一 SQL 形状，`tradedate/open/high/low/close/volume/adjclose/amount` 8 列）。

两个文件**互斥**，顺序回退语义精确：
- ref #201 已把 6 个指数从 `stock_daily.parquet` 剔除 → 指数代码在 stock 文件必然查空，
  回退 index 文件是**确定性路由**而非碰运气；
- 反向股票代码在 index 文件也不存在（index 文件只含指数/板块），fallback 不会泄漏。

index 行回退后同样 cache-warm 进内存 `stock_daily` 表，1w/1M 的 `date_trunc` 聚合
（647+ 行）对指数路径**零新增代码**复用。消息契约（FetchRequest）与 GUI 调用方零改动。

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
`import` 的数据质量校验（ref #136）做日期范围对比时，parquet 侧 `tradedate`
先 `CAST AS DATE` 规范化为 `YYYY-MM-DD`，再与 Dolt 侧的 DATE 值比较。

### SEPA 数据表读取原语（epic #139）

4 个只读原语，模式与 `fetch_cross_section` 一致（`read_parquet()` + 内存 DuckDB 查询），
文件名 = Dolt 表名 + `.parquet`，与 `stock_daily.parquet` 同目录（由 `import-compass --table ...` 生成）；
`fetch_concept_member` 已随 issue #283 D4 移除（题材模块改用 stock_basic.industry 分组）：

| 方法 | 文件 | 日期过滤 | 说明 |
|---|---|---|---|
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

**`import-compass` append 表增量合并（ref #298）**：所有走 `import_append_table` 的
append 表（fin_*、capital_main_flow、dragon_list、block_trade、institution_survey、
index_daily）的 merge 分区列必须与生产 Dolt 表全主键一致；`block_trade` 全主键为
`(symbol, trade_date, price, volume, amount, buyer, seller)`。merge 失败时的 fallback
必须执行不带 `--since` 的真全量导出，不能再用过滤后的增量数据覆盖旧 parquet
（#298 历史丢失根因）。

**导入侧换算与过滤（ref #201）**：主查询对 `volume × 100`（手→股）、`amount × 1000`（千元→元），
并无条件追加 `symbol NOT IN (6 个指数代码)` 过滤（即使 `--symbols` 显式指定指数也剔除）；
symbol 枚举查询（symbols.txt）同步过滤。Dolt 源表 `final_a_stock_eod_price` 保持原样（手/千元）。

**采集器字符串 TRIM（issue #235）**：collectors 写 Dolt 的 INSERT SELECT 中对用户可见
文本列统一包 `TRIM(col) AS col`（覆盖 stock_basic / fin_indicators / 财务三表 /
institution_survey / block_trade；与 concept_member 先例 `TRIM(BOARD_NAME)` 一致）。
语义要点：
- **仅文本列**，标识符列（SECUCODE/SECURITY_CODE/ORG_CODE 等）与日期列不 TRIM；
- **仅去 ASCII 空格 U+0020**——`TRIM()` 不剥离全角空格 U+3000（Dolt 实证），脏数据
  检测需叠加 `LIKE CONCAT('%', CHAR(0x3000), '%')` 宽字符谓词；
- **财务三表列清单逐表独立**——LISTING_STATE 仅存在于 balance_sheet；
- institution_survey 的分组键为 `HEX(TRIM(RECEIVE_OBJECT))`（gk 同步 TRIM 才能合并
  'A'/'A ' 组）；
- 回归测试：`collectors/tests/test_trim_imports.py`（RED→GREEN，含 U+3000
  characterization 锁定盲区）。

## Python 采集器代理层（issue #294）

collectors 对东财/THS/交易所官网的 HTTPS 抓取默认走本地 proxy_pool 代理层，
解决 VPS 固定 IP 上 push2his/THS 的限流断连（curl 56）。

### 架构

- `collectors/proxy_pool_client.py`：`ProxyPool` 客户端——`get_proxy`
  （`GET /get/?type=https`）、`delete_proxy`（`GET /delete/`）、`pool_count`
  （`GET /count/`）、`record_state`（原子写 `proxy_pool_state.json`）。
- `collectors/common.py`：`make_proxy_pool()` + 请求包装 `proxy_get` / `proxy_post`
  （async）与 `proxy_get_sync` / `proxy_post_sync`（同步 `requests.Session`），
  以及 `fetch_paginated(..., *, pool=None)`。
- 接入面：东财 datacenter（balance_sheet / cash_flow / income / fin_indicators /
  block_trade / dragon / institution_survey）、push2（main_flow）、push2his + THS +
  腾讯兜底（index_daily）、三大交易所官网（stock_basic_official）。
- `collectors/proxy_keepalive.py`：后台常驻喂源循环（freeproxy json + realtime 双源，
  本地 `/tmp/freeproxy.json` 快照兜底）。
- compose 部署（`scripts/proxy_pool/docker-compose.yml`）把 `proxy_redis` 的 6379
  以 `127.0.0.1:6379:6379` loopback 暴露到宿主机，保证 keepalive / fetch_freeproxy
  默认 `redis://@127.0.0.1:6379/0` 可直接灌池（issue #296）。

### 行为契约

- proxy-first：有 https 代理必走代理；`/get/?type=https` 返回的 `proxy` 经
  `{"http": "http://IP:PORT", "https": "http://IP:PORT"}` 传入每请求 `proxies`。
- 池空/API 不可达/畸形响应 → 醒目打印 `[proxy] WARN/ERROR: https pool empty,
  falling back to direct`（每实例首次）+ 写 `proxy_pool_state.json`（时间戳/池计数/
  是否降级/原因）+ 直连，绝不因无代理失败。
- 坏代理（请求异常，非 HTTP 状态码）→ `delete_proxy` 出池 + 换下一个；有界重试
  `DEFAULT_PROXY_MAX_ATTEMPTS=3` 后直连兜底一次，直连仍失败交给模块既有 retry。
- HTTP 429/5xx 不误删代理，由各模块既有重试处理。
- 环境变量：`COMPASS_PROXY_API_URL`（默认 `http://127.0.0.1:5010`）、
  `COMPASS_PROXY_DISABLE=1`（禁用代理层）、`COMPASS_CSV_DIR`（state 文件位置）。

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

完整配置参考见 `.dsh/kb/user/config.md`。

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
| 采集器导入语义（ref #139，F3 修复；index_daily 于 #303 纳入） | 整表替换 / merge 追加 | 5 个时间序列表（main_flow/dragon/block_trade/institution_survey/index_daily）`import_replace_table(merge=True)`：CREATE IF NOT EXISTS + INSERT IGNORE 按 PK 去重，增量窗口 CSV 追加进已有表；concept_member 保持全量重写（版本快照） | F3 实证：增量窗口 CSV（last_report_date 之后）+ 整表替换 → institution_survey 40096 行被覆盖成 29 行（历史丢失）；merge 使同日期重跑行数不增、历史完整（293769 行稳定） | 整表替换在增量窗口下破坏完整历史；concept_member 是版本快照语义，重写为预期行为 |
| index_daily 纳入每日 SEPA 管线（ref #303） | 继续只靠手动跑 collectors/index_daily 或独立 sync / 纳入 `scripts/sepa_daily.sh` 标准 5 源流程 | 纳入 `scripts/sepa_daily.sh`：step2 fetch+import、step3 `COLLECTOR_TABLES` allowlist、step4 增量锚点与导入列表均含 `index_daily` | 用户 2026-08-25 观察到指数数据停留在旧交易日；“更新数据库”只导 Parquet 不跑采集器时 `index_daily` 会滞后；每日流程已含其余 4 个 SEPA 时序源，纳入后保持单入口、增量锚点一致 | 单独维护 `index_daily` 流程造成两套刷新入口、易漏跑 |
| 财务采集器导入语义（ref #160） | 整表替换 / merge 追加 | 财务四表（fin_balance_sheet/fin_income/fin_cash_flow/fin_indicators）`import_replace_table(merge=True)`：DDL 改 `CREATE TABLE IF NOT EXISTS`、`INSERT IGNORE` 按 PK `(symbol, report_date)` 去重；fetch 保持报告期级增量（`since` 锚点读 data_updates，最新报告期会重抓以捕获期内新披露公司） | 增量窗口 + 整表替换 = 历史丢失（fin_balance_sheet 130927 行被覆盖成 1 行测试样例行）；merge 使同报告期重跑幂等、历史永不丢失 | 整表替换在报告期级增量窗口下破坏完整历史；保留替换语义的旧实现（RENAME aside → DROP → CREATE → INSERT SELECT）是 #160 数据丢失事故根因 |
| fin_indicators 修订检测（issue #135，替代 #27） | REPORTDATE 窗口增量 + `--refresh N` / **UPDATE_DATE 时间锚点增量 + UPSERT** | 增量模式改为 `filter=(UPDATE_DATE>='{anchor}')`，锚点 = `min(data_updates.last_updated, state.json.last_update_date)`（env-aware `common.dolt_dir()`）；import 改 UPSERT（SELECT 侧全列别名 + ODKU 无前缀别名引用，Dolt 2.2.3 不支持限定源列引用/VALUES()）；CSV 整文件 keep-LAST 去重 | REPORTDATE 过滤无法感知同一报告期的数据修订（五粮液 2025Q1 修订案例：REPORTDATE 不变、UPDATE_DATE 变 2026-04-30）；UPDATE_DATE 锚点只抓有更新的行（新披露+旧修订一体），是 #27 `--refresh N` 的精准版；`--refresh N` 不再实现 | `MAX(update_date)` 作锚点会因预标未来日期漏行；`--refresh N` 无条件重拉低效；锚点 min 双源防跨日 fetch/import 或单独 import 导致的锚点超前漏抓 |
| 采集器长文本导入（ref #139，F3 修复） | `dolt table import -c` 类型推断 / 显式宽 schema 建表 | `dolt_table_import(create_sql=...)` 先 CREATE 宽临时表（如 RECEIVE_OBJECT VARCHAR(1000)）再 `-u` 导入 | F3 实证：dolt `-c` 推断字符串固定 varchar(200)，长 UTF-8 值按字节截断产生畸形字节（org_name 900 字节 → 198 字节），post-import ALTER 无法修复已截断数据 | `-c` 推断在长文本表上静默截断，破坏 utf8mb4 插入 |
| institution_survey 去重分组键（ref #139，F3 修复） | HEX(org_name) 仅按机构 / 完整复合键 | `GROUP BY s, d, gk`（s/d 为已派生 symbol/date，gk=HEX(org_name)），列用 MAX() 重新派生 | F3 实证：仅按 org 分组把同机构不同 symbol/date 的事件坍缩成一行（长信基金 484 事件 → 1 行，全表 293916 行 → 40115 行，丢失 86%）；复合键在保留去重同时保留每个 (symbol, survey_date, org) 事件；实测 Dolt 2.2.3 对中文列 GROUP BY 无 bug（无需纯 ASCII 键） | 仅按 org 分组破坏事件粒度，是静默数据丢失 |

| backtest_result 表结构（ref #154） | 存每日持仓明细 / 只存每日净值 | PK(trade_date) + strategy_nav/benchmark_nav/update_date | 净值曲线足以支撑绩效复盘；明细体积大且可重算 | 明细持久化无查询需求；全表 DELETE + `dolt table import -a` 单快照替换幂等 |
| stock_daily 单位换算（ref #201） | 引擎侧换算 / **import 侧 SQL 换算** | import 侧 `volume × 100`、`amount × 1000` | 源库 amount 为千元、volume 为手（茅台 08-03 实证 ratio≈1000）；import 侧修正后所有下游（SEPA/GUI/温度计/回测）一次性拿到元/股，且无需各模块各自换算 | 引擎侧修正只修 SEPA，GUI 与温度计（`total_amount/1e12` 假设元）仍错 |
| 指数代码剔除（ref #201） | 内存过滤 / **主查询 + 枚举查询 WHERE NOT IN** | 6 个指数（SH000300/SH000852/SH000905/SH000906/SH000985/SZ399300）无条件剔除 | 指数非股票，混入导致 SEPA 硬过滤线被指数占位（修复前 top50 仅 2 只）；双查询过滤使 parquet 与 symbols.txt 一致，COUNT 汇总复用同一 where 自动一致 | 仅内存过滤则 parquet 仍含指数行，下游仍被污染 |
| 财务三表报表版本（ref #202） | DMSK 主干版（RPT_DMSK_FN_INCOME/BALANCE/CASHFLOW，46/57/48 字段）/ F10 完整版（RPT_F10_FINANCE_GINCOME/GBALANCE/GCASHFLOW，203/319/254 字段） | **F10 完整版** | DMSK 主干版是金融机构模板（塞满银行保险专用科目），缺研发费用/营业外收支分开/其他收益/公允价值变动/资产信用减值/少数股东损益/EPS/综合收益、商誉/无形资产/开发支出/递延税/合同资产/应付债券/租赁负债、购建固定资产支付/取得子公司股权/税费返还/投资明细/分配股利等关键科目（ref #68 选型失误）；F10 `columns=ALL` 全量返回无数据丢失 | DMSK 主干版字段严重不全，无法支撑基本面分析 |
| 财务三表字段保留（ref #202） | 裁剪常用字段 / 全字段保留 | **全字段保留（含 `_YOY` 同比列）** | Parquet 列式存储 NULL 压缩成本≈0；未来免返工；Dolt 宽表 204-320 列可承受 | 裁剪清单需维护且可能再次漏字段 |
| 财务三表导入语义（ref #202，修正 ref #160） | merge 追加 / replace 原子替换 | **replace（全表原子重建）**：旧表 rename aside → DDL 建新表 → INSERT SELECT → 失败回滚 | F10 新 schema 与旧 DMSK 字段集不兼容，merge（CREATE IF NOT EXISTS + INSERT IGNORE）会保留旧结构表；本次为 schema 变更后的一次性全量重抓（2020 至今，无增量窗口），replace 匹配重建契约；未来增量恢复 merge | merge 无法重建新 schema 表；增量窗口 + replace 会丢历史（ref #160 教训），但本次是全量重建非增量窗口 |
| 财务三表 UPDATE_DATE 增量 + ODKU（issue #299，取代 ref #202 的 replace 契约） | REPORT_DATE 窗口增量 + replace / **UPDATE_DATE 时间锚点增量 + merge/ODKU** | 三表 `run(..., incremental=True)` 使用 `fetch_by_update_date`（filter=`(UPDATE_DATE>='{anchor}')`，sort=UPDATE_DATE）；anchor = `min(data_updates.last_updated, state.json.last_update_date)`，两源皆无 → 固定 `2020-01-01` 走一次全历史 UPDATE_DATE 拉取（不回退 REPORT_DATE 枚举）；import 改 `import_replace_table(merge=True)` + `INSERT ... ON DUPLICATE KEY UPDATE`（SELECT 侧全列唯一别名 + ODKU 无前缀别名引用，Dolt 2.2.3 兼容）；state.json 记 `last_update_date` + `last_report_date` | REPORT_DATE 过滤无法感知同一报告期的历史修订；replace 在增量窗口下会清掉 CSV 外的既有历史；UPDATE_DATE 锚点一次抓新披露+旧修订，ODKU 覆盖同 PK 旧值、历史永不丢失；无 anchor 固定 2020-01-01 保证首跑仍走增量路径 | `MAX(update_date)` 作锚点会因预标未来日期漏行；REPORT_DATE 枚举在首跑时仍是全量但无法捕获修订；replace 与增量窗口语义冲突 |
| 财务三表临时表导入（ref #202） | Dolt `-c` 推断 / 显式宽 schema + `create_sql` | **显式宽 schema**（`_TMP_INC_DDL`/`_TMP_BS_DDL`/`_TMP_CF_DDL`，203-319 列全字段 + REPORT_DATE VARCHAR） | Dolt `-c` CSV 导入推断行尺寸上限 65504 字节，203+ 列真实 CSV 溢出（实测 income 203 列 80032 字节报错）；显式 DDL + `dolt table import -u` 绕开限制，与 institution_survey 长文本表同模式 | `-c` 推断在宽表上静默失败，无法导入真实 F10 数据 |
| 财务三表 REPORT_DATE 处理（ref #202） | 裸插 / CAST | `CAST(REPORT_DATE AS DATE)` | F10 API 返回 `"2024-12-31 00:00:00"` 带时间格式，显式 CAST 入 DATE 列避免依赖宽松模式隐式截断 | 裸插依赖 Dolt 宽松模式，行为隐式 |
| 采集器字符串 TRIM（issue #235） | Python strip / SQL 层 TRIM / 不处理 | **SQL 层 TRIM**：写 Dolt 的 INSERT SELECT 中对文本列包 `TRIM(col) AS col` | 单点修复覆盖所有导入路径（与 concept_member `TRIM(BOARD_NAME)` 先例一致，ref #217）；与下游无关（Dolt 是唯一写库路径）；e706dfc 后 Dolt 现库无脏数据，无需重导 | Python strip 需逐采集器改抓取侧且 CSV 中转层不受控；不处理则题材 Tag 尾随空格根因残留 |
| TRIM 范围（issue #235） | 全部 VARCHAR 列 / 仅文本列 | **仅文本列**：stock_basic(name/board/full_name/industry/region)、fin_indicators(name/industry/board_name/trade_market/trade_market_zjg/security_type/data_type/qdate/date_label/dividend_plan/dividend_year)、财务三表(SECURITY_NAME_ABBR/ORG_TYPE/REPORT_TYPE/REPORT_DATE_NAME/CURRENCY/OPINION_TYPE/LISTING_STATE 逐表独立)、institution_survey(org_name/survey_type)、block_trade(buyer/seller) | 标识符列（SECUCODE/SECURITY_CODE/ORG_CODE 等）与日期列（NOTICE_DATE/UPDATE_DATE/list_date/delist_date）规范化无空格风险；dragon_list.seat_type 为 Python 派生枚举；误 TRIM 标识符会破坏前缀逻辑 | 全列 TRIM 对数值列报错（SQL 类型错误），对标识符列无意义 |
| TRIM 字符语义（issue #235，Oracle 实证） | 默认 `TRIM()` 仅去 U+0020 / REPLACE 变体去 U+3000 | **默认 `TRIM()`（仅 U+0020）** | Dolt `TRIM()` 与 `TRIM(BOTH ' ' FROM ...)` 均不剥离 U+3000（实测 HEX 保留 `E38080`）；`[[:space:]]` POSIX 类亦为 ASCII-only（Go RE2）；脏数据计数用 `LIKE CONCAT('%', CHAR(0x3000), '%')` 补充检测，Dolt 现库 U+3000 计数 0 | REPLACE 变体超出 #235 范围且会改变既有 U+3000 数据语义；如需扩展需用户批准 |
| institution_survey gk 键 TRIM（issue #235） | gk 保持 HEX(RECEIVE_OBJECT) / **HEX(TRIM(RECEIVE_OBJECT))** | SELECT 值 `MAX(TRIM(RECEIVE_OBJECT))` + 分组键 `HEX(TRIM(RECEIVE_OBJECT))` | gk 不 TRIM 时 'A'/'A ' 分两组残留脏行（Dolt 实证）；TRIM 后合并为单组单行 | 只 TRIM SELECT 值不 TRIM gk 无法合并分组 |
| 财务三表 TRIM 列清单（issue #235，Oracle 实证） | 三表共用同一清单 / **逐表独立** | balance_sheet 7 列**含** LISTING_STATE；cash_flow/income 6 列**无** LISTING_STATE | DDL 实证：LISTING_STATE 仅存在于 fin_balance_sheet（cash_flow/income COLS 无此列），共用清单会导致 cash_flow/income SQL 报错 | 共用清单基于错误假设（"同上 COLS 集"），会导致整表导入失败 |
| hook issue 校验（ref #213） | 逐 issue `gh issue view` / **单次 `gh issue list` 批量** | `gh issue list --repo qiboda/compass --state open --json number --limit 5000 --jq '.[].number'` 一次拉取 + 本地 `grep -qx` 查集 | 无界 API 调用（每次 commit/push 对每个唯一 issue 一次）触发限流误报；批量后每 commit/push 恰好 1 次调用；fail-closed（GH_FAIL/空集拒绝）保持与现状一致的拒绝语义 | 逐条查询在 push 大范围时 API 调用数无界（#213 根因）；`gh issue list` 默认分页 30/页须 `--limit 5000` 拉全量 |
| hook 批量查询共享（ref #213，用户 F2=B） | 提取共享脚本 / **内联重复** | commit-msg + pre-push 各自内联实现批量查询（不新建共享脚本） | 用户确认 F2=B：改动小、符合现状（两 hook 本就独立内联）；mirror-drift guard 扩展覆盖批量查询片段防一改一漏 | 共享脚本增加 hook 依赖面，违背用户明确决策 |
| index_daily 独立建表（epic #255） | 混入 stock_daily / 独立 index_daily 表 + 独立 parquet | **独立建表/独立 parquet**（compass_data 库，不写 investment_data） | 尊重 ref #201「股票数据剔除指数」决策，指数不污染选股/评分；双文件互斥使 fetch_bars 路由确定性成立 | 混入 stock_daily 重新引入 ref #201 修复的污染问题（指数占位 SEPA 过滤线） |
| index_daily 列对齐（epic #255） | 自定义列名 / 与 DuckDbProvider 查询列对齐 | DDL 按 DuckDbProvider 查询列设计（trade_date/open/high/low/close/volume/amount + PK(symbol, trade_date)），导出时 `trade_date → tradedate` 重命名 | 查询/聚合/前复权代码零改动；`adjclose=close` 占位使 factor=1.0 恒等 | 自定义列名需改 duckdb.rs 查询 SQL 与映射，波及共享路径 |
| 双 parquet 路由（epic #255） | FetchRequest 加 domain 字段 / 双文件 fallback / 独立 IndexProvider | DuckDbProvider 双文件 fallback（stock → index） | 零消息契约改动、零 GUI 调用方改动；ref #201 剔除使路由确定性成立（指数在 stock 文件必然查空）；1w/1M 聚合逻辑直接复用 | domain 字段扩大消息契约波及三处消费点；独立 provider 重复 fetch/聚合逻辑 |
| index_basic 名称表（epic #255） | 名称硬编码在 GUI / 独立名称表 | 独立 `index_basic` 表 + `index_basic.parquet`（symbol/name/index_type），全量覆盖导出 | picker 与板块列表需要名称与类型（官方/概念/行业）来源；板块由 clist 动态发现，硬编码无法覆盖新板块 | GUI 硬编码使新板块不可搜索，且名称与数据分离维护 |
| name_en/industry_en 列（epic #266） | i18n 静态键 / **数据层英文列 + collectors 静态映射表** | 数据层英文列（index_basic.name_en + stock_basic.industry_en）+ `name_en_mapping.csv` import JOIN | 数据动态增长，静态键不可维护；映射表随仓库版本可审；未收录 NULL 回退中文按需增量；全链路 Dolt→parquet→DuckDB→GUI | i18n 键需穷举全部数据名且随新增失效；数据库映射表需额外同步链路 |
| 旧 parquet 兼容（epic #266） | 硬失败 / **读取侧降级 None** | `ParquetReader` 对新列 try-fallback（binder 缺列错误 → 无列查询，`name_en: None`） | 存量 parquet 无需重导即可启动；GUI 按语言回退中文；`is_missing_column` 仅匹配具体缺列短语，genuine 错误传播 | 硬失败强迫全量重导，升级窗口大 |
| 行业后缀匹配（epic #266） | 精确匹配 / **双键（原样 + 去罗马数字后缀）** | import JOIN 条件 `m.key = TRIM(industry) OR (REGEXP 后缀 AND m.key = LEFT(len-1))` | 旧数据"白酒Ⅱ"类后缀行业可命中基础键"白酒"；双键防膨胀（`<>` guard） | 单键匹配漏掉后缀行业 |
| 腾讯回退成交额来源（issue #286） | 继续 `fqkline/get` 填 0 / **切 `newfqkline/get` 解析成交额** | 切 `newfqkline/get`，解析 day 行 index 8 成交额（万元→元），缺失/畸形降级 0 | `fqkline/get` 日线只有 6 字段无成交额，官方指数 amount 全 0；`newfqkline/get` 同域、分页参数一致，实测 30 个官方指数均返回非 0 成交额 | 继续 `fqkline/get` 无法满足官方指数成交额展示；从其它源补需引入新依赖 |
| index_daily 增量语义（issue #292） | 全量拉取 + INSERT IGNORE 去重 / **按 symbol MAX(trade_date) 真增量** | THS 行业只拉 MAX 年份→今年（MAX 为 12-31 时从次年启动）并过滤旧行；官方指数东财 `beg=MAX+1`、腾讯增量翻页遇 `<= MAX` 停止；新 symbol 全量回填；空增量（周末/停牌）按成功 no-op | 跨日 sync 不再回拉 2007 全史，避免旧年份 404/502/504 风暴与小时级耗时；空增量不误触发 fast-fail | 全量+去重不符合“增量更新”预期，单次 sync 90 行业约 1 小时、大量失败 |
| 东财板块 1000 采集废弃（issue #281） | 等待东财解封补采 1000 板块 / **采用标准行业（同花顺 90）** | 采用标准行业，板块 1000 不再补采 | 需求已改为标准行业分类，同花顺 90 行业链路已覆盖板块数据；#281 于 2026-08-20 关闭 | 补采 1000 板块需等待东财 push2his 解封、与现行行业分类重复且维护成本高 |
| collectors 代理注入粒度（issue #294） | 会话级绑定 / 每请求级 `proxies` | **每请求级包装**（`proxy_get`/`proxy_post` 等） | curl_cffi 支持每请求 `proxies`；坏代理可精确 `delete` 并换下一个，不重建会话 | 会话级绑定无法在同一请求内轮换坏代理，重建 AsyncSession 开销大且复杂 |
| 池空降级行为（issue #294） | 抛错 / 静默直连 / 醒目警告+写 state+直连 | **醒目警告+写 `proxy_pool_state.json`+直连** | 锁定决策：绝不因无代理失败，但降级必须可观测 | 抛错违背"绝不因无代理失败"；静默直连不可观测 |
| 坏代理判定（issue #294） | HTTP 非 2xx / 仅请求异常 | **仅请求异常触发 delete+轮换** | 429/5xx 多为服务端限流或业务错误，不应误杀可用代理 | HTTP 状态码由各模块既有 retry 处理，误删降低池质量 |
| 有界重试后兜底（issue #294） | 放弃请求 / 直连兜底 | **直连兜底一次**，失败交给模块既有 retry | 锁定决策"池空/坏代理均降级直连" | 放弃请求导致数据缺失，违反不因代理失败 |
| keepalive 实现（issue #294） | 独立脚本 / 并入 main.py | **独立 `proxy_keepalive.py` + `--once`** | 可后台常驻也可单轮测试/冒烟，职责单一 | 并入 main.py 增加 CLI 复杂度，且 sync 不应被 keepalive 阻塞 |
| 快照兜底解析（issue #294） | keepalive 自写下载逻辑 / 重构 fetch_freeproxy | **拆 `fetch_json_payload` + `records_from_json_data` 复用** | 单一解析/过滤/归一化实现，快照与在线源同路径 | keepalive 自写重复逻辑，后续变更易漂移 |
| append-table merge 分区键（issue #298） | 用各自 parquet 侧分区列（可能窄于 Dolt PK）/ **与生产 Dolt 全主键一致** | `block_trade` 从 `(symbol, trade_date, price)` 扩为 `(symbol, trade_date, price, volume, amount, buyer, seller)`；其余 append 表已一致；merge fallback 改为真正全量导出（不带 `--since`） | 增量 merge 按分区键去重，分区键窄于 Dolt PK 会把同窄 key 的多条真实行折叠成一行，导致 `row count mismatch` 或静默丢历史（#298 实测 block_trade 19724→8872）；fallback 用 since 过滤数据覆盖全文件同样丢历史 | 弃用 merge 改为全量重导可避开 bug 但失去增量性能；只修 block_trade 不修 fallback 无法关闭 #298 的 fallback 丢历史根因 |
