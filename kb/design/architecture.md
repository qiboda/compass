# 架构

## Compass 是什么？

Compass 是一个**本地优先的 A 股股票图表应用**。与依赖远程服务器进行每次交互的
网页股票查看器不同，Compass 将所有 OHLCV 数据导入并缓存到本地。一旦数据导入完成，
图表渲染即时完成——无需网络调用、无限流、无需 API 密钥。

它有两个面孔：

| 面孔 | 二进制 | 用途 |
|---|---|---|
| **图表应用** | `compass` | 交互式 K 线图：股票搜索、时间周期选择、十字准线、缩放、平移。通过 egui 以原生桌面窗口运行。仅从本地 Parquet 文件读取数据。 |
| **数据管线** | `compass-data` | 离线数据管理——从 Dolt 导入、导出为其他格式、备份。EastMoney 数据通过 Python collector 脚本获取。 |

两者共享同一个库 crate（`compass-core`），其中定义了数据模型、provider trait
以及所有 I/O 逻辑。

## Crate 之间的关系

```
compass (GUI binary)
  │
  ├── main.rs        ─ CompassApp (eframe::App), entry point, wiring
  ├── state.rs       ─ SharedState with Dynamic<T> reactive fields
  ├── messages.rs    ─ AppMessage, FetchRequest, FetchResponse
  ├── tabs.rs        ─ Tab/TabKind/TabViewer (egui_dock bridge)
  ├── backend.rs     ─ wire_backend, BackendHandle, AsyncDispatcher wiring
  ├── dispatcher.rs  ─ register_citizens, lifecycle draining, message routing
  ├── citizens/
  │   ├── chart.rs   ─ ChartCitizen: OHLCV candlestick chart
  │   └── logger.rs  ─ LoggerPanel: scrollable log viewer
  ├── widgets/
  │   ├── searchable_dropdown.rs ─ StockPicker widget, filter_stocks()
  │   ├── toast.rs     ─ ToastManager: 状态通知
  │   └── modal.rs     ─ Modal: 预留的对话框组件（未启用）
  │
  ├── compass-core (library)
  │     ├── model.rs      ─ shared types: AppConfig, Exchange, StockBasic, Bar
  │     ├── data/mod.rs   ─ Module declarations
  │     ├── data/provider.rs ─ DataProvider, DataWriter, NegativeCache traits
  │     ├── data/duckdb.rs   ─ DuckDbProvider (in-memory + Parquet-backed)
  │     ├── data/parquet.rs   ─ ParquetReader (main database)
  │     ├── data/symbol.rs    ─ Exchange inference, code conversion
  │     └── data/synthetic.rs ─ Test data generator
  │
  └── compass-data (CLI binary)
        └── import / import-compass / export / backup subcommands
```

`compass-core` 不包含任何 UI 代码。它提供用于获取、存储和查询股票数据的 trait
和实现。GUI 和 CLI 是薄编排层，负责连接 provider 并派发工作。

GUI 二进制（`compass`）使用 **egui-mobius citizen 模式**——一种响应式架构，其中
UI 面板被建模为 `Citizen` 结构体，通过 outbox 进行事件派发；共享状态通过
`Dynamic<T>` 响应式字段管理；异步工作通过 `Signal`/`Slot` 类型化通道路由到
运行在专用 tokio runtime 上的 `AsyncDispatcher`。

## Citizen 模式架构

核心架构挑战：**egui 在主线程上同步运行，但所有数据 I/O（HTTP、DuckDB）都需要
异步 tokio。** 如果在主线程上阻塞等待 I/O，UI 会冻结。如果在主线程上使用异步，
egui 会崩溃。

解决方案使用 **egui-mobius citizen 模式**，一种受 Elm 和 Flux 启发的 Level 3
响应式架构。三层负责分离关注点：

| 层 | 名称 | 职责 |
|---|---|---|
| **1. 表现层** | `Citizen` 面板 + `egui_dock` | 渲染 UI、发出 outbox 消息 |
| **2. 响应式状态层** | `SharedState` 与 `Dynamic<T>` | 持有应用状态；变更时自动通知 |
| **3. 异步后端层** | `Signal`/`Slot` + `AsyncDispatcher` | 在 tokio runtime 上执行 I/O；将结果写回状态 |

### Layer 1: Citizen 与 DockArea

UI 被拆分为两个 `Citizen` 面板，放置在 `egui_dock::DockArea` 内部，上方渲染
工具栏：

| Citizen | 文件 | 角色 |
|---|---|---|
| **ChartCitizen** | `citizens/chart.rs` | 通过 `egui-charts` 渲染 OHLCV K 线图。从共享状态响应式读取 `bars`，在数据变化时重新渲染。 |
| **LoggerPanel** | `citizens/logger.rs` | 可滚动日志视图。从共享状态读取日志条目。 |

面板排列在 `egui_dock::DockArea` 内，为用户提供可重排、可调整大小的选项卡式
界面。顶部工具栏提供股票搜索、交易所选择和 Fetch 按钮。

```
┌──────────────────────────────────────────────┐
│  Toolbar                                     │
│  [Symbol ▾] [Exchange ▾] [TF: 1d] [Fetch]   │
├──────────────────────────────────────────────┤
│  egui_dock::DockArea                         │
│  ┌──────────────────────────────────────────┐│
│  │  Chart (candlestick)                     ││
│  │  ┌───┬───┬───┬───┬───┬───┐              ││
│  │  │   │   │   │   │   │   │              ││
│  │  │   │   │   │   │   │   │              ││
│  │  └───┴───┴───┴───┴───┴───┘              ││
│  ├──────────────────────────────────────────┤│
│  │  Logger (scrollable)                     ││
│  └──────────────────────────────────────────┘│
└──────────────────────────────────────────────┘
```

工具栏使用 `CompassApp` 的局部状态（交易所索引、股票选择器），并在 Fetch
时直接调用 `dispatcher::handle()`。这替代了之前 ControlCitizen 使用的 outbox
模式。

```
CompassApp::ui() each frame:
  1. Render toolbar (symbol picker, exchange combo, Fetch)
  2. Render DockArea → Chart and Logger citizens
  3. Drain citizen lifecycle messages from dispatcher
  4. request_repaint_after(200ms) for continuous update
```

### Layer 2: 基于 Dynamic\<T\> 的响应式状态

状态存放在 `SharedState`（定义于 `state.rs`）中，这是一个每个字段均为
`egui_mobius_reactive` 的 `Dynamic<T>` 的结构体：

```rust
pub struct SharedState {
    pub symbol:    Dynamic<String>,         // current symbol
    pub timeframe: Dynamic<String>,         // current timeframe
    pub bars:      Dynamic<Vec<Bar>>,        // OHLCV bars
    pub loading:   Dynamic<bool>,            // fetch in-flight
    pub error:     Dynamic<Option<String>>,  // last error
    pub log:       Dynamic<Vec<String>>,     // log entries
}
```

`Dynamic<T>` 将值包装在 `Arc<RwLock<T>>` 后面，提供 `get()`、`set()` 和
`subscribe()` 方法。多个读取者共享同一底层存储——无需单独的
`Arc<Mutex<CompassState>>` 包装。

与旧版 `CompassState` + `Arc<Mutex<>>` 方案的主要区别：

- **无手动版本计数器**：`bars_version` 已移除。chart citizen 在每帧比较
  `bars.len()`；差异触发数据重建。响应式运行时也可自动通知订阅者。

- **无 Mutex 竞争**：`Dynamic<T>` 内部使用 `RwLock`，但每个字段独立。写入
  `bars` 不会锁定 `loading`，因此不同字段的读取永远不会相互竞争。

- **免克隆读取**：citizen 通过 `Dynamic::get()` 读取，返回克隆值。对于
  `Vec<Bar>`，这是 O(n) 的克隆操作——可以接受，因为每只股票的 K 线数量很小
  （每只股票不到 10,000 条）。图表仅在数量变化时重新渲染。

### Layer 3: 基于 Signal/Slot 的异步后端

不再使用手动的 `mpsc` 通道 + 工作线程循环，应用使用 egui-mobius 的 Level 3
异步派发：

```
┌─ UI THREAD (eframe) ─────────────────────┐
│                                           │
│  Toolbar (CompassApp local state)         │
│    user clicks Fetch                      │
│    dispatcher::handle(FetchBars)          │
│         │                                 │
│         ▼                                 │
│  dispatcher::handle()                     │
│    state.loading.set(true)                │
│    work_signal.send(FetchRequest) ───┐    │
│                                     │    │
└─────────────────────────────────────│────┘
                                       │
                                ┌──────▼─────────────────────┐
                                │  AsyncDispatcher (tokio)   │
                                │                             │
                                │  attach_async(work_slot,   │
                                │    result_signal,          │
                                │    |req| async {           │
                                │      reader.fetch(req)     │
                                │      → FetchResponse       │
                                │    })                      │
                                │                             │
                                └──────┬─────────────────────┘
                                       │
                                ┌──────▼─────────────────────┐
                                │  result_slot.start()       │
                                │    |resp| {                │
                                │      state.bars.set(bars)  │
                                │      state.loading.set(false)
                                │      egui_ctx.request_repaint()
                                │    }                       │
                                └────────────────────────────┘
```

连线在启动时一次性完成，位于 `backend.rs`：

1. **`factory::create_signal_slot::<FetchRequest>()`** —— 创建一个
   `Signal<FetchRequest>`（发送端）和 `Slot<FetchRequest>`（接收端）。

2. **`AsyncDispatcher::new()`** —— 持有 tokio runtime。其
   `attach_async()` 方法连接一个 `Slot<FetchRequest>`（输入）、
   一个 `Signal<FetchResponse>`（输出）和一个异步工作函数。

3. **`result_slot::start()`** —— 一个在 UI 线程上运行的闭包，每当
   `FetchResponse` 到达时执行。它将结果写入 `Dynamic<T>` 字段并调用
   `request_repaint()`。

`BackendHandle` 结构体持有 `AsyncDispatcher`。只要它存活（存储在 `CompassApp`
上），tokio runtime 就持续运行。丢弃它会干净地关闭一切。

### 线程总结

| 线程 | 角色 | 代码 |
|---|---|---|
| **主线程 (UI)** | egui 渲染、citizen outbox 排空、事件路由、result slot 处理器 | `CompassApp::ui()` |
| **AsyncDispatcher** | Tokio 多线程 runtime，接收 `FetchRequest`，运行 `DuckDbProvider`，发送 `FetchResponse` | `AsyncDispatcher`，通过 `backend.rs` |

旧模式使用手动 `std::thread::spawn` + `mpsc::channel` +
`Arc<Mutex<CompassState>>`。新模式用框架管理的原语替换了这三者：
citizen 管理表现层，`Dynamic<T>` 管理状态，`Signal`/`Slot` +
`AsyncDispatcher` 管理异步 I/O。

### DuckDB 的 spawn_blocking

DuckDB 的 C API 是同步的。所有 DuckDB 查询都在
`tokio::task::spawn_blocking` 内部运行，将阻塞工作移至专用线程池。
这使得 tokio runtime 对其他异步任务（HTTP 请求、计时器）保持响应。
这部分与之前的架构没有变化。

## 数据管线：从用户点击到图表

当您输入 `600519`、选择 `1d` 并点击 "Fetch" 时，发生以下流程：

```
UI (CompassApp::ui)
  │  user clicks "Fetch" button
  │  state.symbol.set("600519")
  │  state.timeframe.set("1d")
  │  dispatcher::handle(AppMessage::FetchBars, state, work_signal)
  │    state.loading.set(true)
  │    work_signal.send(FetchRequest { symbol:"600519", timeframe:"1d", ... })
  │
  ▼
AsyncDispatcher (tokio runtime)
  │  work_slot receives FetchRequest
  │
  ▼
DuckDbProvider::fetch_bars("600519", "1d", start, end)
  │
  ├─ 1. Query in-memory stock_daily table → cache hit? Return bars.
  │
  ├─ 2. Cache miss → read parquet_data/stock_daily.parquet via read_parquet()
  │     with WHERE symbol = ? filtering
  │
  ├─ 3. Cache-warm: INSERT OR IGNORE parquet data into in-memory table
  │     Subsequent queries hit memory, not disk
  │
  └─ 4. Return FetchResponse to result_signal
  │
  ▼
result_slot handler (called on UI thread)
  │  state.bars.set(resp.bars)
  │  state.loading.set(false)
  │  state.error.set(None or error)
  │  egui_ctx.request_repaint()
  │
  ▼
UI (next frame)
  │  ChartCitizen::show() reads state.bars.get()
  │  bars.len() differs from previous → rebuilds BarData
  │  chart.show(ui) renders candlestick chart
```

### 为什么只使用本地数据？

随着 #31 和 #32 的实现，GUI 从本地 Parquet 文件读取所有数据。无远程回退、
不使用 negative cache、无飞行中请求去重。数据管线（从 Dolt 导入、从 EastMoney
采集）离线运行；GUI 仅查询已落盘的数据。

## 数据管线：CLI (compass-data)

CLI 在 GUI 运行之前离线管理数据。其子命令形成一条管线：

```
Dolt investment_data ──import─────────► parquet_data/
Dolt compass_data ────import-compass──► parquet_data/
parquet_data/ ────────export──────────► duckdb / csv / parquet-dir
parquet_data/ ────────backup──────────► Baidu Cloud (zip)
```

项目还维护自己的 Dolt 仓库 `compass_data/`，用于自定义可变数据（公司信息、
财务指标、自选股列表），与只读的 `investment_data` 并存。查询可跨两个数据库
联结：`compass_data.stock_basic JOIN investment_data.final_a_stock_eod_price`。
使用示例见 `kb/dev/process.md#dolt-database-queries`。

### collectors：Python 数据管线

```
EastMoney API ──collectors──► CSV ──import──► compass_data (Dolt)
```

`collectors/` 目录包含 Python 脚本（uv + curl_cffi），用于从 EastMoney 公开
API 获取数据并导入 Dolt：

| 脚本 | 用途 | 数据 |
|---|---|---|
| `main.py` | 统一 CLI：fetch/import/sync/sync-investment | — |
| `fetch_stock_basic.py` | 公司基本信息 | 12,388 只股票，13 个字段 |
| `fetch_fin_indicators.py` | 财务指标 | 126K 行，37 个字段，2020-2026 |
| `fetch_balance_sheet.py` | 资产负债表 | 57 个字段，按季度，RPT_DMSK_FN_BALANCE |
| `fetch_income.py` | 利润表 | 46 个字段，按季度，RPT_DMSK_FN_INCOME |
| `fetch_cash_flow.py` | 现金流量表 | 48 个字段，按季度，RPT_DMSK_FN_CASHFLOW |

工具链：`uv`（Python 依赖管理器）+ `ruff`（lint/格式化）+
`pytest`（16 个测试）+ `mypy`（类型检查）。CI 通过 GitHub Actions 运行，
pre-commit/pre-push hooks 在每次变更时强制执行 lint + 测试。

关键设计决策：
- **curl_cffi** 而非 httpx/aiohttp：EastMoney 检查 TLS 指纹（JA3/JA4）；
  curl_cffi 模拟 Chrome 以绕过检测
- **CSV 作为中间格式**：eastmoney → CSV → Dolt，而非直接写入
- **增量模式**：状态文件（`.state.json`）记录上次获取日期；
  `--incremental` 标志仅获取新的报告期间
- **已知限制**：基于 REPORTDATE 的增量无法检测已获取期间的修订
  （例如五粮液 2025Q1 修订）。计划使用周期性 `--refresh N` 标志（见 issue #27）

### import：Dolt investment_data → Parquet
- 通过 `dolt sql -r parquet` 查询 Dolt `investment_data` 数据库
- 从 `final_a_stock_eod_price` 表中提取 6000+ 只股票（18M+ 行）
- 写入单个 `parquet_data/stock_daily.parquet` 文件，包含 `symbol` 列
- 直接写入完整数据集（无合并模式，无 `--overwrite` 标志）
- `--since` 支持增量导入较新数据
- 同时写入 `stock_basic.parquet` 和 `stock_daily.symbols.txt`

### import-compass：Dolt compass_data → Parquet
- 将我们自己的表（`stock_basic`、`fin_indicators`、`fin_balance_sheet`、
  `fin_income`、`fin_cash_flow`）导入 Parquet
- `--overwrite` 替换已有数据；默认合并/跳过（仅新增数据）
- `--since` 用于增量导入

### export：Parquet → 其他格式
- 读取 parquet_data/ 目录
- 导出为 DuckDB、CSV 或 parquet-dir 格式
- `--overwrite` 替换已有数据

### backup：Parquet → 百度云
- 使用 Python zipfile 压缩 `parquet_data/`（无系统 `zip` 依赖）
- 通过 `baidupcs` CLI（`BaiduPCS-Go`）上传到百度云
- 带时间戳的文件名：`parquet_data-YYYYMMDD-HHMMSS.zip`
- 目标文件夹：百度云上的 `/compass/`
- `--keep-zip` 标志在上传后保留本地 zip 文件

**覆盖语义**：`import-compass` 和 `export` 默认合并/跳过——已有数据保留，
仅添加新数据。传入 `--overwrite` 进行替换。`import` 始终直接从 Dolt 写入
完整数据集。

## 存储策略：为什么同时使用 DuckDB 和 Parquet？

```
Compass uses two database formats for different purposes:

  Parquet files (parquet_data/)
    ├─ Source of truth — the canonical data store
    ├─ Stock basic: stock_basic.parquet (one file for all symbols)
    ├─ Stock daily: stock_daily.parquet (single file with symbol column)

  DuckDB (in-memory for GUI, file-backed for export)
    ├─ GUI — in-memory with Parquet fallback (reads parquet_data/ on cache miss)
    ├─ CLI export — file-backed DuckDB output (compass.duckdb)
    └─ no_data_marks — negative cache table (trait implemented, GUI does not use)
```

### 为什么以 Parquet 作为唯一数据源？

- **列式存储**：DuckDB 查询仅读取需要的列（例如 `SELECT close` 只读取 close
  列）。对于跨数千条 K 线的分析查询，比行式格式快得多。
- **按股票分区**：每只股票一个文件。添加新股票就是新建一个文件——无需重建表。
  删除就是 `rm`。
- **可直接查询**：DuckDB 的 `read_parquet()` 函数允许直接用 SQL 查询 Parquet
  文件，无需将其加载到表中。
- **可移植**：Parquet 是开放标准。可以用 Python（pandas、polars）、R 或任何
  DuckDB 实例打开。无厂商锁定。
- **紧凑**：列式压缩减少存储。6000+ 只股票 × 30 年 ≈ 可管理的磁盘占用。

### 为什么用 DuckDB 做缓存？

- **写入友好**：INSERT OR REPLACE/IGNORE 语义；自动处理主键冲突。Parquet
  仅支持追加，更难更新。
- **内存模式**：测试使用 `DuckDbProvider::new_in_memory()` 创建完全隔离的
  数据库，零清理成本。
- **内置捆绑**：`duckdb` crate 捆绑 C 库——无系统依赖。
- **OLAP 优化**：DuckDB 专为分析工作负载（聚合、窗口函数、时序查询）构建，
  完美映射股票数据。

### 读取路径

GUI 通过 `DuckDbProvider`（内存 DuckDB，缓存未命中时用 `read_parquet()`
回退）从 Parquet 读取数据。CLI 将导出数据写入文件-backed DuckDB。这种
两层设计分离了"快速写入与缓存"（DuckDB）和"持久、可查询存储"（Parquet）
的关注点。

## 符号约定：Dolt-native 前缀代码

Compass 中的每只股票由其带交易所前缀的 Dolt-native 符号标识：
`"SZ000001"`、`"SH600519"`、`"BJ836149"`。2 字母前缀（SZ/SH/BJ）是规范
标识符的一部分——它出现在 Parquet 文件名中、数据库列中以及 API 中。
为方便使用，接受裸 6 位数字输入，并通过交易所推断解析。

旧版 `ts_code` 格式（`"000001.SZ"`）已废弃，因为它将标识与元数据混在一起：
交易所已经可以从代码区间推断，后缀是冗余的。

完整的市场分段、交易所推断规则、显式前缀和时间周期映射见
`kb/design/symbols.md`。

## 配置系统

Compass 在启动时从 `~/.config/compass/config.toml` 加载配置。所有字段均为
可选——缺失的键回退到 `AppConfig::default()` 中定义的合理默认值。

```toml
[parquet]
dir = "/data/compass-data/parquet_data"   # parquet data directory

[dolt]
investment_data_dir = "/data/compass-data/investment_data"
compass_data_dir = "/data/compass-data/compass_data"

[app]
default_symbol = "000001"     # what to show on startup
default_timeframe = "1d"
```

配置路径为 `$HOME/.config/compass/config.toml`。如果文件不存在或无法解析，
应用以所有默认值启动——无需手动设置。完整参考见 `kb/user/config.md`。

## 日志

日志同时写入**两个输出端**：

1. **stderr** —— 始终输出；级别由 `RUST_LOG` 环境变量控制
2. **`logs/compass.log`** —— 每日滚动文件，ANSI 已剥离

```sh
RUST_LOG=debug cargo run    # verbose: see every HTTP request, DuckDB query
RUST_LOG=info cargo run     # normal: state transitions, fetch counts, errors
RUST_LOG=warn cargo run     # quiet: only problems
```

文件 appender 使用 `tracing-appender` 的每日滚动——每天一个新文件
（`compass.log.2025-07-23`），当天始终是 `compass.log`。

## 库选型

Compass 中的每个库选择都是经过深思熟虑的。以下是每个库的选择理由：

| # | 决策 | 选择 | 理由 |
|---|---|---|---|
| 1 | GUI 框架 | egui 0.35 + eframe | 纯 Rust 即时模式 GUI。无 HTML/CSS/JS，无 webview 依赖。编译为单个原生二进制文件。 |
| 2 | 图表组件 | egui-charts（qiboda fork，`compass` 分支） | K 线图，内置平移、缩放、十字准线。与 egui 生态系统匹配。从上游 fork 以进行 compass 特定修复。 |
| 3 | 异步运行时 | tokio（rt-multi-thread） | DuckDbProvider 使用 tokio::spawn_blocking 处理同步 DuckDB 查询。CLI 使用 current_thread 以简化。 |
| 4 | HTTP 客户端 | reqwest 0.12（rustls-tls） | 库用于 `DataError::Network`。GUI 无直接 HTTP 依赖——所有数据均为本地。 |
| 5 | 数据库 | duckdb 1.0（bundled） | OLAP 优化的列式引擎。原生读写 Parquet。`bundled` feature 自带 C 库——无需系统 duckdb。 |
| 6 | 数据库线程 | spawn_blocking + Mutex | DuckDB 是同步 C 库。`spawn_blocking` 将查询移至线程池，不阻塞异步运行时。DuckDB 连接上的 Mutex 确保独占访问。 |
| 7 | 序列化 | serde + serde_json | 配置解析和测试数据。所有数据类型派生 serde。 |
| 8 | 时间处理 | chrono 0.4 | UTC 时间戳、日期算术（range_start/end 计算）、JSON 解析支持。 |
| 9 | 错误类型 | thiserror 2（库）、anyhow 1（二进制） | 库中使用精确的 `DataError` 枚举，带 `From` 实现以支持 `?` 传播。二进制入口点使用 `anyhow` 包装上下文。 |
| 10 | 日志 | tracing + subscriber + appender | 结构化、异步、级别过滤。通过 tracing-appender 每日滚动文件。 |
| 11 | 异步 trait | async-trait 0.1 | Rust 原生异步 trait 仍不稳定。此宏是标准替代方案。 |
| 12 | 配置 | toml → Deserialize | 简单、可读的格式。每个字段 `#[serde(default)]` 使部分配置可用。 |
| 13 | CLI 参数 | clap 4（derive） | derive 宏从结构体生成 CLI 解析器。类型安全、自文档化。 |
| 14 | 进度条 | indicatif 0.17 | 长时间运行的 CLI 操作（导入）的 spinner + 进度条。 |
| 15 | 并发 | futures Semaphore + buffer_unordered | 批量导入的有界并行。Semaphore 限制并发操作；buffer_unordered 在结果到达时处理，同时保持顺序。 |
| 16 | 响应式状态 | egui_mobius_reactive `Dynamic<T>` | 每个字段的 `Dynamic<T>` 替代了单体 `Arc<Mutex<CompassState>>`。无手动版本计数器，无跨字段锁竞争。每个字段可独立读写。 |
| 17 | Citizen 模式 | egui_citizen（Citizen trait） | 框架管理 citizen 生命周期（register、activate、deactivate、drain），消除手动线程布线。Citizen 使用 outbox 模式——不直接耦合后端。 |
| 18 | 停靠布局 | egui_dock 0.20 | 可停靠的选项卡式面板，支持调整大小和重排。通过 TabViewer 桥接到 citizen 激活。替代手动面板布局。 |
| 19 | 异步派发 | egui_mobius `Signal`/`Slot` + `AsyncDispatcher` | 类型化通道替代 `mpsc::channel` 进行命令派发。`AsyncDispatcher` 管理自己的 tokio runtime——无需 `std::thread::spawn` + `rt.block_on` 样板代码。 |
| 20 | Provider trait | DataProvider + DataWriter + NegativeCache | 基于 trait 的数据后端抽象：DuckDB、Parquet——全部在同一接口后面。可通过 mock 实现进行测试。 |
| 21 | Parquet 存储 | DuckDB read_parquet + COPY TO | 按股票分区的列式格式。无需加载到表中即可查询。 |
| 22 | Dolt 导入 | dolt CLI → Parquet（直接） | 18M+ 行的离线批量导入。Dolt `sql -r parquet` 直接写入二进制 Parquet，跳过 CSV 中间步骤。 |

## 延伸阅读

- **数据提供者**：`kb/design/data-providers.md` —— trait 体系及各 provider
  实现的深入说明
- **符号约定**：`kb/design/symbols.md` —— 市场分段、代码转换、时间周期映射
- **API 参考**：`cargo doc --open` —— 所有公开 API 的完整类型级文档

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 数据访问策略：GUI 读取数据的来源 | 在线 API 直接请求 / 本地文件缓存 / 纯本地无回退 | 纯本地 Parquet 文件，无在线回退 | 本地读取零延迟、无网络依赖、无 API 限流；数据管线（import/collector）离线运行，GUI 只查询已落盘数据 | 在线 API 增加延迟和失败点；缓存策略需处理过期和同步问题，增加复杂度 |
| 异步架构：UI 线程与 I/O 分离方案 | 手动 std::thread + mpsc / 框架托管的 citizen 模式 | egui-mobius citizen 模式：Citizen trait + Dynamic\<T\> + Signal/Slot + AsyncDispatcher | 消除手动线程布线、Arc\<Mutex\> 竞争和版本计数器；Citizen 通过 outbox 解耦，AsyncDispatcher 自管 tokio runtime | 手动线程方案代码量大、易出错；Dynamic\<T\> 提供字段级独立读写，无跨字段锁竞争 |
| 规范存储格式：Parquet 单文件 vs 其他方案 | 每标的单独文件 / 单文件含 symbol 列 / DuckDB 做主存储 | 单个 `stock_daily.parquet`，symbol 列分区查询 | 列式存储、谓词下推、开放标准、工具链兼容（Python/R/DuckDB）；单文件管理简单，无需处理数千个文件 | 单文件追加困难（写入需重写整个文件），但通过 `import --since` 增量导入缓解；每标的单独文件增加文件管理开销 |
| 测试覆盖率门槛：CI 强制 80% vs 无门槛 | 无门槛（continue-on-error）/ 总覆盖率 80% / 总 + 每 crate 各 80% + Python 全量 80% | 总 ≥80% 且每 crate（core/data/compass）各 ≥80%，Python `--cov=.` 全量 ≥80% | 防止核心库高覆盖率拉平 GUI/CLI 短板；GUI 以 egui_kittest 无头集成测试达成；Python 未测文件按 0% 计，杜绝假达标 | 仅总覆盖率可被高覆盖模块掩盖；单门槛无法约束 Python 侧 |

符号约定（Dolt-native 前缀格式 vs ts_code）的决策记录见 `kb/design/symbols.md`。
