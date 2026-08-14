# llm-screener-engine - Work Plan

## TL;DR (For humans)

**What you'll get:** 选股器的 Filter AST 从"受限反向转换"升级为**通用递归求值器**——之前运行时报「引擎暂不支持」的连续上涨、窗口计数、或组、取反等条件现在真正可执行；筛选条件以 Filter JSON 格式持久化到配置文件（旧配置仍可读）。

**Why this approach:** 引擎直接消费与 GUI/未来 LLM（Batch 4）共享的同一 AST 格式，消灭"两套类型 + 受限文法"的中间层；持久化同步迁移到 AST，Or/Not/连续上涨组合首次可保存。

**What it will NOT do:** 不做 LLM 客户端（#247）、不改 GUI 构建器（#245）、不删 ScreenerQuery 旧类型（旧配置可读）、不新增第三方依赖、不改 21 个既有语义测试的引擎行为。

**Effort:** Medium（6 个实现任务，涉及 compass-strategy + compass 2 个 crate + bench）
**Risk:** Medium - 求值器必须逐位复刻既有 21 个语义测试（MA/突破/动量/放量/退市边界）；NDayHigh 语义歧义（前 N 根不含最新）已按引擎实现锁定
**Decisions to sanity-check:** D1（删除 filter_to_query 全套机制）、D3（持久化双格式 `filter` JSON key）、D5（Delisted(true) 求值支持）、NDayHigh 前 N 根语义、Count 每日滑动窗口求值

Your next move: 批准后执行（门禁 3.5/4 步测试委派 → 实现）。Full execution detail follows below.

---

> TL;DR (machine): Medium effort, Medium risk, 6 implementation todos + 4 final-verification tasks; general Filter AST evaluator in compass-strategy (new screener_eval.rs) replacing filter_to_query reverse-compile; Filter JSON persistence ([screener] filter key, dual-format load); criterion bench for parity evidence; filter_to_query/ScreenerError::UnsupportedFilter/unsupported_save i18n deleted; 21 legacy integration tests stay green as regression baseline.

## Scope
### Must have
- `compass-strategy`：通用 Filter AST 求值器 `evaluate(filter, basic, series, now) -> bool`（新模块 `screener_eval.rs`）——递归求值 Filter 树：MetaCond 基于 StockBasic（Industry/Exchange/Board/ListYears/Delisted/MarketCap），SeriesCond 基于 bars 序列（Cmp/UpDays/Count/VolumeSurge），And/Or/Not 布尔组合；窗口不足/NaN → 不匹配（false），不 panic、不 NaN、无新错误类型
- `compass-strategy`：`run_screener` 直接消费求值器（逐 symbol evaluate），删除 `filter_to_query`/`convert_filter`/`SeenFields`/`mark_seen`/`momentum_pair`/`bullish_pair` 反编译机制 + `ScreenerError::UnsupportedFilter` 变体（全部成为死代码）
- `compass-strategy`：既有 21 个语义集成测试（tests/screener.rs，已走 `&Filter::from(q)` 边界）在求值器下保持相同断言——回归基线；lib.rs L758-1125 的 UnsupportedFilter/roundtrip 测试块改写为求值器语义测试
- `compass`：持久化迁移——`[screener]` 节新增 `filter = "<Filter JSON>"` key（保存写新格式）；加载双解析（`filter` key 存在→JSON，否则 legacy 11 键→`Filter::from`，缺失→默认）；`FullConfig.screener` 类型改 `ScreenerSection`；`save_screener_config(&Filter)`；`ScreenerPanel::new(restore: Option<&Filter>, on_save: Box<dyn Fn(&Filter)>)`；GUI 保存 oracle（screener.rs:208 filter_to_query 调用）移除；`unsupported_save` i18n key（en.yml:128/zh.yml:126）删除；compass crate 加 `serde_json` workspace 依赖
- 测试：RED first——"连续 N 天每日涨幅 > X%"正确性、Count 窗口计数、Or/Not 求值、持久化 round-trip（新格式 + legacy 迁移读取）失败测试先行（门禁 3.5/4 步委派）；cargo test 全绿 + compass-strategy 覆盖率 ≥95%
- 性能：criterion bench（compass-strategy 新 benches/）合成 6000 标的 × 400 天数据跑 `run_screener(&Filter)`，与迁移前（filter_to_query 路径）对比 elapsed——同量级证据落盘
- doc-sync：`kb/design/architecture.md`（AST 章节 + 决策记录追加）、`kb/user/config.md`（[screener] 新格式）、`kb/user/gui.md` + `kb/design/ui.md`（"Batch 3 支持后自然消失"注释落地）、`kb/dev/testing.md`（如新增测试模式）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不做 LLM 客户端（Batch 4 #247）
- 不改 GUI 构建器（Batch 2 #245 的 screener_builder.rs 视图模型/filter_to_items 不动）
- 不删除 `ScreenerQuery`/`From<ScreenerQuery> for Filter`——旧配置可读、21 测试继续用 `Filter::from(q)` 构造
- 不新增第三方依赖（serde_json 是 workspace 已有；criterion 是 workspace 已有 dev-dep）
- 不改变既有 21 个语义测试的引擎行为（求值器必须复刻：MA 含最新 N 根、breakout 前 N 根不含最新、momentum 含 base、volume 3N 嵌套基线、delisted 默认排除、missing total_share + cap 条件 → 不匹配）
- 不改 READ_WINDOW_DAYS / 数据获取路径（fetch_cross_section/load_all_stock_basics 不动）
- 不实现额外新序列函数（只接线已有 up_days/count_in_window/volume_surge；Sma/ChangePct/NDayHigh/DayPct/AvgVolume 作为 factor 求值内联实现，不单独 pub）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD**（门禁 3.5/4 步 RED 失败测试先行 → 实现 GREEN → 独立 QA 复核）+ rstest/tokio::test（既有模式，见 kb/dev/testing.md）；求值器纯函数用 `#[cfg(test)]` 内嵌单测 + 集成测试走既有 build_fixture（tempdir + DuckDB → Parquet）
- Evidence: `.omo/evidence/task-<N>-llm-screener-engine.txt`（attemptDir = `.omo/evidence/`）
- 覆盖率：`cargo llvm-cov nextest --json --summary-only > cov.json && bash scripts/check-coverage.sh cov.json`（compass-strategy 阈值已 95%，不得跌破）
- 构建：`cargo check -p compass-strategy -p compass` / `cargo clippy --workspace`（mold 已配置）

## Execution strategy
### Parallel execution waves
> Target 3-6 todos per wave. Waves 1-2 can run after gate-3.5/4 RED tests land.
- **Wave 1**（Todo 1）：求值器核心 `screener_eval.rs`（纯逻辑，无跨 crate 依赖，仅依赖已存在的 screener_series.rs + compass-types AST + compass-core 模型）
- **Wave 2**（Todo 2, 3）：run_screener 接入求值器 + 删除反编译机制（lib.rs 改动）；持久化双格式（main.rs + ScreenerSection，独立于引擎侧，可并行）
- **Wave 3**（Todo 4, 5）：GUI 调用点迁移（依赖 Todo 3 的 save_screener_config(&Filter) 签名）+ criterion bench（依赖 Todo 2 完成后才有对比意义，可与 4 并行）
- **Wave 4**（Todo 6）：doc-sync + 决策记录（依赖全部实现）
- **F-wave**：4 个验证任务（依赖全部 todo）

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1. screener_eval.rs 求值器核心 | 门禁 RED 测试 | 2 | — |
| 2. run_screener 接入 + 删除反编译 | 1 | 5 | 3 |
| 3. 持久化双格式 | — | 4 | 2 |
| 4. GUI 调用点迁移 | 3 | 6 | 5 |
| 5. criterion bench | 2 | 6 | 4 |
| 6. doc-sync + 决策记录 | 1,2,3,4,5 | F-wave | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. compass-strategy: 通用 Filter AST 求值器 `screener_eval.rs`（evaluate 递归 + Meta/Series/布尔求值 + factor 求值）
  What to do / Must NOT do: 新建 `crates/compass-strategy/src/screener_eval.rs`，lib.rs 加 `pub mod screener_eval;`。实现 `pub fn evaluate(filter: &Filter, basic: &StockBasic, series: &[&CrossSectionBar], now: NaiveDate) -> bool`：
  - 递归分发：`Meta(meta)` → `evaluate_meta`；`Series(cond)` → `evaluate_series`；`And(v)` → 全真（短路）；`Or(v)` → 任一真（短路）；`Not(f)` → `!evaluate(f)`
  - `evaluate_meta`（复刻 screen_symbol lib.rs:391-455 语义，逐条对照）：
    - `Industry(v)` → `basic.industry.as_deref().is_some_and(|i| v.contains(i))`（空 v = 真）
    - `Exchange(v)` → `compass_core::data::symbol::exchange_of_symbol(&basic.symbol)` ∈ v（复用 lib.rs:406 的显式前缀 + 裸码回退，大小写不敏感由 parse_explicit_prefix 保证）；空 v = 真
    - `Board(v)` → `basic.board.as_deref().is_some_and(|b| v.contains(b))`；空 v = 真
    - `ListYears(n)` → `basic.list_date.is_some_and(|d| now - d >= Duration::days(n*365))`；list_date None → false（受约束时剔除，lib.rs:419-427）
    - `Delisted(false)` → `basic.delist_date.is_none()`；`Delisted(true)` → `basic.delist_date.is_some()`（**D5：Delisted(true) 现支持**，AST 全支持——From 层永不产出但求值器处理）
    - `MarketCap{min,max}` → market_cap = `basic.total_share * latest_close / 1e8`；**缺失 total_share 的剔除必须按 lib.rs:435-444 门控**：`if basic.total_share.is_none() && (min.is_some() || max.is_some()) → false`（cap 条件激活才剔除）；`total_share` None 且 min/max 均 None → market_cap 按 0.0 继续（通过边界检查，排序垫底）——**GUI 默认 6 卡片恒含 MarketCap{None,None}（screener_builder.rs 默认卡片），无条件剔除会静默丢弃缺失 share 的股票，破坏现状行为**；latest_close 取 `series.last()?.close`（**空序列用 `last()` Option 传播，不 panic——evaluate 是 pub API，lib.rs 的 run_screener 保证非空但 API 边界自身不 panic**）；min/max Option 边界（`>=`/`<=`，lib.rs:446-455）。**注意**：求值器此处只判条件，market_cap 计算逻辑与 run_screener 行组装共用同一公式，但求值器内自包含计算（不依赖外部传入的 cap）。**测试必含**：`MarketCap{min:None, max:None}` + 缺失 total_share → **匹配**（复刻 lib.rs:435-444 的 0.0 语义）；`MarketCap{min:Some, ..}` + 缺失 total_share → 不匹配
  - `evaluate_series`：
    - `Cmp{factor, op, value}` → `factor_value(factor) op value_value(value)`；factor 或 value 求值窗口不足/NaN → false（不匹配）。`op` 用 CmpOp 直接比较（Eq/Ne/Gt/Ge/Lt/Le）
    - `UpDays{n, min_pct}` → `screener_series::up_days(series, n, min_pct).unwrap_or(false)`（None→false）
    - `Count{factor, op, value, window, at_least}` → 在最近 `window` 根内逐日求值：**索引基循环（不要用 count_in_window 的 `Fn(&CrossSectionBar)` 闭包——闭包只收 bar 无法表达索引相关 factor，见 screener_series.rs:57-67 签名）**：`series.len() < window` → false；否则 `let count = ((series.len()-window)..series.len()).filter(|&i| day_matches(series, i, factor, op, value)).count()` ≥ at_least。**每日求值语义**：`day_matches(series, i, ...)` 计算"以 i 为最新根"的 factor 值（Sma(n) 用 `series[i-n+1..=i]`；DayPct 用 i 与 i-1；ChangePct(n) 用 i 与 i-n；AvgVolume(n) 用 `series[i-n+1..=i]`；**NDayHigh(n) 与顶层 Cmp 同一定义：`series[i-n..i]` 前 n 根最大值（不含 i）——保持 factor 语义单一来源，top-level breakout 与 Count 内一致**；Close 用 i 的 adjclose）并与 value 比较；**i 处窗口不足（如 Sma(60) 但 i<59）→ 该日不计入**。value 为 `Factor(f)` 时对每日求值同 factor
    - `VolumeSurge{days, times}` → `screener_series::volume_surge(series, days, times).unwrap_or(false)`
  - `factor_value`（复用/内联既有 helper 语义，**不单独 pub**）：`Close` → `series.last().adjclose`；`Sma(n)` → `ma(series, n)`（lib.rs:504-508，窗口不足 → None）；`ChangePct(n)` → `momentum_return(series, n)`（lib.rs:538-542：`(last - series[len-n-1]) / base * 100`，需 n+1 根）；`DayPct` → 最新日涨幅 `(last - prev)/prev*100`（需 2 根，base=0 → None）；`AvgVolume(n)` → 最近 n 根 volume 均值（需 n 根）；`NDayHigh(n)` → **前 n 根（不含最新）adjclose 最大值**（**锁定语义**：复刻 matches_breakout lib.rs:527-535 `series[window_start..series.len()-1]`，需 n+1 根；这是 Close > NDayHigh(n) 与 breakout 一致的关键）
  - 窗口保护：所有 factor 求值返回 `Option<f64>`，NaN/Inf/除零 → None → Cmp 不匹配
  - **测试（RED 已由门禁 3.5/4 步提供，本 todo 补全实现侧单测）**：`#[cfg(test)] mod tests` 内嵌——用既有 TestBar/build_fixture 模式（可从 lib.rs:593-707 移植 fixture 或复用 screener_series.rs:112-136 bars helper）；覆盖：UpDays 真/假/窗口不足、Count 计数边界（恰好 window/不足/零匹配/全匹配/每日滑动窗口不足不计入）、Or/Not 短路、深层嵌套 And/Or/Not（深度 ≥10）、Delisted(true) 新语义、MarketCap 边界（min/max/None 侧）、factor 窗口边界（Sma60 缺根、ChangePct 缺 base、DayPct 单根、NDayHigh 缺根、NaN 输入）
  MUST NOT: 不实现 pub 的独立 factor 函数（Sma/ChangePct 等仅模块内私有）；不改 screener_series.rs 既有函数；不改 compass-types AST；不 panic、不 unwrap（用 Option 传播）；不接入 run_screener（本 todo 仅求值器本体 + 单测）。
  Parallelization: Wave 1 | Blocked by: 门禁 3.5/4 步 RED 测试（实现前必须已有失败测试） | Blocks: 2
  References (executor has NO interview context - be exhaustive): .omo/plans/llm-screener-engine.md（本 plan 求值语义锁定）; .omo/drafts/llm-screener-engine.md（决策 D1-D6）; crates/compass-strategy/src/screener_series.rs:18-104（up_days/count_in_window/volume_surge 契约：Option 返回、窗口不足 None、NaN→None）; crates/compass-strategy/src/lib.rs:391-455（screen_symbol Meta 语义）; crates/compass-strategy/src/lib.rs:504-558（ma/matches_breakout/momentum_return/matches_volume 语义基准）; crates/compass-types/src/screener.rs:17-210（Filter/MetaCond/SeriesFactor/CmpOp/FactorRef/SeriesCond 定义）; crates/compass-core/src/data/symbol.rs:53（exchange_of_symbol）; crates/compass-core/src/model.rs（StockBasic/CrossSectionBar 字段）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy screener_eval` 全绿；门禁 3.5/4 步 RED 测试在此实现后转 GREEN（`cargo test -p compass-strategy` 含 adversarial/requirement 测试）；求值器单测覆盖上述矩阵全部通过；无 `unwrap()` 于生产代码
  QA scenarios (name the exact tool + invocation): happy: `cargo test -p compass-strategy` 求值器单测 + 门禁测试全绿; failure: UpDays 窗口不足返回 false 而非 panic；NaN factor 返回 false；深层嵌套不栈溢出（深度 10 足够——AST 深度由 GUI/LLM 构造，无递归深度上限需求，但 10 层验证不 panic）; Evidence .omo/evidence/task-1-llm-screener-engine.txt
  Commit: Y | `feat(strategy): general Filter AST evaluator (screener_eval)` + 独立成行 `ref #246`

- [ ] 2. compass-strategy: run_screener 接入求值器 + 删除 filter_to_query 反编译机制
  What to do / Must NOT do: 修改 `crates/compass-strategy/src/lib.rs`：
  - `run_screener`（L68-122）删除 `let query = filter_to_query(filter)?;`（L73）与 `screen_symbol(&query, ...)`（L98）调用，改为逐 symbol：`if !screener_eval::evaluate(filter, basics_row, series, now) { continue; }` 后组装 ScreenerRow（market_cap/change_20d/latest_price/industry 计算保留，lib.rs:490-501 逻辑不变——market_cap 公式 `total_share * latest.close / 1e8`、missing share → 0.0 排序垫底、change_over(series,20) 显示列）
  - 删除死代码：`filter_to_query`（L143-151）、`convert_filter`（L184-249）、`convert_meta`（L252-290）、`convert_series`（L293-334）、`momentum_pair`（L340-362）、`bullish_pair`（L366-382）、`SeenFields`（L158-170）、`mark_seen`（L173-181）——全部删除，不再有任何调用方（GUI oracle 移除在 Todo 4）
  - `ScreenerError`（L35-44）删除 `UnsupportedFilter` 变体；更新 crate 级文档注释（L1-17：现在描述通用求值器，不再是"受限反向转换"）
  - lib.rs `#[cfg(test)]` 测试块（L758-1125）改写：删除 UnsupportedFilter 相关测试（up_days_predicate_is_unsupported/count_predicate_is_unsupported/not_node_is_unsupported/or_node_is_unsupported/delisted_true_is_unsupported/const_value_cmp_is_unsupported/isolated_sma_left_operand_is_unsupported/top_level_single_change_pct_cmp_is_unsupported/momentum_pair_with_mismatched_days_is_unsupported/non_pair_sub_and_is_unsupported/momentum_with_reversed_bounds_is_unsupported/duplicate_* 系列，L761-1050）；**L1129-1136 的 `run_screener_unsupported_filter_shape_returns_error` 也必须删除或改写**——它断言 UpDays 返回 `ScreenerError::UnsupportedFilter`（`expect_err`），求值器接入后 UpDays 正常求值返回 `Ok`，该测试必然失败（改写为正向断言：UpDays 在平序列上求值 false → 零行结果）；round-trip 测试（L1052-1125：`Filter::from(q)` 结构断言）**改写为不经 `convert`/`filter_to_query` 的纯 From 层断言**——`convert` helper（L754）随反编译删除后这些测试编译不过，改写为直接断言 `Filter::from(query)` 的结构（From 层不动，结构断言仍有效）；保留 run_screener Filter 入口语义测试（L1139-1247：nested_combo/delisted_excluded/delisted_included/flat_conditions，应全部继续通过——它们走求值器而非反编译）
  - 21 个集成测试（tests/screener.rs）**必须保持断言不变**（回归基线）——本 todo 验收的核心
  MUST NOT: 不改 screen_symbol 的语义（本 todo 删除它，但求值器必须已复刻其行为——Todo 1 负责）；不改 From<ScreenerQuery> for Filter（compass-types 不动）；不改 tests/screener.rs 的断言（只允许全部保持绿色）；不改 READ_WINDOW_DAYS/数据路径。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 5
  References: crates/compass-strategy/src/lib.rs:68-122（run_screener 现状）; lib.rs:35-44（ScreenerError）; lib.rs:143-382（反编译机制全集）; lib.rs:758-1125（测试块改写范围）; tests/screener.rs:142-698（21 个回归测试，全部 `&Filter::from(q)` 调用）; crates/compass/src/citizens/screener.rs:208（filter_to_query 的另一生产调用——Todo 4 移除，本 todo 删除函数前须确认无其他引用：grep `filter_to_query` 全仓确认只剩 lib.rs 内部与 screener.rs:208）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy` 全绿（21 集成测试断言不变 + 求值器测试 + 门禁测试）；`grep -rn "filter_to_query\|UnsupportedFilter" crates/` 在 Todo 2 完成后仅剩 Todo 4 未动的 screener.rs:208（本 todo 收尾时报告该引用待 Todo 4 清除）；`cargo check -p compass-strategy` 无警告
  QA scenarios: happy: `cargo test -p compass-strategy --test screener` 21 测试全绿（与迁移前输出一致）; failure: 求值器语义偏差 → 既有测试红（这正是回归基线的价值——不允许靠改断言通过）; Evidence .omo/evidence/task-2-llm-screener-engine.txt
  Commit: Y | `feat(strategy): run_screener evaluates Filter AST directly, remove reverse-compile` + 独立成行 `ref #246`

- [ ] 3. compass: 持久化双格式——[screener] 节 `filter` JSON key + legacy 兼容读取 + save_screener_config(&Filter)
  What to do / Must NOT do: 修改 `crates/compass/src/main.rs`：
  - 新增 `ScreenerSection` 类型（main.rs 内或独立模块）：**`#[derive(Serialize, Deserialize, Default)]`** 结构（**必须三个 derive——save 路径 `toml::Value::try_from(&ScreenerSection)` 需要 Serialize，`FullConfig` 的 `#[serde(default)] screener: ScreenerSection` 需要 Default**；Default 实现为 `filter: None, legacy: ScreenerQuery::default()`），`#[serde(default)] filter: Option<String>`（新格式，保存/读取 Filter JSON）+ `#[serde(flatten)] legacy: ScreenerQuery`（旧格式 11 键扁平，ScreenerQuery serde 契约全字段 default——flatten 兼容缺失键）。实现 `fn resolve(&self) -> Result<Filter, String>`：`filter` Some → `serde_json::from_str::<Filter>`（JSON 解析失败 → Err）；None → `Ok(Filter::from(self.legacy.clone()))`。**flatten 兼容性注**：load 侧 flatten 已被 FullConfig 现有 `#[serde(flatten)] app: AppConfig`（main.rs:241）+ load_config 测试证明可行（toml 0.8.23）；save 侧避免依赖 flatten 序列化——直接构造含 `filter` key 的 `toml::Value::Table`（见 save 说明）
  - `FullConfig`（L239-247）`screener: ScreenerQuery` → `screener: ScreenerSection`；`load_config`（L267-297）默认分支改 `ScreenerSection::default()`；**resolve 时机**：load_config 返回后、ScreenerPanel::new 前 resolve（main.rs:102 处 `Some(&config.screener)` → 需先 resolve 出 `Filter` 再传 `Some(&filter)`；resolve 失败（坏 JSON）→ warn 日志 + 回退 `Filter::from(ScreenerQuery::default())`，不 panic、不拒绝启动）
  - `save_screener_config`（L403-428）签名改 `&Filter`：**构造 `toml::Value::Table` 仅含 `filter` key**（`let mut table = toml::map::Map::new(); table.insert("filter".to_string(), toml::Value::String(serde_json::to_string(filter)?));`）→ 插入 `[screener]` 节。**保存写新格式**（filter key），旧 11 键随下次保存被替换（迁移语义：旧配置首次加载可读、首次保存后转新格式）。**不依赖 `toml::Value::try_from(&ScreenerSection)` 的 flatten 序列化**（toml 0.8.23 ser 侧 flatten 未验证）
  - main.rs 测试更新：`save_screener_config_roundtrips`（L2879-2930）改走 `&Filter`（构造 Filter 而非 ScreenerQuery 断言 industries/ma/breakout JSON round-trip + `[app]` 保留）；`save_screener_config_creates_file_when_missing`（L2932-2964）断言文件含 `filter` key（而非仅 `[screener]`）；`load_config_parses_screener_section`（L2966-3004）改为两种：legacy 扁平（industries + breakout，断言 resolve 后 Filter 等价）与 `filter = "<json>"`（断言 resolve 后 Filter 精确相等）；`load_config_missing_screener_section_uses_default`（L3006-3040）断言 resolve 后 == `Filter::from(ScreenerQuery::default())`；`save_theme_config_preserves_other_sections`（L4165-4224）改含 filter key 的 [screener] 断言跨节保留；新增坏 JSON 测试：`filter = "{not json"` → resolve Err → 回退默认 + 不 panic
  - compass Cargo.toml 加 `serde_json = { workspace = true }`
  MUST NOT: 不改 ScreenerQuery serde 契约（legacy 读取依赖它）；不改 `From<ScreenerQuery> for Filter`；不删除 legacy 扁平 key 的读取能力（旧配置必须可读）；不改其他 config 节（[app]/[watchlist]/主题读写不动）；`save_watchlist_config`/`save_theme_config`/`save_language_config`/`rewrite_config_file`（main.rs:366-394,436-563）不动。
  Parallelization: Wave 2 | Blocked by: — | Blocks: 4
  References: crates/compass/src/main.rs:239-247（FullConfig）; main.rs:267-297（load_config + 默认分支）; main.rs:396-428（save_screener_config）; main.rs:2879-3040, 4165-4224（持久化测试现状）; crates/compass-types/src/lib.rs:133-195（ScreenerQuery serde 契约：11 字段全 `#[serde(default)]`，exclude_delisted 默认 true）; crates/compass/Cargo.toml（dependencies——serde 已有，serde_json 需加）; .omo/plans/llm-screener-engine.md（D3 持久化决策）
  Acceptance criteria (agent-executable): `cargo test -p compass` 持久化相关测试全绿（新旧格式 + 坏 JSON + 跨节保留）；`cargo check -p compass` 通过；手动构造 legacy 配置（仅 11 键）加载 → resolve 等价 `Filter::from(query)`；新格式 `filter = "{\"And\":[]}"` 加载 → resolve == `Filter::And(vec![])`
  QA scenarios: happy: `cargo test -p compass` 持久化测试全绿; failure: 坏 JSON 不 panic 不拒绝启动（回退默认 + warn）；legacy 无 filter key 正常读取; Evidence .omo/evidence/task-3-llm-screener-engine.txt
  Commit: Y | `feat(gui): persist screener Filter AST JSON in [screener] config section` + 独立成行 `ref #246`

- [ ] 4. compass: GUI 调用点迁移——ScreenerPanel::new 收 &Filter + oracle 移除 + i18n 清理
  What to do / Must NOT do: 修改 `crates/compass/src/citizens/screener.rs` + `crates/compass/src/main.rs`：
  - `ScreenerPanel::new`（screener.rs:129-152）签名：`restore: Option<&ScreenerQuery>` → `Option<&Filter>`，`on_save: Box<dyn Fn(&ScreenerQuery) + Send + Sync>` → `Box<dyn Fn(&Filter) + Send + Sync>`；`on_save` 字段类型（screener.rs:107）同步改；restore 分支逻辑改写：`Filter::from(query.clone())` 不再需要——直接对 `filter` 匹配（`Filter::Meta(MetaCond::Delisted(false))` → default_root_cards；`Filter::And(v) if v.is_empty()` → default_root_cards；否则 `filter_to_items(filter)`）
  - filter 按钮 handler（screener.rs:196-214）删除 oracle 块：`match compass_strategy::filter_to_query(&filter) { Ok(query) => (self.on_save)(&query), Err(_) => ...unsupported_save... }` → 直接 `(self.on_save)(&filter)`；删除 `shared_state.screener_error.set(unsupported_save)` 分支（L210-212）与相关注释
  - 删除 `unsupported_save` i18n key：crates/compass-i18n/locales/en.yml:128、zh.yml:126；**同时删除 `unsupported_run`（en.yml:127、zh.yml:125）**——screener.rs:245-249 的 `err.starts_with("unsupported filter shape")` 分支删除后该 key 无引用（错误显示简化为通用分支）；删除 screener.rs:1292 的测试断言引用（`filter_click_uncompressible_state_shows_hint_without_save` L1238-1289 改写：现在所有组合都可保存 → 该测试改为断言"组合可保存且 on_save 收到 Filter"）；`filter_click_compresses_to_legacy_query_on_save`（L1184-1236）改写为 `filter_click_saves_filter_ast`（断言 on_save 收到与 builder 一致的 Filter）
  - **签名迁移影响的 GUI 测试不止 L1184-1294**：`restore_seeds_builder_cards_from_query`（screener.rs:1050-1099）与 `restore_query_renders_cards_and_filter_saves_equivalent_query`（screener.rs:1986-2130）都向 `ScreenerPanel::new` 传 `Some(&ScreenerQuery)` 或 `Box<dyn Fn(&ScreenerQuery)>`——签名改后必须同步改写（restore 传 `Some(&filter)`、on_save 闭包收 `&Filter`）。改写后运行 `cargo test -p compass` 全绿
  - main.rs:99-109（ScreenerPanel::new 调用）：`Some(&config.screener)` → resolve 出的 `&Filter`（Todo 3 的 resolve 结果）；`Box::new(|q| save_screener_config(q))` → `Box::new(|f| save_screener_config(f))`（闭包参数类型自然跟随签名）
  - backend.rs:598/647 测试中的 `Filter::from(ScreenerQuery)` 构造无需改（仍有效）；检查 backend.rs 错误分支是否引用 `UnsupportedFilter` 字符串（screener.rs:245-249 的 `err.starts_with("unsupported filter shape")` 分支——ScreenerError::UnsupportedFilter 删除后该分支永不触发，删除或简化为通用错误显示）
  MUST NOT: 不改 screener_builder.rs 的 filter_to_items/group_to_filter/leaf_to_filter（视图模型保持）；不改 messages.rs 的 RunScreenerRequest.filter: Filter（已是 AST）；不改 tabs.rs/dispatcher.rs/state.rs（无 ScreenerQuery 依赖）；不保留任何 `filter_to_query` 引用（本 todo 完成后全仓 grep 应为零）。
  Parallelization: Wave 3 | Blocked by: 3 | Blocks: 6
  References: crates/compass/src/citizens/screener.rs:107,129-152,196-214,229-258（签名/restore/oracle/错误显示）; screener.rs:1050-1099,1184-1294,1986-2130（签名迁移影响的保存/restore 测试改写）; crates/compass/src/main.rs:99-109（panel 构造）; crates/compass-i18n/locales/en.yml:125-128, zh.yml:123-126（unsupported_save + unsupported_run key 删除）; crates/compass/src/backend.rs:149,598-662（run_screener 调用 + 测试）; .omo/plans/llm-screener-engine.md（D1 决策：oracle 移除）
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿（含改写后的保存测试）；`grep -rn "filter_to_query\|unsupported_save\|UnsupportedFilter" crates/` 返回零匹配（Todo 4 完成后）；`cargo check --workspace` 通过
  QA scenarios: happy: `cargo test -p compass` GUI 保存测试全绿; failure: grep 残留 filter_to_query/unsupported_save 引用 → 测试失败; Evidence .omo/evidence/task-4-llm-screener-engine.txt
  Commit: Y | `feat(gui): ScreenerPanel saves Filter AST directly, drop legacy save oracle` + 独立成行 `ref #246`

- [ ] 5. compass-strategy: criterion bench——run_screener(&Filter) 迁移前后性能对比
  What to do / Must NOT do: 新建 `crates/compass-strategy/benches/screener_eval.rs`（criterion）：合成 6000 个标的 × 400 根日线 bar（固定种子、确定性数据——可用循环生成 closes 带趋势/波动，非随机或 seeded RNG），构造代表性 Filter 与空 Filter 两档；基准函数 `run_screener(&filter, &reader, now)`（reader 指向内存 DuckDB→Parquet tempdir，复用 build_fixture 思路但规模化）；输出 `criterion::criterion_group!(screener, bench_run_screener)`；compass-strategy Cargo.toml 加 `criterion = { workspace = true }` dev-dep + `[[bench]] name = "screener_eval"`（如需要）
  **代表性 Filter 必须 legacy 可表达（关键）**：旧路径（a1dbcad）的 run_screener 走 filter_to_query 受限文法，`UpDays`/`Count`/`Or`/`Not` 会直接返回 `ScreenerError::UnsupportedFilter`（Err 近零耗时）——含这些节点的 Filter 在旧路径上测得的是"报错速度"而非筛选耗时，对比无效。**必须用旧文法可接受的形状**：如 `And([Meta(Industry(["白酒"])), Meta(MarketCap{min:Some(100.0),max:None}), Series(Cmp{Close,Gt,Factor(Sma(20))}), Series(Cmp{Close,Gt,Factor(NDayHigh(60))})])`（Industry+MarketCap+Sma20+NDayHigh60 全部在旧 accept-grammar 内，lib.rs:252-334 可转换）；空 Filter 两档即可（`And([])` 两端都全市场扫描）。
  **对比基线**：迁移前（filter_to_query 路径）在同一 bench 代码下测得 elapsed。实操：Todo 5 执行时用 `git worktree add /tmp/compass-baseline a1dbcad`（当前 HEAD 即迁移前状态）跑同一 bench 文件（bench 文件需能独立于新求值器编译——旧 commit 无 bench 文件/无 criterion dev-dep 时，将 bench 文件 + Cargo.toml criterion dev-dep checkout 到临时 worktree 或对旧代码手动执行等价计时脚本；**criterion dev-dep 必须在基线 worktree 的 Cargo.toml 同样加上，否则旧侧无法编译 bench**），对比两档的 `median`/`mean` elapsed，记录到 `.omo/evidence/task-5-llm-screener-engine.txt`
  MUST NOT: 不引入新依赖（criterion 已在 workspace dev-deps）；不测真实数据库（合成数据即可——性能对比相对值有效）；不改 run_screener 签名；bench 不进 CI（本地证据，plan 不新增 CI 步骤）；不造假数据（6000 标的 × 400 根必须真实生成并至少断言 bar 数）。
  Parallelization: Wave 3 | Blocked by: 2 | Blocks: 6
  References: crates/compass-strategy/Cargo.toml（当前 dev-deps）; Cargo.toml:45（workspace criterion = "0.5"）; crates/compass-strategy/tests/screener.rs:113-140（build_fixture 模式：DuckDB 内存 → COPY PARQUET → ParquetReader）; crates/compass-strategy/src/lib.rs:68-122（run_screener 签名）; kb/dev/testing.md（benchmark 章节现状——若已有 bench 约定优先遵循）
  Acceptance criteria (agent-executable): `cargo bench -p compass-strategy --bench screener_eval` 成功输出两组（迁移前后）elapsed；对比结论写入 evidence 文件：新路径 ≤ 2× 旧路径（同量级），或超量级时给出根因分析（求值器理论复杂度 O(symbols × window) 与旧 screen_symbol 相同——不应超）
  QA scenarios: happy: bench 输出数值 + evidence 记录对比; failure: 新路径显著劣化（>2×）→ 不得声称完成，需优化求值器（如 Meta 先求值短路、factor 缓存）后重测; Evidence .omo/evidence/task-5-llm-screener-engine.txt
  Commit: Y | `bench(strategy): run_screener Filter AST evaluator parity benchmark` + 独立成行 `ref #246`

- [ ] 6. doc-sync + 决策记录
  What to do / Must NOT do: 按 AGENTS.md 变更类型 → kb/ 映射表更新：
  - `kb/design/architecture.md`：§选股器表达式 AST 章节（L77-132）更新——"AST 不直接求值——通用求值器属 Batch 3"（L82-83）改为描述已落地的通用求值器；受限反向转换描述（L119-124）改为求值器 + 持久化；M4 决策记录（L574）追加/修订为 Batch 3 决策（通用求值器替代受限转换、持久化双格式、Delisted(true) 支持）；`## 决策记录` 章节追加 Batch 3 决策行（D1 删除反编译、D3 持久化双格式、D5 Delisted(true)、NDayHigh 前 N 根语义、Count 每日滑动窗口）
  - `kb/user/config.md`：[screener] 节（L32-48）更新——新增 `filter` key 说明（Filter JSON 字符串，新格式）；L46-48 注释（"AST 到 Batch 3 才作为持久化格式"）改为已落地描述；legacy 11 键标注为"旧格式，读取兼容、保存迁移"
  - `kb/user/gui.md`：L154-156 已知限制（"或组/Not/连续上涨运行时报引擎暂不支持…引擎 Batch 3 支持后自然消失"）改为已支持描述
  - `kb/design/ui.md`：L276 同类注释更新（若含"Batch 3 支持后自然消失"字样）
  - `kb/dev/testing.md`：如新增了求值器测试模式（每日滑动窗口求值等），在测试章节补充；覆盖率章节（L247-279）无需改（阈值已 95%）
  - 确认所有 `kb/design/` 相关文件含 `## 决策记录` 章节（缺失则补齐）
  MUST NOT: 不写 GUI 构建器文档（Batch 2 已写）；不写 LLM 文档（Batch 4）；不在 AGENTS.md 添加重复内容（仅当索引需更新时一句话摘要）；不硬编码版本号。
  Parallelization: Wave 4 | Blocked by: 1,2,3,4,5 | Blocks: F-wave
  References: kb/design/architecture.md:77-132,563-578（AST 章节 + 决策记录）; kb/user/config.md:32-48（[screener] 节）; kb/user/gui.md:150-156（已知限制）; kb/design/ui.md:276（同类注释）; kb/dev/testing.md:247-279（覆盖率章节）; AGENTS.md（kb/ 映射表）
  Acceptance criteria (agent-executable): grep 确认 architecture.md 含 Batch 3 决策记录行 + 通用求值器描述；config.md 含 `filter` key 说明；gui.md/ui.md 无"Batch 3 支持后自然消失"残留；`grep -rn "Batch 3 支持后自然消失\|AST 到 Batch 3 才作为持久化格式" kb/` 零匹配
  QA scenarios: happy: grep 断言通过; failure: 任一残留文本匹配 → 测试失败; Evidence .omo/evidence/task-6-llm-screener-engine.txt
  Commit: Y | `docs: screener Filter AST evaluator + persistence format + decision records` + 独立成行 `ref #246`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit: 逐条核对 6 个 todo 的 Acceptance criteria 证据落盘 `.omo/evidence/task-{1..6}-llm-screener-engine.txt`；`git log` 确认每个 commit 含独立成行 `ref #246`；覆盖率脚本退出 0（compass-strategy ≥95%）
- [ ] F2. Code quality review: `cargo clippy --workspace` 无新警告；`cargo fmt --check` 通过；新增 pub 项（evaluate）带 `///` 文档注释；无 `unwrap()`/panic 路径（求值器 Option 传播）；`grep -rn "filter_to_query\|unsupported_save\|UnsupportedFilter" crates/` 零匹配
- [ ] F3. Real manual QA: `cargo test --workspace` 全绿（21 集成测试断言不变 + 求值器/持久化/GUI 新测试 + 覆盖率脚本）；`cargo bench -p compass-strategy --bench screener_eval` 结果与 evidence 一致（迁移前后同量级）
- [ ] F4. Scope fidelity: 核对 Must NOT have 清单——无 LLM（#247）、无 GUI 构建器改动（screener_builder.rs 视图模型未动，`git diff` 验证）、无 ScreenerQuery 删除、无新第三方依赖、21 测试断言未改、READ_WINDOW_DAYS/数据路径未动

## Commit strategy
- 每个 commit 独立成行 `ref #246`（hook 校验，指向 OPEN issue）
- 顺序：1→2→3→4→5→6（3 可与 2 并行、5 可与 4 并行——按依赖矩阵；每个 todo 一个 commit）
- Commit → Review：每次 commit 后运行 `/review-work`（goal/quality/security/QA/context 5 并行），发现问题最多 2 轮修复
- 禁止 auto-push：用户明确说 "push" 才 push；push 前 `git fetch origin master && git rebase origin/master`
- push 前写反思（/skwy-reflect），反思 commit 随 PR 同批推送
- push 后追加完成 comment（`gh issue comment 246`）+ 关闭 issue #246；PR 创建参考 epic #243

## Success criteria
- [ ] `evaluate()` 通用求值器实现，UpDays/Count/Or/Not 真实过滤（Todo 1 验收 + 门禁 3.5/4 步测试 GREEN）
- [ ] run_screener 直接消费求值器，filter_to_query 全套机制删除，21 集成测试断言不变（Todo 2 验收）
- [ ] 持久化双格式落地：保存写 `filter` JSON、加载 legacy 兼容、坏 JSON 回退不崩溃（Todo 3 验收）
- [ ] GUI 保存路径迁移 &Filter，oracle/unsupported_save 全清（Todo 4 验收）
- [ ] criterion bench 证据：迁移前后同量级（Todo 5 验收）
- [ ] kb/ 文档同步 + 决策记录（Todo 6 验收）
- [ ] 全部 commit 引用 `ref #246`，cargo test 全绿 + compass-strategy 覆盖率 ≥95%，push 后 issue 收尾（comment + close）
