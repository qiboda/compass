# 架构

## Compass 是什么？

Compass 是一个**本地优先的 A 股股票图表应用**。与依赖远程服务器进行每次交互的
网页股票查看器不同，Compass 将所有 OHLCV 数据导入并缓存到本地。一旦数据导入完成，
图表渲染即时完成——无需网络调用、无限流、无需 API 密钥。

它有两个面孔：

| 面孔 | 二进制 | 用途 |
|---|---|---|
| **图表应用** | `compass` | 交互式 K 线图：股票搜索、时间周期选择、十字准线、缩放、平移。通过 egui 以原生桌面窗口运行。仅从本地 Parquet 文件读取数据。 |
| **数据管线** | `compass-data` | 离线数据管理——从 Dolt 导入、导出为其他格式、备份。EastMoney 数据通过 Python collector 脚本获取；官方指数在东财失败/empty 时回退腾讯源（issue #278）。 |

两者共享同一个库 crate（`compass-core`），其中定义了数据模型、provider trait
以及所有 I/O 逻辑。

## Crate 之间的关系

```
compass (GUI binary)
  │
  ├── main.rs        ─ CompassApp (eframe::App), entry point, wiring
  ├── state.rs       ─ SharedState with Dynamic<T> reactive fields
  ├── messages.rs    ─ AppMessage, FetchRequest/FetchResponse, RunScreenerRequest/Response
  ├── tabs.rs        ─ Tab/TabKind/TabViewer (egui_dock bridge)
  ├── backend.rs     ─ wire_backend, BackendHandle, AsyncDispatcher wiring (2 channels)
  ├── dispatcher.rs  ─ register_citizens, lifecycle draining, message routing
  ├── citizens/
  │   ├── chart.rs   ─ ChartCitizen: OHLCV candlestick chart (空态 EmptyState)
  │   ├── logger.rs  ─ LoggerPanel: scrollable log viewer + 导出按钮
  │   └── screener.rs ─ ScreenerPanel: condition form (Card 分区) + DataTable
  │
  ├── compass-ui (library — 通用 GUI 组件库，零业务依赖)
  │     ├── tokens/      ─ design token 六类（color/spacing/typography/radius/shadow/motion）
  │     ├── theme/       ─ CompassTheme: token → egui::Visuals/Style 直构 + chart 薄封装
  │     ├── fonts.rs     ─ 思源黑体 + JetBrains Mono 全内嵌注册
  │     ├── dock_style.rs ─ egui_dock 0.20 Style 深度定制
  │     └── widgets/     ─ atoms + molecules（Button/Modal/Toast/Sidebar/StatusBar/
  │                        DataTable/MultiSelect/SearchableDropdown/... 16+8）
  │
  ├── compass-core (library)
  │     ├── model.rs      ─ shared types: AppConfig, Exchange, StockBasic, CrossSectionBar, Bar
  │     ├── data/mod.rs   ─ Module declarations
  │     ├── data/provider.rs ─ DataProvider, DataWriter, NegativeCache traits
  │     ├── data/duckdb.rs   ─ DuckDbProvider (in-memory + Parquet-backed)
  │     ├── data/parquet.rs   ─ ParquetReader (main database, fetch_cross_section)
  │     ├── data/symbol.rs    ─ Exchange inference, code conversion
  │     └── data/synthetic.rs ─ Test data generator
  │
  ├── compass-types (library)
  │     ├── lib.rs      ─ ScreenerQuery/ScreenerRow/MaCondition/... (cross-crate boundary types)
  │     └── screener.rs ─ Filter AST（Filter/MetaCond/SeriesFactor/SeriesCond/CmpOp/FactorRef
  │                        + From<ScreenerQuery> for Filter 编译层）
  │
  ├── compass-strategy (library)
  │     ├── lib.rs              ─ run_screener 选股引擎（元数据 + 技术面条件，收 &Filter）
  │     └── screener_series.rs  ─ 序列函数（up_days / count_in_window / volume_surge）
  │
  └── compass-data (CLI binary)
        └── import / import-compass / export / backup subcommands
```

`compass-core` 不包含任何 UI 代码。它提供用于获取、存储和查询股票数据的 trait
和实现。GUI 和 CLI 是薄编排层，负责连接 provider 并派发工作。

依赖方向：`compass → compass-ui`（UI 组件库，compass-ui 零业务依赖）、
`compass → compass-strategy → compass-core`，`compass-strategy → compass-types`，
`compass → compass-types`；`compass-core` 不依赖 `compass-types`（无循环）。

GUI 二进制（`compass`）使用 **egui-mobius citizen 模式**——一种响应式架构，其中
UI 面板被建模为 `Citizen` 结构体，通过 outbox 进行事件派发；共享状态通过
`Dynamic<T>` 响应式字段管理；异步工作通过 `Signal`/`Slot` 类型化通道路由到
运行在专用 tokio runtime 上的 `AsyncDispatcher`。

## 选股器表达式 AST（epic #243，Batch 1-3）

选股器条件在 `compass-types` 中有一个可序列化的表达式 AST（`screener.rs`），
作为 config 持久化与未来 LLM 输出（Batch 4）的统一格式。旧版 `ScreenerQuery`
保留为兼容层/迁移面，单向编译成 AST；引擎入口 `run_screener(&Filter, ...)`
直接以通用递归求值器（`screener_eval.rs`，Batch 3）逐 symbol 求值——不再有
受限反向转换。

### 类型系统

```
Filter        Meta(MetaCond) | Series(SeriesCond) | And(Vec<Filter>) | Or(Vec<Filter>) | Not(Box<Filter>)
MetaCond      Industry(Vec<String>) | Exchange(Vec<String>) | Board(Vec<String>) | ListYears(u32)
              | Delisted(bool) | MarketCap{min, max}（亿元，None 侧不限）
SeriesFactor  Close | Sma(u32) | ChangePct(u32) | DayPct | AvgVolume(u32) | NDayHigh(u32)
CmpOp         Eq | Ne | Gt | Ge | Lt | Le（serde snake_case）
FactorRef     Const(f64) | Factor(SeriesFactor)
SeriesCond    Cmp{factor, op, value} | UpDays{n, min_pct} | Count{factor, op, value, window, at_least}
              | VolumeSurge{days, times}
```

`Filter` 是递归 tag-union：`Meta` 元数据约束、`Series` 序列条件、`And`/`Or`/`Not`
布尔组合。`MetaCond` 集合语义（`Industry(vec![...])`）让多选 OR 比嵌套 `Or`
简洁；`UpDays` 内建谓词（通达信 UPNDAY 先例）语义清晰，`Count` 为通用兜底。
全部枚举 derive serde（tagged JSON；未知 tag / 缺非 Option 字段反序列化报错，
Option 字段缺省 `None`），并实现 `and`/`or`/`not` 方法与 `&`/`|`/`~` 运算符重载
（Zipline 式构造体验）。`Filter`/`SeriesCond` 无 `Default`——空查询由编译层以
`And(vec![])` 表达；仅 `MetaCond`（`Industry(vec![])`）与 `SeriesFactor`
（`Close`）实现 `Default`。

**C1：`Cmp.value` 类型为 `FactorRef`**（`Const(f64)` / `Factor(SeriesFactor)`）——
因子间比较（`Close > Sma(20)`、`Sma(5) > Sma(20)`、`Close > NDayHigh(days)`）
用普通 `f64` 无法表达；`FactorRef` 统一比较两侧，无需新增独立 `CmpFactor`
变体。

### Crate 归属

- **AST 类型 → `compass-types`**：跨 crate 边界类型，与 `ScreenerQuery` 同域；
  serde 使 config 持久化与 LLM 输出共用同一格式。`From<ScreenerQuery> for Filter`
  单向编译层覆盖全部 11 类既有条件（BullishAlign → 嵌套 `And(Cmp{Sma(5),Gt,
  Sma(20)}, Cmp{Sma(20),Gt,Sma(60)})`，momentum → 双边界嵌套 `And`；空查询 →
  `And(vec![])`）。
- **求值器 → `compass-strategy`**（`screener_eval.rs`）：`evaluate(filter, basic,
  series, now) -> bool` 递归求值整个 AST——Meta 走 `StockBasic`（含
  `Delisted(true)` 仅退市语义），Series 走 bars 扫描（Cmp/UpDays/Count/
  VolumeSurge），And/Or/Not 布尔组合；窗口不足/NaN → 不匹配（false），不
  panic、不 NaN。`run_screener` 逐 symbol 调用求值器后组装行（市值
  `total_share × latest.close / 1e8` 亿元、20 日涨幅显示列），市值降序截断
  `MAX_RESULTS`。
- **持久化 → `compass`**（main.rs `ScreenerSection`）：`[screener]` 节
  `filter = "<Filter JSON>"` 新格式（AST 原样持久化）；旧 11 键扁平
  `ScreenerQuery` 仍可读取（`#[serde(flatten)]` 兼容），`resolve` 优先新格式、
  缺失回退 `Filter::from(legacy)`，坏 JSON 回退默认不崩溃。旧配置首次加载可读、
  首次保存后迁移为新格式。

### 序列函数（screener_series.rs）

`up_days`/`count_in_window`/`volume_surge` 三个纯函数，遵循 sepa/indicators.rs
契约：窗口不足返回 `None`、NaN/非有限输入返回 `None`、零除防护、不 panic。
`volume_surge` 匹配引擎语义（近 `days` 日均量 ≥ `times` × 近 `3×days` 日均量，
基线嵌套含近期窗口）。求值器（Batch 3）直接接线这三个函数；Sma/ChangePct/
NDayHigh/DayPct/AvgVolume 作为 factor 在 `screener_eval::factor_at` 内联求值
（按索引滑窗，窗口不足 → 该日不计入），不单独 pub。

### LLM 基础设施（epic #243 Batch 4，ref #247）

自然语言 → Filter AST 链路（供未来 #153 行业新闻分析复用客户端）：

- **通用 LLM 客户端 → `compass-core::llm`**（`LlmConfig{base_url, api_key,
  model}` + `LlmClient::chat_json(system, user) -> Result<Value, LlmError>`）：
  OpenAI 兼容 `POST {base_url}/chat/completions`，reqwest 直调（无 SDK），
  60s 超时，`response_format: json_object`。`LlmError` 五变体
  （EmptyConfig/Network/Http{status,body}/NoContent/InvalidJson）。crate 归属
  理由：跨 GUI/未来 CLI 复用，reqwest/serde_json/httpmock 依赖已就位。
- **业务层 → `compass`**（`llm_screener.rs`）：`build_screener_prompt`（system
  prompt 内嵌 Filter AST serde schema + 枚举 + 示例 + 严格 JSON 约束）与
  `parse_filter_response`（strip 围栏 → serde 反序列化 → 语义校验）。纯函数，
  可单测。
- **语义校验 → `compass-types::validate_filter`**：窗口/计数参数 > 0、
  `Count.at_least ≤ window`、`MarketCap.min ≤ max`、f64 有限性（NaN/Inf 拒绝）、
  递归深度上限 32（防栈溢出）。空 `And/Or` 合法（构建器空状态）。
- **通道 → `compass::backend` 第五 `AsyncDispatcher`**（`RunLlmRequest{prompt,
  seq}` / `RunLlmResponse{filter, error, seq}`）：handler 串起
  prompt → chat_json → parse_filter_response；`seq` 守卫丢弃被取消/过期的
  在途响应（Esc 取消安全）。响应写 `SharedState.llm_result`，GUI 在
  loading→idle 迁移时消费并入构建器。
- **配置 → `[llm]` 节**（`.dsh/kb/user/config.md`）：api_key 缺省 = 入口隐藏、
  零网络请求；base_url/model 缺省回退默认。

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
| **ScreenerPanel** | `citizens/screener.rs` | 条件选股器：条件输入表单 + 结果表格（排序、计数、点击行切换图表）。通过第二条 Signal/Slot 通道把 `RunScreenerRequest` 发给后端，`run_screener` 在 tokio 上执行。 |

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
                                │      logger.log_info(...)  │
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
   `FetchResponse` 到达时执行。它将结果写入 `Dynamic<T>` 字段，**先写
   显示日志再清除 `loading`**（fetch/screener/SEPA/index 四个 slot 同构，
   保证 `loading==false` 可观察时日志已存在，ref #276），最后调用
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

当您在输入框搜索 `600519`（或 `SH600519`/`sh.600519`，D11 自由文本）、
选择 `1d` 并点击 "Fetch" 时（提交值规范化为带前缀的 `SH600519`），
发生以下流程：

```
UI (CompassApp::ui)
  │  user clicks "Fetch" button
  │  state.symbol.set("SH600519")
  │  state.timeframe.set("1d")
  │  dispatcher::handle(AppMessage::FetchBars, state, work_signal)
  │    state.loading.set(true)
  │    work_signal.send(FetchRequest { symbol:"SH600519", timeframe:"1d", ... })
  │
  ▼
AsyncDispatcher (tokio runtime)
  │  work_slot receives FetchRequest
  │
  ▼
DuckDbProvider::fetch_bars("SH600519", "1d", start, end)
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
使用示例见 `.dsh/kb/dev/database.md`。

### collectors：Python 数据管线

```
EastMoney API ──collectors──► CSV ──import──► compass_data (Dolt)
```

`collectors/` 目录包含 Python 脚本（uv + curl_cffi），用于从 EastMoney 公开
API 获取数据并导入 Dolt：

| 脚本 | 用途 | 数据 |
|---|---|---|
| `main.py` | 统一 CLI：fetch/import/progress/sync/sync-investment | — |
| `fetch_stock_basic.py` | 公司基本信息 | 12,388 只股票，13 个字段 |
| `fetch_fin_indicators.py` | 财务指标 | 126K 行，37 个字段，2020-2026 |
| `fetch_balance_sheet.py` | 资产负债表 | 319 个字段，按季度，RPT_F10_FINANCE_GBALANCE |
| `fetch_income.py` | 利润表 | 203 个字段，按季度，RPT_F10_FINANCE_GINCOME |
| `fetch_cash_flow.py` | 现金流量表 | 254 个字段，按季度，RPT_F10_FINANCE_GCASHFLOW |

工具链：`uv`（Python 依赖管理器）+ `ruff`（lint/格式化）+
`pytest`（16 个测试）+ `mypy`（类型检查）。CI 通过 GitHub Actions 运行，
pre-commit/pre-push hooks 在每次变更时强制执行 lint + 测试。

关键设计决策：
- **curl_cffi** 而非 httpx/aiohttp：EastMoney 检查 TLS 指纹（JA3/JA4）；
  curl_cffi 模拟 Chrome 以绕过检测
- **CSV 作为中间格式**：eastmoney → CSV → Dolt，而非直接写入
- **增量模式**：状态文件（`{REPORT_NAME}.state.json`）记录 `last_update_date` 与
  `last_report_date`；财务指标与财务三表（balance_sheet/income/cash_flow）的
  `--incremental` 使用 **UPDATE_DATE 时间锚点**（`UPDATE_DATE>='{anchor}'`），
  一次拉取新披露与历史修订，不再按 REPORT_DATE 报告期枚举；导入使用
  **merge + `INSERT ... ON DUPLICATE KEY UPDATE`**（ODKU），历史永不丢失、
  同 PK 修订覆盖旧值（issue #135 / #299）
- **无 anchor 首跑**：财务三表 `--incremental` 在无 anchor 时固定
  `2020-01-01` 走 UPDATE_DATE 全历史拉取，不回退 REPORT_DATE 枚举（issue #299）

### 自动回补缺失数据（issue #308）

数据管线允许不每天运行：`collectors/main.py sync` 在采集前用
`investment_data.ts_trade_day_calendar`（SSE `is_open=1`）与各日频表 Dolt
现有日期做缺口扫描，缺失时自动回补——`capital_main_flow` 走 EastMoney
`fflow/daykline` 逐股历史 API，`index_daily`/`dragon_list`/`block_trade`
按显式范围回补；回补失败严格 abort。`scripts/update-database.sh`
（原每日一键脚本，已彻底改名）在 import-compass 之后调用
`sepa backfill-dates` 补算缺失的 SEPA 派生表，并在 import 后通过
`check-stock-daily` 硬校验 `stock_daily.parquet` 的交易日历缺口。

### import：Dolt investment_data → Parquet
- 通过 `dolt sql -r parquet` 查询 Dolt `investment_data` 数据库
- 从 `final_a_stock_eod_price` 表中提取 6000+ 只股票（18M+ 行）
- 写入单个 `parquet_data/stock_daily.parquet` 文件，包含 `symbol` 列
- 直接写入完整数据集（无合并模式，无 `--overwrite` 标志）
- `--since` 支持增量导入较新数据
- 同时写入 `stock_daily.symbols.txt`

### import-compass：Dolt compass_data → Parquet
- 将我们自己的表（`stock_basic`、`fin_indicators`、`fin_balance_sheet`、
  `fin_income`、`fin_cash_flow`）导入 Parquet
- `--overwrite` 替换已有数据；默认合并/跳过（仅新增数据）
- `--since` 用于增量导入
- append 表（fin_*、capital_main_flow、dragon_list、block_trade、
  institution_survey、index_daily）的 merge 分区列必须与生产 Dolt 全主键一致；
  merge 失败 fallback 改为不带 `--since` 的真全量导出（ref #298）

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
输入层（CLI `--symbols`、GUI 提交、配置）只接受显式前缀输入——不带前缀的
代码一律拒绝（D9）；旧配置中的裸码值在加载时自动迁移补前缀（D10）。
GUI 搜索框接受自由文本（D11），但选中/提交值规范化为前缀形式。

旧版 `ts_code` 格式（`"000001.SZ"`）已废弃，因为它将标识与元数据混在一起：
交易所已经可以从代码区间推断，后缀是冗余的。

完整的市场分段、交易所推断规则、显式前缀和时间周期映射见
`.dsh/kb/design/symbols.md`。

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
default_symbol = "SZ000001"     # what to show on startup
default_timeframe = "1d"
```

配置路径为 `$HOME/.config/compass/config.toml`。如果文件不存在或无法解析，
应用以所有默认值启动——无需手动设置。完整参考见 `.dsh/kb/user/config.md`。

## 日志

日志同时写入**两个输出端**：

1. **stderr** —— 始终输出；级别由 `RUST_LOG` 环境变量控制
2. **`logs/compass.log`** —— 每日滚动文件，ANSI 已剥离

```sh
RUST_LOG=debug scripts/run.sh    # verbose: see every HTTP request, DuckDB query
RUST_LOG=info scripts/run.sh     # normal: state transitions, fetch counts, errors
RUST_LOG=warn scripts/run.sh     # quiet: only problems
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

- **数据提供者**：`.dsh/kb/design/data-providers.md` —— trait 体系及各 provider
  实现的深入说明
- **符号约定**：`.dsh/kb/design/symbols.md` —— 市场分段、代码转换、时间周期映射
- **API 参考**：`cargo doc --open` —— 所有公开 API 的完整类型级文档

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|------|------|------|------|----------|
| 数据访问策略：GUI 读取数据的来源 | 在线 API 直接请求 / 本地文件缓存 / 纯本地无回退 | 纯本地 Parquet 文件，无在线回退 | 本地读取零延迟、无网络依赖、无 API 限流；数据管线（import/collector）离线运行，GUI 只查询已落盘数据 | 在线 API 增加延迟和失败点；缓存策略需处理过期和同步问题，增加复杂度 |
| 异步架构：UI 线程与 I/O 分离方案 | 手动 std::thread + mpsc / 框架托管的 citizen 模式 | egui-mobius citizen 模式：Citizen trait + Dynamic\<T\> + Signal/Slot + AsyncDispatcher | 消除手动线程布线、Arc\<Mutex\> 竞争和版本计数器；Citizen 通过 outbox 解耦，AsyncDispatcher 自管 tokio runtime | 手动线程方案代码量大、易出错；Dynamic\<T\> 提供字段级独立读写，无跨字段锁竞争 |
| 规范存储格式：Parquet 单文件 vs 其他方案 | 每标的单独文件 / 单文件含 symbol 列 / DuckDB 做主存储 | 单个 `stock_daily.parquet`，symbol 列分区查询 | 列式存储、谓词下推、开放标准、工具链兼容（Python/R/DuckDB）；单文件管理简单，无需处理数千个文件 | 单文件追加困难（写入需重写整个文件），增量导入由 `import-compass` 的 merge 语义承担（`import` 总是全量直写，`--since` 仅过滤子集 + 覆盖全文件，非增量）；每标的单独文件增加文件管理开销 |
| 测试覆盖率门槛：CI 强制 80% vs 无门槛 | 无门槛（continue-on-error）/ 总覆盖率 80% / 总 + 每 crate 各 80% + Python 全量 80% | 总 ≥80% 且每 crate（core/data/compass）各 ≥80%，Python `--cov=.` 全量 ≥80% | 防止核心库高覆盖率拉平 GUI/CLI 短板；GUI 以 egui_kittest 无头集成测试达成；Python 未测文件按 0% 计，杜绝假达标 | 仅总覆盖率可被高覆盖模块掩盖；单门槛无法约束 Python 侧 |
| C1（#244）：`Cmp.value` 的类型 | `f64` / `FactorRef` / 新增独立 `CmpFactor` 变体 | `FactorRef { Const(f64), Factor(SeriesFactor) }` | 因子间比较（`Close>Sma(20)`、`Sma(5)>Sma(20)`、`Close>NDayHigh(days)`）用普通 `f64` 不可表达；`FactorRef` 统一比较两侧，对 `Cmp` 变体侵入最小，serde JSON 形态单一 | 独立 `CmpFactor` 变体分裂比较形态，反向转换与求值都要处理两个变体；纯 `f64` 无法表达 MA/突破等既有条件 |
| C2（#244）：BullishAlign 的 AST 映射 | handoff 表原文 `Close>Sma(20) && Sma(20)>Sma(60)` / 引擎语义 `Sma(5)>Sma(20) && Sma(20)>Sma(60)` | 引擎语义（ma5>ma20 && ma20>ma60，strategy lib.rs:233-238） | 与 `screen_symbol` 引擎实现一致，行为保持（编译不改变筛选结果） | handoff 表原文与引擎不符，按它编译会改变筛选语义 |
| C3（#244）：序列函数范围 | 3 个（UpDays/Count/VolumeSurge）/ 6 个（+Sma/ChangePct/NDayHigh） | 3 个 | 其余 3 个已有私有 helper（strategy lib.rs:221-259）可复用，避免 Batch 3 前的死代码；偏差已在 PR/issue 说明 | 6 个独立函数在 Batch 1 无调用方，纯冗余 |
| M4（#244）：`run_screener(&Filter)` 的 Batch 1 执行路径 | 通用 Filter 求值器（Batch 3）/ 受限私有反向转换 / 保持旧签名 | 受限私有反向转换（`filter_to_query` accept-grammar）+ `ScreenerError::UnsupportedFilter` | 不改引擎逻辑、GUI 语义不变、Batch 1 零风险；Batch 3 再实现真求值器 | 通用求值器属 Batch 3 范围，提前实现违背批次边界；保持旧签名则 AST 无消费者 |
| 覆盖率门槛（#244）：compass-types | 维持 80% / 提升至 95% | 95% | issue #244 验收标准，用户已确认（2026-08-12）；改动 check-coverage.sh / ci.yml / AGENTS.md / .dsh/kb/dev/testing.md | 维持 80% 达不到 #244 验收要求 |
| exclude_delisted 缺失语义（#244） | 布尔直接编码（`Delisted(true/false)`）/ 存在性编码（仅 `Delisted(false)` 产出，缺失即不排除） | `exclude_delisted: true` → `Meta(Delisted(false))`；`false` → 不产出节点；反向按"存在 → true、缺失 → false"还原 | 存在性编码对 bool 无损，且与 `ScreenerQuery::default()`（exclude_delisted=true）匹配——默认查询产出裸 `Delisted(false)` 节点而非空 `And` | 布尔直接编码无法区分"false 与未设置"；`Delisted(true)`（仅退市）在 ScreenerQuery 中不可表达，反向只能拒绝 |
| B1（#246）：Filter 求值入口 | 通用递归求值器 / 保留受限反向转换 / 扩展 accept-grammar | 通用递归求值器（`screener_eval.rs`，`evaluate() -> bool`），删除 `filter_to_query` 全套机制 | issue #246 验收要求 UpDays/Count/Or/Not 真实过滤——受限文法无法表达；通用求值器消灭"两套类型 + 受限文法"中间层，与 GUI/LLM 共享同一 AST | 保留/扩展受限文法违背 Batch 3 目标；ScreenerQuery 仅保留为 config 迁移面 |
| B2（#246）：求值语义基准 | 逐条对照新实现 / 复刻既有 `screen_symbol` 语义 | 复刻 `screen_symbol`（ma 含最新 N 根、breakout 前 N 根不含最新、momentum 含 base、volume 3N 嵌套基线、missing total_share + cap 条件剔除、delisted 默认排除） | 21 个既有语义集成测试是回归基线，断言不允许改 | 新语义会破坏既有测试契约与用户预期 |
| B3（#246）：`Delisted(true)` 求值 | 拒绝（延续 #244）/ 支持 | 求值器支持：`delist_date.is_some()` 匹配仅退市 | 通用求值器完整支持 AST——`From<ScreenerQuery>` 永不产出但求值器处理；语义自然 | 拒绝会留下 AST 无法求值的死形状 |
| B4（#246）：持久化格式 | 继续存 `ScreenerQuery` 11 键 / `[screener]` 加 `filter` JSON key / 独立 JSON 文件 | `[screener]` 节 `filter = "<Filter JSON>"`，加载双解析（新格式优先、legacy 11 键回退、坏 JSON 回退默认） | AST 是 config 与 LLM 的统一格式（Batch 1 决策）；旧配置可读（验收要求）、首次保存后迁移；`serde_json` 为 workspace 既有依赖 | 独立文件破坏 config.toml 单文件约定；继续存 11 键无法表达 Or/Not/UpDays |
| B5（#246）：性能验证 | 仅凭复杂度论证 / criterion bench 对比迁移前后 | criterion bench（6000 标的 × 400 bar 合成数据）对比 a1dbcad 基线 | 验收要求"全市场筛选性能不劣于现状"需客观证据；实测两档（空 Filter + legacy 可表达混合 Filter）中位数同量级（差异 <3%） | 复杂度论证无数据支撑；代表性 Filter 含 UpDays 会在旧路径直接 Err（bench 对比无效） |
| B6（#246）：MarketCap 缺失 total_share | 无条件剔除 / 按 `min/max` 激活门控 | `min`/`max` 任一 Some 才剔除；均 None → 按 0.0 通过（排序垫底） | 复刻 `screen_symbol`（strategy lib.rs:435-444）；GUI 默认 6 卡片恒含 `MarketCap{None,None}`，无条件剔除会静默丢弃缺失 share 的股票 | 无条件剔除破坏 GUI 默认状态行为 |
| B7（#246）：`NDayHigh` 语义 | 含最新 N 根 / 前 N 根（不含最新） | 前 N 根（不含最新）最大值，需 N+1 根 | 复刻 `matches_breakout`（strategy lib.rs:527-535）——`Close > NDayHigh(days)` 与 breakout 一致；Count 内逐日求值同一定义（`series[i-n..i]`） | 含最新会让 breakout 恒 false（Close 必 ≤ 含自身的 max）；双定义会漂移 |

> 注：M4（#244）的"受限私有反向转换"执行路径已被 B1（#246）取代——通用求值器
> 落地后 `filter_to_query`/`ScreenerError::UnsupportedFilter` 全部删除，M4 仅为
> Batch 1 阶段决策的历史记录。

| D1（#247）：LLM 客户端 crate 归属 | compass-core / compass（GUI）/ compass-strategy | compass-core `llm` 模块（`LlmClient::chat_json`） | 跨 GUI 与未来 #153（行业新闻分析）复用；reqwest/serde_json/httpmock 依赖已就位；无 SDK 依赖 | GUI 内建则 CLI/其他消费者无法复用；strategy 与网络 I/O 无关 |
| D2（#247）：语义校验函数归属 | compass-types `validate_filter` / compass 内私有 | compass-types 纯函数 | AST 同域类型（GUI/后端/测试三方共用）；serde 反序列化后校验自然衔接 | compass 内私有则测试与复用受限 |
| D3（#247）：prompt 构建/响应解析归属 | compass `llm_screener` / compass-core | compass（业务层） | prompt 依赖 Filter AST schema（compass-types）+ 业务语义（单位/示例），属应用层；compass-core 保持通用客户端职责 | compass-core 混入业务 prompt 破坏"通用客户端"复用定位 |
| D4（#247）：LLM 请求通道 | 第五 `AsyncDispatcher` 通道 / 复用 run_screener 通道 | 第五通道（`RunLlmRequest/Response`，含 seq 守卫） | 与 sepa/index 通道模式完全同构；LLM 是独立后端职责（网络 I/O + 解析校验）；seq 守卫保证 Esc 取消后在途响应不混入 | 复用 screener 通道破坏单一职责、错误语义混杂 |
| D5（#247）：API key 存储 | config.toml 明文 / 系统钥匙串 / GUI 输入框 | `[llm]` 节明文（与项目其他配置同级） | 桌面本地应用、配置即文本的既有惯例；无密钥管理依赖 | 钥匙串引入平台差异与额外依赖，超出辅助功能定位 |
| C4（#267）：抓取进度存储形态 | JSON 进度文件 / SQLite / 日志行 | `csv_dir()/<name>.progress.json` 原子写（tmp+os.replace） | 轻量零依赖、跨进程可读、与 CSV 同目录便于排查；CSV 保持一次性写入语义 | SQLite 过重；日志行无结构化查询 |
| C5（#267）：progress target 范围 | 11 个全量名 / 仅 6 个接入者 | 仅 6 个接入者（main_flow/block_trade/index_daily/institution_survey/concept_member/dragon） | 未接入 target 查询必失败，choices 收敛到真实有效值（append 型采集器无进度文件） | 全量 choices 误导用户 |

> 注：设计文件 `.dsh/designs/llm-screener-llm.md` §4 的"拒绝空 And/Or、深度 > 8"
> 与实现契约（`validate_filter` 空 And/Or 合法、深度上限 32）不一致——以后者为准：
> 空 And/Or 是构建器空状态的合法 AST（LLM 返回空 And 时合并为无操作，不报错），
> 深度 32 在 serde recursion limit（128）内有防栈溢出余量；构建器模板外形状
> （如 `Count`、单边 `Cmp`）由 `llm_screener::ensure_builder_roundtrip` 在解析层
> 拒绝并提示换一种描述——避免 Unknown 只读卡在运行/持久化时被静默丢弃（ref #247）。

符号约定（Dolt-native 前缀格式 vs ts_code）的决策记录见 `.dsh/kb/design/symbols.md`。
