# GUI 全量 i18n 设计（issue #222）

> 设计方：ui-designer。本文件为**过程归档**；最终权威版将同步至
> `kb/design/ui.md`（含本方案经用户确认后的决策要点）。
> 机制契约（grill-me 已锁定）：rust-i18n v4.2.1 + 新 crate `compass-i18n`
> （单一 `locales/` 目录，`init!()` 宏 + re-export `t!`/`set_locale`）；
> egui-charts fork 自带独立 i18n；compass-strategy 返回语义 key。

## 目标

1. GUI 全部用户可见文本走 `t!()` 键查找，zh/en 双语言，键架构允许未来加语言
   （加一个 locale 文件即扩展）。
2. 语言切换**即时生效**（含 fork 图表侧），并持久化到 config.toml。
3. 英文态下布局不破损：全部高风险面板逐项给出缓解策略。
4. 测试断言从字面中文改为**键解析**，保证 zh 默认 + en 双语言测试稳定。

## 现状

| 位置 | 字符串 | 现状问题 |
|---|---|---|
| `crates/compass/src/main.rs` | 窗口标题 "Compass — Stock Chart"、启动 Modal（数据未就绪/知道了/取消）、自选 toast（已添加 {symbol} 到自选/已从自选移除）、移除 Modal（移除自选/确定要从自选中移除…/移除/保留）、日志导出 toast、状态栏（加载中…/本地数据源 · {} 只）、Fetch 按钮（加载中…/Fetch **混排**）、tooltip 切换侧边栏、主题 toast、**"Data fetched successfully"（英文不一致）**、自选组标题、前复权 Tag | ~35 处，中英混排、不一致 |
| `crates/compass/src/tabs.rs` | dock 标签 图表/日志/选股器/东方SEPA（`TabKind::title() -> &'static str`） | 返回 &'static str，需改键 |
| `crates/compass/src/citizens/sepa.rs` | 12 列头（`COLUMNS: [ColumnSpec; 12]`，`header: &'static str`）、市场温度、共 {shown} 行 · {date} 评分、计算中…/刷新、SEPA 评分计算中…（全市场）、暂无 SEPA 评分数据/点击刷新计算全市场 TOP50 评分、点击排名行查看评分详情、总分 {:.1}、五模块标签 | **`ColumnSpec.header: &'static str` 是硬约束**（`data_table.rs:65`）——键无法静态化，必须改 API |
| `crates/compass/src/citizens/screener.rs` | 6 列头、筛选/筛选进行中…、基础条件/技术面条件、行业/交易所/板块/上市时长/市值(亿)/排除退市、不限/≥1年/≥3年/≥5年、均线、MaKind 标签（`label() -> &'static str`）、突破新高/动量/量能、N:/min%:/max%:/倍数: | MaKind 需改键；表单标签英文态变宽 |
| `crates/compass/src/citizens/chart.rs` | 暂无图表数据/输入代码并点击 Fetch | — |
| `crates/compass/src/citizens/logger.rs` | 日志、导出日志 tooltip | — |
| `crates/compass/src/backend.rs` | ~10 条英文错误（failed to open duckdb/parquet、no data for、failed to run screener/sepa） | 后台线程产生，`set_locale` 进程级可跨线程 |
| `crates/compass-ui/src/widgets/sidebar.rs` | 搜索自选、添加/删除 tooltip、自选股为空、点击 + 添加关注的股票 | 组件内自带字符串 |
| `crates/compass-ui/src/widgets/searchable_dropdown.rs` / `dropdown.rs` | 无匹配结果、搜索…、`format_display`（`{exchange} | {symbol} | {name}` 分隔符模板） | 分隔符 `|` 可保留 |
| `crates/compass-ui/src/widgets/modal.rs` | 默认 Confirm/Cancel（英文，真实场景全部覆盖） | 默认值改 `t!()` |
| `crates/compass-ui/src/widgets/multi_select.rs` | 全部 | — |
| `crates/compass-ui/src/widgets/data_table.rs` | 无符合条件、共 {count} 行 | — |
| egui-charts fork `src/chart/renderers/tooltip.rs` / `crosshair.rs` / `scales/time_formatter.rs` | 时间:/开盘:/最高:/最低:/收盘:/成交量:/涨跌:；日期 `%Y年%-m月%-d日`/`%-m月%-d日 %H:%M:%S`/`%-m月%-d日 %H:%M`/`%-m月`/`%-m月%-d日` | fork 自带独立 locales/ |
| `crates/compass-strategy/src/sepa/temperature.rs` / `scoring.rs` | 沪深300趋势/中证1000趋势/涨停数/成交额/赚钱效应、{n} 家、{:.2}万亿、position 80%-100%/40%-70%/0%-20%；SepaFactor 标签（均线结构/价格位置/相对强度/板块涨幅/板块成交额/板块扩散/新闻热度/量价配合/筹码集中/大资金流入/VCP质量/突破确认/波动惩罚(ATR)/深度回撤/放量滞涨）+ notes（距一年高点回撤 {:.1}%/动量分位 {:.0}%/无板块数据/v1 无新闻数据/v1 默认 10/20） | 纯逻辑 crate，返回 key |

**关键约束**：`ColumnSpec.header: &'static str`（`data_table.rs:65`）与
`TabKind::title() -> &'static str`（`tabs.rs:60`）、`MaKind::label() -> &'static str`
（`screener.rs:51`）——`t!()` 返回 `String`（rust-i18n v4 `__t` 返回运行时查找的
str 再 to_string），**无法赋给 `&'static str` 字段**。方案：这些 API 的字段/返回值
改为「持有键的 `&'static str`」，渲染时 `t!()` 解析（详见 §键树 + §布局）。

---

## 设计方案

### 1. 键命名空间（完整键树 —— 契约）

规则：
- 键 = 点分小写 snake_case；域前缀：`app` / `tab` / `toolbar` / `sidebar` /
  `statusbar` / `common` / `chart` / `logger` / `screener` / `sepa` / `widgets` /
  `toast` / `error` / `modal`；fork 独立域 `chart.*`（fork 自带 locales，不与
  compass 冲突）。
- 变量占位符：`%{name}`（rust-i18n v4 语法）。
- **数据不翻译**：股票名（贵州茅台）、行业/题材名（白酒、茅指数）、代码、日期值、
  数值——它们是数据，仅模板/标签走键。
- `t!()` 返回值赋给 `&'static str` 字段/函数返回值的场景 → 该 API 改为返回
  **键名**，渲染处调 `t!(key)`（locale 切换即时生效，且避免存储陈旧译文）。

#### zh.yml / en.yml 全量键

```yaml
# ── 应用框架 ─────────────────────────────────────────────
app:
  title: "Compass — Stock Chart"        # en: 同（品牌名；见待确认 Q1）

tab:
  chart: 图表            # en: Chart
  logger: 日志            # en: Log
  screener: 选股器        # en: Screener
  sepa: 东方SEPA          # en: East SEPA

toolbar:
  fetch: 获取数据          # en: Fetch        （现状 zh 显示英文 Fetch，见待确认 Q2）
  loading: 加载中…        # en: Loading...
  adjust: 前复权          # en: Adj.          （短形式，见 §布局）
  toggle_sidebar: 切换侧边栏  # en: Toggle sidebar
  language: 中文          # en: English       （语言名用母语呈现，无需翻译键）

sidebar:
  group_watchlist: 自选   # en: Watchlist
  search_placeholder: 搜索自选  # en: Search watchlist
  add_tooltip: 添加       # en: Add
  delete_tooltip: 删除    # en: Remove
  empty_title: 自选股为空  # en: Watchlist is empty
  empty_desc: 点击 + 添加关注的股票  # en: Click + to add stocks

statusbar:
  loading: 加载中…        # en: Loading...
  source: 本地数据源 · %{count} 只  # en: Local data · %{count} symbols

common:
  loading: 加载中…        # en: Loading...
  refresh: 刷新           # en: Refresh
  confirm: 确认           # en: Confirm
  cancel: 取消            # en: Cancel
  remove: 移除            # en: Remove
  search: 搜索…           # en: Search…
  no_matches: 无匹配结果   # en: No matches
  all: 全部               # en: All

chart:
  empty_title: 暂无图表数据  # en: No chart data
  empty_desc: 输入代码并点击 Fetch  # en: Enter a code and click Fetch

logger:
  title: 日志             # en: Log
  export_tooltip: 导出日志 # en: Export log

modal:
  startup:
    title: 数据未就绪      # en: Data not ready
    body: |
      未在本地数据目录中找到股票列表（stock_basic.parquet）。
      请先用数据管线导入数据：cargo run --bin compass-data -- import-compass --table stock_basic
      # en: No stock list found in the local data directory (stock_basic.parquet).
      #     Import data first: cargo run --bin compass-data -- import-compass --table stock_basic
      #     （命令行本身不翻译）
    confirm: 知道了        # en: Got it
  remove:
    title: 移除自选         # en: Remove from watchlist
    body: 确定要从自选中移除 %{symbol} 吗？  # en: Remove %{symbol} from watchlist?
    confirm: 移除           # en: Remove
    cancel: 保留            # en: Keep

toast:
  theme_switched: 主题已切换  # en: Theme switched
  language_switched: 语言已切换  # en: Language switched
  fetch_success: 数据获取成功  # en: Data fetched successfully   （修复现状英文不一致）
  watchlist_added: 已添加 %{symbol} 到自选  # en: Added %{symbol} to watchlist
  watchlist_removed: 已从自选移除 %{symbol}  # en: Removed %{symbol} from watchlist
  log_exported: 日志已导出: %{path}  # en: Logs exported: %{path}
  log_export_failed: 日志导出失败: %{error}  # en: Log export failed: %{error}
  sepa_updated: SEPA 评分已更新 · %{count} 只  # en: SEPA scores updated · %{count}

error:
  duckdb_open: 打开 DuckDB 失败: %{e}   # en: Failed to open DuckDB: %{e}
  parquet_open: 打开 Parquet 失败: %{e}  # en: Failed to open Parquet: %{e}
  no_data: 没有 %{symbol} 的数据          # en: No data for %{symbol}
  screener_run: 选股运行失败: %{e}        # en: Screener run failed: %{e}
  sepa_run: SEPA 计算失败: %{e}           # en: SEPA run failed: %{e}
  # 说明：%{e} 为底层 DataError Display（英文技术细节），不做翻译

# ── 选股器 ───────────────────────────────────────────────
screener:
  filter: 筛选             # en: Filter
  filtering: 筛选进行中…    # en: Filtering…
  card_basic: 基础条件      # en: Basic
  card_technical: 技术面条件 # en: Technical
  industry: 行业            # en: Industry
  exchange: 交易所          # en: Exchange
  board: 板块               # en: Board
  list_years: 上市时长      # en: Listed ≥
  any: 不限                # en: Any
  years_1: ≥1年            # en: ≥1y
  years_3: ≥3年            # en: ≥3y
  years_5: ≥5年            # en: ≥5y
  market_cap: 市值(亿)      # en: Mkt Cap(Bn)
  exclude_delisted: 排除退市  # en: Excl. delisted
  ma: 均线                  # en: MA
  ma_above20: 站上 MA20     # en: Above MA20
  ma_above60: 站上 MA60     # en: Above MA60
  ma_bullish: 多头排列 MA5>MA20>MA60  # en: Bullish MA5>20>60
  breakout: 突破新高         # en: New High
  momentum: 动量            # en: Momentum
  volume: 量能              # en: Volume
  n_label: N:               # en: N:          （locale 中性，仍走键统一）
  min_pct: min%:            # en: min%:
  max_pct: max%:            # en: max%:
  times: 倍数:              # en: ×:
  table:
    code: 代码              # en: Code
    name: 名称              # en: Name
    latest: 最新价           # en: Price
    change_20d: 20日涨跌幅    # en: 20D Chg%
    market_cap: 市值(亿)     # en: Mkt Cap(Bn)
    industry: 行业           # en: Industry

# ── SEPA 面板 ────────────────────────────────────────────
sepa:
  thermometer: 市场温度      # en: Market Temp
  count: 共 %{shown} 行 · %{date} 评分  # en: %{shown} rows · scored %{date}
  no_data: 暂无评分数据       # en: No score data yet
  computing: 计算中…         # en: Computing…
  computing_full: SEPA 评分计算中…（全市场）  # en: Computing SEPA scores (full market)…
  refresh: 刷新             # en: Refresh
  empty_title: 暂无 SEPA 评分数据  # en: No SEPA score data
  empty_desc: 点击刷新计算全市场 TOP50 评分  # en: Click refresh to score the full-market TOP50
  detail_hint: 点击排名行查看评分详情  # en: Click a row to view score details
  total_score: 总分 %{score}  # en: Total %{score}
  table:
    rank: 排名              # en: Rank
    code: 代码              # en: Code
    name: 名称              # en: Name
    total: 总分             # en: Score
    trend: 趋势             # en: Trend
    theme: 题材             # en: Theme
    capital: 资金           # en: Capital
    pattern: 形态           # en: Pattern
    risk: 风险              # en: Risk
    industry: 行业          # en: Industry
    latest: 最新价          # en: Price
    change: 涨跌幅          # en: Chg%
  module:
    trend: 趋势             # en: Trend
    theme: 题材             # en: Theme
    capital: 资金           # en: Capital
    pattern: 形态           # en: Pattern
    risk: 风险              # en: Risk
  position:
    full: 80%-100%         # en: 80%-100%     （locale 中性，键化统一）
    mid: 40%-70%           # en: 40%-70%
    low: 0%-20%            # en: 0%-20%
  unit:
    percent: %{v}%          # en: %{v}%
    count: %{v} 家          # en: %{v}
    trillion: %{v}万亿       # en: %{v}T
  indicator:
    hs300_trend: 沪深300趋势   # en: HS300 Trend
    zz1000_trend: 中证1000趋势  # en: CSI1000 Trend
    limit_up: 涨停数          # en: Limit-ups
    amount: 成交额            # en: Turnover
    breadth: 赚钱效应         # en: Breadth
  factor:
    ma_structure: 均线结构        # en: MA structure
    price_position: 价格位置      # en: Price position
    relative_strength: 相对强度    # en: Rel. strength
    sector_gain: 板块涨幅          # en: Sector gain
    sector_amount: 板块成交额      # en: Sector turnover
    sector_diffusion: 板块扩散     # en: Sector breadth
    news_heat: 新闻热度            # en: News heat
    volume_price: 量价配合         # en: Vol-price fit
    chip_concentration: 筹码集中   # en: Chip concentration
    big_capital_inflow: 大资金流入  # en: Big-cap inflow
    vcp_quality: VCP质量           # en: VCP quality
    breakout_confirm: 突破确认      # en: Breakout confirm
    vol_penalty: 波动惩罚(ATR)      # en: Vol penalty (ATR)
    deep_drawdown: 深度回撤         # en: Deep drawdown
    volume_stagnation: 放量滞涨     # en: Vol up, price stall
  note:
    drawdown: 距一年高点回撤 %{pct}%  # en: Drawdown %{pct}% from 1y high
    momentum_percentile: 动量分位 %{pct}%  # en: Momentum percentile %{pct}%
    no_sector_data: 无板块数据  # en: No sector data
    news_v1: v1 无新闻数据      # en: v1: no news data
    news_default: v1 默认 10/20  # en: v1: default 10/20

# ── 组件内部（compass-ui）────────────────────────────────
widgets:
  searchable_dropdown:
    no_matches: 无匹配结果      # en: No matches   （复用 common.no_matches 亦可）
  data_table:
    count: 共 %{count} 行       # en: %{count} rows
    empty: 无符合条件            # en: No matching rows

# ── egui-charts fork（fork 自带 locales/，与上互不冲突）────
chart:
  tooltip:
    time: 时间:                # en: Time:
    open: 开盘:                # en: Open:
    high: 最高:                # en: High:
    low: 最低:                 # en: Low:
    close: 收盘:               # en: Close:
    volume: 成交量:            # en: Volume:
    change: 涨跌:              # en: Change:
  date:
    full: "%Y年%-m月%-d日"       # en: "%Y-%m-%d"
    full_time: "%-m月%-d日 %H:%M:%S"  # en: "%m-%d %H:%M:%S"
    crosshair: "%-m月%-d日 %H:%M"     # en: "%m-%d %H:%M"
    axis_month: "%-m月"              # en: "%b"
    axis_day: "%-m月%-d日"           # en: "%b %-d"
  realtime: 实时                # en: Realtime
  legend:
    open: O                    # en: O          （单字母，locale 中性，键化统一）
    high: H                    # en: H
    low: L                     # en: L
    close: C                   # en: C
    volume: V                  # en: V
```

#### compass-strategy 语义键契约（纯逻辑侧返回键，不调 t!()）

```rust
// compass-types 模型调整（变更建议，供 plan 细化）
SepaIndicator {
    label_key: &'static str,   // 如 "sepa.indicator.hs300_trend"
    value: f64,                // 原始数值（62.0 / 45.0 / 1.23）
    unit_key: &'static str,    // "sepa.unit.percent" | "sepa.unit.count" | "sepa.unit.trillion"
    delta_pct: Option<f64>,
    heat: f32,
}
MarketThermometer {
    score: f64,
    position_key: &'static str,  // "sepa.position.full" | "mid" | "low"
    position_pct: f64,
    indicators: Vec<SepaIndicator>,
}
SepaFactor {
    label_key: &'static str,   // "sepa.factor.ma_structure" 等
    score: f64,
    max: f64,
    note_key: Option<&'static str>,   // 如 "sepa.note.drawdown"
    note_args: ...            // 键对应的参数（%{pct}），由 UI 层 t!() 组装
}
```

UI 侧渲染（`sepa.rs` 现 `ind.label`/`ind.value_text`/`t.position` 的消费点）改为：
`t!(ind.label_key)` + `t!(ind.unit_key, v = 格式化后的值)` + `t!(t.position_key)`。
> ⚠️ 锁定决策只提了 SepaIndicator.label/value_text/position 三项；
> **SepaFactor.label/note 在 scoring.rs 也是中文**，detail 面板直接渲染——为达成
> 全量 i18n 必须一并键化（待确认 Q3）。

### 2. 语言切换 UI（配置 + 应用内下拉，双通道）

**推荐：应用内下拉即时切换 + 持久化 config.toml**（restart 也生效）。

- **入口**：工具栏 Group D（显示组）主题 Dropdown 右侧加一个语言 Dropdown
  （宽度 ~76px），选项 = 母语名 `中文` / `English`（语言名惯例不翻译）。
  与主题 Dropdown 并列，发现性一致（主题即「下拉 + Info toast」先例）。
- **切换动作**：`set_locale("en"|"zh")` → `ui.ctx().request_repaint()` 立即重绘 →
  `ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(t!("app.title")))`
  更新窗口标题（egui 官方示例同款做法）→ Info toast `t!("toast.language_switched")`
  → 写回 config.toml 顶层 `language` 键（复用 `save_watchlist_config` /
  `save_screener_config` 的 toml::Value 读改写模式）。
- **启动**：`load_config()` 读 `language`，非法值回退 "zh" + warn（镜像 theme 处理）；
  `main()` 在构造任何 UI 前 `set_locale(&language)`——`set_locale` 进程级，
  **fork 图表侧同日生效**（锁定决策）。
- **即时性说明**：所有渲染路径每帧 `t!()` 解析 → 切语言下一帧全部更新；
  已物化字符串（已推送的 toast、SharedState 里存下的错误）保持旧语言直至消失/
  下次刷新——可接受（瞬态信息）。`ColumnSpec` 等持键 API 因渲染时解析，天然即时。
- **配置节**：顶层 `language = "zh"`（镜像 `theme`），文档同步
  `kb/user/config.md` + 默认值表。

### 3. CJK / 英文态布局风险评估与缓解

| 风险面板 | 现状（zh） | en 最宽串 | 风险 | 缓解 |
|---|---|---|---|---|
| 工具栏 Fetch | 加载中…/Fetch（混排） | Loading... | 低 | Button 自适宽；zh 键改「获取数据」后与 en 长度接近 |
| 前复权 Tag | 前复权 | Adj. | 低 | en 用短形式 `Adj.`（Tag 自适宽） |
| 工具栏 Group D | 主题下拉 140px | +语言下拉 76px | 低 | 固定宽度下拉，总宽仍 < 1440 首屏；窄窗下工具栏不换行（40px 定高，超宽截断靠现有布局） |
| dock 标签 | 图表/选股器/东方SEPA | Screener / East SEPA | 低 | Tab 栏自适宽；4 标签 en ≈ 250px，任何宽度余量充足 |
| SEPA 12 列表头 | 排名/最新价/涨跌幅 | Industry / Capital / Pattern | **中** | ① `ColumnSpec.header` 改持键、渲染 `t!()`（消除 `&'static str` 约束）；② en 用短形式（Price/Chg%/Score）；③ 表头右对齐 numeric 布局不变；④ 已有横向 ScrollArea 吸收超宽（ref #217） |
| screener 表单标签 | 上市时长/市值(亿) | Listed ≥ / Mkt Cap(Bn) | **中** | ① `MaKind::label()` 改键；② en 用短标签（Above MA20 / Bullish MA5>20>60 / New High）；③ #220 原子组 wrap 检查基于 `label_w + 176` / 固定组宽（158/274/286/390）——en 标签更长导致**更早换行**：可接受（组内原子性保持），但需复跑 5 档宽度对齐测试确认 158 组放得下 "New High" + N: + DragValue；必要时把常量上调 16-24px |
| Sidebar | 搜索自选/点击 + 添加关注的股票 | Search watchlist / Click + to add stocks | 低 | 240px 内放得下；placeholder 走 `t!()`；空态描述换行可 |
| 状态栏 | 本地数据源 · 5324 只 | Local data · 5324 symbols | 低 | 右段 source+clock 并排，en 约 150px+50px 余量充足；极端窄窗 source 短形式备选（`Local · %{count}`） |
| Modal 文本 | 启动引导长 body | 含命令行（不翻译） | 低 | body 换行；命令行作为两语言共享字面量 |
| fork tooltip/十字光标 | 时间: 2024年5月15日 | Time: 2024-05-15 | 低 | tooltip 自适宽；日期格式键化后 en 更短 |
| fork x 轴刻度 | 1月 / 5月15日 | Jan / May 15 | 低 | `%b`/`%b %-d` 宽度相近；刻度间距逻辑不变 |
| 字体 | SourceHanSansCN + JetBrains Mono 内嵌 | 同（思源含拉丁字形） | 无 | 无新字体依赖；en 渲染用思源拉丁字形（锁定设计系统，不加拉丁字体） |

**结论**：无面板在 en 下破损；两个「中」风险（SEPA 表头、screener 表单）的
缓解点已内置——`ColumnSpec`/`MaKind` API 键化（必须）+ en 短标签 + 现有
wrap/ScrollArea 机制吸收。需在实现阶段对 screener 5 档宽度测试与 SEPA
1400px 详情面板测试**追加 en locale 跑一遍**（见 §实现分阶段）。

### 4. 交互效果

| 触发 | 行为 | 时长/缓动 | 目标态 |
|---|---|---|---|
| 语言下拉选择 English/中文 | `set_locale` + 立即重绘 + 窗口标题 `ViewportCommand::Title` + Info toast「语言已切换 / Language switched」+ config 写回 | 即时（无动画；下一帧全界面刷新） | 全界面（含 fork 图表 tooltip/日期/实时按钮）切换语言 |
| 启动（config 无 language / 非法值） | 回退 "zh" + `tracing::warn!` | — | 中文界面 |
| 已物化 toast/错误 | 保持原语言直至消失 | — | 不强制重译（瞬态） |
| 主题与语言双 Dropdown 交互 | 两者独立、同组并列，互不干扰 | — | — |

快捷键：语言切换不占全局快捷键（低频操作，下拉即可）；`Esc` 关闭下拉由
Dropdown 组件现有行为承担。

### 5. 实现分阶段建议

**Phase 0 — 基建**：新建 `crates/compass-i18n`（locales/zh.yml+en.yml、
`init!()` 宏、re-export `t`/`set_locale`）；`compass-ui` 与 `compass` 加依赖
（share-in-workspace）；`AppConfig` 加 `language` 顶层键 + 回退逻辑 +
`main()` 启动 `set_locale`。**本 phase 同时引入测试基建**：kittest 断言改为
键解析辅助函数（如 `tr("sepa.empty_title")`），一次迁移全部现存字面量断言。

**Phase 1 — 应用框架 + 公共键**（main.rs / tabs.rs / logger / chart / sidebar /
statusbar / modal 默认 / multi_select / data_table / dropdown / backend 错误模板）：
先覆盖窗口标题、Modal、toast、状态栏、工具条、组件内部串。此阶段后 zh 界面
外观零变化（键值 = 现状文案），en 仅剩 citizen 面板与 fork 未键化。
`ColumnSpec.header`、`TabKind::title()`、`MaKind::label()` 在本 phase 完成
**键化 API 改造**（持键 + 渲染解析）。

**Phase 2 — SEPA + screener citizen**：12 列表头、市场温度、计数/空态/详情、
表单标签、MaKind 选项、筛选按钮。SEPA 详情面板 `t!(factor.label_key)` +
`t!(note_key, ...)` 组装。

**Phase 3 — strategy 语义键**：compass-types 模型调整（SepaIndicator/MarketThermometer/
SepaFactor 加 key 字段）、temperature.rs/scoring.rs 返回键、sepa.rs 消费点改
`t!()`。纯逻辑 crate 仍零 UI 依赖（键只是字符串常量）。

**Phase 4 — egui-charts fork**：fork 仓库加 rust-i18n 依赖 + 自带 `locales/`
（zh.yml 值 = 现状中文格式串），tooltip/crosshair/time_formatter/labels/realtime
改 `t!()`；compass 侧 bump fork rev。**注意**：fork 是独立仓库/分支，改动经
fork PR 合入后 compass 升引用。

**Phase 5 — 语言切换 UI + en 布局验收**：工具栏语言下拉 + `set_locale` +
`ViewportCommand::Title` + config 写回；en 全界面布局 sweep（screener 5 档宽度
对齐测试、SEPA 1400px 详情面板测试**追加 en 运行**；必要时上调 technical_group
宽度常量）。

**kittest 断言迁移策略**：
- 现状字面量断言（如 `get_by_label("暂无 SEPA 评分数据")`、`assert_eq!(TabKind::Chart.title(), "图表")`）
  → 统一改为经 `t!()` 解析：`get_by_label(t!("sepa.empty_title"))`、`assert_eq!(t!("tab.chart"), ...)`。
- 默认测试 locale = zh（与产品默认一致），绝大多数测试零行为变化。
- en 专项测试（布局 sweep）必须**串行**：`set_locale` 是进程全局，与现有
  `HOME_LOCK`（main.rs tests）同模式加 `LANG_LOCK: Mutex` 保护，避免并行测试
  互相污染 locale。

---

## 待确认

1. **窗口标题**：`app.title` zh 是否保持 "Compass — Stock Chart"（品牌名）而非
   翻译成「Compass — 股票图表」？（推荐：保持品牌英文，Q 低）
2. **Fetch 按钮 zh**：现状 zh 界面显示英文 "Fetch"，键化后 zh 改「获取数据」会
   改变现有界面外观——还是 zh 保留 "Fetch"？（推荐：改为「获取数据」，消除混排）
3. **SepaFactor 键化范围**：锁定决策只列了 SepaIndicator.label/value_text/position，
   但 SepaFactor.label/note 同样出现在 detail 面板（scoring.rs 产中文）——是否一并
   键化？（推荐：一并键化，否则 en 下 detail 面板仍显中文，违背全量 i18n）
4. **语言下拉位置**：工具栏 Group D（推荐）vs 状态栏右段 vs 设置菜单（无设置页）。
5. **fork 日期 en 格式**：`%Y-%m-%d` / `%m-%d %H:%M:%S` / `%b` / `%b %-d` 是否可接受
   （推荐：是；TradingView 风格短格式）。
6. **错误细节 %{e}**：底层 DataError Display 保持英文不翻译（推荐：是，技术细节）。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 键命名空间 | 模块前缀点分 / 扁平键 / 数字 id | 模块前缀点分（app./tab./sepa./widgets./chart.* 等） | 与现有领域划分一一对应（citizens 即域）；IDE/文档可读；未来加语言零重构 | 扁平键难分组易冲突；数字 id 不可读 |
| `&'static str` 串 API（ColumnSpec.header / TabKind::title / MaKind::label） | 运行时 `t!()` 返回 String 存字段 / **持键存 `&'static str` 渲染时解析** / 每次构建 Vec | 持键 + 渲染解析 | `t!()` 返回 String 赋不进 `&'static str`；持键让 locale 切换即时生效且零陈旧译文；DataTable/TabViewer 渲染路径本就每帧执行 | 存 String 译文在语言切换后陈旧（需重建机制）；每次构建 Vec 丢失 const 语义且重复解析 |
| 语言切换机制 | 仅 config 重启生效 / **应用内下拉即时切换 + config 持久化** / 两者（推荐） | 应用内下拉 + config 持久化 | 主题 Dropdown 先例（即时 + Info toast + 持久化）已建立心智；`set_locale` 进程级 + 每帧 `t!()` 使即时切换成本极低；重启后保留选择 | 仅 config 无法即时预览，UX 差 |
| 切换后窗口标题 | 保持启动标题 / `ViewportCommand::Title` 更新 | ViewportCommand::Title | egui 官方示例同款；标题即 `t!("app.title")` 键，切换重发一次即可 | 保持启动标题则 en 下标题仍中文，不一致 |
| 语言下拉位置 | 工具栏 Group D / 状态栏 / 设置页 | 工具栏 Group D（主题旁） | 与主题并列，用户已熟悉「下拉即全局生效」交互；工具栏有空间 | 状态栏位偏角落发现性差；无设置页 |
| 数据串（股票/行业/题材名） | 翻译 / **不翻译** | 不翻译 | 数据源为中文 A 股市场数据，翻译无意义且破坏可检索性（代码/名称匹配） | 翻译数据导致搜索/关联断裂 |
| 后端错误细节 | 全翻译 / **模板翻译 + %{e} 原样** | 模板翻译 + %{e} 原样 | %{e} 为 DataError 技术细节，翻译成本高且失真 | 全翻译需在 error 源头做 locale 感知，收益低 |
| 组件默认文案（modal Confirm/Cancel 等） | 保留英文 / **t!() 键化** | t!() 键化 | 默认值也属用户可见文本；en.yml 恰好还原 "Confirm/Cancel" | 保留英文在 zh 态下不一致 |
| fork 日期格式键化 | fork 内嵌格式串写死 / **fork 自带 locales 键化** | fork 自带 locales 键化 | 锁定决策：fork 用自身 rust-i18n + 自带 locales；`set_locale` 全局同日生效 | fork 写死无法随语言切换 |
| kittest 断言 | 字面量 / **t!() 键解析** | t!() 键解析 | 断言跟随默认 zh 自动正确；en 测试可复用同一断言；加语言零改动 | 字面量在语言切换/加语言后全部失效 |
| en locale 测试并发 | 并行 / **LANG_LOCK 串行** | LANG_LOCK 串行（Mutex） | `set_locale` 进程全局，并行测试互相污染 locale 造成 flaky；复用 HOME_LOCK 先例 | 并行省时但不可靠 |
