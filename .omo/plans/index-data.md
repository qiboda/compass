# index-data - Work Plan

## TL;DR (For humans)

**What you'll get:** Compass 获得指数数据支持——从东财采集交易所官方指数、概念板块、行业板块（约 500 个标的）的日线行情，存入独立 Dolt 表，经数据管线导出本地 Parquet，并在 GUI 中新增「大盘」页签展示大盘概览和板块轮动列表，任何指数/板块都能像股票一样搜索并查看 K 线。

**Why this approach:** 三件事撑起整个方案——① 指数数据**独立建表/独立 Parquet**（尊重既有「股票数据剔除指数」的决策，不污染选股评分）；② GUI 数据路由用**双文件回退**（先查股票文件，查不到再查指数文件），不改任何现有消息契约，股票功能零回归；③ 板块代码用 `BK` 前缀（保留东财原始代码），采集器零转换。

**What it will NOT do:** 不拉分钟线；不自己计算指数（拉不到就跳过）；不做任何在线数据回退（GUI 只读本地）；不把指数混入股票数据；不在本阶段让选股/回测消费指数数据。

**Effort:** Large
**Risk:** Medium - 东财板块指数历史仅约 2019 年起、接口限流需 host 轮换；GUI 大盘 tab 是新通道 + 新页签，改动面最大
**Decisions to sanity-check:** ① 板块符号用 `BK` 前缀（vs 自定义命名空间）② 双 parquet 回退路由（vs 改消息契约）③ 大盘 tab 入口 + 核心指数 6 只白名单 ④ index_basic 名称表（新表，picker/板块列表依赖）⑤ 官方指数硬编码清单（约 30 只）

Your next move: approve, or run a high-accuracy review. Full execution detail follows below.

---

> TL;DR (machine): Large effort, Medium risk — 4 batches × 8 todos：东财指数采集器+双 Dolt 表 → import-compass 导出 → BK 前缀符号体系 → GUI 大盘 tab + 双 parquet 路由；真实数据冒烟 + F1-F4 收尾。

## Scope
### Must have
- **C1 数据采集**：Dolt `index_daily` 表 + `index_basic` 表（均建在 **compass_data** 库 `/data/compass-data/compass_data`，非 investment_data——issue #255 body 的「主数据源」表述仅指项目整体架构，建表目标明确为 compass_data）+ `collectors/fetch_index_daily.py`（东财 push2his kline API：官方指数 `secid={1|0}.{code}`、板块 `90.BKxxxx`、`klt=101`、`fqt=0`；板块列表 clist `fs=m:90 t:3/t:2 f:!50` 取 f12/f14 名称为 index_basic 数据源）+ `main.py do_sync()` 第 11 步 + `fetch`/`import` CLI choices + 盘后增量（`last_report_date` 驱动）+ 新标的自动补全量历史 + Python tests
  - `index_daily` DDL：`(symbol VARCHAR(20) NOT NULL, trade_date DATE NOT NULL, index_type VARCHAR(20) NOT NULL, open/close/high/low/volume/amount DOUBLE, update_date DATE, PRIMARY KEY (symbol, trade_date))`——**与 DuckDbProvider 查询列对齐**（duckdb.rs:534 查询 trade_date/open/high/low/close/volume/adjclose 7 列 + parquet 回退含 amount 8 列，parquet 导出时补 `adjclose=close`）
  - `index_basic` DDL：`(symbol VARCHAR(20) NOT NULL PRIMARY KEY, name VARCHAR(100), index_type VARCHAR(20))`——picker 与板块列表的名称/类型来源
  - 官方指数清单：**硬编码枚举**（对齐 akshare index_zh_em 做法，约 30 只主流官方指数：上证指数 SH000001/深证成指 SZ399001/创业板指 SZ399006/沪深300 SH000300/中证500 SH000905/中证1000 SH000852 等），采集器逐个 `secid` 拉取；板块清单从 clist 全量发现（新板块自动入库）
- **C2 数据管线**：`import-compass` 新增 `CompassTable::IndexDaily` + `CompassTable::IndexBasic`（`import_append_table` 模式，index_daily PK (symbol, trade_date) 增量 merge；index_basic 全量覆盖）+ 导出 `index_daily.parquet`（含 `index_type` + `adjclose=close` 占位列）+ `index_basic.parquet`（symbol/name/index_type）+ `export` 到 DuckDB 路径 + 数据质量校验（落库行数/日期范围/数值合理性）+ 真实数据冒烟 + Rust tests
- **C3 符号体系**：`BK` + 4 位前缀命名空间——`parse_explicit_prefix`/`infer_exchange_prefix`/`exchange_of_symbol`（compass-core symbol.rs）、`validate_symbol`（parquet.rs:34）、`strip_exchange_prefix`/`normalize_query`（searchable_dropdown.rs:61-89，compass-ui）、`sync_picker_from_symbol`（main.rs:897）、`normalize_symbol_filter`（import_dolt.rs:11-40）+ `kb/design/symbols.md` 更新（BK 前缀 + 指数符号 + ref #201 关系）
- **C4 GUI**：新 dock tab「大盘」（`TabKind::Market` + 核心指数 Card 6 只白名单 + 板块 DataTable + Segmented 行业/概念/官方指数 + 手动刷新）+ 工具栏 `StockPicker` 合并标的（stock + index_basic ~6500）+ `DuckDbProvider::fetch_bars` 双 parquet fallback（stock_daily → index_daily）+ **第四条 `RunIndexSnapshotRequest` 通道**（SEPA 同构：backend.rs 新增 AsyncDispatcher + messages/state 扩展，index_snapshot 三件套镜像 sepa_*——此通道是 design 与 plan 一致采纳的方案，draft 决策表 #5 的「零消息契约改动」表述已过时，以本 plan 为准）+ 前复权 Tag 对指数/板块隐藏 + `index.*` i18n（zh/en 对称）+ GUI tests
- **文档同步**：`kb/design/data-providers.md`（index_daily/index_basic schema + 双 parquet 路由）、`kb/user/cli.md`（import-compass 新表）、`kb/design/symbols.md`（BK 前缀 + 指数符号约定 + ref #201 关系）
- **决策记录**：`kb/design/` 涉及文件补 `## 决策记录` 章节（BK 前缀命名空间、双 parquet 路由、大盘 tab 入口、白名单、index_basic 名称表）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 分钟线数据（决策 4：日线 + 聚合周/月线，不拉分钟线）
- 自算指数兜底（决策 9：官方指数算法复杂需自由流通股本，板块拉不到也跳过）
- 在线数据回退（GUI 只读本地 Parquet）
- 侧栏自选支持指数/板块（V2，需扩展 Sidebar BK Tag 配色）
- `dispatcher::handle` 365 天周期范围修改（V2，独立优化）
- 板块成分股列表 GUI 展示（V2）
- SEPA 评分/回测消费指数数据（本 epic 只做数据 + GUI 基础展示，消费留后续 issue）
- `index_daily` 混入 `stock_daily.parquet`（ref #201 剔除约定：独立建表/独立 parquet）
- 修改 `import`（import_dolt.rs）的股票导出逻辑（INDEX_SYMBOLS 剔除保持不变）
- 不新增 UI 依赖/组件（全部复用现有 24 组件）
- 禁止 `unwrap()`/`as` 类型逃逸；禁止覆盖已有测试

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: TDD（RED → GREEN）+ 既有框架（Python `pytest --cov=.` ≥95%；Rust `#[cfg(test)]` + 覆盖率门槛 compass-data 95% / compass 90% / compass-core 95%）
- Evidence: `.omo/evidence/`（ulw-loop 外），`task-<N>-index-data.<ext>` per todo
- Python 测试：内存 Dolt（COMPASS_DATA_DIR tempdir）+ stub AsyncSession（模拟东财响应）；Rust：内存 DuckDB + tempdir parquet
- 真实数据冒烟（提交前强制）：`import-compass --table index_daily` 落库行数、日期范围（官方指数全历史 vs 板块约 2019-12 起）、数值合理性（点位/涨跌幅区间）写入 `.omo/evidence/`

## Execution strategy
### Parallel execution waves
- **Wave 1**（独立，可并行）：T1 C1 采集器 + Dolt 表（Python，index_daily + index_basic）；T2 C3 符号体系 BK 前缀（Rust core，测试先行）——互不依赖
- **Wave 2**（依赖 C1 完成但 T2 独立）：T3 C2 import-compass IndexDaily/IndexBasic + export；T4 C3 的 GUI 消费点（sync_picker_from_symbol 等）
- **Wave 3**（依赖 C2 + C3）：T5 C4 双 parquet 路由（DuckDbProvider）；T6 C4 大盘 tab（TabKind::Market + 快照通道 + i18n）；T7 C4 工具栏合并 + 前复权 Tag
- **Wave 4**（文档，可与实现并行但决策记录需随实现）：T8 文档同步 + 决策记录
- **Wave 5**（收尾）：真实数据冒烟 + F1-F4 final verification

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| T1 C1 采集器+Dolt（index_daily+index_basic） | — | T3 | T2 |
| T2 C3 符号体系 core | — | T4, T5, T6, T7 | T1 |
| T3 C2 import-compass（IndexDaily+IndexBasic+export） | T1 | T5, T6 | T2 |
| T4 C3 GUI 消费点 | T2 | T7 | T3 |
| T5 C4 双 parquet 路由 | T2, T3 | T6 | T4 |
| T6 C4 大盘 tab | T3, T5 | — | T7 |
| T7 C4 工具栏合并 | T2, T4 | — | T6 |
| T8 文档+决策记录 | T1-T7（内容） | — | T1-T7（可先起草） |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #256 | C1 采集器 + Dolt index_daily/index_basic（fetch_index_daily.py + do_sync 第 11 步 + CLI） | — |
| pending | #257 | C2 import-compass IndexDaily/IndexBasic + export + 数据质量校验 | #256 |
| pending | #258 | C3 BK 前缀符号体系（symbol.rs/validate_symbol/normalize_query/sync_picker） | #256 |
| pending | #259 | C4 GUI 大盘 tab + 工具栏合并 + 双 parquet 路由 | #257, #258 |
- [ ] 1. C1: `collectors/fetch_index_daily.py` — 新增 Dolt `index_daily` + `index_basic` 表与东财采集器（官方/概念/行业三类）
  What to do / Must NOT do: 新建 `collectors/fetch_index_daily.py`，仿 `fetch_main_flow.py` 结构（run() → CSV → import_to_dolt() → `common.import_replace_table`）。DDL 见 Must have 的 schema 契约（index_daily PK (symbol, trade_date) + index_type 列；index_basic PK symbol + name + index_type）。官方指数用硬编码清单（约 30 只，含 SH000001/SZ399001/SZ399006/SH000300/SH000905/SH000852 等）逐个 push2his kline 拉全量（`beg=0&end=20500000`, `klt=101`, `fqt=0`）；板块清单从 clist `fs=m:90 t:3`（概念）/`t:2`（行业）`f:!50` 分页发现，名称写入 index_basic；板块 K 线 `secid=90.BKxxxx`。增量：`last_report_date` 短路（common.py:172-186），新标的自动补全量。`main.py do_sync()` 加第 11 步 + `data_updates` 更新循环（main.py:474-483）+ `fetch`/`import` CLI choices（main.py:496/503）。限流：host 轮换 + 秒级间隔（handoff 调研）。
  Must NOT do: 不自算指数（决策 9）；不拉分钟线（决策 4）；不写 investment_data（写 compass_data `/data/compass-data/compass_data`）；采集器不修改现有 10 个采集器。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T3
  References: collectors/fetch_main_flow.py:1-282（模板：_exchange_prefix/_num/run/import_to_dolt）、collectors/fetch_concept_member.py:61-200（board list clist 分页模式）、collectors/common.py:172-186（last_report_date）、198-301（import_replace_table merge）、collectors/main.py:400-484（do_sync）、496-503（CLI choices）、.omo/handoff.md（东财接口调研：push2his secid/klt/fqt/clist fs）
  Acceptance criteria (agent-executable): `uv run pytest collectors/tests/test_index_daily.py -q` 全绿（mock 东财响应 + tempdir Dolt COMPASS_DATA_DIR）；`uv run pytest collectors/tests/ --cov=. --cov-fail-under=95 -q` 通过；`main.py` CLI choices 含 index_daily
  QA scenarios: happy — 模拟 3 个官方指数 + 2 板块响应 → CSV 行数正确、index_basic 含名称、do_sync 第 11 步执行；failure — 东财 429/空响应 → 跳过并记录（不崩溃、不写半截数据）。Evidence `.omo/evidence/task-1-index-data.txt`
  Commit: Y | feat(collectors): index_daily/index_basic 采集器（东财三类指数 + 板块）
- [ ] 2. C3: 符号体系 `BK` 前缀扩展（compass-core symbol.rs + parquet.rs validate_symbol + import_dolt.rs normalize_symbol_filter）
  What to do / Must NOT do: `parse_explicit_prefix`（symbol.rs:15-31）加 `BK` 分支（"BK0475" → ("BK","0475")）；`infer_exchange_prefix`（37-49）对非 6 位数字返回 None 不变（BK 4 位天然落 None）；`exchange_of_symbol`（53-61）对 BK 前缀原样返回；`validate_symbol`（parquet.rs:34-38）加 BK+4 位数字分支；`normalize_symbol_filter`（import_dolt.rs:11-40）加 BK+4 位校验。**测试先行**：先写 `#[cfg(test)]` 失败测试（BK0475 通过全链路）。
  Must NOT do: 不改股票 SH/SZ/BJ 行为；不把 BK 混入 `stock_basic`/`stock_daily`；不改 `INDEX_SYMBOLS` 剔除逻辑（import_dolt.rs:98-100，股票导出保持无指数）。
  Parallelization: Wave 1 | Blocked by: — | Blocks: T4, T5, T6, T7
  References: crates/compass-core/src/data/symbol.rs:15-61（parse/infer/exchange + 测试 64-139）、crates/compass-core/src/data/parquet.rs:31-38（validate_symbol）、crates/compass-data/src/import_dolt.rs:11-40（normalize_symbol_filter）、.omo/designs/index-data.md 符号体系章节（BK+4 位决策记录）
  Acceptance criteria (agent-executable): `cargo test -p compass-core symbol` 与 `cargo test -p compass-data` 全绿；新增测试断言 `parse_explicit_prefix("BK0475")==("BK","0475")`、`validate_symbol("BK0475")` OK、`validate_symbol("BK047")` Err、`validate_symbol("BK04755")` Err
  QA scenarios: happy — BK0475 全链路解析/校验/规范化；failure — 3 位/5 位/非数字 BK 拒绝、BK 前缀与其他前缀组合不破坏现有行为。Evidence `.omo/evidence/task-2-index-data.txt`
  Commit: Y | feat(symbols): BK 前缀命名空间（板块符号解析/校验/规范化）
- [ ] 3. C2: `import-compass` 新增 `CompassTable::IndexDaily`/`IndexBasic` 导出 parquet + export 到 DuckDB
  What to do / Must NOT do: `CompassTable` enum（import_compass.rs:22-57）加 `IndexDaily`（增量 import_append_table 模式，PK (symbol, trade_date)，table_name="index_daily"）+ `IndexBasic`（全量覆盖，仿 ConceptMember 模式 import_compass.rs:446-460）。index_daily 导出 SQL `SELECT symbol, index_type, tradedate, open, high, low, close, volume, amount, adjclose FROM index_daily ORDER BY symbol, trade_date`（adjclose=close 占位）；`import_compass.rs` 测试内 schema 常量对齐。export.rs（export.rs:9-75）加 index_daily/index_basic 到 DuckDB 路径（save_* 或泛型）。数据质量校验：落库行数 vs Dolt 源、日期范围、点位数值合理性（0 < close < 100000 且非负）。真实数据冒烟见 T8/冒烟步骤。
  Must NOT do: 不把 index_daily 混入 stock_daily.parquet；不改 import_dolt.rs 的股票导出；adjclose 必须 = close（不能缺列，DuckDbProvider 查询固定 7 列 duckdb.rs:534）。
  Parallelization: Wave 2 | Blocked by: T1 | Blocks: T5, T6
  References: crates/compass-data/src/import_compass.rs:22-57（enum+FromStr）、345-437（import_append_table）、446-460（ConceptMember 全量覆盖）、crates/compass-data/src/export.rs:9-75（run_export）、crates/compass-core/src/data/duckdb.rs:534（查询列契约）
  Acceptance criteria (agent-executable): `cargo test -p compass-data` 全绿；tempdir 测试：造 index_daily Dolt 表 → `import-compass --table index_daily` 生成 parquet，断言列含 index_type/adjclose、行数 = 源
  QA scenarios: happy — 增量 merge（--since 后新行并入不丢旧行）；failure — 空源表跳过不崩溃、行数丢失报错（row count mismatch 路径 import_compass.rs:420）。Evidence `.omo/evidence/task-3-index-data.txt`
  Commit: Y | feat(import-compass): index_daily/index_basic 表导出 parquet + export 到 DuckDB
- [ ] 4. C3-GUI: 符号消费点扩展（searchable_dropdown.rs normalize_query/strip_exchange_prefix + main.rs sync_picker_from_symbol）
  What to do / Must NOT do: `normalize_query`（searchable_dropdown.rs:61-76）与 `strip_exchange_prefix`（80-89）加 `bk` 前缀（"BK0475" → q_code "0475"）；`sync_picker_from_symbol`（main.rs:897-919）接受 BK+4 位（is_prefixed 判断扩展）；`matches_query` 加 "0475" 匹配 BK0475 的测试。测试先行。
  Must NOT do: 不改 GUI 现有股票行为；BK 前缀在 `format_display` 中不得重复（"BK | 0475" 而非 "BK | BK0475"）。
  Parallelization: Wave 2 | Blocked by: T2 | Blocks: T7
  References: crates/compass-ui/src/widgets/searchable_dropdown.rs:57-101（normalize_query/strip_exchange_prefix/matches_query）、crates/compass/src/main.rs:897-919（sync_picker_from_symbol + 测试 2788-2840）、.omo/designs/index-data.md 工具栏章节
  Acceptance criteria (agent-executable): `cargo test -p compass-ui searchable_dropdown` 与 `cargo test -p compass` 全绿；新增测试：query "0475" 匹配 BK0475、query "bk0475" 匹配、sync_picker_from_symbol("BK0475") 回显 name/exchange
  QA scenarios: happy — 搜索 "0475"/"BK0475"/"半导体" 都命中 BK0475；failure — 空查询不匹配、非 BK 前缀不受影响。Evidence `.omo/evidence/task-4-index-data.txt`
  Commit: Y | feat(gui): BK 前缀搜索/回显消费点（normalize_query/sync_picker_from_symbol）
- [ ] 5. C4: `DuckDbProvider::fetch_bars` 双 parquet fallback（stock_daily → index_daily）
  What to do / Must NOT do: duckdb.rs fetch_bars（513-639）内存表 miss 时先查 stock_daily.parquet（现有 564-640），结果为空再查 index_daily.parquet（同样 SQL 形状，tradedate/open/high/low/close/volume/adjclose/amount，含 adjclose=close）；1w/1M date_trunc 聚合（647+）对 index 路径复用。测试：指数符号查 stock 空 → index 命中；股票符号不误入 index。
  Must NOT do: 不改 FetchRequest 消息契约（backend.rs:63-105、messages.rs）；不改 dispatcher 365 天范围（V2）；前复权逻辑不应用到指数（factor=1.0 恒等，因 adjclose=close）。
  Parallelization: Wave 3 | Blocked by: T2, T3 | Blocks: T6
  References: crates/compass-core/src/data/duckdb.rs:513-639（fetch_bars parquet fallback）、642-700（timeframe 聚合）、crates/compass/src/backend.rs:76-105（FetchRequest 处理）、crates/compass/src/dispatcher.rs:63-90（365 天范围）
  Acceptance criteria (agent-executable): `cargo test -p compass-core data::duckdb` 全绿；tempdir 测试：index_daily.parquet 含 SH000001 → fetch_bars("SH000001") 返回 bars；stock_daily.parquet 不含 SH000001 但含 SZ000001 → fetch_bars("SZ000001") 返回股票 bars
  QA scenarios: happy — SH000001 双文件路由命中 index；failure — 两个文件都没有的符号 → 空 bars + 现有 error 路径；1w/1M 聚合对 index 正确（SUM volume）。Evidence `.omo/evidence/task-5-index-data.txt`
  Commit: Y | feat(core): DuckDbProvider 双 parquet fallback（stock_daily → index_daily）
- [ ] 6. C4: 大盘 tab（TabKind::Market + RunIndexSnapshotRequest 通道 + MarketCitizen + 核心指数 Card + 板块 DataTable + i18n）
  What to do / Must NOT do: tabs.rs TabKind（53-92）加 `Market` 变体（title "tab.market"、icon TRENDING_UP、citizen_id "market"）；dispatcher.rs register_citizens（29-43）注册 market；backend.rs wire_backend（53-270）加第四条 AsyncDispatcher `RunIndexSnapshotRequest`/`RunIndexSnapshotResponse`（SEPA 同构 70-72/131-160）；handler 用 ParquetReader 直读 index_daily.parquet + index_basic.parquet，窗口函数 ROW_NUMBER 取每标的最后 2 根算点位+涨跌幅（design SQL）；state.rs 加 `index_snapshot`/`index_snapshot_loading`/`index_snapshot_error`（镜像 sepa_*）；新 citizen `citizens/market.rs`：核心指数 Card（6 只白名单：SH000001/SZ399001/SZ399006/SH000300/SH000905/SH000852）+ Segmented（行业/概念/官方指数，本地内存过滤）+ DataTable（名称/代码/最新/涨跌幅/成交额，默认涨跌幅降序）+ 手动刷新按钮 + EmptyState；行点击 `dispatch_symbol_fetch`（不切 tab，与 SEPA 一致）；i18n `index.*` 命名空间 zh/en 对称（locales/zh.yml、en.yml）。
  Must NOT do: 不新增 UI 依赖/组件（全复用现有 24 组件）；不加自动刷新（纯手动，与 SEPA 一致）；不切 tab；不重排三栏布局。
  Parallelization: Wave 3 | Blocked by: T3, T5 | Blocks: —
  References: crates/compass/src/tabs.rs:53-92（TabKind）、crates/compass/src/dispatcher.rs:29-43（register_citizens）、crates/compass/src/backend.rs:70-72/131-160（SEPA 通道同构）、crates/compass/src/citizens/sepa.rs:722+（kittest 模式）、crates/compass-i18n/locales/zh.yml:136-145（sepa 键结构）、.omo/designs/index-data.md 大盘 tab 章节（Card/DataTable/Segmented 规格 + 交互效果表）
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿（kittest：三 tab 渲染 / Segmented 切换 / 行点击联动 / 空态）；i18n 键完整性测试通过（zh/en 对称）
  QA scenarios: happy — 大盘 tab 渲染核心指数 Card + 板块列表（涨跌幅降序）；点击板块行 → chart 加载 BK K 线；failure — index_daily.parquet 缺失 → EmptyState「暂无指数数据」，无 panic。Evidence `.omo/evidence/task-6-index-data.txt`
  Commit: Y | feat(gui): 大盘 tab（核心指数 + 板块列表 + 快照通道）
- [ ] 7. C4: 工具栏合并标的 + 前复权 Tag 隐藏
  What to do / Must NOT do: main.rs load_stock_list（541）+ 新增 load_index_list（读 index_basic.parquet，symbol/name/index_type）；StockPicker 传入合并列表（stock + index_basic ~6500，D11 过滤 O(n) 仅输入变化时 refilter）；format_display 三段式（SH | 000001 | 上证指数 / BK | 0475 | 半导体）；前复权 Tag：当前标的 index_type 非空或 BK 前缀时隐藏（工具栏渲染处条件判断）；fetch_symbol（868）/fetch_bars（884）对 BK/指数符号走 dispatch_symbol_fetch 链路。
  Must NOT do: 不改 picker 弹窗尺寸/行数逻辑；指数不参与 SEPA/选股器过滤；index_basic.parquet 缺失时 picker 优雅降级（仅股票列表，不 panic）。
  Parallelization: Wave 3 | Blocked by: T2, T4 | Blocks: —
  References: crates/compass/src/main.rs:82/541（load_stock_list）、868-918（fetch_symbol/sync_picker_from_symbol）、crates/compass-ui/src/widgets/searchable_dropdown.rs:110-133（filter_stocks）、.omo/designs/index-data.md 工具栏章节
  Acceptance criteria (agent-executable): `cargo test -p compass` 全绿；kittest/单元测试：picker 合并列表含 BK0475、指数标的前复权 Tag 隐藏、股票 Tag 仍显示
  QA scenarios: happy — 输入 "000001" 同时出 SZ000001 平安银行 + SH000001 上证指数 两行；failure — index_basic.parquet 缺失 → 只有股票列表，无 panic。Evidence `.omo/evidence/task-7-index-data.txt`
  Commit: Y | feat(gui): 工具栏合并指数/板块标的 + 前复权 Tag 按类型隐藏
- [ ] 8. 文档同步 + 决策记录（kb/design/data-providers.md、kb/user/cli.md、kb/design/symbols.md、kb/design/ui.md）
  What to do / Must NOT do: data-providers.md 加 index_daily/index_basic schema 章节（DDL + parquet 布局 + 双 parquet 路由说明 + adjclose=close 约定）；cli.md 加 import-compass --table index_daily/index_basic 文档；symbols.md 加 BK 前缀规则 + 指数符号表 + 更新 ref #201 章节（指数现在独立入库，股票管线仍剔除）；ui.md 同步大盘 tab 设计要点（design 确认后）；每个涉及的 kb/design/ 文件补 `## 决策记录` 章节（BK 前缀命名空间、双 parquet 路由、大盘 tab、index_basic 名称表）。
  Must NOT do: 不删改既有文档章节（只追加/更新）；决策记录必须自包含（what+why+why-not，表格格式）。
  Parallelization: Wave 4 | Blocked by: T1-T7（内容） | Blocks: —
  References: kb/design/data-providers.md:104-136（schema/单位约定）、kb/user/cli.md（import-compass 文档）、kb/design/symbols.md:108-125（指数剔除约定 ref #201）、214-231（前缀规则）、kb/design/ui.md（设计权威文档）
  Acceptance criteria (agent-executable): grep 断言：data-providers.md 含 "index_daily"、symbols.md 含 "BK" 前缀规则、cli.md 含 "index_daily"；所有更新的 kb/design/ 文件含 "## 决策记录"
  QA scenarios: happy — 文档与实现 schema 一致（grep 核对列名）；failure — 决策记录缺失 → 门禁 5c 检查失败。Evidence `.omo/evidence/task-8-index-data.txt`
  Commit: Y | docs: index_daily/index_basic schema + BK 前缀符号约定 + 决策记录

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- 每 todo 一个 commit，`ref #N` 独立成行（引用对应子 issue 编号，epic #255 的子 issues）
- commit 信息风格：`feat(collectors): ...` / `feat(import-compass): ...` / `feat(symbols): ...` / `feat(gui): ...`（对齐仓库既有风格）
- 每个 commit 后运行 `/review-work` 审查（5 并行 agent），发现问题修复重提交（最多 2 轮）
- commit → push 分离：**Never auto-push**；push 前 `git fetch origin master && git rebase origin/master`
- 提交前真实数据冒烟通过（落库行数/日期范围/数值合理性 evidence 落盘 `.omo/evidence/`）
- 禁止 `fixes #N`/`closes #N`（issue 手动关闭）

## Success criteria
- [ ] Dolt `index_daily` 表含三类指数日线，`fetch_index_daily.py` 可全量拉取 + 盘后增量（`last_report_date` 短路）+ 新标的自动补全量历史；Python tests ≥95%
- [ ] `import-compass --table index_daily` 导出 `index_daily.parquet`（含 `index_type`、`adjclose=close`）；真实数据冒烟：落库行数、日期范围（官方指数全历史、板块 ≥2019-12）、数值合理性全部通过并有 evidence
- [ ] 符号体系：`BK0475` 可通过 `validate_symbol`/`parse_explicit_prefix`/`strip_exchange_prefix`/`sync_picker_from_symbol` 全链路；Rust core tests 全绿
- [ ] GUI：工具栏搜索 `SH000001`/`BK0475` 出 K 线（双 parquet fallback 路由）；大盘 tab 渲染核心指数 Card + 板块列表（默认涨跌幅降序）+ Segmented 切换 + 行点击联动；前复权 Tag 对指数隐藏
- [ ] 个股零回归：`stock_daily` 不含指数代码（INDEX_SYMBOLS 剔除保持）；股票图表行为不变；`cargo test` 全绿 + 覆盖率门槛（compass-data 95%、compass 90%、compass-core 95%）
- [ ] kb/ 文档同步 + 决策记录完整（data-providers.md / cli.md / symbols.md）
- [ ] F1-F4 final verification wave 全部 APPROVE（plan 合规 / 代码质量 / 真实 QA / scope fidelity）
