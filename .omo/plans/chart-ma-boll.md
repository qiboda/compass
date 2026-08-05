# chart-ma-boll - Work Plan

## TL;DR (For humans)

**What you'll get:** 图表上叠加 MA(5/10/60/120/250) 五条均线和 BOLL(20,2) 布林带（带图例行），且 K 线价格改为前复权显示（工具栏有「前复权」标记）。指标实时计算、不存储。

**Why this approach:** 指标计算与渲染解耦——compass-core 纯函数负责数学（GUI 与未来选股器共用），GUI 侧自定义 Indicator 薄胶水接入 egui-charts 现成渲染管线（零 fork 变更）；前复权在 fetch 层一次性缩放，渲染层无感知。

**What it will NOT do:** 不存储指标、无 MACD、无参数调节控件、无 BOLL 通道填充、无复权模式切换、不改 vendored egui-charts、不扩展选股器。

**Effort:** Medium
**Risk:** Low - 所有技术面已由 Momus + Oracle 双审核验，无阻塞项
**Decisions to sanity-check:** ① 前复权公式 factor_i = adjclose_i/close_i（Oracle 确认与权威公式一致）；② 单实例 8 线 Indicator + NaN 暖机占位；③ 缓存指纹含 symbol；④ 1w/1M 先缩放后聚合

Your next move: 已批准。执行在独立 worker session（`$start-work`）。执行完成前 issue-workflow 创建子 issues 跟踪批次。

---

> TL;DR (machine): Medium effort, Low risk, 5 components / 3 batches; MA+BOLL overlays + 前复权 K-line; review-approved (Momus APPROVE-WITH-CHANGES + Oracle SOUND-WITH-CONCERNS, 4 fixes adopted).

## Scope
### Must have
- C1: compass-core `indicators` 纯函数模块（`ma` / `bollinger` / `adjust_ohlc`）+ 单测（compass-core 95% 门槛）
- C2: fetch 层前复权缩放（duckdb.rs 三子路径 + parquet.rs fetch_bars_blocking 带出 adjclose，factor_i = adjclose_i/close_i，先缩放后聚合）
- C3: GUI 渲染——自定义 8 线 Indicator（crates/compass/src/citizens/indicators.rs）+ IndicatorRegistry + `show_with_indicators` + 图例行自绘（左上第二行 85% alpha chip）+ 工具栏「前复权」Tag
- C4: compass-ui `IndicatorTokens`（8 色暗/亮）+ ChartCitizen 每帧 set_colors 应用
- C5: docs 同步（kb/design/ui.md 设计要点 + kb/design/data-providers.md 前复权说明 + design 文档 §6 公式更正 + 决策记录）
- 每任务失败测试先行（TDD），commit 引用 epic 子 issue `ref #<N>`

### Must NOT have (guardrails, anti-slop, scope boundaries)
- MACD / 任何其他指标
- UI 参数调节控件（周期/倍数为代码常量：MA 5/10/60/120/250、BOLL 20/2.0）
- 指标存储/持久化（实时计算）
- BOLL 通道填充（无 fork 补丁）
- K 线复权模式切换开关（本迭代仅前复权一种模式）
- 修改 vendored egui-charts（零 fork 变更）
- 选股器指标条件扩展（独立后续迭代）
- 扩展 `CompassTheme::apply_to_chart` 签名（set_colors 由 ChartCitizen 做，theme 是参数）
- `as any`/`@ts-ignore` 类类型抑制、空 catch、删除测试求通过

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD** - 每任务先写失败测试（RED），再实现（GREEN）。Rust：`#[cfg(test)]` 单测 + egui_kittest 集成测试（citizen 级 chart.rs:83-174 模式 / full-app main.rs new_eframe 模式 / compass-ui widget 级）。
- 证据：`.omo/evidence/task-<N>-chart-ma-boll.<ext>`（测试输出、lsp_diagnostics、cargo 命令输出）
- 覆盖率：CI 强制 compass-core 95% / compass 80%（`cargo llvm-cov` + `scripts/check-coverage.sh`），本计划内补测试到门槛

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.

- **Wave 1（并行）**: T1 (C1 indicators 纯函数), T2 (C2 fetch 前复权), T3 (C4 IndicatorTokens)
- **Wave 2**: T4 (C3 GUI 渲染接入——依赖 T1/T2/T3)
- **Wave 3**: T5 (C5 docs 同步——依赖全部) + F1-F4 最终验证

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 (C1) | — | T4 | T2, T3 |
| T2 (C2) | — | T4 | T1, T3 |
| T3 (C4) | — | T4 | T1, T2 |
| T4 (C3) | T1, T2, T3 | T5 | — |
| T5 (C5) | T4 | — | F1-F4 |

## Todos
> Implementation + Test = ONE todo. Never separate.

> **Issue 跟踪**（issue-workflow epic 模式，batch 状态：pending/in_progress/done）
>
> ### Batch 1
> | Status | Issue | Task | Depends On |
> |--------|-------|------|------------|
> | done | #175 | core: indicators 纯函数模块（ma/bollinger/adjust_ohlc） | — |
> | done | #176 | core: fetch 层前复权缩放 | #175 |
> | done | #177 | ui: IndicatorTokens（8 色暗亮两套） | — |
>
> ### Batch 2
> | Status | Issue | Task | Depends On |
> |--------|-------|------|------------|
> | done | #178 | gui: MA/BOLL 叠加层渲染 + 图例行 + 前复权 Tag | #175, #176, #177 |
>
> ### Batch 3
> | Status | Issue | Task | Depends On |
> |--------|-------|------|------------|
> | in_progress | #179 | docs: MA/BOLL 叠加层 + 前复权设计同步 | #178 |

- [x] 1. compass-core indicators 纯函数模块（ma/bollinger/adjust_ohlc）
  What to do / Must NOT do: 新建 `crates/compass-core/src/indicators.rs`，`crates/compass-core/src/lib.rs` 增 `pub mod indicators;`。三个 pub 纯函数：(a) `pub fn ma(values: &[f64], n: usize) -> Vec<Option<f64>>`——窗口不足/NaN 输入 → None，永不 panic；(b) `pub fn bollinger(values: &[f64], period: usize, k: f64) -> Vec<(Option<f64>, Option<f64>, Option<f64>)>`——(upper, mid, lower)，mid = ma，std = population stddev，窗口不足 → None；(c) `pub fn adjust_ohlc(raw: &[(chrono::NaiveDate, f64, f64, f64, f64, f64)], adjclose: &[f64]) -> Vec<egui_charts::model::Bar>`——factor_i = adjclose_i / close_i（close<=0 或 adjclose 非有限时 factor=1.0 守卫），OHLC × factor，volume 原样，date → `Bar::new`。风格参照 `crates/compass-strategy/src/sepa/indicators.rs:14`（all_finite 防 NaN、Option 返回、同文件 `#[cfg(test)]` fixture 模式）。必须 NOT 做：不引入 egui-charts 类型进函数签名（除 adjust_ohlc 返回 Bar）、不改 sepa/indicators.rs、不 panic、无 unwrap。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T4
  References: crates/compass-core/src/lib.rs:18-19; crates/compass-strategy/src/sepa/indicators.rs:14-23 (ma 模板), :268-592 (fixture 模式); egui-charts Bar 定义 ~/.cargo/git/checkouts/egui-charts-a14ffbf1d5a8ad83/2b18acd/src/model/bar/bar.rs:30-43 (Bar::new 签名)
  Acceptance criteria (agent-executable): `cargo test -p compass-core indicators` 全绿；`cargo clippy -p compass-core` 无警告；`cargo doc -p compass-core --no-deps` 无 missing_docs 警告；compass-core 覆盖率 ≥95%（`cargo llvm-cov` 后 `scripts/check-coverage.sh`）；`ma(&[], 5)` 与 `ma(&[1.0,2.0], 5)` 返回 None 占位、`ma(&[1.0;6], 5)` 全部 Some(1.0)、NaN 窗口内 → None 窗口外不影响；`adjust_ohlc` 已知除权样本：最新日 factor==1.0、close×factor == adjclose、OHLC 全缩放
  QA scenarios (agent-executable): happy——`cargo test -p compass-core indicators::tests` 输出含 N passed；failure——`ma(&[1.0;3], 5)` 窗口不足返回全 None 且不 panic；`adjust_ohlc` 中 close=0 行 factor 回落 1.0 不产生 inf/NaN（单测断言 `is_finite`）。Evidence `.omo/evidence/task-1-chart-ma-boll.txt`
  Commit: Y | `feat(core): add pure indicators module (ma/bollinger/adjust_ohlc)`

- [x] 2. fetch 层前复权缩放（duckdb.rs 三子路径 + parquet.rs fetch_bars_blocking）
  What to do / Must NOT do: `crates/compass-core/src/data/duckdb.rs` 三条子路径全部带出 adjclose 并按 factor_i = adjclose_i/close_i 缩放 OHLC 后写 Bar：① 内存表路径 (527-533 SELECT 增 adjclose)；② parquet fallback (565-570 已有 adjclose，:593-598 丢弃处改为保留并缩放)；③ 1w/1M 聚合路径 (636-679)——**先缩放后聚合**：内层 SELECT 按日 factor 缩放 OHLC，外层 FIRST(open)/MAX(high)/MIN(low)/LAST(close)/SUM(volume)（LAST 取末根 adjclose 对应因子）。`crates/compass-core/src/data/parquet.rs` fetch_bars_blocking (112-170) SQL 增 adjclose 列并缩放。**优先复用 T1 的 `adjust_ohlc`**（先取原始行 + adjclose 数组 → 调 adjust_ohlc → 得 Bar），避免 4 处逻辑复制；聚合路径可先查原始日线再内存聚合或用 SQL 内联同款表达式。必须 NOT 做：不改 DuckDbProvider 构造/backend.rs、不改 CrossSectionBar/fetch_cross_section（选股器路径不受影响）、不改 DataWriter::save_bars、不引入复权模式开关。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T4
  References: crates/compass-core/src/data/duckdb.rs:527-533, :565-598, :636-679, :681-694 (Bar 构造); crates/compass-core/src/data/parquet.rs:112-179, :135-140 (SQL), :157-170 (Bar 构造); crates/compass/src/backend.rs:79 (GUI 每请求新建 provider → fallback 必走)
  Acceptance criteria (agent-executable): `cargo test -p compass-core` 全绿（新增用例：内存表路径缩放正确、parquet fallback 缩放正确、1w/1M 先缩放后聚合——构造含除权日的周数据断言 MAX(high) 为缩放后最大值）；`cargo clippy -p compass-core` 无警告；compass-core 覆盖率 ≥95%
  QA scenarios (agent-executable): happy——内存 DuckDB tempdir fixture（参照 crates/compass-core/src/data/duckdb.rs:1254+ 模式）：插入已知 adjclose 的日线 → fetch_bars("1d") 断言返回 Bar 的 close == adjclose、open/high/low × factor 正确；failure——close=0 行 fetch 不 panic 且 factor=1.0；1w 聚合含除权日断言高低点。Evidence `.omo/evidence/task-2-chart-ma-boll.txt`
  Commit: Y | `feat(core): fetch 层前复权缩放（adjclose 带出 + adjust_ohlc 应用）`

- [x] 3. compass-ui IndicatorTokens（8 色暗/亮）+ token 测试
  What to do / Must NOT do: `crates/compass-ui/src/tokens/color.rs` 在 ColorTokens 下新增 `pub indicator: IndicatorTokens` 子结构（8 字段：ma5/ma10/ma60/ma120/ma250/bb_upper/bb_middle/bb_lower），dark()/light() 两套值按 design 表（dark: #D1D4DC/#F5A623/#BA68C8/#00BCD4/#A1887F/#90A4AE×3；light: #1B2430/#B57A00/#7B1FA2/#00838F/#6D4C41/#546E7A×3），复用 text_primary/warning 值不新增冗余。参照现有 ChartTokens (color.rs:7-50) 结构 + 每字段 dark/light 配对测试 (color.rs:164-305 模式)。必须 NOT 做：不改 vendored egui-charts IndicatorSemanticTokens、不硬编码色值到 GUI 代码（一律经 tokens 取用）、不扩展 ChartTokens（指标色与图表骨架色职责分离，design 决策记录已锁）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T4
  References: crates/compass-ui/src/tokens/color.rs:7-50 (ChartTokens 结构 + dark/light), :54-102 (ColorTokens), :164-305 (配对测试); .omo/designs/chart-ma-boll.md §1 配色表 + 决策记录; kb/design/ui.md:13-19 (token 系统)
  Acceptance criteria (agent-executable): `cargo test -p compass-ui` 全绿（含新增 IndicatorTokens dark/light 16 值断言测试）；`cargo clippy -p compass-ui` 无警告；`cargo doc -p compass-ui --no-deps` 无 missing_docs 警告
  QA scenarios (agent-executable): happy——新增测试断言 dark.indicator.ma5 == #D1D4DC 等 16 值精确匹配；failure——(编译期) 结构缺字段即编译失败。Evidence `.omo/evidence/task-3-chart-ma-boll.txt`
  Commit: Y | `feat(ui): add IndicatorTokens (MA/BOLL 8 色暗亮两套)`

- [x] 4. GUI 渲染接入（自定义 8 线 Indicator + 图例行 + 前复权 Tag）
  What to do / Must NOT do: (a) 新建 `crates/compass/src/citizens/indicators.rs`：实现 `egui_charts::studies::Indicator` 的 8 线结构 `MaBollIndicator`——`calculate(&mut self, data: &[Bar])` 内取 `bar.close` 序列（fetch 后已是前复权价）调 compass-core `ma()`/`bollinger()`，`values()` 每 bar 返回 `Multiple(vec![ma5, ma10, ma60, ma120, ma250, bb_u, bb_m, bb_l])`，**暖机行 `Multiple([f64::NAN; 8])` 占位**（renderer NaN 自动跳过 → 逐线独立暖机）；实现 7 个必实现方法（name/calculate/values/colors/set_colors/set_visible/clone_box，其余默认）；`line_cnt()=8`、`line_names()` 8 个、`colors()` 来自 8 色。(b) `crates/compass/src/citizens/chart.rs` ChartCitizen 增字段 `registry: IndicatorRegistry` + `cache_key: Option<(String, usize, i64, i64)>`（symbol, len, 首根 time, 末根 time）；`show()` (61-80) 在 update_data 后：读 `app_theme.tokens().color.indicator` 8 色 → 若 cache_key 变化则 `registry.calculate_all(&bars)`（更新指纹）→ 对 registry 各指标 `set_colors`（**ChartCitizen 做，不扩展 apply_to_chart 签名**）→ `self.chart.show_with_indicators(ui, None, Some(&registry))` 替换 `chart.show(ui)`；bars 空时跳过（保留 EmptyState）。(c) 图例行：show 返回后 `ui.painter_at(response.rect)` 在 `rect.min + vec2(40.0, 30.0)` 画第二行（vendored legend 在 y=12，行高 ~16）：`MA5 <v> MA10 <v> ... │ BOLL <u> / <m> / <l>`，值 mono 12px JetBrains Mono + 线色着色，暖机/NaN 显示 `—`，整行 85% alpha `bg_panel_alt` chip + 1px border_strong + radius.sm；取数 `chart.state.visible_range()` → `values()[end.saturating_sub(1)]`（end 开区间）。(d) `crates/compass/src/main.rs` 工具栏 Group B 周期 Segmented 后 (884 附近) 加非交互 `Tag`「前复权」`TagVariant::Custom` + info 色。必须 NOT do：不改 vendored egui-charts、不扩展 apply_to_chart 签名、图例不拦截鼠标、无动画、无参数控件、数值不跟随 hover bar。
  Parallelization: Wave 2 | Blocked by: T1, T2, T3 | Blocks: T5
  References: crates/compass/src/citizens/chart.rs:16-20 (struct), :61-80 (show), :83-174 (kittest 模式); egui-charts studies 接口 ~/.cargo/git/checkouts/egui-charts-a14ffbf1d5a8ad83/2b18acd/src/studies/indicator_trait.rs:54-150 (trait), studies/mod.rs:92-153 (registry), widget/mod.rs:620-625 (show_with_indicators), chart/renderers/indicator.rs:89-100 (line_segment 渲染), model/chartstate.rs:150-165 (visible_range, end 开区间), chart/renderers/labels.rs:311-313 (legend 锚点 rect.min+(40,12)); crates/compass/src/main.rs:884-903 (工具栏 Group B); crates/compass-ui/src/widgets/tag.rs (Tag 组件)
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿（kittest 新增：① 有 bars 时 8 线渲染不 panic + 图例行文字 `MA5`/`BOLL` 存在（用 registry values 断言而非像素）；② bars 空时 EmptyState 不 panic；③ 切标的 cache_key 变化触发重算——构造两股同 len 同末根 time 断言指标值更新；④ 「前复权」Tag 存在）；`cargo clippy -p compass` 无警告；`cargo doc -p compass --no-deps` 无 missing_docs 警告；compass 覆盖率 ≥80%
  QA scenarios (agent-executable): happy——`cargo test -p compass citizens::chart` kittest 全过；kittest 断言 registry.indicators()[0].values()[end-1] 的 Multiple 值 == 手工计算的 MA5/BOLL（缩放后 close 输入）；failure——切标的同指纹场景断言 values 更新（防缓存碰撞回归）；空 bars 断言无 panic。Evidence `.omo/evidence/task-4-chart-ma-boll.txt`
  Commit: Y | `feat(gui): 图表 MA/BOLL 叠加层 + 图例行 + 前复权 Tag`

- [x] 5. docs 同步（kb/design/ui.md + data-providers.md + design 公式更正 + 决策记录）
  What to do / Must NOT do: (a) `kb/design/ui.md` 追加本 feature 设计要点（MA 白/黄/紫/青/棕配色表暗亮两套、BOLL slate 三线不填充、图例行左上第二行 85% chip、前复权 Tag、叠放层级）到设计变更记录表 + 决策记录表；(b) `kb/design/data-providers.md` 补前复权说明（fetch_bars 返回前复权价、factor_i = adjclose_i/close_i、1w/1M 先缩放后聚合）；(c) `.omo/designs/chart-ma-boll.md` §6 公式更正标注（`adjclose_latest/adjclose_i` → `factor_i = adjclose_i/close_i`，附 Oracle 核验理由）；(d) `.omo/plans/chart-ma-boll.md` 任务表格填子 issue 编号。必须 NOT do：不改 kb/ 其他文件、不写新 kb/ 文件（除非 docs skill 判定必要）、不关闭任何 issue。
  Parallelization: Wave 3 | Blocked by: T4 | Blocks: —
  References: kb/design/ui.md:185-208 (设计变更记录 + 决策记录格式); kb/design/data-providers.md:89-127 (Schema 章节), :331-335 (决策记录); .omo/designs/chart-ma-boll.md:231-246 (§6), :284-300 (决策记录)
  Acceptance criteria (agent-executable): 三个目标文件更新到位；ui.md 决策记录表含本 feature 行；data-providers.md 含前复权说明；design 文档 §6 更正标注；`git diff` 审查通过
  QA scenarios (agent-executable): happy——grep 断言 ui.md 含 `MA` 配色/「前复权」/BOLL 关键行、data-providers.md 含 `factor`、design §6 含更正标注；failure——git diff 检查无意外文件变更。Evidence `.omo/evidence/task-5-chart-ma-boll.txt`
  Commit: Y | `docs: MA/BOLL 叠加层 + 前复权设计同步（ui.md/data-providers.md/design 更正）`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — 5 todos 全 done、验收标准逐条核验、commit message 含 `ref #<sub-N>`
- [ ] F2. Code quality review — `/review-work` 5 并行 agent（goal/quality/security/QA/context）全过；lsp_diagnostics 干净；无类型抑制/空 catch
- [ ] F3. Real manual QA — `cargo test` 全 workspace 绿 + `cargo build` 成功；覆盖率达标的 `scripts/check-coverage.sh` 输出
- [ ] F4. Scope fidelity — 对照 Must NOT have 逐条核查（无 MACD/无参数控件/无存储/无填充/无 fork 变更/无 apply_to_chart 签名扩展）

## Commit strategy
- 每 todo 一个 commit（epic 工作流：一个 epic = 一个 PR = 多个 commit，每个 commit 引用子 issue `ref #<sub-N>`）
- push 前 rebase origin/master；push 前用户确认；push 后反思 commit（/reflect）+ 追加完成 comment + 关闭子 issues + 关闭 epic（issue-workflow 阶段 4）

## Success criteria
- [ ] K 线前复权显示（OHLC 全缩放）+ 工具栏「前复权」Tag
- [ ] MA 5/10/60/120/250 五线 + BOLL(20,2) 三线叠加显示（暗/亮两套主题 8 色可辨）
- [ ] 图例行左上第二行（值 mono + 线色 + 85% chip，暖机显示 —）
- [ ] 指标实时计算不存储；缓存指纹 (symbol, len, 首末 time) 无碰撞
- [ ] 暖机逐线独立（MA5 bar 5 起画、MA250 bar 249 起画）
- [ ] 1w/1M 先缩放后聚合（除权日周线高低点准确）
- [ ] 全 workspace 测试绿、覆盖率达标（core 95% / compass 80%）、clippy/doc 干净
- [ ] docs 同步 + 决策记录完整
