//! Editor architecture — Blender-style Workspace / Screen / Area / Editor
//! five-layer skeleton (issue #357).
//!
//! This module is the **type registry + instance-state contract** layer:
//! [`EditorKind`] identifies an editor type, [`EDITOR_REGISTRY`] declares its
//! layout roles (Header always, Sidebar/Toolbar per role), [`Workspaces`]
//! owns the per-workspace dock trees. It intentionally contains **zero UI
//! wiring** — the citizens keep rendering through `tabs.rs` until the
//! per-editor migration phases (plan §2).
//!
//! ## Mapping (design §3)
//!
//! - `EditorKind` + `EDITOR_REGISTRY` ≈ Blender `SpaceType` registry
//! - `EditorLayout` ≈ `ARegion` slots (header/panel/toolbar)
//! - `ScreenLayout { dock_state }` ≈ `bScreen` area vertex graph
//!   (`DockState` tree = the rectangle split graph, lock-in decision 1)
//! - `Workspace` / `WorkspaceId` / `Workspaces` ≈ `wm_workspace.h`
//! - `EditorView` + `EditorCtx` + `EditorFrame` ≈ per-SpaceData instance
//!   layer (`SpaceView3D` …), converging the `tabs.rs` 16-field borrow bomb
//!   (friction F5)
//!
//! Skeleton consumers have landed through phase 3; any new `dead_code`
//! warning here is a real one and should be handled explicitly (plan §2.3).

use egui_dock::DockState;
use egui_mobius::signals::Signal;
use serde::{Deserialize, Serialize};

use compass_core::data::symbol::{exchange_of_symbol, parse_explicit_prefix};
use compass_core::model::{IndexBasic, StockBasic};
use compass_i18n::t;
use compass_ui::widgets::sidebar::{Sidebar, SidebarEvent, SidebarGroup, SidebarItem};
use compass_ui::widgets::toast::ToastManager;

use crate::messages::{
    FetchRequest, RunIndexSnapshotRequest, RunLlmRequest, RunScreenerRequest, RunSepaRequest,
};
use crate::state::SharedState;
use crate::tabs::Tab;
use crate::theme::CompassTheme;

// ---------------------------------------------------------------------------
// EditorKind — editor type identifier (SpaceType 类比)
// ---------------------------------------------------------------------------

/// Editor type identifier (serde persists as snake_case strings:
/// `"chart"`, `"watchlist"`, … so config formats stay stable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind {
    Chart,
    Screener,
    Sepa,
    Market,
    Logger,
    Watchlist,
}

impl EditorKind {
    /// i18n key of the editor's display title (`editor.*` key tree).
    pub fn title_key(&self) -> &'static str {
        match self {
            Self::Chart => "editor.chart",
            Self::Screener => "editor.screener",
            Self::Sepa => "editor.sepa",
            Self::Market => "editor.market",
            Self::Logger => "editor.logger",
            Self::Watchlist => "editor.watchlist",
        }
    }

    /// Phosphor icon glyph shown next to the tab title (design §4.1).
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Chart => egui_phosphor::regular::CHART_LINE,
            Self::Screener => egui_phosphor::regular::FUNNEL_SIMPLE,
            Self::Sepa => egui_phosphor::regular::GAUGE,
            Self::Market => egui_phosphor::regular::TREND_UP,
            Self::Logger => egui_phosphor::regular::TERMINAL,
            Self::Watchlist => egui_phosphor::regular::LIST_STAR,
        }
    }
}

// ---------------------------------------------------------------------------
// EditorLayout — ARegion-style component slots (Header 必备 / Sidebar /
// Toolbar 按角色注册，空槽零渲染)
// ---------------------------------------------------------------------------

/// Header region spec: always present (default 32px, `bg_panel_alt`,
/// right-side Display Options slot). A unit struct in phase 0 — the chrome
/// wrapper (`EditorFrame`) owns the rendering in the migration phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderLayout;

/// Sidebar region spec. Only registered by Chart/Screener (design §6);
/// `None` elsewhere means no sidebar and the N key is a no-op (design §8.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarLayout {
    pub default_visible: bool,
    pub default_width: f32,
    pub width_range: (f32, f32),
}

/// Toolbar region spec: no editor registers one this round (design §6/D7) —
/// an empty slot allocates no area and costs zero rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarLayout;

/// Component spec for one editor type: Header mandatory, Sidebar/Toolbar per
/// role (lock-in decision 1/7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorLayout {
    pub header: HeaderLayout,
    pub sidebar: Option<SidebarLayout>,
    pub toolbar: Option<ToolbarLayout>,
}

/// Registry entry (SpaceType 类比): static & immutable.
#[derive(Debug, Clone, Copy)]
pub struct EditorDescriptor {
    pub kind: EditorKind,
    pub title_key: &'static str,
    pub icon: &'static str,
    pub layout: EditorLayout,
}

/// Static registry: adding an editor = one `EditorKind` variant + one
/// descriptor (compile-time exhaustiveness guarantee, design §4.1).
pub static EDITOR_REGISTRY: [EditorDescriptor; 6] = [
    EditorDescriptor {
        kind: EditorKind::Chart,
        title_key: "editor.chart",
        icon: egui_phosphor::regular::CHART_LINE,
        layout: EditorLayout {
            header: HeaderLayout,
            sidebar: Some(SidebarLayout {
                default_visible: true,
                default_width: 240.0,
                width_range: (200.0, 320.0),
            }),
            toolbar: None,
        },
    },
    EditorDescriptor {
        kind: EditorKind::Screener,
        title_key: "editor.screener",
        icon: egui_phosphor::regular::FUNNEL_SIMPLE,
        layout: EditorLayout {
            header: HeaderLayout,
            // Independent sidebar spec (2026-09-05 designer ruling, P1-3):
            // the condition-builder cards are atomic groups (ref #220,
            // label+control never wraps) whose widest leaf (`Momentum`)
            // needs ~470px; min = 470 + 16px panel padding = 486. Default
            // 500 is the first GROUP_ALIGNMENT_WIDTHS test anchor.
            sidebar: Some(SidebarLayout {
                default_visible: true,
                default_width: 500.0,
                width_range: (486.0, 640.0),
            }),
            toolbar: None,
        },
    },
    EditorDescriptor {
        kind: EditorKind::Sepa,
        title_key: "editor.sepa",
        icon: egui_phosphor::regular::GAUGE,
        layout: EditorLayout {
            header: HeaderLayout,
            sidebar: None,
            toolbar: None,
        },
    },
    EditorDescriptor {
        kind: EditorKind::Market,
        title_key: "editor.market",
        icon: egui_phosphor::regular::TREND_UP,
        layout: EditorLayout {
            header: HeaderLayout,
            sidebar: None,
            toolbar: None,
        },
    },
    EditorDescriptor {
        kind: EditorKind::Logger,
        title_key: "editor.logger",
        icon: egui_phosphor::regular::TERMINAL,
        layout: EditorLayout {
            header: HeaderLayout,
            sidebar: None,
            toolbar: None,
        },
    },
    EditorDescriptor {
        kind: EditorKind::Watchlist,
        title_key: "editor.watchlist",
        icon: egui_phosphor::regular::LIST_STAR,
        // Q6 定案: Watchlist is a dock leaf tab, NOT a sidebar — no fixed
        // width, no N key, split-ratio-owned width.
        layout: EditorLayout {
            header: HeaderLayout,
            sidebar: None,
            toolbar: None,
        },
    },
];

// ---------------------------------------------------------------------------
// Workspace / ScreenLayout (wm_workspace.h / bScreen 类比)
// ---------------------------------------------------------------------------

/// Workspace identifier — persisted as `"chart"` / `"screener"` / `"sepa"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceId {
    Chart,
    Screener,
    Sepa,
}

impl WorkspaceId {
    /// i18n key of the workspace name — the Topbar Segmented segment label
    /// (design §7.1: 图表/选股/SEPA复盘).
    pub fn title_key(&self) -> &'static str {
        match self {
            Self::Chart => "workspace.chart",
            Self::Screener => "workspace.screener",
            Self::Sepa => "workspace.sepa",
        }
    }

    /// Segmented segment icon (design §7.1: CHART_LINE / FUNNEL_SIMPLE /
    /// GAUGE — the same glyphs as the default editor of each workspace).
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Chart => egui_phosphor::regular::CHART_LINE,
            Self::Screener => egui_phosphor::regular::FUNNEL_SIMPLE,
            Self::Sepa => egui_phosphor::regular::GAUGE,
        }
    }

    /// The main editor kind of this workspace's default layout — the
    /// restore-side counterpart of `default_layout`'s `focus_main_leaf`
    /// choice (review 37c81bfe P1-2): each workspace re-focuses its own
    /// editor after a persisted-layout rebuild.
    pub fn default_editor_kind(&self) -> EditorKind {
        match self {
            Self::Chart => EditorKind::Chart,
            Self::Screener => EditorKind::Screener,
            Self::Sepa => EditorKind::Sepa,
        }
    }

    /// Stable config string (`[layout] active_workspace` / `[[layout.
    /// workspaces]] id`, design §9.1) — matches the serde snake_case names.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Chart => "chart",
            Self::Screener => "screener",
            Self::Sepa => "sepa",
        }
    }

    /// Parse a config string back into a [`WorkspaceId`]; `None` for unknown
    /// ids (the caller then falls back to the default layout, design §9.1).
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "chart" => Some(Self::Chart),
            "screener" => Some(Self::Screener),
            "sepa" => Some(Self::Sepa),
            _ => None,
        }
    }
}

/// bScreen 类比: one screen = one area tree (egui_dock vertex graph).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenLayout {
    pub dock_state: DockState<Tab>,
    #[serde(default)]
    pub active_tab: Option<egui_dock::TabPath>,
}

/// WorkSpace 类比: a task workspace holding one or more screen layouts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// Lock-in decision 3: multi-screen reserved; v1 holds exactly 1.
    #[serde(default)]
    pub layouts: Vec<ScreenLayout>,
    #[serde(default)]
    pub active_screen: usize,
}

/// Application-level workspace container (window-manager 类比).
pub struct Workspaces {
    /// v1 always exactly 3 (Chart/Screener/Sepa).
    pub all: Vec<Workspace>,
    pub active: usize,
}

impl Workspaces {
    /// Editor kinds currently present in workspace `idx`'s active screen
    /// (design §7.1): the Topbar `⋮ 添加编辑器` menu lists only registered
    /// kinds absent from this set — the "re-open a closed tab" entry.
    pub fn visible_kinds(&self, idx: usize) -> Vec<EditorKind> {
        self.all
            .get(idx)
            .and_then(|w| w.layouts.get(w.active_screen))
            .map(|screen| {
                screen
                    .dock_state
                    .iter_surfaces()
                    .flat_map(|s| s.node_tree().into_iter().flat_map(|t| t.tabs()))
                    .map(|t| t.kind())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Default three-workspace container (design §5). Each workspace holds
    /// exactly one screen layout (lock-in D3) built from [`Self::default_layout`].
    /// Used by the production constructor and as the corrupted-config
    /// fallback in phase 4.
    pub fn default() -> Self {
        let all = [WorkspaceId::Chart, WorkspaceId::Screener, WorkspaceId::Sepa]
            .into_iter()
            .map(|id| Workspace {
                id,
                layouts: vec![ScreenLayout {
                    dock_state: Self::default_layout(id),
                    active_tab: None,
                }],
                active_screen: 0,
            })
            .collect();
        Workspaces { all, active: 0 }
    }

    /// Switch the active workspace: in phase 3 this also saves the current
    /// dock tree in memory and marks the layout dirty for persistence
    /// (design §4.4/§9.2). Editor instance state is global — switching
    /// loses nothing (design §4.5).
    pub fn switch(&mut self, target: WorkspaceId) {
        if let Some(idx) = self.all.iter().position(|w| w.id == target) {
            self.active = idx;
        }
    }

    /// Build the default dock tree for a workspace — extracted from
    /// `main.rs:156-174` (design §4.4/§5).
    ///
    /// Layout contract (design §5.1-5.3 + arbitration Q1/Q5/Q6):
    /// - Chart: Watchlist left leaf + Chart main + Logger bottom (split_left
    ///   then split_below). NOTE (A2 correction): egui_dock's `fraction`
    ///   is the share of the **left/top child** — `split_left(root, 0.75, …)`
    ///   would give Watchlist 75%, so the intent "Chart main keeps 0.75" is
    ///   `split_left(root, 0.25, [Watchlist])`.
    /// - Screener: Screener main + Logger bottom (Q5).
    /// - Sepa: [Sepa, Market] single main leaf + Logger bottom (Q1/Q5).
    pub fn default_layout(id: WorkspaceId) -> DockState<Tab> {
        match id {
            WorkspaceId::Chart => {
                let mut d = DockState::new(vec![Tab::new(EditorKind::Chart)]);
                if let Some(surface) = d.get_surface_mut(egui_dock::SurfaceIndex::main())
                    && let Some(tree) = surface.node_tree_mut()
                {
                    let _ = tree.split_left(
                        egui_dock::NodeIndex::root(),
                        0.25,
                        vec![Tab::new(EditorKind::Watchlist)],
                    );
                    let _ = tree.split_below(
                        egui_dock::NodeIndex::root(),
                        0.75,
                        vec![Tab::new(EditorKind::Logger)],
                    );
                    focus_main_leaf(tree, EditorKind::Chart);
                }
                d
            }
            WorkspaceId::Screener => {
                let mut d = DockState::new(vec![Tab::new(EditorKind::Screener)]);
                if let Some(surface) = d.get_surface_mut(egui_dock::SurfaceIndex::main())
                    && let Some(tree) = surface.node_tree_mut()
                {
                    let _ = tree.split_below(
                        egui_dock::NodeIndex::root(),
                        0.75,
                        vec![Tab::new(EditorKind::Logger)],
                    );
                    focus_main_leaf(tree, EditorKind::Screener);
                }
                d
            }
            WorkspaceId::Sepa => {
                let mut d = DockState::new(vec![
                    Tab::new(EditorKind::Sepa),
                    Tab::new(EditorKind::Market),
                ]);
                if let Some(surface) = d.get_surface_mut(egui_dock::SurfaceIndex::main())
                    && let Some(tree) = surface.node_tree_mut()
                {
                    let _ = tree.split_below(
                        egui_dock::NodeIndex::root(),
                        0.75,
                        vec![Tab::new(EditorKind::Logger)],
                    );
                    focus_main_leaf(tree, EditorKind::Sepa);
                }
                d
            }
        }
    }
}

/// egui_dock's `Tree::split` leaves `focused_node` on the node created by
/// the *last* split (tree/mod.rs:534) — for the default layouts above that
/// is the bottom Logger leaf, so the add-editor menu (`push_to_focused_leaf`)
/// would drop new editors into the logger pane until the user clicks a tab
/// title. Direct focus back to the main editor leaf (review 26b1a84c P2-1).
pub(crate) fn focus_main_leaf(tree: &mut egui_dock::Tree<Tab>, kind: EditorKind) {
    if let Some((node, _)) = tree.find_tab(&Tab::new(kind)) {
        tree.set_focused_node(node);
    } else {
        // Fail-soft (review 81e84e15 P1-1): the invariant "the kind was
        // just constructed" holds for default_layout, but the persisted
        // restore path feeds user data — a closed main tab (egui_dock
        // tabs are closeable by default) produces a valid topology
        // without the main kind, and the config contract (§9.1) must
        // never block startup. Log and skip; the active-editor gates
        // simply stay off until the user clicks a tab.
        tracing::warn!(kind = ?kind, "focus_main_leaf: kind missing from layout, leaving dock focus untouched");
    }
}

// ---------------------------------------------------------------------------
// Dock topology persistence (design §9.1, dock_version = 2)
// ---------------------------------------------------------------------------

/// Topology format version (design §9.1): single source of truth lives in
/// the TOML `[layout] dock_version` key. `!= 2` (missing/older/future)
/// falls back to `Workspaces::default()` with a warning — v1 (egui_dock
/// native `DockState<Tab>` JSON) was never released, so there is no
/// migration code. Bump this whenever the JSON shape changes.
pub const DOCK_TOPOLOGY_VERSION: u32 = 2;

/// Serialized dock topology (design §9.1): a recursive split/leaf tree
/// carrying only split direction, the a-side fractional share and leaf tab
/// sequences. No egui types inside — this deliberately sidesteps
/// egui_dock#197 (`Rect::NOTHING` = ±inf serializes as `null`, at
/// `dock_state/tree/node/mod.rs:145`, breaking any direct `DockState`
/// round-trip; <https://github.com/anhosh/egui_dock/issues/197>, still
/// unfixed in 0.21.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockTopology {
    pub root: DockNode,
}

/// Recursive topology node. Externally tagged by serde, so `Split`
/// serializes as `{"split":{"dir":..,"fraction":..,"a":..,"b":..}}` and
/// `Leaf` as `{"leaf":{"tabs":[..]}}` (design §9.1 schema — `lowercase`
/// because the design pins the tag names in lowercase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockNode {
    Split {
        dir: DockDir,
        fraction: f32,
        a: Box<DockNode>,
        b: Box<DockNode>,
    },
    Leaf {
        tabs: Vec<EditorKind>,
    },
}

/// Split orientation (design §9.1): `Horizontal` = a left / b right,
/// `Vertical` = a top / b bottom; `fraction` is always the **a-side** share
/// — the same semantics as egui_dock `Split{fraction}` ("fraction taken by
/// the top child", `node/split.rs:14`; verified against the renderer
/// `dock_area/show/mod.rs`), so rebuild needs no complement transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockDir {
    Horizontal,
    Vertical,
}

impl DockTopology {
    /// Validate a parsed topology (design §9.1): every fraction strictly
    /// inside `(0.0, 1.0)` and finite (egui_dock drags never produce real
    /// 0/1; NaN/out-of-range = corrupted), every leaf non-empty (`Node::Empty`
    /// is a closed/deleted intermediate state — never persisted), and no
    /// duplicate editor kind within a workspace (each kind has exactly one
    /// editor instance, design §4.5; a duplicated tab would render the same
    /// instance twice into clashing ids).
    pub fn validate(&self) -> Result<(), String> {
        fn walk(node: &DockNode, kinds: &mut Vec<EditorKind>) -> Result<(), String> {
            match node {
                DockNode::Leaf { tabs } => {
                    if tabs.is_empty() {
                        return Err("empty leaf in topology".to_string());
                    }
                    kinds.extend(tabs.iter().copied());
                    Ok(())
                }
                DockNode::Split {
                    dir: _,
                    fraction,
                    a,
                    b,
                } => {
                    if !fraction.is_finite() || !(0.0 < *fraction && *fraction < 1.0) {
                        return Err(format!("fraction out of range (0,1): {fraction}"));
                    }
                    walk(a, kinds)?;
                    walk(b, kinds)
                }
            }
        }
        let mut kinds = Vec::new();
        walk(&self.root, &mut kinds)?;
        let mut seen = std::collections::HashSet::new();
        if let Some(dup) = kinds.iter().find(|k| !seen.insert(**k)) {
            return Err(format!("duplicate editor kind in topology: {dup:?}"));
        }
        Ok(())
    }
}

/// Extract the visual topology of the main surface (design §9.1): only
/// split directions, fractions and leaf tab sequences are kept; runtime
/// state (rects, viewports, focus, active tab) is deliberately dropped.
/// Returns `None` when the tree is a degenerate empty/Empty state — the
/// caller then skips saving rather than persisting an unusable topology.
pub fn extract_topology(dock_state: &DockState<Tab>) -> Option<DockTopology> {
    let tree = dock_state
        .get_surface(egui_dock::SurfaceIndex::main())?
        .node_tree()?;
    let root = tree.root_node()?;
    extract_node(tree, egui_dock::NodeIndex::root(), root).map(|root| DockTopology { root })
}

fn extract_node(
    tree: &egui_dock::Tree<Tab>,
    idx: egui_dock::NodeIndex,
    node: &egui_dock::Node<Tab>,
) -> Option<DockNode> {
    match node {
        egui_dock::Node::Empty => None,
        egui_dock::Node::Leaf(leaf) => {
            let tabs = leaf.tabs.iter().map(|t| t.kind()).collect::<Vec<_>>();
            if tabs.is_empty() {
                None
            } else {
                Some(DockNode::Leaf { tabs })
            }
        }
        egui_dock::Node::Horizontal(split) => {
            let a = extract_node(tree, idx.left(), &tree[idx.left()])?;
            let b = extract_node(tree, idx.right(), &tree[idx.right()])?;
            Some(DockNode::Split {
                dir: DockDir::Horizontal,
                fraction: split.fraction,
                a: Box::new(a),
                b: Box::new(b),
            })
        }
        egui_dock::Node::Vertical(split) => {
            let a = extract_node(tree, idx.left(), &tree[idx.left()])?;
            let b = extract_node(tree, idx.right(), &tree[idx.right()])?;
            Some(DockNode::Split {
                dir: DockDir::Vertical,
                fraction: split.fraction,
                a: Box::new(a),
                b: Box::new(b),
            })
        }
    }
}

/// Rebuild a `DockState` from a persisted topology (design §9.1): mirrors
/// egui_dock's forward split construction — the a-side tab sequence is
/// seeded into the leaf first, then `split_right`/`split_below` attach the
/// b-side as a new leaf with the fraction interpreted as the **a-side**
/// share (verified: `Tree::split` keeps `old` in the a slot for
/// `Split::Right`/`Split::Below`, `tree/mod.rs` around :497-527). The
/// rebuilt arena differs from a user-dragged one but is visually
/// identical; runtime state starts fresh (`active_tab` = None, design §9.1:
/// "切回 workspace 即重建" semantics).
pub fn dock_state_from_topology(topo: &DockTopology) -> DockState<Tab> {
    let mut dock = DockState::new(Vec::new());
    grow_node(&mut dock, egui_dock::NodeIndex::root(), &topo.root);
    // No focus fix here: the rebuild leaves egui_dock's focused_node on
    // the last split's b-side (tree/mod.rs:534), and the "main editor"
    // kind is a semantic choice the caller owns (it knows the workspace
    // id; the first pre-order leaf of the chart layout is the Watchlist,
    // not the Chart). `resolve_workspaces` re-focuses per workspace id
    // after this call (review 37c81bfe P1-2).
    dock
}

/// Grow the tree from `idx`: the leaf at `idx` must currently exist (it is
/// seeded with the *a-side* tab sequence beforehand by the split step), and
/// after splitting the recursion continues into both children — each of
/// which holds exactly its own subtree's pre-order tab sequence.
fn grow_node(dock: &mut DockState<Tab>, idx: egui_dock::NodeIndex, node: &DockNode) {
    let tree = dock.main_surface_mut();
    match node {
        DockNode::Leaf { tabs } => {
            // The leaf is already seeded with exactly these tabs; setting
            // them again makes the invariant airtight (cheap).
            let leaf = tree.leaf_mut(idx).expect("grow_node: leaf position");
            leaf.tabs = tabs.iter().map(|k| Tab::new(*k)).collect();
        }
        DockNode::Split {
            dir,
            fraction,
            a,
            b,
        } => {
            let a_tabs = preorder_kinds(a);
            let b_tabs = preorder_kinds(b);
            let leaf = tree
                .leaf_mut(idx)
                .expect("grow_node: split position is a leaf");
            leaf.tabs = a_tabs.iter().map(|k| Tab::new(*k)).collect();
            let b_tabs = b_tabs.into_iter().map(Tab::new).collect();
            match dir {
                DockDir::Horizontal => {
                    tree.split_right(idx, *fraction, b_tabs);
                }
                DockDir::Vertical => {
                    tree.split_below(idx, *fraction, b_tabs);
                }
            }
            grow_node(dock, idx.left(), a);
            grow_node(dock, idx.right(), b);
        }
    }
}

/// Pre-order kind sequence of a node: for a split this is a's sequence
/// followed by b's — exactly the order leaves appear visually (left/top
/// first) and the order `extract_topology` emits.
fn preorder_kinds(node: &DockNode) -> Vec<EditorKind> {
    match node {
        DockNode::Leaf { tabs } => tabs.clone(),
        DockNode::Split { a, b, .. } => {
            let mut out = preorder_kinds(a);
            out.extend(preorder_kinds(b));
            out
        }
    }
}

// ---------------------------------------------------------------------------
// EditorView + EditorCtx + EditorFrame (SpaceData 实例层)
// ---------------------------------------------------------------------------

/// Editor instance trait — replaces the 16-field `TabViewer` borrow struct
/// (friction F5). `body` is required; `header`/`sidebar` render only when
/// the `EditorLayout` registers the slot (empty impl costs nothing).
pub trait EditorView {
    /// Contract identity (design §4.4): the kind the instance serves.
    /// Consumed by contract tests and kept for symmetric access; the
    /// frame dispatches via the descriptor's `kind`, not the instance.
    #[allow(dead_code)]
    fn kind(&self) -> EditorKind;

    fn header(&mut self, _ui: &mut egui::Ui, _ctx: &mut EditorCtx<'_>) {}

    fn body(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>);

    fn sidebar(&mut self, _ui: &mut egui::Ui, _ctx: &mut EditorCtx<'_>) {}
}

/// Editor context: the borrow bundle that `TabViewer` passes today; the
/// remaining fields (signals, toast manager, industries, …) are attached
/// during the migration phases as each editor moves in (design §4.2).
pub struct EditorCtx<'a> {
    pub state: &'a SharedState,
    pub theme: &'a CompassTheme,
    /// Signal bundle — every App-level trigger an editor needs to fire
    /// (work/screener/sepa/index/llm). phase 2b+ editors use these;
    /// the chart uses the `chart_action` out-param instead (see below).
    /// phase 3 converges the tab-viewer field set onto this struct.
    pub signals: &'a EditorSignals<'a>,
    /// Index list backing the 前复权 hide guard (design/plan §4.1:
    /// `is_index_or_board` logic moves with the control into the chart).
    pub index_list: &'a [IndexBasic],
    /// Out-param channel for App-level chart header actions (timeframe /
    /// adjust / fetch). The editor writes the action during render; the
    /// owner consumes it after `show_inside` returns — same pattern as the
    /// existing `logger_export_clicked` out-param (design §4.2 "按需并入").
    /// Single slot: same-frame multi-action is last-wins (unreachable under
    /// single-pointer egui semantics — one click yields one action).
    pub chart_action: &'a mut Option<ChartHeaderAction>,
    /// Screener condition-builder context (plan §4.2): industry/board lists
    /// backing the card multi-selects. Attached now that the screener moves
    /// in; phase 3 converges the tab-viewer field set onto this struct.
    pub screener_industries: &'a [String],
    pub screener_boards: &'a [String],
    /// Out-param channel for the Logger export button (plan §4.5): the
    /// editor writes the click during render; the owner opens the
    /// save-file dialog after `show_inside` returns — same pattern as
    /// `chart_action` and the pre-2e `logger_export_clicked` TabViewer
    /// field (design §4.2 "按需并入").
    pub logger_export_clicked: &'a mut bool,
    /// Toast sink (design §4.2 reserved field): editors that persist
    /// user-visible config (watchlist add/remove) push toasts through this
    /// instead of owning an App-level toast channel. No consumer yet as of
    /// phase 3 — watchlist feedback goes through `watchlist_action`; revisit
    /// at phase 5 before removing the field.
    #[allow(dead_code)]
    pub toasts: &'a mut ToastManager,
    /// Stock metadata list backing watchlist row names/exchange tags
    /// (plan §4.6 — migrated from the old `render_sidebar` lookup).
    pub stock_list: &'a [StockBasic],
    /// Out-param channel for watchlist editor actions (plan §4.6): the
    /// editor writes at most one action per frame during render; the owner
    /// fetches/adds/opens the removal modal after `show_inside` returns.
    pub watchlist_action: &'a mut Option<WatchlistAction>,
    /// Out-param channel for the sidebar toggle (design §8.2 mouse entry):
    /// the Display Options menu of a sidebar-having editor writes the click
    /// during render; the owner (TabViewer) flips the per-kind visibility
    /// map after `show_inside` returns — same pattern as `chart_action`.
    pub sidebar_toggle_requested: &'a mut bool,
}

/// Bundle of the five citizen-trigger signals (design §4.2 `EditorSignals`).
pub struct EditorSignals<'a> {
    pub work: &'a Signal<FetchRequest>,
    pub screener: &'a Signal<RunScreenerRequest>,
    pub sepa: &'a Signal<RunSepaRequest>,
    pub index: &'a Signal<RunIndexSnapshotRequest>,
    pub llm: &'a Signal<RunLlmRequest>,
}

/// App-level action an editor header can request. Only actions that must be
/// handled by the App owner live here; purely internal editor state (indicator
/// visibility, display options, candle style, MA/BOLL parameters) stays
/// inside the editor instance and never crosses this channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartHeaderAction {
    /// Switch the period (0="1d", 1="1w", 2="1M").
    Timeframe(usize),
    /// Switch the price adjustment mode (0="qfq", 1="hfq", 2="none").
    Adjust(usize),
    /// Fetch the current symbol (loading state handled by the owner).
    Fetch,
}

/// Whether `symbol` is an index/board (epic #255 C4): BK-prefixed board
/// codes or any symbol listed in index_basic.parquet. Drives the 前复权
/// tag hide guard — indexes are not adjusted (fqt=0), so showing the tag
/// would be wrong information. Migrated out of `CompassApp` with the
/// control (plan §4.1).
pub fn is_index_or_board(index_list: &[IndexBasic], symbol: &str) -> bool {
    parse_explicit_prefix(symbol).0 == "BK"
        || index_list
            .iter()
            .any(|i| i.symbol == symbol && !i.index_type.is_empty())
}

/// Runtime chrome wrapper (ARegion 类比): Header bar + (Sidebar | body)
/// split. The header renders into a top panel; the sidebar is a left panel
/// shown only when the `EditorLayout` registers one (and its `sidebar_visible`
/// flag is set); the body fills the remainder.
pub struct EditorFrame {
    pub sidebar_visible: bool,
}

impl EditorFrame {
    /// Renders one editor with its unified chrome.
    ///
    /// Panel order matters inside `show_inside`: top header first, then the
    /// sidebar (if any), then the central body.
    ///
    /// Panel ids are salted with `desc.kind`: several editors can render in
    /// the same frame (e.g. the default workspace has Chart and Screener
    /// leaves side by side), so a fixed id would clash — egui would draw the
    /// 🔥 overlap warning in debug builds and, worse, share `PanelState`
    /// (sidebar widths cross-talking between editors, reviewer P1-1).
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        desc: &EditorDescriptor,
        editor: &mut dyn EditorView,
        ctx: &mut EditorCtx<'_>,
    ) {
        egui::Panel::top(egui::Id::new(("editor_header", desc.kind))).show(ui, |ui| {
            editor.header(ui, ctx);
        });

        let sidebar = desc.layout.sidebar.as_ref();
        if let Some(side) = sidebar.filter(|_| self.sidebar_visible) {
            let range = egui::Rangef::new(side.width_range.0, side.width_range.1);
            egui::Panel::left(egui::Id::new(("editor_sidebar", desc.kind)))
                .default_size(side.default_width)
                .size_range(range)
                .resizable(true)
                .show(ui, |ui| {
                    editor.sidebar(ui, ctx);
                });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            editor.body(ui, ctx);
        });
    }
}

/// Outliner-style watchlist editor (design §6 Watchlist 行, plan §4.6):
/// an editor whose header slot holds the search row (input + add button)
/// and whose body renders the 自选 group list. It is **not** a Sidebar
/// (Q6): it lives as an independent dock leaf in the chart workspace, has
/// no default width, no N-key registration and no citizen (its
/// `EditorKind::citizen_id()` returns `None`).
pub struct WatchlistEditor {
    /// Search filter text (migrated from the old global `sidebar_search`;
    /// owned per-editor instance now).
    pub search: String,
}

impl WatchlistEditor {
    /// Create an editor with an empty search filter.
    pub fn new() -> Self {
        Self {
            search: String::new(),
        }
    }

    /// `egui::Id` of the watchlist search input, for the Ctrl+K focus
    /// shortcut (migrated verbatim from the old `sidebar_search_input_id`:
    /// the header renders the input inside a `UiBuilder` child of
    /// `id("sidebar_body")`, and the `Input` widget derives its own id the
    /// same way — hash of the salt value, not the string).
    pub fn search_input_id() -> egui::Id {
        let body = egui::Id::new("sidebar_body");
        let child = egui::IdSalt::new("child");
        let input = egui::IdSalt::new("compass_input");
        body.with(child).with(child).with(child).with(input)
    }
}

/// App-level action produced by the watchlist editor (plan §4.6): the
/// editor itself is pure render + event collection; the owner (App)
/// consumes these after the frame pass — same channel pattern as
/// `chart_action` / `logger_export_clicked`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchlistAction {
    /// A row was clicked: fetch that symbol (global single-symbol state).
    Select { symbol: String },
    /// The add (＋) button was clicked: insert the current symbol.
    Add,
    /// The delete (×) button of a row was clicked: open the danger
    /// confirm modal (App-level, modal is not an editor concern).
    DeleteRequest { symbol: String },
}

impl EditorView for WatchlistEditor {
    fn kind(&self) -> EditorKind {
        EditorKind::Watchlist
    }

    /// Header (design §6 Watchlist 行): the search row — filter input +
    /// `[添加]` IconButton — migrated from the `Sidebar` widget composite
    /// (plan §4.6; Ctrl+K focus semantics via [`WatchlistEditor::search_input_id`]).
    /// Add clicks are collected into the action out-param (the App inserts
    /// the current symbol); the input mutates `self.search` in place.
    fn header(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {
        let tokens = *ctx.theme.tokens();
        let sidebar = Sidebar::new(&tokens);
        let events = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("sidebar_body")),
                |ui| sidebar.search_row(ui, &mut self.search, ui.available_width()),
            )
            .inner;
        for event in events {
            if let SidebarEvent::Add = event {
                *ctx.watchlist_action = Some(WatchlistAction::Add);
            }
        }
    }

    /// Body (design §6 Watchlist 行): the 自选 group list backed by
    /// `SharedState.watchlist`, filtered by `self.search`. Row clicks /
    /// add / delete are collected into `ctx.watchlist_action` (single-slot,
    /// last-wins per frame — unreachable under single-pointer semantics).
    fn body(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {
        let tokens = *ctx.theme.tokens();
        let sidebar = Sidebar::new(&tokens);
        let current_symbol = ctx.state.symbol.get();
        let watchlist = ctx.state.watchlist.get();
        let query = self.search.trim().to_lowercase();

        let mut items = Vec::new();
        for symbol in &watchlist {
            let stock = ctx.stock_list.iter().find(|s| &s.symbol == symbol);
            let name = stock
                .map(|s| s.name.clone())
                .unwrap_or_else(|| symbol.clone());
            let exchange = stock
                .map(|s| exchange_of_symbol(&s.symbol).to_string())
                .unwrap_or_default();
            let matches = query.is_empty()
                || symbol.to_lowercase().contains(&query)
                || name.to_lowercase().contains(&query);
            if matches {
                items.push(SidebarItem {
                    symbol: symbol.clone(),
                    name,
                    exchange,
                    selected: symbol == &current_symbol,
                });
            }
        }
        let groups = [SidebarGroup {
            title: t!("sidebar.group_watchlist").to_string(),
            items,
        }];

        let events = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("watchlist_body")),
                |ui| sidebar.show_list(ui, &groups, ui.available_width()),
            )
            .inner;

        for event in events {
            match event {
                SidebarEvent::Select { symbol } => {
                    *ctx.watchlist_action = Some(WatchlistAction::Select { symbol });
                }
                SidebarEvent::Search(_) => {}
                SidebarEvent::Add => {
                    *ctx.watchlist_action = Some(WatchlistAction::Add);
                }
                SidebarEvent::DeleteRequest { symbol } => {
                    *ctx.watchlist_action = Some(WatchlistAction::DeleteRequest { symbol });
                }
            }
        }
    }
}

/// One instance per `EditorKind` (design §4.5 — decision B, see design
/// §11 decision record #5). `get_mut` is the single dispatch point that
/// `TabViewer::ui` uses, replacing six scattered matches (friction F5/F6).
pub struct EditorInstances {
    pub chart: crate::citizens::chart::ChartCitizen,
    pub screener: crate::citizens::screener::ScreenerPanel,
    pub sepa: crate::citizens::sepa::SepaPanel,
    pub market: crate::citizens::market::MarketPanel,
    pub logger: crate::citizens::logger::LoggerPanel,
    pub watchlist: WatchlistEditor,
}

impl EditorInstances {
    /// Kind → mutable instance dispatch. The single match point that
    /// `TabViewer::ui` uses — replacing the six scattered field borrows
    /// (friction F5/F6, design §4.5).
    pub fn get_mut(&mut self, kind: EditorKind) -> &mut dyn EditorView {
        match kind {
            EditorKind::Chart => &mut self.chart,
            EditorKind::Screener => &mut self.screener,
            EditorKind::Sepa => &mut self.sepa,
            EditorKind::Market => &mut self.market,
            EditorKind::Logger => &mut self.logger,
            EditorKind::Watchlist => &mut self.watchlist,
        }
    }
}

// ===========================================================================
// Phase 0 unit tests — design §11 组 A + plan §2.2
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::ui_fixes_218::LANG_LOCK;
    use compass_i18n::t;

    /// Key-resolution test helper (mirrors tabs.rs): resolves a key through
    /// the shared compass-i18n dictionary.
    fn tr(key: &str) -> String {
        t!(key).to_string()
    }

    // ------------------------------------------------------------------
    // EDITOR_REGISTRY 完整性（plan §2.2 / 设计 §11 组 A）
    // ------------------------------------------------------------------

    #[test]
    fn registry_has_exactly_six_entries_with_distinct_kinds() {
        assert_eq!(EDITOR_REGISTRY.len(), 6);
        let mut kinds = std::collections::HashSet::new();
        for desc in &EDITOR_REGISTRY {
            assert!(kinds.insert(desc.kind), "duplicate kind: {:?}", desc.kind);
        }
        assert_eq!(kinds.len(), 6);
    }

    #[test]
    fn registry_header_always_present() {
        // Lock-in D1/D7: header is mandatory, never an Option.
        for desc in &EDITOR_REGISTRY {
            let _assert_header_is_unit = desc.layout.header;
        }
    }

    #[test]
    fn registry_sidebar_only_chart_and_screener() {
        // Design §6: exactly Chart + Screener register a sidebar; the rest
        // (incl. Watchlist, Q6) are None.
        let with_sidebar: Vec<_> = EDITOR_REGISTRY
            .iter()
            .filter(|d| d.layout.sidebar.is_some())
            .collect();
        assert_eq!(with_sidebar.len(), 2);
        assert!(with_sidebar.iter().any(|d| d.kind == EditorKind::Chart));
        assert!(with_sidebar.iter().any(|d| d.kind == EditorKind::Screener));
    }

    #[test]
    fn registry_toolbar_none_for_all() {
        // Design §6/D7: no editor registers a toolbar this round.
        for desc in &EDITOR_REGISTRY {
            assert!(desc.layout.toolbar.is_none(), "{:?} has toolbar", desc.kind);
        }
    }

    #[test]
    fn registry_title_keys_and_icons_nonempty() {
        for desc in &EDITOR_REGISTRY {
            assert!(!desc.title_key.is_empty());
            assert!(!desc.icon.is_empty());
            assert_eq!(desc.title_key, desc.kind.title_key());
            assert_eq!(desc.icon, desc.kind.icon());
        }
    }

    #[test]
    fn editor_kind_title_key_precise_i18n_keys() {
        // Design §4.1: `title_key()` returns the exact `editor.*` key per
        // variant — pins the mapping that EDITOR_REGISTRY title_keys must
        // match (a wrong-but-consistent "editor.x" would otherwise pass the
        // desc-vs-kind consistency check).
        for (kind, expected) in [
            (EditorKind::Chart, "editor.chart"),
            (EditorKind::Screener, "editor.screener"),
            (EditorKind::Sepa, "editor.sepa"),
            (EditorKind::Market, "editor.market"),
            (EditorKind::Logger, "editor.logger"),
            (EditorKind::Watchlist, "editor.watchlist"),
        ] {
            assert_eq!(kind.title_key(), expected, "title_key for {:?}", kind);
        }
    }

    #[test]
    fn editor_kind_icon_precise_glyphs() {
        // Design §4.1: icon() returns the documented Phosphor glyph per
        // editor (CHART_LINE / FUNNEL_SIMPLE / GAUGE / TREND_UP / TERMINAL /
        // LIST_STAR) — pins the exact glyph mapping.
        for (kind, expected) in [
            (EditorKind::Chart, egui_phosphor::regular::CHART_LINE),
            (EditorKind::Screener, egui_phosphor::regular::FUNNEL_SIMPLE),
            (EditorKind::Sepa, egui_phosphor::regular::GAUGE),
            (EditorKind::Market, egui_phosphor::regular::TREND_UP),
            (EditorKind::Logger, egui_phosphor::regular::TERMINAL),
            (EditorKind::Watchlist, egui_phosphor::regular::LIST_STAR),
        ] {
            assert_eq!(kind.icon(), expected, "icon for {:?}", kind);
        }
    }

    #[test]
    fn registry_title_keys_exist_in_i18n() {
        // Design §11 组 A: title_key must resolve in the i18n dictionary
        // (LANG_LOCK serialized mode, tabs.rs:210-217 precedent).
        let _guard = LANG_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        compass_i18n::set_locale("zh");
        for desc in &EDITOR_REGISTRY {
            let resolved = tr(desc.title_key);
            assert!(
                !resolved.is_empty() && resolved != desc.title_key,
                "title_key {} must resolve in zh.yml",
                desc.title_key
            );
        }
        compass_i18n::set_locale("en");
        for desc in &EDITOR_REGISTRY {
            let resolved = tr(desc.title_key);
            assert!(
                !resolved.is_empty() && resolved != desc.title_key,
                "title_key {} must resolve in en.yml",
                desc.title_key
            );
        }
        // Restore the zh default (tabs.rs precedent leaves the locale at zh;
        // never leak en out of the LANG_LOCK guard).
        compass_i18n::set_locale("zh");
    }

    #[test]
    fn chart_sidebar_spec_follows_contract() {
        // Design §4.1: default 240.0 / 200.0..=320.0 — aligned with
        // main.rs:905-913 (pre-migration).
        let desc = EDITOR_REGISTRY
            .iter()
            .find(|d| d.kind == EditorKind::Chart)
            .expect("chart in registry");
        let sb = desc.layout.sidebar.expect("sidebar registered");
        assert_eq!(sb.default_width, 240.0);
        assert_eq!(sb.width_range, (200.0, 320.0));
    }

    #[test]
    fn screener_sidebar_spec_is_independent() {
        // 2026-09-05 designer ruling (reviewer P1-3): the screener sidebar is
        // decoupled from the chart spec — condition cards need ≥486px
        // (widest `Momentum` leaf ≈470px + 16px panel padding), default 500px
        // is the first GROUP_ALIGNMENT_WIDTHS anchor.
        let desc = EDITOR_REGISTRY
            .iter()
            .find(|d| d.kind == EditorKind::Screener)
            .expect("screener in registry");
        let sb = desc.layout.sidebar.expect("sidebar registered");
        assert_eq!(sb.default_width, 500.0);
        assert_eq!(sb.width_range, (486.0, 640.0));
    }

    // ------------------------------------------------------------------
    // EditorKind serde round-trip + unknown fallback（设计 §11 组 A）
    // ------------------------------------------------------------------

    #[test]
    fn editor_kind_serde_roundtrip() {
        for kind in [
            EditorKind::Chart,
            EditorKind::Screener,
            EditorKind::Sepa,
            EditorKind::Market,
            EditorKind::Logger,
            EditorKind::Watchlist,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<EditorKind>(&json).unwrap(), kind);
        }
        assert_eq!(
            serde_json::to_string(&EditorKind::Watchlist).unwrap(),
            "\"watchlist\""
        );
        assert_eq!(
            serde_json::to_string(&EditorKind::Chart).unwrap(),
            "\"chart\""
        );
    }

    #[test]
    fn editor_kind_unknown_string_errors_not_panics() {
        // Design §11 组 A: corrupt input must be a Result error, never a
        // panic (load path falls back to defaults, §9.1).
        let result: Result<EditorKind, _> = serde_json::from_str("\"bogus\"");
        assert!(result.is_err());
    }

    #[test]
    fn editor_kind_serde_exact_snake_case_strings() {
        // Design §4.1: every variant persists as its exact snake_case string
        // ("chart", "screener", "sepa", "market", "logger", "watchlist") —
        // pinning all six (a pure round-trip would pass a
        // `#[serde(rename = "…")]` mutation on a single variant).
        for (kind, expected) in [
            (EditorKind::Chart, "\"chart\""),
            (EditorKind::Screener, "\"screener\""),
            (EditorKind::Sepa, "\"sepa\""),
            (EditorKind::Market, "\"market\""),
            (EditorKind::Logger, "\"logger\""),
            (EditorKind::Watchlist, "\"watchlist\""),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, expected, "serde string for {:?}", kind);
        }
    }

    // ------------------------------------------------------------------
    // WorkspaceId serde round-trip
    // ------------------------------------------------------------------

    #[test]
    fn workspace_id_serde_roundtrip() {
        for (id, expected) in [
            (WorkspaceId::Chart, "\"chart\""),
            (WorkspaceId::Screener, "\"screener\""),
            (WorkspaceId::Sepa, "\"sepa\""),
        ] {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, expected);
            assert_eq!(serde_json::from_str::<WorkspaceId>(&json).unwrap(), id);
        }
        let result: Result<WorkspaceId, _> = serde_json::from_str("\"bogus\"");
        assert!(result.is_err());
    }

    #[test]
    fn workspace_id_as_str_matches_serde_and_roundtrips() {
        // review 41e74cc0 P3-7: the hand-maintained as_str/from_str table
        // must stay in lockstep with serde's snake_case rename — the config
        // [layout] strings go through the manual table, the serde paths
        // through the derive; a drift would silently break persistence.
        for id in [WorkspaceId::Chart, WorkspaceId::Screener, WorkspaceId::Sepa] {
            assert_eq!(
                format!("\"{}\"", id.as_str()),
                serde_json::to_string(&id).unwrap(),
                "as_str must match the serde snake_case name for {id:?}"
            );
            assert_eq!(WorkspaceId::from_str(id.as_str()), Some(id));
        }
        assert_eq!(WorkspaceId::from_str("bogus"), None);
    }

    // ------------------------------------------------------------------
    // default_layout signature + tree structure（设计 §11 组 A）
    // ------------------------------------------------------------------

    #[test]
    fn default_layout_signature_contract() {
        let _: fn(WorkspaceId) -> DockState<Tab> = Workspaces::default_layout;
    }

    /// Collects leaf editor kinds in pre-order (left subtree first).
    fn collect_kinds(
        tree: &egui_dock::Tree<Tab>,
        idx: egui_dock::NodeIndex,
        out: &mut Vec<EditorKind>,
    ) {
        match &tree[idx] {
            egui_dock::Node::Leaf(leaf) => {
                for t in leaf.tabs() {
                    out.push(t.kind());
                }
            }
            egui_dock::Node::Vertical(_) | egui_dock::Node::Horizontal(_) => {
                collect_kinds(tree, idx.left(), out);
                collect_kinds(tree, idx.right(), out);
            }
            egui_dock::Node::Empty => {}
        }
    }

    /// Collects splits in pre-order as (vertical, fraction).
    fn collect_splits(
        tree: &egui_dock::Tree<Tab>,
        idx: egui_dock::NodeIndex,
        out: &mut Vec<(bool, f32)>,
    ) {
        match &tree[idx] {
            egui_dock::Node::Vertical(s) => {
                out.push((true, s.fraction));
                collect_splits(tree, idx.left(), out);
                collect_splits(tree, idx.right(), out);
            }
            egui_dock::Node::Horizontal(s) => {
                out.push((false, s.fraction));
                collect_splits(tree, idx.left(), out);
                collect_splits(tree, idx.right(), out);
            }
            egui_dock::Node::Leaf(_) | egui_dock::Node::Empty => {}
        }
    }

    #[test]
    fn chart_default_layout_watchlist_left_chart_main_logger_bottom() {
        // 设计 §5.1 + Q5/Q6：Watchlist 左 leaf（split_left，fraction 为左子
        // 份额 → 0.25 = Watchlist 25% / Chart 主 75%）+ Logger 底部（0.75）。
        let mut dock = Workspaces::default_layout(WorkspaceId::Chart);
        let tree = dock.main_surface_mut();
        let mut kinds = Vec::new();
        collect_kinds(tree, egui_dock::NodeIndex::root(), &mut kinds);
        assert_eq!(
            kinds,
            vec![EditorKind::Watchlist, EditorKind::Chart, EditorKind::Logger,]
        );
        let mut splits = Vec::new();
        collect_splits(tree, egui_dock::NodeIndex::root(), &mut splits);
        assert_eq!(splits, vec![(true, 0.75), (false, 0.25)]);
    }

    #[test]
    fn screener_default_layout_screener_main_logger_bottom() {
        // 设计 §5.2 + Q5：Screener 主 + Logger 底。
        let mut dock = Workspaces::default_layout(WorkspaceId::Screener);
        let tree = dock.main_surface_mut();
        let mut kinds = Vec::new();
        collect_kinds(tree, egui_dock::NodeIndex::root(), &mut kinds);
        assert_eq!(kinds, vec![EditorKind::Screener, EditorKind::Logger]);
        let mut splits = Vec::new();
        collect_splits(tree, egui_dock::NodeIndex::root(), &mut splits);
        assert_eq!(splits, vec![(true, 0.75)]);
    }

    #[test]
    fn sepa_default_layout_sepa_market_then_logger() {
        // 设计 §5.3 + Q1/Q5：SEPA + Market 叠主 leaf 两 tab，Logger 底。
        let mut dock = Workspaces::default_layout(WorkspaceId::Sepa);
        let tree = dock.main_surface_mut();
        let mut kinds = Vec::new();
        collect_kinds(tree, egui_dock::NodeIndex::root(), &mut kinds);
        assert_eq!(
            kinds,
            vec![EditorKind::Sepa, EditorKind::Market, EditorKind::Logger]
        );
        let mut splits = Vec::new();
        collect_splits(tree, egui_dock::NodeIndex::root(), &mut splits);
        assert_eq!(splits, vec![(true, 0.75)]);
    }

    // ==================================================================
    // [adversarial round 2 — after interface commit 8a5ea19 + phase-1
    //  default_layout landing] 前一轮 DEFERRED 的可实测部分，逐条补写。
    // 与 requirement agent 的分工：本条只攻击坏边界/非法输入/mutation 防护，
    // 不重复 happy path。
    // ==================================================================

    // ------------------------------------------------------------------
    // TabKind → EditorKind 身份映射（plan §3.1 A3「过渡枚举」契约）：
    // 旧 tab 迁移后 kind() 必须保持身份——错映射 = 用户打开的旧 tab 在
    // 迁移后「变脸」（chart↔screener 级错乱），mutation 防护。
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Workspaces::switch 边界（设计 §4.4/§9.2 语义承诺：target 不在 all
    // → no-op（active 不变、绝不 panic）；存在 → active = 该 id 在 all 中
    // 的 position，而非固定索引/顺序假设）。
    // ------------------------------------------------------------------

    fn make_workspace(id: WorkspaceId) -> Workspace {
        Workspace {
            id,
            layouts: Vec::new(),
            active_screen: 0,
        }
    }

    #[test]
    fn workspaces_switch_existing_id_uses_position() {
        // all 乱序：switch 必须按 position 查找（若实现硬编码
        // Chart=0/Screener=1/Sepa=2 顺序假设，乱序 all 下必错）。
        let mut ws = Workspaces {
            all: vec![
                make_workspace(WorkspaceId::Sepa),
                make_workspace(WorkspaceId::Chart),
            ],
            active: 0,
        };
        ws.switch(WorkspaceId::Chart);
        assert_eq!(ws.active, 1, "Chart 的 position 是 1（all 乱序）");
        ws.switch(WorkspaceId::Sepa);
        assert_eq!(ws.active, 0);
    }

    #[test]
    fn workspaces_switch_unknown_target_is_noop() {
        // no-op 语义：合法变体但不在 all 中 → active 保持不变。
        let mut ws = Workspaces {
            all: vec![
                make_workspace(WorkspaceId::Chart),
                make_workspace(WorkspaceId::Screener),
            ],
            active: 1,
        };
        ws.switch(WorkspaceId::Sepa); // 变体合法，但 all 中不存在
        assert_eq!(ws.active, 1, "不存在 target 必须 no-op，active 不得漂移");
        ws.switch(WorkspaceId::Sepa);
        assert_eq!(ws.active, 1);
    }

    #[test]
    fn workspaces_switch_empty_all_is_noop_no_panic() {
        // 空 all（load 失败回退后可能出现的非法状态）。
        let mut ws = Workspaces {
            all: Vec::new(),
            active: 0,
        };
        ws.switch(WorkspaceId::Chart);
        assert_eq!(ws.active, 0, "空 all 下 switch 必须 no-op 且不 panic");
    }

    // ------------------------------------------------------------------
    // ScreenLayout / topology persistence（设计 §9.1）：树形态复刻
    // 旧内联布局（Chart+Market+Sepa 顶层，root 下 split Logger 0.75 /
    // Screener 0.5）——直接 DockState serde 因上游 #197 弃用，持久化
    // 走 DockTopology 提取/重建（见下方 topology_* 测试组）。
    // ------------------------------------------------------------------

    fn chart_engineered_layout() -> ScreenLayout {
        let mut dock = DockState::new(vec![
            Tab::new(EditorKind::Chart),
            Tab::new(EditorKind::Market),
            Tab::new(EditorKind::Sepa),
        ]);
        if let Some(surface) = dock.get_surface_mut(egui_dock::SurfaceIndex::main())
            && let Some(tree) = surface.node_tree_mut()
        {
            let _ = tree.split_below(
                egui_dock::NodeIndex::root(),
                0.75,
                vec![Tab::new(EditorKind::Logger)],
            );
            let _ = tree.split_below(
                egui_dock::NodeIndex::root(),
                0.5,
                vec![Tab::new(EditorKind::Screener)],
            );
        }
        ScreenLayout {
            dock_state: dock,
            active_tab: None,
        }
    }

    // ------------------------------------------------------------------
    // Dock topology (design §9.1, dock_version=2): the phase-4 persistence
    // contract. The direct `DockState<Tab>` serde path is *deliberately*
    // abandoned (egui_dock#197: `Rect::NOTHING` = ±inf serializes as null;
    // still unfixed in 0.21.1) — persistence goes through `DockTopology`,
    // which carries only dir/fraction/tabs.
    // ------------------------------------------------------------------

    #[test]
    fn topology_default_layouts_roundtrip_preserves_structure() {
        // extract → rebuild → extract must be exactly equal for all three
        // default layouts: split directions, fractions (a-side share) and
        // leaf tab sequences are lossless.
        for id in [WorkspaceId::Chart, WorkspaceId::Screener, WorkspaceId::Sepa] {
            let dock = Workspaces::default_layout(id);
            let topo = extract_topology(&dock)
                .unwrap_or_else(|| panic!("default {id:?} layout must extract"));
            assert!(
                topo.validate().is_ok(),
                "default {id:?} topology must validate: {:?}",
                topo.validate()
            );
            let rebuilt = dock_state_from_topology(&topo);
            let topo2 = extract_topology(&rebuilt)
                .unwrap_or_else(|| panic!("rebuilt {id:?} layout must extract"));
            assert_eq!(topo, topo2, "topology round-trip lost structure for {id:?}");
        }
    }

    #[test]
    fn topology_chart_matches_design_schema_example() {
        // The Chart default tree must serialize exactly into the shape the
        // design §9.1 example documents (external tag + field names +
        // snake_case dir + a-side fraction), pinning the persisted schema.
        let dock = Workspaces::default_layout(WorkspaceId::Chart);
        let topo = extract_topology(&dock).expect("chart layout");
        let json = serde_json::to_string(&topo).expect("chart topology serializes");
        let expected: DockTopology = serde_json::from_str(
            r#"{"root":{"split":{"dir":"vertical","fraction":0.75,
                  "a":{"split":{"dir":"horizontal","fraction":0.25,
                       "a":{"leaf":{"tabs":["watchlist"]}},
                       "b":{"leaf":{"tabs":["chart"]}}}},
                  "b":{"leaf":{"tabs":["logger"]}}}}}"#,
        )
        .expect("design §9.1 example parses");
        assert_eq!(topo, expected);
        assert_eq!(
            serde_json::to_string(&expected).unwrap(),
            json,
            "serialized shape must match the design §9.1 example exactly"
        );
    }

    #[test]
    fn topology_validation_rejects_bad_fraction() {
        // (0.0, 1.0) exclusive: 0/1 endpoints (egui_dock drags never produce
        // real ones), NaN and out-of-range values are all corrupted.
        for bad in [0.0, 1.0, -0.1, 1.5, f32::NAN, f32::INFINITY] {
            let topo = DockTopology {
                root: DockNode::Split {
                    dir: DockDir::Vertical,
                    fraction: bad,
                    a: Box::new(DockNode::Leaf {
                        tabs: vec![EditorKind::Chart],
                    }),
                    b: Box::new(DockNode::Leaf {
                        tabs: vec![EditorKind::Logger],
                    }),
                },
            };
            assert!(topo.validate().is_err(), "fraction {bad} must be rejected");
        }
    }

    #[test]
    fn topology_validation_rejects_empty_leaf_and_duplicate_kinds() {
        let empty_leaf = DockTopology {
            root: DockNode::Leaf { tabs: vec![] },
        };
        assert!(
            empty_leaf.validate().is_err(),
            "empty leaf must be rejected (Node::Empty intermediate state)"
        );

        let dup = DockTopology {
            root: DockNode::Split {
                dir: DockDir::Vertical,
                fraction: 0.5,
                a: Box::new(DockNode::Leaf {
                    tabs: vec![EditorKind::Chart],
                }),
                b: Box::new(DockNode::Leaf {
                    tabs: vec![EditorKind::Chart],
                }),
            },
        };
        assert!(
            dup.validate().is_err(),
            "duplicate editor kind must be rejected (one instance per kind, design §4.5)"
        );
    }

    #[test]
    fn topology_corrupt_json_errors_not_panics() {
        // Corrupted [layout] dock strings must be a serde Err — never a
        // panic on the startup path (design §9.1: fall back to defaults).
        assert!(serde_json::from_str::<DockTopology>("").is_err());
        assert!(serde_json::from_str::<DockTopology>(r#"{"root":null}"#).is_err());
        assert!(
            serde_json::from_str::<DockTopology>(r#"{"root":{"split":{"tabs":[]}}}"#).is_err(),
            "split without dir/fraction must Err"
        );
        assert!(
            serde_json::from_str::<DockTopology>(r#"{"root":{"leaf":{"tabs":["bogus"]}}}"#)
                .is_err(),
            "unknown EditorKind string must Err"
        );
    }

    #[test]
    fn topology_rebuild_handles_engineered_layout() {
        // The historical inline layout (Chart/Market/Sepa top + Logger 0.75
        // + Screener 0.5 below) round-trips through the topology path — the
        // guard that used to be `screen_layout_serde_roundtrip_preserves_tree`.
        // (Direct `DockState<Tab>` serde stays off the persistence path:
        // egui_dock#197, https://github.com/anhosh/egui_dock/issues/197.)
        let original = chart_engineered_layout();
        let topo = extract_topology(&original.dock_state).expect("engineered layout");
        topo.validate().expect("engineered topology valid");
        let rebuilt = dock_state_from_topology(&topo);
        let topo2 = extract_topology(&rebuilt).expect("rebuilt engineered layout");
        assert_eq!(
            topo, topo2,
            "topology round-trip 不保真：树/fraction/tab 序列在持久化中丢失 → 阶段 4 无法恢复布局"
        );
    }

    #[test]
    fn screen_layout_corrupt_json_errors_not_panics() {
        // 损坏配置必须是一声 Err，绝不允许 panic 让 app 启动失败
        // （设计 §9.1：load 路径回退默认的前提是类型层只报错不炸）。
        let json = serde_json::to_string(&chart_engineered_layout()).unwrap();
        let truncated = &json[..json.len().saturating_sub(3)];
        assert!(
            serde_json::from_str::<ScreenLayout>(truncated).is_err(),
            "截断 JSON 必须 Err"
        );
        assert!(
            serde_json::from_str::<ScreenLayout>(r#"{"active_tab":null}"#).is_err(),
            "dock_state 缺失（必填字段）必须 Err"
        );
        assert!(
            serde_json::from_str::<ScreenLayout>(r#"{"dock_state":123,"active_tab":null}"#)
                .is_err(),
            "dock_state 类型不匹配必须 Err"
        );
        assert!(serde_json::from_str::<ScreenLayout>("").is_err());
    }

    #[test]
    fn active_tab_path_serde_roundtrip() {
        // ScreenLayout.active_tab 的持久化前提：TabPath + 三个索引类型
        // 均必须可 serde（egui_dock serde feature 依赖链，缺一即断）。
        let path = egui_dock::TabPath::new(
            egui_dock::SurfaceIndex::main(),
            egui_dock::NodeIndex::root(),
            egui_dock::TabIndex::from(0),
        );
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(
            serde_json::from_str::<egui_dock::TabPath>(&json).unwrap(),
            path
        );
    }

    #[test]
    fn workspace_serde_omitted_layout_fields_default() {
        // 向后兼容契约：无 layouts / active_screen 字段的老配置必须可加载
        // 并取文档默认值；id 缺失 / 非法字符串必须 Err 而非 panic。
        let ws: Workspace = serde_json::from_str(r#"{"id":"chart"}"#).unwrap();
        assert_eq!(ws.id, WorkspaceId::Chart);
        assert!(
            ws.layouts.is_empty(),
            "layouts 字段缺省必须为默认（空 vec）"
        );
        assert_eq!(ws.active_screen, 0, "active_screen 字段缺省必须为 0");

        let ws: Workspace =
            serde_json::from_str(r#"{"id":"sepa","layouts":[],"active_screen":0}"#).unwrap();
        assert_eq!(ws.id, WorkspaceId::Sepa);
        assert_eq!(ws.active_screen, 0);

        assert!(
            serde_json::from_str::<Workspace>(r#"{}"#).is_err(),
            "id 缺失必须 Err（id 是主键，无默认）"
        );
        assert!(
            serde_json::from_str::<Workspace>(r#"{"id":"bogus"}"#).is_err(),
            "非法 id 字符串必须 Err（load 路径负责回退默认，类型层只报错）"
        );
    }
}
