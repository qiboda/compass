# 图表应用（GUI）

## 启动

```sh
cargo run
```

应用打开一个 1280×720 的暗色主题窗口，标题为 "Compass — Stock Chart"。

## 界面

### 工具栏

顶部工具栏将所有控件排列在一行中：

| 控件 | 图标 | 用途 |
|---|---|---|---|
| **Symbol** | 🔍 | 可搜索输入框 — 输入代码前缀（如 `600`）或名称子串（如 `平安`）过滤股票列表，弹窗列出匹配项。显示格式为 `交易所 \| 代码 \| 名称`。 |
| **TF** | ⏱ | 组合框 — 选择 `1d`（日线）、`1w`（周线）或 `1M`（月线）。控制 OHLCV 柱的聚合。 |
| **Fetch** | ⬇ | 按钮 — 从本地数据库加载所选股票的图表数据。 |
| **Theme** | 🎨 | 组合框 — 在 `compass_dark` 和 `compass_light` 预设之间切换。全局应用于所有 UI 元素。 |

### 状态提示

状态消息以**吐司通知**（toast notifications）形式显示在窗口右上角。

| 类型 | 图标 | 含义 | 自动消失 |
|---|---|---|---|
| **Loading** | ⏳ | 正在从本地数据库加载数据 | —（保持到加载完成） |
| **Success** | ✅ | 操作完成（获取、导入、导出） | 3 秒 |
| **Warning** | ⚠ | 非关键问题（如数据过期） | 5 秒 |
| **Error** | ❌ | 发生错误（网络、无数据、无效代码） | 8 秒 |

吐司使用 Phosphor 图标字形。消息垂直堆叠；新消息出现时旧消息淡出。

### 图表区域

图表显示 K 线柱，支持：

- **平移**：点击并水平拖拽
- **缩放**：鼠标滚轮
- **十字准线**：悬停在 K 线上查看 OHLCV 详情
- **可见柱数**：默认显示 100 根 K 线

### 日志

可滚动的日志面板显示获取状态、错误和 citizen 生命周期事件。

### 主题切换

提供两种内置视觉主题：

| 预设 | 描述 |
|---|---|
| `compass_dark` | 默认暗色主题（TradingView 风格） |
| `compass_light` | 亮色主题，适合白天使用 |

点击工具栏中的 **🎨 (PALETTE)** 按钮，打开下拉菜单选择主题。更改即时应用于所有 UI 元素 — 图表背景、工具栏、面板、按钮和文字颜色。无需重启。

当前主题持久化到 `~/.config/compass/config.toml` 的 `[app].theme` 下，下次启动时恢复。

### 吐司通知

状态反馈以临时吐司通知形式显示，锚定在窗口**右上角**。每条吐司显示一个 Phosphor 图标字形、简短消息，并在预设时长后自动消失。

| 类型 | 图标 | 消失时间 | 示例 |
|---|---|---|---|
| Success | ✅ | 3 秒 | "Data loaded: sh.600519 (100 bars)" |
| Warning | ⚠ | 3 秒 | "No data available for this date range" |
| Error | ❌ | 8 秒 | "Network error: connection timeout" |
| Info | ℹ | 3 秒 | "Import complete: 2,430 records" |

吐司垂直堆叠；最多同时显示 5 条。较早的通知上滑并淡出，为新通知腾出空间。点击吐司可立即关闭。

### 模态对话框与文件对话框（预留组件）

`Modal` 与 `egui-file-dialog` 已在代码中接线（组件存在、随每帧渲染），但**当前未绑定任何操作**——
没有代码调用它们打开对话框。破坏性操作确认、导入/导出文件选择等能力为预留功能，尚未启用。

## 数据流程

点击 "Fetch" 时：

1. 所选交易所为股票代码添加前缀（如 `sh.600519`）
2. **查询本地 Parquet** — `DuckDbProvider` 通过 DuckDB 的 `read_parquet()` 直接读取 `stock_daily.parquet`
3. **显示图表** — 柱状数据以 K 线形态呈现

GUI 是**仅限本地**的：它从不调用东方财富或任何在线 API。所有数据来自本地 Parquet 主数据库。如果某只股票不在数据库中，GUI 显示 "no data" 消息 — 需先用数据管线（`compass-data import` 或 Python 采集器）导入数据。

## 股票代码

从下拉框中选择或输入搜索：

| 代码 | 股票 | 交易所 |
|---|---|---|
| `000001` | 平安银行 | SZ |
| `600519` | 贵州茅台 | SH |
| `688001` | 华兴源创 | SH |
| `300750` | 宁德时代 | SZ |
| `830799` | 艾融软件 | BJ |

交易所下拉框过滤股票列表。当选择某个交易所（SH/SZ/BJ）时，股票代码会自动添加前缀（如 `sh.600519`）后再获取数据。选择 "全部" 时不添加前缀。

## 配置默认值

创建 `~/.config/compass/config.toml` 来设置启动偏好：

```toml
theme = "compass_light"

[app]
default_symbol = "600519"
default_timeframe = "1d"
```

全部选项见 [配置](config.md)。

## 数据前置条件

图表应用通过 DuckDB 的 `read_parquet()` 直接从 `parquet_data/stock_daily.parquet` 读取 OHLCV 数据（内存模式，无需持久化 DuckDB 文件）。首次使用前，请确保数据已就绪：

```sh
# 从 Dolt 导入（完整历史）
cargo run --bin compass-data -- import
# 数据已就绪 — parquet_data/stock_daily.parquet 是数据源
```

如果某只股票不在本地数据库中，GUI 显示 "no data" 消息 — 无在线回退。请先导入缺失的数据。

要使股票下拉框有数据，`stock_basic.parquet` 必须存在于 parquet 数据目录中（默认：`parquet_data/`）。该文件由 `compass-data import`（或 `import-compass --table stock_basic`）创建。
