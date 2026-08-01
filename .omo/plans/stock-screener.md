# stock-screener - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 在 compass 中新增一个"选股"标签页：输入条件（行业、板块、上市时长、市值、均线、创新高、动量、放量、排除退市）后一键筛选全市场 A 股，得到一张可排序的结果表，点击任意一只股票立即在图表中查看其 K 线。选股条件会自动保存，下次打开应用仍然保留。

**Why this approach:** 把选股拆成三层：底层只负责"把全市场行情一次性读进内存"（新增数据读取原语），中间层负责"纯计算"（新选股引擎 crate，不含任何界面代码，易于测试），最上层是界面（新标签页，复用现有 citizen/dock/toast 架构）。数据读取与计算分离，任何一层都能独立测试。

**What it will NOT do:** 不做基本面/估值筛选与 MACD（留待第二版，需要先补财务数据导出）；不改图表的前复权显示；不做自选股相关功能（已确认不需要）；不做选股结果的历史保存。

**Effort:** Large
**Risk:** Medium - 新增 2 个 crate 与 1 个 GUI 标签页，GUI 消息通道与 config 持久化是主要复杂度
**Decisions to sanity-check:** ①选股条件保存到 config.toml 的 `[screener]` 节（不修改现有 AppConfig，避免 core 依赖新 types crate）②量能条件定义为"近 N 日均量 ≥ X 倍 × 近 3N 日均量"③突破 = 严格高于前 N 日最高价 ④市值 = 总股本 × 最新收盘价 ÷ 1 亿（亿元）⑤动量默认窗口 20 日、区间 0%~100%

Your next move: 阅读完整计划并批准，或要求先运行 high-accuracy review。完整执行细节见下。

---

> TL;DR (machine): Large effort / Medium risk - 5-component feature（core 原语 + 2 新 crate + GUI tab + CI 脚本）across 5 waves, 10 todos

## Scope
### Must have
- **C1 core 横截面原语**（epic #105 / sub #106）：`ParquetReader::fetch_cross_section(range_start, range_end) -> Result<Vec<CrossSectionBar>, DataError>`，直读 `stock_daily.parquet`，返回全市场含 `adjclose/close/volume` 的日线；`CrossSectionBar` 结构体定义于 core `model.rs`。单元测试（tempdir + COPY TO 造 parquet）。
- **C2 compass-types crate**（sub #107）：workspace 新成员；`ScreenerQuery`（serde Serialize+Deserialize，**手动 Default impl**）、`ScreenerRow`（6 展示列）、条件子类型（`MaCondition/BreakoutCondition/MomentumCondition/VolumeCondition`）。单元测试（serde 往返 + Default 语义）。
- **C3 compass-strategy crate**（sub #108）：选股引擎 `run_screener(query, reader, now) -> Result<ScreenerResult, ScreenerError>`（`ScreenerResult { rows: Vec<ScreenerRow>, total: usize }`）；元数据条件（行业多选 OR / 交易所多选 OR / 板块多选 OR / 上市时长下限 / 市值区间 / 排除退市）+ 技术面条件（均线关系单选 / 突破 N 日新高 / 动量区间 / 量能倍数），多条件 AND；结果按市值降序、上限 100、**total 记录截断前总数**；全量单元测试（TDD）。
- **C4 GUI Screener tab**（sub #109）：`TabKind::Screener` 新 tab（citizen + egui_dock）；左侧条件输入区（行业可搜索多选、交易所/板块 CheckBox 多选、上市时长预设、市值 min/max 输入、均线下拉+启用、突破 N 日、动量 min/max、量能倍数、排除退市 CheckBox）+ 手动"筛选"按钮；右侧结果表格 6 列（代码/名称/最新价/20日涨跌幅/市值/行业）+ 表头排序 + 上限 100 计数文案（"共 N 只，已显示前 100"，N 来自 `ScreenerResult.total`）+ 空结果占位 + loading spinner + 错误 toast；点击结果行 → 裸 6 位代码写 `shared_state.symbol` + 复用 `dispatcher::handle(AppMessage::FetchBars, ...)` 切换图表（StockPicker 选中同步）；空条件 = 全市场（非退市）市值降序前 100；条件持久化 config.toml `[screener]` 节（重启恢复）。egui_kittest 无头集成测试。
- **C5 CI 覆盖率脚本**（epic 级）：`scripts/check-coverage.sh` 增加 `compass-strategy` / `compass-types` 两个 per-crate 门槛（≥80%）。
- **文档同步**（compass-workflow 强制）：`kb/user/gui.md`、`kb/design/architecture.md`（crate 关系 + citizen 表格）、`kb/design/data-providers.md`（fetch_cross_section + CrossSectionBar + 决策记录）、`kb/user/config.md`（[screener] 节）、`kb/dev/testing.md`（如需）、`AGENTS.md`（crate 列表——归 todo 9 独有）。
- **实现后反思**（compass-workflow 强制）：`/reflect` 追加 `kb/dev/reflections.md`。

### Must NOT have (guardrails, anti-slop, scope boundaries)
- **不得实现**：基本面/估值条件、MACD（第二版，需先补 fin_indicators.parquet 导出）；`fin_indicators.parquet` 导出管线本身
- **不得实现**：图表前复权显示改造（独立 issue）
- **不得实现**：watchlist/自选股相关功能（用户已确认"不需要自选股"）
- **不得实现**：结果持久化（仅条件持久化）；缓存/增量读取（契约：每次全量重读、无缓存）
- **不得修改**：`compass-core` 的 `DataProvider` trait（fetch_cross_section 不进 trait，避免三 impl 牵连）；`AppConfig` 结构（避免 core→types 依赖，见 todo 7）
- **不得引入**：rayon 或任何新外部依赖（除 workspace 已有 path 依赖与 `toml` dev-dependency）；`as any`/`@ts-ignore` 类类型压制；生产代码 `unwrap()`（用 `.expect(msg)` 或 `?`）
- **不得**：任何 commit 缺 `ref #N`；PR 合并前关闭 issue；push（除非用户明确指示）

## Verification strategy
> Per-todo QA is fully agent-executed (automated); the final wave F3 manual GUI QA is executed by the user.
- Test decision: **TDD（RED→GREEN）**，每 todo 先写失败测试再实现；框架 rstest + tokio::test（core）、tempdir + `COPY TO (FORMAT PARQUET)` 造 parquet（core）、`#[cfg(test)]` 单元（strategy/types）、egui_kittest 无头集成（GUI）。见 `kb/dev/testing.md`。
- Evidence: `.omo/evidence/task-<N>-stock-screener.txt`（attemptDir = `.omo/evidence/`，无 ulw-loop 时）——每个 todo 的 QA 命令输出保存于此
- 质量门：`cargo test`（workspace 全绿）、`cargo clippy -- -D warnings`、`cargo fmt --check`、`cargo doc --no-deps`（`#![warn(missing_docs)]` 无警告）——每个 todo 完成时运行
- **覆盖率提前检查（Oracle 风险提示）**：Wave 3 完成后（todo 7 结束时）立即本地运行 `cargo llvm-cov --json --summary-only > cov.json && bash scripts/check-coverage.sh 80 cov.json`——compass crate 新增约 600 行 UI 代码、types crate 为纯类型定义，均可能跌破 80% per-crate 门槛；早发现早补测试，勿等 CI

## Execution strategy
### Parallel execution waves
- **Wave 1**（并行）：Todo 1（core 原语）+ Todo 2（types crate）——互不依赖
- **Wave 2**：Todo 3（strategy 引擎）——依赖 Todo 1+2
- **Wave 3**（Todo 4→5→6→7 串行；Todo 8 与 Todo 4 并行）：GUI 4 个 todo 均修改 `citizens/screener.rs`（Todo 4 创建该文件，5/6/7 继续编辑），单文件争用 → 必须串行；Todo 8（CI 脚本）仅依赖 Todo 2+3，可与 Todo 4 并行
- **Wave 4**（并行）：Todo 9（文档，依赖 1-7 全部完成）
- **Wave 5**：Todo 10（/reflect 反思）

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | — | 3 | 2 |
| 2 | — | 3, 8 | 1 |
| 3 | 1, 2 | 4, 5, 6, 7, 8 | — |
| 4 | 3 | 5, 6, 7 | 8 |
| 5 | 4 | 6, 7 | — |
| 6 | 5 | 7 | — |
| 7 | 6 | 9, 10 | — |
| 8 | 2, 3 | 10 | 4 |
| 9 | 1-7 | 10 | — |
| 10 | 1-9 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [ ] 1. core: `CrossSectionBar` 类型 + `ParquetReader::fetch_cross_section` 原语 + 单元测试
  What to do / Must NOT do:
  - 在 `crates/compass-core/src/model.rs` 新增 `pub struct CrossSectionBar { pub symbol: String, pub trade_date: NaiveDate, pub adjclose: f64, pub close: f64, pub volume: f64 }`（serde derive；`#![warn(missing_docs)]` 每字段 `///` 注释）
  - 在 `crates/compass-core/src/data/parquet.rs` 新增 `pub fn fetch_cross_section(&self, range_start: NaiveDate, range_end: NaiveDate) -> Result<Vec<CrossSectionBar>, DataError>`，同步阻塞方法（仿 `load_all_stock_basics` `parquet.rs:314-359`）：`SELECT symbol, CAST(tradedate AS VARCHAR) AS tradedate, adjclose, close, volume FROM read_parquet('{escaped}') WHERE tradedate >= ? AND tradedate <= ? ORDER BY symbol, tradedate`（**注意列名小写 `tradedate`**，见 `parquet.rs:101`）；`stock_daily.parquet` 不存在时返回空 vec（仿 load_all_stock_basics）
  - **先 verify 再实现（r4 修正）**：① `stock_daily.parquet` 的 `tradedate` 列实际类型——**真实数据核验为 TIMESTAMP**（`CAST(tradedate AS VARCHAR)` 产出 `"1991-04-04 00:00:00"` 带时间分量），**NOT DATE**；② DuckDB 绑定 NaiveDate 参数：绑定为 "YYYY-MM-DD" 字符串（仿 `fetch_bars_blocking` `parquet.rs:92-93,108`）；③ **tradedate VARCHAR → NaiveDate 解析：必须用既有 `date_str_to_utc`（`parquet.rs:362-377`，fetch_bars_blocking/load_all_stock_basics 已在用）或显式 `NaiveDate::parse_from_str("%Y-%m-%d %H:%M:%S")`——绝不可用 `%F`/`from_str`（r4 blocking 陷阱：对 "1991-04-04 00:00:00" 解析失败 → filter_map 全行丢弃 → 生产环境静默空结果，而 DATE-typed 测试 fixture 会通过）**
  - **Must NOT**：改 `DataProvider` trait / 改 `fetch_bars` 现有行为 / 动 adjclose 丢弃逻辑（`duckdb.rs:593-599` 保持现状——**master 同步后行号更新，原 703-709**）/ 用 `unwrap()` / 加 rayon
  - 测试（TDD，RED 先）：复用 `create_test_stock_daily_parquet` 造数模式（`parquet.rs:440-502`）：tempdir 造含 2+ symbol、含 adjclose/close/volume 的 stock_daily.parquet → 断言返回全 market 行数、字段正确、日期过滤生效（含边界日期包含）、文件缺失返回空 vec；**额外 RED 测试（r4 修复）：造 TIMESTAMP-typed tradedate 的 fixture（或含时间分量的字符串行）→ 断言日期正确解析为 NaiveDate（防 `%F` 陷阱回归）**
  Parallelization: Wave 1 | Blocked by: — | Blocks: 3
  References (executor has NO interview context - be exhaustive): `crates/compass-core/src/model.rs:105-129`（StockBasic 样式模板）、`crates/compass-core/src/data/parquet.rs:314-359`（load_all_stock_basics 模板）、`parquet.rs:77-144`（fetch_bars_blocking：日期绑定 + tradedate 处理）、`parquet.rs:41-43`（escape_sql_path）、`parquet.rs:101`（tradedate 列名）、`parquet.rs:440-502`（测试造数）、`crates/compass-core/src/data/duckdb.rs:84-96`（stock_daily DDL：adjclose/close/volume 字段确认——master 同步后行号更新）、`kb/design/data-providers.md:120-125`（parquet 布局与列类型——master 同步后行号更新）、`kb/dev/testing.md:111-119`（测试模式）、`crates/compass-core/src/data/mod.rs`（模块声明，若需 pub 导出）
  Acceptance criteria (agent-executable): `cargo test -p compass-core` 通过（含新测试）；`cargo clippy -p compass-core -- -D warnings` 0 警告；`cargo doc --no-deps` 无 missing_docs 警告
  QA scenarios (name the exact tool + invocation): happy: 造 3 symbol×5 日 parquet → `fetch_cross_section(全范围)` 返回 15 行且 adjclose/close/volume 数值正确；failure: 无 stock_daily.parquet 的 tempdir → 返回空 vec 不 panic；failure: 日期过滤 → 只返回范围内行。证据 `.omo/evidence/task-1-stock-screener.txt`
  Commit: Y | `feat(core): add fetch_cross_section cross-section primitive\n\nref #106`

- [ ] 2. crate: 新建 `compass-types`（ScreenerQuery/ScreenerRow 交界类型）+ 单元测试
  What to do / Must NOT do:
  - 创建 `crates/compass-types/`：`Cargo.toml`（deps: serde、chrono；dev-deps: toml【serde 往返测试必需】；`#![warn(missing_docs)]`）、`src/lib.rs`
  - 注册到根 `Cargo.toml` workspace members（`Cargo.toml:3-7`，当前 members: compass-core/compass/compass-data）
  - 类型定义（全部 `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`；**serde 加固（B2 修复——语义陷阱：per-field `#[serde(default)]` 对 bool 用 `bool::default()`=false 而非 `ScreenerQuery::default()` 的 true，容器级 default 对部分节无效）**：
    - `ScreenerQuery` 字段：`exclude_delisted: bool` 用 **`#[serde(default = "default_exclude_delisted")]`**（模块级 `fn default_exclude_delisted() -> bool { true }`）；其余 `Vec`/`Option` 字段用 `#[serde(default)]`；**条件子结构体字段（`ma`/`breakout`/`momentum`/`volume`）用 `#[serde(default)]`（Option → None）**
    - **条件子结构体 `BreakoutCondition`/`MomentumCondition`/`VolumeCondition` 各加容器级 `#[serde(default)]`（B2 修复——空表如 `breakout = {}` 反序列化用各自手动 Default impl，不报错）**；`MaCondition` 为枚举无需
    - `pub struct ScreenerQuery { pub industries: Vec<String>, pub exchanges: Vec<String>, pub boards: Vec<String>, pub list_years: Option<u32>, pub market_cap_min: Option<f64>, pub market_cap_max: Option<f64>, pub exclude_delisted: bool, pub ma: Option<MaCondition>, pub breakout: Option<BreakoutCondition>, pub momentum: Option<MomentumCondition>, pub volume: Option<VolumeCondition> }`（字段 `///` 注释；空 Vec = 不限）
    - `pub enum MaCondition { AboveMa20, AboveMa60, BullishAlign }`（`#[serde(rename_all = "snake_case")]`，单元变体序列化为字符串）
    - `pub struct BreakoutCondition { pub days: u32 }`
    - `pub struct MomentumCondition { pub days: u32, pub min_pct: f64, pub max_pct: f64 }`
    - `pub struct VolumeCondition { pub days: u32, pub times: f64 }`
    - `pub struct ScreenerRow { pub symbol: String, pub name: String, pub latest_price: f64, pub change_20d: f64, pub market_cap: f64, pub industry: String }`
  - **手动 Default impl 语义（BLOCKING 决策，derived Default 无法表达）**：
    - `ScreenerQuery::default()` → `exclude_delisted: true`，其余字段空/None
    - `VolumeCondition::default()` → `{ days: 20, times: 2.0 }`（契约默认 2 倍/20 日）
    - `BreakoutCondition::default()` → `{ days: 60 }`（契约默认 N=60）
    - `MomentumCondition::default()` → `{ days: 20, min_pct: 0.0, max_pct: 100.0 }`（GUI 默认区间）
    - `MaCondition` 无 Default（通过 `Option` 使用）
  - **Must NOT**：迁移 StockBasic 等 core 已有类型（仅交界类型）；添加 core 依赖（core 不得依赖 types——依赖方向 contract）；加非必要运行时依赖
  - 测试（TDD）：serde TOML 往返（query → toml string → 反序列化 → 相等，需 toml dev-dep）；Default 语义断言（`ScreenerQuery::default().exclude_delisted == true`、`VolumeCondition::default().days == 20 && times == 2.0` 等）；**partial `[screener]` 节反序列化（B2 断言——① 仅含部分字段的 TOML → 缺 `exclude_delisted` 时结果为 `true`（`default_exclude_delisted` 生效，非 bool 默认 false）；② `breakout = {}` 空表 → `BreakoutCondition::default()`；③ 整体 `[screener]` 缺省 → `ScreenerQuery::default()`）**
  Parallelization: Wave 1 | Blocked by: — | Blocks: 3, 8
  References (executor has NO interview context - be exhaustive): 根 `Cargo.toml:3-7`（workspace members）、`crates/compass-core/Cargo.toml`（crate 结构模板）、`crates/compass-core/src/model.rs:175-266`（serde default 用法模板——**参考其写法但本 crate 用手动 Default**）、`kb/design/architecture.md:19-49`（crate 关系图——本 todo 后需更新）、`.omo/drafts/stock-screener.md`（ScreenerQuery 字段设计决策 + Default 语义）
  Acceptance criteria (agent-executable): `cargo test -p compass-types` 通过；`cargo clippy -p compass-types -- -D warnings` 0 警告；`cargo doc --no-deps` 无 missing_docs；workspace 编译通过（`cargo check`）
  QA scenarios (exact tool + invocation): happy: serde 往返保持相等（`cargo test -p compass-types serde_roundtrip`）；happy: Default 断言（`cargo test -p compass-types default_semantics`）；failure: 未知字段反序列化不 panic（serde 默认忽略）。证据 `.omo/evidence/task-2-stock-screener.txt`
  Commit: Y | `feat(types): add compass-types crate with screener boundary types\n\nref #107`

- [ ] 3. crate: 新建 `compass-strategy`（选股引擎：元数据 + 技术面条件筛选）+ 全量测试
  What to do / Must NOT do:
  - 创建 `crates/compass-strategy/`：`Cargo.toml`（deps: `compass-types`(path)、`compass-core`(path)、chrono、thiserror、tracing；dev: rstest、tempfile）、`src/lib.rs`（`#![warn(missing_docs)]`），注册根 Cargo.toml workspace members
  - 引擎签名：`pub fn run_screener(query: &ScreenerQuery, reader: &ParquetReader, now: NaiveDate) -> Result<ScreenerResult, ScreenerError>`（**同步**；`ScreenerError` thiserror 枚举，本 crate 定义）；`pub struct ScreenerResult { pub rows: Vec<ScreenerRow>, pub total: usize }`——**total 为截断前的匹配总数**（用于 GUI "共 N 只" 文案，BLOCKING 决策）
  - 实现步骤（内部结构按需拆 mod，遵守 250 LOC/文件 上限）：
    1. 读全量：`reader.fetch_cross_section(now - 400 天, now)`（400 日历日 ≈ **268 交易日（真实数据核验：截至 2026-07-28 最大交易日的最近 400 日历日）**，覆盖 MA60+突破 60+动量 20 上限）+ `reader.load_all_stock_basics()`
    2. 元数据过滤：行业（`industries` 非空时 symbol 的 industry ∈ 集合，多值 OR）；交易所（symbol 前缀 ∈ exchanges，多值 OR，见 `kb/design/symbols.md:52-68` 推断规则——**先 verify 再实现：扩展启发式为 `6→SH、8/92→BJ、其余→SZ`，因真实数据含 92xxxx 北交所代码（已核验 stock_daily.parquet 含 920992），symbols.md 文档遗漏 92 段**）；板块（board ∈ boards，多值 OR）；上市时长（`list_date` 非空时 `now - list_date >= list_years*365` 天；**list_date 缺失且 list_years 设置 → 剔除**）；市值区间（`total_share * 最新 close / 1e8` ∈ [min,max]，单位为**亿元**，显式 ÷1e8——已核验 `total_share` 单位为股）；**排除退市（exclude_delisted=true 时剔除 `delist_date` 非空标的；即仅保留 delist_date 为 NULL/空的——BLOCKING 方向修正，字面意思与"排除退市"一致）**；**幽灵符号剔除（BLOCKING 决策——真实数据核验：stock_daily 含 6122 个 symbol、stock_basic 仅 5888，约 234 个 daily-only 幽灵符号无 basics 行且无 delist_date 可判退市 → 一律剔除，避免污染 total 与空条件结果；与"非退市 A 股"意图一致）**
    **`total_share` 缺失策略（BLOCKING 决策——真实数据核验：`compass_data.stock_basic` 中 `total_share IS NULL` 占 2664/5888 ≈ 45%）**：market_cap 无法计算 → ① 市值区间条件启用时（market_cap_min/max 任一 Some）→ **剔除**该股；② 否则 market_cap 记 0.0（排序置底，确定性）；③ 显示列：market_cap == 0.0 时 GUI 显示 "—"
    3. 技术面过滤（**全部基于 adjclose**；**条件窗口按交易日 bar 计数（返回数组索引），非自然日——BLOCKING 决策**）：均线（最新 adjclose 与 MA5/MA20/MA60 关系：AboveMa20=`adjclose>MA20`，AboveMa60=`adjclose>MA60`，BullishAlign=`MA5>MA20>MA60`）；突破（**最新 adjclose 严格 `>` 前 days 根 bar 的 adjclose 最大值（不含当日）**——"创 N 日新高"语义，`>` 不含相等；**勿用 high 列——fetch_cross_section 不返回 high，全部基于 adjclose**）；动量（`(adjclose[last] - adjclose[last-days]) / adjclose[last-days] * 100 ∈ [min_pct, max_pct]`，days 为 bar 数）；量能（**近 days 日均量 >= times × 近 3×days 日均量（截至最新 bar、嵌套含近 days 日的重叠基线窗口——即基线窗口 ⊇ 近 days 窗口，BLOCKING 语义定案）**；days 默认 20、times 默认 2.0，来自 `VolumeCondition::default()`）
    4. **窗口不足策略（统一，BLOCKING 决策）**：某标的数据量少于**该条件所需最少 bar 数** → **跳过该条件**（不部分计算、不降级）。每条件最少 bar 数定义：均线 = 对应 MA 的 days（MA20 需 20、MA60 需 60，BullishAlign 需 60）；动量 = days + 1（公式索引 last-days 需第 days+1 根）；突破 = days + 1（不含当日的前 days 根）；量能 = **3×days（嵌套基线窗口：近 3N 根含近 N 根，恰好 3N 根即可计算）**。`change_20d` 为显示列不可跳过：少于 20 bars 时用可用 bars 计算（至少 2 bars），不足 2 bars 记 0.0
    5. 多条件 AND；条件内多值 OR（行业/交易所/板块）
    6. 排序：market_cap 降序（f64 比较），**相等时 symbol 升序（确定性，便于测试）**；`rows` 截断前 100，**total = 截断前匹配总数**
    7. 组装 `ScreenerRow { symbol, name(来自 StockBasic；symbol 在 stock_basic 缺失时回退为 symbol 自身), latest_price(最新 close), change_20d(近 20 日 adjclose 涨跌幅 %), market_cap(亿元；total_share 缺失时按第 2 步策略记 0.0), industry }`
    **`basics 有行但 cross-section 无 bar` 策略（BLOCKING 决策——真实数据核验：301677 C欣兴工具、920065 千岸科技 有 basics 行、delist_date 为空、total_share 有值，但 stock_daily.parquet 零 bar）**：此类标的一律从结果剔除（无法计算最新价/市值），**不计入 total**——在元数据过滤阶段（step 2）完成，与幽灵符号剔除并列
  - **已核验事实（勿再探查）**：`model::StockBasic` 字段含 `board/list_date/delist_date/total_share/name/industry`（`model.rs:106-130`——master 同步后行号更新）✅；**master 已删除 duckdb.rs 本地 StockBasic（双类型问题已解决，本计划无需处理）**✅；`stock_daily.parquet`/`stock_basic.parquet` 的 symbol 列均为**裸 6 位代码**（import 时 `SUBSTRING(symbol,3)` 去前缀，`import_dolt.rs:144,179`——master 同步后行号更新）——引擎 join key 直接用裸码，无需前缀归一化 ✅；exchange 推断用前缀规则（见步骤 2 的 92 扩展）
  - **Must NOT**：引入 rayon/新外部依赖；依赖 GUI 代码；async（引擎纯同步）；改 core 任何代码（除调用既有 API）；裸 `unwrap()`
  - 测试（TDD，RED 先）：复用 `parquet.rs:440-502` 造数模式在 strategy 测试里造多标的 parquet + tempdir `ParquetReader`；每条件独立测试 + 组合 AND 测试 + 排序/截断/total 测试 + 排除退市测试（**断言退市股被剔除、方向正确**）+ **total_share NULL 混合 fixture 测试（市值条件启用→剔除；未启用→market_cap 0.0 置底）** + **突破严格 `>` 测试（相等不通过）** + **幽灵符号剔除测试（daily 有 basics 无 → 剔除）** + **basics 有行但无 bar 剔除测试（301677 型标的 → 剔除且不计 total）** + **量能窗口边界测试（恰好 3N 根 → 计算；不足 3N 根 → 跳过）** + **92xxxx 交易所归属测试（92→BJ）** + 空条件（全市场非退市市值降序前 100）+ 边界（窗口不足跳过、list_date 缺失剔除、市值单位）
  Parallelization: Wave 2 | Blocked by: 1, 2 | Blocks: 4, 5, 6, 7, 8
  References (executor has NO interview context - be exhaustive): `crates/compass-core/src/data/parquet.rs:314-359`（load_all_stock_basics 返回 `model::StockBasic`，字段含 industry/board/list_date/delist_date/total_share/name）、`crates/compass-core/src/model.rs:105-129`（StockBasic 字段——先 verify 再使用）、`crates/compass-core/src/data/parquet.rs:440-502`（parquet 测试造数）、`crates/compass-core/src/data/parquet.rs:77-144`（fetch_bars_blocking 空结果 NoData 语义——本引擎应容忍单标的缺数据）、`kb/design/symbols.md:52-68`（exchange 推断：symbol 前缀 SH/SZ/BJ）、`crates/compass-types/src/lib.rs`（ScreenerQuery/ScreenerResult 类型——todo 2 产物）、`.omo/drafts/stock-screener.md`（D4/D5 决策：指标基于 adjclose；市值 = total_share × 最新 close ÷ 1e8）
  Acceptance criteria (agent-executable): `cargo test -p compass-strategy` 全绿（新测试覆盖每条件+组合+排序+total+空条件+退市方向）；`cargo clippy -p compass-strategy -- -D warnings` 0 警告；`cargo doc --no-deps` 无 missing_docs；workspace `cargo check` 通过
  QA scenarios (exact tool + invocation): happy: 造 5 标的（含满足/不满足各条件的样本 + 1 只退市股 + 1 只 total_share NULL）→ 各条件筛选断言结果集 + 退市股被排除 + NULL 股按策略处理；happy: **显式 >100 场景（造 120 个匹配标的）→ `rows.len() == 100 && total == 120`**；happy: 突破相等场景 → 不通过（严格 `>`）；failure: 空 parquet 目录 → 返回空 rows 与 total 0 不 panic；failure: 某标的窗口不足 → 跳过该条件不崩。证据 `.omo/evidence/task-3-stock-screener.txt`
  Commit: Y | `feat(strategy): add screener engine with metadata and technical conditions\n\nref #108`

- [ ] 4. GUI 基础设施: `TabKind::Screener` 变体 + ScreenerPanel citizen + 异步通道 + SharedState 字段 + dock 装配
  What to do / Must NOT do:
  - `crates/compass/src/tabs.rs`：加 `pub const SCREENER_ID: &str = "screener";`、`TabKind::Screener` 变体、`title()`（"Screener"）与 `citizen_id()` match 分支（仿 `tabs.rs:45-65`）、`TabViewer` 加 `screener: &'a mut ScreenerPanel`、`run_screener_signal: &'a Signal<RunScreenerRequest>`、**`work_signal: &'a Signal<FetchRequest>`、`screener_industries: &'a [String]`、`screener_boards: &'a [String]`** 字段 + `ui()` match 分支（仿 `tabs.rs:107-134`——**B2 修复：work_signal 必须进 TabViewer，todo 6 的点击行需要它派发 FetchBars；industries/boards 是 show() 签名所需，一并传入（N2 修复）**）
  - 新建 `crates/compass/src/citizens/screener.rs`：`ScreenerPanel` 实现 `Citizen` trait（三方法，仿 `citizens/logger.rs:11-42`）+ `new()` + **`show(&mut self, ui: &mut egui::Ui, shared_state: &SharedState, run_screener_signal: &Signal<RunScreenerRequest>, work_signal: &Signal<FetchRequest>, industries: &[String], boards: &[String])`**（签名固定，勿留 "..."；行业/板块列表由 CompassApp 从已有 `stock_list` 派生传入，panel 不直接读 parquet；**work_signal 用于 todo 6 点击行后派发 FetchBars——B2 修复**）；`citizens/mod.rs` 加 `pub mod screener;`
  - `crates/compass/src/messages.rs`：加 `RunScreenerRequest { query: ScreenerQuery }` / `RunScreenerResponse { rows: Vec<ScreenerRow>, total: usize, error: Option<String> }`（**total 无条件包含**，仿 `messages.rs:14-27`——BLOCKING 决策）
  - `crates/compass/src/backend.rs`：`wire_backend` 增加第二条 Signal/Slot 通道（`RunScreenerRequest` → `AsyncDispatcher.attach_async` → `spawn_blocking` 调 `compass_strategy::run_screener` → `RunScreenerResponse` → result_slot 写回 **`screener_result/screener_total/screener_loading/screener_error` 四个 Dynamic** + `request_repaint`）（仿 `backend.rs:36-106`；**B2 修复：`wire_backend` 返回形状改为 3-tuple `(Signal<FetchRequest>, Signal<RunScreenerRequest>, BackendHandle)`，现有调用点 main() 同步适配；`BackendHandle` 增加第二个 `AsyncDispatcher<RunScreenerRequest, RunScreenerResponse>` 字段持有新 runtime（N2 修复）；backend.rs 自身的测试模块（如 `let (work_signal, _backend) = wire_backend(...)` 的解构）同步适配 3-tuple；screener 通道闭包内需 `ParquetReader::new(config.parquet.dir)` 构造 reader 传给 run_screener（N2 修复）**；signal 句柄由 CompassApp 持有后传入 TabViewer/panel）
  - `crates/compass/src/state.rs`：`SharedState` 加 `screener_result: Dynamic<Vec<ScreenerRow>>`、`screener_total: Dynamic<usize>`、`screener_loading: Dynamic<bool>`、`screener_error: Dynamic<Option<String>>`（仿 `state.rs:10-23`）
  - `crates/compass/src/dispatcher.rs`：`RegisteredCitizens` 加 `screener` + `register_citizens` 注册（仿 `dispatcher.rs:27-34`）
  - `crates/compass/src/main.rs`：`CompassApp` 加 `screener: ScreenerPanel`、`run_screener_signal: Signal<RunScreenerRequest>`、`screener_industries: Vec<String>`、`screener_boards: Vec<String>`、**`last_screener_error: Option<String>`（toast 转换检测，仿 `last_error`——B2 修复配套）**、**`last_screener_synced_symbol: String`（r4 修复——反向同步的变更检测标记，初始化为启动时 `shared_state.symbol` 值）** 字段；main() 装配（从 `stock_list` 派生 distinct industries/boards → 注册 → 构造 → 加进 dock_state，仿 `main.rs:97-106`）；**`build_compass_app` 测试 helper（`main.rs:559-607`）同步新增字段**；TabViewer 构造处传 `&mut self.screener` + `&self.run_screener_signal` + `&self.work_signal`（`main.rs:280-287`）
  - 依赖添加：`crates/compass/Cargo.toml` 加 `compass-types`、`compass-strategy`（path 依赖）
  - **Must NOT**：实现条件 UI 或结果表格（后续 todo）；改 core；动现有 FetchBars 通道（仅 wire_backend 返回形状 3-tuple 的适配）
  - 测试（TDD）：egui_kittest `Harness::new_eframe` + `build_compass_app` 渲染不 panic（仿 `main.rs:858-873`）；tab 切换用 `DockState::set_active_tab`（kb/dev/testing.md:246：tab 按钮无 AccessKit label）断言 Screener panel 渲染
  Parallelization: Wave 3 | Blocked by: 3 | Blocks: 5, 6, 7
  References (executor has NO interview context - be exhaustive): `crates/compass/src/tabs.rs:45-65`（TabKind）、`tabs.rs:107-134`（TabViewer）、`crates/compass/src/citizens/logger.rs:11-42`（纯渲染 citizen 样板）、**`crates/compass/src/citizens/chart.rs`（citizen 样板：`show(&mut self, ui, state, app_theme)` 渲染 + 状态读取；注意其本身不发 signal——signal 发送模式在 `main.rs:325-352` 工具栏与 `backend.rs:36-106` wire_backend，两者都参考）**、`crates/compass/src/dispatcher.rs:27-34`（注册）、`crates/compass/src/messages.rs:9-27`（消息类型）、`crates/compass/src/backend.rs:36-106`（wire_backend 模板——返回形状改为 3-tuple）、`crates/compass/src/state.rs:10-39`（SharedState）、`crates/compass/src/main.rs:97-106`（dock 装配）、`main.rs:224-242`（CompassApp 字段——stock_list 已存在）、`main.rs:559-607`（build_compass_app helper——必须同步）、`kb/dev/testing.md:239-247`（egui_kittest + tab 切换限制）、`kb/design/architecture.md:54-115`（citizen 架构）
  Acceptance criteria (agent-executable): `cargo test -p compass` 通过（新 kittest 测试）；`cargo clippy -p compass -- -D warnings` 0 警告；`cargo doc --no-deps` 无 missing_docs；`cargo check --workspace` 通过
  QA scenarios (exact tool + invocation): happy: `Harness::new_eframe` 渲染含新 tab 的 app 无 panic；failure: 未加 screener 字段到 build_compass_app 则编译失败（必须全绿）。证据 `.omo/evidence/task-4-stock-screener.txt`
  Commit: Y | `feat(gui): add TabKind::Screener dock tab with async channel plumbing\n\nref #109`

- [ ] 5. GUI 条件输入 UI + 筛选触发 + loading/toast
  What to do / Must NOT do:
  - `citizens/screener.rs` 的 `show()` 实现左侧条件输入区（ScrollArea）：
    - 行业：可搜索多选（顶部过滤输入框 + 滚动 CheckBox 列表，行业集合来自 show() 参数 `industries`）
    - 交易所：CheckBox 多选（SH/SZ/BJ，仿 `kb/design/symbols.md:9-20`）
    - 板块：CheckBox 多选（**来自 show() 参数 `boards`——由数据动态收集，不硬编码列表；数据为空时显示空列表占位**；注意：中小板已并入主板（2021），以数据实际值为准）
    - 上市时长：ComboBox 预设（不限/≥1年/≥3年/≥5年 → list_years: None/1/3/5）
    - 市值区间：min/max 输入（亿元，f64 Option）
    - 均线：启用 CheckBox + 关系 ComboBox（站上MA20/站上MA60/多头排列）
    - 突破：启用 CheckBox + N 日输入（默认 60，`BreakoutCondition::default()`；**UI 上限 250——超 400 日读取窗口的条件会静默跳过；N 下限 1**）
    - 动量：启用 CheckBox + N 日 + min% + max% 输入（默认 20 日/0%~100%，`MomentumCondition::default()`；**N 上限 250；N 下限 1**）
    - 量能：启用 CheckBox + N 日 + 倍数 X 输入（默认 20/2.0，`VolumeCondition::default()`；**N 上限 80——3N 嵌套基线窗口需 ≤ 读取窗口的交易日数，400 日历日 ≈ 268 交易日（真实数据核验），3×80=240 ≤ 268 留余量；N 下限 1**）
    - 排除退市：CheckBox（默认勾选，`ScreenerQuery::default().exclude_delisted`）
    - "筛选"按钮 → 构造 `ScreenerQuery` → `run_screener_signal.send(request)`（show() 参数）+ `screener_loading.set(true)`
  - loading：`screener_loading` 为 true 时显示 spinner（仿工具栏 loading 处理）
  - toast：`screener_error` 变化时 `toast.push(ToastLevel::Error, msg)`（仿 `main.rs:357-372` 转换检测）——**转换检测逻辑放 `CompassApp::ui`（N4 修复——panel 的 show() 签名无 ToastManager 访问，且 `last_screener_error` 字段属 CompassApp；实现位置：CompassApp::ui 中对比 `screener_error` 与 `last_screener_error`，变化时 push toast）**
  - 条件默认值：`ScreenerQuery::default()` 初始化 panel 内部 query 状态
  - **Must NOT**：实现结果表格或空结果占位（todo 6）；结果持久化
  - 测试（TDD）：kittest 渲染条件区 + 修改某条件值 + 点击筛选按钮 → 断言 `screener_loading` 变 true（仿 `main.rs:752-767` 交互测试模式）
  Parallelization: Wave 3 | Blocked by: 4 | Blocks: 6, 7
  References (executor has NO interview context - be exhaustive): `crates/compass/src/citizens/screener.rs`（todo 4 创建）、`crates/compass/src/widgets/toast.rs:92-99,116-174`（toast API）、`crates/compass/src/widgets/searchable_dropdown.rs`（StockPicker 搜索模式参考）、`crates/compass/src/main.rs:357-372`（toast 转换检测）、`main.rs:325-352`（工具栏加载模式）、`kb/design/symbols.md:9-20`（交易所/板块）、`crates/compass-types/src/lib.rs`（Default 语义）、`.omo/drafts/stock-screener.md`（ScreenerQuery 默认值）
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿（新交互测试）；`cargo clippy -p compass -- -D warnings` 0 警告；无头环境 `cargo test -p compass` 可跑（egui_kittest 纯 CPU）
  QA scenarios (exact tool + invocation): happy: 渲染条件区无 panic + 点筛选触发 loading；failure: 空条件点筛选不 panic。证据 `.omo/evidence/task-5-stock-screener.txt`
  Commit: Y | `feat(gui): add screener condition input panel with filter trigger\n\nref #109`

- [ ] 6. GUI 结果表格 + 表头排序 + 上限 100 计数 + 点击行切换图表
  What to do / Must NOT do:
  - `citizens/screener.rs` `show()` 右侧结果区：`egui::Grid` 或表格 6 列（代码/名称/最新价/20日涨跌幅/市值/行业），数据源 `shared_state.screener_result`；**空结果占位文案"无符合条件的股票"（rows 空且非 loading 时）——归本 todo 独有**（todo 5 不实现）
  - 计数文案：`screener_total` > 100 时显示"共 {screener_total} 只，已显示前 100"，否则显示"共 {screener_total} 只"（数据来自 `shared_state.screener_total`）
  - 表头点击排序（内存排序，字段：symbol/name/latest_price/change_20d/market_cap/industry；点击表头在升/降序间切换；**默认市值降序**）——排序状态存 ScreenerPanel 局部字段
  - 点击结果行 → **`shared_state.symbol.set(裸 6 位代码原样)`（真实 parquet symbol 列即裸码，import 时已去前缀——已核验，无需去前缀逻辑；BLOCKING 决策）** + **复用现有通道：`dispatcher::handle(AppMessage::FetchBars, &shared_state, &work_signal, timeframe)`（timeframe 从 `shared_state.timeframe.get()` 取；经 show() 参数传入的 `work_signal` 派发——BLOCKING 决策，单一机制，勿二选一）**；StockPicker 选中同步：**新增反向同步逻辑（r4 机制定案——不存在可复用代码；用 `CompassApp.last_screener_synced_symbol` 标记检测变更，勿用每帧条件同步或与 picker 当前值比较，二者均会破坏用户下拉选择）**：每帧在 `CompassApp::ui` 中：① 若 `shared_state.symbol.get() != last_screener_synced_symbol` → 更新标记为当前 symbol；② **仅当**新 symbol 为裸 6 位数字码（`len==6 && all ascii_digit`）→ 同步 `stock_picker.selected_symbol/selected_name`（**name 来源：从 `shared_state.screener_result` 中按 symbol 查 row.name，查不到回退 `stock_list` 查找，再查不到留空——r4 定案**）+ **`selected_exchange = ""`（r4 blocking 修复——否则残留旧 exchange（如 "SZ"）会导致点击 SH/BJ 股票后 picker 显示 "SZ | 600519" 错误 + 下次工具栏 Fetch 产生 "sz.600519" 错误前缀 → NoData；清空后工具栏发裸码与 parquet 键一致）**；③ prefixed symbol（工具栏路径写的 "sz.000001"，main.rs:329-344）不触发任何 picker 修改（guard 阻断 + 标记仍更新，避免下一帧重复处理——r4 修复）；**市值列：`market_cap == 0.0` 渲染 "—"（B4 修复：显示规则从 todo 3 移入本 todo 实现）**
  - **Must NOT**：结果持久化；把结果写进 config；自行直接调 work_signal（必须经 dispatcher::handle）
  - 测试（TDD）：kittest 注入假 `screener_result`（`Dynamic<Vec<ScreenerRow>>` set 假数据）→ 断言表格渲染行数/点击市值表头排序翻转/点击行触发 `shared_state.symbol` 变化；**kittest：注入 0.0 market_cap 行 → 市值列渲染 "—"（B4 断言）**；**kittest：设置 prefixed symbol（"sz.000001"）→ 断言 picker 不被同步且 `last_screener_synced_symbol` 已更新（B3 guard + r4 标记断言）**；**kittest：点击裸码行（600519）→ 断言 picker `selected_exchange` 被清空（B1 修复断言，防 stale exchange 错误前缀）**；**kittest：用户在 dropdown 选择一只股票（symbol 未变化）→ 断言 picker 选择保留、`selected_exchange` 不被清空（r4 机制断言——防每帧同步破坏下拉选择）**
  Parallelization: Wave 3 | Blocked by: 5 | Blocks: 7
  References (executor has NO interview context - be exhaustive): `crates/compass/src/citizens/screener.rs`、`crates/compass/src/state.rs:10-23`（Dynamic API：get/set）、`crates/compass/src/dispatcher.rs:54-81`（handle/AppMessage）、`crates/compass/src/main.rs:325-352`（symbol set + FetchBars 模式——**复用此路径**）、`crates/compass/src/widgets/searchable_dropdown.rs:24-49`（StockPicker selected_symbol 字段——先 verify 字段名）
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿；`cargo clippy -p compass -- -D warnings` 0 警告；排序切换断言（默认降序 → 点击后升序）
  QA scenarios (exact tool + invocation): happy: 假数据 3 行渲染 + 点击市值表头排序翻转；happy: 点击行 → symbol 变为 6 位代码；failure: 空结果不 panic 显示占位。证据 `.omo/evidence/task-6-stock-screener.txt`
  Commit: Y | `feat(gui): add screener results table with sorting and chart linkage\n\nref #109`

- [ ] 7. GUI config 持久化: `[screener]` 节 load/save
  What to do / Must NOT do:
  - **方案偏离声明（BLOCKING 修正）**：本 todo **偏离 draft D3**（D3 原为"AppConfig 加 Serialize + [screener] 节"）——因 `ScreenerQuery` 在 compass-types 而 `AppConfig` 在 compass-core，若 AppConfig 内嵌 `ScreenerQuery` 将产生 **core→types 依赖**，违反依赖方向契约（todo 2 已确立 core 不得依赖 types）。故改为：**不触碰 AppConfig**，GUI 单独解析/写入 config.toml 的 `[screener]` 节。此偏离需用户在审批时知情（已列入 TL;DR "Decisions to sanity-check"）
  - `crates/compass/src/main.rs`：`load_config()` 扩展——解析 config.toml 时组合结构 `#[derive(Deserialize)] struct FullConfig { #[serde(flatten)] app: AppConfig, #[serde(default)] screener: ScreenerQuery }`（**已核验：toml 0.8 对 `#[serde(flatten)]` 序列化/反序列化均可用，无需 fallback；AppConfig 所有字段含 `#[serde(default)]`，flatten + 部分节加载路径可靠**）；**`load_config` 返回类型改为 `FullConfig`，现有调用点适配（N1 修复——实测需改：`load_stock_list(&config)`→`&config.app`（main.rs:82）、`wire_backend(config.clone(), …)`→`config.app.clone()`（main.rs:86）、`&config.theme`→`&config.app.theme`（main.rs:110）；**测试 `main.rs:616-716` 的 `config.app.default_symbol` → `config.app.app.default_symbol`（N1 修复——`config.app` 已是 AppConfig，需再进一层到 AppSection）、`config.parquet.dir` → `config.app.parquet.dir`**；**补漏（r4）：`SharedState::new(&config.app.default_symbol, &config.app.default_timeframe)`（main.rs:64-65）与 `StockPicker::new(&config.app.default_symbol, …)`（main.rs:108）同样需改 `config.app.app.*`**），仿 `main.rs:178-199` 错误回退语义
  - **ScreenerQuery serde 加固（Oracle B 修复配套）**：todo 2 的 `ScreenerQuery` 所有字段加 `#[serde(default)]`（容器级或逐字段）——手写/部分编辑的 `[screener]` 节缺字段时不致反序列化失败触发 load_config 整体回退
  - 新增 `fn save_screener_config(query: &ScreenerQuery) -> Result<(), String>`：读现有 config.toml 全文 → 更新 `[screener]` 节 → 序列化 → `fs::write` 回 `~/.config/compass/config.toml`（路径与 load 一致）。**保存机制明确：用 `toml::Value` 读全文 → 插入/替换 `screener` 表 → `toml::to_string` 写回（接受注释丢失与节重排——文档注明）；config.toml 不存在时新建仅含 `[screener]` 节的文件（N9 修复——不会因缺少 app/parquet 节导致下次加载回退，因 AppConfig 字段均有 serde default）**
  - 时机：**筛选按钮点击时保存当前 query**（`ScreenerPanel` 经 CompassApp 提供 `save_screener_config` 调用路径——show() 增加 `on_save: &dyn Fn(&ScreenerQuery)` 回调参数；**回调闭包存储为 `CompassApp` 字段 `screener_save_cb: Box<dyn Fn(&ScreenerQuery)>`（N3 修复——临时闭包放 TabViewer 每帧构造会 borrow-check 失败），由 CompassApp 构造时创建 `Box::new(|q| { let _ = save_screener_config(q); })` 并在 TabViewer 构造处传 `&*self.screener_save_cb`**）
  - 启动恢复：CompassApp 构造时从 config 读取的 `ScreenerQuery` 初始化 ScreenerPanel 内部 query 默认值
  - **Must NOT**：保存结果集；改 AppConfig 结构；把 screener 条件塞进 AppConfig（保持 core 无 types 依赖）
  - 测试（TDD）：临时 HOME 环境（`HOME_LOCK` 串行锁，`main.rs:614`）+ 写 config → `save_screener_config` → 重读断言相等；缺 config 文件回退默认
  Parallelization: Wave 3 | Blocked by: 6 | Blocks: 9, 10
  References (executor has NO interview context - be exhaustive): `crates/compass/src/main.rs:178-199`（load_config 模板）、`main.rs:614`（HOME_LOCK）、`crates/compass-core/src/model.rs:175-266`（AppConfig——**勿改**）、`.omo/drafts/stock-screener.md`（D3 决策及其偏离声明）、`kb/user/config.md:1-46`（config 文档——本 todo 后需更新）、`crates/compass-types/src/lib.rs`（ScreenerQuery serde）
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿（持久化往返测试）；`cargo clippy -p compass -- -D warnings` 0 警告
  QA scenarios (exact tool + invocation): happy: 临时 HOME 写 query → 重读相等；failure: HOME 无 config 文件 → 默认值不 panic。证据 `.omo/evidence/task-7-stock-screener.txt`
  Commit: Y | `feat(gui): persist screener conditions to config.toml [screener] section\n\nref #109`

- [ ] 8. CI 覆盖率脚本: `check-coverage.sh` 增加新 crate 门槛
  What to do / Must NOT do:
  - `scripts/check-coverage.sh`：在 `:61-63` 三个 check 行后追加 `check "compass-strategy" "select(.filename | contains(\"/crates/compass-strategy/\"))"` 与 `check "compass-types" "select(.filename | contains(\"/crates/compass-types/\"))"`
  - 验证 `.github/workflows/ci.yml:107-134` coverage job 无需改动（调用同一脚本，llvm-cov workspace 级 JSON 含全部 crate）
  - **Must NOT**：改覆盖率阈值（80% 不变）；改其他 crate 的 check
  - 测试（行为检查，非文本计数）：本地 `cargo llvm-cov --json --summary-only > cov.json && bash scripts/check-coverage.sh 80 cov.json` 验证脚本对新 crate 生效且不破坏现有检查；构造缺少新 crate 的 cov.json → 脚本 FAIL（新 crate MISSING 触发 fail=1）
  Parallelization: Wave 3（与 Todo 4 并行） | Blocked by: 2, 3 | Blocks: 10
  References (executor has NO interview context - be exhaustive): `scripts/check-coverage.sh:57-64`（check 函数与三 crate 行）、`.github/workflows/ci.yml:107-134`（coverage job）、`kb/dev/testing.md:218-237`（覆盖率说明）
  Acceptance criteria (agent-executable): `bash scripts/check-coverage.sh 80 cov.json` 退出码 0（本地生成 cov.json）；缺失新 crate 的 cov.json 触发 FAIL（退出码 1）
  QA scenarios (exact tool + invocation): happy: 完整 cov.json 全绿（退出码 0）；failure: 构造缺失新 crate 的 cov.json → 脚本 FAIL（退出码 1）。证据 `.omo/evidence/task-8-stock-screener.txt`
  Commit: Y | `ci: enforce coverage thresholds for compass-strategy and compass-types\n\nref #105`

- [ ] 9. 文档同步: kb/ 文件 + AGENTS.md
  What to do / Must NOT do:
  - `kb/design/architecture.md`：crate 关系图（`:19-49`）加 compass-strategy/compass-types；citizen 表格（`:74-115`）加 ScreenerPanel
  - `kb/design/data-providers.md`：新增 `fetch_cross_section` + `CrossSectionBar` 说明（横截面原语章节）；**记录 `tradedate` 列实际类型为 TIMESTAMP（r4 修复——文档 `:120-125` 只记列名未记类型，补全防止后续误用 `%F` 解析）**；决策记录表（`:282-290` 追加本次决策（fetch_cross_section 位置、CrossSectionBar 字段、市值单位、窗口语义——master 同步后行号更新）
  - `kb/user/gui.md`：Screener tab 使用说明（条件、结果列、排序、图表联动、持久化）
  - `kb/user/config.md`：`[screener]` 节文档（字段 + 默认值）
  - `AGENTS.md`：workspace crate 列表 + 覆盖率门槛描述（**归本 todo 独有，todo 8 不碰 AGENTS.md**）
  - `kb/dev/testing.md`：如需（新增测试模式无新框架则注明无需更新）
  - `kb/dev/reflections.md`：本项不做（todo 10 处理）
  - **Must NOT**：修改 kb/github/ 下 bot 角色文件；编造未实现功能文档
  - 验证：`cargo doc --no-deps` 仍无警告；文档与代码行为一致（人工核对清单）
  Parallelization: Wave 4 | Blocked by: 1-7 | Blocks: 10
  References (executor has NO interview context - be exhaustive): `.opencode/skills/docs/SKILL.md`（变更→kb 映射表）、`kb/design/architecture.md:19-49,74-115`、`kb/design/data-providers.md:1-5,282-290`（master 同步后行号更新）、`kb/user/gui.md`、`kb/user/config.md:1-46`、`AGENTS.md`（Workspace/Crates 相关章节）、`kb/dev/testing.md`
  Acceptance criteria (agent-executable): 各 kb 文件 diff 审查通过；`kb/design/` 相关文件含 `## 决策记录` 章节（缺失则补齐，门禁 Step 4c）
  QA scenarios (exact tool + invocation): 无运行测试；git diff 核对清单逐条勾选。证据 `.omo/evidence/task-9-stock-screener.txt`
  Commit: Y | `docs: sync kb/ and AGENTS.md for screener feature\n\nref #105`

- [ ] 10. 实现后反思: `/reflect` 追加 `kb/dev/reflections.md`
  What to do / Must NOT do:
  - 调用 `/reflect`（reflect skill），按 `kb/dev/reflections.md` 格式追加条目：做了什么、哪里出错、教训、User corrections（本 session 无用户纠正则注明无）
  - 趋势分析：检查最近 10 条记录识别重复模式
  - **Must NOT**：跳过（compass-workflow 强制）；编造流程事实（git 命令客观验证）
  Parallelization: Wave 5 | Blocked by: 1-9 | Blocks: —
  References (executor has NO interview context - be exhaustive): `.opencode/skills/reflect/SKILL.md`、`kb/dev/reflections.md`（格式与历史条目）、AGENTS.md（Reflection 章节）
  Acceptance criteria (agent-executable): `kb/dev/reflections.md` 有新条目（git diff 确认）
  QA scenarios (exact tool + invocation): git diff 显示追加条目；条目含要求的小节。证据 `.omo/evidence/task-10-stock-screener.txt`
  Commit: Y | `docs: add reflection for stock-screener implementation\n\nref #105`

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA（由用户执行：实际打开 GUI 验证选股流程）
- [ ] F4. Scope fidelity

## Commit strategy
- 每个 todo 完成后立即 commit（不等待）；`git add <变更文件>` 仅暂存本 todo 相关文件（**不 `git add .`**）
- 消息格式：`<type>(<scope>): <summary>\n\nref #N`——映射：Todo 1→#106、Todo 2→#107、Todo 3→#108、Todo 4-7→#109、Todo 8/9/10→#105（epic 收尾）
- **只用 `ref #N`，绝不用 `fixes #N`/`closes #N`**（避免自动关 issue）
- commit 后立即运行 `/review-work`（5 agent 并行审查）；发现问题修复后重新 commit（最多 2 轮）
- **绝不自动 push**——等用户明确说 "push"
- 每个 commit 必须含 `ref #N`（pre-push hook 强制）
- 本次实现全部在本 worktree（feat/stock-screener 分支）内进行；master 只允许 docs/lint/typo/反思类直推

## Success criteria
- [ ] `cargo test`（workspace）全绿，含 core/strategy/types/GUI 新增测试
- [ ] `cargo clippy -- -D warnings`、`cargo fmt --check`、`cargo doc --no-deps` 全过
- [ ] GUI 可打开 Screener tab：条件输入 → 筛选 → 结果表格（排序/上限/计数文案）→ 点击行切换图表
- [ ] 空条件 = 全市场（非退市）市值降序前 100，计数显示正确 total
- [ ] 条件持久化：重启后恢复
- [ ] 4 个子 issue（#106-#109）各自 commit 引用正确；CI 覆盖率脚本覆盖新 crate
- [ ] kb/ 文档同步完成；`kb/dev/reflections.md` 追加反思
