# 设计文档 — Blender 式编辑器架构重构（issue #357）

> 本文档为 `subagent_ui_designer` 产出，属 **过程归档**（`.dsh/designs/`）；
> 经用户确认后，最终设计要点以 `.dsh/kb/design/ui.md` 为准（AGENTS.md 权威文档约定）。
> 所有设计均遵守 `.dsh/plans/handoff.md` 中锁定的 8 条 grill-me 决策，无违背。
>
> **修订记录（v2，2026-09-05）**：用户对整体方案 + Q1-Q6 + P1 全部仲裁确认；
> 因仲裁改动的段落均以 `> **【2026-09-05 用户仲裁更新】**` 标注。8 条锁定决策内容未改动。
> **修订记录（v3，2026-09-05）**：阶段 2b 代码评审（subagent 70ebfccf，commit df44802）
> 发现两处设计契约悬空（实现忠实执行设计但设计层矛盾），已裁决并勘误，涉及段落以
> `> **【2026-09-05 2b 评审勘误】**` 标注：① §6 Screener 行「列宽重置」暂缓（DataTable
> 无列宽调整能力）；② Screener sidebar 宽度与 Chart 参数解耦，专用 500.0/(486.0..=640.0)。
> **修订记录（v3，2026-09-05 追加）**：③ §6 SEPA 行右端 `⋮` 菜单内容裁决（2c 评审）——
> 含「重置排序」一项（不渲染 ⋮ 候选被否，理由见 §13 决策记录新增行；plan §4.3 相应文本建议同步）。
> **修订记录（v4，2026-09-05）**：④ §6 Market 行右端 `⋮` 内容裁决 + ⑤ §6 Screener 行 ⋮ 补
> 「重置排序」（委托方 two-menu 裁决，同日 2b/2c 后续）——两项均与 SEPA 2c 裁决同构
> （表头排序持久生效 → 恢复构造业务默认 → 动作幂等）；Screener 委托背景一处事实勘误
> （`new()` 实则调用 `set_sort(MARKET_CAP_COLUMN, true)`，见 §6 新注记与 §13 新增行），
> 以代码为准；§13 决策记录行数 18 → 20。

---

## 1. 目标

将 Compass 的 GUI 从「一个全局 Toolbar + 一个全局左栏 + 5 个各自为政的面板 tab」
重构为 **Blender 式五层编辑器架构**（Application → Workspace → Screen → Areas → Editor）：
- 布局具备**角色化结构**：每个编辑器（Editor）声明自己的 Header/Sidebar/Toolbar 组件集，
  新增编辑器按角色注册，而非复制一套 chrome 代码。
- 布局具备**任务化组织**：三个内置 Workspace（图表/选股/SEPA 复盘），顶部切换，
  每 Workspace 一个 Screen（DockState 树），布局可持久化、可恢复。
- 布局具备**可扩展性**：`layouts: Vec<ScreenLayout>` 为多屏幕预留数据结构，
  EditorKind 注册表 + 实例状态分离为「同一编辑器多实例」预留语义升级路径。

非目标（本轮）：多屏幕 UI、浮窗、编辑器多实例 UI、键位可重绑定系统、自研分区引擎。

## 2. 现状分析：结构性摩擦点（逐项引用代码）

| # | 摩擦点 | 具体表现 | 代码位置 |
|---|---|---|---|
| F1 | **全局 Toolbar 与 Chart 编辑器耦合** | 周期/复权/Fetch 只对图表有意义，却常驻渲染于所有面板激活态；用户在「选股器」tab 激活时看到的周期/复权控件是无效信息 | `crates/compass/src/main.rs:1395-1486`（`render_toolbar` 四组：标的/周期+复权/Fetch/显示），`main.rs:898-900`（`Panel::top("toolbar")`） |
| F2 | **面板 chrome 各自为政** | 每个 citizen 自绘自己的标题行/工具条：Logger 自绘 `SectionTitle`+导出按钮（`logger.rs:45-60`）；SEPA 自绘「② 工具条」（计数+TOP-N+刷新）；Market 自绘「② 工具条」（计数+Segmented+刷新）；Screener 自绘条件区+运行按钮；没有统一的 header/sidebar 声明模型，无 N 键入口，样式无法一站式定制 | `citizens/logger.rs:45-60`、`citizens/sepa.rs`（工具条区）、`citizens/market.rs`（工具条区）、`citizens/screener.rs`（条件区+DataTable） |
| F3 | **自选股全局左栏绑死在 Application 层** | 左侧 `Panel::left` 由 `sidebar_visible: bool` 全局布尔控制（`main.rs:903-911`、字段 `main.rs:856-857`、工具栏开关 `main.rs:1459-1466`）；自选股不能入 dock、不能进其他 workspace、只能出现在「图表语境」 | `main.rs:903-911`、`main.rs:1220-1270`（`render_sidebar`） |
| F4 | **布局不可持久化** | `DockState` 从不序列化；启动时硬编码重建（Chart/Market/Sepa 顶 leaf、Logger 0.75 split、Screener 0.5 split）；用户拖拽布局每次启动丢失 | `main.rs:154-172`；面板宽度硬编码：sidebar `main.rs:904-907`、SEPA 详情 `citizens/sepa.rs:35`（`DETAIL_PANEL_WIDTH: f32 = 280.0`） |
| F5 | **TabViewer 巨型借用结构** | 每帧构造 16 字段 `TabViewer`（`tabs.rs:206-230`），`ui()` 里 6 处 match 分发 + `main.rs:921-945` 内联装配——新增编辑器要同时改 TabKind/TabViewer/注册表/main.rs 构造/测试锚点 6+ 处 | `tabs.rs:206-239`、`main.rs:921-945` |
| F6 | **TabKind↔CitizenId 1:1 硬耦合** | `TabKind::citizen_id()` 每个变体绑定固定 `CitizenId`（`tabs.rs:101-109`），使「dock tab」与「citizen 激活追踪」完全绑定，无法表达「同一类型多处出现」 | `tabs.rs:101-109`、`dispatcher.rs:30-46`（`register_citizens` 5 个硬编码 ID） |
| F7 | **快捷键是全局的不是上下文敏感的** | `1/2/3` 切周期、`/` 聚焦标的输入框在任意 tab/面板激活时都生效（文本焦点守卫之外无 context 判定）；在选股 workspace 按 `1/2/3` 无意义 | `main.rs:1083-1117`（`handle_shortcuts`） |
| F8 | **每 leaf 单 tab 的隐性假设** | dock 样式注释明确「每 leaf 单 tab 结构下所有 tab 都是 active」（`dock_style.rs:36-42`），以 focused_leaf 区分高亮；这与「同一 area 多编辑器」的 Blender 心智有张力（本轮不解决，样式保留） | `compass-ui/src/dock_style.rs:36-42` |
| F9 | **SharedState 平面膨胀** | 20 个 `Dynamic<T>` 字段（`state.rs:11-66`），各编辑器专属状态（`screener_*`/`sepa_*`/`index_*`/`llm_*`/`watchlist`/`industry_names`）无归属边界，未来按编辑器拆分状态无处安放 | `state.rs:11-66`、`state.rs:74-101` |

**扩展受阻场景**：新增一个「财务」编辑器需要 TabKind 变体 → title/icon/citizen_id 三方法 → TabViewer 字段+match 分支 → `register_citizens` → `main.rs` 构造+字段+初始布局 → 测试锚点，共 6 文件 10+ 处，且必须硬塞进既有某个 workspace/leaf——正是「面板各自为政」的结构性根因（F5/F6）。

## 3. Blender 五层映射表

| Blender 概念（源码） | Compass 对应物（类型） | 说明与差异 |
|---|---|---|
| **Application**（`wmWindowManager` + 进程级） | `CompassApp`（eframe::App，`main.rs:815-876`）+ 应用级状态容器（主题/语言/时钟/配置） | 相同点：进程级单例、持有全局配置。差异：Compass 不引入窗口级侧栏/工具条（Blender 源码证实 global areas 只有 topbar/statusbar → 锁定决策 4） |
| **global areas**（`wmWindow` 的 Topbar/Statusbar，不在 bScreen areas 内） | Topbar（`Panel::top`，40px：workspace 切换条 + 标的选择器 + 主题/语言）+ StatusBar（`Panel::bottom`，26px：摘要/状态/时钟） | 直接对应：global areas 独立于 Screen/DockState 渲染，不随 workspace 切换重建 |
| **Workspace**（`WorkSpace`，含 screens 链 + `WorkSpaceType` 注册表） | `Workspace { id, layouts: Vec<ScreenLayout>, active_screen }` + `WorkspaceId::Chart/Screener/Sepa` | 对应 `wm_workspace.h` 的 workspace 持 screen 列表；差异：Blender workspace 可有多个 screen 且窗口级切换，Compass v1 每 workspace 仅 1 screen（`Vec` 预留） |
| **Screen**（`bScreen` —— area 顶点图，`ScreenListBase`） | `ScreenLayout { dock_state: DockState<Tab> }` | 同构点：`DockState` 树 = bScreen 的矩形分割顶点图（锁定决策 1 的原话）。差异：egui_dock 无浮窗/多窗口；`DockState` 已内建 serde feature（0.20.1 `dock_state/mod.rs:44` 有 `cfg_attr(feature="serde", derive(Serialize, Deserialize))`），是本设计持久化的关键事实 |
| **Area**（`ScrArea`，矩形区域 + `SpaceLink *spacedata`） | egui_dock leaf（单个 tab 容器）+ active tab | 同构：leaf 持有「编辑器列表」，active tab 即当前 SpaceType；差异：Blender area 是绝对矩形+subwindow，egui_dock leaf 是 tab 容器（若 leaf 内多 tab，语义与 ScrArea 的 spacedata 链表完全对应） |
| **SpaceType**（`spacetypes.h` 全局注册表：spaceid、`new()`、`convert()`、区注册） | `EditorDescriptor` 静态注册表（`EDITOR_REGISTRY: [EditorDescriptor; N]`） | 对应注册表条目：name/id、布局角色（layout）、标题/图标。差异：Rust 静态表替代 C 全局链；`convert()`（编辑器切换时保留数据）由「每 Kind 单实例全局容器」近似承担（见 §5 实例所有权） |
| **SpaceData**（`SpaceView3D`/`SpaceOutliner` 等 per-instance 状态根） | 编辑器实例结构体（`ChartCitizen`/`ScreenerPanel`/`SepaPanel`/`MarketPanel`/`LoggerPanel`/新 `WatchlistEditor`） | 直接对应：每编辑器实例自带状态（chart 的 `IndicatorRegistry`+`cache_key`（`chart.rs:25-49`）、screener 的 `builder_root`（`screener.rs:96-98`）、sepa 的选择/TOP-N）。**差异**：Compass 每 Kind 单实例（现状），非「每 area 一实例」 |
| **ARegion**（`screen.h`：header/panel/toolbar 子矩形，`R_TYPE_*`） | `EditorLayout` 声明的三个槽位：`Header`（必备）/`Sidebar`/`Toolbar`（可选） | 对应 head=Header、panel=Sidebar、toolbar=Toolbar 区域；差异：Blender header 可上/下，Compass v1 恒顶部；Blender region 是 area 内绝对矩形，Compass 用 egui 布局流 |
| **keymap + operator**（WM keymap：`N`→`screen.region_toggle` 的上下文绑定） | 集中式 `ShortcutRouter`（`ctx.input` 轮询 + 焦点守卫 + 活跃编辑器判定） | 差异（重要）：Blender 有完整 keymap/context 系统（按 active area/region/mode 解析按键）；egui **无全局按键监听**，只能每帧 `ctx.input(|i| i.key_pressed(..))` 轮询（现有 `handle_shortcuts` 即此机制）。本项目不引入 keymap DSL，用「集中路由 + 上下文判定」近似（见 §8） |

## 4. Rust 类型/模块方案（类型草案，非实现）

### 4.1 EditorKind + EditorLayout（Editor 注册表，SpaceType 类比）

```rust
/// 编辑器类型标识（serde 用 snake_case 字符串：`"chart"`、`"watchlist"`…，持久化稳定）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorKind { Chart, Screener, Sepa, Market, Logger, Watchlist }

impl EditorKind {
    pub fn title_key(&self) -> &'static str { /* "editor.chart" / "editor.watchlist" … */ }
    pub fn icon(&self) -> &'static str     { /* CHART_LINE / FUNNEL_SIMPLE / GAUGE / TREND_UP / TERMINAL / LIST_STAR */ }
}

/// 组件规范：Header 必备；Sidebar/Toolbar 按角色注册（锁定决策 1/7）。
pub struct EditorLayout {
    pub header:  HeaderLayout,            // 恒有：默认高 32px，bg_panel_alt，右端 Display Options 槽
    pub sidebar: Option<SidebarLayout>,   // None = 无侧栏（N 键无操作）
    pub toolbar: Option<ToolbarLayout>,   // None = 空槽：不分配区域、零渲染成本（Blender 源码证明 Outliner 无 toolbar）
}
pub struct SidebarLayout { pub default_visible: bool, pub default_width: f32, pub width_range: (f32, f32) }
// Chart 推荐默认：default_width 240.0、width_range 200.0..=320.0（对齐现状 main.rs:905-906）
// Screener 专用（2026-09-05 2b 评审勘误裁决）：default_width 500.0、width_range (486.0, 640.0)
// 注：SidebarLayout 值按编辑器独立配置（Chart/Screener 各自指定）；Watchlist 不是 Sidebar，不受此规则约束（Q6 定案，见 §5.1）。

/// 注册表条目（SpaceType 类比）：静态不可变
pub struct EditorDescriptor {
    pub kind: EditorKind,
    pub title_key: &'static str,
    pub icon: &'static str,
    pub layout: EditorLayout,
}

/// 静态注册表：新增编辑器 = 加一个 EditorKind 变体 + 一个 descriptor（编译期穷尽保证）
pub static EDITOR_REGISTRY: [EditorDescriptor; 6] = [ /* chart, screener, sepa, market, logger, watchlist */ ];
```

> **【2026-09-05 2b 评审勘误】（裁决 2：Screener sidebar 专用宽度）**：`SidebarLayout` 改为
> **每编辑器独立配置**，不再有统一推荐默认值。Chart 保持 `default_width 240.0 /
> width_range (200.0, 320.0)`（指标参数控件窄，与现状 `main.rs:905-906` 对齐）；
> **Screener 专用 `default_width 500.0 / width_range (486.0, 640.0)`**。推导：条件卡片为
> 原子组（ref #220：label+control 永不跨行），最宽叶片 = Momentum 估宽 **470px**
> （`leaf_row_min_width`，`screener.rs:1272-1285`），加 Sidebar Panel 内边距 8×2=16px
> （egui 0.35 `Frame::side_top_panel` → `Margin::symmetric(8, 2)`）得下限 **486px**；
> 默认取 **500px**（内容区 484px ≥ 470px，且恰为 `GROUP_ALIGNMENT_WIDTHS` 首个测试锚
> 500px，`screener.rs:1882`，可直接复用既有 kittest 覆盖）；上限 **640px** 为余量选择：
> 1440px 默认窗口下结果表仍余 ≥800px（6 列 DataTable 可用），给用户放大构建器的空间。
> 原 240/(200,320) 系从 Chart
> 继承：内容区仅 224px，最窄叶片卡片（Unknown 300px）也无法容纳，控件必然横向溢出被
> Panel clip——即 2b 评审发现的矛盾根因。实现注意：EDITOR_REGISTRY Screener 条目
> （`editor/mod.rs:156-160`）与契约测试 `chart_and_screener_sidebar_spec_follows_contract`
> （`editor/mod.rs:611-624`，现断言 Chart/Screener 同为 240/(200,320)）须同步拆分。

### 4.2 EditorView + EditorCtx（实例层，SpaceData 类比）

```rust
/// 编辑器实现 trait —— 替代 TabViewer 16 字段借用结构（F5 的解药）。
/// 现 5 个 citizen 的 `show()` 按阶段 2 拆分为 header/sidebar/body 三方法；
/// body 必选，header/sidebar 按 EditorLayout 注册位渲染（空实现零成本）。
pub trait EditorView {
    fn kind(&self) -> EditorKind;
    fn header(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {}  // 默认空（仍渲染 Header 容器 + Display Options）
    fn body(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>);
    fn sidebar(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {} // 仅 layout.sidebar.is_some() 时被调
}

/// 编辑器上下文：把 TabViewer 的 16 个 &mut 字段收拢为一个可克隆引用束
pub struct EditorCtx<'a> {
    pub state: &'a SharedState,
    pub theme: &'a CompassTheme,
    pub signals: &'a EditorSignals,       // work/screener/sepa/index/llm 五个 Signal 引用束
    pub toasts: &'a mut ToastManager,
    pub request_repaint: ...,
    // screener_industries / screener_boards / logger_export_clicked 等按需并入
}
```

### 4.3 EditorFrame（运行时 chrome 包装，ARegion 类比）

```rust
/// 按 EditorLayout 包一层 chrome：Header 条 + (Sidebar | body) 分栏。
/// 统一处理 N 键显隐、宽度拖动、Header 高度、Display Options 槽位；
/// 编辑器自己的 show() 不再自绘标题行（F2 的解药）。
pub struct EditorFrame {
    pub sidebar_visible: bool,            // 初始 = layout.sidebar.default_visible
}
impl EditorFrame {
    pub fn show(&mut self, ui: &mut egui::Ui, desc: &EditorDescriptor,
                editor: &mut impl EditorView, ctx: &mut EditorCtx<'_>) { /* … */ }
}
```

### 4.4 Workspace / ScreenLayout（WorkSpace / bScreen 类比）

```rust
#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceId { Chart, Screener, Sepa }   // 持久化字符串："chart" 等

/// bScreen 类比：一个屏幕 = 一棵 Area 树（egui_dock 顶点图）
#[derive(Serialize, Deserialize)]
pub struct ScreenLayout {
    pub dock_state: DockState<Tab>,       // Tab = { kind: EditorKind }（serde 已具备，见 §9）
    #[serde(default)] pub active_tab: Option<TabPath>,  // 恢复时聚焦的 tab（Kittest 既有 set_active_tab 先例）
}

/// WorkSpace 类比：一个任务 workspace，持有一个或多个屏幕布局
#[derive(Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    #[serde(default)] pub layouts: Vec<ScreenLayout>,  // 锁定决策 3：预留多屏幕；初始恰 1 个
    #[serde(default)] pub active_screen: usize,
}

/// 应用级（window manager 类比）
pub struct Workspaces {
    pub all: Vec<Workspace>,              // v1 恒 3 个（Chart/Screener/Sepa）
    pub active: usize,
}
impl Workspaces {
    /// 切换：把当前 workspace 的 DockState 就地保存（即时保存触发点之一），
    /// 换 active；编辑器实例状态全局共享，切换无状态丢失（§4.5 实例所有权）
    pub fn switch(&mut self, target: WorkspaceId) { /* … */ }
    pub fn default_layout(id: WorkspaceId) -> DockState<Tab> { /* 抽出 main.rs:154-172 改为按 id */ }
}
```

### 4.5 Editor 实例子系统（ScrArea + SpaceType + SpaceData 类比的关键决策）

```rust
/// 每 EditorKind 单实例全局容器（决策 B，见 §11 决策记录 #5）。
/// 现状的 CompassApp 五 citizen 字段（main.rs:818-822）收拢为这一类，
/// 通过 EditorKind → &mut dyn EditorView 分发；Tab 仍是轻量 Copy 值，不含实例句柄。
pub struct EditorInstances {
    pub chart:    ChartCitizen,
    pub screener: ScreenerPanel,
    pub sepa:     SepaPanel,
    pub market:   MarketPanel,
    pub logger:   LoggerPanel,
    pub watchlist: WatchlistEditor,       // 新：Outliner 式（§6）
}
impl EditorInstances {
    /// 按 kind 返回可变引用（内部 match）—— TabViewer::ui 的分发点，替代 6 处 match 散落
    pub fn get_mut(&mut self, kind: EditorKind) -> &mut dyn EditorView;
}
```

**实例所有权说明**（Blender 差异记录）：Blender 的 SpaceType 允许多 area 多实例（各持 SpaceData）。
本方案 v1 维持「每 Kind 单实例」（与现有 citizen/Dispatcher one-hot 模式一致，`dispatcher.rs:30-46`），
因为：① 渐进映射是锁定决策 1；② SEPA/大盘/选股行点击共享「单一当前标的」心智
（`dispatch_symbol_fetch` 写 `SharedState.symbol`，`dispatcher.rs:100-108`），多实例图表会破坏该语义；
③ per-area 状态需序列化契约升级，超出本轮。为未来铺路：`EditorDescriptors` + kind 分发已把
「Tab 携带 instance id 升级为 per-area 实例」的改动面收敛到 `EditorInstances::get_mut` 一处（句柄化）。

### 4.6 应用级状态容器（Application 层）

```rust
/// 现有 CompassApp 字段按职责分组（不引入新 crate）
pub struct WallClock { pub clock: String }              // Time 驱动 StatusBar 时钟（main.rs:1354 现状）

pub struct CompassApp {
    // Layer A: Application 级
    pub workspaces: Workspaces,                          // 新 §4.4
    pub theme: CompassTheme, pub dock_style: egui_dock::Style,
    pub language: String, pub toast: ToastManager, pub modal: Modal, pub file_dialog: FileDialog,
    // Layer B: 编辑器实例（§4.5）
    pub editors: EditorInstances,
    // Layer C: 跨层基础设施（后端句柄/信号/标的选择器——选择器归 Topbar 后保留在 App 级）
    pub shared_state: Arc<SharedState>, pub signals: EditorSignals,
    pub stock_picker: StockPicker<…>, pub picker_list: Vec<…>,
    pub _backend_handle: BackendHandle,
}
```

## 5. 三个内置 Workspace 设计

### 5.1 「图表」（默认，`WorkspaceId::Chart`）

**编辑器组成**：Watchlist（左）+ Chart（主）+ Logger（底）。
现状的 Market/Sepa 两 tab 从本 workspace 移出（→ SEPA 复盘 workspace，见 §13 决策记录 / §14 仲裁 Q1）。

> **【2026-09-05 用户仲裁更新】（Q1 定案）**：Market/Sepa 两 tab **拆出**本 workspace、
> 归「SEPA 复盘」workspace 获用户确认（推荐采纳）——「图表」workspace 名实相符、
> 任务划分纯粹；SEPA 复盘 workspace 组成相应更新（见 §5.3）。

**Watchlist 布局（用户仲裁 Q6 定案）**：位于 Chart leaf 左侧的**独立 dock leaf**，以 tab 形式承载（可与其它编辑器叠 tab/拖动重排）；**宽度不设硬编码默认值**，由 dock split 比例 + 用户拖拽 + 持久化决定（无 240px 固定宽，与 SidebarLayout 的 default_width 规则无关——Watchlist 不是 Sidebar）。

> **【2026-09-05 用户仲裁更新】（Q6 定案）**：Watchlist 位置由用户自定义——**左侧 tab、无固定侧栏宽度**，
> 推翻「左侧竖 split 240px」与「主区叠 tab」两个推荐选项；含义＝图表 workspace 内**左侧独立 dock leaf、
> 以 tab 形式承载**，宽度不设硬编码默认值，由 dock split 比例 + 用户拖拽 + 布局持久化决定；
> 与 `SidebarLayout.default_width` 规则无关（Watchlist 非 Sidebar，不注册 N 键）。

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Topbar(40px): [图表|选股|SEPA 复盘]          │ [标的选择器] │ [主题] [语言] │
├──────────────┬───────────────────────────────────────────────────────────┤
│ 标的自选       │ [图表]                                                      │
│ Watchlist     │  Chart 编辑器                                                │
│ (header: 搜索 │   header: [1d|1w|1M][复权▾] │ [MA/BOLL▾ 指标]    [Fetch]│[⋮] │
│   + 添加按钮)  │   body:    K 线主区（空态 EmptyState 引导）                    │
│ 左侧 leaf tab │  sidebar(右侧, N 键): 指标参数 + 图层设置                       │
│ (宽度随 dock)  │                                                               │
├──────────────┴───────────────────────────────────────────────────────────┤
│ [日志]                                                                      │
├──────────────────────────────────────────────────────────────────────────┤
│ StatusBar(26px): 标的摘要 | 状态 | 数据源 · 时钟                              │
└──────────────────────────────────────────────────────────────────────────┘
```

**Tab 切换行为**：
- Watchlist / Chart 为同一 DockState 树中左、主两 leaf（`Tree::split_right(NodeIndex::root(), 0.75, ...)` 镜像法）；
  Watchlist leaf 与 Chart leaf 均可被用户拖动重排/关闭（关闭后经 Topbar 标的选择器或菜单重新打开——v1 提供「重开」入口：Topbar 标的选择器旁 `+` 下拉列出可注册编辑器，Blender 的 `Add Editor` 菜单类比）。
- Logger 底部 leaf，可关（重开同上）。
- **关闭/重排即写入持久化**（§9 即时保存）。

### 5.2 「选股」（`WorkspaceId::Screener`）

**编辑器组成**：Screener（主，Sidebar = 条件构建器）+ **Logger（底部，用户仲裁 Q5 定案：选股与 SEPA 复盘两处都含 Logger**，与现状习惯一致；错误同时走 inline + toast）。

> **【2026-09-05 用户仲裁更新】（Q5 定案）**：选股 workspace 底部**含 Logger**——推翻原推荐
> （原推荐选股无 Logger）；用户明确选择「两处都含」，与现状习惯一致；错误处理仍走
> inline + toast 双通道（Logger 为追加记录通道，不替代错误双通道）。

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Topbar: [图表|选股|SEPA 复盘] │ [标的选择器] │ [主题] [语言]                  │
├──────────────────────────────────────────────────────────────────────────┤
│ [选股器]                                                                   │
│  header: [共 N 只] · [运行… 状态 chip]             [N] [⋮]                  │
│  ┌───────────┬─────────────────────────────────────────────────────────┐ │
│  │ sidebar   │ 结果 DataTable(6 列：代码/名称/最新/20日涨幅/市值/行业)      │ │
│  │ 条件构建器 │  （默认市值降序；行点击 dispatch_symbol_fetch 联动图表）      │ │
│  │ (N 键,    │                                                          │ │
│  │  500px)   │                                                          │ │
│  │ ──────────│                                                          │ │
│  │ 根组卡片   │                                                          │ │
│  │ (AND/OR)  │                                                          │ │
│  │ +11 类    │                                                          │ │
│  │ +子分组    │                                                          │ │
│  │ [运行/清空]│                                                          │ │
│  └───────────┴─────────────────────────────────────────────────────────┘ │
├──────────────────────────────────────────────────────────────────────────┤
│ [日志]                                                                      │
├──────────────────────────────────────────────────────────────────────────┤
│ StatusBar                                                                    │
└──────────────────────────────────────────────────────────────────────────┘
```

**Tab 切换行为**：进入本 workspace 时 `active_screen.layouts[0].dock_state` 挂载；
Screener 条件/结果状态在全局实例中保留（来回切 workspace 不丢）；Logger 底部 leaf（Q5 定案）。

> **【2026-09-05 用户仲裁更新】（Q5）**：本段落「Tab 切换行为」因 Q5 补入 Logger——Logger
> 为底部独立 leaf，可关/重开行为与图表 workspace 一致（见 §5.1 Tab 切换行为）。

### 5.3 「SEPA 复盘」（`WorkspaceId::Sepa`）

**编辑器组成**：Sepa + Market 叠同一主 leaf 两 tab（保留现状用户习惯的 `[东方SEPA|大盘]` 相邻关系），Logger 底部。

> **【2026-09-05 用户仲裁更新】（Q1 + Q5 合并结果）**：本 workspace = **SEPA + 大盘 + Logger**——
> ① Q1 定案把 Market/Sepa 两 tab 从「图表」workspace 拆出归入本 workspace（推荐采纳）；
> ② Q5 定案本 workspace 底部含 Logger。SEPA 主 leaf 内 tab 相邻关系（`[东方SEPA|大盘]`）不变。

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Topbar: [图表|选股|SEPA 复盘] │ [标的选择器] │ [主题] [语言]                  │
├──────────────────────────────────────────────────────────────────────────┤
│ [东方SEPA] [大盘]                                                            │
│  header: [共 N 行 · 日期] [TOP 50|30] [刷新]           [N] [⋮]              │
│  body:   ① 市场温度计 Card → ② 12 列表格 → ③ 详情面板 280px（body 内嵌，    │
│          非 Sidebar——报告详情是 SEPA 编辑器主体构成，不注册 N 键）            │
│  大盘 tab:header: [共 N 个 · 日期] [行业板块|官方指数] [刷新]   [N] [⋮]       │
│          body: ① 核心指数 Card → ② 板块/指数列表                              │
├──────────────────────────────────────────────────────────────────────────┤
│ [日志]                                                                      │
├──────────────────────────────────────────────────────────────────────────┤
│ StatusBar                                                                    │
└──────────────────────────────────────────────────────────────────────────┘
```

**Tab 切换行为**：主 leaf 内两 tab（egui_dock tab 语义）互切；SEPA 筛选/TOP-N 截断/选中行、
Market 段选择的本地副本策略不变（SEPA `rows.clone().truncate(top_n)` 先例——**绝不回写 shared_state**）。

## 6. Editor 库存清单

| Editor | Header 结构（编辑器切换器位置 + 控件） | Sidebar | Toolbar |
|---|---|---|---|
| **Chart** | 切换器 = dock tab 栏（`[图表]`，Blender header type-switcher 同构，**不在 header 内重复做切换器**）；header 左端 Mode Toggle 组：周期 `Segmented 1d\|1w\|1M` + 复权 `Dropdown`（指数/板块隐藏逻辑保留 `editor/mod.rs:395`）→ 指标切换 `Dropdown`（MA/BOLL 显隐 + 参数入口，`id_salt("indicators")`）→ 右端：`Fetch` Primary 按钮（loading 禁用+spinner）+ Display Options（`⋮` 菜单：十字准线/交易量/图例开关，及 N 键提示） | **有**：指标参数（MA 周期编辑、BOLL 参数）+ 图层设置（K 线样式/成交量子开关）——Blender「Separated Data Properties from Tools」的 N-panel 类比（锁定决策 7）；N 键显隐，默认 `visible=true`（Q4 定案） | **无**（锁定决策 7：无工具数据支撑）。空槽表达：`EditorLayout { toolbar: None }`——`EditorFrame` 见 None 不分配任何区域（Blender 源码证明 Outliner 同样无 toolbar） |
| **Screener** | 切换器 = dock tab；header：计数标签「共 N 只」（`screener_total`）+ 运行中状态 chip + 右端 `⋮`（**「重置排序」+「清除结果」两项**；2026-09-05 裁决补入「重置排序」——恢复业务默认市值降序 `MARKET_CAP_COLUMN=4`，`screener.rs:151-153`，i18n 键 `editor.screener_header.reset_sort`；「列宽重置」暂缓，见下方勘误注记） | **有**：条件构建器（根组卡片+11 类+子分组，从现状主区上段迁入，`screener.rs` 的 `builder_root`/`builder_root_operator` 状态不变）；N 键显隐，默认 `visible=true`；**sidebar 专用宽度 `default_width 500.0 / width_range (486.0, 640.0)`**（与 Chart 240 解耦，见 §4.1 勘误）；「运行/清空」按钮随构建器入 sidebar 底部 | 无 |
| **SEPA** | 切换器 = dock tab；header = 现状「② 工具条」升级：计数「共 N 行 · 日期」+ `Segmented [TOP 50,TOP 30]` + 刷新（Primary + spinner；纯手动）；右端 `⋮`（菜单含**「重置排序」**一项，2026-09-05 2c 裁决：SEPA 表头点击排序持久生效——`table` 为 panel 字段、`set_sort` 仅构造时调用（`sepa.rs:170-172`）、`set_rows` 不重置排序，用户乱序后无显式复位入口；菜单项恢复官方默认（rank asc + 得分列 3..=8 降序），动作幂等；i18n 键 `editor.sepa_header.reset_sort`） | 无（详情面板 280px 是 body 内部右栏，保持 `sepa.rs:35` 布局与垂直堆叠约束 `ref #221`） | 无 |
| **Market** | 切换器 = dock tab；header = 现状「② 工具条」：计数 + `Segmented [行业板块\|官方指数]` + 刷新；右端 `⋮`（菜单含**「重置排序」**一项，2026-09-05 裁决：Market 表头点击排序持久生效——`table` 为 panel 字段（`market.rs:94`）、`set_sort(CHANGE_COLUMN, true)` 仅构造时调用（`market.rs:115-120`）、`set_rows` 不重置排序（`data_table.rs:110-112`），用户乱序后无显式复位入口；菜单项恢复板块轮动业务默认 = **涨幅降序**（`CHANGE_COLUMN=3`，`market.rs:66-68`），动作幂等；i18n 键 `editor.market_header.reset_sort`） | 无 | 无 |
| **Logger** | 切换器 = dock tab；header = 现状 `SectionTitle`（标题+计数+导出按钮，`logger.rs:45-60`）升级为 EditorFrame Header 槽：`[日志] · N 条` 左 + `[导出]` 右 | 无 | 无 |
| **Watchlist**（新，Outliner 式） | 切换器 = dock tab；header：搜索输入框（`Ctrl+K` 聚焦语义从旧侧栏迁来，`main.rs:1104-1107`）+ `[添加]` IconButton（添加当前标的，`main.rs:1264` 语义）；位置：图表 workspace 左侧独立 dock leaf tab（Q6 定案），宽度随 dock 不固定 | 无 | 无 |

> **【2026-09-05 2b 评审勘误】（裁决 1：列宽重置暂缓）**：Screener header 右端 `⋮`
> **当前仅「清除结果」一项**（截至 2b 评审时；2026-09-05 后续裁决补入「重置排序」，
> 见下方 2026-09-05 ⋮ 菜单裁决注记——即本段「仅一项」描述不再反映最终契约）。
> 原契约「列宽重置/清除结果」两项中的「列宽重置」**暂缓**——
> 事实核实：DataTable（`crates/compass-ui/src/widgets/data_table.rs`）的 `ColumnSpec` 仅
> `header: &'static str` + `numeric: bool` 两字段（:69-74），pub API 为
> `new/set_rows/set_selected/set_tokens/set_sort/sort_descending/set_descending_default/show`
> （:97-151），无任何列宽调整/列宽状态 API，「列宽重置」当前无可重置对象。**触发条件**：
> DataTable 获得列宽调整能力（如列宽拖拽或可配置列宽）的同一 PR 再补入该菜单项，
> 并同步本段契约与 plan 相应文本。实现现状对应：`screener.rs:221-231`（menu 仅
> `editor.screener_header.clear_results` 一项）；i18n 键 `editor.screener_header.*`
> 仅需 clear_results（列宽重置键在能力落地前不定义）。

> **【2026-09-05 ⋮ 菜单裁决】（Q2：Screener ⋮ 补「重置排序」）**：Screener 结果表 ⋮
> 补入**「重置排序」**一项（位置：菜单上项，「清除结果」移至下项——非破坏性动作在上、
> 破坏性在下；沿 OS 菜单惯例），行为同 SEPA 2c 裁决：恢复构造默认 = **市值降序**
> （`set_sort(4, true)`，`screener.rs:151-153`），动作幂等。**委托背景事实勘误**：综述称
> `new()` 无 set_sort 调用（初始化默认 = 第一列升序 = 按代码排序）——**以代码为准**：
> `screener.rs:151-153` 实际调用 `set_sort(MARKET_CAP_COLUMN, true)` +
> `set_descending_default(MARKET_CAP_COLUMN, true)`（默认市值降序，与 §5.2「默认市值降序」
> 一致）；且 `DataTable::show` 每帧应用 `sort_rows`（`data_table.rs:176`），**无「backend
> 原始顺序」显示模式**——初始顺序 = 构造排序态 = 市值降序（并列按列 0 代码升序，
> `data_table.rs:281`），reset 目标明确。「clear_results 后重跑即恢复」不成立：`clear_results`
> 只清 `screener_result/total`（`screener.rs:228-230`）不改 table 排序态，重跑走 `set_rows`
> （`screener.rs:364`）同样保留排序态——乱序在重跑后仍保留（与 SEPA 2c 同因）。
> i18n 键 `editor.screener_header.reset_sort`（zh 重置排序 / en Reset sort，镜像
> `editor.sepa_header.reset_sort`，`zh.yml:61` / `en.yml:63`）。

> **【2026-09-05 用户仲裁更新】（§6 相关行确认）**：① **Chart 行**——Fetch 按钮在 header
> **右端**（Q3 定案，推荐采纳：Primary + loading，与 Display Options `⋮` 同端）；Sidebar
> **默认显示** `default_visible=true`（Q4 定案，推荐采纳）；P1 定案：dock tab 栏 = 编辑器
> 切换器，Mode Toggle 紧贴 header 左端（采纳推荐）。② **Watchlist 行**——位置补全为
> 「图表 workspace 左侧独立 dock leaf tab（Q6 定案），宽度随 dock 不固定」：无硬编码
> 默认宽，不进 SidebarLayout 规则，显隐走 dock tab 关闭/重开（P5 已确认）。

**EditorLayout 空槽表达总则**：`toolbar: None` / `sidebar: None` 的编辑器，`EditorFrame` 不渲染对应区域、不注册 N 键、不占布局——「空区域零渲染成本」（锁定决策 1）。

## 7. 全局 chrome（Application 层两个 global area）

### 7.1 Topbar（40px，`Panel::top`，现状 `render_toolbar` 重构后）

```
┌──────────────────────────────────────┬──────────────────────────────────────────────────┐
│ [图表|选股|SEPA 复盘](workspace 切换)  │ [标的选择器 ▾] │ [⋮ 添加编辑器] │ [主题] [语言]   │
└──────────────────────────────────────┴──────────────────────────────────────────────────┘
   左：Workspace 切换（Segmented，3 段，图标+键化名：CHART_LINE 图表 / FUNNEL_SIMPLE 选股 / GAUGE SEPA 复盘）
   中：标的选择器（SearchableDropdown，stock+index 合并列表 = 现状 picker_list）
   右：`⋮ 添加编辑器`（下拉列出注册表 EditorKind 中未在屏幕上的，重建已关闭 tab——v1 便捷入口）
   最右：主题 Dropdown + 语言 Dropdown（从现状 Group D 迁入，行为/持久化不变 main.rs:1405-1467）
```

> **【2026-09-05 用户仲裁更新】（Q2 定案）**：主题/语言入口在 **Topbar 最右段**获用户确认
> （推荐采纳）；ASCII 图补画最右段「[主题] [语言]」。StatusBar 不放主题/语言（§7.2）。

交互：
- workspace 切换：点击段位 → `Workspaces::switch(target)` → 当前 workspace 的 DockState 就地保存并**即时写入 config**（§9）→ 挂载目标 DockState → `request_repaint()`。切换后 toast 不弹（布局变化本身即反馈，避免噪音；与 theme/language 的 Info toast 先例区分——那是「配置已写回」的提示）。
- 切换动画：egui 无布局过渡——**瞬时**（锁定决策隐含的克制原则，与 `ref #245` 动画范围决策一致；kittest 稳定优先）。
- 标的选择器交互保留现状（交易所/代码/名称格式、↑↓/Enter 键盘导航、`/` 聚焦——`/` 语义不变，全局生效）。

### 7.2 StatusBar（26px，`Panel::bottom`，现状改版最小化）

| 段 | 内容 | 变化 |
|---|---|---|
| 左 | 标的摘要（mono 价格 + 红涨绿跌，`latest_quote`） | 不变（`main.rs:1352-1393`） |
| 中 | 状态点（loading 脉冲 / error 红点 / idle） | 不变 |
| 右 | 数据源信息（N 只）+ 本地时钟（每秒刷新） | 不变；**新增可选** workspace 名 caption（`workspace.chart` 等）——推荐省略（摘要已含标的信息），待确认 |

主题/语言**不放** StatusBar（Topbar 右端已由 Q2 定案，见 §13 决策记录「主题/语言入口」行）；StatusBar 保持 26px 纯信息角色。

> **【2026-09-05 用户仲裁更新】（Q2）**：主题/语言位置已确认——Topbar 最右段；StatusBar
> 保持 26px 纯信息角色（推荐采纳，本节无实际变更，仅确认）。

## 8. N 键语义（各编辑器 Sidebar 显隐）

### 8.1 机制（egui 无全局按键轮询的事实约束）

- egui 不提供 winit 式全局按键监听/钩子；唯一机制是**每帧驱动内轮询**：
  `ui.ctx().input(|i| i.key_pressed(egui::Key::N))`——只能在 `App::ui` 主分发点轮询一次，
  不可注册「按下即回调」。现有 `handle_shortcuts`（`main.rs:1083-1117`）即此模式，N 键并入同一路由。
- **焦点守卫（必须）**：`let editing_text = ui.ctx().memory(|m| m.focused().is_some());`
  文本输入框（标的选择器/自选股搜索/LLM 输入）聚焦时 N 不触发——否则输代码/输入自带字母 n
  的词会误弹侧栏（与 `1/2/3` 守卫同因，`main.rs:1087`）。补充：弹层（Dropdown/MultiSelect 弹出）
  内置输入框同样有焦点，守卫全覆盖。
- **重复触发防抖**：`key_pressed` 只在本帧按下事件为真（egui 自动区分 repeat？——`key_pressed` 含系统
  自动重复；需要仅初始按下时用 `key_pressed` + 本帧 `!i.key_down` 状态机或忽略 repeat 的 `i.keys_down`，
  v1 采用「长按 repeat 允许」（toggle 语义可接受），不额外做 repeat 抑制——记录为已知取舍。

### 8.2 作用对象：当前聚焦编辑器

```
N 键按下且 !editing_text
  → 判定 active editor：
     ① `dock_state.focused_leaf()`（egui_dock 0.20 `dock_state/mod.rs:370`，与现有样式高亮同源）
     ② fallback：`last_interacted_kind`（TabViewer::on_tab_button click 时更新，`tabs.rs:340-348` 已有钩子）
  → `EDITOR_REGISTRY` 查该 kind 的 `layout.sidebar`：
     Some(layout) → 翻转 `sidebar_visibility[kind]`（会话级 HashMap，默认 = layout.default_visible）
     None         → 无操作（不提示，Blender 中无 panel region 的编辑器 N 亦无操作）
```

> **【2026-09-05 用户仲裁更新】（Q4）**：Chart/Screener 的 Sidebar **默认显示**
> （`default_visible=true`）已确认（推荐采纳）；N 键翻转机制不变——本节无逻辑变更，仅确认默认值方向。

- **状态存储**：`sidebar_visibility: HashMap<EditorKind, bool>`（App 级，会话级不持久化——
  与 adjust 模式会话态先例一致 `main.rs:1132-1142`；v1 持久化面最小，§11 决策记录 #12）。
- **动画**：显隐瞬时（egui 布局无过渡；N-panel 弹入滑动动画超本轮）。
- **可访问性**：Header 右端 Display Options 中的「侧栏」按钮保留（IconButton + tooltip「显示/隐藏侧栏 (N)」），
  鼠标用户与键盘用户双入口；按钮 label 走 i18n 键 `editor.toggle_sidebar`。

## 9. 持久化

### 9.1 config.toml `[layout]` 节 schema 草案

```toml
[layout]
active_workspace = "chart"          # WorkspaceId 字符串："chart" | "screener" | "sepa"
dock_version = 1                     # egui_dock serde 格式版本；加载失败→回退默认布局(warn)

# 每 Workspace 一条（v1：一 workspace = 一屏；[[…]] 表数组为多屏预留）
[[layout.workspaces]]
id = "chart"
active_screen = 0
dock = """{"tabs":[...]}"""          # DockState<Tab> 的 JSON（egui_dock serde feature 输出）
tab_widths = [...]                   # 预留：面板宽度组（split 比例已在 dock 树内；此处放 v1 面板固定宽）

[[layout.workspaces]]
id = "screener"
active_screen = 0
dock = """{...}"""

[[layout.workspaces]]
id = "sepa"
active_screen = 0
dock = """{...}"""
```

**格式决策事实依据**：
- egui_dock 0.20.1 提供 `serde` feature（`Cargo.toml: serde = ["dep:serde", "egui/serde"]`），
  `DockState<T: Serialize>` 可直接序列化（`dock_state/mod.rs:44` derive）——**开 feature，不引新依赖**。
- TOML 内嵌 JSON 字符串沿用 `[screener] filter = "<JSON>"` 先例（`main.rs:301-310`）。
- 写盘用现有 **read-modify-write** 模式（`save_screener_config`/`save_watchlist_config`/`save_theme_config`，
  `main.rs:552-673`）：读 `toml::Value` → 改 `[layout]` → 整体写回；未知节/注释丢失为既有已接受取舍。
- 加载失败（坏 JSON / dock_version 不匹配 / TOML 语法错）→ `Workspaces::default()` 三默认布局 + warn
  （对齐 load_config 的「配置永远不阻止启动」原则 `main.rs:399-431`）。

> **【2026-09-05 用户仲裁更新】（Q6 相关）**：Watchlist 宽度**不进** `tab_widths` 固定宽预留组——
> 其宽度由 DockState 树内的 split 比例持久化（用户拖拽即写，§9.2 的 DockStateChange 触发保存）；
> `tab_widths` 仍仅预留（SEPA 详情 280px 等 body 内嵌右栏宽度，现状 `sepa.rs:35` 不变）。

### 9.2 启动恢复 / 切换即时保存

```
启动：load_config → [layout] 解析 → 每 workspace DockState::from(serde) → active_workspace 挂载
缺失/损坏：default_layout(id)（main.rs:154-172 抽成 per-id 构造）
切换：Workspaces::switch(target) := 保存当前 DockState（内存）+ 立即写 config.toml（即时保存）
布局改动：DockArea::show_inside 返回 Option<DockStateChange>（TabMoved/TabClosed/…）
        → Some(_) 即触发保存（同帧末尾，写盘频率 = 用户拖拽频率，可接受）
退出：无需退出钩子（保存已即时）
```

## 10. 迁移路径（分阶段，每阶段独立可验证）

| 阶段 | 内容 | 验证方式 |
|---|---|---|
| **0 代码勘察/类型骨架** | 开启 egui_dock `serde` feature；新建 `editor/` 模块：`EditorKind`/`EditorLayout`/`EditorDescriptor`/`EDITOR_REGISTRY`/`WorkspaceId`/`ScreenLayout`/`Workspace`/`default_layout`（纯类型 + serde，**零 UI 改动**） | `cargo check` + 新单测（§11 测试锚点组 A）；现有 kittest 全绿（无行为变化） |
| **1 EditorKind/Layout 骨架落地，不迁 UI** | `TabKind` 保留，新增 `From<TabKind> for EditorKind` 映射 + descriptor 引用；`Workspaces` 默认布局函数抽离（`main.rs:154-172` 只搬逻辑不接 UI） | 单测：映射全变体、descriptor 完整性（Header 恒存在、title_key/icon 非空）；`cargo test` 全绿 |
| **2 逐编辑器迁移**（每子步独立 commit） | 2a Chart：周期/复权/Fetch 从 `render_toolbar` 移入 Chart header（`main.rs:1408-1456` 移除）→ 2b Screener：条件构建器 → Sidebar（`screener.rs` 布局调整，状态不动）→ 2c SEPA：工具条 → header → 2d Market：工具条 → header → 2e Logger：SectionTitle → header → 2f Watchlist：`render_sidebar`（`main.rs:1220-1270`）→ `WatchlistEditor`（Panel::left 移除 `main.rs:903-911`，Watchlist 入图表 workspace dock 树**左侧独立 leaf**——Q6 定案形态，宽度随 dock 无默认值） | 每子步：既有 kittest 更新 + 新 kittest 断言（label 存在性）；`scripts/run.sh` 截图（多模态视觉 + 形状/像素交叉验证，禁单一目测，AGENTS.md 品质准则） |
| **3 Workspace 容器与切换** | `Workspaces` 接入 CompassApp；Topbar 改造（workspace Segmented + 标的选择器同层，移入主题/语言至最右段；移除 Group B/C/D 残留）；`EditorCtx` 收敛 TabViewer 借用束 | kittest：切换 workspace → 断言中央 UI 变化（「选股器」标题/DataTable 表头存在）；既有 `citizens_register_and_activate` 类单测更新 |
| **4 持久化** | `[layout]` 节读写、启动恢复、切换即时保存、DockStateChange 触发保存 | round-trip 单测（§11 组 C）+ kittest 两阶段（构造→保存→载入→树相等）；手改 config 损坏值 → 回退默认 + warn（`RUST_LOG=debug scripts/run.sh` 观察） |
| **5 收尾** | 快捷键上下文化（`1/2/3` 仅在图表 workspace 生效——以 active workspace + 活跃编辑器判定；`/` 保持全局）、N 键全链路、新 i18n 键、doc-sync（`.dsh/kb/design/ui.md`/`architecture.md`/`user/gui.md`/`user/config.md`）、决策记录 | `just check` 全绿（覆盖率门槛维持：compass 90%）；截图验收清单逐项核 |

每阶段 commit 引用 `ref #357`；阶段 2 子步每个引用同 issue（同一 epic/PR 内）。

> **【2026-09-05 用户仲裁更新】（阶段 2f/3）**：2f 描述已按 Q6 定案明确 Watchlist 入图表
> workspace dock 树的形态——**左侧独立 dock leaf tab、宽度随 dock 不固定**（无硬编码默认宽）；
> 阶段 3 Topbar 改造已含 Q2 定案（主题/语言 → Topbar 最右段）。两处无计划性冲突。

## 11. 测试锚点（egui_kittest 无头可测面）

既有模式来源（`.dsh/kb/dev/testing.md`）：
- `Harness::new_ui(|ui| …)` 纯 CPU 无显示服务器（testing.md:367）；`Harness::new_eframe` 驱动完整 App（:368）。
- `get_by_label`/`Node::click`/`type_text`（:369）；`node.value()` 而非 `node.label()`（:390-392）。
- 虚拟时间 `ctx.input(|i| i.time)` 驱动动画 + `step_dt` 细粒度步进（:370-382，toast/modal 先例）。
- **限制**：egui_dock 0.20 tab 按钮无 AccessKit label → tab 切换用程序化 `DockState::set_active_tab`（:383，`dock_state/mod.rs:237`）；`dock_style.rs:221-334` 已有「点击 tab → 形状扫描 accent」渲染链验证先例。
- 渲染断言（`response.rect.width()`）优先字段断言（:415-420）；形状扫描 `unique_colors` 模式（dock_style.rs:337-362）。

**组 A — 注册表/纯逻辑（单测，无头）**：
- `EDITOR_REGISTRY` 完整性：6 kind 各恰 1 条、`layout.header` 恒存在、`sidebar/toolbar` 与预期角色一致、title_key/icon 非空且 title_key 在 i18n 字典存在（`LANG_LOCK` 串行模式，`tabs.rs:358-375`）。
- `EditorKind`/`WorkspaceId` serde：snake_case 字符串 round-trip + 未知值回退（对齐 `adjust_index_from_value` 回退测试风格 `main.rs:1785-1808`）。
- `default_layout(id)`：三 workspace 默认 DockState 结构断言（split 层级、tab 序列、宽度比例；**图表 workspace 断言 Watchlist leaf 在左、Chart 主 leaf，Chart 右/下层级与 Logger 底部**）。

**组 B — workspace 切换（kittest）**：
- Topbar workspace Segmented 是自定义组件（有 AccessKit label，可 `get_by_label`）→ 点击「选股」→ 断言「选股器」编辑器 header 存在、（chart 特有控件如「Fetch」消失）。
- 程序化 `Workspaces::switch` + 单测断言 active index/保存触发。

**组 C — 布局持久化 round-trip（单测为主）**：
- 构造 `DockState<Tab>`（三 workspace 默认树）→ `serde_json::to_string` → `from_str` → 递归断言节点结构/tab kind 序列/宽度相等（`Tab`/`TabKind` 需 derive `Serialize, Deserialize`）。
- 损坏 JSON / 缺 `dock` 键 → `default_layout` 回退 + 不 panic（对齐 `load_config` 回退测试）。
- TOML 层：`[layout]` 与未知节共存时 read-modify-write 不丢其他节（镜像 `save_theme_config` 测试思路）。

**组 D — EditorFrame/N 键（kittest）**：
- Chart 编辑器：`harness.event(Event::Key { key: N, .. })` → 断言 sidebar 容器（指标参数 label）出现/消失（渲染级：`harness.output().shapes` 或 label 查询）。
- 文本焦点守卫：先 `type_text` 聚焦输入框 → 再发 N → 断言 sidebar 状态不变。
- 无 sidebar 编辑器（Logger）→ 发 N → 无状态变化。
- Header 必备断言：每个编辑器渲染后 header 右端 Display Options `⋮` 可查询。
- **Chart sidebar 默认显示（Q4 定案）**：新 App 启动（默认布局）后 Chart sidebar 可见（指标参数 label 可查询）。

**组 E — 回归保护**：
- 迁移不破坏语义的既有测试全绿：`dispatch_symbol_fetch` 系列（dispatcher.rs:262-356）、
  SEPA 表格垂直堆叠 `ref #221` 断言、`focused tab accent ring` 渲染链（dock_style.rs:221-334）、
  toast/modal 虚拟时间动画（ref #168/#171）。

## 12. 决策冲突点（P1-P5 —— 2026-09-05 全部经用户仲裁确认；本节保留为过程归档）

> 本节 5 个映射歧义/决策缝隙均无硬冲突；用户 2026-09-05 对整体方案批准时一并采纳全部推荐
> （P1 单独列出确认）。推荐列即定案内容，「若选择其他」「影响」列保留供归档参考。

| # | 冲突/歧义 | 涉及锁定决策 | 推荐 = 用户定案（✅ 已确认 2026-09-05） | 若选择其他 | 影响 |
|---|---|---|---|---|---|
| P1 | 「Chart header：编辑器切换器 → Mode Toggle」字面理解为 header 内切换器；但 dock tab 栏已是类型切换载体 | D1 + D7 | ✅ **dock tab 栏 = 编辑器切换器**（Blender header 的 type-switcher 同构）；Mode Toggle 紧贴其下方 header 左端 | header 内再做切换器 | header 内做会与 dock tab 双重切换语义冲突、交互重复；推荐方案零新增 |
| P2 | 「主题/语言 → topbar/statusbar」未指定具体位置 | D6 | ✅ **Topbar 最右段**（现状用户已习惯在顶部找到） | StatusBar | StatusBar 26px 全屏角落、发现性差 |
| P3 | D6 拆解清单**未包含 Fetch 按钮**的去向 | D6（遗漏） | ✅ **Chart header 右端**（Primary + loading） | 保留 Topbar | Fetch 语义绑定 Chart 编辑器；选股/SEPA workspace 无图表时 Topbar 上有 Fetch 是无效控件——保留会延续 F1 耦合 |
| P4 | 图表 workspace 是否保留 Market/Sepa 三 tab（现状 `main.rs:155-158`） | D2/D7（workspace=任务划分） | ✅ **拆出**：Market/Sepa 归 SEPA 复盘 workspace | 保留现状三 tab | 拆出后「图表」workspace 名实相符、Blender 式任务划分纯粹；保留则迁移更平滑但 workspace 语义混乱 |
| P5 | 自选股编辑器化后，其「显隐」走什么通道（锁定决策未明） | D5 + 「侧栏开关→各编辑器 N 键」 | ✅ Watchlist 非 Sidebar：显隐 = dock tab 关闭/`⋮ 添加编辑器`重开；N 键仅作用于注册了 Sidebar 的编辑器 | N 键兼作 workspace 左栏开合 | 推荐方案与 Blender Outliner（无 toolbar、无 N-panel）完全一致 |

## 13. 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| Area 引擎 | 保留 egui_dock 0.20 深度定制 / 自研分区引擎 / 换 egui_tiles | 保留 egui_dock（锁定决策 1） | DockState 树 = bScreen 顶点图同构；0.20.1 已有 serde feature（`dock_state/mod.rs:44`）与 7 态 Style 深度定制；现有 dock_style/kittest 渲染链测试全复用 | 自研拖拽/重排/tab/序列化成本极高且无收益；egui_tiles 自述开发早期（既有决策记录 `ref #119 D1`） |
| Screen 多布局 | `layouts: Vec<ScreenLayout>` 预留 / 单字段 | `Vec` 预留（锁定决策 3） | 数据结构预留零 UI 成本；`ScreenLayout` 本身即 serde 单屏；未来屏幕切换不破坏持久化契约 | 单字段日后升级需改 schema 与加载逻辑 |
| Editor 注册表 | 静态 const 表 + enum（编译期穷尽） / `once_cell`/inventory 动态全局表 | 静态表 | 6 编辑器固定；match 穷尽 + 静态表完整性与 i18n 键可单测；无全局可变状态；kittest 断言「恰 6 条」 | 动态注册需全局初始化、错误处理与运行时迭代，收益低（新增编辑器两处改动可接受） |
| 自选股 | 编辑器化（EditorKind::Watchlist，Outliner 类比）/ 保留全局左栏 | 编辑器化（锁定决策 5） | 可入 dock（任意 workspace 组合/拖动/重开）；解耦 Application 层（F3）；Blender Outliner 先例 | 保留全局左栏延续「面板各自为政」，无法纳入布局持久化 |
| per-Editor 状态所有权 | 每 Kind 单实例（现 citizen 全局容器演进）/ per-area 多实例（Blender 完全式） | 单实例 + kind 分发（句柄化预留） | 渐进映射（D1）；现有 citizen/Dispatcher one-hot/SharedState 响应式全复用；SEPA/大盘/选股行点击共享「单一标的」语义（`dispatcher.rs:100-108`）；`EditorInstances::get_mut` 单点分发为未来升级留缝 | per-area 实例需每个编辑器状态打 serde 契约、layout 树与实例生命周期绑定，风险与工作量远超本轮；会破坏「同屏多个结果表联动单一图表」的用途假设 |
| 持久化格式 | config.toml `[layout]` 节（DockState JSON 内嵌 + dock_version）/ 独立 layout 文件 | config.toml 单文件（锁定决策 8） | 保持项目单配置约定；`save_*_config` read-modify-write 先例成熟（`main.rs:552-673`）；坏值回退默认不阻止启动 | 独立文件破坏单文件约定、需新加载路径与失败处理 |
| N 键实现机制 | `ctx.input` 轮询集中路由 / 引入 keymap-operator 系统 | ctx.input 轮询（现有 `handle_shortcuts` 演进） | egui 无全局按键监听（唯一机制即每帧轮询）；现有模式已验证（`main.rs:1083-1117` + 焦点守卫）；keymap DSL 需运行时注册/重绑定/序列化，超出本轮 | keymap 系统是 Blender 完整基础设施复刻，属过度设计（单一 N 键） |
| 图表 workspace 归属 Market/Sepa | 保留现状三 tab / 拆至 SEPA 复盘 workspace | 拆至 SEPA 复盘（用户仲裁 Q1 定案，2026-09-05 确认） | workspace=任务语义（D2）；三 tab 属「复盘」任务；拆分后 Chart header 结构（D7）更纯粹 | 保留让「图表」workspace 名不符实，且 Topbar/header 拆分收益打折 |
| Fetch 按钮去向 | 保留 Topbar Group C / Chart header | Chart header 右端（用户仲裁 Q3 定案，2026-09-05 确认） | Fetch 语义绑定 Chart 编辑器；workspace 无 Chart 时 Topbar 上的 Fetch 是无效控件（F1 根因之一） | 保留 Topbar 继续全局 chrome 与编辑器耦合 |
| 主题/语言入口 | Topbar 右端 / StatusBar | Topbar 右端（用户仲裁 Q2 定案，2026-09-05 确认） | 发现性最优；现状用户已在顶部建立心智（`main.rs:1405-1467` 迁入即可） | StatusBar 紧凑信息区、点击不便；两处重复 |
| 编辑器内部状态持久化范围 | 仅 DockState 树 + 面板宽度 / 含各编辑器内部状态（builder_root、TOP-N、排序） | 仅树 + 宽度（v1） | 锁定决策 8 字面范围；编辑器内部状态 serde 需逐 citizen 打标（MultiSelect 瞬态不可序列化）、契约与测试矩阵爆炸 | 内部状态持久化收益低（builder 有 config 级 `[screener]` 兜底），风险高 |
| Sidebar 显隐持久化 | 会话级（默认 default_visible）/ `[layout]` 持久化 | 会话级 | 与 adjust 会话态先例一致（`main.rs:1136` 注释）；v1 持久化面最小 | 持久化需 per-screen 拆键 + 恢复时序处理，收益低 |
| Logger 默认布局 | 选股无 Logger / 两处都含 | **两处都含**（用户仲裁 Q5 定案，2026-09-05 确认——推翻原推荐「选股无 Logger」） | 用户明确选择与现状一致的习惯；错误处理仍走 inline + toast 双通道 | 版面纯净收益低于用户习惯一致性 |
| Watchlist 布局 | 左侧竖 split 240px（resizable）/ 主区叠 tab / 左侧 tab 无固定宽度 | **左侧 tab 无固定宽度**（用户仲裁 Q6 定案，2026-09-05 确认——用户自定义，推翻两个推荐选项） | 保持左栏空间习惯（左侧）+ 无硬编码宽度（dock split + 拖拽 + 持久化控制），可与其它编辑器叠 tab | 固定 240 默认宽与「主区叠 tab」均未获用户选择 |
| Screener ⋮ 菜单「列宽重置」 | 删除契约项 / **暂缓（记录为待 DataTable 能力）** | **暂缓**（2026-09-05 2b 评审勘误） | DataTable（`compass-ui/src/widgets/data_table.rs`）`ColumnSpec` 仅 header+numeric，pub API 无任何列宽调整/状态能力（:69-151），「列宽重置」无可重置对象；保留契约意图（重置是列宽功能的补全项）待 DataTable 列宽调整上线首日一并实现 | 删除会丢失功能意图（列宽调整是数据表常见需求）；虚构空菜单项违反「不实现不可用功能」原则 |
| SEPA ⋮ 菜单内容 | 不渲染 ⋮ / **⋮ 含「重置排序」一项** / ⋮ 含其他动作 | **⋮ 含「重置排序」一项**（2026-09-05 2c 裁决） | 事实：SEPA 表头点击排序持久生效（`table` 为 panel 字段 `sepa.rs:143-144`，`set_sort(0, false)` 仅构造时调用 `sepa.rs:170-172`，`set_rows` 不重置排序 `data_table.rs:110-112`）；用户乱序后唯一复位路径是自行点击表头 rank 列，须已知「官方顺序 = rank asc」这一隐含约定（发现性差）；re-run 后乱序仍保留；恢复目标由代码明确定义（rank asc + 得分列 3..=8 降序），API 齐备（`set_sort`/`set_descending_default`）且动作幂等 | 不渲染：官方顺序复位依赖用户自行发现表头点击路径；其他动作无对象——清除结果（数据为后台一次性 `RunSepaRequest` 报告，无「清除」语义）、列宽重置（DataTable 无列宽调整 API，同 2b Screener 裁决）、报告导出（无既有实现，不虚构空菜单项） |
| Screener Sidebar 宽度 | 沿用 Chart 240.0/(200.0..=320.0) / **Screener 专用 500.0/(486.0..=640.0)** / 窄宽重排（卡片纵向堆叠） | **Screener 专用 500.0/(486.0..=640.0)**（2026-09-05 2b 评审勘误） | 条件卡片为原子组（ref #220：label+control 永不跨行），最宽叶片 Momentum 估宽 470px（`screener.rs:1272-1285`）+ Panel 内边距 8×2=16px = 下限 486px；默认 500px 恰为 `GROUP_ALIGNMENT_WIDTHS` 首个测试锚（`screener.rs:1882`），复用既有 kittest 覆盖；240px 下内容区仅 224px 无法容纳任何叶片（最窄 300px），控件必被 Panel clip（2b 评审矛盾根因） | 沿用 Chart 值直接复现溢出；窄宽重排需改 `render_leaf_row`/`render_leaf_params` 布局与全部 wrap 测试锚，工作量大且破坏原子组心智，留待后续专项设计 |
| Market ⋮ 菜单内容 | 不渲染 ⋮ / **⋮ 含「重置排序」一项** / ⋮ 含其他动作 | **⋮ 含「重置排序」一项**（2026-09-05 裁决，与 SEPA 2c 同构） | 事实与 SEPA 完全同构：`table` 为 panel 字段（`market.rs:94`），`set_sort(CHANGE_COLUMN, true)` 仅构造时调用（`market.rs:115-120`），`set_rows` 不重置排序（`data_table.rs:110-112`），表头点击排序持久生效（`toggle_sort` `data_table.rs:262-269`）；用户按其他列（最新价/成交额/名称）乱序后，唯一复位路径 = 点回 CHANGE 列头——须已知「官方默认 = 涨幅降序」（`market.rs:66-68`）这一隐含约定，且误点一次活跃列头即翻向升序、须再点一次（发现性差）；刷新/segment 切换均不清排序态（`filter_rows` 只过滤本地副本 `market.rs:370-380`，绝不写 shared_state；`set_rows` 保留排序）；恢复目标由代码明确定义（板块轮动默认 = `set_sort(3, true)`），API 齐备（`set_sort` + 只读 `sort_column()` getter `data_table.rs:143-145`，SEPA 2c 先例）且动作幂等 | 不渲染：§6 已声明「右端 ⋮」槽位，空 ⋮ 槽零收益且丢弃「官方顺序显式复位」意图（SEPA 2c 同因被否）；其他动作无对象——清除结果（快照为后端一次性 `RunIndexSnapshotRequest` 报告，无清除语义，同 SEPA 2c 排除）、列宽重置（DataTable 无列宽调整 API，同 2b Screener 裁决）、导出（无既有实现，不虚构空菜单项）、刷新（header 已有 Primary + spinner 按钮，重复） |
| Screener ⋮ 补「重置排序」 | 补（同 SEPA 模式）/ 不补（clear_results 后重跑即恢复） / 暂缓（随 2d/PR 收尾再定） | **补**（2026-09-05 裁决） | 先勘误：委托综述称 `new()` 无 set_sort 调用（默认 = 第一列升序按代码）——**以代码为准**：`screener.rs:151-153` 实际调用 `set_sort(MARKET_CAP_COLUMN=4, true)` + `set_descending_default(4, true)`（默认市值降序，与 §5.2 契约一致）；`DataTable::show` 每帧应用 `sort_rows`（`data_table.rs:176`），无「backend 原始顺序」显示模式——初始顺序 = 构造排序态 = 市值降序（并列按列 0 代码升序，`data_table.rs:281`），reset 目标明确；「clear_results 后重跑即恢复」不成立：`clear_results` 只清 `screener_result/total`（`screener.rs:228-230`）不改 table；重跑走 `set_rows`（`screener.rs:364`）同样保留排序态——乱序在重跑/重筛后仍保留（与 SEPA 2c 同因）；动作幂等（`set_sort(4, true)` 恒等恢复构造默认） | 不补：「复位入口冗余」反例成立——唯一手动路径（点回市值列头）需已知隐含默认约定，误点一次活跃列头即翻向升序、须两次点击；暂缓：无新增不确定性（对象/API/目标顺序全部已存在，SEPA 2c 先例已定模式），暂缓只延后一致体验且让「仅清除结果」菜单悬在中间态 |

## 14. 用户仲裁记录（2026-09-05，设计确认 + 6 问全定案）

| # | 问题 | 用户选择 | 设计影响 |
|---|---|---|---|
| 整体 | 方案批准？ | **批准** | 进入 PRE-IMPLEMENTATION GATE 后续（plan → RED 测试 → 实现） |
| Q1 | 图表 workspace 是否拆出 Market/Sepa | **拆出**（→ SEPA 复盘 workspace，推荐采纳） | §5.1 组成不变；SEPA 复盘 = SEPA + 大盘 + Logger |
| Q2 | 主题/语言入口 | **Topbar 最右段**（推荐采纳） | §7.1 |
| Q3 | Fetch 按钮去向 | **Chart header 右端**（推荐采纳） | §6 Chart 行 |
| Q4 | Chart Sidebar 默认态 | **默认显示**（推荐采纳） | §6 Chart 行、§8 default_visible=true |
| Q5 | 选股 workspace 是否含 Logger | **两处都含 Logger**（推翻推荐：推荐选股无 Logger） | §5.2 选股 workspace 底部 + Logger；SEPA 复盘同含 |
| Q6 | Watchlist 位置 | **左侧 tab、无固定侧栏宽度**（用户自定义，推翻两个推荐选项） | §5.1：左侧独立 dock leaf tab，宽度由 dock split + 拖拽 + 持久化决定，不设硬编码默认宽 |
| P1 | header 编辑器切换器 vs dock tab | 批准整体方案即采纳推荐：**dock tab 栏 = 编辑器切换器**，Mode Toggle 紧贴 header 左端 | §6 Chart 行 |

---

**设计摘要（给主 agent 汇报用）**：方案把 Compass 当前「全局 Toolbar + 全局左栏 + 5 面板各自为政」重构为 Blender 五层（Application→Workspace→Screen→Areas→Editor）：
- **核心设计**：egui_dock 0.20 保留为 Area 引擎（serde feature 已具备，DockState=bScreen 顶点图）；EditorKind 静态注册表 + EditorLayout 组件规范（Header 必备/Sidebar/Toolbar 槽位，空槽零渲染）；EditorFrame 统一 chrome，TabViewer 16 字段借用束收敛为 EditorCtx；每 Kind 单实例（句柄化预留多实例升级）；三个内置 Workspace（图表=自选+Chart+日志 / 选股=Screener+条件构建器 Sidebar+日志 / SEPA 复盘=SEPA+大盘+日志）。
- **交互亮点**：workspace 顶部 Segmented 切换 + DockStateChange 即时持久化（`[layout]` 节 + dock_version 回退）；N 键按聚焦编辑器切换 Sidebar（ctx.input 轮询 + 文本焦点守卫 + focused_leaf/on_tab_button 缓存）；Chart header 承载 Mode Toggle（周期/复权）→ 指标 → Fetch，不建 Toolbar（空槽表达）。
- **约束**：全文遵守 handoff 8 条锁定决策；5 个映射歧义/决策缝隙（P1-P5，无硬冲突）已给推荐——**2026-09-05 用户仲裁全部确认**（整体批准 + Q1-Q6 定案 + P1 采纳推荐）。
- **开放问题**：无（Q1-Q6 + P1 已全部定案，见 §14；2b 评审两处勘误已裁决，见 §4.1/§6/§13；
  2026-09-05 ⋮ 菜单两项裁决（Market ⋮ 内容 + Screener ⋮ 补「重置排序」）已落地，见 §6/§13）；
  **决策记录** 20 行（§13，含 2026-09-05 2b 评审新增 2 行、2c 裁决新增 1 行、⋮ 菜单裁决新增 2 行）。
