//! Requirement-acceptance contract tests — issue #357「Blender 式编辑器架构
//! 重构（Workspace/Screen/Area/Editor 五层）」.
//!
//! 验收依据（权威）:
//! - `.dsh/designs/editor-architecture.md` v2（2026-09-05 用户批准 + Q1-Q6 + P1 仲裁定案）
//! - `.dsh/plans/editor-architecture.md` v2（阶段 0-5、8 锁定决策 D1-D8、A1-A4 假设）
//!
//! ## 形态说明（为何是 source-contract 集成测试）
//!
//! `compass` 是**纯 bin crate**（无 lib.rs）——集成测试无法 `use compass::…`
//! 引用内部符号（与 `tests/requirement_index_market.rs` 同一前提与先例）。
//! 因此本文件采用该先例的 **source-contract 模式**：读 crate 源码 / i18n
//! 字典文本，断言 plan/design 声明的**接口契约字符串**存在。
//!
//! - 当前（阶段 0 已落地 SHA 8a5ea19）: 阶段 0 / editor.*+workspace.*
//!   i18n 相关断言 GREEN；后续阶段断言保持 RED → 作为「对应阶段未落地」
//!   守卫；阶段落地后按其状态重新委派复核。
//! - 本文件自身**编译恒绿**——不引用任何未存在的符号，不阻塞其余测试编译。
//!
//! 字符串断言与契约措辞**逐字一致**（testing.md ref #265 自验可信度要求：
//! ① fixture/契约逐字一致；② 关键断言 mutation 验证——本文件断言均为
//! 「契约标识符存在性」，实现若改名/漏改即失败，mutation 有效）。
//!
//! ## DEFERRED 状态（阶段 1 落地更新 —— 映射骨架已落地）
//!
//! 模板 ①②（EditorKind serde / EDITOR_REGISTRY 完整性）+ 阶段 1 映射骨架
//! 已落地（`src/editor/mod.rs`：default_layout 三树 + 结构单测 + switch
//! 边界单测；`src/tabs.rs`：From<TabKind> 映射 + Tab 载荷 EditorKind +
//! TabViewer 6 分支），**不再保持 DEFERRED**。
//!
//! 仍保持 DEFERRED（逐一触发条件见文件尾部模板块，随阶段推进更新）:
//! ④ Workspaces::switch 保存 mark 语义（当前仅翻转 active index）→ 阶段 3
//! 接线；⑤ DockState<Tab> JSON round-trip + 损坏回退 → 阶段 4（load 路径；
//! 注意: round-trip 测试当前 `#[ignore]`——egui_dock#197 上游 bug
//! （Rect::NOTHING ±∞ → serde_json null），见 editor/mod.rs 测试注释，
//! 阶段 4 修复后取消 ignore 恢复）；⑥⑦⑧ kittest → 阶段 3/5。
//!
//! RED vs current code（阶段 1 后）: 阶段 0/1 契约（EditorKind /
//! EDITOR_REGISTRY / Workspaces / ScreenLayout / default_layout 三树 /
//! From 映射 / editor.* + workspace.* i18n 键）已存在且相关断言 GREEN；
//! EditorView impl（阶段 2 逐编辑器）、N 键路由（阶段 5）、[layout] 配置节
//! （阶段 4）、workspace 切换 UI（阶段 3）仍缺失 —— 其余断言保持 RED
//! 直至对应阶段落地。

use std::path::{Path, PathBuf};

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_rel(rel: &str) -> Option<String> {
    std::fs::read_to_string(crate_root().join(rel)).ok()
}

/// Compass 主 crate 源码（相对 manifest）。
const MAIN_SRC: &str = "src/main.rs";
const TABS_SRC: &str = "src/tabs.rs";
/// i18n 字典在独立 crate（相对 manifest 的 `../compass-i18n/…`，
/// 见 requirement_index_market.rs 同款路径）。
const ZH_YML: &str = "../compass-i18n/locales/zh.yml";
const EN_YML: &str = "../compass-i18n/locales/en.yml";

/// 读取 editor 模块源码（plan §2.1「新建 src/editor/ 模块」——r#mod.rs 与
/// r#rs 单文件两种形态都接受；两者皆缺 → None（RED））。
fn read_editor_mod() -> Option<String> {
    read_rel("src/editor/mod.rs").or_else(|| read_rel("src/editor.rs"))
}

/// 读取整个 src/ 目录全部 .rs 内容并拼接（跨模块断言用——EditorView
/// 实现可能按模块就近放在 citizens/、tabs.rs 等文件）。
fn read_all_src() -> String {
    let mut all = String::new();
    fn walk(dir: &Path, out: &mut String) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|e| e == "rs")
                    && let Ok(s) = std::fs::read_to_string(&p)
                {
                    out.push_str(&s);
                }
            }
        }
    }
    walk(&crate_root().join("src"), &mut all);
    all
}

// ===========================================================================
// 需求 A — 阶段 0-1: TabKind→EditorKind 全映射 + serde
// （设计 §4.1 / plan §2-§3 / plan A3：Tab.kind 载荷切换 → EditorKind）
// ===========================================================================

#[test]
fn editor_kind_enum_exists_with_six_variants() {
    // 设计 §4.1: `pub enum EditorKind { Chart, Screener, Sepa, Market,
    // Logger, Watchlist }`，6 变体；serde 持久化契约。
    let src = read_editor_mod().expect("editor 模块必须存在（plan 阶段 0）");
    assert!(
        src.contains("pub enum EditorKind"),
        "EditorKind enum must exist（设计 §4.1）"
    );
    for variant in ["Chart", "Screener", "Sepa", "Market", "Logger", "Watchlist"] {
        assert!(
            src.contains(variant),
            "EditorKind 必须含变体 {variant}（设计 §4.1）"
        );
    }
    assert!(
        src.contains("Serialize, Deserialize"),
        "EditorKind 必须 derive Serialize/Deserialize（持久化契约, 设计 §4.1）"
    );
}

#[test]
fn editor_kind_serde_uses_snake_case_strings() {
    // 设计 §4.1: serde 用 snake_case 字符串（"chart"、"watchlist"…），持久化稳定。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("snake_case") && src.contains("rename_all"),
        "EditorKind 必须 #[serde(rename_all = \"snake_case\")]（设计 §4.1 字符串持久化契约）"
    );
}

#[test]
fn tab_kind_to_editor_kind_mapping_contract() {
    // plan §3.1 + A3: 阶段 1 为 `impl From<TabKind> for EditorKind`（5 变体全映射，
    // Watchlist 不经 From）；阶段 2 收尾 TabKind 删除、Tab 载荷切为 EditorKind。
    // 两形态任一满足即通过（A3 过渡语义，F4 记录）；全部源码范围检索。
    // 限定：From<TabKind> 全 src 检索（跨文件可能）；Tab 载荷形态必须落在
    // tabs.rs（EditorDescriptor 的 `kind: EditorKind` 字段不构成映射证据，
    // 避免阶段 1 漏写 From 时假阳性，review P2-1）。
    let all = read_all_src();
    let tabs = read_rel(TABS_SRC).unwrap_or_default();
    let transitions_ok =
        all.contains("From<TabKind> for EditorKind") || tabs.contains("kind: EditorKind");
    assert!(
        transitions_ok,
        "TabKind→EditorKind 映射缺失：既无 From<TabKind>（阶段 1 形态）也无 Tab 载荷 EditorKind（阶段 2+ 形态，plan A3）"
    );
}

#[test]
fn tab_serde_derives_for_persistence() {
    // plan §2.1 阶段 0: `tabs.rs` 的 `Tab`/`TabKind` 补 `Serialize, Deserialize`
    // derive（设计 §11 组 C：DockState<Tab> 需可直接序列化）。
    let tabs = read_rel(TABS_SRC).expect("src/tabs.rs must exist");
    assert!(
        tabs.contains("Serialize, Deserialize"),
        "Tab/TabKind 必须 derive Serialize/Deserialize（plan 阶段 0 / 设计 §11 组 C）——\
         editor/mod.rs 中的 Serialize derive 不构成 Tab 持久化证据，必须限定 tabs.rs（review P2-2）"
    );
}

// ===========================================================================
// 需求 B — 阶段 0: EDITOR_REGISTRY + EditorLayout 注册契约
// （设计 §4.1/§6: Header 恒在、Sidebar/Toolbar 按角色、空槽零渲染）
// ===========================================================================

#[test]
fn editor_registry_declares_six_entries() {
    // 设计 §4.1: `pub static EDITOR_REGISTRY: [EditorDescriptor; 6]`（恰 6 条）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("EDITOR_REGISTRY"),
        "EDITOR_REGISTRY must exist（设计 §4.1）"
    );
    assert!(
        src.contains("[EditorDescriptor; 6]"),
        "EDITOR_REGISTRY 必须恰为 [EditorDescriptor; 6]（6 编辑器固定，设计 §4.1）"
    );
}

#[test]
fn editor_descriptor_fields_contract() {
    // 设计 §4.1: EditorDescriptor { kind, title_key, icon, layout }。
    let src = read_editor_mod().expect("editor 模块必须存在");
    for field in [
        "kind: EditorKind",
        "title_key",
        "icon",
        "layout: EditorLayout",
    ] {
        assert!(
            src.contains(field),
            "EditorDescriptor 必须含字段 {field}（设计 §4.1）"
        );
    }
}

#[test]
fn editor_layout_header_always_present() {
    // 设计 §4.1/§6: HeaderLayout 恒有（`header` 必备槽位，非 Option）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("pub header") && src.contains("HeaderLayout"),
        "EditorLayout.header 必须为恒有的 HeaderLayout（锁定决策 D1/D7: Header 必备）"
    );
}

#[test]
fn sidebar_only_chart_and_screener_register_sidebar() {
    // 设计 §6 清单: 仅 Chart/Screener 注册 Sidebar（`sidebar: Some(…)`），
    // 其余 4 个 = None。按 EditorLayout 空槽零渲染（D1）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    let some_count = src.matches("Some(SidebarLayout").count();
    assert!(
        some_count >= 2,
        "注册表必须至少 2 处 Some(SidebarLayout)（Chart/Screener），实际 {some_count}（设计 §6）"
    );
}

#[test]
fn toolbar_is_none_for_all_editors() {
    // 设计 §6: 本轮无任何编辑器注册 Toolbar（Chart 空槽表达 toolbar: None）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        !src.contains("Some(ToolbarLayout"),
        "本轮无编辑器注册 Toolbar（设计 §6/D7: 不建 Chart Toolbar）"
    );
    assert!(
        src.contains("toolbar: None"),
        "EditorLayout 必须显式表达 toolbar: None（空区域零渲染成本, D1）"
    );
}

#[test]
fn editor_layout_sidebar_default_width_rule() {
    // 设计 §4.1: SidebarLayout { default_visible, default_width: 240.0,
    // width_range: 200.0..=320.0 }（仅适用于注册 Sidebar 的 Chart/Screener）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("default_visible") && src.contains("default_width"),
        "SidebarLayout 必须含 default_visible/default_width（设计 §4.1）"
    );
}

// ===========================================================================
// 需求 C — 阶段 1: 三个 workspace 默认布局（设计 §5 + Q1/Q5/Q6 定案）
// ===========================================================================

#[test]
fn default_layout_function_exists() {
    // 设计 §4.4: `Workspaces::default_layout(id) -> DockState<Tab>` 按 id 构造。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("fn default_layout"),
        "default_layout(id) 按 WorkspaceId 构造默认树（设计 §4.4/§5）"
    );
}

#[test]
fn chart_workspace_tree_watchlist_left_chart_main_logger_bottom() {
    // 设计 §5.1 + Q6/A2: 图表 = Watchlist 左独立 leaf（split_left 0.75）+ Chart 主
    // + Logger 底（split_below 0.75）。split_left 语义已核实（plan A2）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("split_left"),
        "图表 workspace 必须使用 split_left 放置 Watchlist 左侧 leaf（设计 §5.1/Q6/A2）"
    );
    assert!(
        src.contains("split_below"),
        "Logger 底部 leaf 必须经 split_below（设计 §5.1）"
    );
    assert!(
        src.contains("0.75"),
        "split 比例 0.75 契约（Watchlist/Logger 默认比例，plan A2）"
    );
    assert!(
        src.contains("Watchlist") && src.contains("Logger"),
        "图表 workspace 默认树必须含 Watchlist + Logger（设计 §5.1/Q5/Q6）"
    );
}

#[test]
fn screener_workspace_tree_screener_main_logger_bottom() {
    // 设计 §5.2 + Q5: 选股 = Screener 主 + Logger 底（Q5 定案含 Logger）。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("split_below") && src.contains("Logger"),
        "选股 workspace 必须含底部 Logger leaf（Q5 定案：两处都含 Logger）"
    );
}

#[test]
fn sepa_workspace_tree_sepa_market_then_logger() {
    // 设计 §5.3 + Q1/Q5: SEPA 复盘 = [Sepa, Market] 叠主 leaf 两 tab + Logger 底。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("Sepa") && src.contains("Market"),
        "SEPA 复盘 workspace 必须含 Sepa + Market（Q1 定案：拆出归 SEPA 复盘）"
    );
    assert!(
        src.contains("split_below") && src.contains("Logger"),
        "SEPA 复盘 workspace 必须含底部 Logger leaf（Q5 定案）"
    );
}

#[test]
fn watchlist_not_sidebar_no_fixed_width() {
    // 设计 §5.1/Q6/P5: Watchlist 是「左侧独立 dock leaf tab、无固定宽度」，
    // 不进 SidebarLayout 宽度规则、不注册 Sidebar（P5: N 键仅作用注册了
    // Sidebar 的编辑器）。守卫：若实现把 Watchlist 也注册 Sidebar（匹配数
    // >2）即失败；Watchlist 经 split_left 入树由上一条断言覆盖。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.matches("Some(SidebarLayout").count() <= 2,
        "Watchlist 不得注册 Sidebar（Q6/P5：无固定宽度、无 N 键）"
    );
}

// ===========================================================================
// 需求 D — 阶段 2: 编辑器切换器（dock tab 栏 = EditorKind 切换器, P1 定案）
// ===========================================================================

#[test]
fn six_editors_implement_editor_view() {
    // 设计 §4.2/§6: 每个编辑器实现 EditorView（阶段 2 逐编辑器迁移）。
    // 实现可能按模块就近放在 citizens/ 等文件 —— 全 src/ 范围统计。
    let all = read_all_src();
    assert!(
        all.contains("trait EditorView"),
        "EditorView trait 必须存在（设计 §4.2: kind/header/body/sidebar 四方法）"
    );
    let impl_count = all.matches("impl EditorView for").count();
    assert!(
        impl_count >= 6,
        "必须至少 6 个编辑器实现 EditorView（设计 §6 清单 6 编辑器），实际 {impl_count}"
    );
}

#[test]
fn tab_viewer_dispatches_by_editor_kind() {
    // P1 定案: dock tab 栏 = 编辑器切换器；TabViewer::ui 按 EditorKind 分支分发。
    // 断言 tabs.rs 中存在 EditorKind 变体分发（而非 TabKind 残引）。
    let tabs = read_rel(TABS_SRC).expect("src/tabs.rs must exist");
    let kind_refs = [
        tabs.contains("EditorKind::Chart"),
        tabs.contains("EditorKind::Logger"),
        tabs.contains("EditorKind::Screener"),
        tabs.contains("EditorKind::Sepa"),
        tabs.contains("EditorKind::Market"),
        tabs.contains("EditorKind::Watchlist"),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    assert!(
        kind_refs >= 2,
        "TabViewer 必须按 EditorKind 分发（A3: Tab.kind 载荷切换；当前仅 {kind_refs} 个变体引用）"
    );
}

// ===========================================================================
// 需求 E — 阶段 3: Workspace 容器与切换（设计 §4.4/§7.1、plan §5）
// ===========================================================================

#[test]
fn workspace_types_exist() {
    // 设计 §4.4: WorkspaceId（serde snake_case: chart/screener/sepa）、
    // ScreenLayout { dock_state, active_tab }、Workspace { id, layouts, active_screen }、
    // Workspaces { all, active }。
    let src = read_editor_mod().expect("editor 模块必须存在");
    for item in [
        "pub enum WorkspaceId",
        "pub struct ScreenLayout",
        "pub struct Workspace",
        "pub struct Workspaces",
    ] {
        assert!(src.contains(item), "必须存在 {item}（设计 §4.4）");
    }
    assert!(
        src.contains("active_screen") && src.contains("layouts"),
        "Workspace 必须含 layouts: Vec<ScreenLayout> 与 active_screen（D3 多屏预留）"
    );
}

#[test]
fn workspace_id_serde_snake_case() {
    // 设计 §4.4: `#[serde(rename_all = "snake_case")]` 于 WorkspaceId ——
    // 持久化字符串 "chart"/"screener"/"sepa".
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("snake_case") && src.contains("rename_all"),
        "WorkspaceId 必须 #[serde(rename_all = \"snake_case\")]（设计 §4.4 持久化契约）"
    );
}

#[test]
fn workspace_switch_function_contract() {
    // 设计 §4.4/§9.2: Workspaces::switch(target)：保存当前 DockState（内存）+
    // 换 active + 触发持久化标记；plan §5.1: 切换 toast 不弹、实例不重建。
    let src = read_editor_mod().expect("editor 模块必须存在");
    assert!(
        src.contains("fn switch") && src.contains("WorkspaceId"),
        "Workspaces::switch(target) 必须存在（设计 §4.4/§9.2）"
    );
}

#[test]
fn topbar_embeds_workspace_switcher_keys() {
    // 设计 §7.1: Topbar 左 = workspace Segmented 3 段（键 workspace.chart/
    // workspace.screener/workspace.sepa）；中=标的选择器；最右=主题/语言（Q2）。
    // 键绑定落在 WorkspaceId::title_key()（editor 模块），Topbar 经
    // `w.id.title_key()` 使用——两个源文件的并集含三个键即可。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    let editor_src = read_editor_mod().expect("editor 模块必须存在");
    let key_binding_src = format!("{main}\n{editor_src}");
    assert!(
        key_binding_src.contains("workspace.chart")
            && key_binding_src.contains("workspace.screener")
            && key_binding_src.contains("workspace.sepa"),
        "Topbar workspace 切换器必须使用 workspace.* i18n 键（设计 §7.1）"
    );
    assert!(
        main.contains("Segmented"),
        "workspace 切换必须为 Segmented 3 段控件（设计 §7.1）"
    );
}

// ===========================================================================
// 需求 F — 阶段 4: [layout] 持久化（设计 §9 / plan §6 / 组 C）
// ===========================================================================

#[test]
fn layout_config_section_added_to_full_config() {
    // plan §6.1: FullConfig 增 `#[serde(default)] layout: LayoutSection`；
    // 设计 §9.1: [layout] 节 = active_workspace + dock_version + [[layout.workspaces]]。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("layout: LayoutSection") || main.contains("layout: Layout"),
        "FullConfig 必须含 layout 节（plan §6.1 阶段 4）"
    );
    assert!(
        main.contains("active_workspace") && main.contains("dock_version"),
        "[layout] 节必须含 active_workspace + dock_version（设计 §9.1 schema）"
    );
}

#[test]
fn layout_persistence_save_function_contract() {
    // plan §6.1: save_layout_config（镜像 save_*_config read-modify-write）；
    // 设计 §9.2: 切换/布局改动 → 即时写盘。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("save_layout") || main.contains("save_layout_config"),
        "必须存在 save_layout_config（plan §6.1 阶段 4）"
    );
}

#[test]
fn layout_load_fallback_to_defaults() {
    // 设计 §9.1/§9.2: 加载失败（坏 JSON / dock_version 不匹配 / TOML 语法错）
    // → Workspaces::default() 三默认布局 + warn，不 panic（配置永远不阻止启动）。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("default_layout") && main.contains("warn"),
        "[layout] 损坏必须回退 default_layout + warn（设计 §9.1, 对齐 load_config 原则）"
    );
}

// ===========================================================================
// 需求 G — 阶段 5: N 键（设计 §8 / plan §7.2）+ 快捷键上下文（plan §7.1）
// ===========================================================================

#[test]
fn n_key_routed_in_shortcut_handler() {
    // 设计 §8.1: N 键并入 handle_shortcuts 同一路由（ctx.input 轮询 +
    // key_pressed(Key::N)）；§8.2: 翻转 sidebar_visibility[HashMap<EditorKind, bool>]。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("Key::N") && main.contains("key_pressed"),
        "N 键必须经 handle_shortcuts 路由（ctx.input 轮询, 设计 §8.1）"
    );
    assert!(
        main.contains("sidebar_visibility"),
        "N 键翻转状态必须是 sidebar_visibility（HashMap<EditorKind,bool>, 设计 §8.2）"
    );
}

#[test]
fn n_key_uses_editor_registry_layout_sidebar() {
    // 设计 §8.2: 判定 = 查 EDITOR_REGISTRY[kind].layout.sidebar — Some 翻转 /
    // None 无操作（不提示）。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("EDITOR_REGISTRY") || main.contains("editor_registry"),
        "N 键判定必须经 EDITOR_REGISTRY 查 sidebar（设计 §8.2）"
    );
}

#[test]
fn timeframe_shortcuts_scoped_to_chart_workspace() {
    // plan §7.1: 1/2/3（周期）仅「图表 workspace + Chart 编辑器激活」生效；
    // Screener/SEPA workspace 按 1/2/3 无操作（上下文隔离契约）。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("Num1") && main.contains("Num2") && main.contains("Num3"),
        "1/2/3 周期快捷键路由必须保留（plan §7.1）"
    );
    assert!(
        main.contains("WorkspaceId::Chart") || main.contains("active_workspace"),
        "1/2/3 必须被 workspace 上下文判定约束（Screener/SEPA 按 1/2/3 无操作, plan §7.1）"
    );
}

#[test]
fn text_focus_guard_preserved_for_n_key() {
    // 设计 §8.1: 文本输入框聚焦时 N 不触发（editing_text 守卫，main.rs:1077 模式）。
    let main = read_rel(MAIN_SRC).expect("src/main.rs must exist");
    assert!(
        main.contains("editing_text") && main.contains("focused()"),
        "N 键必须保留文本焦点守卫（editing_text, 设计 §8.1）"
    );
}

// ===========================================================================
// 需求 H — 阶段 5: i18n 新键（设计 §7.1/§8.2、plan §7.3：zh/en 对称）
// ===========================================================================

const EDITOR_KEYS: [&str; 8] = [
    "editor.chart",
    "editor.watchlist",
    "editor.screener",
    "editor.sepa",
    "editor.market",
    "editor.logger",
    "editor.toggle_sidebar",
    "editor.add",
];

const WORKSPACE_KEYS: [&str; 3] = ["workspace.chart", "workspace.screener", "workspace.sepa"];

/// 解析 yml 键树为完整点分键集合。
///
/// 兼容项目字典的**双风格**（zh.yml 现状同时使用）：
/// - 嵌套树：`tab:` + 缩进 `  chart: 图表` → 键 `tab.chart`
/// - 扁平点分键：`tab.market: 大盘` → 键 `tab.market`
fn key_set(yml: &str) -> Vec<String> {
    let mut keys = Vec::new();
    // 缩进栈: (indent, name)；用于嵌套树的前缀重建。
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in yml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let name = trimmed
            .split_once(':')
            .map(|(n, _)| n.trim())
            .unwrap_or(trimmed);
        if name.is_empty() {
            continue;
        }
        // 退出比当前更深的缩进层级（嵌套树风格）。
        while let Some(&(i, _)) = stack.last() {
            if i >= indent {
                stack.pop();
            } else {
                break;
            }
        }
        if name.contains('.') {
            // 扁平点分键：本身就是完整键，不进栈。
            keys.push(name.to_string());
        } else {
            let full = if stack.is_empty() {
                name.to_string()
            } else {
                let mut full = stack
                    .iter()
                    .map(|(_, n)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                full.push('.');
                full.push_str(name);
                full
            };
            keys.push(full);
            stack.push((indent, name.to_string()));
        }
    }
    keys
}

#[test]
fn i18n_editor_keys_exist_in_zh() {
    let zh = read_rel(ZH_YML).expect("compass-i18n zh.yml must exist");
    let keys = key_set(&zh);
    for key in EDITOR_KEYS {
        assert!(
            keys.contains(&key.to_string()),
            "zh.yml 必须含 editor 键 {key}（plan §7.3 新 i18n 键清单）"
        );
    }
}

#[test]
fn i18n_editor_keys_exist_in_en() {
    let en = read_rel(EN_YML).expect("compass-i18n en.yml must exist");
    let keys = key_set(&en);
    for key in EDITOR_KEYS {
        assert!(
            keys.contains(&key.to_string()),
            "en.yml 必须含 editor 键 {key}（plan §7.3 新 i18n 键清单）"
        );
    }
}

#[test]
fn i18n_editor_key_sets_symmetric_zh_en() {
    // plan §7.3 + KEY_TREE 测试（compass-i18n/src:598）: zh/en 键集必须完全对称。
    let zh = read_rel(ZH_YML).expect("zh.yml must exist");
    let en = read_rel(EN_YML).expect("en.yml must exist");
    let (zh_keys, en_keys) = (key_set(&zh), key_set(&en));
    for key in EDITOR_KEYS {
        assert!(
            zh_keys.contains(&key.to_string()) && en_keys.contains(&key.to_string()),
            "editor 键 {key} 必须在 zh/en 同时存在（对称契约）"
        );
    }
}

#[test]
fn i18n_workspace_keys_zh_en_symmetric() {
    // 设计 §7.1/plan §7.3: workspace.chart/screener/sepa 三键 zh/en 对称。
    let zh = read_rel(ZH_YML).expect("zh.yml must exist");
    let en = read_rel(EN_YML).expect("en.yml must exist");
    let (zh_keys, en_keys) = (key_set(&zh), key_set(&en));
    for key in WORKSPACE_KEYS {
        assert!(
            zh_keys.contains(&key.to_string()) && en_keys.contains(&key.to_string()),
            "workspace 键 {key} 必须在 zh/en 同时存在（设计 §7.1）"
        );
    }
}

#[test]
fn i18n_layout_namespace_zh_en_symmetric() {
    // plan §7.3: `layout.*`（错误/回退消息）键树存在且 zh/en 对称。
    let zh = read_rel(ZH_YML).expect("zh.yml must exist");
    let en = read_rel(EN_YML).expect("en.yml must exist");
    let (zh_keys, en_keys) = (key_set(&zh), key_set(&en));
    let zh_layout = zh_keys.iter().any(|k| k.starts_with("layout."));
    let en_layout = en_keys.iter().any(|k| k.starts_with("layout."));
    assert!(
        zh_layout && en_layout,
        "[layout] 错误/回退消息 i18n 键树必须存在且 zh/en 对称（plan §7.3）"
    );
}

// ===========================================================================
// DEFERRED 强编译/行为级测试模板（状态随阶段推进更新）
// ===========================================================================
//
// 模板 ①② 已 LANDED（SHA 8a5ea19 阶段 0 后）—— 见 `src/editor/mod.rs`
// `#[cfg(test)] mod tests`（Phase 0 unit tests）：registry_has_exactly_six_
// entries_with_distinct_kinds / registry_header_always_present /
// registry_sidebar_only_chart_and_screener / registry_toolbar_none_for_all /
// registry_title_keys_and_icons_nonempty / registry_title_keys_exist_in_i18n /
// chart_and_screener_sidebar_spec_follows_contract /
// editor_kind_serde_roundtrip / editor_kind_unknown_string_errors_not_panics /
// editor_kind_serde_exact_snake_case_strings（二轮补）/ editor_kind_title_key_
// precise_i18n_keys（二轮补）/ editor_kind_icon_precise_glyphs（二轮补）/
// workspace_id_serde_roundtrip / default_layout_signature_contract。
//
// 以下模板仍 DEFERRED（引用未落地接口，无法编译）—— 触发条件随对应阶段
// commit 更新：
//
// 模板③ default_layout 三树结构（DockState 节点遍历）
//   触发条件：阶段 1 commit（`Workspaces::default_layout` 当前仍
//   `unimplemented!()`，editor/mod.rs:248-251）。
//   图表: split_left(Watchlist 左, 0.75) + split_below(Logger, 0.75)；
//   选股: Screener 主 + split_below(Logger, 0.75)；
//   SEPA: [Sepa, Market] 主 leaf + split_below(Logger, 0.75)。
//   遍历 API: dock_state.get_surface_mut(egui_dock::SurfaceIndex::main())
//   → node_tree 递归断言 split 方向/fraction/leaf tab 序列。
//
// 模板④ Workspaces::switch 纯逻辑单测
//   触发条件：阶段 1（保存当前 DockState 到内存 + 脏标记落地）或阶段 3
//   （switch 接 UI）；当前仅翻转 active index（editor/mod.rs:240-244），
//   保存/标记语义不可测。
//   断言: active index 变化 + 保存触发 mark。
//
// 模板⑤ DockState<Tab> JSON round-trip + 损坏回退
//   触发条件：阶段 4 load 路径（`[layout]` 反序列化 → 回退 defaults）；
//   前置已满足：`egui_dock` serde feature 已开启（Cargo.toml，对抗测试
//   phase0_manifest_enables_egui_dock_serde 已 GREEN）、Tab/TabKind 已
//   derive serde。
//   断言: 三默认树 to_string→from_str→递归节点/tab kind 序列/宽度相等；
//   损坏 JSON / 缺 dock 键 → default_layout 回退 + 不 panic。
//
// 模板⑥ workspace 切换 kittest（设计 §11 组 B）
//   触发条件：阶段 3（Workspaces 接入 UI，Topbar Segmented 生效）。
//
// 模板⑦ N 键 kittest（设计 §11 组 D）
//   触发条件：阶段 5（N 键路由 handle_shortcuts 落地）。
//
// 模板⑧ 1/2/3 上下文隔离 kittest（plan §7.1）
//   触发条件：阶段 5（workspace 上下文判定落地）。
