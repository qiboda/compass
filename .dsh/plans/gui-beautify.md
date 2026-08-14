# gui-beautify - Work Plan

## TL;DR (For humans)

**What you'll get:** compass GUI 换装成专业金融终端风格 — 3 套可切换主题（暗色/亮色/深蓝），工具栏带 Phosphor 图标，右上角 toast 通知，确认框弹窗，文件选择器。全局视觉统一，关闭后记住主题偏好。

**Why this approach:** 封装 egui-charts fork 已有的 Theme 系统而非从零自建，避免重复造轮子。theme.rs 只做适配层（预设定义 + 切换逻辑 + 持久化），底层颜色由 egui-charts 的 `Theme::apply_to_config()` 和 `apply_to_egui()` 驱动。toast/modal 自建是因为外部库不维护或版本不兼容。

**What it will NOT do:** 不改变 DockArea 布局结构，不引入 egui_colors/egui-notify/egui-modal，不换字体，不做 Card/Sidebar/StatusBar（Wave 3）。

**Effort:** Medium (6 batches, ~10 files)
**Risk:** Low — egui-charts Theme API 已验证支持完整颜色配置
**Decisions to sanity-check:** 封装 egui-charts Theme 而非自建（D1），toast/modal 自建而非用外部库（D4/D5）

Your next move: 批准后执行。完整实施细节如下。

---

> TL;DR (machine): Medium effort, Low risk. Wrap egui-charts Theme as compass adapter layer + egui-phosphor icons + self-built toast/modal + egui-file-dialog. 6 batches, 2 waves, 10 files.

## Scope
### Must have
- `crates/compass-core/src/model.rs`: AppConfig 加 `theme: Option<String>` 字段 + 解析测试
- `crates/compass/src/theme.rs`: 封装 egui-charts Theme 适配层 — 3 套预设 (compass_dark/compass_light/compass_blue)，`apply_theme()` 调用 egui-charts 的 `apply_to_egui()`，`apply_to_chart()` 调用 `Theme::apply_to_config()` + 十字准线色
- `crates/compass/src/widgets/toast.rs`: Toast 通知 — 右上角叠放，4 类型 (info/success/warning/error)，Phosphor 图标 + 文字 + 进度条，3 秒自动消失
- `crates/compass/src/widgets/modal.rs`: Modal 确认框 — 全屏半透明遮罩 + 居中面板，title + body + OK/Cancel，Tab 焦点锁定，`egui::Area` 实现
- `crates/compass/src/main.rs`: CompassApp 加字段 (theme, dock_style, toast, modal)，ui() 首行主题应用，工具栏 Phosphor 图标+文字，主题切换 PALETTE 按钮+下拉，toast/modal 渲染层
- `crates/compass/src/citizens/chart.rs`: 删除每帧 `Theme::dark()` + `apply_to_egui()` 调用，改用 `self.theme.apply_to_chart()`
- `crates/compass/src/tabs.rs`: 可选 dock_style 同步（视觉验证后决定）
- `crates/compass/Cargo.toml`: 新依赖 egui-phosphor="0.13", egui-file-dialog="0.14"
- `crates/compass/src/widgets/mod.rs`: 加 `pub mod toast; pub mod modal;`
- 测试: theme 解析/切换/持久化，toast 显示/关闭/自动消失，modal 打开/关闭/遮罩
- 文档: `kb/user/gui.md`, `kb/user/config.md`, 实施后 `kb/dev/reflections.md`

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不引入 egui_colors / egui-notify / egui-modal
- 不换字体 (保持 SourceHanSansCN)
- 不改变 DockArea 布局结构 (Wave 3)
- 不实现 Card/Sidebar/StatusBar 自定义组件 (Wave 3)
- 不实现 egui-data-table / egui_tiles
- 不删除 `chart.rs` 的 `Theme::dark()` 前确认 `theme.rs::apply_theme()` 已就位

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after (exploratory first, characterization tests after)
- Framework: rstest + tokio::test for state logic, cargo test for compilation, lsp_diagnostics for type errors
- Widget tests: scoped to pure state logic (push/pop/expire for toast, open/close for modal). Visual assertions deferred to manual QA (F3). egui::Context creation in unit tests uses `egui::Context::default()` where needed.
- Evidence: cargo test output, lsp_diagnostics clean, cargo build success

## Execution strategy

### Wave 1 — Foundation (Batches 1-3)
Theme system + persistence + chart integration + icons.

### Wave 2 — UX (Batches 4-6)
Toast + modal + file-dialog. Depends on Wave 1 completing (needs theme applied globally).

### Render layer order (from bottom to top in ui())
1. Toolbar
2. DockArea (Chart + Logger)
3. Toast overlay (TopRight anchor, via egui::Area)
4. Modal overlay (fullscreen, via egui::Area with modal=true)

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | — | 2, 3 | — |
| 2 | 1 | 4, 6 | 3 |
| 3 | 1 | 5, 9 | 2 |
| 4 | 2, 3 | — | 5 |
| 5 | 3, 6 | — | 4 |
| 6 | 2 | 7, 8 | 9 |
| 7 | 6 | — | 8, 9 |
| 8 | 6 | — | 7, 9 |
| 9 | 3, 6 | — | 7, 8 |
| 10 | 1-9 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.

- [ ] 1. model.rs: Add theme field to AppConfig with serde parsing
  What to do: Add `#[serde(default)] pub theme: Option<String>` to AppConfig struct. Add `AppConfig::resolve_theme()` -> `ThemePreset` helper. Add TOML parsing test for `theme = "compass_blue"`.
  Must NOT: Do NOT put theme in AppSection — it's an app-level config, not app-behavior config.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2, 3
  References: `crates/compass-core/src/model.rs:169-180` (AppConfig), `:230-254` (AppSection), `:322-339` (tests)
  Acceptance criteria: `load_config()` returns `AppConfig { theme: Some("compass_dark") }` from TOML with `[theme] compass_dark`. `resolve_theme()` maps "compass_dark" → custom ThemePreset. None → Dark default.
  QA scenarios: happy: `parse config with theme = "compass_light"` → resolves to light. failure: `parse config with theme = "invalid"` → falls back to Dark. Evidence: `cargo test -p compass-core -- theme`
  Commit: Y | feat(core): add theme field to AppConfig

- [ ] 2. theme.rs: Create compass theme adapter wrapping egui-charts Theme
  What to do: Create `crates/compass/src/theme.rs`. Define `CompassTheme` struct holding `egui_charts::theme::Theme`. Define 3 presets via `CompassTheme::compass_dark()`, `compass_light()`, `compass_blue()` each returning `CompassTheme`. Implement `apply_theme(ctx)` calling `egui_charts::theme::apply_to_egui(ctx, &self.inner)`. Implement `apply_to_chart(chart: &mut Chart)` calling `chart.config = self.inner.apply_to_config(chart.config.clone())` AND setting `chart.chart_options.crosshair.*_color` from `self.inner.semantic.chart.crosshair_line`. Add `theme_names()` -> `&[&str]`.
  Must NOT: Do NOT define colors from scratch — always derive from `Theme::from_preset()` with overrides. Do NOT forget crosshair colors in `ChartOptions`.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 4, 6
  References: `/data/codes/egui-charts/src/theme/mod.rs:347-361` (apply_to_config), `/data/codes/egui-charts/src/theme/mod.rs:262+` (apply_to_egui), `/data/codes/egui-charts/src/config/crosshair.rs:48-84` (CrosshairOptions), `crates/compass/src/citizens/chart.rs:59-60` (current Theme usage)
  Acceptance criteria: `CompassTheme::compass_dark().apply_theme(ctx)` sets dark visuals on ctx. `apply_to_chart(chart)` updates chart.config.bullish_color AND chart.chart_options.crosshair.vert_line_color. `theme_names()` returns ["compass_dark", "compass_light", "compass_blue"].
  QA scenarios: happy: apply compass_dark → ctx.style().visuals.dark_mode == true, chart.config.background_color is dark. failure: apply to null context → no panic (handled gracefully at call site). Evidence: `cargo test -p compass -- theme::tests`
  Commit: Y | feat(gui): add compass theme adapter wrapping egui-charts

- [ ] 3. Cargo.toml + fonts: Add egui-phosphor dependency + font registration
  What to do: Add `egui-phosphor = "0.13"` to `crates/compass/Cargo.toml` after egui-charts. Add `egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular)` in `setup_cjk_fonts()` before `ctx.set_fonts(fonts)`. Verify font loads correctly.
  Must NOT: Do NOT add multiple variants — Regular only (500KB). Do NOT remove CJK font registration.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 5, 9
  References: `crates/compass/Cargo.toml:15` (insertion point), `crates/compass/src/main.rs:34-45` (setup_cjk_fonts), `bg_b54fd5eb` (phosphor compat)
  Acceptance criteria: `cargo build` succeeds. `egui_phosphor::regular::CHART_LINE` compiles.
  QA scenarios: happy: build passes with phosphor dep. failure: missing variant → compile error caught. Evidence: `cargo build -p compass`
  Commit: Y | feat(gui): add egui-phosphor dependency and font registration

- [ ] 4. chart.rs: Remove per-frame Theme::dark(), use compass theme
  What to do: In `ChartCitizen` add `theme: CompassTheme` field. In `show()` method, delete lines 59-60 (`Theme::dark()` + `apply_to_egui()`). Instead call `self.theme.apply_to_chart(&mut self.chart)` at start of show(). Remove `use egui_charts::theme::Theme` import.
  Must NOT: Do NOT remove color application entirely — replace it, don't just delete. Do NOT call apply_to_egui here (it's done in main.rs::ui() globally).
  Parallelization: Wave 1 | Blocked by: 2, 3 | Blocks: —
  References: `crates/compass/src/citizens/chart.rs:4` (import), `:9-11` (doc), `:58-69` (show method)
  Acceptance criteria: Compile passes. Chart renders with compass theme colors instead of hardcoded dark. No `Theme::dark()` call remaining in chart.rs.
  QA scenarios: happy: chart renders with compass_dark preset colors (candles green/red match theme). failure: no crash if theme not applied (graceful default). Evidence: `cargo build -p compass`, `lsp_diagnostics crates/compass/src/citizens/chart.rs`
  Commit: Y | refactor(gui): use compass theme in ChartCitizen

- [ ] 5. main.rs: Toolbar Phosphor icons + theme switcher UI
  What to do: Replace toolbar text labels with icon+text: "Symbol:" → `egui_phosphor::regular::MAGNIFYING_GLASS` + "Symbol", "TF:" → `egui_phosphor::regular::CLOCK` + "TF", "Fetch" → `egui_phosphor::regular::DOWNLOAD_SIMPLE` + " Fetch". Add `PALETTE` icon button before theme dropdown ComboBox. Theme dropdown lists compass_dark/compass_light/compass_blue only. UI scaffolding only — actual `self.theme` update is wired in Batch 6.
  Must NOT: Do NOT use pure icons without text labels. Do NOT put theme dropdown before existing toolbar elements — append it at end.
  Parallelization: Wave 1 | Blocked by: 3, 6 | Blocks: —
  References: `crates/compass/src/main.rs:249-303` (render_toolbar), `:251` (Symbol label), `:256` (TF label), `:270` (Fetch button), `:296-301` (spinner + error), `bg_71ab44c5` (phosphor integration map)
  Acceptance criteria: Toolbar shows icon+text for all 4 elements. PALETTE button opens dropdown with theme names. Selecting theme immediately updates visuals.
  QA scenarios: happy: click theme dropdown → select "compass_light" → UI switches to light. failure: phosphor font not loaded → fallback to text-only (graceful). Evidence: `cargo build -p compass`, visual smoke test via `cargo run`
  Commit: Y | feat(gui): add Phosphor icons and theme switcher to toolbar

- [ ] 6. main.rs: Wire theme + dock_style into CompassApp lifecycle
  What to do: Add `theme: CompassTheme` and `dock_style: egui_dock::Style` fields to CompassApp struct. In constructor (run_native closure), resolve theme from `config.resolve_theme()` and init `dock_style = Style::from_egui(ctx.style())`. At top of `ui()`: call `self.theme.apply_theme(ui.ctx())`. Rebuild `self.dock_style` on theme switch. Pass dock_style to `DockArea::new(...).style(self.dock_style.clone())`. Persist theme to config when changed.
  Must NOT: Do NOT rebuild dock_style every frame — only on theme switch. Do NOT call apply_theme more than once per frame.
  Parallelization: Wave 1 | Blocked by: 2 | Blocks: 7, 8
  References: `crates/compass/src/main.rs:54` (config load), `:71-116` (constructor), `:208-219` (CompassApp), `:222-225` (ui() entry), `bg_c3d68e2d` (theme integration map), `bg_0fb9b91a` (dock style API)
  Acceptance criteria: Theme applies globally on startup. Switching theme updates both egui visuals and dock tabs. Dock style colors match active theme.
  QA scenarios: happy: startup with config.theme="compass_light" → light theme active. switch to compass_dark → all UI including dock tabs turns dark. failure: no config file → Dark default. Evidence: `cargo build -p compass`, `lsp_diagnostics crates/compass/src/main.rs`
  Commit: Y | feat(gui): wire theme and dock_style into CompassApp

- [ ] 7. toast.rs: Self-built Toast notification widget
  What to do: Create `crates/compass/src/widgets/toast.rs`. Define `ToastLevel` enum (Info/Success/Warning/Error), `Toast` struct (message, level, created_at, duration), `ToastManager` struct (toasts: Vec<Toast>, max: usize = 10). `ToastManager::push(level, msg)` adds toast. `ToastManager::render(ctx)` draws toasts at TopRight via `egui::Area`, auto-removes expired ones. Info/Success/Warning: 3s timeout. Error: 8s timeout (must be noticed). Each toast shows Phosphor icon + text + progress bar. Add tests: push+render+auto-expire.
  Must NOT: Do NOT use external crate. Do NOT block UI thread for toast timing. Do NOT implement custom toast templates — text+icon only.
  Parallelization: Wave 2 | Blocked by: 6 | Blocks: —
  References: `crates/compass/src/widgets/searchable_dropdown.rs` (widget pattern), `bg_48be8e06` (integration surface), `bg_608d6707` (egui-notify API reference)
  Acceptance criteria: `toast.push(Success, "done")` → toast appears at top-right with check icon. After 3s, toast removed. 4 levels each have distinct icons.
  QA scenarios: happy: push info toast → green icon + text visible top-right → disappears after 3s. failure: push 10 toasts → older ones remain stacked (no overflow crash). Evidence: `cargo test -p compass -- toast::tests`
  Commit: Y | feat(gui): add self-built Toast notification widget

- [ ] 8. modal.rs: Self-built Modal confirmation widget
  What to do: Create `crates/compass/src/widgets/modal.rs`. Define `Modal` struct (is_open, title, body, on_confirm callback). `Modal::show(ctx)` renders fullscreen semi-transparent overlay via `egui::Area` with `Sense::click()` to block input, plus centered Frame panel with title/body/OK/Cancel buttons. OK calls on_confirm and closes. Cancel just closes. Add tests: open → overlay visible → close → overlay gone.
  Must NOT: Do NOT use external crate. Do NOT allow Tab to escape modal. Attempt focus trapping via `ui.ctx().memory_mut(|mem| mem.focus().lock())` or `egui::Area` modal mode. **Known risk**: egui::Area does NOT natively support focus trapping — if Tab-escape proves infeasible within budget, accept as known limitation documented in code comments. Do NOT support arbitrary widget content — title+body text only for now.
  Parallelization: Wave 2 | Blocked by: 6 | Blocks: —
  References: `crates/compass/src/widgets/searchable_dropdown.rs` (widget pattern), `bg_48be8e06` (integration surface), `bg_91b54f74` (egui-modal API reference)
  Acceptance criteria: `modal.open("title", "body")` → overlay covers full window → underlying clicks blocked → click Cancel → modal closed. Tab key cycles only inside modal.
  QA scenarios: happy: open modal → overlay visible → click OK → on_confirm called, modal closed. failure: open modal → Tab 5 times → focus stays within modal panel. Evidence: `cargo test -p compass -- modal::tests`
  Commit: Y | feat(gui): add self-built Modal confirmation widget

- [ ] 9. main.rs: Integrate toast + modal + file-dialog into CompassApp
  What to do: Add `toast: ToastManager`, `modal: Modal`, `file_dialog: FileDialog` fields to CompassApp. Initialize in constructor with `FileDialog::new()`. In `ui()` after DockArea: call `self.toast.render(ui.ctx())` then `self.modal.show(ui.ctx())`. Replace `main.rs:300` error colored_label with `self.toast.push(Error, err)` (error toasts use 8s timeout for visibility). Add file dialog trigger as placeholder button. Wire `self.file_dialog.update(ui)` for result handling. Note: file dialog is thin integration — no persistence, no custom filters, no initial directory config. Full file dialog UX deferred to future work.
  Must NOT: Do NOT put toast/modal inside DockArea — they must render on top of everything. Do NOT forget to call `file_dialog.update(ui)` every frame.
  Parallelization: Wave 2 | Blocked by: 3, 6 | Blocks: —
  References: `crates/compass/src/main.rs:208-219` (CompassApp), `:221-246` (ui loop), `:299-301` (error label), `bg_48be8e06` (toast/modal/filedialog integration map)
  Acceptance criteria: Error during Fetch → toast appears top-right. Modal opens on confirm action. File dialog opens on button click and returns selected path.
  QA scenarios: happy: fetch fail → red toast "Data load error" appears → disappears after 3s. modal open → full overlay blocks chart interaction. file dialog → select parquet → path returned. Evidence: `cargo build -p compass`, `lsp_diagnostics crates/compass/src/main.rs`
  Commit: Y | feat(gui): integrate toast, modal, and file-dialog into CompassApp

- [ ] 10. kb/ docs: Update gui.md, config.md, reflections.md
  What to do: Update `kb/user/gui.md` — add theme switching section, toolbar icon description. Update `kb/user/config.md` — add `theme` field to schema and defaults table. Invoke `/reflect` to write `kb/dev/reflections.md` entry.
  Must NOT: Do NOT modify kb/design/architecture.md unless theme system significantly changes architecture (it wraps existing Theme, so minimal impact).
  Parallelization: Wave 2 | Blocked by: 1-9 | Blocks: —
  References: `kb/user/gui.md`, `kb/user/config.md:35-55`, `.opencode/skills/reflect/SKILL.md`
  Acceptance criteria: gui.md mentions theme switching and toolbar icons. config.md documents `theme` field with valid values. reflections.md has post-implementation entry.
  QA scenarios: Read updated docs → verify all new features documented. Evidence: manual review of kb/ file changes
  Commit: Y | docs: update kb/ for GUI beautification

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE.

- [ ] F1. Plan compliance audit: verify all 10 todos completed, all scope IN items addressed, no scope OUT items slipped in
- [ ] F2. Code quality review: `cargo test && cargo clippy -- -D warnings && cargo fmt --check` all pass
- [ ] F3. Real manual QA: `cargo run` → verify toolbar icons visible, theme dropdown works, toast appears on fetch, chart renders with correct colors
- [ ] F4. Scope fidelity: verify no egui_colors/egui-notify/egui-modal deps in Cargo.toml, DockArea layout unchanged, font unchanged

## Commit strategy

| # | Commit |
|---|---|
| 1 | `feat(core): add theme field to AppConfig` — ref #72 |
| 2 | `feat(gui): add compass theme adapter wrapping egui-charts` — ref #72 |
| 3 | `feat(gui): add egui-phosphor dependency and font registration` — ref #72 |
| 4 | `refactor(gui): use compass theme in ChartCitizen` — ref #72 |
| 5 | `feat(gui): add Phosphor icons and theme switcher to toolbar` — ref #72 |
| 6 | `feat(gui): wire theme and dock_style into CompassApp` — ref #72 |
| 7 | `feat(gui): add self-built Toast notification widget` — ref #73 |
| 8 | `feat(gui): add self-built Modal confirmation widget` — ref #73 |
| 9 | `feat(gui): integrate toast, modal, and file-dialog into CompassApp` — ref #73 |
| 10 | `docs: update kb/ for GUI beautification` — ref #45 |

All ref sub-issues #72 (Wave 1) and #73 (Wave 2). Epic: #45.

9 atomic commits, single PR from `pr/gui-beautify` branch. Push only on explicit user command.

## Success criteria

1. 3 套主题可切换，全局 UI + dock tabs + chart candles 同步变色
2. 工具栏按钮带 Phosphor 图标 + 文字
3. 关闭重开后主题偏好保留（AppConfig 持久化）
4. Toast 通知正常弹出并自动消失（4 种类型）
5. Modal 确认框阻挡底层交互
6. 文件选择器可正常打开
7. `cargo test` 全绿，`cargo clippy` 无警告
8. `kb/` 文档已更新
