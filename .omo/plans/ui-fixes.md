# ui-fixes - Work Plan

## TL;DR (For humans)

**What you'll get:** 修复 A 股图表应用的四个 GUI 问题——切换 K 线周期后立即刷新数据、图表日期全部显示中文（x 轴紧凑 + 提示完整）、选股器条件标签与控件永远同行、SEPA 前 50 排名表格在真实窗口中正确显示。

**Why this approach:** 前三个问题根因已定位且方案经设计确认（K线切换缺状态同步与重载触发、日期格式硬编码在 egui-charts fork、选股器换行切点落在标签与控件之间）；第四个问题的引擎与测试环境已验证正常，剩余差异集中在真实窗口布局（egui_dock + 表格高度），因此先复现拿证据再修，不盲改。

**What it will NOT do:** 不做 GUI 全面中文化（已另立 #222）；不改图表刻度密度；不引入新依赖；不重构 SEPA 引擎/异步链路；不改选股器条件逻辑。

**Effort:** Medium
**Risk:** Medium - #221 真实窗口根因需复现确认，若 dock 布局改动波及 Chart tab 需谨慎
**Decisions to sanity-check:** #219 直接在 fork DefaultTimeFormatter 硬编码中文（不做 locale 配置）；#219 格式串锁定 `%-m`/`%-d` 去填充（避免 "06月"）；tooltip 前缀 7 个全中文化（含 Change:）+ tracking 缩写；#218 切换无条件触发 fetch（loading 不拦截）；#221 修复方向以复现证据为准，候选含 DataTable 高度显式化 / horizontal 布局改造

---

> TL;DR (machine): Medium effort; 4 independent GUI fixes (timeframe reload, Chinese dates in chart fork, screener layout atoms, SEPA table in dock); #221 reproduce-first.

## Scope

### 用户确认决策记录（覆盖 .omo/designs/ 中"待确认"项）
grill-me + 用户 4 项决策确认（2026-08-09），**优先级高于** `.omo/designs/` 归档设计文档中标注"待确认"的条目：
1. 两份设计方案整体接受（`ui-fixes-chinese-date.md` + `ui-fixes-screener-layout.md`）
2. **#219 tooltip 时间分档**：按 bar 周期分档（日线+纯日期 `2024年5月15日`，盘中带时间）——覆盖设计中"统一格式"备选
3. **#219 tooltip 英文前缀一并中文化**（非"保持现状"）：`Time:`→`时间:`、`Open:`→`开盘:`、`High:`→`最高:`、`Low:`→`最低:`、`Close:`→`收盘:`、`Volume:`→`成交量:`、**`Change:`→`涨跌:`**（共 7 个前缀，含 Metis 审查补充的 `Change:`——默认 `show_change: true`，config/tooltip.rs L114，实际界面必然显示）；tracking tooltip 缩写 `O:/H:/L:/C:/Vol:`（L186-193）compass 默认不用 tracking，**一并中文化**为 `开:/高:/低:/收:/量:` 保持一致性
4. **#220 行距提至 sm(8px)**：外层 horizontal_wrapped `item_spacing.y = tokens.spacing.sm`——覆盖设计中"建议 spacing.sm（待确认）"

### 关键格式锁定（Metis B1 审查修正）
chrono 0.4.45 的 `%m`/`%d` 是**零填充**（输出 "06月"/"06月15日"），与中文习惯（"6月"）不符。**所有月份/日期格式串锁定用 `%-m`/`%-d`（去填充修饰符）**：
- x 轴 Month → `%-m月`（"6月"）；DayOfMonth → `%-m月%-d日`（"6月15日"）
- crosshair 时级 → `%-m月%-d日 %H:%M`（"5月15日 14:30"）；日级+ → `%Y年%-m月%-d日`（"2024年5月15日"）
- tooltip 完整式 → `%Y年%-m月%-d日`；盘中 → `%-m月%-d日 %H:%M:%S`
- 同步修正 `.omo/designs/ui-fixes-chinese-date.md` 中格式串（当前写的是零填充版）

### Must have
- **#218**：`set_timeframe` 内更新 `shared_state.timeframe` + 触发 `fetch_symbol`（立即重载）；**切换必须触发 fetch（不因 loading 跳过）**——loading 中的数据属旧周期，跳过则图表与标签不一致（正是 #218 原 bug）；`timeframe_index` 初始化与 `default_timeframe` 对齐（提取独立 helper `timeframe_index_from_value`，与 `timeframe_label` 双向同步）；现有两个测试更新为"切换触发 fetch"断言 + 新增 shared_state/fetch 断言
- **#219**：fork（/data/codes/compass-project/egui-charts，branch compass）三处格式化点改中文——time_formatter.rs（Month `6月`、DayOfMonth `6月15日`）、crosshair.rs（时级 `5月15日 14:30`、日级+ `2024年5月15日`）、tooltip.rs（分档 + 7 前缀中文化 + tracking 缩写中文化）；**fork 新增 crosshair/tooltip 格式断言测试**（当前两文件无 mod tests，且验收含 "2024年5月15日" 却无测试覆盖）；fork 测试更新（time_formatter.rs L366-378、timescale_marks.rs L643-669）；push fork → **确认 fork CI（ci.yml）通过** → compass `cargo update -p egui-charts` 拉新 commit
- **#220**：screener.rs `basic_conditions`（L339-391）+ `technical_conditions`（L394-452）每条件组包进独立 `ui::horizontal` 原子单元（组间才换行）；外层 `horizontal_wrapped` 行距 `item_spacing.y = tokens.spacing.sm`（8px）；kittest 宽度参数化 y 对齐断言（500px 为超出设计下限 >600px 的应力测试，注明意图）
- **#221**：先 dock 环境复现（`build_compass_app_with_stocks` + `sized_harness` 1440×900 模式，**非** double_tab_leaf 的手工 DockState 模式——后者仅作 dock 交互参考）+ 注入 sepa_data 50 行 + 断言表格体）确认根因 → 修复 → 补表格体行内容断言防回归
- **文档同步**：`kb/design/ui.md`（#219 日期格式规范 + #220 布局决策记录，追加 `## 决策记录` 行）+ `kb/user/gui.md`（日期格式/选股器行为/SEPA 表格）；`.omo/designs/` 两份归档同步修正格式串与"待确认"状态

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不做 #222（GUI 全面中文化/i18n）——仅 tooltip 前缀中文化属 #219 范围；**tooltip 以外 label（图例/菜单等）归 #222**
- 不引入新依赖 crate（fork 改动仅 chrono 格式化串 + 前缀字符串）
- 不改 fork 时间刻度密度/间距逻辑（min_spacing/target_density 保持）
- 不重构 SEPA 引擎/异步链路（已排除）
- #221 不盲改布局——必须先复现拿到证据（AGENTS.md 禁止凭视觉猜）
- **#218 不做 loading 时延迟 fetch**（复杂语义，不推荐）——切换即触发，行为确定
- 生产代码禁止 unwrap()/as 类型转换

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD（门禁 3.5/4 步 RED 测试先行 → 实现 GREEN）; fork 用 crate 内 `cargo test --lib`（**实现前记录实际基线，不以 "518" 硬编码**——fork 源码现有 703 个 #[test]，2b18acd 基线以实测为准）; compass 用 `cargo test`（含 kittest 无头集成测试）
- Evidence: `.omo/evidence/ui-fixes/task-<N>-<sub>.log`（实现前后测试输出、fork/compass 构建产物）

## Execution strategy

### Parallel execution waves
- **Wave 0（RED 准备）**：门禁 3.5 步 skwy-adversarial-test（#218-#221 对抗性测试）+ 门禁 4 步 skwy-requirement-test（#218-#221 需求验收测试）——两个测试 agent 并行委派
- **Wave 1（实现）**：#218、#220、#221 三个 compass 内改动可并行实现（不同文件）；#219 依赖 fork 修改独立进行
- **Wave 2（验证）**：全量 `cargo test` + `cargo clippy -D warnings` + `cargo fmt --check`
- **Wave 3（收尾）**：文档同步 → review-work → rebase + reflect + push → issue 收尾

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. 对抗性测试 RED | plan 批准 | 3, 4, 5, 6, 7 | 2 |
| 2. 需求测试 RED | plan 批准 | 3, 4, 5, 6, 7 | 1 |
| 3. #218 实现 | 1, 2 | 8 | 4, 5, 6, 7 |
| 4. #219 fork 修改 | 1, 2 | 5, 8 | 3, 6, 7 |
| 5. #219 fork push + cargo update | 4 | 8 | 3, 6, 7 |
| 6. #220 实现 | 1, 2 | 8 | 3, 4, 5, 7 |
| 7. #221 复现 + 修复 | 1, 2 | 8 | 3, 4, 5, 6 |
| 8. 全量验证 + 文档同步 | 3, 5, 6, 7 | 9 | — |
| 9. review + push + 收尾 | 8 | — | — |

## Todos

- [ ] 1. 委派 skwy-adversarial-test：写 #218-#221 对抗性测试（RED，plan 批准后立即）
  What to do / Must NOT do: 委派 `skwy-adversarial-test` agent（load_skills=["skwy-adversarial-test"]）针对四个子 issue 的接口契约写刁钻但真实有效的对抗性测试——边界/错误路径/非法输入/并发。测试写在各 crate 对应 `#[cfg(test)]` 模块。禁止修改生产代码；禁止 cargo run/git 写操作。若某个子 issue 无接口契约返回 DEFERRED 并记录。
  Parallelization: Wave 0 | Blocked by: — | Blocks: 3, 4, 5, 6, 7
  References: crates/compass/src/main.rs（set_timeframe L746-751, fetch_symbol L770-780, tests L1451/L2423）、crates/compass/src/citizens/screener.rs（L339-452, tests L483-741）、crates/compass/src/citizens/sepa.rs（L135-152, tests L731-857）、/data/codes/compass-project/egui-charts（time_formatter.rs L55-83, crosshair.rs L200-225, tooltip.rs L50-60）
  Acceptance criteria: 每个子 issue 有对抗性测试文件落盘（RED 失败输出）或 DEFERRED 记录（含原因）
  QA scenarios: happy — `cargo test` 显示新对抗性测试失败（因实现未改）；failure — 测试文件编译失败需修复。Evidence `.omo/evidence/ui-fixes/task-1-adversarial.log`
  Commit: N（RED 测试由主 agent 审查后与实现一并提交）

- [ ] 2. 委派 skwy-requirement-test：写 #218-#221 需求验收失败测试（RED）
  What to do / Must NOT do: 委派 `skwy-requirement-test` agent（load_skills=["skwy-requirement-test"]）按各 issue 验收标准写需求验收测试（happy path + 基本错误路径）。#218 断言 shared_state.timeframe 更新 + FetchBars 请求发出（复用 ctrl_enter_triggers_fetch 模式 L2408-2421）；#219 fork 测试断言中文输出（格式串用 `%-m`/`%-d` 去填充版，断言 "6月"/"6月15日" 无前导零）；#220 宽度参数化 y 对齐断言（query_all_by_label_contains("全部") 索引 + center().y 容差 1.0px）；#221 表格体断言。禁止修改生产代码。
  Parallelization: Wave 0 | Blocked by: — | Blocks: 3, 4, 5, 6, 7
  References: 同 todo 1 + egui_kittest 查询 API（get_by_label_contains 多匹配 panic，须 query_all_by_label_contains）、kb/dev/testing.md
  Acceptance criteria: 每个子 issue 需求测试落盘且 RED（当前代码下失败）
  QA scenarios: happy — `cargo test` 显示新测试失败且因正确原因（断言未满足而非编译错）；failure — 编译失败修复。Evidence `.omo/evidence/ui-fixes/task-2-requirement.log`
  Commit: N

- [ ] 3. #218 实现：set_timeframe 同步 + 立即重载 + index 初始化对齐
  What to do / Must NOT do: 修改 `crates/compass/src/main.rs`——① `set_timeframe`（L746-751）内：先更新 `self.timeframe_index = idx`，再 `self.shared_state.timeframe.set(timeframe_value(idx))`，然后**无条件调用 `self.fetch_bars()`**（loading 守卫不拦截切换——loading 中的数据属旧周期，跳过则图表与标签不一致；dispatcher 每次 fetch 同步置 loading=true，L80，最后一次请求生效）。② `timeframe_index` 初始化（L162 生产 + L1262 测试辅助）改为从 `default_timeframe` 派生——提取独立 helper `fn timeframe_index_from_value(&str) -> usize`（1d→0/1w→1/1M→2，未知值回退 0），生产初始化与测试复用，与 `timeframe_label`（L1090-1097）的 match 双向同步（注释标注）。让 todo 1/2 的 RED 测试转 GREEN。禁止改 dispatcher/backend 数据层。禁止 unwrap()。
  Parallelization: Wave 1 | Blocked by: 1, 2 | Blocks: 8
  References: crates/compass/src/main.rs L162（index 初始化）、L526（字段）、L710-751（快捷键+set_timeframe）、L770-790（fetch_symbol/fetch_bars）、L1010-1032（Segmented+Fetch 按钮）、L1090-1097（timeframe_label）、L1451-1465/L2423-2440（现有测试）；crates/compass/src/state.rs L15/L51（shared_state.timeframe）；compass-core/src/model.rs L347-349（default_timeframe 默认）
  Acceptance criteria: `cargo test` 全绿（含新 RED 测试转 GREEN）；`cargo clippy -D warnings` 无警告；切 1d/1w/1M 后 loading 置位 + shared_state.timeframe 更新
  QA scenarios: happy — **loading/SharedState 断言用 `Harness::new_ui` 模式**（render_toolbar 路径，复用 render_toolbar_fetch_sets_loading L1491-1508 已验证稳定）：Segmented 点击后 shared_state.timeframe=="1w" + loading true；**sized_harness 的 digit_keys 测试断言同步字段**（timeframe_index / shared_state.timeframe，确定性强，不断言 loading——避免 wire_backend 异步线程一帧内完成 fetch 导致的 flaky）；failure — 双击同一周期不重复触发（`idx != self.timeframe_index` 守卫），启动 default_timeframe="1w" 时 index==1。Evidence `.omo/evidence/ui-fixes/task-3-timeframe.log`
  Commit: Y | fix(timeframe): switch K-line unit reloads immediately and sync shared state

- [ ] 4. #219 fork 修改：中文日期格式化 + tooltip 分档 + 前缀中文化
  What to do / Must NOT do: 修改 `/data/codes/compass-project/egui-charts`（branch compass）——① `src/scales/time_formatter.rs` L59 Month→`%-m月`、L61 DayOfMonth→`%-m月%-d日`（Year `%Y` 不变，Time/TimeWithSeconds 不变；`%-` 去填充避免零填充 "06月"）。② `src/chart/renderers/crosshair.rs` L218→`%-m月%-d日 %H:%M`、L221→`%Y年%-m月%-d日`。③ `src/chart/renderers/tooltip.rs` 分档（floating L57 与 tracking L182 按 bar 周期：日线+纯日期 `%Y年%-m月%-d日`，盘中 `%-m月%-d日 %H:%M:%S`——从 visible_data 推导 bar duration 传入，轻微签名变更，复用 crosshair L200-222 的中位数推导法）+ 前缀中文化：`Time:`→`时间:`、`Open:`→`开盘:`、`High:`→`最高:`、`Low:`→`最低:`、`Close:`→`收盘:`、`Volume:`→`成交量:`、**`Change:`→`涨跌:`（L75，默认 show_change=true 必然显示）**、tracking 缩写 `O:/H:/L:/C:/Vol:`→`开:/高:/低:/收:/量:`（L186-193）。④ **新增 fork 测试**：crosshair 日级格式（"2024年5月15日"）与 tooltip 前缀中文化输出各加断言（两文件现无 mod tests；格式化逻辑若不便测则提取纯函数）——验收标准"2024年5月15日"必须有测试覆盖，禁止凭视觉验证。⑤ 更新现有测试 time_formatter.rs L366-378（"Jun"→"6月"）与 timescale_marks.rs L643-669（"Jun"/"Jun 15"→中文）。fork 内 `cargo test --lib` 全绿 + clippy。禁止改刻度密度/间距逻辑；禁止动 LocaleTimeFormatter 结构。
  Parallelization: Wave 1 | Blocked by: 1, 2 | Blocks: 5, 8
  References: /data/codes/compass-project/egui-charts/src/scales/time_formatter.rs L55-83/L366-378；src/chart/renderers/crosshair.rs L200-225（无 mod tests）；src/chart/renderers/tooltip.rs L50-60/L75/L175-193（无 mod tests；Change: 前缀 L75；tracking 缩写 L186-193）；src/scales/timescale_marks.rs L643-669；config/tooltip.rs L114（show_change 默认 true）；.omo/designs/ui-fixes-chinese-date.md（已确认设计，格式串同步修正为 `%-` 去填充版）
  Acceptance criteria: fork `cargo test --lib` 全绿（以实测基线为准，预计 ~700 测试）；`cargo clippy -D warnings` 无警告；中文断言（"6月"/"6月15日"/"2024年5月15日"）通过；新增 crosshair/tooltip 测试覆盖验收标准中的格式串
  QA scenarios: happy — test_default_formatter 断言 Month=="6月"（无前导零）、DayOfMonth=="6月15日"；新增 crosshair 格式断言 "2024年5月15日"；新增 tooltip 前缀断言 "涨跌:"（Change: 中文化）。failure — 零填充回归（"06月" 断言失败）。Evidence `.omo/evidence/ui-fixes/task-4-fork.log`
  Commit: Y（fork 仓库内独立 commit）| feat(l10n): render chart dates in Chinese compact/full formats

- [ ] 5. #219 依赖更新：push fork + 确认 fork CI + compass cargo update
  What to do / Must NOT do: fork 内 `git push origin compass`；**确认 fork CI（.github/workflows/ci.yml）通过或记录失败原因**（fork 有完整 CI 配置，push 会触发——格式改动若破坏未更新的测试会留红分支）；compass worktree 执行 `cargo update -p egui-charts` 拉新 commit；确认 Cargo.lock L1730 的 commit hash 更新为 fork 新 HEAD；`cargo build` + `cargo test`（compass kittest 全绿）。禁止 force-push。
  Parallelization: Wave 1 | Blocked by: 4 | Blocks: 8
  References: Cargo.toml L20（git+branch=compass）、Cargo.lock L1728-1730（锁定 commit）、fork git status（branch compass @ 2b18acd）、fork .github/workflows/ci.yml
  Acceptance criteria: Cargo.lock 指向新 fork commit；fork CI 通过（或记录失败原因）；compass `cargo test` 全绿
  QA scenarios: happy — `git log --oneline -1` 在 fork 显示新 commit，Cargo.lock hash 匹配，fork CI 绿；failure — cargo update 失败需查 toolchain 排查卡；fork CI 红需回 fork 修复重推。Evidence `.omo/evidence/ui-fixes/task-5-update.log`
  Commit: N（Cargo.lock 变更随 #219 的 compass 侧 commit 提交——todo 9 的 commit 流程中，compass 侧 `fix(chart): update egui-charts fork for Chinese dates` + `ref #219` 含 Cargo.lock）

- [ ] 6. #220 实现：选股器条件原子组 + 行距 sm
  What to do / Must NOT do: 修改 `crates/compass/src/citizens/screener.rs`——`basic_conditions`（L342 的 horizontal_wrapped 内）每「标签+控件」组包进独立 `ui::horizontal`（组内不换行），组间保留 `ui.add_space(tokens.spacing.md)`；外层 horizontal_wrapped 设置 `ui.spacing_mut().item_spacing.y = tokens.spacing.sm`（8px）；`technical_conditions`（L397）同模式（Checkbox+条件参数段整体原子，含 210px Dropdown / DragValue 组）。让 RED 测试转 GREEN。禁止改条件逻辑/控件参数/组间 md 间距。
  Parallelization: Wave 1 | Blocked by: 1, 2 | Blocks: 8
  References: crates/compass/src/citizens/screener.rs L315-452（condition_form/basic_conditions/technical_conditions）、L483-741（tests）；crates/compass-ui/src/widgets/（section_title.rs/multi_select.rs/dropdown.rs/checkbox.rs）；.omo/designs/ui-fixes-screener-layout.md（已确认设计）
  Acceptance criteria: `cargo test` 全绿（新 y 对齐断言通过）；宽度参数化测试在 500-1200px 各宽度下组内 center().y 相等（**500px 是超出设计支持下限 >600px 的应力测试，意在证明原子组在任何宽度下组内不拆行；产品实际支持宽度为 >600px**）
  QA scenarios: happy — 窄窗口（如 700px）下"行业"标签与其 MultiSelect trigger 同 y；failure — 组间仍可换行（宽窗口单行、窄窗口多行）。Evidence `.omo/evidence/ui-fixes/task-6-screener.log`
  Commit: Y | fix(screener): wrap each condition label+control in atomic horizontal group

- [ ] 7. #221 复现根因 + 修复 + 表格体断言
  What to do / Must NOT do: ① 复现——新增 dock 测试（main.rs 测试模块）：**采用 `build_compass_app_with_stocks` + `sized_harness`（1440×900）模式**（与 ctrl_enter_triggers_fetch 一致；`app.shared_state` 是 pub 字段可直接 `sepa_data.set(Some(...))` 注入 50 行样例后传给 sized_harness）→ 断言表格体 label 存在。**double_tab_leaf 测试（L1663-1785）是手工构建 DockState 的另一套模式，仅作 dock 交互断言参考，不复制其装配方式**。若复现失败（表格体渲染正常），聚焦 `Widget rect changed` 警告坐标（1705-1888 x 980-1012 超出 1440×900 窗口）继续排查 dock 布局（egui_dock tab body 外层 ScrollArea [true,true] 双向滚动条 + expand_to_include_rect 与 DataTable 无 max_scroll_height 的交互）。② 修复——按证据定稿：候选 (a) DataTable 显式 `.max_scroll_height()`/`.auto_shrink()`（data_table.rs L175 TableBuilder 链）；(b) results_area 的 `ui.horizontal`（sepa.rs L327-341）表格宽度显式分配（available_width - 280 - spacing）；(c) 其他证据指向的方向。③ 防回归——表格体行内容断言（如第一行代码 label）。禁止在根因未确认前改布局；禁止改引擎/异步。
  Parallelization: Wave 1 | Blocked by: 1, 2 | Blocks: 8
  References: crates/compass/src/main.rs L117-135（dock 树）、L600-621（DockArea）、L1200-1293（测试辅助 build_compass_app_with_stocks/sized_harness，L1663-1785 double_tab_leaf 仅参考）、L158（pub shared_state）；crates/compass/src/citizens/sepa.rs L135-152/L307-341（show/results_area）、L731-857（tests）；crates/compass-ui/src/widgets/data_table.rs L145-232（TableBuilder，无 max_scroll_height）；egui_dock-0.20.1 `src/widgets/dock_area/show/leaf.rs` L1163-1258（tab body ScrollArea L1247 [true,true] + expand_to_include_rect L1255）、tab_viewer.rs L106（scroll_bars 默认 [true,true]）
  Acceptance criteria: 复现测试（dock + 数据）能捕获问题或证明正常；修复后 SEPA 表格在 dock 环境渲染 50 行；表格体断言测试通过；`cargo test` 全绿
  QA scenarios: happy — 新 dock 测试断言表格体第一行存在；failure — 修复前测试失败（RED），修复后 GREEN。Evidence `.omo/evidence/ui-fixes/task-7-sepa.log`
  Commit: Y | fix(sepa): render ranking table rows in docked panel

- [ ] 8. 全量验证 + 文档同步
  What to do / Must NOT do: **前置检查**——确认仓库现有 #209/#214 "CI Failure: master" 与本 epic 无关（记录证据，避免收尾误判）。`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test`（compass workspace 全量）+ `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`（新增 pub 项时）；fork `cargo test --lib` + clippy。同步 `kb/design/ui.md`（追加 #219 日期格式规范决策记录 + #220 布局决策记录，含 `## 决策记录` 表格行）与 `kb/user/gui.md`（日期格式/选股器布局/SEPA 表格行为）；**同步修正 `.omo/designs/ui-fixes-chinese-date.md` 与 `ui-fixes-screener-layout.md` 的格式串（`%-` 去填充）与"待确认"状态（标注已被用户决策覆盖）**。文档与代码同批 commit。
  Parallelization: Wave 2 | Blocked by: 3, 5, 6, 7 | Blocks: 9
  References: kb/design/ui.md（L207 已有决策记录章节，追加行）、kb/user/gui.md、AGENTS.md 变更类型→kb 映射表；#209/#214（CI Failure 前置检查）
  Acceptance criteria: 全部验证命令通过；kb 文件更新且含决策记录；#209/#214 与本 epic 无关的证据落盘
  QA scenarios: happy — 各命令 exit 0；failure — 覆盖率门槛（compass 80%）不因新代码跌破。Evidence `.omo/evidence/ui-fixes/task-8-verify.log`
  Commit: Y | docs(ui): sync Chinese date format and screener layout decisions

- [ ] 9. 实现后审查 + push 流程
  What to do / Must NOT do: 按子 issue 运行 `/review-work`（每个子 issue commit 后一次，PR 前完整 diff 一次，两层审查）；修复 review 发现的范围内问题（≤3 文件直接修，无关建 issue）；全部通过后：`git fetch origin master` + 落后则 rebase → 用户确认 push → `/skwy-reflect` 写反思 commit（ref #217）→ 同批 push → 追加完成 comment + 关闭子 issues + 关闭 epic（push 成功后）。**#219 compass 侧 commit 在此流程中执行：`fix(chart): update egui-charts fork for Chinese dates` + `ref #219`（含 Cargo.lock）**。禁止自动 push（等用户明确指令）。
  Parallelization: Wave 3 | Blocked by: 8 | Blocks: —
  References: kb/dev/process.md（push gate）、skwy-workflow skill（实现后审查章节）、skwy-reflect skill
  Acceptance criteria: review 无阻塞问题；push 成功；epic 收尾完成（comment + close）
  QA scenarios: happy — `git log origin/feat/ui-fixes-217` 含全部子 issue commit + 反思 commit；failure — review 阻塞问题需修复重审。Evidence `.omo/evidence/ui-fixes/task-9-review.log`
  Commit: Y | docs: reflection for GUI fixes epic (ref #217)

## Final verification wave

- [ ] F1. Plan compliance audit — 逐条核对 plan Todos 完成状态；四个子 issue 验收标准逐项勾选；RED 测试已转 GREEN 且有失败→成功证据
- [ ] F2. Code quality review — review-work 结果无阻塞；clippy/fmt/doc 全绿；无 unwrap()/as 类型逃逸
- [ ] F3. 行为验证（agent 可执行，零人工）— 四个修复行为全部由 kittest/无头断言验证：① 切换周期 loading+shared_state 断言；② fork 格式串单测（"6月"/"2024年5月15日"）；③ 选股器 y 对齐断言；④ SEPA dock 表格体断言。**不依赖用户手动运行 GUI**——若真实 GUI 冒烟不可行（无 X11），以 kittest 等价验证为准并记录
- [ ] F4. Scope fidelity — 未做 #222 内容（除 tooltip 前缀）；未改刻度密度/引擎；kb 文档与实现一致

## Commit strategy

- 每个子 issue 独立 commit，`ref #<sub-N>` 独立成行（#218→`ref #218`，以此类推）
- #219 拆两个 commit：fork 仓库内 `feat(l10n): ...`（fork 无 issue 引用规范，用 fork 自身风格）；compass 侧 `fix(chart): update egui-charts fork for Chinese dates` + `ref #219`（含 Cargo.lock，todo 9 流程执行）
- 文档同步 commit `ref #217`（epic）
- 反思 commit `ref #217`（push 前编写，同批推送）
- 禁止 `fixes #N`/`closes #N`；禁止自动 push

## Success criteria

- [ ] #218: 切换 1d/1w/1M（Segmented/快捷键）立即重载，图表+标签同步；启动选中项与 default_timeframe 一致
- [ ] #219: x 轴/十字光标/tooltip 全中文；fork 测试中文断言；compass cargo test 全绿
- [ ] #220: 每条件组标签+控件同行；组间可换行；技术面卡同修；kittest y 对齐断言通过
- [ ] #221: SEPA 排名表格真实窗口显示 50 行；根因确认并修复；表格体断言防回归
- [ ] 全量 cargo test/clippy/fmt 绿；kb 文档同步；PR 审查通过；epic 收尾完成
