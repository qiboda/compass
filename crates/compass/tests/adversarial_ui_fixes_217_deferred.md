# compass adversarial tests for epic #217 (#218 / #220 / #221) — DEFERRED

Status: **DEFERRED** — adversarial tests for the three compass-crate
sub-issues cannot be landed as compilable RED tests under the current
constraints. This file is the complete hand-off: every test below is fully
written, compiles as-is once placed inside the target `#[cfg(test)]` module,
and is RED against the current implementation.

> **Update (2026-08-09)**: the parallel skwy-requirement-test agent has
> landed RED tests for #218 (`src/citizens/ui_fixes_218.rs`), #220
> (screener.rs L742-846) and #221 (`src/citizens/ui_fixes_221.rs`) by
> mounting under `crate::citizens` (`mod.rs` `#[cfg(test)]`) — a path this
> sandbox still denies. The unique *adversarial* angles not covered by the
> requirement tests are flagged below; the remaining gap is a **permission**
> one, not a missing-interface one.
>
> Also updated: `crates/compass/tests/adversarial_219_fork_formats.rs`
> **has been landed** — a real RED test for the fork formats reachable
> through the public API via the consumer crate (see that file).

## Why DEFERRED (three independent blockers)

1. **Permission** (primary): this workspace denies `edit`/`write` outside
   `**/tests/**`. The compass crate is a **pure-bin crate**
   (`Cargo.toml` has only `[[bin]] main.rs`, no `[lib]`), so its tests all
   live in in-source `#[cfg(test)]` modules under `src/**`
   (`main.rs`, `src/citizens/screener.rs`, `src/citizens/sepa.rs`) — exactly
   the paths the permission set rejects. AGENTS.md anticipates this (three
   layer guarantee, layer 1: "指令约束只改 mod tests 内"), but the harness
   denies the writes mechanically, so the in-source modules cannot be
   touched.
2. **No viable `tests/` fallback** (revised after probe): `crates/compass/tests/`
   integration tests **are** compiled and run by `cargo test -p compass`
   (proven by `probe_bin_tests.rs` — it executes), but they cannot
   `use compass::...` because the crate has **no `[lib]` target** (only
   `[[bin]] main.rs`). The code under attack is crate-private
   (`CompassApp::set_timeframe`, `ScreenerPanel::basic_conditions`,
   `SepaPanel::results_area`, the `build_compass_app` test helpers), so no
   public-API adversarial test is expressible from `tests/`. Exception: the
   fork formats (#219) ARE reachable through the public API of the pinned
   `egui-charts` dependency — landed in
   `adversarial_219_fork_formats.rs`.
3. **Current baseline does not compile** (transient — parallel requirement
   agent is actively editing `ui_fixes_221.rs`; as of the last observed run
   `cargo test -p compass` failed first on `screener.rs:754`
   `kittest::Node` (fixed), then on `ui_fixes_221.rs:119` `find_tab`
   returning `TabPath` vs tuple (in progress)):

```
error[E0308]: mismatched types
   --> crates/compass/src/citizens/ui_fixes_221.rs:119:9
...
expected `TabPath`, found `(_, _)`
```

---

## #218 — K线切换立即重载 (main.rs)

### Adversarial points

| # | Attack | Current behavior (RED) | Plan contract |
|---|--------|------------------------|---------------|
| A1 | `set_timeframe(1)` must sync `shared_state.timeframe` | stays `"1d"` — index/timeframe desync | update `shared_state.timeframe` in `set_timeframe` |
| A2 | switch must dispatch a fetch (loading) | never sets loading | unconditional `fetch_bars()` after index update |
| A3 | rapid 1d→1w→1M leaves consistent final state | `"1d"`, no fetch | last request wins, state consistent |
| A4 | switch back to original timeframe restores state + refetches | never syncs | both switches fetch; state ends `"1d"` |
| A5 | double-click same timeframe is guarded (`idx != timeframe_index`) | nothing happens at all (also nothing on first click) | first click fetches, second is a no-op |
| A6 | startup `default_timeframe="1w"` ⇒ `timeframe_index == 1` | hard-coded `0` at main.rs L162 | derive index from `default_timeframe` via `timeframe_index_from_value` |
| A7 | toolbar Segmented click syncs shared state (UI path) | index changes, state stays `"1d"` | click → `set_timeframe` → sync |

### Ready-to-land code (append to `crates/compass/src/main.rs` `mod tests`)

Also requires the helper `build_compass_app_with_default_timeframe`
(defined below — mirrors `build_compass_app_with_stocks` but constructs
`SharedState::new("SZ000001", default_timeframe)` exactly like `main()` L55-58).

```rust
    // ------------------------------------------------------------------
    // Adversarial tests — #218 (timeframe switch reload + state sync)
    // ------------------------------------------------------------------

    fn build_compass_app_with_default_timeframe(
        egui_ctx: egui::Context,
        default_timeframe: &str,
    ) -> CompassApp {
        let config = AppConfig::default();
        let shared_state = Arc::new(SharedState::new("SZ000001", default_timeframe));
        let (work_signal, run_screener_signal, sepa_signal, _backend_handle) =
            crate::backend::wire_backend(config, shared_state.clone(), egui_ctx);
        let mut dispatcher = Dispatcher::new();
        let registered = crate::dispatcher::register_citizens(&mut dispatcher);
        let theme = CompassTheme::compass_dark();
        let theme_tokens = *theme.tokens();
        let chart = ChartCitizen::new(CitizenId::new(CHART_ID), registered.chart);
        let logger = LoggerPanel::new(CitizenId::new(LOGGER_ID), registered.logger);
        let screener = ScreenerPanel::new(
            CitizenId::new(SCREENER_ID),
            registered.screener,
            None,
            Box::new(|_| {}),
            &theme_tokens,
        );
        let sepa = SepaPanel::new(CitizenId::new(SEPA_ID), registered.sepa, &theme_tokens);
        let stock_picker = StockPicker::new(theme_tokens, "SZ000001", stock_projection());
        let dock_style = egui_dock::Style::default();
        let mut dock_state =
            DockState::new(vec![Tab::new(TabKind::Chart), Tab::new(TabKind::Sepa)]);
        if let Some(surface) = dock_state.get_surface_mut(egui_dock::SurfaceIndex::main())
            && let Some(tree) = surface.node_tree_mut()
        {
            let _ = tree.split_below(
                egui_dock::NodeIndex::root(),
                0.75,
                vec![Tab::new(TabKind::Logger)],
            );
            let _ = tree.split_below(
                egui_dock::NodeIndex::root(),
                0.5,
                vec![Tab::new(TabKind::Screener)],
            );
        }
        let startup_symbol = shared_state.symbol.get();
        CompassApp {
            dock_state,
            dispatcher,
            chart,
            logger,
            screener,
            sepa,
            run_screener_signal,
            sepa_signal,
            screener_industries: Vec::new(),
            screener_boards: Vec::new(),
            shared_state,
            work_signal,
            stock_list: Vec::new(),
            stock_picker,
            timeframe_index: 0,
            theme,
            dock_style,
            _backend_handle,
            toast: ToastManager::new(theme_tokens),
            modal: Modal::new(theme_tokens),
            file_dialog: egui_file_dialog::FileDialog::new(),
            last_error: None,
            last_loading: false,
            last_screener_error: None,
            last_sepa_error: None,
            last_sepa_loading: false,
            last_screener_synced_symbol: startup_symbol,
            sidebar_visible: true,
            sidebar_search: String::new(),
            status_clock: String::new(),
            symbol_input_id: None,
            pending_delete: None,
            delete_confirmed: std::rc::Rc::new(std::cell::RefCell::new(false)),
            startup_modal_shown: false,
        }
    }

    #[test]
    fn adversarial_218_set_timeframe_syncs_shared_state() {
        let mut app = build_compass_app(egui::Context::default());
        app.set_timeframe(1);
        assert_eq!(
            app.shared_state.timeframe.get(),
            "1w",
            "A1: set_timeframe(1) must sync shared_state.timeframe"
        );
        app.set_timeframe(2);
        assert_eq!(app.shared_state.timeframe.get(), "1M");
    }

    #[test]
    fn adversarial_218_set_timeframe_triggers_fetch() {
        let mut app = build_compass_app(egui::Context::default());
        app.set_timeframe(1);
        assert!(
            app.shared_state.loading.get(),
            "A2: timeframe switch must dispatch a FetchBars request"
        );
    }

    #[test]
    fn adversarial_218_rapid_switches_leave_consistent_state() {
        let mut app = build_compass_app(egui::Context::default());
        app.set_timeframe(0);
        app.set_timeframe(1);
        app.set_timeframe(2);
        assert_eq!(
            app.shared_state.timeframe.get(),
            "1M",
            "A3: rapid 1d->1w->1M must end on the last selection"
        );
        assert!(app.shared_state.loading.get(), "A3: a fetch must be in flight");
    }

    #[test]
    fn adversarial_218_switch_back_to_original_syncs_state() {
        let mut app = build_compass_app(egui::Context::default());
        app.set_timeframe(1);
        app.set_timeframe(0);
        assert_eq!(
            app.shared_state.timeframe.get(),
            "1d",
            "A4: switching back must restore the original timeframe"
        );
        assert!(
            app.shared_state.loading.get(),
            "A4: both switches must trigger fetches"
        );
    }

    #[test]
    fn adversarial_218_same_timeframe_click_is_guarded() {
        let mut app = build_compass_app(egui::Context::default());
        app.set_timeframe(1);
        assert_eq!(app.shared_state.timeframe.get(), "1w");
        app.set_timeframe(1);
        assert_eq!(
            app.timeframe_index,
            1,
            "A5: same-index call must not re-fire (idx != timeframe_index guard)"
        );
        assert_eq!(app.shared_state.timeframe.get(), "1w");
    }

    #[test]
    fn adversarial_218_startup_index_matches_default_timeframe() {
        let app = build_compass_app_with_default_timeframe(egui::Context::default(), "1w");
        assert_eq!(app.shared_state.timeframe.get(), "1w");
        assert_eq!(
            app.timeframe_index, 1,
            "A6: startup index must derive from default_timeframe \"1w\""
        );
        let app_1m = build_compass_app_with_default_timeframe(egui::Context::default(), "1M");
        assert_eq!(app_1m.timeframe_index, 2);
        let app_unknown = build_compass_app_with_default_timeframe(egui::Context::default(), "5m");
        assert_eq!(
            app_unknown.timeframe_index, 0,
            "A6b: unknown default_timeframe must fall back to index 0"
        );
    }

    #[test]
    fn adversarial_218_toolbar_click_syncs_shared_state_timeframe() {
        let mut app = build_compass_app(egui::Context::default());
        {
            let mut harness = egui_kittest::Harness::new_ui(|ui| {
                app.render_toolbar(ui);
            });
            harness.run();
            harness.get_by_label("1w").click();
            harness.step();
        }
        assert_eq!(
            app.shared_state.timeframe.get(),
            "1w",
            "A7: Segmented click must sync shared state through set_timeframe"
        );
        assert_eq!(app.timeframe_index, 1);
    }
```

### Interface list needed to un-DEFER #218

- No new public API: only write access to `crates/compass/src/main.rs`
  `#[cfg(test)]` module, plus (for A6's unknown-value fallback) the plan's
  `timeframe_index_from_value(&str) -> usize` helper as a **crate-private
  `fn`** so the test can call it directly — or keep the assertion against
  `build_compass_app_with_default_timeframe("5m")` which needs only the
  production behavior, no helper symbol.

---

## #220 — 选股器原子组 (src/citizens/screener.rs)

### Adversarial points

| # | Attack | Current behavior (RED) | Plan contract |
|---|--------|------------------------|---------------|
| B1 | 500px (below design min >600px) keeps every label+control on one row | `horizontal_wrapped` may split between label and control | each group wrapped in `ui::horizontal` |
| B2 | technical conditions expanded (`ma_enabled` etc.) stay atomic | 210px MA dropdown + checkbox can split | same atomic pattern in `technical_conditions` |
| B3 | multi-line row gap == sm (8px) | egui default `item_spacing.y = 3.0` | outer `horizontal_wrapped` `item_spacing.y = tokens.spacing.sm` |
| B4 | groups still wrap *between* groups at narrow width (guards against over-atomizing) | wraps at arbitrary widget boundaries | group boundaries only |

> Note: parallel skwy-requirement-test agent has already landed B1/B2
> alignment tests (screener.rs L742-846). This record adds the missing
> adversarial angle — **B3 row-gap = sm(8px)** — plus a B4 wrap-boundary
> negative control. Coordinate with the main agent to merge without
> duplicating the y-alignment assertions.

### Ready-to-land code (append to `crates/compass/src/citizens/screener.rs` `mod tests`)

```rust
    // ------------------------------------------------------------------
    // Adversarial tests — #220 (atomic groups: row-gap + wrap boundary)
    // ------------------------------------------------------------------

    /// B3: when the wrapped layout produces multiple rows, the vertical gap
    /// between consecutive rows must be the design token sm (8px), not egui's
    /// default 3px. Current implementation never sets item_spacing.y -> gap is
    /// 3px -> RED.
    #[test]
    fn adversarial_220_multiline_row_gap_uses_sm() {
        let (mut panel, _shared) = panel_with_form();
        let mut harness = egui_kittest::Harness::builder()
            .with_size([500.0, 600.0])
            .build_ui(|ui| panel.basic_conditions(ui));
        harness.run();

        // Collect all label/control rects, cluster by row, then measure the
        // gap between consecutive rows.
        let mut rects: Vec<(f32, f32)> = Vec::new();
        for label in ["行业", "交易所", "板块", "上市时长", "市值(亿)"] {
            if let Some(n) = harness.query_by_label(label) {
                let r = n.rect();
                rects.push((r.min.y, r.max.y));
            }
        }
        for n in harness.query_all_by_label_contains("全部") {
            let r = n.rect();
            rects.push((r.min.y, r.max.y));
        }
        if let Some(n) = harness.query_by_label_contains("不限") {
            let r = n.rect();
            rects.push((r.min.y, r.max.y));
        }
        rects.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut rows: Vec<(f32, f32)> = Vec::new();
        for (top, bottom) in rects {
            if let Some(last) = rows.last_mut() {
                if top - last.1 < 5.0 {
                    last.1 = last.1.max(bottom);
                    continue;
                }
            }
            rows.push((top, bottom));
        }
        assert!(
            rows.len() >= 2,
            "500px must force multiple rows (got {})",
            rows.len()
        );
        for pair in rows.windows(2) {
            let gap = pair[1].0 - pair[0].1;
            assert!(
                (gap - 8.0).abs() <= 1.0,
                "row gap must be spacing.sm (8px), got {gap}px"
            );
        }
    }

    /// B4: groups may wrap between groups at narrow width — a guard against
    /// over-atomizing the layout into one unbreakable line.
    #[test]
    fn adversarial_220_groups_wrap_between_groups_not_inside() {
        let (mut panel, _shared) = panel_with_form();
        let mut harness = egui_kittest::Harness::builder()
            .with_size([500.0, 600.0])
            .build_ui(|ui| panel.basic_conditions(ui));
        harness.run();

        let selects: Vec<_> = harness
            .query_all_by_label_contains("全部")
            .collect();
        assert_eq!(selects.len(), 3);
        // 行业 trigger and 上市时长 dropdown must be on DIFFERENT rows at
        // 500px (proving rows still wrap between groups).
        let industry_y = selects[0].rect().center().y;
        let years_y = harness
            .query_by_label_contains("不限")
            .expect("上市时长 dropdown rendered")
            .rect()
            .center()
            .y;
        assert!(
            (industry_y - years_y).abs() > 1.0,
            "groups must still wrap between groups at 500px"
        );
    }
```

### Interface list needed to un-DEFER #220

- No new API: write access to `crates/compass/src/citizens/screener.rs`
  `#[cfg(test)]` module. Merge with the parallel requirement-test additions
  (y-alignment) into one coherent block.

---

## #221 — SEPA dock 表格 (main.rs + src/citizens/sepa.rs)

### Adversarial points

| # | Attack | Expected current behavior | Plan contract |
|---|--------|---------------------------|---------------|
| C1 | inject `sepa_data` (50 rows) in a docked app, activate 东方SEPA tab, table body row must render | reproduce-first: may be the #221 bug (body invisible in dock) or may pass | root-cause reproduce + regression guard |
| C2 | all 50 rows render (`共 50 行` + each unique symbol label) | depends on C1 | table renders 50 rows in dock |
| C3 | empty data (None) → empty state, no panic | should pass (guard) | empty state without panic |
| C4 | error state renders without panic | should pass (guard) | error branch without panic |
| C5 | row click still dispatches fetch | **not kittest-testable** — TableBuilder row clicks cannot be simulated (documented in sepa.rs tests L806-807) | covered at handler level (`dispatch_symbol_fetch` already tested) |

### Ready-to-land code (append to `crates/compass/src/main.rs` `mod tests`)

Reuses existing `build_compass_app`, `sized_harness`, and
`text_pos_containing` (all in the same module).

```rust
    // ------------------------------------------------------------------
    // Adversarial tests — #221 (SEPA table inside the dock)
    // ------------------------------------------------------------------

    fn sepa_sample_data_50() -> compass_types::SepaData {
        use compass_types::{MarketThermometer, SepaData, SepaDetails, SepaIndicator, SepaRow};
        let rows = (1..=50)
            .map(|rank| SepaRow {
                symbol: format!("SH600{:03}", rank),
                name: format!("股票{:04}", rank),
                rank,
                total_score: 80.0 - rank as f64,
                trend: 20.0,
                theme: 18.0,
                capital: 15.0,
                pattern: 15.0,
                risk: 0.0,
                industry: "白酒".to_string(),
                themes: vec!["茅指数".to_string()],
                latest_price: 1500.0,
                change_pct: 2.5,
                details: SepaDetails {
                    trend: vec![],
                    theme: vec![],
                    capital: vec![],
                    pattern: vec![],
                    risk: vec![],
                },
            })
            .collect();
        SepaData {
            rows,
            thermometer: MarketThermometer {
                score: 72.0,
                position: "半仓 50%".to_string(),
                position_pct: 50.0,
                indicators: vec![SepaIndicator {
                    label: "上涨占比".into(),
                    value_text: "62%".into(),
                    delta_pct: Some(2.0),
                    heat: 0.8,
                }],
            },
            date: "2026-08-02".to_string(),
        }
    }

    fn click_sepa_tab(harness: &mut egui_kittest::Harness<'static, CompassApp>) {
        let shapes: Vec<egui::Shape> = harness
            .output()
            .shapes
            .iter()
            .map(|clipped| clipped.shape.clone())
            .collect();
        let pos = text_pos_containing(&shapes, "东方SEPA").expect("sepa tab title rendered");
        harness.event(egui::Event::PointerMoved(pos + egui::vec2(10.0, 10.0)));
        harness.step();
        harness.event(egui::Event::PointerButton {
            pos: pos + egui::vec2(10.0, 10.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
        harness.event(egui::Event::PointerButton {
            pos: pos + egui::vec2(10.0, 10.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(3);
    }

    #[test]
    fn adversarial_221_dock_sepa_table_body_renders_after_injection() {
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.sepa_data.set(Some(sepa_sample_data_50()));
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        click_sepa_tab(&mut harness);

        let rows = harness.query_all_by_label_contains("SH600001");
        assert!(
            rows.count() > 0,
            "C1: SEPA table body must render the first ranking row inside the dock"
        );
    }

    #[test]
    fn adversarial_221_dock_sepa_renders_all_50_rows() {
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.sepa_data.set(Some(sepa_sample_data_50()));
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        click_sepa_tab(&mut harness);

        let _ = harness.get_by_label("共 50 行");
        for rank in 1..=50 {
            let symbol = format!("SH600{:03}", rank);
            assert!(
                harness.query_all_by_label(&symbol).count() >= 1,
                "C2: row {rank} ({symbol}) must render in the docked table body"
            );
        }
    }

    #[test]
    fn adversarial_221_dock_sepa_empty_state_no_panic() {
        let app = build_compass_app(egui::Context::default());
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        click_sepa_tab(&mut harness);
        let _ = harness.get_by_label("暂无 SEPA 评分数据");
    }

    #[test]
    fn adversarial_221_dock_sepa_error_state_no_panic() {
        let mut app = build_compass_app(egui::Context::default());
        app.shared_state.sepa_error.set(Some("boom".to_string()));
        let mut harness = sized_harness(app);
        harness.run_steps(3);
        click_sepa_tab(&mut harness);
        let _ = harness.query_all_by_label_contains("boom");
    }
```

### Interface list needed to un-DEFER #221

- No new API: write access to `crates/compass/src/main.rs` `#[cfg(test)]`
  module. C5 (row click → fetch inside dock) is explicitly **not
  kittest-testable** (TableBuilder click limitation, already documented in
  `sepa.rs` tests); the handler path is covered by the existing
  `row_click_sets_selected_and_dispatches_fetch` + `dispatch_row_fetch_*`
  tests.

---

## Summary table

| Sub-issue | Adversarial tests landed | Status | Coverage vs requirement agent |
|-----------|--------------------------|--------|------------------------------|
| #218 | code ready (A1-A7); **A3/A4/A5/A6b unique** | DEFERRED (permission: `src/main.rs` mod tests) | requirement agent covered A1/A2/A6/A7 in `ui_fixes_218.rs`; A3 rapid-switch consistency, A4 switch-back, A5 double-click guard, A6b unknown-value fallback are NOT covered |
| #219 | **LANDED** — `crates/compass/tests/adversarial_219_fork_formats.rs` (real RED via consumer-crate public API) | RED | crosshair/tooltip inline formats still DEFERRED (need pure-function extraction) |
| #220 | code ready (B3 row-gap unique; B4 wrap boundary) | DEFERRED (permission: `src/citizens/screener.rs` mod tests) | requirement agent covered B1/B2 y-alignment in screener.rs L742-846; B3 (row gap == sm 8px) and B4 (wrap between groups) are the adversarial additions |
| #221 | code ready (C1-C4; C5 untestable) | DEFERRED (permission: `src/main.rs` mod tests) | requirement agent covered C1/C2 in `ui_fixes_221.rs`; C3 empty-state-no-panic and C4 error-state-no-panic are the adversarial additions |

## Recommended un-DEFER path

1. Main agent grants write access to the in-source `#[cfg(test)]` modules
   (the AGENTS.md layer-1 mechanism) and re-delegates with the same issue
   context — every test in this file is drop-in ready.
2. Fix the baseline compile error first (screener.rs:754 `kittest::Node` →
   the parallel requirement agent's #220 code) — RED requires a compiling
   baseline.
3. For #219 crosshair/tooltip: after the implementer extracts the pure
   format functions (interface list in the fork record), re-delegate with
   the commit SHA for two-stage adversarial tests.

## RED evidence (landed tests)

`cargo test -p compass adversarial_219` on the current tree (egui-charts fork
@ 2b18acd pinned in Cargo.lock):

```
running 4 tests
test adversarial_219_day_of_month_labels_chinese_depadded ... FAILED
test adversarial_219_timezone_cross_day_keeps_chinese ... FAILED
test adversarial_219_month_labels_chinese_depadded ... FAILED
test adversarial_219_zero_padded_forms_are_forbidden ... ok

---- adversarial_219_month_labels_chinese_depadded stdout ----
assertion `left == right` failed: January must render '1月', not '01月' (%-m de-padding)
  left: "Jan"
 right: "1月"

---- adversarial_219_day_of_month_labels_chinese_depadded stdout ----
assertion `left == right` failed
  left: "Jun 15"
 right: "6月15日"

---- adversarial_219_timezone_cross_day_keeps_chinese stdout ----
assertion `left == right` failed: Tokyo conversion crosses midnight to June 16
  left: "Jun 16"
 right: "6月16日"

test result: FAILED. 3 failed; 1 passed (zero-padding guard is expected to
pass until a half-done localization introduces the padded forms)
```

GREEN contract: after the fork formats switch to `%-m月`/`%-m月%-d日` and
`cargo update -p egui-charts` pulls the new commit, all four tests pass.
