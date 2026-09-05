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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
                }
                d
            }
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
    /// [添加] IconButton — migrated from the `Sidebar` widget composite
    /// (plan §4.6; Ctrl+K focus semantics via [`WatchlistEditor::search_input_id`]).
    /// Add clicks are collected into the action out-param (the App inserts
    /// the current symbol); the input mutates `self.search` in place.
    fn header(&mut self, ui: &mut egui::Ui, ctx: &mut EditorCtx<'_>) {
        let tokens = *ctx.theme.tokens();
        let sidebar = Sidebar::new(&tokens);
        let events = ui
            .scope_builder(
                egui::UiBuilder::new().id(egui::Id::new("sidebar_body")),
                |ui| sidebar.search_row(ui, &mut self.search),
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
                |ui| sidebar.show_list(ui, &groups),
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
    // ScreenLayout(→ DockState<Tab>) serde round-trip：阶段 4 布局持久化
    // 的类型层前置。树形态复刻 main.rs:156-174（Chart+Market+Sepa 顶层，
    // root 下 split Logger 0.75 / Screener 0.5）——未来提取到
    // default_layout 后该形态必可持久化。
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

    #[test]
    #[ignore = "egui_dock#197 upstream bug: Rect::NOTHING(±inf) serialized as null by serde_json; \
                phase 4 fix via custom topology serde or dock upgrade, then un-ignore"]
    fn screen_layout_serde_roundtrip_preserves_tree() {
        // 往返字节等价 = 树结构 + split 比例 + tab 身份全部无损。
        // 已知上游阻塞：egui_dock 0.20.1 + serde_json 下 DockState<Tab>
        // round-trip 崩溃（invalid type: null, expected f32）——新节点 rect
        // 初始为 Rect::NOTHING(±∞)，serde_json 将其序列化为 null。
        // 上游同源: https://github.com/anhosh/egui_dock/issues/197
        // 阶段 4 修复路径（二选一，届时评估）:
        //   ① 升级 egui_dock（需查上游修复版本与 egui 兼容性）
        //   ② ScreenLayout 自定义 serde 仅持久化拓扑（rect/viewport 为运行期缓存）
        // 修复后移除 #[ignore] 恢复本测试。
        let original = chart_engineered_layout();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ScreenLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            json,
            "ScreenLayout round-trip 不保真：树/fraction 在持久化中丢失 → 阶段 4 无法恢复布局"
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
