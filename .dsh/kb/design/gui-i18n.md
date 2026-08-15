# GUI i18n — 数据名称契约（权威文档）

> 原始设计方案归档于 `.dsh/designs/gui-i18n.md`；本文件是 **UI 设计权威文档**
> （epic #266 修订后），与代码同步维护。框架文本（模板/标签）i18n 见
> `.dsh/designs/gui-i18n.md` 与 `crates/compass-i18n/`。

## 数据名称翻译（epic #266）

语言切换（中↔英）后，UI 框架文本随 `compass_i18n::set_locale` 即时切换；**数据名称**
（指数/行业/概念/主题名）由数据层携带英文名，GUI 按当前语言取用。

### 机制

- **数据层**：`index_basic.name_en` + `stock_basic.industry_en` 列；全链路
  Dolt → parquet → DuckDB → GUI（B1/B2）
- **映射表**：`collectors/name_en_mapping.csv`（`section,key,value` 三节：
  index 按 symbol / industry 按行业中文 / concept 按概念中文），随仓库提交；
  import 时 LEFT JOIN 写入（`COMPASS_NAME_EN_MAPPING` 环境变量可注入路径）；
  未收录 → NULL → 回退中文，按需增量
- **GUI 取用**：`compass::i18n_name::display_name(locale, zh, en)`——
  `locale=="en"` 且 `en` 非空 → en；空/None/非 en locale → zh（`Some("")`
  视为未映射，回退中文，绝不渲染空白 label）

### 各位置取用规则

| 位置 | 数据 | 规则 |
|---|---|---|
| 大盘核心卡片（6 指数） | `IndexRow.name/name_en` + `CORE_INDEX_WHITELIST (symbol, zh, en)` 三元组 | row 存在 → row 的 locale 名（snapshot 优先于 fallback）；row 缺失 → 三元组 per locale |
| 大盘榜单 name 列 | `IndexRow` | `display_name(locale, name, name_en)` |
| SEPA 行业 + 主题 | `SepaRow.industry/industry_en` + themes（概念名） | industry 经 `display_name`；theme 经**概念名 zh→en 映射**（D1-A：GUI 层由 `index_basic.name_en` 的 concept 行构建，`SharedState.concept_names`），未命中回退中文 |
| screener 行业下拉 | `industries`（zh 键）+ `SharedState.industry_names`（zh→en） | en locale 显示 en label、**存储值保持 zh 键**（引擎 filter 精确匹配）；共享同一 en label 的多 zh 键（如 `Mining ← {B 采矿业, 采矿业}`）显示回退 zh，避免反查歧义（P1-1） |
| screener 表格行业列 | `ScreenerRow.industry/industry_en` | `display_name` |
| 搜索（picker/下拉） | 三路匹配 | `code` + `name` + `name_en`（`StockProjection.name_en` 投影）；**仅对有 name_en 的实体生效**；股票无 name_en（D0-B），维持 code+name 两路 |
| 侧边栏 watchlist 搜索 | symbol/name | 维持两路（watchlist 为股票，无 name_en） |

### 范围边界（已锁定决策）

- **股票名不翻译**（D0-B）：显示回退中文；不参与英文搜索；映射表不收录股票
- **概念名直译**：概念名映射键 = 概念中文名（concept_member / index_basic concept 行）
- **主题名**（SEPA themes）：来自 `concept_member.concept_name`，经概念名映射翻译
- 代码、日期值、数值永不翻译

### 覆盖率现状（B1 真实数据验证）

- 行业 75/75 = 100%；概念 486/503 = 96.6%（财报预告/短线统计类 17 个按需增量）；
  官方指数 30/30

## 决策记录

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| 数据名翻译机制 | i18n 静态键 / **数据层英文列** | 数据层英文列（name_en/industry_en） | 数据动态增长（6000+ 标的），静态键不可维护；列随数据全链路流动，GUI 只读 | 静态键需穷举全部数据名且随新增数据失效 |
| 映射表载体 | 数据库表 / **collectors 静态 CSV** | `name_en_mapping.csv` 随仓库提交 | import 时 JOIN 写入，数据可追溯、版本可审；未收录 NULL 回退，按需增量 | 数据库表需额外同步链路；静态 CSV 随仓库 diff 可审查 |
| 股票名 | 纳入 / **不纳入（D0-B）** | 不纳入 | 无可靠英文名源；显示回退中文；搜索两路 | 收集 6000+ 英文名成本高且易错 |
| SEPA 主题名机制 | concept_member 加列 / **GUI 层概念名映射（D1-A）** | GUI 层映射（index_basic.name_en 的 concept 行构建） | 不动 concept_member schema；概念名直译数据复用 index_basic | 加列扩 schema 面，超出已锁决策 |
| 指数 fallback | i18n key / **whitelist 三元组** | `(symbol, zh, en)` 三元组 | fallback 是数据不是框架文本；三元组与快照同源 | i18n key 混用数据与框架文本 |
| 行业下拉显示 | label/value 同源 / **label/value 分离** | en label + zh 存储键（反查回环） | 引擎 filter 精确匹配 zh 键；显示可本地化；共享 en label 回退 zh 防歧义 | 同源则 en 界面无法显示英文或存储被污染 |
| 搜索匹配 | 两路 / **三路（code+name+name_en）** | 三路（有 name_en 的实体） | "SSE"→上证指数 类英文查询可达；股票不受影响（D0-B） | 全量三路需股票英文名，不可行 |
| 概念名映射一致性 | 概念名 key 对齐验证 | B5 真实数据冒烟确认 | concept_member 概念名与 index_basic concept 行 name 需对齐，未对齐则 themes 回退中文（不崩） | —（冒烟待执行） |
