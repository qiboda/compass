# modal-virtual-time - Work Plan

## TL;DR (For humans)

**What you'll get:** 弹窗（Modal）动画不再依赖真实时钟，改用界面引擎的虚拟时间——测试从此与机器快慢无关，彻底消除慢机器上偶发失败的隐患（issue #171，与 toast 弹通知 #168 同类的根治）。

**Why this approach:** 直接沿用 toast 弹通知已验证成功的方案（egui 虚拟时间 `ctx.input(|i| i.time)`），同一代码库同一模式，测试用细粒度步进精确跨过动画时长——完全确定性，无竞态。

**What it will NOT do:** 不改动画时长、不改外观、不改 toast 组件、不引入时钟注入抽象、不改变任何界面行为——纯时间源替换 + 测试清理 + 文档同步。

**Effort:** Short
**Risk:** Low - 模式与 #168 toast 同构，探索已逐行验证时序；风险点已固化为 4 条实现不变量
**Decisions to sanity-check:** ① `open/close/toggle` 显式收 `now: f64`（grill Q1 锁定）；② main.rs 测试保持默认步进 0.25s 只删 workaround（grill Q2 锁定）；③ modal 测试改细粒度 0.01s 步进（grill Q3 锁定）

Your next move: 计划已批准（用户已确认「开始」）——按 Wave 1 进入实现。全量执行细节见下。

---

> TL;DR (machine): Short effort, low risk — modal 动画时间源墙钟→egui 虚拟时间 f64，移除 8 处测试 workaround，同步 3 处 kb 文档；5 todos + 4 final verifiers。

## Scope
### Must have
- `crates/compass-ui/src/widgets/modal.rs`：产品代码动画时间源从墙钟 `Instant` 改为 egui 虚拟时间 f64（`open(now: f64)` / `close(now: f64)` / `toggle(now: f64)` 显式收参；`open_started`/`close_started: Option<f64>`；`progress_since` 与三个 progress 方法收 `now: f64`；`show()` 内 `let now = ctx.input(|i| i.time)`）
- `crates/compass-ui/src/widgets/modal.rs` 测试：harness 改 `with_step_dt(0.01)`；4 处 wall-clock workaround（:402/:588/:622/:651）移除；边界测试（:485-521）Duration 算术改 f64；全部 `open()/close()/toggle()` 调用点加 `now` 参数
- `crates/compass/src/main.rs` 产品代码：:473 `self.modal.open(now)`（`ui.ctx().input(|i| i.time)`）；`request_watchlist_removal`（:804）新增 `now: f64` 参数，:764 调用点从 `render_sidebar` 传 `ui.ctx().input(|i| i.time)`，:820 `self.modal.open(now)`
- `crates/compass/src/main.rs` 测试：删除 4 处 workaround 行对（:1746-47 / :1960-61 / :2005-06 / :2021-22）
- kb 文档同步（Q4）：`kb/design/ui.md` 决策记录表新增 Modal 动画时间源行；`kb/dev/toolchain.md` 排查卡追加 modal 同根因第二实例；`kb/dev/testing.md` §295+§296 更新
- `.omo/plans/modal-virtual-time.md` 计划文件随 PR 提交

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不改 `crates/compass-ui/src/widgets/toast.rs`（#168 已修，仅作参考模式）
- 不改动画时长常量（BACKDROP 120ms / PANEL 150ms / CLOSE 100ms 保持 `std::time::Duration`，modal.rs:18/:20/:22）
- 不改 `sized_harness`（main.rs:1177-1181）的 step_dt —— 保持默认 0.25s（Q2 锁定：一 step 确定跨过全部动画时长）
- 不引入 Clock trait / 时间源注入抽象（Q1 明确排除，toast #168 同理由）
- 不改 egui / egui_kittest 版本（0.35）
- 不碰 main.rs:597 的无关 `Duration::from_millis(200)`（request_repaint_after 节流）
- 不把 main.rs 测试中的 `harness.step()` 换成 `harness.run()`（见不变量 1）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **compile-forced refactor + 行为锁定测试**（签名迁移原子性使传统 RED→GREEN 不可拆分，见 Execution strategy 说明）。新增「虚拟时间语义回归测试」作为真正的行为 RED（实现若仍用墙钟则失败）。框架：Rust `#[cfg(test)]` + egui_kittest。
- Evidence: `.omo/evidence/task-<N>-modal-virtual-time.<ext>`（命令输出重定向落盘）
- 核心验证命令：
  - `cargo nextest run -p compass-ui widgets::modal` ×20 无失败（issue 验收标准 3）
  - `cargo nextest run -p compass sidebar_delete_modal sidebar_toggle startup_modal` ×20（main.rs 涉及测试）
  - `cargo test --workspace` 全量通过（issue 验收标准 4）
  - `cargo clippy -- -D warnings`（CI 强制，未用 import 即失败）
  - `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`（Step 5a）
  - `grep -n "Instant" crates/compass-ui/src/widgets/modal.rs crates/compass/src/main.rs` → 零命中（产品 + 测试均无）
  - `scripts/check-coverage.sh`（compass-ui 80% 阈值，测试重写不得掉覆盖）

## Execution strategy
### 关于 RED→GREEN 的诚实声明（Metis Gap 1 修正）
签名迁移（`open()` → `open(now: f64)`）是编译原子的：中间态（测试已更新、产品未改）无法编译，传统"先写失败测试再实现"不可拆分。本项目 AGENTS.md 允许"纯重构：先用特征测试锁定当前行为"。因此：
- **RED 形态**：① 新增「虚拟时间语义回归测试」——注入不同 `now` 值断言 progress 跟随注入值；若实现错误地仍取墙钟 `Instant::now()`，该测试失败（真正的行为 RED）。② 迁移后的确定性测试（`with_step_dt` + `run_steps`）锁定每个状态机行为。
- **GREEN 形态**：产品代码从 `ctx.input(|i| i.time)` 取时间后，上述测试全绿。
- 执行顺序：Todo 1（产品签名+实现）与 Todo 2（测试迁移）在同一编译单元内原子完成——先改产品签名，用编译器错误枚举全部测试调用点，再批量更新测试，最后全绿。不允许出现"跳过测试直接实现"的中间提交。

### 不变量（Metis Gap 4/5/6/8 修正——实现者必须遵守）
1. **run() 陷阱**：egui_kittest `_try_run` 在 `repaint_delay != Duration::ZERO` 时立即返回（lib.rs:355-360），而 modal 的 `request_animation_repaint` 用 `request_repaint_after(≥16ms)` → `harness.run()` 永远不会完成动画。main.rs 测试删除 workaround 后依赖**显式 `harness.step()`** 推进——**绝不把 step() 换成 run()**。
2. **重开入场点击**：`confirm_button_consumes_callback_exactly_once` 重开（:655）后须 `run_steps(16)`（16×10ms=160ms > PANEL_DURATION 150ms）等入场动画结束再点 Confirm（:657）——入场 scale 动画运行中破坏 hit-testing（main.rs:1958-59 注释实证）。
3. **f64 精确端点**：边界测试保持端点字面量（`start + 0.12`、`start + 0.10`），依赖 f64 IEEE 精确性（`x/x == 1.0`、`0.0/x == 0.0`）保住 `assert_eq!(..., 0.0/1.0)` 精确断言；中点用现有容差。
4. **:402 中间断言保留**：`close_starts_closing_state_machine` harness 化后，须在 `close()` 与 `run_steps(11)` 之间保留原中间断言（`closing` 为 true、`close_started.is_some()`、`is_open` 仍 true，modal.rs:397-399）。

### Parallel execution waves
- **Wave 1**（原子编译单元）：Todo 1 + Todo 2（modal.rs 产品 + 测试，必须同批才能编译）
- **Wave 2**：Todo 3 + Todo 4（main.rs 产品调用点 + 测试 workaround 删除）
- **Wave 3**：Todo 5（kb 文档 3 处同步）
- 顺序执行（Wave 间有编译依赖），无并行。

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 (modal.rs 产品) | — | 2, 3 | 无 |
| 2 (modal.rs 测试) | 1 | 5 | 1（同批原子） |
| 3 (main.rs 产品) | 1 | 4 | 无 |
| 4 (main.rs 测试) | 3 | 5 | 无 |
| 5 (kb 文档) | 1-4 | — | 无 |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [ ] 1. modal.rs 产品代码：动画时间源改 egui 虚拟时间 f64
  What to do: ① `use std::time::{Duration, Instant}` → `use std::time::Duration`（:10）。② `progress_since`（:25-28）签名改 `(started: f64, now: f64, duration: Duration)`，去 `saturating_duration_since`，改为 `((now - started) / duration.as_secs_f64().max(0.001)).clamp(0.0, 1.0) as f32`（照抄 toast.rs:24-26）。③ 字段 :62/:64 `Option<Instant>` → `Option<f64>`，`new()` :100-101 保持 None。④ `open(&mut self)` → `open(&mut self, now: f64)`（:123-129，`self.open_started = Some(now)`）。⑤ `close(&mut self)` → `close(&mut self, now: f64)`（:133-138，`self.close_started = Some(now)`）。⑥ `toggle` → `toggle(&mut self, now: f64)`（:141-146，透传给 open/close）。⑦ `entry_progress/panel_progress/close_progress` 参数 `Instant` → `f64`（:183/:190/:197）。⑧ `show()` :222 `let now = Instant::now()` → `let now = ctx.input(|i| i.time);`；内部 close 调用 :358 → `self.close(now)`。⑨ 更新 pub 项 doc 注释：open/close/toggle/字段/progress_since 标注 "egui virtual seconds (`ctx.input(|i| i.time)`)"（照 toast.rs:73-83 惯例）。⑩ 本 todo 只改产品代码签名与实现——测试调用点由 Todo 2 同批处理（编译中间态允许，但不得提交）。
  Must NOT do: 不改常量值；不改 show() 的渲染/布局逻辑；不加 Clock trait；不 `as any` 式类型压制。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2, 3
  References: crates/compass-ui/src/widgets/modal.rs:10/:25-28/:62-64/:95-109/:123-147/:183-201/:215-360; crates/compass-ui/src/widgets/toast.rs:10/:24-26/:73-83/:109-132（参考模式）
  Acceptance criteria (agent-executable): `grep -n "Instant" crates/compass-ui/src/widgets/modal.rs` 产品代码段零命中（仅测试段可含——测试由 Todo 2 清理）；`cargo build -p compass-ui` 通过；pub 项 doc 含 "virtual" 或 "ctx.input"
  QA scenarios: happy: `cargo nextest run -p compass-ui widgets::modal` 全绿（Todo 2 后）；failure: 若仍用墙钟，新增虚拟时间回归测试（Todo 2 中）红。Evidence `.omo/evidence/task-1-modal-virtual-time.txt`
  Commit: N（与 Todo 2 同批）

- [ ] 2. modal.rs 测试：迁移全部调用点 + 移除 4 workaround + 新增虚拟时间语义回归测试
  What to do: ① `harness_for_modal`（:524-530）：`Harness::new_ui(...)` → `Harness::builder().with_step_dt(0.01).build_ui(...)`。② 全部 `open()/close()/toggle()` 测试调用点加 `now: f64` 实参（编译器强制枚举，覆盖 :387/:395-396/:411-412/:423/:427/:429/:545/:562/:601/:634/:655/:671/:689 等全部命中——以编译器报错为准，逐一修复）。③ 3 处 kittest workaround（:588/:622/:651）：删 `borrow_mut().close_started = Some(Instant::now() - Duration::from_millis(200))` 行，改为 `harness.run_steps(11)`（11×10ms=110ms > CLOSE_DURATION 100ms）。④ :402 纯状态测试 harness 化：`modal.open(now)` → `modal.close(now)` → **保留中间断言**（closing、close_started.is_some()、is_open——不变量 4）→ `run_steps(11)` → 断言关闭；删除 `show(&egui::Context::default())` 直调。⑤ 边界测试 :485-490/:499-503/:510-521：`start + Duration::from_millis(60/120)` → `start + 0.06/0.12` 等 f64 字面量，**端点保持字面量精确**（不变量 3）。⑥ 新增虚拟时间语义回归测试（行为 RED 锁定）：构造 modal，`open(5.0)` 断言 `entry_progress(5.0)==0.0` 且 `entry_progress(5.12)==1.0`；`close(10.0)` 断言 `close_progress(10.05)==0.5`、`close_progress(10.1)==1.0`——证明动画由注入 now 驱动、与墙钟无关。
  Must NOT do: 不把 `run_steps` 换成 `run()`（不变量 1）；不改断言语义（只改时间表达）；不删测试、不降覆盖。
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 5
  References: modal.rs:369-725（测试模块全）、:524-530（harness）、:402/:588/:622/:651（workaround）、:485-521（边界）、:393-406/:559-595/:598-628/:631-664（4 个测试体）；toast.rs:556-565/:646/:684（run_steps 先例）；toast.rs:461/:476/:615（负虚拟戳先例）
  Acceptance criteria (agent-executable): `cargo nextest run -p compass-ui widgets::modal` ×20 连续无失败（落盘 `.omo/evidence/task-2-modal-virtual-time-20x.txt`）；`grep -n "Instant::now" crates/compass-ui/src/widgets/modal.rs` 测试段零命中；新回归测试存在且通过
  QA scenarios: happy: 上述命令全绿；failure: 故意将 show() 的 now 源改回 `Instant::now()` → 新回归测试必红（证明其锁行为）。Evidence `.omo/evidence/task-2-modal-virtual-time.txt`
  Commit: N（与 Todo 1 同批）

- [ ] 3. main.rs 产品代码：调用点传入虚拟时间，request_watchlist_removal 加 now 参数
  What to do: ① :473 `self.modal.open()` → `self.modal.open(ui.ctx().input(|i| i.time))`（ui() 内 ui.ctx() 可用，:456/:522 先例）。② `request_watchlist_removal(&mut self, symbol: &str)`（:804）→ `request_watchlist_removal(&mut self, now: f64, symbol: &str)`；:820 `self.modal.open(now)`。③ :764 `SidebarEvent::DeleteRequest { symbol } => self.request_watchlist_removal(&symbol)` → `self.request_watchlist_removal(ui.ctx().input(|i| i.time), &symbol)`（render_sidebar :721 有 ui）。④ 确认 main.rs 无其他 modal.open/close/toggle 产品调用（explore 已证仅 :473/:820/:522）。
  Must NOT do: 不改 show() 调用（:522 无需变）；不改 `is_open()` 调用（:544/:805）；不引入 ctx 到 request_watchlist_removal 之外的方法。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: crates/compass/src/main.rs:455-475（ui() 内 open 调用点）、:721-766（render_sidebar 事件分发）、:804-821（request_watchlist_removal）
  Acceptance criteria (agent-executable): `cargo build -p compass` 通过；`grep -n "modal.open\|modal.close\|modal.toggle" crates/compass/src/main.rs` 产品代码段全部带 now/ctx 实参
  QA scenarios: happy: `cargo nextest run -p compass sidebar_delete_modal sidebar_toggle startup_modal` 绿（Todo 4 后）；failure: 编译错误若漏传参。Evidence `.omo/evidence/task-3-modal-virtual-time.txt`
  Commit: N（与 Todo 4 同批）

- [ ] 4. main.rs 测试：删除 4 处 wall-clock workaround 行对
  What to do: 删除以下行对（不替换为任何等价物，直接删）：
  - :1746-47 `harness.state_mut().modal.close_started = Some(std::time::Instant::now() - std::time::Duration::from_millis(200));`（sidebar_toggle_hides_and_reshows_sidebar :1737）
  - :1960-61 `...open_started = Some(...)`（sidebar_delete_opens_danger_modal_and_removes_on_confirm :1924）
  - :2005-06 `...open_started = Some(...)`（sidebar_delete_modal_cancel_keeps_watchlist :1992）
  - :2021-22 `...close_started = Some(...)`（同一测试）
  删除后行为验证（依赖默认 step_dt 0.25s 一 step 跨过 100ms 关闭 / 120-150ms 入场，Q2 已实证）：每处 workaround 前后的 `harness.step()` 保留即可让动画完成。确认删除后 main.rs 无 `std::time::Instant` 残留（:597 的 Duration::from_millis(200) 为无关 request_repaint_after 节流，保留）。
  Must NOT do: 不把 step() 换成 run()（不变量 1）；不调整 sized_harness 的 step_dt；不新增其他 workaround。
  Parallelization: Wave 2 | Blocked by: 3 | Blocks: 5
  References: main.rs:1737-1768/:1924-1989/:1992-2029（3 个测试体）、:1742-49/:1958-64/:2002-24（workaround 上下文）、:1177-1181（sized_harness 默认 step_dt）
  Acceptance criteria (agent-executable): `grep -n "Instant" crates/compass/src/main.rs` 零命中；`cargo nextest run -p compass sidebar_delete_modal sidebar_toggle startup_modal` ×20 无失败（落盘 `.omo/evidence/task-4-modal-virtual-time-20x.txt`）
  QA scenarios: happy: 上述命令全绿；failure: 若删除后某测试红，诊断是动画未完成还是断言漂移——按不变量 1-4 修（不得回退 workaround）。Evidence `.omo/evidence/task-4-modal-virtual-time.txt`
  Commit: N（与 Todo 3 同批）

- [ ] 5. kb 文档同步（Q4）
  What to do: ① `kb/design/ui.md` 决策记录表（:207-228）在 Toast 行（:220）后新增 Modal 行——同构：| Modal 动画时间源 | 真实墙钟 `Instant::now()` / egui 虚拟时间 `ctx.input(\|i\| i.time)` / 注入 Clock trait | egui 虚拟时间（f64 秒字段 `open_started`/`close_started`，open/close 显式收 `now: f64`） | 与 Toast 同构（ref #168）——kittest 下虚拟时间每帧按 predicted_dt 推进、完全确定，根治慢 CI wall-clock 漂移（ref #171） | 墙钟驱动动画使 kittest 测试依赖机器负载、慢 CI 间歇失败（#155 修后仍发 #168，modal 同根因）；Clock trait 过度设计 |。② `kb/dev/toolchain.md` 排查卡（:154-182）追加：modal 动画同根因第二实例已根治（ref #171）——症状行补 modal 测试名、修复段补"modal 已随 #171 改虚拟时间"。③ `kb/dev/testing.md` :295 "toast 动画即此模式，ref #168" → "toast/modal 动画即此模式，ref #168/#171"；:296 "避免...workaround" 规则文末追加"modal 的 4+4 处实例已随 #171 移除"。
  Must NOT do: 不改 kb 其他章节；不新增 kb 文件；不引用已关闭 issue（ref 前缀仅用于 OPEN #171）。
  Parallelization: Wave 3 | Blocked by: 1-4 | Blocks: —
  References: kb/design/ui.md:207-228（决策记录表）、:220（Toast 行模板）；kb/dev/toolchain.md:154-182（排查卡）；kb/dev/testing.md:285-296（时间敏感陷阱节）
  Acceptance criteria (agent-executable): `grep -n "Modal 动画时间源" kb/design/ui.md` 命中新行；`grep -n "modal" kb/dev/toolchain.md` 排查卡含第二实例；`grep -n "toast/modal" kb/dev/testing.md` 命中
  QA scenarios: happy: 三处 grep 命中；failure: 若 ui.md 决策表无 Modal 行则 Step 5c 不通过。Evidence `.omo/evidence/task-5-modal-virtual-time.txt`
  Commit: N（与实现代码同批，见 Commit strategy）

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit：逐 todo 核对验收标准——`grep -n "Instant" crates/compass-ui/src/widgets/modal.rs crates/compass/src/main.rs` 零命中（含测试段）；无 Clock trait；常量未变；sized_harness step_dt 未改；无 workaround 残留（grep `Instant::now() - Duration` 零命中）
- [ ] F2. Code quality review：运行 `/review-work`（5 并行 agent：goal/quality/security/QA/context），范围内问题 ≤2 轮修复
- [ ] F3. Real manual QA：issue #171 验收标准全量执行——`cargo nextest run -p compass-ui widgets::modal` ×20、`cargo nextest run -p compass sidebar_delete_modal sidebar_toggle startup_modal` ×20、`cargo test --workspace`、`cargo clippy -- -D warnings`、`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`、`scripts/check-coverage.sh`——全部通过，输出落盘 `.omo/evidence/`
- [ ] F4. Scope fidelity：对照 Scope OUT 逐条核查（toast.rs/常量/sized_harness/Clock trait/版本/step→run 无违反）；确认 issue 行号引用为实际行号（handoff 的 1746-47/1960-61/2005-06/2021-22）

## Commit strategy
- commit 1（实现，原子）：`refactor(ui): modal 动画时间源从墙钟改为 egui 虚拟时间\n\nref #171` —— 含 modal.rs（产品+测试）+ main.rs（产品+测试）变更 + `.omo/plans/modal-virtual-time.md` 计划文件
- commit 2（文档，随实现同批）：`docs(kb): 同步 modal 虚拟时间决策记录与排查卡\n\nref #171` —— kb/design/ui.md + kb/dev/toolchain.md + kb/dev/testing.md
- 每个 commit 必须带 `ref #171`（指向 OPEN issue，commit-msg hook 校验）；不 push（等用户明确指令）；commit 后运行 `/review-work`（compass-workflow 强制）

## Success criteria
- issue #171 验收标准 4 条全绿：
  - [ ] modal 产品代码无 `Instant::now()`（动画时间源为 egui 虚拟时间）
  - [ ] modal/main.rs 测试无"重置时间戳"workaround（改用细粒度 step 推进）
  - [ ] `cargo nextest run -p compass-ui widgets::modal` 连续 20 次无失败
  - [ ] workspace 全量测试通过
- 文档同步完成（ui.md 决策记录 / toolchain.md 排查卡 / testing.md 模式描述）
- commit 含 `ref #171`，review 无阻塞问题，PR 就绪
