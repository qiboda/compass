# Handoff — data-name-i18n worktree

**用途**：数据名称翻译 epic（issue #266）——`index_basic` / `stock_basic` 增加英文名列（`name_en` / `industry_en`），collectors 静态映射表 import 时 JOIN 写入，GUI 按语言取用 + 搜索三路匹配。

**Epic**：https://github.com/qiboda/compass/issues/266

**⚠️ 启动第一步：同步原始分支**
`git fetch origin master && git rebase origin/master`（worktree 创建后 master 可能推进），冲突解决后再开始。

## 已锁定的 grill-me 决策（2026-08，用户确认）

| # | 决策 | 内容 |
|---|---|---|
| 1 | 范围 | 所有 tab 的数据名称（大盘核心卡片/榜单、SEPA 行业+主题、screener 行业下拉、搜索下拉），**有合适翻译就翻译**（中↔英双向）；专用于中文/无合适翻译的不翻译 |
| 2 | 机制 | **数据层加英文名列**：`index_basic.name_en` + `stock_basic.industry_en`；全链路 Dolt → parquet → DuckDB → GUI |
| 3 | 来源策略 | 核心指数官方译名（SSE Composite、CSI 300…）、行业标准译名、**概念名直译**（AI Concept…）；**股票名不纳入**（回退中文） |
| 4 | 载体 | **collectors 静态映射表**（`name_en_mapping.csv` 随仓库提交），import 时 JOIN 写入；未收录 → NULL → 回退中文，按需增量 |
| 5 | 搜索 | 搜索**三路匹配** `code` + `name` + `name_en`（"Moutai"→茅台、"SSE"→上证指数） |

## 关键现状（主 session 已探明）

- 生产代码 UI 框架文本已全部走 i18n key；硬编码中文只有数据名称（`market.rs` CORE_INDEX_WHITELIST 6 指数 fallback、SEPA industry/themes、screener 行业下拉）
- `index_basic`：Dolt 表（compass_data 库），schema `(symbol, name, index_type)`，来源 `fetch_index_daily.py`（EastMoney），import 走 `import_index_basic`（INSERT IGNORE，PK symbol）
- `stock_basic`：12 列 schema（含 name/board/full_name/industry/region），来源三大交易所官网 `fetch_stock_basic_official.py`，import 走 `main.py _import_stock_basic`（全量 DELETE + INSERT）
- GUI 数据流：`ParquetReader::load_all_index_basics` → `IndexBasic`（compass-core model）→ picker；`build_index_snapshot`（backend.rs）→ `IndexRow`（compass-types）→ market/SEPA 渲染
- SEPA 行业名来自 `row.industry`（stock_basic.industry），主题名 `row.themes`（概念名，存于 SEPA 结果）
- 映射表匹配键：`index_basic` 按 symbol（SH000001/BK0475）；`stock_basic.industry` 按行业字符串（"白酒Ⅱ" 带后缀，import 前有 trim）

## 下一步（gate 剩余步骤，按序执行）

1. **Step 1 DESIGN**：数据名称取用 + 渲染规则属数据/逻辑变更，无布局/视觉变更——可跳过 ui-designer（判断后记录）
2. **Step 3 PLAN**：DSH plan mode 产出 `.dsh/plans/data-name-i18n.md`（任务批次 + DAG + 验收），exit_plan_mode 批准
3. **Step 2 ISSUE**：epic #266 已建；plan 批准后按 epic 模式创建子 issues（--parent 266）
4. **Step 3.5/4 TESTS**：plan 批准后委派 skwy-adversarial-test / skwy-requirement-test 写 RED 测试（Python collectors + Rust 双侧）
5. **Step 5b DOCS**：按 AGENTS.md 映射表更新 `.dsh/kb/design/data-providers.md`（schema 变更）+ `.dsh/kb/design/gui-i18n.md`（契约变更）+ `.dsh/kb/user/gui.md`（如有 UI 行为变化）；**注意 `.dsh/designs/gui-i18n.md` 的"数据名不翻译"契约需同步修订**
6. **Step 5c**：相关 design 文档补「决策记录」章节
7. 映射表首版（核心指数 + 全量行业 + 概念直译）批量生成，提交前请用户 review
