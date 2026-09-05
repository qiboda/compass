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
//! Phase 0 ships the type skeleton only; most items are consumed by phases
//! 1-5 (default_layout trees, EditorView impls, Workspaces wiring). The
//! `dead_code` allowance is expected until then and should be revisited when
//! the last skeleton consumer lands (plan §2.3).
#![allow(dead_code)]

use egui_dock::DockState;
use serde::{Deserialize, Serialize};

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
            sidebar: Some(SidebarLayout {
                default_visible: true,
                default_width: 240.0,
                width_range: (200.0, 320.0),
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
    /// `main.rs:156-174` (design §4.4/§5). Real trees land in phase 1.
    pub fn default_layout(id: WorkspaceId) -> DockState<Tab> {
        let _ = id;
        unimplemented!("phase 1: per-workspace default dock trees")
    }
}

// ---------------------------------------------------------------------------
// EditorView + EditorCtx + EditorFrame (SpaceData 实例层)
// ---------------------------------------------------------------------------

/// Editor instance trait — replaces the 16-field `TabViewer` borrow struct
/// (friction F5). `body` is required; `header`/`sidebar` render only when
/// the `EditorLayout` registers the slot (empty impl costs nothing).
pub trait EditorView {
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
}

/// Runtime chrome wrapper (ARegion 类比): Header bar + (Sidebar | body)
/// split, unified N-key visibility, width drag, Display Options slot.
/// Implementation lands with the phase-2 editor migrations (design §4.3).
pub struct EditorFrame {
    pub sidebar_visible: bool,
}

impl EditorFrame {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        desc: &EditorDescriptor,
        editor: &mut impl EditorView,
        ctx: &mut EditorCtx<'_>,
    ) {
        let _ = (ui, desc, editor, ctx);
        unimplemented!("phase 2: EditorFrame chrome wrapper")
    }
}

/// Placeholder for the Outliner-style watchlist editor (design §6);
/// filled in during phase 2f (migration of `render_sidebar`).
pub struct WatchlistEditor;

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
    /// Kind → mutable instance dispatch. Phase 2 wires the real match once
    /// each citizen implements `EditorView`.
    pub fn get_mut(&mut self, kind: EditorKind) -> &mut dyn EditorView {
        let _ = kind;
        unimplemented!("phase 2: per-kind dispatch once citizens implement EditorView")
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
    }

    #[test]
    fn chart_and_screener_sidebar_spec_follows_contract() {
        // Design §4.1: default 240.0 / 200.0..=320.0 — aligned with
        // main.rs:905-913 (pre-migration).
        for kind in [EditorKind::Chart, EditorKind::Screener] {
            let desc = EDITOR_REGISTRY
                .iter()
                .find(|d| d.kind == kind)
                .expect("kind in registry");
            let sb = desc.layout.sidebar.expect("sidebar registered");
            assert_eq!(sb.default_width, 240.0);
            assert_eq!(sb.width_range, (200.0, 320.0));
        }
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
    // default_layout signature（signature stub; real trees in phase 1）
    // ------------------------------------------------------------------

    #[test]
    fn default_layout_signature_contract() {
        let _: fn(WorkspaceId) -> DockState<Tab> = Workspaces::default_layout;
    }
}
