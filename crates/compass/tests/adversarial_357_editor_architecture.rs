//! Adversarial tests — issue #357 (Blender-style editor architecture
//! refactor: Workspace / Screen / Area / Editor five layers).
//!
//! Written at gate 3.5, BEFORE implementation (worktree HEAD = 0182dcb,
//! no `editor/` module yet). Per the RED/GREEN contract the tests must
//! COMPILE and RUN and FAIL ON ASSERTIONS — so this file only attacks the
//! contracts that are attackable today without referencing not-yet-existing
//! types.
//!
//! ## Attack surface covered here: deterministic "old code must go away"
//!   contracts (the plan gives NO fallback path for these, so a correct
//!   implementation MUST make these assertions pass):
//!
//! - phase 0 (plan §2.1): `egui_dock` serde feature enabled in
//!   `crates/compass/Cargo.toml`. Without it `DockState<Tab>` cannot be
//!   persisted at all — phase 4 is unbuildable. Attacking the manifest is
//!   cheaper than DEFERRING on the whole serde round-trip.
//! - phase 2f (plan §4.6): the global watchlist left panel
//!   (`Panel::left("sidebar")`, `sidebar_visible`, `sidebar_search`,
//!   `render_sidebar`) MUST be deleted — Watchlist becomes a dock leaf tab
//!   with NO hardcoded 240px width (user arbitration Q6). Residual global
//!   sidebar code is the #1 source of "works for chart, breaks elsewhere"
//!   regressions when editors migrate into workspaces.
//! - phase 1/3 (plan §3.1/§5.1): the inline `DockState::new(...)` +
//!   `split_below(NodeIndex::root(), ...)` initial-layout construction in
//!   `main.rs` MUST be extracted to `Workspaces::default_layout(id)` and the
//!   app MUST mount the active workspace's dock_state. A leftover inline
//!   construction in `main.rs` means workspace switching can never work.
//! - phase 4.0 (plan §4.0): `Tab.kind` payload switches `TabKind` →
//!   `EditorKind`; `main.rs` must no longer construct tabs via `TabKind::`.
//!
//! ## DEFERRED attack surfaces (types do not exist yet — see delegation
//!   report; re-delegate after the first compilable interface commit):
//!   * `[layout]` persistence: corrupted/truncated/invalid-UTF-8 JSON,
//!     `dock_version` skew (reject future, accept past), `active_workspace`
//!     out-of-range, write-vs-switch race (dirty flag + fingerprint compare)
//!   * `Workspaces::switch`: empty workspace, close-to-last-tab leaf,
//!     NodePath boundaries, rapid switch races
//!   * `EditorCtx`/`EditorInstances::get_mut`: borrow-concurrency semantics
//!   * contextual shortcuts (phase 5): `1/2/3` outside chart workspace,
//!     N-key on sidebar-less editors, focus guard
//!   * parameterized MA/BOLL (phase 2a): period=0 / window > sample count —
//!     compass-core `ma`/`bollinger` already defend ([...] in
//!     `compass-core/src/indicators.rs`: `ma_zero_window_all_none`,
//!     `bollinger_zero_period_all_none`), the NEW attackable surface is the
//!     `MaBollIndicator` parameterization once it ships
//!   * TabKind→EditorKind identity: has NO adversary today — multi-instance
//!     per-area state is explicitly OUT OF SCOPE (plan §0.2, design §4.5
//!     "每 Kind 单实例"); attacking it would be attacking a non-promise
//!   * dock split ratio normalization (0/negative/NaN) on restore — needs
//!     the persistence load path
//!
//! ## RED / GREEN
//!   Current tree: every test FAILS (assertion), i.e. RED.
//!   After phase 0: only `phase0_manifest_enables_egui_dock_serde` passes.
//!   After phase 2f: the three sidebar tests pass.
//!   After phase 3/4.0: the layout-extraction tests pass.

use std::path::Path;

// ---------------------------------------------------------------------------
// Test helpers — read the CURRENT sources at run time (not compile time), so
// a correct implementation makes the assertion flip without touching this
// file. `CARGO_MANIFEST_DIR` is the compass crate root at compile time.
// ---------------------------------------------------------------------------

fn compass_manifest() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Root workspace manifest — the serde feature might land there as a
/// workspace-level dependency table (`egui_dock = { version, features }`)
/// with `egui_dock = { workspace = true }` in the crate manifest.
fn workspace_manifest() -> String {
    // CARGO_MANIFEST_DIR = crates/compass; the workspace root manifest is two
    // levels up (fix: was ../Cargo.toml, which points at crates/Cargo.toml).
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn main_src() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The `egui_dock` dependency line of crates/compass/Cargo.toml.
fn egui_dock_dependency_line(manifest: &str) -> &str {
    manifest
        .lines()
        .find(|l| l.trim_start().starts_with("egui_dock"))
        .unwrap_or_else(|| panic!("egui_dock dependency line missing from Cargo.toml"))
}

// ---------------------------------------------------------------------------
// Phase 0 — egui_dock serde feature (plan §2.1)
// ---------------------------------------------------------------------------

/// RED: #357 — phase 0 — the `egui_dock` serde feature must be enabled.
///
/// Contract: `egui_dock = { version = "0.20", features = ["serde"] }`
/// (plan §2.1, "开 feature，不引新依赖"). Without it `DockState<Tab>` has no
/// `Serialize` — phase 4 persistence is unbuildable, and the whole `[layout]`
/// contract is a lie. Attacked as a manifest assertion because the serde
/// round-trip tests themselves are DEFERRED on the `ScreenLayout` type.
///
/// Expected GREEN: the `egui_dock` dependency line (crate or workspace
/// manifest) contains `features = ["serde"]`.
#[test]
fn phase0_manifest_enables_egui_dock_serde() {
    let manifest = compass_manifest();
    let crate_dep = egui_dock_dependency_line(&manifest);
    let serde_enabled_in_crate =
        crate_dep.contains("features = [") && crate_dep.contains("\"serde\"");
    let workspace = workspace_manifest();
    let ws_enabled = workspace
        .lines()
        .any(|l| l.trim_start().starts_with("egui_dock") && l.contains("\"serde\""));
    assert!(
        serde_enabled_in_crate || ws_enabled,
        "RED: egui_dock serde feature not enabled — phase 4 persistence is \
         unbuildable. Found crate dep line: `{crate_dep}`; workspace manifests \
         with an egui_dock line carrying \"serde\" are accepted."
    );
}

// ---------------------------------------------------------------------------
// Phase 2f — global watchlist left panel must be gone (Q6, plan §4.6)
// ---------------------------------------------------------------------------

/// RED: #357 — phase 2f — `Panel::left("sidebar")` (240px global left
/// watchlist panel) must be deleted.
///
/// Q6 定案: Watchlist 是图表 workspace 左侧独立 dock leaf tab、宽度随 dock
/// split 比例 + 拖拽 + 持久化，**无固定宽度、无全局左栏**。Any residual
/// `Panel::left` in main.rs means the old global sidebar is still coupled to
/// the Application layer (friction F3) and Watchlist cannot live in other
/// workspaces.
///
/// Expected GREEN: no `Panel::left(` in main.rs.
#[test]
fn phase2f_global_watchlist_left_panel_removed() {
    let src = main_src();
    assert!(
        !src.contains("Panel::left("),
        "RED: global `Panel::left(` still present in main.rs — the watchlist \
         must be a dock leaf tab (Q6), not an Application-level sidebar. \
         Hardcoded width `default_size(240.0)` should be gone with it."
    );
    assert!(
        !src.contains("default_size(240.0)"),
        "RED: hardcoded sidebar width `default_size(240.0)` still in main.rs \
         — Q6 forbids a fixed 240px width for the watchlist."
    );
}

/// RED: #357 — phase 2f — `sidebar_visible`/`sidebar_search` state must be
/// migrated into the WatchlistEditor instance (plan §4.6).
///
/// Expected GREEN: neither field name appears in main.rs.
#[test]
fn phase2f_sidebar_visible_state_removed() {
    let src = main_src();
    assert!(
        !src.contains("sidebar_visible"),
        "RED: `sidebar_visible` still in main.rs — the app-level sidebar \
         toggle must vanish; Watchlist visibility is dock tab open/close (P5)."
    );
    assert!(
        !src.contains("sidebar_search"),
        "RED: `sidebar_search` still in main.rs — the search state must move \
         into WatchlistEditor instance state."
    );
}

/// RED: #357 — phase 2f — `render_sidebar` must be removed from main.rs
/// (its body moves into `WatchlistEditor`, plan §4.6).
///
/// Expected GREEN: no `fn render_sidebar(` in main.rs.
#[test]
fn phase2f_render_sidebar_function_removed() {
    let src = main_src();
    assert!(
        !src.contains("fn render_sidebar("),
        "RED: `fn render_sidebar(` still in main.rs — the watchlist body must \
         be owned by the WatchlistEditor, not by CompassApp."
    );
}

// ---------------------------------------------------------------------------
// Phase 1/3 — inline DockState construction must be extracted (plan §3.1/§5.1)
// ---------------------------------------------------------------------------

/// RED: #357 — phase 1/3 — the initial dock layout is no longer built inline
/// in `main.rs` (extracted to `Workspaces::default_layout(id)`).
///
/// Contract: `main.rs:156-174` (DockState::new + split_below(root, ...))
/// physically leaves main.rs; the app mounts the active workspace's
/// dock_state (plan §5.1). A leftover inline construction in main.rs means
/// workspace switching can never mount per-workspace layouts.
///
/// Expected GREEN: none of `DockState::new(` / `split_below(` /
/// `split_left(` / `split_right(` appear in main.rs.
#[test]
fn phase1_3_inline_default_layout_extracted() {
    let src = main_src();
    for needle in [
        "DockState::new(",
        "split_below(",
        "split_left(",
        "split_right(",
    ] {
        assert!(
            !src.contains(needle),
            "RED: `{needle}` still in main.rs — initial layout construction \
             must be extracted to `Workspaces::default_layout(id)` (plan §3.1)"
        );
    }
}

/// RED: #357 — phase 4.0 — main.rs no longer constructs tabs via `TabKind::`
/// (payload switched to `EditorKind`, plan §4.0).
///
/// A3 fallback note: even under the "keep TabKind as internal transition
/// enum" fallback, main.rs must construct tabs with `EditorKind` — a
/// `TabKind::` construction in the app shell means the 1:1 hard coupling
/// (F6) survives.
///
/// Expected GREEN: no `TabKind::` in main.rs.
#[test]
fn phase40_tab_kind_payload_switched() {
    let src = main_src();
    assert!(
        !src.contains("TabKind::"),
        "RED: `TabKind::` still used in main.rs — Tab payload must be \
         EditorKind (plan §4.0); the TabKind↔CitizenId 1:1 hard coupling \
         (friction F6) survives otherwise."
    );
}
