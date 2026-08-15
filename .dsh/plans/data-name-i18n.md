# Plan: data-name-i18n — 数据名称翻译（epic #266）

- **Worktree**: `data-name-i18n`（分支 `feat/data-name-i18n`）
- **Epic**: https://github.com/qiboda/compass/issues/266
- **状态**: 待用户批准（2026-08-15 呈现；grill-me 决策 + D0/D1 用户裁决已锁定）
- **前置**: handoff `.dsh/plans/handoff.md`

## 背景

语言切换后 UI 框架文本正常切换，但**数据名称**（指数/行业/概念/主题）不随语言变化。
本次 epic 推翻 `.dsh/designs/gui-i18n.md` §1 的「数据不翻译」契约（L54/L432）：
所有 tab 的数据名称**有合适翻译就翻译**（中↔英双向），专用于中文/无合适翻译的保留原样。

## 已锁决策（grill-me 共识 + 用户裁决）

| # | 决策 | 内容 |
|---|---|---|
| 1 | 机制 | 数据层加英文名列：`index_basic.name_en` + `stock_basic.industry_en`；全链路 Dolt → parquet → DuckDB → GUI |
| 2 | 来源 | 核心指数官方译名（SSE Composite、CSI 300…）、行业标准译名、概念名直译；**股票名不纳入**（显示回退中文） |
| 3 | 载体 | `collectors/name_en_mapping.csv` 静态映射表随仓库提交，import 时 JOIN 写入；未收录 → NULL → 回退中文 |
| 4 | 搜索 | 三路匹配 `code`+`name`+`name_en`，**仅对有 name_en 的实体生效**；股票（无 name_en）维持 code+name 两路（**D0-B 用户裁决 2026-08-15**） |
| 5 | SEPA 主题名 | GUI 层用 `index_basic.name_en`（concept 行）构建「概念名 zh→en」映射，渲染 themes 按名查询，未命中回退中文；不动 concept_member schema（**D1-A 用户裁决 2026-08-15**） |
| 6 | 指数 fallback | `CORE_INDEX_WHITELIST` 扩为 `(symbol, zh, en)` 三元组 |

**验收标准 3 修订**（D0-B 连带）：原 "Moutai"→茅台 不可达（股票无英文名），
改为 "SSE"→上证指数 类可达目标；issue #266 追加 comment 记录修订。

## 关键现状（代码探明，2026-08-15）

- **index_basic 链**：`fetch_index_daily.py` `BASIC_DDL`（symbol PK, name, index_type）
  → `index_basic.csv` → `_import_index_basic`（INSERT IGNORE）→ Dolt
  → `import_compass.rs::import_index_basic`（SELECT symbol, name, index_type）→ `index_basic.parquet`
  → `parquet.rs::load_all_index_basics` → `IndexBasic`（compass-core model.rs:79）
  → `backend.rs::build_index_snapshot` → `IndexRow`（compass-types lib.rs:363）→ market.rs 渲染
- **stock_basic 链**：`fetch_stock_basic_official.py` 12 列 COLUMNS → `stock_basic_official.csv`
  → `main.py::_import_stock_basic`（DELETE + 12 列 INSERT）→ Dolt
  → `import_compass.rs::import_stock_basic`（717 行 9 列 INSERT）→ `stock_basic.parquet`
  → `parquet.rs::load_all_stock_basics`/`get_stock_basic_blocking` → `StockBasic`（model.rs）
  → screener 行业下拉（main.rs:133 `stock_list` 去重）/ SEPA 引擎（strategy lib.rs:134 industry）
- **SEPA**：`SepaRow.industry`（StockBasic.industry）+ `SepaRow.themes`（concept_member.concept_name，
  strategy scoring.rs:237-245）→ sepa.rs `row_cells`（L457-461 拼接 industry · theme）
- **搜索**：`searchable_dropdown.rs::matches_query`（L99-104：symbol starts_with / code / name contains）
  股票 picker；`main.rs::render_sidebar`（L1192-1194：watchlist symbol/name contains）
- **语言**：`compass_i18n::locale()` 读当前 locale（"zh"/"en"）；`set_locale` 进程级
- **契约文档**：`.dsh/designs/gui-i18n.md` L54/L432「数据不翻译」需修订；
  `.dsh/kb/design/` 无 gui-i18n.md（需新建权威契约）

## 任务批次（DAG：B1→B2→B3→B4；B5 收尾）

### B1 — collectors 数据层 + 映射表（Python）
- 新增 `collectors/name_en_mapping.csv`：三节（指数 symbol→name_en / 行业中文→industry_en / 概念名→name_en）
- `fetch_index_daily.py`：`BASIC_DDL` 加 `name_en VARCHAR(100)`；`_import_index_basic` INSERT JOIN 映射表（键：symbol）
- `main.py`：`_import_stock_basic` INSERT JOIN 映射表写 `industry_en`（键：industry 字符串，trim 后缀 "白酒Ⅱ"）
- `fetch_stock_basic_official.py`：CSV 12 列不变（映射在 Dolt import 侧 JOIN）
- pytest：映射加载 / JOIN 写入 / 未收录 NULL / 带后缀行业匹配 / 概念名匹配

### B2 — Rust 数据层（compass-core + compass-data）
- `model.rs`：`IndexBasic` + `name_en: Option<String>`；`StockBasic` + `industry_en: Option<String>`（✅ 接口骨架已落地，serde default）
- `parquet.rs`：`load_all_index_basics` / `load_all_stock_basics` / `get_stock_basic_blocking` SELECT 加列 + 旧文件降级（try-fallback，待实现）
- `import_compass.rs`：`import_index_basic` SELECT 加 `name_en`；`import_stock_basic` 加 `industry_en`；测试 DDL 同步（待实现）
- `export.rs`：DuckDB mirror `AS SELECT * FROM read_parquet` 自动继承 schema，仅测试 DDL 同步

### B3 — GUI 渲染取用（compass-types + compass-strategy + compass）
- `compass-types`：`IndexRow` + `name_en: Option<String>`；`SepaRow` + `industry_en: Option<String>`
- `backend.rs build_index_snapshot`：name_map 携带 name_en（B3a）
- `compass-strategy`：SepaRow 构建携带 `industry_en`（lib.rs:134 处，B3b）
- 新增语言取用 helper（compass crate）：`locale=="en" && name_en.is_some() → name_en`，否则 `name`（B3c）
- `market.rs`：核心卡片（三元组 fallback）+ 榜单 name 列按 locale 取用（B3d）
- `sepa.rs`：`row_cells` 行业（industry_en）+ 主题（概念名 zh→en 映射，D1-A）按 locale 取用（B3e）
- `screener.rs`：行业下拉（industries zh/en 显示）+ 表格行业列（B3f）

### B4 — 搜索三路匹配
- `searchable_dropdown.rs`：`StockProjection` 加 `name_en_of`；`matches_query` 三路（name_en 有值才参与）
- 指数/板块 picker（消费 `load_all_index_basics` 处）匹配 name_en："SSE"→上证指数
- 测试：英文 query 命中 name_en；中文 query 命中 name；股票维持 code+name

### B5 — docs + 冒烟 + 收尾
- 修订 `.dsh/designs/gui-i18n.md`「数据不翻译」契约（L54/L432）
- 新建 `.dsh/kb/design/gui-i18n.md`（权威契约，含「决策记录」章节）
- `.dsh/kb/design/data-providers.md`（schema 变更 + 决策记录）
- `.dsh/kb/user/gui.md`（UI 行为变化：数据名随语言切换）
- issue #266 追加 comment（验收 3 修订说明）
- 真实数据冒烟：collectors import → `dolt query name_en` → `import-compass` → parquet → GUI 验证

## 子 issue 分解（epic 模式，--parent 266，每个子 issue 一个 commit 批次）

| 子 issue | 批次 | 内容 | 状态 |
|---|---|---|---|
| [#268](https://github.com/qiboda/compass/issues/268) | B1 | collectors 数据层 + 映射表（Python + pytest） | ✅ done（28420ce + f3ccb45） |
| [#269](https://github.com/qiboda/compass/issues/269) | B2 | Rust 数据层（compass-core/compass-data + 测试） | ✅ done（ea9d802） |
| [#270](https://github.com/qiboda/compass/issues/270) | B3 | GUI 渲染取用（market/sepa/screener + 测试） | in_progress |
| [#271](https://github.com/qiboda/compass/issues/271) | B4 | 搜索三路匹配（picker/dropdown + 测试） | pending |
| [#272](https://github.com/qiboda/compass/issues/272) | B5 | docs 同步 + 冒烟 + 验收修订 | pending |

DAG: #268 → #269 → #270 → #271 → #272

## 验收（修订后）

1. 英文界面：大盘/SEPA/screener 的指数、行业、概念、主题名显示对应英文；无译名回退中文
2. 中文界面：所有名称显示中文（name_en 不干扰）
3. 英文界面搜索 "SSE" 能找到上证指数（有 name_en 实体三路匹配）；股票 code+name 两路不变
4. `name_en_mapping.csv` 随仓库提交，import 全链路（Dolt → parquet → DuckDB）写入新列
5. 全链路测试通过（Python pytest ≥95%、Rust 门禁 + 覆盖率阈值、真实数据冒烟）

## 验证门禁（F-wave）

- F1 合规审计：每个 commit 独立成行 `ref #<sub-N>`，指向 OPEN 子 issue
- F2 双 agent 审查：每个子 issue commit 后 + PR 前完整 diff 两层审查
- F3 测试 + 覆盖率：Rust workspace 总 ≥93%（compass-core/compass-data ≥95%、compass ≥90%）、Python ≥95%
- F4 scope fidelity：对照 issue 验收逐条核对，证据落盘 `.dsh/evidence/`

## 测试策略（RED→GREEN）

- 门禁 3.5/4：plan 批准后委派 `subagent_skwy_adversarial_test`（接口契约：新列 / 取用 helper / 三路匹配）
  + `subagent_skwy_requirement_test` 写 RED 测试（Python collectors + Rust 双侧）
- 实现：B1→B5 逐批次 GREEN，每批次 commit + 独立验证
- 真实数据冒烟在 B1/B2 完成后执行（import 数据级验证：落库行数、name_en 非空率、数值合理性）
