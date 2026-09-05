# Plan — Blender 式编辑器架构重构（issue #357）

> **本文件为重构后的实施计划 v2（重出稿）**。v1 仅作参考基线；本计划以
> `.dsh/designs/editor-architecture.md`（设计 v2，2026-09-05 用户仲裁后定稿）为**最终权威**，
> 逐条呼应设计 §10 迁移路径与 §11 测试锚点。
>
> - **Issue**: https://github.com/qiboda/compass/issues/357（OPEN，A-GUI / C-Feature / D-Complex / P-High）
> - **Worktree**: `.worktrees/editor-architecture`（分支 `feat/editor-architecture`，base = master @ `0182dcb` = Merge #356，`git rev-parse HEAD` 与 `origin/master` 一致，已同步）
> - **设计**: `.dsh/designs/editor-architecture.md` v2（用户 2026-09-05 批准 + Q1-Q6 + P1 全部仲裁定案，§13 决策记录 15 行）
> - **状态**: 🟡 plan 待用户批准 → 批准后进入 RED 测试门禁（3.5 → 4）

---

## 0. 背景与契约

### 0.1 本 plan 的契约不变式（F4 scope fidelity 的核对基准）

来自 handoff 的 **8 条锁定决策**（grill-me 14 题全按推荐）：

| # | 决策 | 出处 |
|---|---|---|
| D1 | 渐进映射：保留 egui_dock 0.20 作 Area 引擎（DockState 树 = bScreen 顶点图）；Editor = 类型注册表 + 实例状态；EditorLayout 组件（Header 必备，Sidebar/Toolbar 按角色，空槽零渲染） | handoff:13 |
| D2 | Workspace：3 内置（图表默认/选股/SEPA 复盘）；顶部 Tab 切换；布局持久化 | handoff:14 |
| D3 | Screen：每 Workspace 一个 Screen-layout（`layouts: Vec<ScreenLayout>` 预留，初始 1 个，不做多 layout UI） | handoff:15 |
| D4 | Application（window 级）= 窗口容器 + Topbar/StatusBar 两个 global area + Workspace 集合 + 全局状态；**不引入窗口级侧栏/工具条** | handoff:16 |
| D5 | 自选股 = 独立「标的自选编辑器」（Outliner 类比），不再是全局左栏 | handoff:17 |
| D6 | 全局 Toolbar 拆解：标的搜索→Topbar；周期/复权/指标→Chart header（Mode Toggle 紧贴切换器）；主题/语言→Topbar 最右；侧栏开关→各编辑器 N 键 | handoff:18 + 仲裁 Q2/Q3 |
| D7 | Editor 内部：Chart header 切换器→Mode Toggle→指标→Display Options；Chart Sidebar = 指标参数+图层设置（N 键）；不建 Chart Toolbar（空槽）；Screener/SEPA/Market/Logger ≥ Header+Main，Screener 条件构建可上 Sidebar | handoff:19-23 |
| D8 | 持久化：config.toml `[layout]` 节：`active_workspace` + 每 Workspace 的 egui_dock 树 serde + 面板宽度；启动恢复 + 切换即时保存 | handoff:24 |

**用户仲裁定案（2026-09-05，设计 §14，plan 逐条呼应）**：

| # | 定案 | 影响点（设计章节） |
|---|---|---|
| Q1 | Market/Sepa **拆出**「图表」workspace，归「SEPA 复盘」workspace | §5.1/§5.3 |
| Q2 | 主题/语言入口在 **Topbar 最右段** | §7.1 |
| Q3 | Fetch 按钮在 **Chart header 右端** | §6 Chart 行 |
| Q4 | Chart Sidebar **默认显示**（`default_visible=true`） | §6/§8.2 |
| Q5 | **两处（选股 + SEPA 复盘）都含底部 Logger**（推翻原推荐） | §5.2/§5.3 |
| Q6 | Watchlist＝图表 workspace 左**独立 dock leaf tab**、**无固定宽度**（非 240px 硬编码、非主区叠 tab），宽度由 dock split 持久化 | §5.1/§9.1 |
| P1 | dock tab 栏＝编辑器切换器；Mode Toggle 紧贴 header 左端（header 内不做重复切换器） | §6 Chart 行 |

### 0.2 范围外声明（本轮不做，F4 核对）

多屏幕 UI、浮窗/多窗口、编辑器多实例 UI、keymap 重绑定系统、自研分区引擎、编辑器内部状态持久化
（builder_root、TOP-N、排序等——仅 DockState 树 + 宽度持久化，设计 §13「编辑器内部状态持久化范围」行）。

### 0.3 澄清问题与假设

**聚焦澄清问题：无阻塞性问题。** 设计 v2 契约完整（§1-§14 全覆盖、仲裁 6 项闭环、开放问题=无）。
以下 4 项为**实现路径级假设**（不改变设计契约；若与主 agent/用户理解不符须在计划批准时指出）：

- **A1（重要）**：设计 §9.2 称 "`DockArea::show_inside` 返回 `Option<DockStateChange>`"——**已核实 egui_dock 0.20.1 源码无此类型**（`widgets/dock_area/show/mod.rs:23` `show_inside` 返回 `()`；全 crate `grep DockStateChange` 零命中；0.20.1 仅有 `TabViewer::on_close`/`on_tab_button` 回调）。**设计契约不变**（"任意布局改动 → 立即写盘"），实现路径改为：**脏标记 + 帧末比对**—— `on_tab_button`/`on_close` 置脏 + `ui.input(|i| i.pointer.any_released())` 作为拖拽移动的启发式置脏，帧末对当前 DockState 序列化指纹与上次保存值比对，不同才写盘（写盘频率 = 用户拖拽频率，与设计意图一致）。
- **A2**：Watchlist 左侧 leaf 的默认 split 比例取 **0.75**（设计 §5.1 以 `Tree::split_right(root, 0.75, …)` 描述、方向措辞含混；按「Watchlist 在左、Chart 主区占 0.75」的语义定案——egui_dock 0.20.1 中**新 leaf 在左**需用 `Tree::split_left(root, 0.75, [Watchlist])`（split_left: 新节点置于旧节点左侧，旧节点占 fraction），已核实 `dock_state/tree/mod.rs:394-431`。设计契约「Watchlist 左、无固定宽度」不变）。
- **A3**：`Tab.kind` 载荷从 `TabKind` 切换为 `EditorKind`（设计 §4.4 类型草案 `Tab = { kind: EditorKind }`、§6 Watchlist 行要求第 6 种 kind——TabKind 无 Watchlist 变体，不切换则 2f 无法落地）。`TabKind` 在阶段 2 收尾删除（F6 硬耦合解药；设计 §10 阶段 1 "TabKind 保留" 按过渡语义理解）；若删除导致既有测试锚点成本爆炸，允许降级为 editor/ 内部过渡枚举（不暴露、不持久化、F4 记录偏差）。
- **A4**：设计 §2 F9 记 SharedState "20 个 Dynamic 字段"——**实为 24 个**（`state.rs:11-66`：symbol/timeframe/adjust/bars/loading/error/log/screener_result/screener_total/screener_loading/screener_error/llm_loading/llm_error/llm_result/llm_input/llm_seq/sepa_data/sepa_loading/sepa_error/index_snapshot/index_snapshot_loading/index_snapshot_error/industry_names/watchlist）。本 plan 以源码为准（24），不影响任何设计决策。

### 0.4 现状关键事实（写入 plan 前逐条核实）

- 初始 DockState：`main.rs:156-174`（Chart/Market/Sepa 顶 leaf → `split_below(root, 0.75, [Logger])` → `split_below(root, 0.5, [Screener])`）。
- CompassApp 字段 `main.rs:817-876`：`sidebar_visible`(:858-859)、`sidebar_search`(:861)、`symbol_input_id`(:865)；`Panel::top("toolbar")`(:900)、`Panel::left("sidebar")` default 240/range 200-320(:905-913)、`Panel::bottom("statusbar")`(:916)、CentralPanel + DockArea + **16 字段 TabViewer 内联装配**(:920-944)。
- `handle_shortcuts` `main.rs:1073-1107`：`/`(symbol_input_id)、Ctrl+Enter(fetch)、Ctrl+K(sidebar 搜索)、1/2/3(timeframe)，`editing_text` 守卫(:1077)。
- `render_sidebar` `main.rs:1222-1272`（Sidebar widget + Select/Add/DeleteRequest 事件）；`add_to_watchlist`/`remove_from_watchlist` :1274-1308（save_watchlist_config + toast）；`request_watchlist_removal` modal :1310-1333。
- `render_toolbar` `main.rs:1397-1544`：Group A 标的选择器(:1403-1406) / Group B 周期+复权(:1411-1436，指数隐藏逻辑 `is_index_or_board` :1211-1217) / Group C Fetch(:1439-1456) / Group D 侧栏开关+主题+语言(:1459-1525)；loading→success / error→toast 转移逻辑 :1528-1543。
- `tabs.rs`：`TabKind`(:59-99，`citizen_id()` 1:1 硬耦合)、`Tab`(:109-129，**无 serde derive**)、`TabViewer`(:142-160，16 字段)、`ui()` 5 分支 match(:169-194)、`on_tab_button`→dispatcher.activate(:196-200)。
- `state.rs:11-66`：SharedState 24 个 Dynamic 字段；`state.rs:74-101` new() 初始化。
- `dispatcher.rs:30-46` register_citizens 5 硬编码 ID；`dispatcher.rs:100-108` dispatch_symbol_fetch（SEPA/大盘/选股行点击共享「单一当前标的」语义——否定多实例图表）。
- `dock_style.rs:36-42`：注释明示「每 leaf 单 tab，EVERY tab active」（F8，本轮保留）。
- `citizens/logger.rs:45-60`：SectionTitle（标题+计数+导出按钮）→ 返回 bool export_clicked。
- `citizens/sepa.rs:34`：`DETAIL_PANEL_WIDTH: f32 = 280.0`（body 内嵌右栏，非 Sidebar）。
- `citizens/screener.rs:176-…`：show(7 参数) 内含 `condition_builder` + 运行按钮（builder_root 状态在 screener.rs:89-100 附近）。
- `citizens/chart.rs:83`：show(ui, state, app_theme)，registry + cache_key(:20-33)；`indicators.rs` MaBollIndicator 固定 MA(5/10/60/120/250)+BOLL(20,2.0)，`visible: bool` 字段已存在但**无 UI 开关**；compass-core `ma(values, n)`/`bollinger(closes, 20, 2.0)` 已参数化（参数编辑可行性已核实）。
- egui_dock 0.20.1（已核实 registry 源码）：serde feature 存在（`serde = ["dep:serde", "egui/serde"]`）；`DockState` derive `Serialize/Deserialize`（`dock_state/mod.rs:44`，translations `serde(skip)` :50）；`NodePath`/`TabPath` 均带 serde derive（`node_index.rs:101-103`、`tab_index.rs:16-18`）；`focused_leaf()` :370；`set_active_tab` :237；`split_below/split_left/split_right` :380/:394/:430。
- 配置读写：`FullConfig`(main.rs:281-289) = flatten app + screener + watchlist + llm（仅 Deserialize）；`load_config` :407-432（**配置永远不阻止启动**）；`save_*_config` 4 个 read-modify-write 先例 :554-675（toml::Value 重写，注释丢失为已接受取舍）；TOML 内嵌 JSON 先例 `[screener] filter` :303-312。
- i18n：`crates/compass-i18n/locales/{zh,en}.yml`（rust-i18n）；键对称性测试（KEY_TREE，compass-i18n/src:598）；现有 `tab.*`/`toolbar.*`/`sidebar.*`/`statusbar.*` 键。
- 测试基建：`egui_kittest::Harness::new_ui`（纯 CPU）/`new_eframe`（完整 App，main.rs:2464 先例）；`harness.key_press`（ui_fixes_218.rs:220-237 先例）；egui_dock tab 无 AccessKit label → 程序化 `set_active_tab`（testing.md:383）；render 断言 `response.rect` 优于字段断言（testing.md:415-420）；动画用 `ctx.input(|i| i.time)` + `step_dt`（testing.md:370-382）。
- 覆盖率：workspace 93% 总门槛、compass crate 90%（scripts/check-coverage.sh 内置阈值表）。
- evidence 既有命名先例：`.dsh/evidence/F1-plan-compliance-*.txt` / `F2-code-quality-*.txt` / `F3-coverage-*.txt` / `F4-scope-fidelity-*.txt`。

---

## 1. 阶段总览

| 阶段 | 内容 | commit 数（估） | 依赖 | 门禁 | 设计章节 |
|---|---|---|---|---|---|
| **0** | 类型骨架与 serde feature（egui_dock serde + editor/ 模块纯类型 + Tab/TabKind serde derive + 组 A 单测，零 UI 改动） | 1-2 | — | cargo check/test 全绿（现有测试无行为变化） | §10 阶段 0 / §11 组 A |
| **1** | 映射骨架（`From<TabKind> for EditorKind` + descriptor 引用 + `default_layout(id)` 抽离 + 映射/完整性单测，不接 UI） | 1 | 阶段 0 | cargo test 全绿 | §10 阶段 1 / §11 组 A |
| **2** | 逐编辑器迁移（2a Chart / 2b Screener / 2c SEPA / 2d Market / 2e Logger / 2f Watchlist；每子步独立 commit 且**独立可编译**） | 6（2f 前含 Tab.kind→EditorKind 切换） | 阶段 1 | 每子步 kittest 更新 + 新断言；run.sh 截图（多模态 + 像素/形状交叉验证） | §10 阶段 2 / §6 / §11 组 D/E |
| **3** | Workspace 容器与切换（Workspaces 接入 CompassApp；Topbar 改造：workspace Segmented + 标的选择器 + ⋮添加编辑器 + 主题/语言最右；EditorCtx 收敛 TabViewer 借用束） | 2-3 | 阶段 2 | 组 B kittest 绿；手工冒烟 3 workspace 切换 | §10 阶段 3 / §4.2 / §4.4 / §7.1 / §11 组 B |
| **4** | 持久化（`[layout]` 节读写 + 启动恢复 + 切换/布局变动即时保存 + dock_version 回退） | 2 | 阶段 3 | 组 C round-trip/回退测试绿；手改坏 config 启动回退不 panic | §9 / §10 阶段 4 / §11 组 C |
| **5** | 收尾（快捷键上下文化 + N 键全链路 + 新 i18n 键 + doc-sync + 决策记录核对） | 2 | 阶段 4 | 组 D kittest 绿；`just check` 全绿；覆盖率达门槛 | §8 / §7 / §10 阶段 5 / §11 组 D/E |

**每个 commit message 独立成行 `ref #357`**；阶段 2 子步同 issue（同一 epic/PR 内）。
**每个 commit 独立可编译**（`cargo check` 过，`cargo test` 相关测试绿）。

---

## 2. 阶段 0 — 类型骨架与 serde feature（设计 §10 阶段 0，零 UI 行为变化）

### 2.1 变更面

- `crates/compass/Cargo.toml`：`egui_dock = { version = "0.20", features = ["serde"] }`（打开 serde feature——已核实 0.20.1 提供，**不引新依赖**）。
- 新建 `crates/compass/src/editor/` 模块（纯类型 + serde，零 UI）：
  - `EditorKind`（6 变体 Chart/Screener/Sepa/Market/Logger/Watchlist，serde snake_case 字符串 `"chart"`…）；
    `title_key()`/`icon()`（照设计 §4.1；Watchlist 图标用 LIST_STAR、标题键 `editor.watchlist`）。
  - `EditorLayout { header, sidebar: Option<SidebarLayout>, toolbar: Option<ToolbarLayout> }`（§4.1；HeaderLayout 恒有，`SidebarLayout { default_visible, default_width: 240.0, width_range: 200.0..=320.0 }` 仅适用于注册 Sidebar 的 Chart/Screener）。
  - `EditorDescriptor` + `static EDITOR_REGISTRY: [EditorDescriptor; 6]`（§4.1）。
  - `WorkspaceId`（serde snake_case：chart/screener/sepa）、`ScreenLayout { dock_state: DockState<Tab>, active_tab: Option<TabPath> }`、`Workspace { id, layouts: Vec<ScreenLayout>, active_screen }`、`Workspaces { all, active }` + `default_layout(id)` 签名（§4.4；**内容在阶段 1 从 main.rs:156-174 抽离**）。
  - `EditorView` trait（`kind()`/`header()`/`body()`/`sidebar()`，§4.2）、`EditorCtx`（§4.2 字段束）、`EditorFrame`（§4.3 chrome 包装壳）、`EditorInstances`（§4.5，6 个实例字段 + `get_mut(kind)` 分发签名）。
- `tabs.rs`：`Tab`/`TabKind` 补 `Serialize, Deserialize` derive（设计 §11 组 C 明确要求；为阶段 4 + 阶段 2 切换铺路）。**Tab.kind 仍为 TabKind，行为零变化**。
- `main.rs`：`mod editor;` 挂载；CompassApp **不接线**（本阶段不增字段）。

### 2.2 测试（设计 §11 组 A 先行）

- `EDITOR_REGISTRY` 完整性：6 kind 各恰 1 条；Header 恒存在；sidebar/toolbar 与预期角色一致（Chart/Screener `sidebar: Some`，其余 None；全部 `toolbar: None`）；title_key/icon 非空且 title_key 在 i18n 字典存在（`LANG_LOCK` 串行模式，tabs.rs:210-217 先例）。
- `EditorKind`/`WorkspaceId` serde round-trip（snake_case）+ 未知字符串回退（对齐 `adjust_index_from_value` 回退测试风格 main.rs:1794-1825）。
- `Workspaces::default_layout` 三树结构断言（此阶段先以签名 stub 编排，真实树在阶段 1 落地后补断言——见 §3.2）。
- 既有 kittest 全绿（无行为变化）。

### 2.3 验收点

`cargo check` + `cargo test` 全绿；`cargo clippy` 无新 warn；无 UI 行为变化（kittest 无断言改动）。
**此 commit 即「首个可编译接口 commit」**——若 3.5 对抗性测试当时 DEFERRED，此后携带 SHA 重新委派。

### 2.4 风险与回退

- 风险：serde derive 给 DockState 链路引入编译错误（Tab 未 derive / TabKind 未 derive）——阶段 0 内同 commit 修复即可，无用户可见影响。
- 回退：本阶段纯增量，`git revert` 单 commit 即回退。

---

## 3. 阶段 1 — 映射骨架（设计 §10 阶段 1，不迁 UI）

### 3.1 变更面

- `impl From<TabKind> for EditorKind`（全 5 变体 + Watchlist 无对应——Watchlist 不经 From 构造，直接 `EditorKind::Watchlist`）。
- `EDITOR_REGISTRY` 各 descriptor 引用既有 `TabKind::title/icon` 语义（title_key 复用 `tab.*` 键或新 `editor.*` 键——**采用新 `editor.*` 键树**，与设计 §6 标题键一致；`tab.*` 键阶段 2 逐步退役，避免双键维护）。
- `Workspaces::default_layout(id)`：从 `main.rs:156-174` 抽离为**按 id 的纯函数**（只搬逻辑不接 UI；main.rs 仍用旧的内联 DockState，直到阶段 3 切换）。
  - Chart：`DockState::new([Chart])` → `split_left(root, 0.75, [Watchlist])`（Watchlist 左，§5.1/Q6/假设 A2）→ `split_below(root, 0.75, [Logger])`（Logger 全宽底部，§5.1）。
  - Screener：`DockState::new([Screener])` → `split_below(root, 0.75, [Logger])`（§5.2/Q5）。
  - Sepa：`DockState::new([Sepa, Market])` → `split_below(root, 0.75, [Logger])`（§5.3/Q1/Q5）。
- `Workspaces::switch(target)` 签名 + 语义骨架（保存当前 DockState → 换 active → 挂载——阶段 3 接线）。

### 3.2 测试（设计 §11 组 A 扩展）

- 映射全变体覆盖（`From<TabKind>` 5/5）；descriptor 与 i18n 字典一致性（LANG_LOCK）。
- `default_layout` 三树结构断言：split 层级、tab 序列、宽度比例；**Chart workspace 断言 Watchlist leaf 在左、Chart 主 leaf、Logger 底部**（设计 §11 组 A 原文）。
- `Workspaces::switch` 纯逻辑单测（active index 变化 + 保存触发 mark，不接 UI）。

### 3.3 验收点

`cargo test` 全绿（新增单测 + 既有全绿）；无 kittest 断言改动（无 UI 行为变化）。

### 3.4 风险与回退

- 风险：default_layout 三树结构断言与 egui_dock split 语义（fraction 方向）不符——A2 已按源码语义定方向；若断言暴露偏差，修函数不修契约。
- 回退：单 commit 回退；阶段 1 不接 UI，回退无用户可见影响。

---

## 4. 阶段 2 — 逐编辑器迁移（设计 §10 阶段 2；2a-2f 每子步独立 commit）

### 通用约定

- 每个子步 **commit 后** 委派 `subagent_review`（P0-P1 清零，最多 2 轮）。
- 每子步更新既有 kittest（label 断言改动）+ 新增断言（label 存在性 / 渲染 rect）；`scripts/run.sh` 截图做**多模态视觉 + 形状/像素交叉验证**（禁单一目测，AGENTS.md 品质准则）。
- 每子步验证点含：**对应编辑器语义不丢**（如 SEPA 垂直堆叠 ref #221 断言、dispatch_symbol_fetch 联动、watchlist 增删 toast）。

### 4.0 前置小步（并入 2a commit）— Tab.kind 载荷切换（假设 A3）

- `Tab.kind: TabKind` → `EditorKind`；`Tab::title/icon/citizen_id` 委托 `EditorKind` 对应方法；
  `EditorKind::citizen_id() -> Option<CitizenId>`（5 个现有 kind → Some，Watchlist → None——Watchlist 不是 1:1 citizen，见 §4.6）。
- `TabViewer::ui` 的 5 分支 match 改为 `EditorKind` 分支（映射表 + Watchlist 占位分支）；`tabs.rs` 既有 `TabKind` 测试迁移到 `EditorKind`。
- **`TabKind` 删除**（F6 解药；若删除致测试成本爆炸按 A3 降级保留为内部过渡枚举）。
- 验证：`cargo test` 全绿（纯迁移，无行为变化）+ 既有 kittest 绿。

### 4.1 2a — Chart（设计 §6 Chart 行；commit 1）

**改动**：
- `render_toolbar` Group B（周期 Segmented + 复权 Dropdown，main.rs:1411-1436）与 Group C（Fetch，main.rs:1439-1456）**移入 Chart 编辑器 header**；header 左端 Mode Toggle（`1d|1w|1M` + 复权``Dropdown`，`is_index_or_board` 隐藏逻辑 main.rs:1211-1217 随迁）→ 指标切换 Dropdown（MA/BOLL 显隐 + 参数入口，`id_salt("indicators")`，per §6）→ 右端 Fetch（Primary + loading，per Q3）+ Display Options `⋮` 菜单（十字准线/交易量/图例开关 + N 键提示，per §6）。
- `render_toolbar` 移除 Group B/C（Group A 标的选择器留 Topbar——阶段 3 才动 Topbar，2a 阶段 Group A 仍在 toolbar 渲染）；Group D 中**侧栏开关**（main.rs:1459-1466）随 Chart sidebar 语义迁走；主题/语言（main.rs:1467-1524）本阶段留在 Group D（阶段 3 迁 Topbar 最右）。
- Chart 编辑器实现 `EditorView`：`header` 渲染上述控件（时间框架/复权/Fetch 动作经 EditorCtx 通道回 CompassApp 既有 `set_timeframe`/`set_adjust`/`fetch_bars`）；`sidebar` 渲染指标参数（MA 周期编辑、BOLL 参数——**参数化 MaBollIndicator**：将固定 5/10/60/120/250 + 20/2.0 改为实例字段，compass-core `ma(values,n)`/`bollinger(closes,20,2.0)` 已参数化，无需改 vendored）+图层设置（K 线样式/成交量子开关，per §6）；默认显示 `default_visible=true`（Q4）。
- Screener 布局函数（阶段 1 产的 default_layout）相应调整——**不适用**：2a 只动 Chart 编辑器自身，default_layout 在阶段 3 才接 UI。

**删旧**：`main.rs` Group B/C 代码、Chart 相关 toolbar 控件。

**验证点**：kittest——Chart header 控件 label 可查询（`1d/1w/1M`、Fetch）；Fetch 点击仍触发 fetch（loading 状态转移）；指标 Dropdown 显隐切换生效；sidebar 默认可见（指标参数 label 可查询，组 D 语义预置）；既有 dispatch/toast 测试绿。

### 4.2 2b — Screener（设计 §6 Screener 行；commit 2）

- 条件构建器（`screener.rs` `condition_builder`，builder_root/builder_root_operator 状态**不动**）从主区上段迁入 **Sidebar**（N 键显隐，默认 visible=true）；「运行/清空」按钮随迁 sidebar 底部（§6）。
- header：计数标签「共 N 只」(`screener_total`) + 运行中状态 chip + 右端 `⋮`（列宽重置/清除结果）。
- Screener 实现 `EditorView`（header/sidebar/body 拆分；body 保留结果 DataTable 6 列语义）。
- 验证点：kittest——sidebar 条件构建器 label 可查询 + N 键语义的 sidebar 显隐（组 D 预置）；结果表头存在；运行按钮迁移后仍触发（screener_loading 转移）。

### 4.3 2c — SEPA（设计 §6 SEPA 行；commit 3）

- 「② 工具条」→ header：计数「共 N 行 · 日期」+ `Segmented [TOP 50, TOP 30]` + 刷新（Primary + spinner，手动）；右端 `⋮`。
- **详情面板 280px 仍为 body 内部右栏**（`sepa.rs:34` 布局与垂直堆叠约束 ref #221 不变；不注册 Sidebar、无 N 键）。
- SEPA 实现 `EditorView`（header + body；body 内温度计 Card → 12 列表格 → 详情右栏）。
- 验证点：kittest——header 计数/TOP Segmented/刷新 label 存在；表格垂直堆叠 ref #221 既有断言绿（组 E）。

### 4.4 2d — Market（设计 §6 Market 行；commit 4）

- 「② 工具条」→ header：计数 + `Segmented [行业板块|官方指数]` + 刷新；右端 `⋮`。
- 本地副本策略不变（切段不清 shared_state；`index_snapshot` 响应式语义不动）。
- Market 实现 `EditorView`。
- 验证点：kittest header label 存在 + 既有 market 渲染测试绿。

### 4.5 2e — Logger（设计 §6 Logger 行；commit 5）

- `SectionTitle`（标题+计数+导出按钮，logger.rs:45-60）→ EditorFrame Header 槽：`[日志] · N 条` 左 + `[导出]` 右。
- `export_clicked` out-param 语义保留（EditorCtx 通道 → file_dialog.save_file 流程 main.rs:950-957 不动）。
- Logger 实现 `EditorView`（header + body；body 为 egui_lens 日志视图）。
- 验证点：kittest——`[日志]` header 存在；导出按钮点击 → 仍走 save-file 流程（既有 logger 测试绿）。

### 4.6 2f — Watchlist（设计 §6 Watchlist 行 / §5.1 / Q6；commit 6）

- `render_sidebar`（main.rs:1222-1272）→ 新 `WatchlistEditor`：header = 搜索框（Ctrl+K 聚焦语义自 main.rs:1094-1097 迁来）+ `[添加]` IconButton（main.rs:1266 语义）；body = 自选列表（复用 compass-ui `Sidebar` widget，groups=[自选]）。
- **删除** `Panel::left("sidebar")`（main.rs:905-913）+ `sidebar_visible`（:858-859）+ `sidebar_search`（:861）迁移入 WatchlistEditor 实例状态；`request_watchlist_removal` modal 流程（:1310-1333）保留（App 级，经 EditorCtx 事件通道）。
- Chart workspace default_layout（阶段 1 产物）接入：Watchlist 为**左侧独立 dock leaf tab、无固定宽度**（Q6，A2 语义）；其宽度由 dock split 比例 + 用户拖拽 + 阶段 4 持久化决定；**不注册 N 键、不入 SidebarLayout 规则**（P5）。
- `register_citizens`（dispatcher.rs:30-46）**不加** Watchlist citizen（非 one-hot 成员；`EditorKind::citizen_id()` 返回 None，`on_tab_button` 对其跳过 activate）。
- 验证点：kittest——Watchlist header 搜索框/添加按钮可查询；左 leaf 存在且**无固定宽度断言**（断言 split 层级而非像素宽，Q6）；增删自选走既有 shared_state.watchlist + save_watchlist_config 链路（toast 断言绿）。
- 阶段 2 收尾：TabKind 若按 A3 完整删除，此 commit 确认 `tabs.rs` 无 TabKind 残引；cargo test 全绿。

### 4.7 阶段 2 风险与回退

- **最大结构风险：借用重构**（TabViewer 16 字段 → 逐编辑器 EditorView 分拆时，Chart header 需 App 级 `timeframe_index`/`adjust_index`/`fetch_bars` 通道）——EditorCtx 在 2a 即引入（阶段 3 收敛），2a 先以最小 ctx（shared_state/theme/signals/toasts）跑通，App 级动作经 ctx 回调/事件通道回传；若借用冲突，降级路径为「每编辑器一个显式参数束」过渡，**不改设计契约**（EditorView trait 形状不变，只改 ctx 组装位置）。
- 回退：每子步独立 commit → 单步 revert，不滚回已通过的编辑器。

---

## 5. 阶段 3 — Workspace 容器与切换（设计 §10 阶段 3 / §4.2 EditorCtx / §4.4 / §7.1 / §11 组 B；commit 7-9）

### 5.1 变更面

- `CompassApp` 接线（设计 §4.6 三层分组）：`dock_state` 字段 → `workspaces: Workspaces`（3 个 workspace，每 workspace 一个 ScreenLayout，初始恰 1 screen——D3）；`chart/logger/screener/sepa/market` 5 字段 → `editors: EditorInstances`（含 WatchlistEditor，§4.5）；侧栏/搜索相关字段（sidebar_visible/sidebar_search 等）按 2f 已迁移。
- `CentralPanel`（main.rs:920-944）：DockArea 挂载 **active workspace 的 `dock_state`**（`workspaces.all[active]`），`TabViewer` 收敛为「dispatcher + editors + EditorCtx 束 + logger_export_clicked out-param」的最小结构（§4.2；16 字段 → 4 字段级）。
- **Topbar 改造**（§7.1，`Panel::top` 保持 40px）：
  - 左：workspace Segmented（3 段：图表/选股/SEPA 复盘，图标 + 键化名）；
  - 中：标的选择器（Group A 迁入，stock+index 合并列表 = picker_list 不动；`/` 聚焦 `symbol_input_id` 语义保留）；
  - 右：`⋮ 添加编辑器`（列出注册表中未在屏幕上的 EditorKind，供重建已关闭 tab——§5.1「重开」入口）；
  - 最右：主题 + 语言 Dropdown（从 Group D main.rs:1467-1524 迁入，行为/持久化不变：save_theme_config/save_language_config + toast 先例）。
  - `render_toolbar` 的 Group D 残留删除；loading→success/error→toast 转移逻辑（:1528-1543）**保留**（属 App 级状态反馈，不随编辑器迁移）。
- `Workspaces::switch(target)`：保存当前 DockState（内存）→ 挂载目标 → `request_repaint()`；切换 toast **不弹**（设计 §7.1 明确）；编辑器实例不重建（跨 workspace 状态保留——screener builder_root、sepa TOP-N、watchlist）。
- fetch/行点击联动语义（dispatch_symbol_fetch → shared_state.symbol）保持全局单标的（§4.5 实例所有权）。

### 5.2 风险与回退（关键，任务硬约束）

- **EditorCtx 收敛不可行的降级路径**：若 16 字段 → 单一 EditorCtx 借用束在编译器层面不可行（如 ctx 同时持 `&mut ToastManager` 与 `&mut editors` 的别名冲突）：
  1. **首选降级**：EditorCtx 只收拢**只读引用束**（shared_state/theme/signals/screener_industries/boards）；可变引用（toasts/export_clicked/App 级动作）保留为 TabViewer 显式字段或经「事件队列」回传（帧末统一处理）。**契约不变**：EditorView trait 签名、EditorInstances::get_mut、EditorFrame 均不动——只改 TabViewer 字段组装。
  2. **声明**：此降级不改设计契约（§4.2 的 EditorCtx 本就是"字段束按需并入"的弹性类型）。**若降级需要改动 EditorView/trait 签名或 EditorKind 语义**（即必须改设计契约）→ **Stop，向用户报告**，不擅自绕过。
- 回退：阶段 3 前置 commit（workspace 容器接入）与后置 commit（EditorCtx 收敛）拆分，可独立 revert。

### 5.3 验证点（设计 §11 组 B）

- kittest：Topbar workspace Segmented（自定义组件，有 AccessKit label）→ 点击「选股」→ 断言「选股器」header 存在、（chart 特有控件如 Fetch 消失）→ 再点「SEPA 复盘」→ 断言 SEPA header 存在、`[东方SEPA|大盘]` 双 tab 相邻。
- `Workspaces::switch` 单测：active index 变化 + 保存触发 mark（阶段 4 接真写盘）。
- 手工冒烟：3 workspace 全流程切换 + 标的选择器键盘导航（↑↓/Enter）+ `/` 聚焦。

---

## 6. 阶段 4 — 持久化（设计 §9 / §10 阶段 4 / §11 组 C；commit 10-11）

### 6.1 变更面

- `FullConfig`（main.rs:281-289）增 `#[serde(default)] layout: LayoutSection`（仅 Deserialize + Default；保存走 read-modify-write）。
- `[layout]` 节（设计 §9.1 schema）：`active_workspace` + `dock_version = 1` + `[[layout.workspaces]]`（id/active_screen/dock="DockState JSON 字符串"/tab_widths 预留）。TOML 内嵌 JSON 沿用 `[screener] filter` 先例（main.rs:303-312）。
- 保存：`save_layout_config`（镜像 save_*_config 读-modify-写先例，main.rs:554-675；注释丢失为既有已接受取舍）。
- **触发**（假设 A1 修正为脏标记 + 帧末指纹比对）：
  - `Workspaces::switch` → 保存当前 workspace DockState + 即时写盘（§9.2）；
  - 布局改动：TabViewer `on_tab_button`/`on_close` 置脏 + 指针释放（`i.pointer.any_released()`）启发式置脏 → 帧末对 DockState 序列化指纹与 last-saved 比对，不同才写（频率 = 用户拖拽频率，语义与设计 §9.2「Some(_) 即触发保存」一致）。
- 启动恢复：`load_config` → `[layout]` 解析 → per-workspace `DockState::from(serde)` → `active_workspace` 挂载；缺失/损坏（坏 JSON / dock_version 不匹配 / TOML 语法错）→ `Workspaces::default()` 三默认布局 + warn（对齐「配置永远不阻止启动」原则 main.rs:407-432）。
- Watchlist 宽度**不进 `tab_widths`**（Q6：宽度 = dock 树内 split 比例持久化）；`tab_widths` 仅预留（SEPA 详情 280px 等 body 内嵌右栏，因 body 不属 DockState 持久化面——v1 面最小原则，设计 §13「编辑器内部状态持久化范围」行）。

### 6.2 测试（设计 §11 组 C）

- 构造三默认树 → `serde_json::to_string` → `from_str` → 递归断言节点结构/tab kind 序列/宽度相等（Tab/EditorKind serde）。
- 损坏 JSON / 缺 dock 键 → `default_layout` 回退 + 不 panic（对齐 load_config 回退测试）。
- TOML 层：`[layout]` 与未知节共存 read-modify-write 不丢其他节（镜像 save_theme_config 测试思路）。
- kittest 两阶段：构造 → 保存（模拟触发）→ 载入 → 树相等（程序化 set_active_tab + set_focused_node，避免 AccessKit 限制）。

### 6.3 验收点

组 C 全绿；`RUST_LOG=debug scripts/run.sh` 手动改坏 config（坏 JSON / 错 version）→ 回退默认 + warn 日志。

### 6.4 风险与回退

- **egui_dock serde 契约漂移**（跨版本序列化格式变更）→ `dock_version` 兜底（设计 §9.1）；v1 版本号升 2 时旧布局丢弃回默认（可接受，设计明确）。
- **序列化性能**：帧末指纹序列化在拖拽帧才发生（6-9 tab 小树，微秒级），无性能风险。
- 回退：`dock_version` 回退已设计；load 失败自动回默认，无启动阻塞。

---

## 7. 阶段 5 — 收尾（设计 §10 阶段 5 / §8 / §11 组 D/E；commit 12-13）

### 7.1 快捷键上下文化（设计 §8 机制 + 阶段 5）

- `1/2/3`（周期）仅**「图表」workspace + Chart 编辑器激活**时生效（active workspace + focused_leaf 判定；Screener/SEPA workspace 按 1/2/3 无操作——与设计「在选股 workspace 按 1/2/3 无意义」一致）。
- `/` 保持全局（标的选择器不变）；Ctrl+Enter 保持全局 fetch；**Ctrl+K** 语义随 2f 迁移（Watchlist 搜索聚焦；仅 Watchlist leaf 存在时生效，否则 no-op）。
- 焦点守卫（`editing_text`）沿用 main.rs:1077 模式；**重复触发**：长按 repeat 允许（toggle 语义可接受，已知取舍，设计 §8.1）。

### 7.2 N 键全链路（设计 §8）

- 并入 `handle_shortcuts` 同一路由（`ctx.input` 轮询，egui 无全局按键监听的事实约束）。
- 判定：`focused_leaf()`（egui_dock :370）优先 → fallback `last_interacted_kind`（on_tab_button 钩子更新，tabs.rs:196-200 既有钩子扩展）。
- 查 `EDITOR_REGISTRY[kind].layout.sidebar`：Some → 翻转 `sidebar_visibility: HashMap<EditorKind, bool>`（App 级，**会话级不持久化**——设计 §13「Sidebar 显隐持久化」行）；None → 无操作（不提示）。
- Chart/Screener 默认 `default_visible = true`（Q4）；Watchlist/SEPA/Market/Logger 不注册 Sidebar → N 无操作（P5）。
- Header 右端 Display Options 内「侧栏」IconButton + tooltip「显示/隐藏侧栏 (N)」保持鼠标/键盘双入口（i18n 键 `editor.toggle_sidebar`）。

### 7.3 i18n 与 doc-sync（强制清单）

- 新 i18n 键：`editor.chart/editor.watchlist/editor.screener/editor.sepa/editor.market/editor.logger/editor.toggle_sidebar/editor.add`、`workspace.chart/workspace.screener/workspace.sepa`、`layout.*`（错误/回退消息）、Display Options 菜单项键；**zh/en 键集对称**（KEY_TREE 测试，compass-i18n/src:598）；旧 `tab.*` 键退役需同步移除或保留兼容（按 §3.1 决策）。
- **doc-sync 清单**：

| 文件 | 变更 | 时点 |
|---|---|---|
| `.dsh/kb/design/ui.md` | **最终权威**：五层架构（Application/Workspace/Screen/Area/Editor）、三 workspace 设计（含 Q1/Q5/Q6 定案）、EditorLayout 规范、编辑器清单（§6）、Topbar/StatusBar（§7）、N 键（§8）、`[layout]` 持久化（§9）——替换既有「全局 Toolbar+左栏」章节 | 阶段 3-5 各关键点同步，阶段 5 定稿 |
| `.dsh/kb/user/gui.md` | 界面/控件/数据流更新：workspace 切换、编辑器 header、N 键、⋮添加编辑器、Watchlist 编辑器化 | 阶段 5 |
| `.dsh/kb/user/config.md` | `[layout]` 节完整参考（键、默认值、dock_version 语义、损坏回退） | 阶段 4 |
| `.dsh/kb/design/architecture.md` | editor/ 模块、EditorInstances/EditorCtx、DockState→ScreenLayout 映射、egui_dock serde feature | 阶段 3 |
| `.dsh/kb/design/gui-i18n.md` | 新增 `editor.*`/`workspace.*` 键树 | 阶段 5 |
| `.dsh/designs/editor-architecture.md` | 过程归档随 PR 提交（已生成） | 随 PR |
| `.dsh/kb/design/ui-widgets.md` | 如需：EditorFrame/workspace Segmented 组件模板条目 | 阶段 5 按需 |

- **决策记录核对（门禁 5c）**：`.dsh/kb/design/ui.md` 补 `## 决策记录` 章节（五层/EditorLayout/N 键/持久化各行，自包含 what+why+why-not），缺失则补齐后再继续。

### 7.4 验证点（设计 §11 组 D/E + F3）

- 组 D：Chart 编辑器发 `Key::N` → sidebar 出现/消失（渲染级 shape 或 label 查询）；文本焦点守卫（type_text 聚焦后 N 不触发）；Logger 发 N 无操作；header Display Options `⋮` 可查询；**Chart sidebar 默认可见**（Q4，kittest 新 App 默认布局后指标参数 label 可查询）。
- 组 E 回归：dispatch_symbol_fetch 系列、SEPA 表格 ref #221 断言、focused tab accent ring 渲染链（dock_style.rs:221-334）、toast/modal 虚拟时间动画（ref #168/#171）全绿。
- `just check` 全绿 + `scripts/check-coverage.sh`（compass ≥90% / workspace ≥93%）。

---

## 8. 验证与门禁（RED 委派 → 实现 → F-wave）

### 8.1 门禁顺序（AGENTS.md PRE-IMPLEMENTATION GATE）

```
plan 批准 → 3.5 对抗性测试(RED) → 4 需求测试(RED) → 阶段 0-5 实现(GREEN)
→ F-wave(F1-F4) → evidence 落盘 → skwy-reflect 反思 commit → push（用户确认后，Never auto-push）
```

### 8.2 RED 测试委派（plan 批准后立即）

| 步 | 委派对象 | 内容（注入对应 skill 方法论） | 产出/契约量 |
|---|---|---|---|
| 3.5 | `subagent_skwy_adversarial_test` | 攻击：注册表完整性（重复/缺失/非法 title_key）、serde 损坏输入回退、N 键焦点守卫与 repeat、workspace 快速切换竞态、DockState round-trip 不变量、配置即时保存与拖拽同帧、多 tab leaf 场景（F8 leaf 语义下的行为） | RED 失败输出 → 记录 SHA；**阶段 0 完成后接口契约已可编译 → 无 DEFERRED，可一次性委派**（若实现者要求更细粒度则按「首个可编译接口 commit」规则重启，规则见 AGENTS.md 3.5 步） |
| 4 | `subagent_skwy_requirement_test` | 验收契约：三 workspace 默认布局（Q1/Q5/Q6）、切换行为、EditorLayout 角色（Header 恒在/Sidebar 按注册）、Watchlist 左 leaf 无固定宽度、N 键语义（Q4/Q5/P5）、持久化 round-trip、`[layout]` 损坏回退 | RED 失败输出 → 记录 SHA |

**RED 测试先于实现阶段 0 落地**；实现每阶段后跑对应 RED → GREEN。

### 8.3 F-wave（实现收尾后一次性执行，ref #181 一致性自检）

| 项 | 内容 | 落盘 |
|---|---|---|
| **F1 合规审计** | 全部实现 commit 提交后核对 `git log 0182dcb..HEAD --format=%B` 逐条独立行 `ref #357`；**审计与 commit 数可复核**（中途不写，收尾一次写，自检 commit 计数与实际一致） | `.dsh/evidence/F1-plan-compliance-editor-architecture.txt` |
| **F2 审查** | 每批实现 commit 后 `subagent_review`（并行可多个独立审查子代理；P0-P1 清零，最多 2 轮） | 审查记录（随 commit review 纪要） |
| **F3 测试+覆盖率** | `scripts/check-coverage.sh`（workspace 93% / compass 90% 门槛）；`cargo llvm-cov nextest --json --summary-only`；**重构删旧代码的覆盖率补偿核对**：删除 render_toolbar 组/render_sidebar 等大面积代码后，用 llvm-cov 对比迁移前后 compass crate 覆盖行数，不足则补组 A/C/D 测试 | `.dsh/evidence/F3-coverage-editor-architecture.txt` |
| **F4 scope fidelity** | 对照 §0.1 契约表逐条核对（8 锁定决策 + 6 仲裁 + P1 + 范围外声明 0.2）；**A1-A4 假设是否成立、TabKind 删除/降级记录** | `.dsh/evidence/F4-scope-fidelity-editor-architecture.txt` |

### 8.4 覆盖率补偿策略（关键）

- 新模块（editor/ registry/serde/default_layout）为纯逻辑 → 组 A 单测充分覆盖（高可测）。
- Workspaces::switch / 布局改动触发 → 组 B/C kittest + 纯逻辑单测（不依赖显示服务器）。
- 若 llvm-cov 显示 compass 低于 90%：优先补 editor/ 单测 + kittest 渲染断言（Harness::new_ui 纯 CPU，CI 无头可跑），不允许以「重构无法测试」为由降门槛。

---

## 9. 提交与发布

### 9.1 commit 纪律

- 每个 commit 独立成行 `ref #357`（commit-msg hook 校验 OPEN 状态）；
- 每个 commit 独立可编译（`cargo check` 通过）——**禁止跨阶段混合 commit**；
- 阶段 2 子步每步一个 commit（2a-2f 共 6 个）+ 4.0 并入 2a；阶段 3/4/5 各 2-3 个；
- **commit → review**：每次 commit 后委派 `subagent_review`（最多 2 轮；docs/lint/typo 可跳过）；
- **commit 和 push 是两个独立操作，禁用 `&&` 串联**；**Never auto-push**——等用户明确 "push"/"推送"。

### 9.2 push 门

1. 用户确认 push → 先加载 `skwy-reflect` 写反思并 commit（ref #119：反思随同批推送，勿待合并后）;
2. `git fetch origin master` → `git log HEAD..origin/master` 非空则 `git rebase origin/master`；
3. push → 创建 PR（feat/editor-architecture → master）；
4. PR 合并后：关闭 issue #357（先 `gh issue comment 357` 追加完成 comment：实现摘要 + 验收状态 + commit 列表 + 方案偏差（A1-A4/TabKind 删除）+ 原因，**遵守 comments.md「永远追加」规范**，**收尾前逐条核实实现存在**）→ close；
5. `.dsh/evidence/` 与设计/计划文档随 PR 提交（`.gitignore` 已放行 `.dsh/`）。

### 9.3 预计 commit 清单（基线，实现时可能微调）

| # | commit | 阶段 |
|---|---|---|
| 1 | feat: editor type skeleton + egui_dock serde | 0 |
| 2 | feat: editor registry + default layout mapping | 1 |
| 3 | feat: migrate chart editor (tab kind switch + header/sidebar) | 2a |
| 4 | feat: migrate screener editor | 2b |
| 5 | feat: migrate sepa editor | 2c |
| 6 | feat: migrate market editor | 2d |
| 7 | feat: migrate logger editor | 2e |
| 8 | feat: migrate watchlist editor | 2f |
| 9 | feat: workspaces container + Topbar | 3 |
| 10 | feat: EditorCtx convergence | 3 |
| 11 | feat: layout persistence | 4 |
| 12 | feat: persisted layout restore + triggers | 4 |
| 13 | feat: contextual shortcuts + N key + i18n | 5 |
| 14 | docs: sync kb ui/gui/config/architecture | 5 |

---

## 10. 风险登记表

| # | 风险 | 概率 | 影响 | 缓解 | 回退 |
|---|---|---|---|---|---|
| R1 | **egui_dock serde 契约漂移**（0.20.1 序列化格式跨版本变化、DockState 内部字段变动） | 中 | 高（布局恢复损坏） | `dock_version` 兜底（设计 §9.1）；load 失败自动回默认 + warn；round-trip 测试锁定当前版本格式 | version 升 2 丢弃旧布局（设计已接受） |
| R2 | **设计 §9.2 DockStateChange API 不存在**（已核实 0.20.1 源码） | 确定 | 中（实现路径阻塞） | A1：脏标记 + 帧末指纹比对替代，**契约不变**（任意布局改动→写盘） | 无（替代路径已定；若用户不接受 A1，需改设计契约文档 → Stop 报告） |
| R3 | **TabViewer 借用冲突**（16 字段 → EditorCtx 收敛；2a 起 Chart header 需 App 级状态通道） | 高 | 高（阶段 2/3 阻塞） | EditorCtx 分两层（只读引用束 + 可变引用/事件队列回传）；每编辑器独立 commit 缩小爆炸半径 | 降级：显式参数束过渡，**不改设计契约**（§5.2）；若必须改 trait 签名 → Stop 报用户 |
| R4 | **持久化兼容性**（旧 config 无 `[layout]` 节） | 确定 | 低 | `#[serde(default)]` + load 回退默认布局 | 无（设计路径即兼容路径） |
| R5 | **覆盖率下降**（重构删除 render_toolbar/render_sidebar 大面积代码，compass 90% 门槛） | 中 | 高（CI 门禁卡） | F3 全覆盖核对 + 补偿策略（§8.4）；新纯逻辑模块高可测 | 补测试至达标；不允许降门槛 |
| R6 | **N 键/快捷键冲突回归**（1/2/3 上下文化波及既有行为；文本焦点误触发） | 中 | 中 | 组 D kittest（焦点守卫、无 sidebar no-op、默认可见）；既有 key_press 测试沿用（ui_fixes_218.rs:220-237） | 单 commit revert 阶段 5 快捷键 commit |
| R7 | **TabKind 删除成本**（`tabs.rs` 既有测试/测试锚点大面积引用） | 中 | 中 | A3 降级：保留为 editor/ 内部过渡枚举（不暴露、不持久化），F4 记录偏差 | 降级选项即时生效 |
| R8 | **egui_dock drag 移动的触发盲区**（0.20.1 无移动回调，只有 on_tab_button/on_close） | 中 | 中 | 指针释放启发式（`i.pointer.any_released()`）置脏 + 帧末指纹比对 | 保存遗漏仅是恢复误差（用户可重新拖拽），不破坏数据 |
| R9 | **Watchlist 宽度语义**（Q6 无固定宽度 vs 既有 240px 默认） | 低 | 低 | split_left 比例 0.75 默认 + 拖拽 + 持久化；**不写 sideber_width 类硬编码** | 无（设计已定案） |
| R10 | **workspace 快速切换竞态**（切换中 fetch/行点击回写 shared_state） | 低 | 中 | 切换为同步内存操作（无异步）；SharedState 响应式单标的语义不变；对抗性测试覆盖（§8.2） | 无（无新异步通道引入） |

---

## 附录 A — 契约对齐索引（F4 核对辅助）

| Plan 章节 | 设计章节 | 测试锚点 |
|---|---|---|
| §2 阶段 0 | §10 阶段 0、§4.1/4.2/4.4/4.5（类型）、§9.1（serde 事实） | §11 组 A |
| §3 阶段 1 | §10 阶段 1、§5.1/5.2/5.3（默认布局）、§4.4 | §11 组 A |
| §4 阶段 2 | §10 阶段 2、§6（编辑器清单）、§5.1（Q5/Q6）、§4.2（EditorView） | §11 组 D/E（逐编辑器） |
| §5 阶段 3 | §10 阶段 3、§4.2（EditorCtx）、§4.4/4.6、§7.1（Topbar） | §11 组 B |
| §6 阶段 4 | §9.1/9.2、§10 阶段 4 | §11 组 C |
| §7 阶段 5 | §8.1/8.2、§7.2（StatusBar 不变）、§10 阶段 5 | §11 组 D/E |

## 附录 B — 执行顺序（批准后）

1. RED：3.5 对抗性 → 4 需求测试（委派）→ 汇总失败断言（RED 先于阶段 0）
2. 阶段 0 → 1 → 2（2a-2f）→ 3 → 4 → 5，每阶段：测试绿 → commit `ref #357` → review
3. F1-F4 验证（收尾一次性）→ evidence 落盘（`.dsh/evidence/`）
4. push 前：skwy-reflect 反思 commit → rebase origin/master → push（用户确认后）→ PR → issue 收尾 #357
