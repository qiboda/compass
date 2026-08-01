# UI 设计（权威文档）

**本文件是 compass GUI 的最终权威 UI 设计文档**，累积式维护 —— 每次设计
（compass-workflow 门禁第 1 步 DESIGN）经用户确认后，将最终设计要点同步至此。

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
  （tab 栏 28px、选中 accent 文字、`hline_below_active_tab_name`、separator 三色），
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

## 交互规范

### 工具栏

- 标的：可搜索输入框（`SearchableDropdown`），`交易所 | 代码 | 名称` 格式，
  弹窗列匹配项，↑↓/Enter 键盘导航，空过滤显示「无匹配结果」
- 周期：`Segmented` 分段选择器 `1d | 1w | 1M`
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
