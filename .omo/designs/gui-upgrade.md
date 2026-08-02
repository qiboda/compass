# compass GUI 全局升级设计方案 v2（epic #119）

> 状态：待评审 · v2 修订日期：2026-08-01 · 设计范围：`crates/compass`（GUI 二进制）+ 新增 `crates/compass-ui`（组件库 crate）
> 对标：TradingView 暗色专业金融终端 + A 股红涨绿跌惯例 · 技术栈：egui 0.35 / egui_dock 0.20.1 / egui-charts（qiboda compass fork）
>
> **v2 相对 v1 的变更**：用户推翻 v1 的 5 条不可变约束（字体可换、DockArea 可改、theme 架构可重写、UI 依赖可新增），并新增核心需求——**设计 compass 通用/自定义 GUI 组件库 + 独立 design token 系统**。本文档为完整 v2 方案，v1 中仍成立的设计（Sidebar/StatusBar/工具栏分组/Modal 真实绑定/交互动画）在新约束下继承并调整。

---

## 一、目标

1. **建立 compass 通用 GUI 组件库**（独立 crate `compass-ui`）：基础/复合/业务三级组件，跨模块复用，自建为主、依赖为辅。
2. **独立 design token 系统**：颜色/间距/字号/圆角/阴影/动效全部 token 化，暗色+亮色两套映射，由 token 直接驱动 egui Style 与图表配置。
3. **三栏式专业终端布局**：Sidebar（自选股）+ 中央 DockArea（深度定制 egui_dock 0.20 Style）+ StatusBar（数据状态/时钟/数据源）。
4. **工具栏逻辑分组**：Symbol / TF / Fetch 分组，Fetch 主操作按钮。
5. **激活占位组件**：Modal / FileDialog 绑定真实操作（启动引导 / 日志导出 / 删除确认）。
6. **字体升级**：思源黑体（中文）+ 等宽数字字体（价格/代码对齐），内嵌优先。
7. **全套交互反馈**：hover / press / loading / error / empty + egui 内置动画 API 过渡，动效时长统一 token 化。

**保留约束**：原生 egui（不引入 Web/Tauri，用户明确确认）；不引入 egui_notify / egui-modal 等重复组件库依赖（自建为主）。

---

## 二、现状盘点（逐文件，含 v2 新增证据）

### 2.1 布局骨架 — `crates/compass/src/main.rs`（1249 行）

| 位置 | 现状 | v2 处置 |
|---|---|---|
| `main.rs:32-55` `setup_cjk_fonts` | 硬编码 `/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf`，仅 Regular 一个字重 | 迁入 compass-ui `fonts::setup_fonts()`：内嵌字体优先 + 系统路径 fallback + 新增等宽字体（见 3.4） |
| `main.rs:61-172` `main()` | 窗口 1280×720 | 提升至 1440×900（待确认 Q8）；字体/主题/组件初始化改走 compass-ui 入口 |
| `main.rs:313-337` `CompassApp` | 18 个字段 | 保持，新增 sidebar/statusbar 相关字段 |
| `main.rs:339-412` `ui()` | 工具栏背景手算 `panel_fill±15`（L344-360） | 删除手算，改读 `ThemeTokens`；SidePanel/TopBottomPanel 接入 |
| `main.rs:443-537` `render_toolbar` | 单行混排无分组 | 用 compass-ui `Toolbar` 容器重构（见 3.3 工具栏设计） |
| `main.rs:422-441` `sync_picker_from_symbol` | screener→picker 反同步 | sidebar 点击复用 |
| `main.rs:263-288` `save_screener_config` | TOML 整文件改写 | watchlist 持久化复用此模式 |
| `main.rs:557-1249` | kittest 断言控件文本 | 组件迁移后测试随迁 compass-ui + bin 内同步更新 |

### 2.2 主题 — `crates/compass/src/theme.rs`（214 行）

- 现架构：`CompassTheme` 包装 `egui_charts::theme::Theme`，`apply_theme` 调 egui-charts 的 `apply_to_egui`（theme.rs:58-60），即 **UI 全局主题由图表库决定**（反向依赖）。
- **v2 证据**：`egui::Visuals`（egui-0.35.0/src/style.rs:985）为 pub struct、字段全 pub（panel_fill L1068 / window_fill L1059 / extreme_bg_color L1041）——**可直接构造，UI 主题可完全自建**；`epaint::Shadow`（epaint-0.35.0/src/shadow.rs:10）pub struct 支持阴影 token。
- egui-charts 侧：`ChartSemanticTokens`（egui-charts-0.2.0/src/theme/semantic.rs:184）含 `grid_line` L193 / `grid_line_major` L194 / `crosshair_line` L201 等——**chart 渲染 token 保留对 egui-charts 的薄封装是合理成本边界**（图表内部渲染深度依赖其 Theme，全自建成本高）。
- **v2 决策**：theme.rs 重写为自建 token 系统（3.2 节），`compass_dark/compass_light` 各映射一套 token。

### 2.3 Dock 桥 — `crates/compass/src/tabs.rs`（235 行）

- `TabKind {Chart, Logger, Screener}`（L50-54），`title()` 硬编码英文（L57-63）。
- **v2 新证据**：egui_dock 0.20.1 `Style`（registry `egui_dock-0.20.1/src/style.rs`）定制能力**远超 v1 假设**：
  - `TabBarStyle`（style.rs:162-185）：`bg_fill` / `height` / `inner_margin` / `corner_radius` / `hline_color` / `fill_tab_bar`
  - `TabStyle`（style.rs:190-228）：`active/inactive/focused/hovered/active_with_kb_focus/inactive_with_kb_focus/focused_with_kb_focus` 共 **7 种交互态**（各含 outline_color / corner_radius / bg_fill / text_color）+ `tab_body`（内边距/描边/圆角/背景）+ `spacing` + `minimum_width` + `hline_below_active_tab_name`
  - `SeparatorStyle`（style.rs:137-157）：`width` / `color_idle/hovered/dragged`（节点分隔线）
  - `OverlayStyle`（style.rs:267-300）：拖拽 overlay 全套
  - `main_surface_border_stroke/rounding`、`dock_area_padding`（style.rs:54-57）
  - README 明示 "Highly customizable look and feel"
- **v2 决策**：**保留 egui_dock，深度定制 Style**（3.1 节）。唯一降级项：tab 选中「下划线滑动动画」无法注入 painter（egui_dock 无自定义绘制 hook），改为颜色/背景过渡（TabInteractionStyle 静态配置 + egui 全局 animation_time 内建过渡）。

### 2.4 组件 — `crates/compass/src/widgets/`

| 文件 | 现状 | v2 处置 |
|---|---|---|
| `toast.rs`（345 行） | 4 色硬编码（L129-144）、无动画、15% 色底 | 迁移 compass-ui 重写：token 化 + 动效（3.5 交互规格） |
| `modal.rs`（385 行） | 完整但零调用；无动画 | 迁移 compass-ui 增强：动画 + danger 按钮 + 三真实场景 |
| `searchable_dropdown.rs`（360 行） | StockPicker 逻辑稳定 | 迁移 compass-ui 为 `SearchableDropdown`，统一行高/高亮 |

### 2.5 面板 — `crates/compass/src/citizens/`

| 文件 | 现状 | v2 处置 |
|---|---|---|
| `chart.rs`（141 行） | 每帧 apply/update/show；无空态 | 空态引导 + symbol 回填；chart token 走新映射 |
| `logger.rs`（81 行） | 包装 egui_lens，导出能力未接线 | 面板工具栏 + 导出按钮 → FileDialog |
| `screener.rs`（849 行） | 单行卷绕表单 + striped 表格 | 表单分区 + 重置按钮 + 涨跌色；表格改用 compass-ui `DataTable` |

### 2.6 状态与数据流

- `state.rs`（53 行）：10 个 `Dynamic<T>`；v2 新增 watchlist 字段（Sidebar）。
- `compass-core/src/model.rs`：`AppConfig`（L194-208）、`AppSection`（L258-266）、`StockBasic`（L106-129，Sidebar 数据充足）。v2 增 `WatchlistConfig`。
- 依赖版本（Cargo.toml workspace）：egui 0.35 / egui_dock 0.20 / egui_extras 0.35 / egui-phosphor 0.13 / egui-file-dialog 0.14 / egui_lens 0.5 / egui_mobius 0.5。registry 已存在 `egui_dock-0.20.1`、`egui-charts-0.2.0`（qiboda fork 的官方基版，fork compass 分支在其上扩展）。

---

## 三、核心架构决策（v2 重点）

### 3.1 Dock 决策：深度定制 egui_dock 0.20（保留，不换库）

| 方案 | 评估 |
|---|---|
| **深度定制 egui_dock 0.20**（选择） | Style 字段证据充分（2.3 节）：tab 栏高度/背景/圆角/7 交互态/分隔线/边框/overlay 全可配；现有 `tabs.rs`/`main.rs` dock_state 初始化/kittest 测试全部复用；需求为固定三面板 + 拖拽调整（egui_dock 内建），**无需网格布局/多子节点** |
| 换 egui_tiles | README 自述"much earlier in development、不完整"（egui_dock-0.20.1/README.md）；需重写 TabViewer 层 + dock_state 初始化 + 全部 kittest；收益（网格布局）非本需求 |
| 自建 tab/dock | 拖拽/重排/浮动/调整大小全手写，数百行高风险；egui_dock 已覆盖且可定制，自建无理由 |

**落地**：`main.rs:141` `Style::from_egui(...)` 改为 `compass-ui` 提供的 `dock_style(tokens)` 构建器；具体定制值见 6.1 节。

### 3.2 Theme 自主化：自建 token 系统 → 直接映射 egui Style + chart 薄封装

**架构**（替代现 theme.rs 的「封装 egui-charts Theme → apply_to_egui 决定全局 UI」）：

```
compass-ui/src/theme/
  tokens/          设计 token 原始值（dark / light 两套字面量）
    color.rs       ColorTokens
    spacing.rs     SpacingTokens
    typography.rs  TypeTokens
    radius.rs      RadiusTokens
    shadow.rs      ShadowTokens
    motion.rs      MotionTokens
  mod.rs           CompassTheme { name, tokens: ThemeTokens }
  apply.rs         ① tokens → egui::Style（Visuals 直接构造，已验证 pub fields）
                   ② tokens.chart → egui-charts ChartSemanticTokens（薄封装，保留 apply_to_config）
```

- **UI 部分全自主**：`apply()` 直接用 `ColorTokens` 构造 `egui::Visuals`（panel_fill/window_fill/extreme_bg_color/widgets 全套）+ `egui::Style`（spacing/text_styles 字号层级），**不再调用 egui-charts `apply_to_egui`**——消除「UI 主题由图表库决定」的反向依赖，UI 与图表解耦（换图表库不影响 UI）。
- **图表部分薄封装**：`apply_to_chart()` 把 `ColorTokens.chart`（grid_line/crosshair/candle_up/candle_down 等）映射到 egui-charts `ChartSemanticTokens`（fork 结构，见 2.2 证据），仍走 `apply_to_config`——图表内部渲染深度依赖其 Theme，全自建成本高收益低。
- **可行性已验证**：`Visuals` pub 字段可构造（egui-0.35.0/src/style.rs:985）；`ChartSemanticTokens` 存在（egui-charts-0.2.0/src/theme/semantic.rs:184）。
- **兼容**：`CompassTheme::all_names()` / `from_config()` 接口保持（bin 侧 `theme` 字段类型不变，仅内部实现替换）；`theme.rs` 迁移到 compass-ui，bin 侧留 re-export。
- 影响面：`main.rs:140-141, 341`（theme 构造/apply/dock_style）、`tabs.rs:124`（theme 传递）、`chart.rs:60`（apply_to_chart）。

### 3.3 组件库代码组织：独立 crate `compass-ui`（推荐）

| 方案 | 评估 |
|---|---|
| **独立 crate `crates/compass-ui`**（选择） | workspace 已有多 crate 模式（core/types/strategy/data）；用户核心需求是「通用可复用组件库」，独立 crate 提供：① 组件与业务解耦的清晰边界（citizens 在 bin，组件在 lib）② 独立 kittest 测试面（不拖累 bin 的覆盖率统计口径）③ 未来 compass-data 或其他工具可复用 ④ 依赖方向单向：`compass-ui ← compass(bin)`，compass-ui 不依赖 core/types（纯 UI，数据模型由调用方传入） |
| `crates/compass/src/widgets/` 扩展 | 现状 3 个组件已在此；但组件与 citizens 同 crate 耦合，无法被 bin 外复用；widgets/ 目录会膨胀为「组件库+业务半成品」混合体，边界模糊 |

**crate 结构**：

```
crates/compass-ui/
  Cargo.toml           依赖: egui, egui-phosphor, egui_extras, egui-charts(workspace fork), egui_dock=0.20, emath；dev: egui_kittest
  # 注: egui-charts 供 theme/apply.rs 映射 ChartSemanticTokens；egui_dock 供 dock_style.rs 构造 Style；emath 供动效 easing 引用
  src/lib.rs           模块导出 + CompassTheme 前置 re-export
  src/tokens/          设计 token（3.4/4 节）
  src/theme/           主题映射（3.2 节）
  src/fonts.rs         字体注册（3.4 节）
  src/widgets/         组件库（第 5 节）
    mod.rs
    button.rs  icon_button.rs  input.rs  dropdown.rs  checkbox.rs
    tag.rs  badge.rs  status_dot.rs  tooltip.rs  empty_state.rs
    card.rs  divider.rs  label.rs  price_text.rs  segmented.rs  section_title.rs
    searchable_dropdown.rs   （迁移自 bin widgets/）
    multi_select.rs
    toast.rs                （迁移自 bin widgets/）
    modal.rs                （迁移自 bin widgets/）
    data_table.rs
    toolbar.rs  sidebar.rs  status_bar.rs
  src/dock_style.rs    egui_dock Style 构建器（3.1 节落地）
```

**迁移范围**：`bin/widgets/{toast,modal,searchable_dropdown}.rs` 三个现有组件迁入 compass-ui 并重写增强；`tabs.rs` TabViewer（依赖 citizen 层）留在 bin。

### 3.4 字体方案：思源黑体（中文）+ JetBrains Mono（数字等宽）

| 方案 | 评估 |
|---|---|
| **思源黑体 + JetBrains Mono，内嵌优先 + 系统 fallback**（选择） | ① 中文正文保持思源黑体（现有 SourceHanSansCN，用户已认可；补 Bold 字重用于标题/主按钮）② 价格/代码/时间等数字场景用 **JetBrains Mono**（等宽，列对齐是金融终端硬需求）③ egui 0.35 无 OpenType tabular-nums 特性支持（已查：FontDefinitions 无 feature 控制），**等宽字体族是 tabular 的等价替代**——价格文本统一 `RichText::monospace()` ④ 字体文件 `include_bytes!` 内嵌（消除 main.rs:33 硬编码系统路径的脆弱性），系统路径保留为 fallback。**体积实测（2026-08-01）**：SourceHanSansCN-Regular.otf 8.4MB + Bold.otf 8.6MB + JetBrainsMono-Regular.ttf 0.27MB ≈ **+17.3MB**（两字重全内嵌）——体积权衡见待确认 Q6（有降级选项：仅内嵌 Regular + Bold 走系统 fallback，+8.7MB） |
| 仅系统路径（现状） | main.rs:33 硬编码 `/usr/share/fonts/...`，换机器/无该字体则中文 tofu；无等宽字体 |
| 思源等宽（Source Han Mono） | 覆盖中文等宽但数字字形一般；与 JetBrains Mono 相比数字可读性/风格差；License 同为 OFL，非优选 |

**注册策略**（fonts.rs）：
- Proportional = [SourceHanSansCN-Regular, SourceHanSansCN-Bold, Default 兜底] + phosphor 图标
- Monospace = [JetBrains Mono, SourceHanSansCN]（数字场景显式 mono）
- 中英混排：正文英文走思源（统一字重视觉）；仅价格/代码/时间/表格数字走 mono。

### 3.5 依赖策略：自建为主、依赖为辅（评估表）

| 场景 | 决策 | 理由 |
|---|---|---|
| Toast / Modal / Tooltip / EmptyState / Tag / Badge / StatusDot / PriceText / SegmentedControl / DataTable / SearchableDropdown / MultiSelect / Card | **自建**（compass-ui） | 用户明确要求自建复用；egui 生态无统一风格实现，引库风格不统一；现有 toast/modal 已是自建雏形 |
| Button / Checkbox / Dropdown / Input | **包装自建**（基于 egui 原生原子 + compass 变体样式） | egui Button/Checkbox/ComboBox 功能够，但样式无法统一变体（primary/danger/icon），包装一层统一 token 接入 |
| 图标 | 保留 egui-phosphor 0.13 | 已用且成熟，自绘图标成本高 |
| 文件对话框 | 保留 egui-file-dialog 0.14 | 平台级能力（目录浏览/最近路径），自建成本极高；已接线（main.rs:164） |
| 表格 | 保留 egui_extras TableBuilder（DataTable 内部使用） | 虚拟滚动/列宽成熟；自建表格成本高 |
| 图表 | 保留 egui-charts（fork） | 核心资产，K 线/十字准线/缩放平移成熟 |
| 布局 dock | 保留 egui_dock 0.20 | 3.1 节证据 |
| **不引**：egui-notify / egui-modal / egui_tiles / egui_plot / egui_commonmark | 明确排除 | notify/modal 与自建重复；tiles 见 3.1；plot 与 charts 重复；无 Markdown 需求 |
| 打开目录按钮（启动引导 Modal） | 可选引 `opener` 0.8 | 桌面打开路径能力；registry 已有 opener-0.8.5。v1 建议不加，v2 保持可选（待确认 Q11） |

---

## 四、design token 系统（完整结构）

> token 分六类，全部定义于 compass-ui/src/tokens/。暗色主值（TradingView 风格）与亮色映射各一套。数值为建议值，落地以渲染校准。

### 4.1 ColorTokens（颜色）

| token | 暗色 | 亮色 | 用途 |
|---|---|---|---|
| `bg_app` | #131722 | #F5F7FA | 应用底色（TradingView 经典背景） |
| `bg_panel` | #1E222D | #FFFFFF | 面板/Card/弹层底 |
| `bg_panel_alt` | #2A2E39 | #EDEFF2 | 次级面板/工具栏/StatusBar |
| `bg_hover` | #2A2E39 | #E8EBEF | 行/控件 hover |
| `bg_active` | #363A45 | #DDE1E6 | 选中/按下态底 |
| `border` | #2A2E39 | #D6DAE0 | 细边框 |
| `border_strong` | #363A45 | #B8BEC7 | 分隔线/强调边框 |
| `text_primary` | #D1D4DC | #1B2430 | 主文本 |
| `text_secondary` | #787B86 | #5A6472 | 次文本 |
| `text_weak` | #5D606B | #8A93A0 | 弱文本/占位 |
| `text_disabled` | #464A55 | #B8BEC7 | 禁用 |
| `accent` | #2962FF | #2962FF | 强调（主操作/选中/链接） |
| `accent_hover` | #4D7FFF | #4D7FFF | accent hover |
| `accent_pressed` | #1E4FD6 | #1E4FD6 | accent pressed |
| `up` | #EF5350 | #D93025 | 涨（A 股红涨） |
| `down` | #26A69A | #0E8F6E | 跌（A 股绿跌） |
| `flat` | #D1D4DC | #5A6472 | 平 |
| `success` | #34C77B | #188A51 | 成功 |
| `warning` | #F5A623 | #B57A00 | 警告 |
| `error` | #EF5350 | #D93025 | 错误/danger |
| `info` | #2962FF | #2962FF | 信息 |
| `selection_bg` | accent 20% alpha | 同 | 选中行/文本选区 |
| `chart.grid_line` | #2D323C | #E4E7EC | 图表网格（对齐 egui-charts ChartSemanticTokens） |
| `chart.grid_line_major` | #363A45 | #D6DAE0 | 主网格 |
| `chart.crosshair` | #64A0A0 | #3D7A7A | 十字准线 |
| `chart.candle_up` | #EF5350 | #D93025 | 阳线（= up） |
| `chart.candle_down` | #26A69A | #0E8F6E | 阴线（= down） |
| `chart.volume_up/down` | up/down 60% alpha | 同 | 成交量柱 |

### 4.2 SpacingTokens（间距/尺寸）

| token | 值 | 用途 |
|---|---|---|
| `xs` | 4px | 紧凑间隔 |
| `sm` | 8px | 组内间隔 |
| `md` | 12px | 组间/常规 |
| `lg` | 16px | 面板内边距 |
| `xl` | 24px | 区块间隔 |
| `xxl` | 32px | 大区块 |
| `control_sm` | 24px | 小控件高（Tag/IconButton sm） |
| `control_md` | 32px | 常规控件高（Button/Input/Dropdown） |
| `control_lg` | 40px | 大控件高（工具栏/主按钮） |
| `toolbar_h` | 40px | 工具栏高 |
| `statusbar_h` | 26px | StatusBar 高 |
| `sidebar_w` | 240px（resizable 200-320） | Sidebar 宽 |
| `tabbar_h` | 28px | dock tab 栏高 |
| `table_row_h` | 18px | 表格行高（保持现状 screener.rs:320） |

### 4.3 TypeTokens（字号）

| token | 值 | 用途 |
|---|---|---|
| `display` | 20px | 大数字/窗口标题 |
| `heading` | 14px | 面板标题/表头 |
| `body` | 12.5px | 正文（egui 默认，保持） |
| `caption` | 11px | 辅助/标签 |
| `mono` | 12px | 价格/代码/时间（JetBrains Mono，`RichText::monospace`） |
| 字重 | Regular / Bold（SourceHanSansCN 补 Bold） | 标题/主按钮 Bold |

### 4.4 RadiusTokens（圆角）

| token | 值 | 用途 |
|---|---|---|
| `sm` | 4px | 输入框/按钮/行 |
| `md` | 6px | Card/面板 |
| `lg` | 10px | Modal/弹层 |
| `pill` | 999px | Tag/Badge |

### 4.5 ShadowTokens（阴影，`egui::Shadow`）

| token | 值 | 用途 |
|---|---|---|
| `popup` | offset(0,4) blur 12 spread 0 rgba(0,0,0,0.35)（亮色 0.15） | Dropdown/Toast 弹层 |
| `modal` | offset(0,8) blur 24 spread 0 rgba(0,0,0,0.50)（亮色 0.25） | Modal backdrop 之上 |

### 4.6 MotionTokens（动效，供第 7 节引用）

| token | 值 | 用途 |
|---|---|---|
| `fast` | 100ms linear | toast 关闭/行 hover |
| `base` | 150ms cubic_out | toast 入场/modal 面板/状态切换 |
| `slow` | 300ms cubic_in_out | 大范围过渡（面板显隐） |
| easing | linear / cubic_out / cubic_in_out（emath::easing） | 统一缓动集合 |

---

## 五、组件库设计（compass-ui）

> 三级分类：**基础组件（atoms）**= 单一职责原子；**复合组件（molecules）**= 组合原子形成交互单元；**业务组件（organisms）**= 留在 bin 的 citizen 面板。每个组件给出：职责 / 配置项（builder 风格，egui 即时模式惯例）/ 视觉规格 / 落地文件。

### 5.1 基础组件（atoms）

| 组件 | 职责 | 配置项（props 概念） | 视觉规格 | 文件 |
|---|---|---|---|---|
| `Button` | 统一按钮（变体/尺寸/loading） | `variant: Default\|Primary\|Danger\|Ghost`；`size: Sm\|Md\|Lg`；`icon: Option<&str>`；`loading: bool`；`disabled` | Primary：accent 底 + 白字 + radius_sm；Danger：error 底；Ghost：透明底 + border；hover accent_hover/背景过渡；loading 内嵌 spinner + 60% 透明度 | `widgets/button.rs` |
| `IconButton` | 图标按钮 | `icon`；`tooltip`；`size` | 32×32（sm 24），hover bg_hover，radius_sm，icon 16px text_secondary | `widgets/icon_button.rs` |
| `Input` | 文本输入（统一外观） | `value`；`placeholder`；`prefix/suffix icon`；`monospace: bool` | 高 control_md，radius_sm，focus 边框 accent 1.5px，placeholder text_weak；monospace 用 mono 字体 | `widgets/input.rs` |
| `Dropdown` | 通用下拉（替代 egui ComboBox 统一风格） | `options`；`selected`；`width`；`searchable`（可选） | 触发控件高 control_md；popup：bg_panel + shadow_popup + radius_md，选项行高 28px hover bg_hover，选中 accent 文字 | `widgets/dropdown.rs` |
| `Checkbox` | 复选（统一外观） | `checked`；`label`；`disabled` | 24px 命中区；勾选 accent | `widgets/checkbox.rs` |
| `Tag` | 短标签（交易所/板块/行业） | `text`；`variant: Exchange\|Board\|Industry\|Custom`；`color` | 高 20px、padding 6px、radius_pill、9-11px 文字；交易所配色：SH 蓝 #2962FF / SZ 绿 #0E9F6E / BJ 紫 #8B5CF6（白字） | `widgets/tag.rs` |
| `Badge` | 数字角标（计数） | `count`；`tone: Neutral\|Accent\|Error` | 高 16px、radius_pill、min-width 16px；Error 红底白字 | `widgets/badge.rs` |
| `StatusDot` | 状态点 | `state: Idle\|Success\|Warning\|Error\|Loading` | 8px 圆点；Loading 呼吸脉冲（motion 800ms sine，animate_value_with_time + sin）；Error 常亮 error 色 | `widgets/status_dot.rs` |
| `Tooltip` | 统一提示 | 包装 egui `on_hover_text` + `on_hover_ui`；`delay`（默认 egui 0.4s） | 默认风格（bg_panel_alt + radius_sm + caption 字号） | `widgets/tooltip.rs` |
| `EmptyState` | 空态占位 | `icon`；`title`；`description`；`action: Option<Button>` | 居中：48px 图标 text_weak + heading 标题 + caption 描述；间距 lg | `widgets/empty_state.rs` |
| `Card` | 容器 | `title: Option`；`padding: Md\|Lg`；`bordered` | bg_panel + radius_md + border 1px；padding lg；标题行 heading + 可选折叠 | `widgets/card.rs` |
| `Divider` | 分隔线 | `vertical: bool`；`strong: bool` | 1px border / border_strong | `widgets/divider.rs` |
| `Label` | 文本层级 | `text`；`level: Primary\|Secondary\|Weak\|Disabled`；`size: Body\|Caption\|Heading` | 对应 ColorTokens + TypeTokens | `widgets/label.rs` |
| `PriceText` | 价格格式化 + 着色 | `price`；`change: Option<f32>`；`tone: Auto\|Up\|Down\|Flat` | mono 字体；Auto 依 change 正负着 up/down 色；`+1.23%` 前缀符号 | `widgets/price_text.rs` |
| `Segmented` | 分段选择器 | `options`；`selected`；`size` | 高 control_md；选中段 bg_panel + accent 文字 + border；未选中透明；整体 bg_panel_alt radius_sm | `widgets/segmented.rs` |
| `SectionTitle` | 面板标题行 | `text`；`count: Option<usize>`；`action: Option<IconButton>` | heading + text_secondary 计数 + 右对齐 action | `widgets/section_title.rs` |

### 5.2 复合组件（molecules）

| 组件 | 职责 | 组合来源 | 视觉规格 | 文件 |
|---|---|---|---|---|
| `SearchableDropdown`（迁移自 StockPicker） | 输入过滤 + 选项列表 + 键盘导航（↑↓/Enter/Esc） | Input + Dropdown | 输入框 control_md；popup 行 22px、选中 accent 竖条、hover bg_hover；空过滤 EmptyState 微版「无匹配结果」；Esc 关闭（保持现有行为） | `widgets/searchable_dropdown.rs` |
| `MultiSelect`（迁移自 screener multi_select_popup） | 多选下拉（搜索 + checkbox + 完成） | Dropdown + Checkbox + Input | popup min_width 220；选项行 checkbox + 文本；「完成」按钮 Primary sm；summary 按钮文案「已选 N 个」 | `widgets/multi_select.rs` |
| `Toast`（迁移自 toast.rs + 增强） | 等级通知 + 动画 + 堆叠 | Card + StatusDot + ProgressBar | 见 7 节动效规格；卡片 bg_panel + 左侧 3px 等级色条 + 图标 + caption/body 文本 + 底部 3px 进度条（现有逻辑保留）；宽度 280px；右上角 16px 锚定 | `widgets/toast.rs` |
| `Modal`（迁移自 modal.rs + 增强） | 阻塞确认/引导 + 动画 + 焦点 | Card + Button + Divider | 见 7 节；backdrop black 60% + shadow_modal；面板 min_width 360、radius_lg、padding xl；标题 heading + body caption/secondary；按钮右对齐：Cancel(Ghost) + Confirm(Primary/Danger) | `widgets/modal.rs` |
| `DataTable`（抽象自 screener 表格） | 可排序列 + 斑马纹 + 行 hover + 涨跌列 + 空态 + 计数 | egui_extras TableBuilder + PriceText + Badge | 表头 22px heading + 排序箭头；行 18px；hover bg_hover；斑马纹 bg_panel/bg_panel_alt 50% 交替；numeric 列 PriceText | `widgets/data_table.rs` |
| `Toolbar` | 分组工具栏容器 | Button/Input/Dropdown + Divider | 高 toolbar_h；组间 Divider(strong) + spacing_lg；组内 spacing_sm；背景 bg_panel_alt + 下边框 border | `widgets/toolbar.rs` |
| `Sidebar`（自选股，新） | 分组列表（自选/最近）+ 搜索 + 增删 | Input + SectionTitle + IconButton + Tag + StatusDot | 宽 sidebar_w；分组标题 caption text_weak；行高 28px hover bg_hover、选中左侧 2px accent 竖条；行：名称 body + 代码 mono caption + Tag(exchange) + hover IconButton(×) | `widgets/sidebar.rs` |
| `StatusBar`（新） | 三段式状态条 | Label + StatusDot + PriceText | 高 statusbar_h；左：标的摘要；中：状态；右：数据源 + 时钟（mono） | `widgets/status_bar.rs` |

### 5.3 业务组件（organisms，留在 bin）

| 组件 | 现状 | 变更 |
|---|---|---|
| `ChartCitizen` | citizens/chart.rs | 空态 EmptyState；symbol 回填；chart token 走新映射 |
| `LoggerPanel` | citizens/logger.rs | 面板头 SectionTitle + 导出 IconButton → FileDialog |
| `ScreenerPanel` | citizens/screener.rs | 表单 Card 分区 + MultiSelect/Checkbox/DragValue 组件化 + DataTable + 重置按钮（Modal） |
| `WatchlistPanel`（新） | — | 组合 Sidebar 组件 + SharedState.watchlist + 持久化 |

### 5.4 与 egui 原生 widget 的边界（明确划分）

| 层 | 决策 |
|---|---|
| **直接用 egui 原生** | `Label/RichText`（经 Label 包装）、`TextEdit`（经 Input 包装）、`DragValue`、`Slider`、`ScrollArea`、`Spinner`（经包装统一尺寸）、`ProgressBar`（toast 进度条内部使用）、`Area`（弹层底层） |
| **直接用 egui_extras** | `TableBuilder`/`Column`（DataTable 内部）、`Strip`（可选） |
| **直接保留的外部库** | egui-phosphor（图标）、egui-file-dialog、egui-charts（图表）、egui_dock（布局）、egui_lens（日志）、egui_mobius（状态/异步，非 UI） |
| **包装自建（外观统一 + 变体）** | Button / Checkbox / Dropdown / Input / Spinner / Tooltip（底层是 egui 原子，加 compass 变体与 token 接入） |
| **必须自建（egui 无对应）** | Toast / Modal / EmptyState / Tag / Badge / StatusDot / PriceText / Segmented / SearchableDropdown / MultiSelect / DataTable / Card / Divider / SectionTitle / Toolbar / Sidebar / StatusBar |

---

## 六、布局方案（v1 继承 + 调整）

### 6.1 全局骨架

```
┌─ Toolbar (40px, bg_panel_alt) ────────────────────────────────────────────────┐
│ [组A 标的] 🔍 Input(220px)   [组B 周期] Segmented[1d|1w|1M]  │  [组C 操作] [⬇ Fetch](Primary)  │  [组D 显示] ☰ IconButton  🎨 Dropdown │
├──────────┬────────────────────────────────────────────────────────────────────┤
│ Sidebar  │  egui_dock DockArea（深度定制 Style）                                 │
│ 240px    │  ┌─ tab 栏 28px（图表|日志|选股器）──────────────────────────────┐   │
│ 自选/最近 │  │  图表（K 线 + 空态引导）                                    │   │
│          │  ├─ 日志（SectionTitle + 导出）────────────────────────────────┤   │
│          │  └─ 选股器（条件表单 + DataTable）─────────────────────────────┘   │
├──────────┴────────────────────────────────────────────────────────────────────┤
│ StatusBar (26px)：● 标的摘要（mono 价格）   │  加载/错误状态   │  本地数据源 · 时钟  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

**egui_dock Style 深度定制值**（compass-ui `dock_style.rs` 提供 `dock_style(tokens)`）：

| 字段（egui_dock-0.20.1/src/style.rs） | 设定值 |
|---|---|
| `tab_bar.bg_fill` | `bg_panel` |
| `tab_bar.height` | 28.0（token tabbar_h） |
| `tab_bar.corner_radius` | {nw:md, ne:md, sw:0, se:0} |
| `tab_bar.hline_color` | `border` |
| `tab.active.bg_fill` | `bg_panel_alt` |
| `tab.active.text_color` | `accent`（选中强调） |
| `tab.inactive.text_color` | `text_secondary` |
| `tab.hovered.bg_fill` | `bg_hover` |
| `tab.hovered.text_color` | `text_primary` |
| `tab.spacing` | 2.0 |
| `tab.tab_body.bg_fill` | `bg_app`（内容区与图表背景统一） |
| `tab.tab_body.inner_margin` | 0（面板自行 padding） |
| `separator.color_idle / hovered / dragged` | `border` / `border_strong` / `accent` |
| `separator.width` | 1.0（extra_interact_width 保持 2.0） |
| `main_surface_border_stroke` | 1px `border` |
| `overlay.selection_color` | accent 50% alpha |
| 降级项 | tab 选中「下划线滑动动画」无法注入 painter（egui_dock 无绘制 hook）→ 用 active/hovered 背景+文字色过渡（egui animation_time 内建）替代 |

### 6.2 Sidebar（v1 继承，组件化）

- 同 v1 3.1：`SidePanel::left` 240px（resizable 200-320）、分组（自选/最近）、行点击切图表（复用 `sync_picker_from_symbol` + `dispatcher::handle`）、hover 删除（Modal 确认）、顶部搜索 + 添加按钮、`[watchlist]` config 持久化、空态。
- 变更：改用 compass-ui `Sidebar` 组件 + `Tag`（交易所徽标）+ `StatusDot`；行内容仅名称/代码/Tag（行情显示 v1 决策不变，待确认 Q4）。

### 6.3 StatusBar（v1 继承，组件化）

- 同 v1 3.2：`TopBottomPanel::bottom` 26px 三段式；左段标的摘要（`PriceText` mono 价格 + 涨跌着色）；中段状态（`StatusDot`：loading 脉冲 / error 红点 / 空闲隐藏）+ 文案；右段数据源信息（启动时 `load_stock_list` count）+ 时钟（mono，每秒刷新，200ms 重绘已覆盖）。
- 变更：改用 compass-ui `StatusBar` 组件 + `StatusDot`/`PriceText`/`Label`。

### 6.4 工具栏分组（v1 继承，组件化）

- 同 v1 3.3：四组（标的/周期/操作/显示），组间 Divider(strong) + spacing_lg，Fetch 为 `Button(Primary, Lg)` 主操作（loading 时禁用 + 内嵌 spinner + 「加载中…」）。
- 变更：TF 用 `Segmented[1d|1w|1M]`（替代 ComboBox，视觉更紧凑）；Theme 用 `Dropdown`；sidebar toggle 用 `IconButton`；整条工具栏用 compass-ui `Toolbar` 容器。
- 快捷键：`/` 聚焦 Symbol、`Ctrl+Enter` Fetch、`Ctrl+K` 聚焦 Sidebar 搜索（v1 保留）。

### 6.5 Modal 真实绑定（v1 继承）

- 场景 1（P1）启动数据缺失引导（load_stock_list 空 → 首帧 Modal，说明 + 「知道了」+ 可选「打开数据目录」）。
- 场景 2（P1）日志导出：Logger 面板「导出日志」→ FileDialog 选路径 → 写文本（egui_lens 导出能力接线，main.rs:164 file_dialog 激活）。
- 场景 3（P2）Sidebar 删除自选确认（Modal Danger 按钮「移除」）。
- Modal 组件增强：动画（7 节）、Danger 变体、焦点管理。

### 6.6 Screener 升级（v1 继承 + 组件化）

- 表单两行分区（基础/技术面）用 `Card` + `MultiSelect` + `Checkbox` + `DragValue`（原生）+ 分区标题。
- 表格改用 `DataTable`（表头排序/涨跌列 `PriceText`/空态/计数）。
- 「重置条件」按钮（Ghost）+ Modal 确认（可选纳入，待确认 Q10）。
- 间距统一 token（v1 的 `add_space(4.0)` 等替换）。

### 6.7 Chart 升级（v1 继承）

- 空态 `EmptyState`（「输入代码并点击 Fetch」）；`chart.set_symbol` 每帧与 `state.symbol` 同步（chart.rs 加一行，替换 "COMPASS" 占位）。

---

## 七、交互效果规格（v1 继承 + 组件级动效 token）

> 实现约束：egui 即时模式。过渡用 `ctx.animate_value_with_time` / `animate_bool_with_time_and_easing`（egui-0.35.0/src/context.rs:3112/3129 已确认），easing 来自 `emath::easing`；动画帧 `request_repaint()`，其余沿用 200ms 兜底重绘（main.rs:410）。

| # | 交互 | 触发 | 时长/缓动（token） | 目标状态 | 落地 |
|---|---|---|---|---|---|
| 1 | Toast 入场 | push 后下一帧 | 150ms cubic_out（base） | 右滑 x+16 → 原位 + alpha 0→1 | toast.rs |
| 2 | Toast 关闭 | 点击 / 到期 | 100ms linear（fast） | alpha→0 + 高度→0 再移除 | toast.rs |
| 3 | Modal 打开 | open() | backdrop 120ms linear；面板 150ms cubic_out（base） | backdrop alpha 0→1；面板 scale 0.95→1.0 + fade | modal.rs |
| 4 | Modal 关闭 | Cancel/OK | 100ms linear（fast） | fade 后置关闭（closing 状态机） | modal.rs |
| 5 | Tab 切换 | 点击 tab | 150ms（egui animation_time 内建过渡） | active bg_panel_alt + accent 文字（下划线动画降级，见 6.1） | dock_style.rs + tabs.rs |
| 6 | Sidebar 行 hover | 鼠标进入 | 100ms linear（fast） | bg → bg_hover；删除按钮 alpha 0→1 | sidebar.rs |
| 7 | Sidebar 行选中 | 点击 | 120ms linear | 左侧 2px accent 竖条 + bg_hover；同步 Chart + Picker | sidebar.rs + main.rs |
| 8 | StatusBar 状态点 | loading=true | 800ms 周期 sine（StatusDot） | 呼吸脉冲 alpha 0.4→1；error 常亮 | status_dot.rs |
| 9 | Fetch 按钮 loading | 点击 → loading | 120ms linear（fast） | Primary → 禁用 + 内嵌 spinner + 「加载中…」 | button.rs + main.rs |
| 10 | 主题切换 | Dropdown 选择 | 即时 + toast Info「主题已切换」 | 全局换肤（无过渡动画，见决策 D6） | main.rs |
| 11 | Dropdown/弹层 | 打开/关闭 | 100ms linear（fast） | fade + 轻微上移 | dropdown.rs/modal.rs |
| 12 | 主按钮 press | 按下 | egui 默认 | 压感（accent_pressed） | button.rs |
| 13 | Tooltip | hover | 0.4s 延迟（egui 默认） | 中文说明 | tooltip.rs |

**反馈状态全覆盖**：loading（工具栏 + StatusDot 脉冲 + toast）／error（toast 8s + StatusBar 红点 + chart 错误文案）／empty（Chart EmptyState + Sidebar 空态 + Screener「无符合条件」）／数据源缺失（启动 Modal + StatusBar「数据未就绪」）。

**快捷键**：`/` 聚焦 Symbol、`Ctrl+Enter` Fetch、`Ctrl+K` Sidebar 搜索、`Esc` 关闭弹层/modal（现有行为保留）、`1/2/3` 切周期（v1 全部保留）。

---

## 八、落地映射汇总

| 改动 | 文件 | 类型 |
|---|---|---|
| **新建组件库 crate** | `crates/compass-ui/Cargo.toml` + `src/{lib.rs, fonts.rs, dock_style.rs}` | 新增 |
| token 六类 | `crates/compass-ui/src/tokens/{color,spacing,typography,radius,shadow,motion}.rs` | 新增 |
| 主题映射（Visuals 直接构造 + chart 薄封装） | `crates/compass-ui/src/theme/{mod,apply}.rs` | 新增 |
| 基础组件 16 个 | `crates/compass-ui/src/widgets/{button,icon_button,input,dropdown,checkbox,tag,badge,status_dot,tooltip,empty_state,card,divider,label,price_text,segmented,section_title}.rs` | 新增 |
| 复合组件 8 个 | `crates/compass-ui/src/widgets/{searchable_dropdown,multi_select,toast,modal,data_table,toolbar,sidebar,status_bar}.rs` | 新增（3 个迁移） |
| bin theme.rs 迁移/重写 | `crates/compass/src/theme.rs` → compass-ui re-export | 重写 |
| 字体注册 | `crates/compass/src/main.rs:32-55` → compass-ui `fonts.rs` | 迁移 |
| 布局接线 | `crates/compass/src/main.rs:339-412`（SidePanel/TopBottomPanel） | 修改 |
| 工具栏 | `crates/compass/src/main.rs:443-537` | 重构 |
| watchlist 持久化 + 启动 Modal | `crates/compass/src/main.rs` | 新增 |
| dock_style | `crates/compass/src/main.rs:141` → `compass-ui::dock_style()` | 修改 |
| Tab 中文标题 + 图标 | `crates/compass/src/tabs.rs` | 修改 |
| Chart 空态 + symbol 回填 | `crates/compass/src/citizens/chart.rs` | 修改 |
| Logger 导出 | `crates/compass/src/citizens/logger.rs` + `main.rs:391` | 修改 |
| Screener 组件化 | `crates/compass/src/citizens/screener.rs` | 重构 |
| SharedState watchlist | `crates/compass/src/state.rs` | 修改 |
| WatchlistConfig | `crates/compass-core/src/model.rs` | 修改 |
| workspace 挂载 | `Cargo.toml`（workspace members + 版本表） | 修改 |
| 测试迁移 + 新增 | compass-ui dev（kittest 组件测试）；bin 测试同步 | 修改/新增 |
| 文档同步 | `kb/user/gui.md`、`kb/user/config.md`、`kb/design/architecture.md`（crate 图） | 修改（docs skill） |

---

## 九、待确认（开放问题）

- **Q1**：~~egui-charts fork（qiboda compass 分支）的 `ChartConfig` 字段公开度~~ —— **已解决（2026-08-01 评审验证）**：fork `ChartConfig`（config/chart.rs:146）字段全 pub（bullish_color/bearish_color/background_color 等 15+），`apply_to_config`（theme/mod.rs:347）存在，`ChartSemanticTokens` 与官方基版结构一致（semantic.rs:184）——**直接构造 config 与保留 apply_to_config 薄封装双路均可行，candle 红涨绿跌可覆盖**。实现时选择直接构造（更符合 theme 自主化目标）。
- **Q2**：Tab 标题中文化（图表/日志/选股器）+ 图标（CHART_LINE/TERMINAL/FUNNEL_SIMPLE）是否采纳。——建议采纳。
- **Q3**：自选股持久化 `config.toml [watchlist]` 是否接受（备选：Dolt 表成本高；纯内存重启丢失）。——建议 config.toml。
- **Q4**：Sidebar 行是否显示最新价/涨跌幅（需逐行 Parquet 查询）。——v1 建议不显示，维持。
- **Q5**：200ms 常驻重绘（main.rs:410）接受还是优化按需。——建议接受（动画/时钟都需要）。
- **Q6**：字体内嵌。**体积实测（2026-08-01）**：两字重全内嵌 +17.3MB（Regular 8.4MB + Bold 8.6MB + Mono 0.27MB）。选项：① 全内嵌（+17.3MB，最强可移植性）；② 仅内嵌 Regular + Bold 系统 fallback（+8.7MB，标题粗体依赖系统字体）；③ 全部系统路径（现状，main.rs:33 硬编码脆弱）。——建议 ①（金融终端便携优先，体积可接受）。
- **Q7**：等宽字体选型：JetBrains Mono（推荐）/ IBM Plex Mono / Roboto Mono（均 OFL）。
- **Q8**：窗口默认 1440×900（Sidebar+StatusBar 压缩中央区后留足图表空间）？
- **Q9**：补齐第三主题 `compass_blue`（Midnight 蓝调，与 accent #2962FF 呼应）？v1 遗留（theme.rs 文档提三套、代码两套）。
- **Q10**：Screener「重置条件」按钮 + 确认 Modal 纳入本 epic？
- **Q11**：启动引导 Modal 的「打开数据目录」按钮是否引 `opener` crate（registry 已有 0.8.5）？——建议可选，默认只留「知道了」。
- **Q12**：compass-ui 迁移范围确认：现有 3 个 widgets（toast/modal/searchable_dropdown）随迁并重写（推荐，bin 测试同步量 ~40 个断言）；若用户希望最小化迁移量，可先只迁组件库新增部分、旧组件后续迭代迁移。
- **Q13**：`hline_below_active_tab_name` 是否开启（选中 tab 底部 2px accent 线，egui_dock 原生支持，替代自定义下划线动画）？——建议开启。

---

## 十、决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| D1：Dock 方案 | 深度定制 egui_dock 0.20 / 换 egui_tiles / 自建 | 深度定制 egui_dock 0.20 | Style 证据充分（style.rs:52-352 全字段可配：TabBarStyle/TabStyle 7 态/分隔线/边框）；现有 tabs.rs/dock_state/kittest 全复用；需求固定三面板无需网格 | egui_tiles 自述"开发早期、功能不全"（README）；重写 TabViewer+测试成本高，收益非需求；自建拖拽/重排/浮动成本极高 |
| D2：Theme 架构 | 自建 token→映射 egui Visuals+chart 薄封装 / 全自建含 chart 渲染 / 保留现状 | 自建 token→映射 | `Visuals` pub 字段可直接构造（egui-0.35.0/style.rs:985）——UI 主题完全自主，消除「UI 由图表库决定」反向依赖；chart 渲染内部深度依赖 egui-charts Theme，薄封装成本边界最优 | 全自建 chart 渲染需重写 K 线/十字准线绘制，成本极高收益低；保留现状则 UI token 无法独立演进 |
| D3：组件库组织 | 独立 crate `compass-ui` / `widgets/` 目录扩展 | 独立 crate | 用户核心需求即「通用可复用组件库」；workspace 多 crate 模式成熟；独立测试面；依赖方向单向（bin→ui） | widgets/ 扩展使组件与业务同 crate 耦合，无法 bin 外复用，目录膨胀边界模糊 |
| D4：字体 | 思源+JetBrains Mono 内嵌 / 仅系统路径 / 思源等宽 | 思源(中) + JetBrains Mono(数字) 内嵌优先+fallback | 数字等宽是金融终端列对齐硬需求；egui 无 tabular-nums（已验证），等宽家族是等价替代；内嵌消除 main.rs:33 硬编码路径脆弱性 | 仅系统路径在无该字体机器上中文 tofu；思源等宽数字字形一般，非最优 |
| D5：组件 vs 依赖 | 自建为主 / 引 egui-notify、egui-modal 等 | 自建为主（仅保留 phosphor/file-dialog/extras/charts/dock） | 用户明确要求自建复用；现有 toast/modal 已是自建雏形；引库风格不统一、重复 | 引库与「自建为主」冲突；file-dialog 是平台级能力自建成本极高，保留 |
| D6：主题切换动画 | 逐色插值 / 即时切换+toast | 即时切换+toast | egui 主题为整树替换，逐色插值需自定义渲染层成本高收益低（v1 决策延续） | 过渡动画在即时模式性价比差 |
| D7：Tab 选中指示 | 自绘下划线动画 / egui_dock 原生能力（hline_below_active_tab_name + 状态色过渡） | 原生能力 | egui_dock 无 painter 注入点，自绘需 fork；`hline_below_active_tab_name` + TabInteractionStyle 覆盖 7 交互态已满足视觉 | 自绘动画与「保留 egui_dock」冲突，fork 维护成本高 |
| D8：布局 | 三栏式（Sidebar+Dock+StatusBar）/ 浮动窗口网格 | 三栏式 | 需求固定面板 + 拖拽调整；egui_dock overlay 已支持拖 tab 出窗（内建浮动能力） | 浮动网格非需求且引入复杂度 |
| D9：Modal 绑定场景 | 保持占位 / 仅启动引导 / 启动引导+日志导出+删除确认 | 三个场景（P1 两个 + P2 一个） | 零新依赖、真实高频；激活闲置 file_dialog（main.rs:164） | 保持占位违背 epic 目标 |
| D10：涨跌色 | A 股红涨绿跌 / TV 绿涨红跌 / 默认 | A 股红涨绿跌（#EF5350/#26A69A） | A 股用户心智（同花顺/东财一致）；token 统一 K 线与文本（依赖 Q1 fork 可配置性） | TV 惯例违背 A 股直觉 |
| D11：组件分类 | 基础/复合/业务三级 / 平铺 | 三级 | 层级清晰：atoms 无业务依赖、molecules 组合复用、organisms 留 bin 与 citizen 模式契合 | 平铺导致业务组件混入通用库，复用边界模糊 |
| D12：Screener 表格 | 迁移为 DataTable 组件 / 就地精修 | 迁移为 DataTable | 表格是最高复用价值组件（未来行情/自选列表都用到）；抽象后 screener 瘦身 | 就地精修仅解眼前，无法跨模块复用 |
