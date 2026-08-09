# UI 设计（权威文档）

**本文件是 compass GUI 的最终权威 UI 设计文档**，累积式维护 —— 每次设计
（skwy-workflow 门禁第 1 步 DESIGN）经用户确认后，将最终设计要点同步至此。

> **组件使用规范**：逐组件的使用规范（何时用、用哪个变体、怎么组合、反模式）
> 见 `kb/design/ui-widgets.md`（权威文档）——24 个组件 × 8 字段统一模板。

> **归档与权威的区别**：`.omo/designs/<feature>.md` 是 ui-designer 产出的
> **过程归档**（原始方案）；`.omo/plans/<feature>.md` 是计划归档。
> 本文件才是 UI 设计的**最终版本**，与代码保持同步。归档文件不删不改，
> 但一切 UI 设计决策以本文件为准。

## 设计系统

### 设计 token（compass-ui）

GUI 全部视觉值来自独立 crate `compass-ui` 的 **design token 系统**
（`.omo/designs/gui-upgrade.md` §4，ref #123）——六类 token 逐项：
**颜色**（`ColorTokens`，暗/亮两套）/ **间距**（`SpacingTokens`）/
**字号**（`TypeTokens`）/ **圆角**（`RadiusTokens`）/ **阴影**（`ShadowTokens`）/
**动效**（`MotionTokens`）。UI 代码不硬编码颜色值（ref #123）。

### 主题预设

| 预设 | 描述 | 状态 |
|---|---|---|
| `compass_dark` | 默认暗色主题（TradingView 风格） | 已实现 |
| `compass_light` | 亮色主题，适合白天使用 | 已实现 |
| `compass_blue` | 深蓝主题 | 计划中（未实现；独立 issue 跟踪，见 `.omo/plans/gui-upgrade.md`） |

**Theme 自主化**（ref #126）：`CompassTheme` 不再封装 egui-charts 的 `Theme`
系统——`apply_theme` 由 `ColorTokens` **直接构造 `egui::Visuals`** 并映射到
`egui::Style`（egui 0.35 `set_theme` + `set_style_of`），消除「UI 主题由图表库
决定」的反向依赖；`apply_to_chart` 将 `ColorTokens.chart` 覆写为 egui-charts
`ChartSemanticTokens`（薄封装，仅图表渲染侧）。

主题持久化到 `~/.config/compass/config.toml` 的 `theme` 键（顶层）。

### 涨跌色（A 股惯例）

- **红涨绿跌**：`up` = #EF5350（红）/ `down` = #26A69A（绿）/ `flat` = 主文本色
- K 线柱、`PriceText`、StatusBar 摘要、Screener 表格共用同一 token（ref #123）

### 字体

- **SourceHanSansCN**（思源黑体）—— 中文字体，Regular + Bold 两字重，`include_bytes!` **内嵌**
- **JetBrains Mono** —— 价格/代码/时间等数字等宽字体（列对齐），内嵌
- **egui-phosphor**（Regular variant）—— 图标字体，随字体注册
- 字号层级：由 `TypeTokens` 定义（display 20 / heading 14 / body 12.5 /
  caption 11 / mono 12），经 `apply_theme` 写入 egui `TextStyle`（ref #124）

### 图标

- **egui-phosphor**（Regular variant）—— 工具栏按钮、toast 通知、状态提示
- 约定：图标 + 文字标签并用（不单独用纯图标，除非空间受限）

### 颜色

颜色全部由 `compass-ui` 的 `ColorTokens` 预设派生（`tokens/color.rs`），
经 `CompassTheme::apply_theme` 映射到 `egui::Visuals`，UI 代码不硬编码。

## 布局结构

```
┌─ 工具栏 (40px, Toolbar 组件)：[标的] [周期 1d|1w|1M] | [操作 Fetch] | [显示 侧栏/主题] ┐
├──────────┬──────────────────────────────────────────────────────────────┤
│ Sidebar  │  DockArea（egui_dock，可拖拽/关/开标签页）                      │
│ 240px    │  标签页: 图表 | 日志 | 选股器（中文标题 + Phosphor 图标）        │
│ 自选/搜索 │                                                                 │
├──────────┴──────────────────────────────────────────────────────────────┤
│ StatusBar (26px)：标的摘要(mono 涨跌) │ 加载/错误状态 │ 数据源 · 时钟        │
└─────────────────────────────────────────────────────────────────────────┘
浮层：Toast 通知（右上角）/ Modal 遮罩（全屏）—— 渲染于三栏之上
```

- **工具栏**：`compass-ui::widgets::Toolbar` 组件（40px），逻辑四组：
  **标的**（`SearchableDropdown`，Symbol 搜索）/ **周期**（`Segmented` 1d|1w|1M）/
  **操作**（`Button(Primary)` Fetch，loading 禁用+spinner）/ **显示**（侧栏切换
  `IconButton` + 主题 `Dropdown`）；组间强分隔线 + 16px 间距（ref #130）
- **Sidebar**：`SidePanel::left` 240px（resizable 200–320），自选股分组列表
  （名称 + mono 代码 + 交易所标签），行点击切图表、hover 删除（Modal 确认）、
  顶部搜索 + 添加按钮；watchlist 持久化到 `[watchlist]` 配置节（ref #131）
- **Dock 区**：egui_dock `DockState` + `compass-ui::dock_style()` 深度定制
  （tab 栏 28px、**仅 focused 面板 tab 高亮**（accent 文字 + accent 描边）、
  非 focused tab 平静（text_primary/secondary、无描边）——因每 leaf 单 tab
  结构下所有 tab 都是 active，靠 focused 区分当前面板；
  `hline_below_active_tab_name`、separator 三色），
  标签页中文标题 + 图标（ref #130）
- **StatusBar**：`TopBottomPanel::bottom` 26px 三段式——左段标的摘要
  （mono 价格 + 涨跌幅红涨绿跌）/ 中段状态（StatusDot：loading 脉冲/error 红点）/
  右段数据源信息 + 本地时钟（每秒刷新）（ref #130）
- **浮层**：Toast（右上角叠放，队列上限 10 条）、Modal（全屏半透明遮罩）
  —— 渲染在布局之上，随每帧渲染

### 面板职责

| 面板 | CitizenId | 职责 |
|---|---|---|
| Chart | `chart` | K 线图表（平移/缩放/十字准线），默认 100 根可见柱；空态引导（「输入代码并点击 Fetch」） |
| Logger | `logger` | 可滚动日志面板（tracing 事件）+ 导出按钮（保存文件对话框） |
| Screener | `screener` | 条件选股（基础/技术面两张卡片 + 结果 `DataTable`） |
| Sepa | `sepa` | 东方SEPA 评分（温度计 + TOP50 排名表 + 详情 + 图表联动），叠入 Chart leaf 双 tab |

### SEPA 评分面板（`TabKind::Sepa`，ref #152）

独立标签页「东方SEPA」，**叠入顶部 Chart leaf 双 tab**（dock tab 栏 `[图表] [东方SEPA]`）；
报告型心智模型（每日预计算排名，打开即读），与选股器的查询型模型分开。

面板内部自上而下：**① 温度计条 → ② 工具条 → ③④ 表格+详情水平分栏**。

- **① 市场温度计 Card**（恒显示）：温度计 icon + 「市场温度」+ score（色阶色）+ 仓位建议 Tag +
  5 指标 chip（chip tint = `score_color(heat)`，delta 箭头 A 股红涨绿跌）
- **② 工具条**：计数标签「共 N 行 · 日期」+ `Segmented ["TOP 50","TOP 30"]` + 刷新按钮
  （Primary + ARROW_CLOCKWISE；loading 禁用 + spinner；**纯手动触发，无自动计算**）
- **③ 12 列表格**（默认排序列 = 排名升序）：排名 `Rank`(1-3 warning) / 代码 `Text` /
  名称 `Text` / 总分 `Score{max:100}` / 趋势 `Score{max:30}` / 题材 `Score{max:25}` /
  资金 `Score{max:20}` / 形态 `Score{max:20}` / 风险 `Score{inverted}`（带符号 `-x.x`，
  按 `1-|v|/max` 反向色阶：0 扣分绿 → 满扣分红）/ 行业 `Text`（有题材时拼
  `行业 · 题材1 · 题材2`）/ 最新价 `Price` / 涨跌幅 `Price`
  **布局约束（ref #221）**：表格必须渲染在**垂直堆叠上下文**中（`allocate_ui_with_layout`
  固定宽度容器，宽 = 可用宽 − 详情面板 280px − spacing）——egui_extras TableBuilder
  假定 header 与 body ScrollArea 垂直堆叠，若直接放 `ui::horizontal` 会被并排
  （body 行渲染到表头右侧，真实 GUI 回归）。
- **④ 右侧详情面板**（~300px，行点击刷新 + 行高亮）：名称 + 排名 Tag + 总分大字 +
  五模块行（标签 + `score/max` + `ProgressBar`，`fill = score_color(norm)`）+
  子项 `SepaFactor` 列表（label + `score/max` + note）+ 题材 Tag 区
- **分数色阶** `score_color(norm)`：norm ≥0.8 success；0.5–0.8 `lerp(warning, success)`；
  0.25–0.5 `lerp(error, warning)`；<0.25 error（norm = `value/max`；风险列 `1-|v|/max`）
- **TOP N 切换**：仅截断**本地副本**（`rows.clone().truncate(top_n)`），**绝不回写
  shared_state**——切回 50 数据不丢
- **行点击**：复用 `dispatch_symbol_fetch`（screener 同源共享函数）联动图表
- **状态**：loading spinner / error colored_label + toast / 空态 EmptyState
  「暂无 SEPA 评分数据 / 点击刷新计算全市场 TOP50」
- **数据流**：第三条 citizen→Signal→AsyncDispatcher 通道（`RunSepaRequest` /
  `RunSepaResponse`），backend handler **进程内**调 `compass_strategy::sepa::run_sepa`
  ——GUI 只读 Parquet，不依赖 CLI 写回

## 交互规范

### 工具栏

- 标的：可搜索输入框（`SearchableDropdown`），`交易所 | 代码 | 名称` 格式，
  弹窗列匹配项，↑↓/Enter 键盘导航，空过滤显示「无匹配结果」
- 周期：`Segmented` 分段选择器 `1d | 1w | 1M`——**切换立即重载**（ref #218）：
  `set_timeframe` 同步 `shared_state.timeframe` 并无条件触发 `fetch_bars()`
  （loading 守卫不拦截切换——旧周期数据与标签不一致）；启动时 `timeframe_index`
  从配置 `default_timeframe` 派生（`timeframe_index_from_value`，与
  `timeframe_label` 双向同步）
- Fetch：`Button(Primary)` 主操作按钮，loading 时禁用 + 内嵌 spinner +「加载中…」
- 主题：`Dropdown` 下拉切换，即时全局生效 + Info toast「主题已切换」

### 快捷键

| 键 | 作用 |
|---|---|
| `/` | 聚焦工具栏标的输入框 |
| `Ctrl+Enter` | 触发 Fetch |
| `Ctrl+K` | 聚焦侧边栏搜索框 |
| `1` / `2` / `3` | 切换周期 1d / 1w / 1M |
| `Esc` | 关闭弹层 / Modal |

> 焦点守卫：`1/2/3` 与 `/` 在文本输入框聚焦时不触发（避免输入代码时误切换周期）。

### 图表（Chart）

- **平移**：点击水平拖拽
- **缩放**：鼠标滚轮
- **十字准线**：悬停 K 线显示 OHLCV 详情
- **空态**：未加载数据时显示 EmptyState 引导
- 数据源：本地 DuckDB `read_parquet()`，**无在线回退**
- **日期格式（中文，ref #219）**：所有日期显示中文——x 轴刻度紧凑式
  （`1月`/`5月15日`/`2024`，按 TickMarkType 固定映射、`%-m`/`%-d` 去填充），
  十字光标与 tooltip 完整式（`2024年5月15日`，按 bar 周期分档：日线+ 纯日期、
  盘中带时间）；tooltip 前缀全中文化（`时间:`/`开盘:`/`最高:`/`最低:`/`收盘:`/
  `成交量:`/`涨跌:`）。实现在 egui-charts fork（`DefaultTimeFormatter` +
  crosshair/tooltip 格式串），compass 侧零配置。
- **MA/BOLL 叠加层**（ref #174）：K 线上叠加 MA(5/10/60/120/250) 五条均线 +
  BOLL(20, 2.0) 三线（8 色暗/亮两套，`IndicatorTokens`）。指标**实时计算不存储**
  （compass-core 纯函数，GUI 每帧经缓存指纹重算）；缓存指纹
  `(symbol, len, 首末 bar time, 末根 close)` 防切标的/前复权重算碰撞。
- **图例行**（左上第二行，vendored OHLC 行下方）：85% alpha `bg_panel_alt`
  chip + 1px `border_strong` + `radius.sm`；MA 项 = caption 标签（`text_secondary`）
  + mono 值（线色）；BOLL 单标签 + 三值 ` / ` 连接；MA/BOLL 组间 1px 竖分隔线；
  数值格式复用 vendored `format_price`（≥100→2 位、≥1→4 位、<1→6 位）；暖机
  显示 `—`；不消费输入事件。
- **前复权 Tag**（工具栏「周期」组内）：非交互 `Tag`（`TagVariant::Custom` +
  info 色）——K 线均为前复权价（fetch 层缩放），本迭代无模式切换开关。

### 反馈状态

| 类型 | 表现 | 自动消失 |
|---|---|---|
| Loading | 工具栏 spinner + StatusBar 脉冲点 | 加载完成 |
| Success toast | 右上角 ✅ | 3 秒 |
| Warning toast | 右上角 ⚠ | 3 秒 |
| Error toast | 右上角 ❌ | 8 秒 |
| Info toast | 右上角 ℹ | 3 秒 |

toast 使用 Phosphor 图标字形，垂直堆叠，队列上限 10 条（超出淘汰最旧）。

### 模态（Modal）

- 全屏半透明遮罩 + 居中面板，`egui::Area` + `Order::Foreground`，
  背板以 `Sense::click()` 吞掉点击（非 `modal=true`——egui::Area 无原生焦点锁定）
- 打开动画（backdrop 120ms + panel scale 150ms）、关闭状态机（100ms fade）、
  Cancel(Ghost) + Confirm(Primary/Danger) 按钮
- **三个真实绑定场景**（ref #131）：
  1. **启动数据缺失引导**：`load_stock_list` 为空时首帧弹出（数据未就绪 + 导入提示 + 知道了）
  2. **日志导出**：Logger 面板导出按钮 → 保存文件对话框 → 写日志文本 → toast 反馈
  3. **移除自选确认**：Sidebar 行 hover 点 × → Danger 确认框（移除/保留）→ 移除 + 持久化

## 设计变更记录

| 日期 | 变更 | 来源归档 | 实现状态 |
|---|---|---|---|
| 2026-08-02 | 初始骨架：基于现有 GUI 提炼设计系统/布局/交互（ref #129） | — | 已实现（与代码同步） |
| 2026-08-02 | v2 全局升级：compass-ui 组件库 + design token + theme 自主化 + 三栏布局（Sidebar/StatusBar）+ 字体内嵌 + Modal 三场景 + 快捷键（ref #119/#123-#131） | `.omo/designs/gui-upgrade.md` | 已实现（与代码同步） |
| 2026-08-04 | MA/BOLL 叠加层（MA5/10/60/120/250 + BOLL 20,2 共 8 线）+ 图例行（左上第二行 chip）+ 工具栏「前复权」Tag（ref #174/#177/#178） | `.omo/designs/chart-ma-boll.md` | 已实现（与代码同步） |
| 2026-08-09 | 新增组件使用规范权威文档 `kb/design/ui-widgets.md`（24 组件 × 8 字段模板，与本文分工：本文管 token/布局/交互，组件文档管组件粒度用法） | `.omo/designs/ui-widgets.md` | 已同步（与代码同步） |
| 2026-08-09 | GUI 四问题修复：图表日期中文（x 轴紧凑 + 十字光标/tooltip 完整，fork 侧）、K 线切换立即重载 + index 对齐、选股器条件原子组 + 行距 sm、SEPA 表格垂直堆叠修复 + MultiSelect id_salt（ref #217/#218/#219/#220/#221） | `.omo/designs/ui-fixes-chinese-date.md` + `.omo/designs/ui-fixes-screener-layout.md` | 已实现（与代码同步） |
| 2026-08-09 | 验收修复：数值列对齐、涨跌幅单一百分比、DataTable 横向滚动、Tag 换行渲染、Button 文字/loading 主题色、concept_name TRIM（ref #217 用户验收 6 项） | `.omo/designs/ui-fixes-sepa-change-column.md` | 已实现（与代码同步） |

> 每次 DESIGN 门禁完成后，在此追加一行：日期、变更摘要、对应
> `.omo/designs/<feature>.md` 归档文件、实现状态。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| UI 设计权威文档位置 | `kb/design/ui.md`（独立文件） / 并入 `kb/user/gui.md` / 每 feature 独立文档 | 新建 `kb/design/ui.md` | 与 kb/user/gui.md（用户手册）职责分离；单一累积式文档满足"一份最终文档"诉求；与 architecture/data-providers/symbols 并列于 kb/design/ | 并入用户手册会混淆设计规范与使用说明；多份独立文档无法形成单一权威版本（ref #129，用户确认） |
| `.omo/designs/` 的定位 | 过程归档 / 权威文档 | 过程归档 | 项目书（kb/）是知识的唯一数据源；设计经用户确认后最终版必须同步到 kb/ | 让归档文件承载权威信息会导致 kb/ 与归档内容漂移（ref #129） |
| 组件库组织 | 独立 crate `compass-ui` / `crates/compass/src/widgets/` 扩展 | 独立 crate `compass-ui` | 用户核心需求即「通用可复用组件库」；workspace 多 crate 模式成熟；依赖方向单向（bin→ui，ui 零业务依赖）；独立 kittest 测试面 | widgets/ 扩展使组件与业务耦合，无法 bin 外复用（ref #119 D3） |
| Theme 架构 | 自建 token→直接构造 egui::Visuals / 保留封装 egui-charts Theme / 全自建含 chart 渲染 | 自建 token→Visuals 直构 + chart 薄封装 | `egui::Visuals` pub 字段可直接构造（egui-0.35 已验证）——UI 主题完全自主，消除「UI 由图表库决定」反向依赖；chart 渲染内部深度依赖 egui-charts Theme，薄封装成本边界最优 | 全自建 chart 渲染需重写 K 线/十字准线绘制成本极高；保留现状则 UI token 无法独立演进（ref #119 D2） |
| Dock 方案 | 深度定制 egui_dock 0.20 / 换 egui_tiles / 自建 | 深度定制 egui_dock 0.20 | egui_dock 0.20.1 `Style` 字段全 pub 可深度定制（TabBarStyle/TabStyle 7 交互态/分隔线/边框）；现有 tabs.rs/dock_state/kittest 全复用；需求固定三面板无需网格 | egui_tiles 自述"开发早期、功能不全"；重写 TabViewer+测试成本高；自建拖拽/重排/浮动成本极高（ref #119 D1） |
| 字体 | 思源+JetBrains Mono 全内嵌 / 仅系统路径 / 思源等宽 | 思源(中)+JetBrains Mono(数字) `include_bytes!` 全内嵌 | 数字等宽是金融终端列对齐硬需求；egui 无 tabular-nums，等宽家族等价替代；内嵌消除硬编码系统路径脆弱性（+17.3MB 用户批准） | 系统路径在无该字体机器上中文 tofu；思源等宽数字字形一般（ref #119 D4） |
| 组件 vs 依赖 | 自建为主 / 引 egui-notify、egui-modal 等 | 自建为主（仅保留 phosphor/file-dialog/extras/charts/dock） | 用户明确要求自建复用；现有 toast/modal 已是自建雏形；引库风格不统一、重复 | 引库与「自建为主」冲突；file-dialog 平台级能力自建成本极高保留（ref #119 D5） |
| 涨跌色 | A 股红涨绿跌 / TradingView 绿涨红跌 / 默认 | A 股红涨绿跌（#EF5350/#26A69A） | A 股用户心智（同花顺/东财一致）；token 统一 K 线与文本 | TV 惯例违背 A 股直觉（ref #119 D10） |
| Modal 绑定场景 | 保持占位 / 仅启动引导 / 三场景全接 | 启动引导 + 日志导出 + 删除确认 | 零新依赖、真实高频；激活闲置 file_dialog | 保持占位违背 epic 目标（ref #119 D9） |
| Toast 动画时间源 | 真实墙钟 `Instant::now()` / egui 虚拟时间 `ctx.input(|i| i.time)` / 注入 Clock trait | egui 虚拟时间（f64 秒字段 `created_at`/`close_started`，manager 缓存 `last_frame_time` 供 `push()` 打戳） | kittest 下虚拟时间按 `predicted_dt` 每帧推进、完全确定——根治慢 CI wall-clock 漂移导致的 flaky（ref #168）；真实 GUI 中 egui 帧时间本就正确驱动动画；无 Clock 注入的 API 膨胀 | 墙钟驱动动画使 kittest 测试依赖机器负载、慢 CI 间歇失败（#155 修后仍发 #168）；Clock trait 为单一消费方引入抽象、过度设计（ref #168） |
| Modal 动画时间源（ref #171） | 真实墙钟 `Instant::now()` / egui 虚拟时间 `ctx.input(|i| i.time)` / 注入 Clock trait | egui 虚拟时间（f64 秒字段 `open_started`/`close_started`，`open(now: f64)`/`close(now: f64)`/`toggle(now: f64)` 显式收参，`show()` 内取 `ctx.input(|i| i.time)`） | 与 Toast 同构（ref #168）——kittest 下虚拟时间按 `predicted_dt` 每帧推进、完全确定，根治慢 CI wall-clock 漂移导致的 flaky（ref #171）；真实 GUI 中 egui 帧时间本就正确驱动动画；无 Clock 注入的 API 膨胀 | 墙钟驱动动画使 kittest 测试依赖机器负载、慢 CI 间歇失败（#155 修后 #168 toast 同根因、#171 modal 同根因）；Clock trait 为单一消费方引入抽象、过度设计（ref #168） |
| MA 五线配色（ref #174） | A 股惯例 白/黄/紫/青/棕 / TV 蓝橙系 / 全灰阶单色 | 白/黄/紫/青/棕（暗亮两套按背景调明度） | 贴合 A 股用户心智（同花顺/东财均线色系）；五色相环均匀分布、2px 下两两可分；避开涨跌红绿与 accent 蓝 | TV 蓝橙系与 accent 蓝冲突；全灰阶丧失周期区分度 |
| MA5/MA10 色值来源（ref #174/#177） | 复用 `text_primary`/`warning` token / 独立新色值 | 复用既有 token 值 | 与语义别名惯例一致（error==up、info==accent）；白/黄正是惯例所需 | 独立色值制造冗余 token |
| BOLL 三线配色（ref #174） | 三线同色 slate / 上中下三色 | 三线同色 slate 灰蓝（`#90A4AE`/`#546E7A`） | 用户心智 BOLL 是整体指标（同花顺三线同色）；中轨=MA20 由图例标签承载 | 三色割裂「一个指标」认知 |
| BOLL 通道填充（ref #174） | 不填充 / 半透明填充 / 填充+三线 | 不填充 | vendored overlay renderer 无填充路径（需 fork 补丁）；填充遮挡 K 线主体；同花顺/东财默认无填充 | 半透明填充遮挡一级数据（OHLC）且增加 fork 维护面 |
| 图例实现位置（ref #174） | ChartCitizen 自绘 overlay / patch vendored `render_legend` | ChartCitizen 自绘 overlay | 零 vendored 变更；图例数据源经公开 `visible_range()` 可得 | patch vendored 与图表库 legend 布局强耦合 |
| 图例数值格式（ref #178 review） | 复用 vendored `format_price` 规则 / 固定 2 位小数 | format_price 规则（≥100→2 位、≥1→4 位、<1→6 位） | 与上方 OHLC legend 精度一致；低价股不失真 | 固定精度在 <1 元股失真 |
| 指标重算时机（ref #174/#178） | 每帧重算 / 数据变化缓存重算 | 缓存重算，键 = `(symbol, len, 首末 bar time, 末根 close)` | 每帧 O(n) 分配浪费；symbol 防切标的碰撞、close 防前复权重算同窗口价格修正后的陈旧叠加 | 每帧重算虽量级小但属工程浪费 |
| 指标色 token 组织（ref #177） | 新增 `IndicatorTokens` 子结构 / 扩展 `ChartTokens` | 新增 `IndicatorTokens`（8 色暗亮两套） | 指标色与图表骨架色语义独立；`ChartTokens` 已被 `apply_to_chart` 逐字段消费 | 扩 ChartTokens 使 apply_to_chart 与指标色映射耦合 |
| 图表日期格式（ref #219） | 全英文 / 仅 x 轴中文 / 全部中文（x 轴紧凑 + 十字光标/tooltip 完整） | 全部中文：x 轴 `1月`/`5月15日`/`2024`（`%-m`/`%-d` 去填充）；十字光标/tooltip `2024年5月15日`（按 bar 周期分档） | 用户确认；A 股终端（同花顺/东财）惯例；紧凑式概览 + 完整式精确双层，日线不再显示无意义 00:00:00 | 仅 x 轴中文留 tooltip 英文混排；统一格式日线显示 00:00:00 |
| 图表日期实现位置（ref #219） | fork `DefaultTimeFormatter` 直接改中文 / LocaleTimeFormatter 中文分支 / compass 侧覆写 | fork `DefaultTimeFormatter` 直接改 + crosshair/tooltip 格式串 | compass 无 locale 配置，fork 默认路径最小改动；LocaleTimeFormatter 是死代码路径 | Locale 分支需 compass 加 `.with_locale()` 扩大改动面 |
| tooltip 标签中文化（ref #219） | 保持英文 / 一并中文化 | 一并中文化（`时间:`/`开盘:`/`最高:`/`最低:`/`收盘:`/`成交量:`/`涨跌:` + tracking 缩写） | 用户确认；避免 "Time: 2024年5月15日" 混排 | 保持英文留混排（#222 全 GUI i18n 另立） |
| K 线切换立即重载（ref #218） | 仅同步 index / 同步 + 触发 fetch / loading 时延迟 | `set_timeframe` 同步 `shared_state.timeframe` 并无条件 `fetch_bars()` | 切换即重载符合直觉；loading 中的数据属旧周期，跳过则图表与标签不一致 | loading 延迟语义复杂且切换被吞 |
| 选股器条件原子组（ref #220） | 单 `horizontal_wrapped` / 每组 `ui::horizontal` + 组间 `add_space(md)` | 每组 `ui::horizontal` 原子单元 + 行距 `item_spacing.y = sm`(8px) | 组内标签+控件永不拆行；换行只发生在组间；egui 0.35 实测 18px 行高钳制 + SectionTitle RTL 空子 ui 游标跳转需 `scope_builder` + 实测标签宽 max_rect 封住 | 自定义换行/表格布局复杂且脆弱 |
| MultiSelect 弹层 id（ref #220 实测） | 默认空 id_salt / 每实例显式 id_salt | 每实例显式 `id_salt`（screener 三处） | `popup_id = ui.id().with("multi_select_popup").with(&id_salt)`——空 salt 时多弹层 Area id 冲突互相覆盖 | 空 salt 依赖调用方容器 id 区分（同一面板内不成立） |
| SEPA 表格布局容器（ref #221） | `ui::horizontal` 直接放表格 / `allocate_ui_with_layout` 垂直容器 | 垂直容器（宽 = 可用宽 − 详情 280px − spacing） | egui_extras TableBuilder 假定 header/body ScrollArea 垂直堆叠，horizontal 布局把二者并排（body 行渲染到表头右侧——真实 GUI 回归，坐标断言复现） | horizontal 直放仅 kittest label 存在性断言通过，位置错误漏检 |
| 数值列对齐（ref #217 验收） | body 一律左对齐 / render_cell 感知列 numeric | `render_cell` 按列 `numeric` 标志右对齐（与 header 同布局） | 表头 numeric 列本就右对齐，body 左对齐导致 9/12 列错位（用户验收：列与表头不对齐）；共享组件一处修复 SEPA + screener | 统一左对齐丢失 numeric 列右对齐惯例 |
| 涨跌幅列单一百分比（ref #217 验收） | `Price{value, change}` 双值渲染 / `percent_only()` 模式 | `PriceText::percent_only()` + `render_cell` 以 `value == change` 识别 | 调用方 value（排序）+ change（着色）职责正确，缺「值即百分比」渲染模式——原实现同值渲染两次 `2.50 +2.50%`（用户验收）；StatusBar `1500.00 +2.50%` 双值语义不受影响 | 新增 DataCell 变体扩大 API；改调用方传参丢排序/着色语义 |
| DataTable 宽度约束（ref #217 验收） | auto 列随内容 / 横向 ScrollArea 吸收溢出 | `ScrollArea::horizontal` 包 TableBuilder（auto_shrink false） | auto 列 min_rect 帧间增长撑宽 horizontal，把 SEPA 详情面板推离面板（用户验收：右侧一团乱，kittest 坐标 1437.9 > 1388 复现） | 固定列宽丢失自适应；截断内容 |
| Tag 渲染方式（ref #217 验收） | `Frame::show` / `allocate_exact_size` + painter + `ui.put` | `allocate_exact_size` 固定 rect + painter 画 pill + `ui.put` 放 Label | Frame 的响应 rect 撑宽 wrapped 父级 max_rect，`horizontal_wrapped` 永不换行——35+ 题材 Tag 单行溢出 280px 面板 4 倍宽（用户验收：海康威视题材换行失败）；`ui.put` 保持 accesskit 可查询 | Frame 简但破坏换行；painter 文本丢失 accesskit |
| Button 文字主题色（ref #217 验收） | 硬编码 `Color32::WHITE` / 统一 `text_primary` | Primary/Danger 统一 `text_primary`（dark 浅灰 / light 深色） | 硬编码白字不随主题切换（用户验收：fetch 按钮文字颜色不跟随主题）；Default/Ghost 已用 text_primary | 白字在 accent 底对比度虽高但不随主题 |
| Button loading 文字色（ref #217 验收） | loading 视同 disabled 用 text_disabled / loading 保留变体色 | loading 保留变体文字色，仅真 disabled 用 text_disabled | loading 由 spinner + 遮罩表达状态，文字变灰在 accent 底几乎不可见（用户验收：加载中字体颜色不对） | loading 变灰符合 disabled 惯例但可读性差 |
| concept_name 空白清理（ref #217 验收） | 原样入库 / INSERT 时 `TRIM(BOARD_NAME)` | 采集器 import 时 TRIM + 清理存量 11 行 | EastMoney BOARD_NAME 带尾随空格（`创新医疗服务   `），SEPA 题材 Tag 渲染拉伸空格（用户验收：第一个字后空格越来越多）；Dolt 数据已同步清理 | 仅 GUI 侧截断掩盖数据质量问题 |
