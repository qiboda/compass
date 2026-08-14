# llm-screener - Work Plan

## TL;DR (For humans)

**What you'll get:** 选股器表达式 AST 类型系统——一组 Rust 枚举类型（Filter/MetaCond/SeriesFactor/SeriesCond），可 JSON 序列化、可用 `and`/`or`/`not` 与 `& | ~` 组合构造；旧版 11 类筛选条件可自动编译成新 AST；新增 3 个序列函数（连续上涨、窗口计数、放量）；`run_screener` 改收新 AST（旧 GUI 行为不变）。

**Why this approach:** enum+serde 让 config 持久化与未来 LLM 输出（Batch 4）共用同一格式；旧查询单向编译保证现有 GUI 零迁移、测试不回归；覆盖率门槛同步提到 95% 维持品质底线。

**What it will NOT do:** 不做 GUI 条件构建器（Batch 2）、不做引擎求值器（Batch 3）、不做 LLM 客户端（Batch 4）、不删旧 ScreenerQuery、不开新 crate、不新增第三方依赖。

**Effort:** Medium（7 个实现任务，涉及 2 个 crate + 覆盖率配置）
**Risk:** Low - 纯类型 + 兼容层，引擎逻辑不动；唯一风险点是 AST 形状修订（FactorRef）已被确认
**Decisions to sanity-check:** C1（Cmp.value 改 FactorRef）、C2（BullishAlign 按引擎语义修正）、C3（仅 3 个序列函数）、覆盖率 95%、M4（run_screener 受限反向转换而非通用求值器）

Your next move: approve the plan, then run `$start-work llm-screener` in a worker session (with `--worktree` for PR work).

---

> TL;DR (machine): Medium effort, Low risk, 7 implementation todos + 4 final-verification tasks; AST types in compass-types + series functions & run_screener adaptation in compass-strategy; compass-types coverage threshold raised to 95%.

## Scope
### Must have
- `compass-types`：AST 类型系统 —— `Filter`（Meta/Series/And/Or/Not）、`MetaCond`（Industry/Exchange/Board/ListYears/Delisted/MarketCap）、`SeriesFactor`（Close/Sma/ChangePct/DayPct/AvgVolume/NDayHigh）、`SeriesCond`（Cmp/UpDays/Count/VolumeSurge）、`CmpOp`（Eq/Ne/Gt/Ge/Lt/Le）、`FactorRef`（Const(f64)/Factor(SeriesFactor)）+ serde（Serialize/Deserialize）+ `and`/`or`/`not` 方法 + `&` `|` `~` 运算符重载（std::ops::BitAnd/BitOr/Not）
- `compass-types`：`From<ScreenerQuery> for Filter` 编译层，覆盖全部 11 类现有条件（industries/exchanges/boards/list_years/market_cap_min/max/exclude_delisted/ma/breakout/momentum/volume），映射语义按引擎实现（lib.rs:102-275），BullishAlign = `And(Cmp{Sma(5),Gt,Sma(20)}, Cmp{Sma(20),Gt,Sma(60)})`
- `compass-strategy`：序列函数库 `up_days`/`count`/`volume_surge`（仅 3 个，遵循 sepa/indicators.rs 契约：Option 返回、窗口保护、NaN→None、不 panic）
- `compass-strategy`：`run_screener` 接口收 `Filter`（`&Filter` 参数），内部受限反向转换 Filter→ScreenerQuery 走既有 screen_symbol 逻辑；新增 `ScreenerError::UnsupportedFilter` 变体；GUI 调用点 backend.rs:149 改为 `Filter::from(query)` 先编译
- 测试：JSON round-trip（serde_json，新增 dev-dep）、运算符构造、11 类映射、序列函数窗口边界、run_screener 兼容
- 覆盖率：compass-types 门槛 80%→95%（check-coverage.sh:28 + ci.yml:49 文案 + AGENTS.md Testing 节 + kb/dev/testing.md 阈值表），全 crate（含既有 SEPA 类型）补测至 95%
- doc-sync：`kb/design/architecture.md`（AST 章节）或 data-providers.md、`kb/dev/testing.md`（覆盖率表）、`AGENTS.md`（Testing 节）、决策记录 `## 决策记录` 章节补齐

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不做 GUI 条件构建器（Batch 2 #245）
- 不做引擎编译 + 序列条件执行（Batch 3 #246）——run_screener 的 Filter 求值仅走受限反向转换兼容路径，不实现通用 Filter 求值器
- 不做 LLM 客户端（Batch 4 #247）
- 不删除/迁移旧 `ScreenerQuery`（Batch 3 才迁移）；不改 GUI 现有调用语义（仅编译先行）
- 不单开新 crate；不新增第三方依赖（除 serde_json dev-dep）
- 不实现 Sma/ChangePct/NDayHigh 独立序列函数（复用既有私有 helper lib.rs:221-259）
- 不重构 screen_symbol / 既有引擎逻辑（仅在其上叠加 Filter 入口）
- 不改动 ScreenerQuery 现有 serde 契约（兼容层单向编译）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（先写失败测试 RED → 实现 GREEN）+ rstest/tokio::test（既有模式，见 kb/dev/testing.md）；JSON 用 serde_json（新增 compass-types dev-dep）
- Evidence: `.omo/evidence/<task>-llm-screener.txt`（attemptDir = `.omo/evidence/`）
- 覆盖率：`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json`（per-crate 阈值表）
- 构建：`cargo build` / `cargo clippy`（mold 链接器已配置）

## Execution strategy
### Parallel execution waves
- **Wave 1**（Todo 1-2）：compass-types AST 类型 + 运算符重载（纯类型，无跨 crate 依赖）
- **Wave 2**（Todo 3）：From<ScreenerQuery> for Filter 编译层（依赖 Wave 1 类型）
- **Wave 3**（Todo 4）：compass-strategy 序列函数库（依赖 compass-core 模型，与 Wave 1/2/4 无依赖，可并行）
- **Wave 4**（Todo 5）：run_screener 收 Filter（反向转换本体依赖 Wave 1 类型；GUI 调用点与测试迁移需 Todo 3 的 From 编译层，Wave 2 完成后即可）
- **Wave 5**（Todo 6-7）：覆盖率 95% + doc-sync（依赖全部实现）

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. AST 类型 + serde | — | 2,3,5 | — |
| 2. 运算符重载 | 1 | 3,5 | 4（若 Wave 3 先行） |
| 3. From<ScreenerQuery> | 1,2 | 5 | 4 |
| 4. 序列函数库 | — | — | 1,2,3,5 |
| 5. run_screener 收 Filter | 1,3 | 6,7 | 4 |
| 6. 覆盖率 95% | 1,2,3,4,5 | 7 | — |
| 7. doc-sync + 决策记录 | 1,2,3,4,5,6 | F-wave | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. compass-types: 定义 AST 类型系统（Filter/MetaCond/SeriesFactor/SeriesCond/CmpOp/FactorRef）+ serde derives + Default
  What to do / Must NOT do: 在 crates/compass-types/src/ 下新增模块（如 `screener.rs`，lib.rs 重导出 pub use）。按 handoff.md:19-53 锁定形状定义 enum；`Cmp.value` 类型为 `FactorRef`（`Const(f64)`/`Factor(SeriesFactor)`，C1 修订）；`CmpOp` 定义 Eq/Ne/Gt/Ge/Lt/Le（serde rename_all = "snake_case"）；全部 derive Debug/Clone/PartialEq/Serialize/Deserialize。**Default 决策（已锁定）：仅 `MetaCond` 与 `SeriesFactor` 实现 Default（MetaCond::default = Industry(vec![])，SeriesFactor::default = Close）；`Filter` 与 `SeriesCond` 不实现 Default**——空查询语义由 From 编译层（Todo 3）用 `And(vec![])` 表达，Filter 本身无默认值。Cargo.toml 增加 `serde_json = { workspace = true }` dev-dep（根 Cargo.toml:30 已有 workspace 定义）。MUST NOT: 不新增其他依赖；不改 ScreenerQuery 既有定义；不写引擎逻辑。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2,3,5
  References (executor has NO interview context - be exhaustive): .omo/handoff.md:19-53（锁定 AST 形状 + 映射表）; crates/compass-types/src/lib.rs:10-21（MaCondition serde 模式：rename_all snake_case）; crates/compass-types/src/lib.rs:137-195（ScreenerQuery + Default 模式）; crates/compass-types/Cargo.toml:7-12（现有依赖，需加 serde_json dev-dep）; Cargo.toml:29-30（workspace serde/serde_json）; crates/compass-strategy/src/sepa/indicators.rs:1-7（纯函数契约风格）
  Acceptance criteria (agent-executable): `cargo check -p compass-types` 通过；`cargo test -p compass-types` 新增 JSON round-trip 测试通过：`serde_json::from_str::<Filter>(&serde_json::to_string(&filter)?)` 与 `filter == decoded` 对每个变体（Meta 6 变体、Series 4 变体、And/Or/Not 嵌套）成立；**Cmp 需同时覆盖 FactorRef 两种形式：`Const(f64)`（momentum 形状）与 `Factor(SeriesFactor)`（MA/突破形状）**
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-types` roundtrip 全变体通过; failure: 手写非法 JSON（未知 tag、缺字段）`serde_json::from_str` 返回 Err（拒绝非法输入）; Evidence .omo/evidence/task-1-llm-screener.txt
  Commit: Y | `feat(types): screener AST types with serde support` + 独立成行 `ref #244`（hook 校验，勿行内）

- [ ] 2. compass-types: 实现 and/or/not 方法与 `&` `|` `~` 运算符重载 + 构造测试
  What to do / Must NOT do: 在 AST 类型（Filter）上实现：`Filter::and(self, other)` / `or` / `not` 方法 + `impl std::ops::BitAnd/BitOr/Not for Filter`（`&`→And、`|`→Or、`~`→Not）。语义：`a.and(b)` = `Filter::And(vec![a, b])`，`a.or(b)` = `Filter::Or(vec![a, b])`，`a.not()` = `Filter::Not(Box::new(a))`；运算符委托方法实现。构造测试覆盖嵌套组合（`(a | b) & ~c`）与求值结构断言。MUST NOT: 不改 Filter 变体定义（仅加 impl）；不实现比较运算符（比较生成 Filter 属 Batch 3 引擎求值范畴——本次仅布尔组合）。
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 3,5
  References: handoff.md:57（锁定决策 1：运算符重载 and/or/not 提供 Zipline 式构造体验）; handoff.md:83（验收：运算符重载构造测试）; crates/compass-types/src/lib.rs（既有类型风格参考）; 无既有 trait 可参考（两 crate 零 trait，全新定义）
  Acceptance criteria (agent-executable): `cargo test -p compass-types` 新增运算符测试通过：断言 `(a & b)` 结构为 `Filter::And(vec![a, b])`、`(a | b | c)` 结构、`~a` 结构、`a.and(b).not()` 嵌套结构
  QA scenarios: happy: `cargo test -p compass-types` 运算符构造断言全绿; failure: 组合后结构断言不匹配则测试失败（编译器验证结构精确性）; Evidence .omo/evidence/task-2-llm-screener.txt
  Commit: Y | `feat(types): Filter boolean operators (and/or/not + &|~)` + 独立成行 `ref #244`

- [ ] 3. compass-types: 实现 `From<ScreenerQuery> for Filter` 编译层，覆盖全部 11 类条件
  What to do / Must NOT do: 按 handoff.md:63-76 映射表实现。关键映射（C1/C2 修订后）：industries/exchanges/boards → `Meta(Industry/Exchange/Board(Vec))`；list_years → `Meta(ListYears(n))`；market_cap_min/max → `Meta(MarketCap{min,max})`；**exclude_delisted 双向语义（已锁定）：`exclude_delisted: true` → `Meta(Delisted(false))`；`exclude_delisted: false` → 不产出 Delisted 节点（缺失即"不排除退市"）；反向转换（Todo 5）按"Delisted(false) 存在 → true、缺失 → false"还原——与 ScreenerQuery::default（true）的差异由 From 在 emit 时显式补齐**；MA AboveMa20/60 → `Series(Cmp{Close, Gt, FactorRef::Factor(Sma(20/60))})`；MA BullishAlign → `And(Cmp{Sma(5),Gt,Sma(20)}, Cmp{Sma(20),Gt,Sma(60)})`（**按引擎语义修正，非 handoff 表原文 Close>Sma20**，见 lib.rs:233-238）；breakout → `Series(Cmp{Close, Gt, FactorRef::Factor(NDayHigh(days))})`；momentum → `And(Cmp{ChangePct(days), Ge, Const(min)}, Cmp{ChangePct(days), Le, Const(max)})`（双边界恒发射）；volume → `Series(VolumeSurge{days, times})`。多条件 AND 组合：`Filter::And(vec![...])` 按 ScreenerQuery 各字段条件排序合并；**组合含 BullishAlign/momentum 时产生嵌套 And（And 内套 And）——这是 From 的合法产出形状，反向转换必须支持（Todo 5）**；空查询（全 None/空 + exclude_delisted=false）→ 空 `And(vec![])`。反向不实现（单向编译）。测试：11 类条件逐一构造 ScreenerQuery → 断言 Filter 结构精确相等。MUST NOT: 不实现 Filter→ScreenerQuery 反向；不改既有条件类型定义；不依赖引擎代码。
  Parallelization: Wave 2 | Blocked by: 1,2 | Blocks: 5
  References: handoff.md:63-76（11 类映射表，含 C2 修正）; handoff.md:60（锁定决策 4：单向编译 From）; crates/compass-strategy/src/lib.rs:102-275（screen_symbol 引擎语义：ma lib.rs:221-240、breakout lib.rs:244-252、momentum lib.rs:255-259、volume lib.rs:263-275）; crates/compass-types/src/lib.rs:137-195（ScreenerQuery 11 字段定义）
  Acceptance criteria (agent-executable): `cargo test -p compass-types` 新增映射测试通过：11 类条件各构造一个 ScreenerQuery 实例，断言 `Filter::from(query)` 结构精确匹配预期 AST；综合全条件查询（含 BullishAlign + momentum 组合）断言嵌套 And 组合结构
  QA scenarios: happy: `cargo test -p compass-types` 11 类映射断言全绿; failure: 空查询（default，exclude_delisted=true）不 panic 且产出 `Meta(Delisted(false))`；exclude_delisted=false 空查询产出 `And(vec![])`; Evidence .omo/evidence/task-3-llm-screener.txt
  Commit: Y | `feat(types): From<ScreenerQuery> for Filter covering all 11 conditions` + 独立成行 `ref #244`

- [ ] 4. compass-strategy: 实现序列函数库 up_days/count/volume_surge + 窗口边界测试
  What to do / Must NOT do: 在 crates/compass-strategy/src/ 新增模块（如 `screener_series.rs` 或 sepa 平级）实现 3 个纯函数（C3 决策：仅 3 个，不做 sma/change_pct/n_day_high 独立函数）：`pub fn up_days(series: &[&CrossSectionBar], n: u32, min_pct: f64) -> Option<bool>`（连续 n 天每日涨幅 > min_pct，需 n+1 根 bar 窗口，窗口不足返回 None；**n=0 语义：返回 Some(true)（空条件恒真）**）；`pub fn count_in_window(series: &[&CrossSectionBar], window: u32, pred: impl Fn(&CrossSectionBar) -> bool) -> Option<usize>`（最近 window 天内满足 pred 的天数，窗口不足返回 None；**无 at_least 参数——Batch 3 引入谓词组合时再按需添加**）；`pub fn volume_surge(series: &[&CrossSectionBar], days: u32, times: f64) -> Option<bool>`（匹配引擎语义 lib.rs:263-275：recent 窗口 avg ≥ times × 3×N 基线 avg，days=0 或窗口不足返回 None）。全部遵循 sepa/indicators.rs:1-7 契约：窗口不足 → None、NaN → None、不 panic、零除防护。测试覆盖窗口边界（n=0、窗口不足、恰好 n+1、除零基线、NaN 输入）。MUST NOT: 不实现 sma/change_pct/n_day_high 独立函数；不改既有 lib.rs 私有 helper；不接入 run_screener（仅纯函数）。
  Parallelization: Wave 3 | Blocked by: — | Blocks: — | Can parallelize with: 1,2,3,5
  References: handoff.md:49-52（SeriesCond 定义：UpDays/Count/VolumeSurge）; crates/compass-strategy/src/sepa/indicators.rs:1-7（纯函数契约：不 panic、不 NaN）; crates/compass-strategy/src/sepa/indicators.rs:60-92（momentum_return/volume_ratio 实现模式）; crates/compass-strategy/src/lib.rs:263-275（matches_volume 引擎语义：3×N 基线）; crates/compass-strategy/src/lib.rs:221-259（ma/momentum 既有私有 helper 参考）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy` 新增序列函数测试通过：每函数 happy path + 窗口边界（n=0 / 窗口不足 / 恰好 n+1 / 除零 / NaN）断言
  QA scenarios: happy: `cargo test -p compass-strategy` 窗口边界矩阵全绿; failure: 窗口不足返回 None 不 panic; Evidence .omo/evidence/task-4-llm-screener.txt
  Commit: Y | `feat(strategy): screener series functions up_days/count/volume_surge` + 独立成行 `ref #244`

- [ ] 5. compass-strategy: run_screener 接口收 `&Filter`（受限反向转换 + ScreenerError::UnsupportedFilter + 既有测试迁移）
  What to do / Must NOT do: 修改 crates/compass-strategy/src/lib.rs:46-50 run_screener 签名：`pub fn run_screener(filter: &Filter, reader: &ParquetReader, now: NaiveDate) -> Result<ScreenerResult, ScreenerError>`；内部将 Filter 受限反向转换为 ScreenerQuery（私有转换函数），走既有 screen_symbol 逻辑（lib.rs:102-275 不动）。**反向转换显式 accept-grammar（只有这些形状被接受，其余一律 UnsupportedFilter）**：① Meta 各变体（Industry/Exchange/Board/ListYears/MarketCap；**Delisted(false) 接受 → exclude_delisted=true；Delisted(true) → UnsupportedFilter**，因为 From 永不产出且 ScreenerQuery 无法表达"仅退市"）；② `Series(Cmp{Close, Gt, FactorRef::Factor(Sma(20|60))})` → ma AboveMa20/60；③ `Series(Cmp{Close, Gt, FactorRef::Factor(NDayHigh(days))})` → breakout；④ momentum 双边界形状 `And(Cmp{ChangePct(d), Ge, Const(min)}, Cmp{ChangePct(d), Le, Const(max)})`（**两个 Cmp 的 days 必须相等**，否则 UnsupportedFilter）→ momentum；⑤ `Series(VolumeSurge{days, times})` → volume；⑥ BullishAlign 双节点形状 `And(Cmp{Sma(5),Gt,Factor(Sma(20))}, Cmp{Sma(20),Gt,Factor(Sma(60))})` → ma BullishAlign；⑦ `And(vec![])` → 全空 ScreenerQuery（exclude_delisted=false）；⑧ 上述节点的任意 And 组合（含 ④⑥ 的 And 内套 And——From 对组合查询的合法产出）。**UnsupportedFilter 触发（From 不可能产出的形状）**：UpDays/Count 谓词、Not 节点、任何 Or 节点、`Cmp{Close,Gt,Const(_)}` 等 Const 值比较（momentum 双边界 ④ 除外）、孤立 Sma(5)/Sma(20) 左操作数（仅 ⑥ 的成对形状可接受）、顶层 `Cmp{ChangePct,Ge,Const}` 单节点（momentum 必须成对）。exclude_delisted 还原：Delisted(false) 存在 → true，缺失 → false。GUI 调用点 crates/compass/src/backend.rs:149 改为 `run_screener(&Filter::from(req.query.clone()), ...)`（From 在 compass-types）。迁移既有 tests/screener.rs 21 处测试（23 处调用：159,184,192,213,250,278,309,341,379,406,425,442,458,473,489,497,530,547,564,592,614,623,672）为 `&Filter::from(query.clone())` 或引用共享转换 helper。新增兼容测试：既有 21 个语义测试（tests/screener.rs:143-672）在 Filter 入口下保持相同断言。MUST NOT: 不改 screen_symbol 引擎逻辑；不实现通用 Filter 求值器（Batch 3）；不改 ScreenerQuery serde 契约；不引入反向 From<&Filter> for ScreenerQuery 的 public 类型（用私有转换函数）。
  Parallelization: Wave 4 | Blocked by: 1,3 | Blocks: 6,7
  References: crates/compass-strategy/src/lib.rs:22-27（ScreenerError）; crates/compass-strategy/src/lib.rs:46-50（run_screener 签名）; crates/compass-strategy/src/lib.rs:102-275（screen_symbol 引擎）; crates/compass/src/backend.rs:149（GUI 唯一调用点）; crates/compass-strategy/tests/screener.rs:143-672（21 个语义测试，23 处调用点）; crates/compass/src/messages.rs:30-34（RunScreenerRequest 携带 ScreenerQuery）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy --test screener` 全绿（既有 21 测试在 Filter 入口下断言不变，含 momentum_filter_bounds 双边界、exclude_delisted 双向）; `cargo check -p compass` 通过（backend.rs 编译）; 新增 UnsupportedFilter 测试：传入 UpDays 谓词 Filter、Not 节点、Delisted(true)、`Cmp{Close,Gt,Const}` → 均返回 Err(ScreenerError::UnsupportedFilter); **新增嵌套组合 round-trip 测试：industries + BullishAlign + momentum 三条件 ScreenerQuery → `Filter::from` → run_screener，断言与直接 ScreenerQuery 入口结果一致（验证 And 内套 And 的元素分类正确）**
  QA scenarios: happy: `cargo test -p compass-strategy --test screener` 全绿; failure: 不识别 Filter 形状返回 UnsupportedFilter 错误而非静默错误结果; Evidence .omo/evidence/task-5-llm-screener.txt
  Commit: Y | `feat(strategy): run_screener accepts Filter via restricted conversion` + 独立成行 `ref #244`

- [ ] 6. 覆盖率：compass-types 门槛 80%→95% + 全 crate 补测至 95%
  What to do / Must NOT do: 修改 scripts/check-coverage.sh:28 `[compass-types]=80` → `95`；.github/workflows/ci.yml:49 step 文案 "data/core 95%, others 80%" 更新为含 compass-types；AGENTS.md Testing 节门槛表（compass-types 80%→95%）；kb/dev/testing.md:252-253,266-268 阈值表同步。运行 `cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json` 测基线；compass-types 未达 95% 处补测（既有 SEPA 类型 SepaQuery/SepaRow/SepaDetails 等 lib.rs:222-345 + 新增 AST 类型 + **Default 实现断言：`assert_eq!(MetaCond::default(), Industry(vec![]))`、`assert_eq!(SeriesFactor::default(), Close)`——确保 Default 一行不落空**），测试补在 compass-types/src/lib.rs `#[cfg(test)] mod tests`（既有模式 lib.rs:347-455）。MUST NOT: 不降其他 crate 门槛；不为凑覆盖率删既有测试；不用 `#[cfg_attr]`/`#[allow(dead_code)]` 等规避手段（用真实构造测试覆盖）。
  Parallelization: Wave 5 | Blocked by: 1,2,3,4,5 | Blocks: 7
  References: scripts/check-coverage.sh:21-30（阈值表）; scripts/check-coverage.sh:82（per-crate filter）; .github/workflows/ci.yml:49-50（CI 步骤）; AGENTS.md Testing 节; kb/dev/testing.md:251-268（覆盖率章节）; crates/compass-types/src/lib.rs:222-345（SEPA 类型，需补测）; crates/compass-types/src/lib.rs:347-455（既有测试模式）
  Acceptance criteria (agent-executable): `cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json` 退出码 0，输出含 `OK: compass-types line coverage` 且百分比 ≥95（脚本输出格式为 "OK: compass-types line coverage <pct>%"）
  QA scenarios: happy: 覆盖率脚本退出 0; failure: 门槛改后未达 95% 则脚本输出 FAIL 且退出非 0（证明门槛生效）; Evidence .omo/evidence/task-6-llm-screener.txt
  Commit: Y | `ci: raise compass-types coverage threshold to 95%` + 独立成行 `ref #244`

- [ ] 7. doc-sync + 决策记录
  What to do / Must NOT do: 按变更类型 → kb/ 映射表（AGENTS.md）更新：`kb/design/architecture.md` 或 `kb/design/data-providers.md` 增加 screener AST 章节（类型归属：AST→compass-types、序列函数→compass-strategy、run_screener 收 Filter）；`kb/design/` 相关文件补 `## 决策记录` 章节（what + why + why-not 表格，含 C1 FactorRef、C2 BullishAlign 修正、C3 三函数范围、M4 受限转换、覆盖率 95%、exclude_delisted 缺失语义 六项决策）；`kb/user/config.md` 若 screener 持久化格式变化则更新（AST 尚未持久化到 config，Batch 3 才做——仅注明现状）；AGENTS.md 若需反映新类型归属。MUST NOT: 不写 GUI 相关文档（Batch 2）；不写 LLM 文档（Batch 4）。
  Parallelization: Wave 5 | Blocked by: 6 | Blocks: F-wave
  References: AGENTS.md（kb/ 映射表）; kb/design/architecture.md:52-56,98（crate 布局）; kb/design/data-providers.md（数据层文档）; kb/dev/testing.md:251-268（覆盖率表，Todo 6 已改）; .omo/handoff.md（决策来源）
  Acceptance criteria (agent-executable): grep 确认 kb/design/ 相关文件含 `## 决策记录` 表格章节且含 C1/C2/C3/M4/覆盖率/exclude_delisted 六项决策；architecture.md 或 data-providers.md 含 "Filter"/"SeriesCond" AST 类型说明
  QA scenarios: happy: grep 决策记录章节与 AST 类型提及通过; failure: 缺任一决策行或 AST 类型提及则 grep 失败; Evidence .omo/evidence/task-7-llm-screener.txt
  Commit: Y | `docs: screener AST design docs + decision records` + 独立成行 `ref #244`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit: 逐条核对 7 个 todo 的 Acceptance criteria 证据落盘 `.omo/evidence/task-{1..7}-llm-screener.txt`；`git log` 确认每个 commit 含独立成行 `ref #244`；覆盖率脚本退出 0
- [ ] F2. Code quality review: `cargo clippy --workspace` 无新警告；`cargo fmt --check` 通过；新增 pub 项全部带 `///` 文档注释（missing_docs 规范）；无 `as any`/unwrap 滥用（Rust 侧：无 panic 路径，序列函数遵循 None 契约）
- [ ] F3. Real manual QA: `cargo test --workspace` 全绿（含既有 21 个 screener 语义测试在 Filter 入口下断言不变 + 新增 AST/映射/序列函数测试）；`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json` 输出含 `OK: compass-types line coverage` 且百分比 ≥95
- [ ] F4. Scope fidelity: 核对 Must NOT have 清单——无 GUI 构建器改动、无引擎求值器、无 ScreenerQuery 删除、无新 crate/第三方依赖（除 serde_json dev-dep）、无 sma/change_pct/n_day_high 独立函数、screen_symbol 引擎逻辑未动（`git diff` 验证 lib.rs:102-275 无逻辑变更）

## Commit strategy
- 每个 commit 独立成行 `ref #244`（hook 校验，指向 OPEN issue）；epic 批量子 issue 引用
- 顺序：1→2→3→4→5→6→7，每完成一个 todo 一个 commit
- Commit → Review：每次 commit 后运行 `/review-work`（goal/quality/security/QA/context 5 并行），发现问题最多 2 轮修复
- 禁止 auto-push：用户明确说 "push" 才 push；push 前 `git fetch origin master && git rebase origin/master`
- push 前写反思（/skwy-reflect），反思 commit 随 PR 同批推送
- push 后追加完成 comment（`gh issue comment 244`）+ 关闭 issue #244；PR 创建参考 epic #243

## Success criteria
- [ ] enum AST（Filter/MetaCond/SeriesFactor/SeriesCond/CmpOp/FactorRef）+ serde JSON round-trip 测试全绿（Todo 1 验收）
- [ ] `From<ScreenerQuery> for Filter` 覆盖全部 11 类现有条件，映射语义与引擎一致（Todo 3 验收，含 C1/C2 修订）
- [ ] UpDays/Count/VolumeSurge 序列函数实现 + 窗口边界单元测试（Todo 4 验收）
- [ ] 运算符重载（and/or/not + `&|~`）构造测试全绿（Todo 2 验收）
- [ ] run_screener 接口收 `&Filter`，GUI 调用点编译先行，既有 21 个语义测试断言不变（Todo 5 验收）
- [ ] compass-types 覆盖率 ≥95%（CI 门槛提升生效，Todo 6 验收）
- [ ] kb/ 文档同步 + `## 决策记录` 章节（Todo 7 验收）
- [ ] 全部 commit 引用 `ref #244`，push 后 issue 收尾（comment + close）
