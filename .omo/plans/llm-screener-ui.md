# llm-screener-ui - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 选股器界面从固定表单升级为 Metabase 风格的条件卡片组——可添加/删除条件卡片、AND/OR 分组任意嵌套、一键清空、支持取反，底层直接操作表达式 AST。现有 6 类基础条件 + 5 类技术条件全部作为预置卡片保留，行为不变。

**Why this approach:** 卡片是 Filter AST 的视图模型（双向纯函数映射，round-trip 结构等价），不新造数据模型——Batch 4 的 LLM 文本路径和本 UI 共享同一 AST；Metabase 范式被业界验证为最直观的条件构建交互。

**What it will NOT do:** 不实现引擎求值（Batch 3 才做，Or/序列条件运行时给友好提示）；不做拖拽排序；不改持久化格式（仍存旧格式，无法表达的组合提示不保存）；不新增任何组件或依赖。

**Effort:** Medium
**Risk:** Medium - 现有表单迁移范围广（screener.rs 1110 行重构 + 既有测试迁移），但 AST 契约与引擎已锁定，UI 层风险可控
**Decisions to sanity-check:** 默认预置 6 卡 vs 空态（已确认预置）；Count 卡延后；legacy 保存边界；取反开关提供

Your next move: plan approved — execute via `$start-work llm-screener-ui` in a worker session.

---

> TL;DR (machine): Medium effort, Medium risk, 6 implementation todos + 4 final-verification tasks; Metabase-style AND/OR nested condition card builder operating on the Batch 1 Filter AST, replacing the fixed ConditionForm in the compass screener citizen.

## Scope
### Must have
- **视图模型 + 双向映射纯函数**（compass crate 业务层，新文件 `crates/compass/src/citizens/screener_builder.rs`）：`CondItem`（Leaf/Group 递归）、`CondGroup{operator: BoolOp, items}`、`CondLeaf{kind, params, negated}`、`LeafKind`（Industry/Exchange/Board/ListYears/MarketCap/Delisted/Ma/Breakout/Momentum/VolumeSurge/UpDays——**不含 Count**）、`LeafParams`；纯函数 `filter_to_items(&Filter) -> Vec<CondItem>`（反向识别）、`leaf_to_filter(&CondLeaf) -> Filter`、`group_to_filter(&CondGroup) -> Filter`（正向构建）；round-trip 结构等价保证「现有功能不回归」
- **11 类模板识别表 + 组合节点规则**：Meta 6 变体、Series 4 类（Ma 三形状/Breakout/Momentum 成对识别/VolumeSurge）、UpDays 序列卡；`And(vec)`/`Or(vec)` → 递归 CondGroup；**单成员 And/Or 折叠（仅显示）**；`Not(Box(x))` → `negated: true`；**无法识别形状 → 只读摘要卡**（mono JSON 摘要 + 删除）
- **条件卡片组 UI**：根组一张 Card（组头 Segmented 且/或 + Badge 条件数 + 清空 IconButton）+ 递归子组（Frame 轻量容器 bg_panel_alt + border_strong + 左缩进）+ 组底添加菜单（Dropdown 含「子分组」）+ Leaf 卡行（类型 Dropdown + 参数控件 + 取反 + 删除）+ 空态 EmptyState；原子组布局沿用 scope_builder 技巧（ref #220）；**全部弹层组件显式 id_salt**
- **默认根组预置 6 张基础卡**（industry/exchange/board/list_years/market_cap/delisted，排除退市勾选）——与现状行为逐项一致
- **就地常驻编辑**：无确认按钮、无向导模态；删除/清空无确认（Metabase 惯例）
- **消息契约**：`RunScreenerRequest { query: ScreenerQuery }` → `{ filter: Filter }`；backend.rs 去 `Filter::from` 中转（并入 Todo 3，与运行按钮同步）
- **legacy 持久化**：on_save 保持 `Fn(&ScreenerQuery)` 签名；**复用引擎 `filter_to_query`**（改 pub）作为压缩判定——Ok 则保存，Err(UnsupportedFilter)（含 Or/Not/UpDays/Unknown/嵌套子组/重复单例字段）→ toast 提示 `screener.builder.unsupported_save` 不保存
- **i18n**：`screener.builder.*` 子命名空间（zh/en 对称），复用既有 screener.*/widgets.*/common.* 键
- **测试**：纯函数 round-trip AST 断言（单测，多成员结构等价 + 裸单节点 leaf 级等价）+ kittest 交互路径（添加/删除/AND-OR 切换/清空/嵌套/restore/取反/未知形状）；既有测试与 GROUP_ALIGNMENT 系列（5 个）迁移到新结构
- **doc-sync**：`kb/design/ui.md`（最终设计要点 + 决策记录补齐）、`kb/user/gui.md`（选股器章节）、`kb/dev/testing.md`（如测试惯例变化）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- **不做引擎求值**（Batch 3 #246）：UI 只构建 AST；Or/Not/UpDays/Unknown/**嵌套子组**运行时经 `starts_with("unsupported filter shape")` 匹配给出友好提示（键 `screener.builder.unsupported_run`），不实现引擎
- **不做 Count 卡**（用户确认项 4 延后）：LeafKind 无 Count；`screener.builder.cond_count` 等键不建
- **不做 AST JSON 持久化**（用户确认项 5）：保持 legacy TOML `[screener]` 保存
- **不删除/迁移 ScreenerQuery**：`ScreenerQuery`/`MaCondition`/`BreakoutCondition`/`MomentumCondition`/`VolumeCondition` 及 `From<ScreenerQuery> for Filter` 全部保留
- **不做拖拽排序/卡片移动**（egui 拖拽成本高、非验收项）
- **不新增 compass-ui 组件**：零新增，全复用现有 24 组件
- **不改 backend 引擎逻辑**：run_screener/screen_symbol 不动（仅 filter_to_query 改 pub/包装）
- **不单开新 crate；不新增第三方依赖**
- **不引入直接 Slot 消息断言惯例**（slot.receiver try_recv 全仓库无先例）：消息断言用 on_save 回调捕获 + shared-state 间接断言
- **不手动写 kittest drag 手势**（无先例）：数字参数断言用字段 seed + build_filter AST 断言
- **不自建 Filter→ScreenerQuery 压缩逻辑**（复用引擎 filter_to_query，避免第三份 accept-grammar 漂移）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（门禁 3.5 步委派 skwy-adversarial-test 写对抗性测试 RED；门禁 4 步委派 skwy-requirement-test 写需求测试 RED；实现一次通过两批测试）+ egui_kittest 无头测试 + 纯函数单测（既有模式，见 kb/dev/testing.md）
- Evidence: `.omo/evidence/task-<N>-llm-screener-ui.txt`（attemptDir = `.omo/evidence/`）
- 构建：`cargo check`（subagent 编译权限分级，见 subagent-compile skill）/ `cargo test` / `cargo clippy`（mold 链接器已配置）
- 覆盖率：`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json`（compass crate 90% 门槛不变）

## Execution strategy
### Parallel execution waves
- **Wave 1**（Todo 1-2 并行）：视图模型纯函数（无跨模块依赖）+ i18n 键（纯 yml + lib.rs 白名单检查，无依赖）
- **Wave 2**（Todo 3）：条件卡片组渲染（依赖 1,2）
- **Wave 3**（Todo 4）：契约变更 + legacy 保存（依赖 1,3）
- **Wave 4**（Todo 5）：测试迁移 + 新增（依赖 3,4 的可编译接口）
- **Wave 5**（Todo 6）：doc-sync + 决策记录（依赖全部实现）

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. 视图模型 + 双向映射 | — | 3,4,5 | 2 |
| 2. i18n 键 | — | 3 | 1 |
| 3. 条件卡片组渲染 + Filter 契约 | 1,2 | 4,5 | — |
| 4. legacy 保存 + UnsupportedFilter 提示 | 1,3 | 5 | — |
| 5. 测试迁移 + 新增 | 3,4 | 6 | — |
| 6. doc-sync + 决策记录 | 1,2,3,4,5 | F-wave | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. compass crate: 视图模型 + 双向映射纯函数（CondItem/CondGroup/CondLeaf/LeafKind/LeafParams + filter_to_items/leaf_to_filter/group_to_filter）
  What to do / Must NOT do: 在 `crates/compass/src/citizens/screener_builder.rs`（新文件，避免 screener.rs 1110 行继续膨胀）定义视图模型与纯函数。类型：`enum BoolOp { And, Or }`；`enum CondItem { Leaf(CondLeaf), Group(CondGroup) }`；`struct CondGroup { operator: BoolOp, items: Vec<CondItem> }`；`struct CondLeaf { kind: LeafKind, params: LeafParams, negated: bool }`；`enum LeafKind`（Industry/Exchange/Board/ListYears/MarketCap/Delisted/Ma/Breakout/Momentum/VolumeSurge/UpDays——**不含 Count**）；`LeafParams` 按 kind 解释（Vec<String> 多选、u32/f64 数值、MaKind、Delisted bool）。纯函数：`pub fn filter_to_items(f: &Filter) -> Vec<CondItem>`（反向识别，模板表见 .omo/designs/llm-screener-ui.md §2：Meta 6 变体各归位；Ma 三形状 `Cmp{Close,Gt,Factor(Sma(20/60))}` 与 BullishAlign `And[Cmp{Sma5,Gt,Sma20},Cmp{Sma20,Gt,Sma60}]` 归 Ma 卡；Breakout `Cmp{Close,Gt,Factor(NDayHigh(n))}`；Momentum 成对识别 `And` 内两个 Cmp 同 factor=ChangePct(n) 且 op 为 Ge+Le（**不同 n 的两 Cmp 不合成动量卡**）；VolumeSurge；UpDays；`And(vec)`/`Or(vec)` → CondGroup 递归；**单成员 And/Or 折叠**：`And(vec![x])` 与裸节点 `x` **渲染同一形态**（单卡不产生多余子组）；`Not(Box(x))` → 识别 x 后置 `negated: true`；**无法识别形状 → `CondLeaf{kind: Unknown, params: Json摘要字符串}` 只读摘要卡**）、`pub fn leaf_to_filter(l: &CondLeaf) -> Filter`、`pub fn group_to_filter(g: &CondGroup) -> Filter`（正向构建）。**round-trip 语义（审查修订，ref #245 评审）**：`group_to_filter` 的根组单成员**产出裸节点**（`CondGroup{And, vec![x]}` → `x`，与 `From<ScreenerQuery>` 的 `1 => nodes.pop()` 形状一致）；嵌套组单成员同理（`And(vec![x])` 子组 → 折叠为 x 作为父级成员，`Or` 同理）。round-trip 断言用**归一化等价**：`filter_to_items(f) → 卡片 → 重建 == f` 对**多成员/嵌套形状**成立（结构精确相等）；对**裸单节点形状**（如 `Meta(Delisted(false))`）断言 `filter_to_items(f)` 产出 1 张卡且 `leaf_to_filter(该卡) == f`（leaf 级等价，不经过 group 包装）。**模块级 `#[cfg(test)] mod tests` 写纯函数单测**：多成员/嵌套 round-trip 结构等价；裸单节点 leaf 级等价；折叠规则（`And(vec![x])` 与裸 `x` 同形态）；Not 包裹；未知形状兜底；Momentum 成对识别（同 factor 成对、**不同 n 不合成**、**反向 Le→Ge 顺序不合成**）；深层嵌套（3 层 And/Or 混合）。MUST NOT: 不写 UI 渲染代码（Todo 3）；不引入 compass-ui/egui 依赖（纯逻辑模块）；不做 Count 卡；不改 compass-types AST；不写 kittest（Todo 5）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 3,4,5
  References (executor has NO interview context - be exhaustive): .omo/designs/llm-screener-ui.md §1-2（视图模型 + 模板表 + 组合节点规则）; .omo/handoff.md（锁定设计）; crates/compass-types/src/screener.rs L17-307（Filter/MetaCond/SeriesCond/SeriesFactor/CmpOp/FactorRef 定义 + From<ScreenerQuery> 产出形状——反向识别必须覆盖 From 全部合法产出）; crates/compass-types/src/screener.rs L221-307（From 编译层形状：Industries→Meta(Industry)、Delisted(false)、Ma 三形状 L246-272、Breakout L274-280、Momentum L281-294、Volume L295-300）+ **L302-306（nodes.len() 匹配：0 → And(vec![])、1 → 裸节点 pop、>1 → And——单成员折叠必须与此形状对齐）**; crates/compass/src/citizens/screener.rs L205-235（build_query 现状——11 字段对应模板）; kb/design/ui-widgets.md（组件规范，视图模型不入 compass-ui 的硬边界）
  Acceptance criteria (agent-executable): `cargo check -p compass` 通过；`cargo test -p compass` 新增单测通过：每 LeafKind 的 `leaf_to_filter` 产物与模板表精确相等（断言 `Filter::PartialEq`）；`filter_to_items` 对 From 产出的全部形状：多成员/嵌套 round-trip 结构等价（`filter_to_items(f) → 卡片 → 重建 == f`）、裸单节点 leaf 级等价（`leaf_to_filter(单卡) == f`）；单成员折叠断言（`filter_to_items(And(vec![x]))` 与 `filter_to_items(x)` 产出同形态）；Momentum 不同 n 与反向 op 不合成断言；未知形状兜底断言；深层嵌套（如 `And[Or[Meta, Series], Not[Meta]]`）round-trip 断言
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass` 新增单测全绿; failure: 手造非法/未知 AST 形状 → `filter_to_items` 不 panic、产出 Unknown 摘要卡（健壮性兜底）; Evidence .omo/evidence/task-1-llm-screener-ui.txt
  Commit: Y | `feat(screener): condition builder view model + Filter mapping` + 独立成行 `ref #245`

- [ ] 2. compass-i18n: `screener.builder.*` 键（zh/en 对称 + 白名单校验）
  What to do / Must NOT do: 在 `crates/compass-i18n/locales/zh.yml` 与 `en.yml` 对称新增（键集必须完全一致，lib.rs:383-392 对称测试强制）：`card_title`（筛选条件）、`add_condition`（添加条件）、`add_group`（子分组）、`group_and`（且 (AND)）、`group_or`（或 (OR)）、`empty_title`（暂无筛选条件）、`empty_desc`（点击「添加条件」构建筛选，或直接筛选查看全市场）、`empty_group`（空分组）、`clear_tooltip`（清空条件）、`delete_tooltip`（删除条件）、`negate_tooltip`（取反（排除））、`unknown_shape`（高级条件）、`unsupported_run`（该条件类型引擎暂不支持，将在后续版本可用: %{e}）、`unsupported_save`（该条件组合无法保存到配置文件）、`cond_up_days`（连续上涨）。**复用既有键不新建**：`screener.industry/exchange/board/list_years/market_cap/exclude_delisted/ma/ma_above20/ma_above60/ma_bullish/breakout/momentum/volume/n_label/min_pct/max_pct/times`、`widgets.multi_select.selected/confirm`、`common.confirm/cancel`、`error.screener_run`。**白名单校验（强制）**：检查 `is_allowed_zh_token`（lib.rs L349-359）——所有新键 zh 值必须含 CJK 字符或 `%{`（本设计键值均含 CJK：筛选条件/添加条件/子分组/且 (AND) 等；`unsupported_run` 含 `%{e}` 插值），**若任何新键值 CJK-free（如纯 "N:" 技术标记）则必须扩展白名单前缀 + 同步两个表驱动测试** `zh_whitelist_prefixes_allow_cjk_free_values`（lib.rs L438-478）与 `zh_whitelist_rejects_non_whitelisted_keys`（lib.rs L483-501）。沿用现有惯例：UI 层用裸 `t!("screener.builder.xxx")` 字符串（screener.rs 现有风格），不新增 KEY_* 常量（除非补进 ALL_KEYS lib.rs L176-322）。MUST NOT: 不建 Count 卡相关键（cond_count/param_window/param_at_least/param_value——用户确认延后）；不改既有 screener.* 键值；不破坏 zh/en 对称（缺一即对称测试失败）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 3
  References (executor has NO interview context - be exhaustive): .omo/designs/llm-screener-ui.md §6（i18n 键组织——减去 Count 相关键）; crates/compass-i18n/locales/zh.yml L86-118（screener.* 既有键）+ L34-42（common.*）+ L189-197（widgets.*）; crates/compass-i18n/src/lib.rs L349-359（is_allowed_zh_token 白名单）+ L383-392（对称测试）+ L415-428（zh_values_are_chinese）+ L438-501（白名单表驱动测试）; crates/compass/src/citizens/screener.rs L50-57 + L472-474（MaKind::label 动态键模式——枚举→键映射的既有模板）
  Acceptance criteria (agent-executable): `cargo test -p compass-i18n` 全绿（对称测试 `locale_files_are_key_symmetric` 通过、`zh_values_are_chinese` 通过——新键 zh 值含 CJK 或命中扩展后白名单）；`grep -c "builder:" crates/compass-i18n/locales/zh.yml` 与 en.yml 相等
  QA scenarios: happy: `cargo test -p compass-i18n` 全绿; failure: 故意只加 zh 不加 en → 对称测试失败（验证强制对称）; Evidence .omo/evidence/task-2-llm-screener-ui.txt
  Commit: Y | `feat(i18n): screener builder namespace keys` + 独立成行 `ref #245`

- [ ] 3. compass crate: 条件卡片组渲染（根组 Card + 递归子组 + 添加/删除/AND-OR/清空/取反/空态）
  What to do / Must NOT do: 在 `crates/compass/src/citizens/screener.rs` 重构 `ScreenerPanel`：删除 `ConditionForm`（L23-39）、`build_query`（L205-235）、`condition_form`（L323-343）、`basic_conditions`（L354-441）、`technical_conditions`（L450-538）及 `ms_industry/ms_exchange/ms_board` 字段；新增 builder 状态（持有根组 `Vec<CondItem>` + `HashMap<String, MultiSelect>`）。**MultiSelect 状态归属（审查修订）**：`CondLeaf.params` 是**唯一数据真相**（AST 构建源）；每帧渲染前从 params 同步 `options`/`selected` 进实例，交互后写回 params；map 值仅缓存 `open`/`filter` 瞬态（`HashMap<String, MultiSelect>` key = 节点路径 `cond_root_0_industry` 等）。**删卡时必须移除该路径的 map 条目**（否则路径移位复用旧实例 → 旧选中值复活/数据丢失）；**切类型时重建该路径实例**（参数重置默认值）；**`set_tokens`（L547-553）改为遍历 map 全部实例**（主题切换不回归）。`ScreenerPanel::new` 签名**保持不变**（L135-141：restore: Option<&ScreenerQuery>）。**restore 初始化规则（审查修订）**：restore 为 `None`，**或** restore 编译为默认空形状（`Filter::from(restore)` 为裸 `Meta(Delisted(false))` 或 `And(vec![])`）→ seed **预置 6 张基础卡**（industry/exchange/board/list_years/market_cap/delisted 排除退市勾选，与现状 default 行为一致）；否则 `Filter::from(restore)` → `filter_to_items` 渲染。渲染结构（.omo/designs/llm-screener-ui.md §3-5）：根组 Card（title `t!("screener.builder.card_title")`）组头行 [Segmented 且/或 + Badge 条件数 + 清空 IconButton]；Leaf 卡行 = 类型 Dropdown（切换类型重置参数默认值 + 重建 MultiSelect 实例）+ 参数控件（按 kind：MultiSelect/Dropdown/DragValue/Checkbox）+ 取反 IconButton（negated → Not 包裹）+ 删除 IconButton（删除时移除 map 条目）；子组 = Frame 轻量容器（`fill(bg_panel_alt)` + `stroke(border_strong)` + `corner_radius(radius.sm)` + 左缩进 spacing.md，**不用 Card-in-Card**）；组底添加菜单 Dropdown（选项 = LeafKind 列表 + 「子分组」项 → 组尾插入默认卡/空子组）；空态 EmptyState（`t!("screener.builder.empty_title")`/`empty_desc`）；Unknown 卡 = 只读摘要（Label mono weak + 删除按钮）。**原子组布局**：Leaf 卡行沿用 `basic_group`/`technical_group` 的 scope_builder 技巧（screener.rs L556-617），label+control 同行、组间换行。**id_salt（ref #220/#222，强制）**：全部 Dropdown/MultiSelect 显式 salt `cond_{路径}_{字段}`（如 `cond_root_0_market_cap_min`）；MultiSelect 实例 map key = 同路径。参数控件按 kind：Industry/Exchange/Board → MultiSelect（options 每帧刷新同现状 L324-325）；ListYears → Dropdown（不限/≥1/≥3/≥5 复用 screener.any/years_1/3/5）；MarketCap → 两个 DragValue（min/max 亿）；Delisted → Checkbox（勾选=卡存在，取消=删卡）；Ma → kind Dropdown（复用 screener.ma_above20/60/ma_bullish）；Breakout → DragValue days（默认 60，1-250）；Momentum → DragValue days/min/max；VolumeSurge → DragValue days/times；UpDays → DragValue n/min_pct。**运行按钮 + 契约变更（审查修订：本 todo 一并完成，避免 Todo 4 编译时序矛盾）**：`crates/compass/src/messages.rs` L30-34 `RunScreenerRequest { query: ScreenerQuery }` → `{ filter: Filter }`（Filter 已 Clone）；`crates/compass/src/backend.rs` L149-150 `run_screener(&Filter::from(req.query.clone()), ...)` → `run_screener(&req.filter, ...)`；运行按钮逻辑（L251-267）改为 `group_to_filter(根组)` → `RunScreenerRequest { filter }`。**同步迁移 backend.rs tests L604/L654 两处 `RunScreenerRequest { query }` 构造**（改为 `RunScreenerRequest { filter: Filter::from(query) }` 或等价，否则契约变更后编译失败）。MUST NOT: 不写 legacy 保存压缩与 UnsupportedFilter 提示（Todo 4）；不实现引擎；不做 Count 卡；不做拖拽；不新增 compass-ui 组件；GROUP_ALIGNMENT 现有测试迁移（Todo 5）——本 todo 可先让旧测试标记编译失败待迁移。
  Parallelization: Wave 2 | Blocked by: 1,2 | Blocks: 4,5
  References (executor has NO interview context - be exhaustive): .omo/designs/llm-screener-ui.md §3-5（布局/交互/组件复用清单）+ §7（不回归保证）; crates/compass/src/citizens/screener.rs L135-141（new 签名）+ L205-235（build_query 替换点）+ L323-538（被替换的 condition_form/basic/technical）+ L547-553（set_tokens 需改）+ L556-617（atomic group 技巧）; crates/compass/src/messages.rs L30-34（RunScreenerRequest 定义）+ L2（ScreenerQuery import 改 Filter）; crates/compass/src/backend.rs L149-150（唯一生产调用点）+ **L604/L654（测试构造点，须同步迁移）**; crates/compass-ui/src/widgets/card.rs L28-63（Card::new().title().padding().show）；segmented.rs L17-42（Segmented .selected().show → Option<usize>）；dropdown.rs L24-63（Dropdown .selected().width().id_salt().show → Option<usize>）；multi_select.rs L45-96（MultiSelect 状态持有 + id_salt + pub options 每帧刷新）；icon_button.rs L18-46（IconButton phosphor glyph .tooltip().small().show → bool）；badge.rs L27-52（Badge::new(tokens, count).tone()）；empty_state.rs L20-43（EmptyState .description().action()）；checkbox.rs L18-34（Checkbox 绑定 &mut bool）; crates/compass-ui/src/tokens/color.rs L116/124（bg_panel_alt/border_strong）+ radius.rs L7（radius.sm=4）; kb/design/ui-widgets.md（id_salt 规则 L219-232/L585-610）; crates/compass-types/src/screener.rs L302-306（From 单节点产出裸节点——restore 默认形状判断依据）
  Acceptance criteria (agent-executable): `cargo check -p compass` 通过；`cargo test -p compass --bin compass` 通过（既有不依赖 ConditionForm 内部字段的测试——结果表/消息测试保持绿；form 相关测试标记迁移至 Todo 5）；`RunScreenerRequest` 全仓 grep 无残留 `query:` 字段构造（含 backend.rs L604/L654 已迁移）；backend.rs 无 `Filter::from` 中转；渲染 smoke：根组 Card + 预置 6 卡渲染无 panic 测试
  QA scenarios: happy: `cargo check -p compass` + `cargo test -p compass` 相关测试绿; failure: 无——渲染正确性由 Todo 5 kittest 验证; Evidence .omo/evidence/task-3-llm-screener-ui.txt
  Commit: Y | `feat(screener): condition card group UI + Filter request contract` + 独立成行 `ref #245`

- [ ] 4. compass crate: legacy 保存压缩（复用引擎 filter_to_query）+ UnsupportedFilter 友好提示
  What to do / Must NOT do: ① **legacy 保存（审查修订：复用引擎 accept-grammar 而非自建）**：`crates/compass-strategy/src/lib.rs` 将私有 `filter_to_query`（受限反向转换函数）改为 `pub`（或新增 `pub fn try_compress_to_query(f: &Filter) -> Result<ScreenerQuery, ScreenerError>` 薄包装，内部调既有 filter_to_query）；compass 已依赖 compass-strategy（backend.rs L22 import run_screener），无新依赖。`ScreenerPanel` 运行按钮 on_save 路径（main.rs L99-109 调用点与签名 `Fn(&ScreenerQuery)` **保持不变**）：`group_to_filter(根组)` 产物传入 try_compress → **Ok(query)** → `(self.on_save)(&query)` 写 legacy TOML；**Err(UnsupportedFilter)** → toast 提示 `t!("screener.builder.unsupported_save")`（复用现有 toast 机制），不写盘。**复用引擎函数保证了重复单例字段检测**（SeenFields：双 momentum/双 Ma/双 Delisted → UnsupportedFilter，引擎 L170-178）与嵌套子组不可压缩性（嵌套普通子组非 pair → UnsupportedFilter，引擎 L209-221）——不自建第三份 accept-grammar。② **UnsupportedFilter 友好提示（审查修订：匹配稳定前缀而非变体名）**：结果区错误文案（screener.rs L287-288）对错误字符串 `starts_with("unsupported filter shape")`（ScreenerError Display 前缀，strategy lib.rs L42-43，**不是** "UnsupportedFilter" 变体名）追加一句 `t!("screener.builder.unsupported_run", e = err)`。③ 明确运行边界（设计 §7 已确认）：根组 Or、Not、UpDays、Unknown、**任何用户自建嵌套子组**（引擎仅接受 momentum/bullish pair 形态的嵌套 And）运行时报 UnsupportedFilter + 友好提示——这是 Batch 2 预期行为，Batch 3 引擎支持后自然消失。MUST NOT: 不改 `save_screener_config`（main.rs L403-428 保持）；不改 `run_screener` 引擎求值逻辑（只把 filter_to_query 改 pub/包装）；不实现通用 Filter→ScreenerQuery 反向（仅复用引擎既有函数）；不改 config.toml 格式；不改 on_save 调用点签名；不新增依赖。
  Parallelization: Wave 3 | Blocked by: 1,3 | Blocks: 5
  References (executor has NO interview context - be exhaustive): crates/compass/src/main.rs L99-109（on_save 传入）+ L403-428（save_screener_config）; crates/compass/src/citizens/screener.rs L251-267（筛选按钮发送路径）+ L284-296（results_area 错误展示）; crates/compass-strategy/src/lib.rs L35-43（ScreenerError 含 UnsupportedFilter + Display "unsupported filter shape: {0}"）; crates/compass-strategy/src/lib.rs filter_to_query（受限反向转换本体——私有，本次改 pub；SeenFields 重复检测 L170-178；嵌套 sub-And 仅 momentum/bullish pair 可接受 L209-221）; crates/compass-strategy/src/lib.rs L128-239（accept-grammar 总览）; .omo/designs/llm-screener-ui.md §7（legacy 保存边界：可压缩则保存，不可表达 toast 提示）+ §5 交互表「运行」行（unsupported_run 键）
  Acceptance criteria (agent-executable): `cargo check -p compass` + `cargo check -p compass-strategy` 通过；`cargo test -p compass` 相关测试绿；构造含 Or/Not/UpDays/Unknown/嵌套子组/重复单例字段的 builder 状态 → 点筛选 → on_save 不被调用 + toast 显示 unsupported_save（单测断言 on_save 回调未触发且 try_compress 返回 Err(UnsupportedFilter)）；错误文案 starts_with("unsupported filter shape") 时追加 unsupported_run 提示（单测断言拼接结果）
  QA scenarios: happy: `cargo test -p compass` + `cargo test -p compass-strategy` 绿（既有 21 个 screener 语义测试断言不变——filter_to_query 仅改 pub 不改变行为）; failure: 重复单例字段（双 momentum）压缩返回 Err 而非静默丢数据; Evidence .omo/evidence/task-4-llm-screener-ui.txt
  Commit: Y | `refactor(screener): legacy save via engine compress + unsupported hints` + 独立成行 `ref #245`

- [ ] 5. compass crate: 测试迁移 + 新增（kittest 交互路径 + 纯函数断言 + GROUP_ALIGNMENT 迁移）
  What to do / Must NOT do: ① **迁移既有测试**（screener.rs tests L632-1110）：`new_form_defaults_match_query_contract`（L659-671）→ 断言新结构默认根组预置 6 卡 + exclude_delisted 勾选；`build_query_reflects_conditions`（L674-695）→ 改为断言 `group_to_filter(根组)` 的 Filter 结构（行业选中 → Meta(Industry)、市场波动 → 对应 Series 形状）；`restore_seeds_form_and_multi_selects`（L698-712）→ `ScreenerPanel::new(Some(&query))` → 断言 filter_to_items 识别出的卡片参数与 query 一致；`multi_selects_are_independent`（L715-731）→ 改为 builder 卡片级多选独立性断言；**GROUP_ALIGNMENT 系列**（L943-1109：4 个宽度扫描 + `condition_groups_still_wrap_between_on_narrow_width` L1010-1032，共 **5 个**）→ 全部迁移到新结构（Leaf 卡行 label+control 同行约束，仍用 `assert_same_row` helper + `GROUP_ALIGNMENT_WIDTHS` + LANG_LOCK + en 测试结尾 set_locale("zh")）。② **backend.rs 测试迁移**（审查修订）：backend.rs tests L604/L654 两处 `RunScreenerRequest { query }` 构造改为 `RunScreenerRequest { filter: Filter::from(query) }` 或等价。③ **新增纯函数断言测试**（screener_builder.rs mod tests，若 Todo 1 已写则复核补缺）：round-trip 多成员/嵌套结构等价 + 裸单节点 leaf 级等价。④ **新增 kittest 交互路径测试**（仿 screener.rs L745-778 模式：`Harness::new_ui` + `LANG_LOCK` + `Queryable` import）：添加（点「添加条件」→ Dropdown 选类型 → 卡片出现 → 参数生效 → 断言 build filter AST）、删除（删除 IconButton 点击 → 卡片消失 + 对应 map 条目移除 → AST 移除）、AND-OR 切换（组头 Segmented 点击 → operator 翻转）、清空（清空 IconButton → 空态 EmptyState 出现）、嵌套（添加菜单选「子分组」→ 子组内加卡 → AST 嵌套 And/Or 结构）、restore（`ScreenerPanel::new(Some(&legacy_query))` → kittest 渲染 → 点击筛选 → 断言 on_save 捕获的 ScreenerQuery 与 legacy 等价——**用 on_save 回调捕获，不用 slot.receiver**；legacy 为**多条件** query 以避开单节点归一化歧义，或对单节点断言归一化等价）、取反（negate 开关 → AST `Not` 包裹）、未知形状（构造含未知 AST 的 restore → 只读摘要卡渲染 + 删除可用）。**DragValue 参数断言**：字段 seed（直接改 builder 卡片参数值）→ 断言渲染文本 + build filter AST（**不写 kittest drag 手势——无先例**）。MUST NOT: 不删既有测试凑数（迁移而非删除）；不用 slot.receiver try_recv（新惯例不引入）；不写 drag 手势；不测引擎行为（Batch 3）。
  Parallelization: Wave 4 | Blocked by: 3,4 | Blocks: 6
  References (executor has NO interview context - be exhaustive): crates/compass/src/citizens/screener.rs L632-1110（既有测试全量——迁移对象）; crates/compass/src/backend.rs L604/L654（两处 RunScreenerRequest 测试构造点——契约变更后必须同步）; crates/compass/src/citizens/ui_fixes_218.rs L39（LANG_LOCK）+ L149-153（sized_harness）; crates/compass-ui/src/widgets/dropdown.rs L231-241（dropdown 交互：点 trigger contains → 点选项 exact label）; crates/compass-ui/src/widgets/multi_select.rs L261-300（弹层交互：trigger 打开 → 选项累积 → 完成/Escape 关闭）; crates/compass-ui/src/widgets/checkbox.rs L85-94（checkbox 点 label 切换）; crates/compass-ui/src/widgets/icon_button.rs L96-105（glyph 查找 "\u{E20C}" 模式）；main.rs L2336-2341（query_all_by_label 多匹配 remove(0)）; .omo/designs/llm-screener-ui.md §8（测试锚点：id 约定 cond_{路径}_{字段} + label 锚点 + 交互路径清单）
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿（含迁移后 GROUP_ALIGNMENT 5 个 + 新增交互路径 8 个 + 纯函数断言 + backend.rs 迁移后测试）；`cargo test -p compass-i18n` 全绿（对称）；无测试删除（git diff 验证既有测试数不减）
  QA scenarios: happy: `cargo test -p compass` 交互路径全绿; failure: 交互路径中任一断言（如 AND-OR 切换后 AST operator 未翻转）失败即 RED 证明测试有效; Evidence .omo/evidence/task-5-llm-screener-ui.txt
  Commit: Y | `test(screener): builder interaction + migration tests` + 独立成行 `ref #245`

- [ ] 6. doc-sync + 决策记录
  What to do / Must NOT do: 按 AGENTS.md 变更类型 → kb/ 映射表更新：① `kb/design/ui.md`——新增「条件构建器」章节（最终设计要点：视图模型双向映射、卡片组布局、就地编辑交互、id_salt 约定、默认 6 卡（含 restore 默认空形状特判）、MultiSelect 状态归属、legacy 保存边界）+ **决策记录章节补齐**（已有 ## 决策记录 L245；追加本批决策：视图模型入 compass 业务层不入 compass-ui、Filter↔卡片双向映射、Frame 子组容器非 Card-in-Card、就地编辑非向导、无确认删除、添加菜单嵌套非拖拽、未知形状只读摘要卡、RunScreenerRequest 携 Filter、**legacy 保存复用引擎 filter_to_query**、screener.builder.* 命名空间、测试双层策略、动画克制、Count 卡延后、取反提供、**单成员折叠与裸节点形状对齐**）。② `kb/user/gui.md`——选股器章节更新（条件卡片组 UI 描述、AND/OR 嵌套、添加/删除/清空操作）。③ `kb/dev/testing.md`——如测试惯例有变化（on_save 捕获断言、迁移的 GROUP_ALIGNMENT 5 个）则补充。④ AGENTS.md 仅索引——如无项目级约定变化不改。MUST NOT: 不改 .omo/designs/llm-screener-ui.md（过程归档，不删不改）；不硬编码版本号；AGENTS.md 只做索引一句话。
  Parallelization: Wave 5 | Blocked by: 1,2,3,4,5 | Blocks: F-wave
  References (executor has NO interview context - be exhaustive): AGENTS.md（变更类型 → kb/ 映射表 + 决策记录规范）; kb/design/ui.md L245（既有决策记录章节）+ L14-149（设计系统/布局/交互规范）; kb/user/gui.md（选股器章节现状）; kb/dev/testing.md L281-340（kittest 章节）; .omo/designs/llm-screener-ui.md（决策来源，12 条决策记录）
  Acceptance criteria (agent-executable): grep 确认 `kb/design/ui.md` 含「条件构建器」章节 + 决策记录表格含本批至少 10 项决策（视图模型归属/双向映射/Frame 容器/就地编辑/无确认删除/消息契约/legacy 保存复用引擎/单成员折叠对齐/i18n 命名空间/Count 延后）；`kb/user/gui.md` 含条件卡片组描述；`.omo/designs/llm-screener-ui.md` 未被修改
  QA scenarios: happy: grep 决策记录章节 + 章节关键词通过; failure: 缺任一决策行或章节关键词则 grep 失败; Evidence .omo/evidence/task-6-llm-screener-ui.txt
  Commit: Y | `docs: screener builder design + decision records` + 独立成行 `ref #245`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit: 逐条核对 6 个 todo 的 Acceptance criteria 证据落盘 `.omo/evidence/task-{1..6}-llm-screener-ui.txt`；`git log` 确认每个 commit 含独立成行 `ref #245`；门禁 3.5 步对抗性测试（skwy-adversarial-test）与门禁 4 步需求测试（skwy-requirement-test）两批 RED 测试的委派记录存在
- [ ] F2. Code quality review: `cargo clippy --workspace` 无新警告；`cargo fmt --check` 通过；新增 pub 项全部带 `///` 文档注释（missing_docs 规范）；无 `as` 转换/unwrap 滥用；`cargo test --workspace` 全绿（含 compass-i18n 对称 + compass-strategy 既有 21 个 screener 语义测试断言不变——filter_to_query 仅改 pub 不改变行为）
- [ ] F3. Real manual QA: `cargo test --workspace` 全绿；`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json` 退出 0（compass 90% 门槛）；GUI 冒烟：`cargo run --bin compass` 手动验证卡片组渲染不 panic（如无显示环境则跳过并在 evidence 注明）
- [ ] F4. Scope fidelity: 核对 Must NOT have——无引擎求值改动（`git diff` 验证 compass-strategy 仅 filter_to_query pub 化/包装，run_screener/screen_symbol 无逻辑变更）、无 Count 卡、无 AST JSON 持久化、无 ScreenerQuery 删除、无拖拽、无新组件、无新 crate/依赖、无 slot.receiver try_recv 惯例引入、无自建压缩逻辑

## Commit strategy
- 每个 commit 独立成行 `ref #245`（hook 校验，指向 OPEN issue）；epic 子 issue 引用
- 顺序：1→2→3→4→5→6，每完成一个 todo 一个 commit（Wave 1 的 1/2 可并行但 commit 分开）
- Commit → Review：每次 commit 后运行 `/review-work`（goal/quality/security/QA/context 5 并行），发现问题最多 2 轮修复
- 禁止 auto-push：用户明确说 "push" 才 push；push 前 `git fetch origin master && git rebase origin/master`
- push 前写反思（/skwy-reflect），反思 commit 随 PR 同批推送
- push 后追加完成 comment（`gh issue comment 245`）+ 关闭 issue #245；PR 创建参考 epic #243

## Success criteria
- [ ] 条件卡片组 UI 落地（AND/OR 嵌套至少 2 层）：根组 Card + 递归子组渲染（Todo 3 验收）
- [ ] 操作的是 Batch 1 的 Filter AST：视图模型双向映射 round-trip（多成员结构等价 + 裸单节点 leaf 级等价）（Todo 1 验收）
- [ ] 现有基础条件功能不回归：默认 6 卡（含 restore 默认空形状特判）+ 全部 11 类模板可表达 + 既有测试迁移不断言变弱（Todo 3/5 验收）
- [ ] egui_kittest 无头测试覆盖：添加/删除/AND-OR 切换/清空/嵌套/restore/取反/未知形状（Todo 5 验收）
- [ ] i18n `screener.builder.*` 键 zh/en 对称 + 对称测试全绿（Todo 2 验收）
- [ ] `RunScreenerRequest` 携 Filter、backend 去 From 中转、legacy 保存复用引擎 filter_to_query + toast、UnsupportedFilter 前缀匹配提示（Todo 3/4 验收）
- [ ] kb/design/ui.md 设计章节 + 决策记录补齐；kb/user/gui.md 更新（Todo 6 验收）
- [ ] 全部 commit 引用 `ref #245`，push 后 issue 收尾（comment + close）
