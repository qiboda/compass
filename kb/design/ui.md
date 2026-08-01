# UI 设计（权威文档）

**本文件是 compass GUI 的最终权威 UI 设计文档**，累积式维护 —— 每次设计
（compass-workflow 门禁第 1 步 DESIGN）经用户确认后，将最终设计要点同步至此。

> **归档与权威的区别**：`.omo/designs/<feature>.md` 是 ui-designer 产出的
> **过程归档**（原始方案）；`.omo/plans/<feature>.md` 是计划归档。
> 本文件才是 UI 设计的**最终版本**，与代码保持同步。归档文件不删不改，
> 但一切 UI 设计决策以本文件为准。

## 设计系统

### 主题预设

| 预设 | 描述 | 状态 |
|---|---|---|
| `compass_dark` | 默认暗色主题（TradingView 风格） | 已实现 |
| `compass_light` | 亮色主题，适合白天使用 | 已实现 |
| `compass_blue` | 深蓝主题 | 计划中（未实现；见 `.omo/plans/gui-beautify.md`） |

主题持久化到 `~/.config/compass/config.toml` 的 `[app].theme`。

### 字体

- **SourceHanSansCN**（思源黑体）—— 中文字体，同时注册 egui-phosphor Regular 图标字体
- 字号层级：随 egui 默认，未定制

### 图标

- **egui-phosphor**（Regular variant）—— 工具栏按钮、toast 通知、状态提示
- 约定：图标 + 文字标签并用（不单独用纯图标，除非空间受限）

### 颜色

颜色全部由 `theme.rs` 中的 `CompassTheme` 预设派生（封装 egui-charts 的
`Theme` 系统），不在 UI 代码中硬编码颜色值。

## 布局结构

```
┌────────────────────────────────────────────────────────┐
│ 工具栏（egui::Frame 全宽填充）                          │
│  [Symbol🔍] [TF⏱] [Fetch⬇] ... [Theme🎨]              │
├────────────────────────────────────────────────────────┤
│ DockArea（egui_dock，可拖拽/关/开标签页）                │
│  ┌───────────┬──────────────────────┐                  │
│  │ Chart     │  (可并排其他面板)      │                  │
│  │ (主区域)   │                      │                  │
│  └───────────┴──────────────────────┘                  │
│  标签页: Chart / Logger / Screener                     │
├────────────────────────────────────────────────────────┤
│ Toast 通知层（右上角，egui::Area 浮层）                  │
│ Modal 遮罩层（全屏，egui::Area + Order::Foreground）     │
└────────────────────────────────────────────────────────┘
```

- **工具栏**：全宽 `egui::Frame` 填充（非 `TopBottomPanel`），一行排列所有控件
  （Symbol 搜索下拉 / TF 切换 / Fetch 按钮 / Theme 下拉）
- **Dock 区**：egui_dock `DockState`，默认 Chart + Logger 垂直分割
  （Chart 75% / Logger 25%）；Screener 默认含于标签栏，可关闭
- **浮层**：Toast（右上角叠放，队列上限 10 条）、Modal（全屏半透明遮罩）
  —— 渲染在 DockArea 之上，随每帧渲染

### 面板职责

| 面板 | CitizenId | 职责 |
|---|---|---|
| Chart | `chart` | K 线图表（平移/缩放/十字准线），默认 100 根可见柱 |
| Logger | `logger` | 可滚动日志面板（tracing 事件） |
| Screener | `screener` | 条件选股（左侧条件 + 右侧结果表格） |

## 交互规范

### 工具栏

- Symbol：可搜索输入框，`交易所 | 代码 | 名称` 格式，弹窗列匹配项
- TF：`1d` / `1w` / `1M` 组合框
- Fetch：从本地 Parquet 加载图表数据
- Theme：下拉切换主题，即时全局生效

### 图表（Chart）

- **平移**：点击水平拖拽
- **缩放**：鼠标滚轮
- **十字准线**：悬停 K 线显示 OHLCV 详情
- 数据源：本地 DuckDB `read_parquet()`，**无在线回退**

### 反馈状态

| 类型 | 表现 | 自动消失 |
|---|---|---|
| Loading | 工具栏 spinner | 加载完成 |
| Success toast | 右上角 ✅ | 3 秒 |
| Warning toast | 右上角 ⚠ | 3 秒 |
| Error toast | 右上角 ❌ | 8 秒 |
| Info toast | 右上角 ℹ | 3 秒 |

toast 使用 Phosphor 图标字形，垂直堆叠，队列上限 10 条（超出淘汰最旧）。
点击关闭为预留能力（当前未绑定交互）。

### 模态（Modal）

- 全屏半透明遮罩 + 居中面板，`egui::Area` + `Order::Foreground`，
  背板以 `Sense::click()` 吞掉点击（非 `modal=true`——egui::Area 无原生焦点锁定）
- 当前组件已接线但**未绑定任何操作**（破坏性操作确认等为预留能力）

## 设计变更记录

| 日期 | 变更 | 来源归档 | 实现状态 |
|---|---|---|---|
| 2026-08-02 | 初始骨架：基于现有 GUI 提炼设计系统/布局/交互（ref #129） | — | 已实现（与代码同步） |

> 每次 DESIGN 门禁完成后，在此追加一行：日期、变更摘要、对应
> `.omo/designs/<feature>.md` 归档文件、实现状态。

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| UI 设计权威文档位置 | `kb/design/ui.md`（独立文件） / 并入 `kb/user/gui.md` / 每 feature 独立文档 | 新建 `kb/design/ui.md` | 与 kb/user/gui.md（用户手册）职责分离；单一累积式文档满足"一份最终文档"诉求；与 architecture/data-providers/symbols 并列于 kb/design/ | 并入用户手册会混淆设计规范与使用说明；多份独立文档无法形成单一权威版本（ref #129，用户确认） |
| `.omo/designs/` 的定位 | 过程归档 / 权威文档 | 过程归档 | 项目书（kb/）是知识的唯一数据源；设计经用户确认后最终版必须同步到 kb/ | 让归档文件承载权威信息会导致 kb/ 与归档内容漂移（ref #129） |
