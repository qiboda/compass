# egui-mobius-integration - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->

**What you'll get:** Compass GUI rebuilt on egui-mobius Level 3 architecture — three dockable panels (Chart, Controls, Log) wired with reactive state and async backend, replacing the current single-panel mpsc + Mutex approach.

**Why this approach:** Level 3 AsyncDispatcher + typed signal/slot matches compass's existing tokio worker pattern exactly. Citizens provide panel composability without rewiring; Dynamic<T> eliminates manual change detection (bars_version) and lock contention.

**What it will NOT do:** No new features (search, multi-symbol, etc.). No changes to data pipeline (CachedProvider, EastMoney, DuckDB stay as-is). egui-charts stays as the chart widget. compass-data CLI crate untouched.

**Effort:** Medium
**Risk:** Medium — egui_citizen is not on crates.io (git dependency); egui_dock 0.20.1 bumps MSRV from 1.85 to 1.92 (project already on 1.96)
**Decisions to sanity-check:** git dependency for egui_citizen (pin to tag later); egui_dock MSRV bump; bars data structure (Vec not HashMap)

Your next move: approve. Full execution detail follows below.

---

> TL;DR (machine): Medium / Medium / 3 citizens + Level 3 wiring + dead code cleanup

## Scope
### Must have
- Add egui_mobius 0.5.0, egui_mobius_reactive 0.5.0, egui_lens 0.5.0, egui_dock 0.20.1, egui_citizen (git) dependencies
- Create `SharedState` with 6 `Dynamic<T>` fields: symbol, timeframe, bars, loading, error, log
- Create 3 citizen panels: ControlCitizen, ChartCitizen, Logger
- Wire Level 3: `Signal<FetchRequest>`, `Slot<FetchResponse>`, `AsyncDispatcher`
- Rewrite `main.rs`: DockArea layout, drain loop, outbox processing
- Delete dead code: `bars_version`, `search_results`, `Cmd::SearchSymbols`, `retry_count`
- Cleanup: remove unused deps (`anyhow`, `serde` from compass), remove stale `#[allow]` in parquet.rs
- Update kb/design/architecture.md
- Write reflection in kb/dev/reflections.md

### Must NOT have (guardrails, anti-slop, scope boundaries)
- NO new features (search, multi-symbol switching, etc.)
- NO changes to data pipeline (CachedProvider, EastMoneyProvider, DuckDbProvider, ParquetProvider)
- NO replacement of egui-charts
- NO modification of compass-data CLI crate
- NO modification of integration tests or benchmarks
- NO config format changes
- NO `unwrap()` in production code (use `.expect()` with message)

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD — write failing tests before implementation, confirm failure, then implement
- Evidence: .omo/evidence/task-<N>-egui-mobius-integration.txt

## Execution strategy
### Parallel execution waves

**Wave 1** (parallel, no dependencies): Dependency + Dead Code
**Wave 2** (parallel, depends on Wave 1): New files (state, messages, tabs, backend)
**Wave 3** (parallel, depends on Wave 2): Citizens (control, chart, logger)
**Wave 4** (depends on Wave 3): App integration (dispatcher + main.rs)
**Wave 5** (depends on Wave 4): Tests + Docs

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
|------|-----------|--------|---------------------|
| 1 | — | 4-7 | 2, 3 |
| 2 | — | 4-7 | 1, 3 |
| 3 | — | 4-7 | 1, 2 |
| 4 | 1-3 | 8-10 | 5, 6, 7 |
| 5 | 1-3 | 8-10 | 4, 6, 7 |
| 6 | 1-3 | 8-10 | 4, 5, 7 |
| 7 | 1-3 | 8-10 | 4, 5, 6 |
| 8 | 4-7 | 11-12 | 9, 10 |
| 9 | 4-7 | 11-12 | 8, 10 |
| 10 | 4-7 | 11-12 | 8, 9 |
| 11 | 8-10 | 12 | — |
| 12 | 8-11 | 13-14 | — |
| 13 | 12 | 14 | — |
| 14 | 12 | — | 13 |

## Todos

- [ ] 1. crates/compass/Cargo.toml: Add egui-mobius ecosystem dependencies for Level 3 integration
  What to do: Add `egui_mobius`, `egui_mobius_reactive`, `egui_lens`, `egui_dock` crates.io deps + `egui_citizen` git dep. Remove unused `anyhow` and `serde` deps.
  Must NOT do: Do NOT add egui-charts replacement. Do NOT remove any other existing deps.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 4-7
  References: drafts/egui-mobius-integration.md (Approval gate section)
  Acceptance criteria: `cargo update && cargo build` succeeds with no new errors
  QA scenarios: `cargo build 2>&1` — expect success; `cargo tree -p compass | grep mobius` shows new deps
  Commit: Y | chore(compass): add egui-mobius dependencies, remove unused anyhow/serde

- [ ] 2. crates/compass-core/src/model.rs: Remove dead code (bars_version, search_results, Cmd::SearchSymbols, retry_count)
  What to do: Delete `bars_version` field from CompassState and its initialization. Delete `search_results` field. Delete `Cmd::SearchSymbols` variant and `#[allow(dead_code)]`. Delete `retry_count` field from ApiConfig and `default_retry_count()`. Update `set_bars()` to not bump version. Update CompassState::new() to not init removed fields.
  Must NOT do: Do NOT change existing fields (current_symbol, current_timeframe, bars, loading, error). Do NOT change SymbolInfo, RealtimeQuote, StockBasic structs. Do NOT delete `BarsMap` type alias.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 4-7
  References: model.rs:237 (bars_version), model.rs:233 (search_results), model.rs:103-108 (Cmd::SearchSymbols), model.rs:149 (retry_count), model.rs:201 (default_retry_count), model.rs:256-261 (set_bars)
  Acceptance criteria: `cargo build` succeeds. Unit tests that reference removed fields fail (RED phase — expected).
  QA scenarios: `cargo test -p compass-core 2>&1` — expect compile errors or test failures for bars_version tests; `cargo build -p compass-core 2>&1` — expect success
  Commit: Y | refactor(compass-core): remove dead code (bars_version, search_results, Cmd::SearchSymbols, retry_count)

- [ ] 3. crates/compass-core/src/data/parquet.rs: Remove stale #[allow(dead_code)] from clone_reader()
  What to do: Remove `#[allow(dead_code)]` annotation on line 327. The method IS called at line 294 — the annotation is stale.
  Must NOT do: Do NOT change the method implementation. Do NOT change any other annotations.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 4-7
  References: parquet.rs:294 (call site), parquet.rs:327 (annotation)
  Acceptance criteria: `cargo build -p compass-core` succeeds without dead_code warning on clone_reader
  QA scenarios: `cargo build -p compass-core 2>&1` — no dead_code warning for clone_reader
  Commit: Y | chore(compass-core): remove stale #[allow(dead_code)] from clone_reader

- [ ] 4. crates/compass/src/state.rs: Create SharedState with Dynamic<T> fields
  What to do: Create `src/state.rs` in the compass crate. Define `SharedState` struct with 6 reactive fields: `symbol: Dynamic<String>`, `timeframe: Dynamic<String>`, `bars: Dynamic<Vec<Bar>>`, `loading: Dynamic<bool>`, `error: Dynamic<Option<String>>`, `log: Dynamic<Vec<String>>`. Implement `SharedState::new(default_symbol, default_timeframe)` constructor. Export `pub use state::SharedState;` from a new `lib.rs` or make it accessible from main.rs.
  Must NOT do: Do NOT create Dynamic fields for panel-local state (symbol_input, timeframe_input stay local in ControlCitizen). Do NOT wrap state in Arc or Mutex — Dynamic<T> handles that.
  Parallelization: Wave 2 | Blocked by: 1-3 | Blocks: 8-10
  References: drafts/egui-mobius-integration.md (Decisions #1); filter_plotter tutorial state.rs pattern
  Acceptance criteria: `cargo build -p compass` compiles the new module (unused code warning OK at this stage)
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): add SharedState with Dynamic<T> reactive fields

- [ ] 5. crates/compass/src/messages.rs: Define AppMessage enum and FetchRequest/FetchResponse types
  What to do: Create `src/messages.rs`. Define `AppMessage` enum with `FetchBars` variant. Define `FetchRequest { symbol: String, timeframe: String, range_start: DateTime<Utc>, range_end: DateTime<Utc> }` — plain owned struct, Send + 'static. Define `FetchResponse { symbol: String, timeframe: String, bars: Vec<Bar>, error: Option<String> }`.
  Must NOT do: Do NOT put Dynamic<T> inside FetchRequest/FetchResponse — these cross the signal boundary as plain values. Do NOT re-add Cmd::SearchSymbols.
  Parallelization: Wave 2 | Blocked by: 1-3 | Blocks: 8-10
  References: drafts/egui-mobius-integration.md (Decisions #2); citizen_signal_async example WorkRequest/WorkResponse pattern
  Acceptance criteria: `cargo build -p compass` compiles
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): define AppMessage and FetchRequest/FetchResponse types

- [ ] 6. crates/compass/src/tabs.rs: Define TabKind, Tab, and TabViewer
  What to do: Create `src/tabs.rs`. Define `TabKind` enum (Control, Chart, Logger). Define `Tab` wrapper with `title()` and `citizen_id()` methods. Define citizen ID constants (CONTROL_ID, CHART_ID, LOGGER_ID). Define `TabViewer` struct with references to SharedState, Dispatcher, and each citizen panel. Implement `egui_dock::TabViewer` trait.
  Must NOT do: Do NOT render panels inside TabViewer::ui() directly — delegate to citizen.show().
  Parallelization: Wave 2 | Blocked by: 1-3 | Blocks: 8-10
  References: filter_plotter example tabs.rs; citizen_signal_async example tabs.rs
  Acceptance criteria: `cargo build -p compass` compiles (may have unused import warnings until citizens exist)
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): add TabKind, Tab, and TabViewer for citizen dock layout

- [ ] 7. crates/compass/src/backend.rs: Wire Level 3 signal/slot + AsyncDispatcher
  What to do: Create `src/backend.rs`. Function `wire_backend(config, shared_state_clones, egui_ctx) -> (Signal<FetchRequest>, BackendHandle)`. Inside: create `factory::create_signal_slot::<FetchRequest>()` and `factory::create_signal_slot::<FetchResponse>()`. Create `AsyncDispatcher::new()`, attach_async with work_slot + result_signal. Work function: async fn that initializes CachedProvider (EastMoney + DuckDB), processes FetchRequest, returns FetchResponse. Result slot handler: writes bars/symbol/timeframe/loading/error to Dynamic<T> fields + calls `ctx.request_repaint()`. Return `BackendHandle` (keeps AsyncDispatcher alive).
  Must NOT do: Do NOT spawn tokio runtime manually — AsyncDispatcher handles it. Do NOT use mpsc channels.
  Parallelization: Wave 2 | Blocked by: 1-3 | Blocks: 8-10
  References: citizen_signal_async backend.rs (full signal/slot wiring); current main.rs:140-233 (worker thread logic to port)
  Acceptance criteria: `cargo build -p compass` compiles
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): wire Level 3 signal/slot with AsyncDispatcher backend

- [ ] 8. crates/compass/src/citizens/control.rs: Create ControlCitizen panel
  What to do: Create `src/citizens/control.rs`. Define `ControlCitizen` struct: `citizen_id`, `citizen_state`, `symbol_input: String`, `timeframe_input: String`, `outbox: Vec<AppMessage>`. Implement `Citizen` trait. `show()` method: renders symbol text input, timeframe ComboBox, Fetch button, loading spinner, error label. On Fetch click: push `AppMessage::FetchBars` to outbox. Read `loading` and `error` from `&SharedState` for display.
  Must NOT do: Do NOT call backend directly. Do NOT write to Dynamic<T> fields (only read loading/error for display).
  Parallelization: Wave 3 | Blocked by: 4-7 | Blocks: 11-12
  References: filter_plotter settings panel (outbox pattern); current main.rs:299-326 (control UI to port)
  Acceptance criteria: `cargo build -p compass` compiles
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): add ControlCitizen panel with symbol input and Fetch outbox

- [ ] 9. crates/compass/src/citizens/chart.rs: Create ChartCitizen panel
  What to do: Create `src/citizens/chart.rs`. Define `ChartCitizen` struct: `citizen_id`, `citizen_state`, `chart: Chart`, `bars_version: u64`. Implement `Citizen` trait. `show()` method: read `bars` from `&SharedState`, detect changes (compare len), rebuild chart data via `chart.update_data(BarData::from_bars(bars))`, render `chart.show(ui)`. Apply theme at frame start.
  Must NOT do: Do NOT fetch data. Do NOT handle user input (that's ControlCitizen). Do NOT spawn threads.
  Parallelization: Wave 3 | Blocked by: 4-7 | Blocks: 11-12
  References: current main.rs:260-266 (chart init), main.rs:285-296 (chart update logic), main.rs:338 (chart show)
  Acceptance criteria: `cargo build -p compass` compiles
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): add ChartCitizen panel with reactive bar updates

- [ ] 10. crates/compass/src/citizens/logger.rs: Create Logger citizen via egui_lens
  What to do: Create `src/citizens/logger.rs`. Wrap `egui_lens::ReactiveEventLogger` — thin wrapper struct that holds citizen_id and citizen_state. `show()` delegates to `ReactiveEventLogger::new(&state.log).show(ui)`. Implement `Citizen` trait.
  Must NOT do: Do NOT implement custom log rendering — egui_lens handles it. Do NOT add extra dependencies beyond egui_lens.
  Parallelization: Wave 3 | Blocked by: 4-7 | Blocks: 11-12
  References: filter_plotter logger panel (thin wrapper pattern); egui_lens 0.5.0 API
  Acceptance criteria: `cargo build -p compass` compiles
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): add Logger citizen via egui_lens

- [ ] 11. crates/compass/src/dispatcher.rs: Create register/drain/handle functions
  What to do: Create `src/dispatcher.rs`. `register_citizens(dispatcher) -> RegisteredCitizens` — register all 3 citizens, activate Chart. `drain_citizen(dispatcher, &SharedState)` — log citizen lifecycle events to Dynamic log. `handle(msg: AppMessage, &SharedState, &Signal<FetchRequest>)` — on FetchBars: snapshot symbol/timeframe, send FetchRequest via signal. Follow source-of-truth discipline: Dynamic<T> is canonical.
  Must NOT do: Do NOT call backend directly. Do NOT manually format log strings — use egui_lens patterns.
  Parallelization: Wave 4 | Blocked by: 8-10 | Blocks: 12
  References: filter_plotter dispatcher.rs; citizen_signal_async dispatcher.rs; Coupling chapter (source-of-truth discipline)
  Acceptance criteria: `cargo build -p compass` compiles
  QA scenarios: `cargo build -p compass 2>&1` — success
  Commit: Y | feat(compass): add citizen dispatcher with register/drain/handle

- [ ] 12. crates/compass/src/main.rs: Rewrite main() and App to citizen pattern
  What to do: Rewrite `main.rs`. Keep `init_tracing()` and `load_config()` unchanged. In `main()`: construct SharedState, wire_backend() to get work_signal + BackendHandle, create Dispatcher + register citizens, construct CompassApp with all pieces. `CompassApp` struct: dock_state, dispatcher, citizens, shared_state, backend_handle, work_signal. `eframe::App::ui()`: render DockArea with TabViewer, drain citizen lifecycle, take control outbox, dispatch messages. Remove: `Arc<Mutex<CompassState>>`, `mpsc::channel`, `start_worker_thread`, old `CompassApp` struct, `Cmd` imports, `_egui_ctx` param.
  Must NOT do: Do NOT change init_tracing or load_config. Do NOT change NativeOptions. Do NOT add new windows or viewports.
  Parallelization: Wave 4 | Blocked by: 8-11 | Blocks: 13-14
  References: current main.rs:21-61 (main), main.rs:240-343 (CompassApp — to be replaced); citizen_signal_async main.rs (dock + drain pattern)
  Acceptance criteria: `cargo build -p compass` succeeds. App launches and shows dock layout with 3 panels.
  QA scenarios: `cargo build -p compass 2>&1` — success; verify old imports (mpsc, Cmd, CompassState, Arc, Mutex) are removed
  Commit: Y | refactor(compass): rewrite main.rs to egui-mobius citizen pattern

- [ ] 13. Rewrite affected tests + add new citizen tests
  What to do: (1) In model.rs tests: rewrite `set_bars_stores_and_bumps_version` → `set_bars_stores_data` (assert bars inserted, no version check). Delete `set_bars_version_wraps`. Keep `set_bars_overwrites_existing_key` and `set_bars_stores_multiple_symbols` but remove version assertions. (2) In main.rs tests: delete `compass_app_new_reads_initial_state` (CompassApp no longer has those fields). Add new test: `shared_state_initializes_with_defaults`. Add new test: `control_citizen_outbox_on_fetch_click`. Add new test: `chart_citizen_detects_bar_changes`. (3) Verify all existing tests pass after refactor.
  Must NOT do: Do NOT delete integration tests or benchmarks. Do NOT change test framework (rstest, tokio::test).
  Parallelization: Wave 5 | Blocked by: 12 | Blocks: —
  References: model.rs:264-331 (affected tests); main.rs:353-378 (affected test); kb/dev/testing.md (test patterns)
  Acceptance criteria: `cargo test` — all tests pass, 0 failures
  QA scenarios: `cargo test 2>&1` — expect "test result: ok" for all crates
  Commit: Y | test: update tests for egui-mobius refactor, remove bars_version tests

- [ ] 14. kb/design/architecture.md + kb/dev/reflections.md: Update documentation
  What to do: (1) Update `kb/design/architecture.md`: replace mpsc/Arc<Mutex> threading section with citizen pattern + Dynamic<T> + AsyncDispatcher. Document new crate structure (citizens/, state.rs, backend.rs, etc.). Update data flow diagram. (2) Append reflection to `kb/dev/reflections.md` following the template: date, issue ref, what was done, what went wrong, lessons learned.
  Must NOT do: Do NOT delete existing architecture content — update sections, don't remove. Do NOT update other kb/ files unless they directly reference changed patterns.
  Parallelization: Wave 5 | Blocked by: 12 | Blocks: —
  References: kb/design/architecture.md (threading + data pipeline sections); kb/dev/reflections.md (reflection template)
  Acceptance criteria: kb files are consistent with new code structure
  QA scenarios: Read kb/design/architecture.md — verify citizen pattern and Dynamic<T> are documented; read kb/dev/reflections.md — verify new entry appended
  Commit: Y | docs: update architecture docs for egui-mobius citizen pattern

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — verify all 14 todos completed, all commits follow format, no scope creep
- [ ] F2. Code quality review — `cargo clippy -- -D warnings && cargo fmt --check` must pass
- [ ] F3. Real manual QA — `cargo run` launches dock layout with 3 panels, Fetch button works, chart renders
- [ ] F4. Scope fidelity — verify no new features added, no config changes, dead code actually removed

## Commit strategy
- One commit per todo (14 commits total)
- Commit format: `type(scope): message ref #24`
- Push after final verification wave passes
- No force-push, no amend after push

## Success criteria
1. `cargo build && cargo test` — all pass, 0 failures
2. `cargo clippy -- -D warnings && cargo fmt --check` — clean
3. App launches with DockArea containing 3 citizens
4. Fetch button triggers backend → chart updates reactively
5. Dead code (bars_version, search_results, Cmd::SearchSymbols, retry_count) fully removed
6. All kb/ docs updated
7. Reflection recorded in kb/dev/reflections.md
