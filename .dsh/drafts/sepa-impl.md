---
slug: sepa-impl
status: approved-plan-written
intent: clear
review_required: false
pending-action: "计划已写入 .omo/plans/sepa-impl.md（用户批准后，实现阶段由执行 agent 在 worktree 内按批次执行）"
plan-file-relation: ".omo/plans/sepa.md 是 issue-workflow 生命周期跟踪表（批次状态）；.omo/plans/sepa-impl.md 是执行计划（任务/验收/QA/commit 细节），二者互补不冲突"
approach: "14 子 issue 分批实现：Batch1 采集(#140-144, 5 collectors) → Batch2 数据层(#145-146) → Batch3 引擎(#147-149) → Batch4 CLI+脚本(#150-151) → Batch5 GUI(#152)。每批独立 commit(ref #N)，一个 epic 一个 PR 一个 worktree。计算层 5 表：technical_factor/industry_factor/capital_factor/final_score/market_temperature。"
---

# Draft: sepa-impl

## Components (topology ledger)

| id | outcome | status | evidence path |
|----|---------|--------|---------------|
| C1 | 5 个 Python collectors（主力资金流/龙虎榜/大宗/机构调研/概念板块）写入 Dolt compass_data | active | collectors/fetch_income.py 范本（common.py 模式） |
| C2 | CrossSectionBar 扩展 open/high/low/amount + fetch_cross_section SQL | active | crates/compass-core/src/model.rs:137-148, parquet.rs:373-428 |
| C3 | import-compass 支持 6 张新表（concept_daily/concept_member/capital_main_flow/dragon_list/block_trade/institution_survey） | active | import_compass.rs:13-57 CompassTable + 增量合并 L83-208 |
| C4 | compass-strategy mod sepa：指标库(MA/ATR/RS/VCP) + 温度计 + 五模块评分 + 过滤 | active | lib.rs run_screener 模式（L44-97） |
| C5 | compass-data sepa CLI 子命令（score/temperature）+ 写回 Dolt | active | main.rs Command 枚举 L24-109（需新 SepaCmd） |
| C6 | scripts/update-database.sh 幂等每日脚本（含 investment_data 更新） | active | scripts/sync-investment-data.sh 骨架 |
| C7 | GUI SEPA 面板扩展 | active | .omo/designs/sepa-gui.md（已确认+审查修订） |

## Open assumptions (announced defaults)

| assumption | adopted default | rationale | reversible? |
|-----------|----------------|-----------|-------------|
| READ_WINDOW_DAYS=400 不足 | SEPA 用独立窗口常量 ~550 日历日 | MA250 需 250 根 bar，400 日仅 268 根余量 18；RS 双窗口+VCP 120 天需更多 | 是（常量） |
| Rust 写回 Dolt 无先例 | 扩展 import_dolt.rs 的 subprocess 封装风格，新增写 SQL/dolt table import 函数，路径取 config.dolt.compass_data_dir | 现有模式 `dolt --data-dir <dir> sql`，写回同构 | 是 |
| compass_data commit/push 无自动化 | update-database.sh 末尾承担 dolt add/commit/push（commit message 含 ref #N） | AGENTS.md 规定每次数据变更后必须 commit/push；脚本是每日唯一入口 | 是 |
| 嵌套子命令无先例 | main.rs 新增 `#[derive(Subcommand)] SepaCmd { Score, Temperature }` + Command::Sepa 变体 | clap derive 原生支持 | 否（接口形态） |
| data_updates 登记 | SEPA 新表沿用 5 列 upsert（table_name/last_updated/source/row_count/last_report_date） | 现有模式统一（main.py:70-76, fetch_income.py:214-219） | 否 |
| symbol 约定 | Dolt 表 symbol 用 `SZ000001` 前缀格式，collectors 用 CONCAT 拼接 | 与 stock_basic/财务表对齐 | 否 |
| concept_member 版本跟踪 | PK(concept_code, symbol) + update_date，非每日快照 | grill 决策 20 | 否 |
| GUI 数据就绪判断 | 资金/题材模块数据未就绪时按空子项渲染（GUI 无感） | 设计待确认 4 | 否 |

## Findings (cited - path:lines)

1. **compass_data Dolt 仓库**：分支 `main`（非 master），remote=doltremoteapi.dolthub.com/skwy/compass_data，13 commits 风格 `feat: ...` + `ref qiboda/compass#N`，作者 `CI <ci@compass.local>`；working tree clean 与 origin 同步；6 表（data_updates/fin_*/stock_basic）。**无任何脚本/CI 自动化 commit/push**——手动/agent 流程。（探索 bg_e560f882）
2. **Rust 侧零写 Dolt 先例**：import_dolt.rs 仅 `run_dolt_sql_csv`(L19)/`run_dolt_sql_parquet`(L40) 只读；写 Dolt 只出现在测试代码（临时库）。import_compass.rs 复用 run_dolt_sql_parquet 只读。（bg_e560f882）
3. **collectors 架构**：fetch_income.py 是重构范本（模块常量+DDL+COLS+async run()+import_to_dolt()+CLI）；common.py 提供 dolt_sql/dolt_sql_csv/dolt_table_import/last_report_date/Throttle/fetch_paginated/write_csv/build_dates；东财统一走 datacenter-web.eastmoney.com/api/data/v1/get；symbol=CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE,'.',-1)), SECURITY_CODE)；main.py 注册 4 触点（dispatch_fetch/import/do_sync/choices×2）。（bg_1e50cd5f）
4. **测试模式**：Python conftest.py StubSession + TestRun + TestImportToDolt（真实 temp Dolt，dolt_env fixture + COMPASS_DATA_DIR monkeypatch）；Rust compass-data 无 tests/ 目录，全部内嵌 #[cfg(test)]；compass-strategy tests/screener.rs 用 tempdir DuckDB COPY parquet fixture。（bg_1e50cd5f, bg_2bd253e5, bg_39a88db6）
5. **run_screener 模式**：fetch_cross_section(range_start, now) 单次全市场加载 + load_all_stock_basics + HashMap<&str, Vec<&CrossSectionBar>> 分组 + 逐标的 Ok(None) 短路 + total_cmp 排序 + MAX_RESULTS 截断；指标基于 adjclose；窗口不足跳过不崩溃；READ_WINDOW_DAYS=400 日历日 ≈ 268 交易日。（bg_2bd253e5）
6. **CLI 结构**：main.rs Command 枚举 4 变体（Import/ImportCompass/Export/Backup）+ load_config + run() dispatch；错误类型 Result<(), Box<dyn Error>>；ImportCompass 已有 --dolt-dir 默认 config.dolt.compass_data_dir；CompassTable FromStr 映射先例；测试内嵌 L236-624。（bg_39a88db6）
7. **import-compass 增量合并**：since.is_some() && !overwrite && path.exists() → DuckDB ROW_NUMBER() OVER (PARTITION BY symbol, report_date ORDER BY priority) UNION ALL 旧(1)+新(2) WHERE rn=1；new_data.len()<500 空守卫；DuckDB 失败回退全量。（bg_39a88db6）
8. **scripts 风格**：set -euo pipefail + 头部注释 + PROJECT_ROOT + preflight（command -v dolt/creds/.dolt）+ red/green/info 彩色输出；sync-investment-data.sh(L124) 是最接近 update-database.sh 的样板；scripts/tests/ 有自测先例。（bg_e560f882）
9. **GUI 接线**：DataCell enum{Text,Price,Count}（data_table.rs:31-44）可加 Score/Rank 变体；TabKind{Chart,Logger,Screener}（tabs.rs:50-54）可加 Sepa；SharedState 字段模式（state.rs:11-34）；wire_backend 2-tuple 波及 main.rs:73/backend.rs 4 处/main.rs:1044；egui_dock 0.20.1 per-tab 状态判定使 dock_style 无需修改。（Oracle 审查 + 代码验证）

## Decisions (with rationale)

1. **窗口常量**：SEPA 独立 `SEPA_WINDOW_DAYS = 550`（≈364 交易日，审查验证余量 ~110 根），覆盖 MA250(250) + VCP(120) + RS(60/20) + 余量——不能复用 READ_WINDOW_DAYS=400（余量仅 18 根）。
2. **写回 Dolt 机制（审查修正）**：dolt table import **无原生 upsert**——`-c` 只建新表、`-u` 只更新已有行（不插入新行）、`-a` 只追加（主键冲突 abort，`--continue` 跳过）。据此：
   - 采集层增量表（collectors 写）：`-a --continue` 追加（幂等重跑安全）
   - 计算层评分表（compass-data 写）：两段式 `DELETE WHERE trade_date=...` + `-a --continue`，或 dolt sql `INSERT ... ON DUPLICATE KEY UPDATE`（dolt 支持 MySQL 语法）
   - compass-data 新增 `sepa.rs`，复用 import_dolt.rs 的 `dolt --data-dir` subprocess 封装风格；**Cargo.toml 新增 `compass-strategy = { path = "../compass-strategy" }` 依赖**（无循环：strategy → core/types，data 已有 core）
3. **commit/push 归属**：update-database.sh 末尾执行 dolt add/commit/push（compass_data 仓库，分支 main，message 含 ref #139），幂等——无变更则跳过；**dolt add 限定 SEPA 相关表**，勿 `add .` 卷入 collectors 未提交变更。
4. **CLI 形态**：`Command::Sepa { #[command(subcommand)] cmd: SepaCmd }`，SepaCmd::{Score{--top, --date}, Temperature}；输出 TOP50 表格 + 写回 Dolt + data_updates 登记。
5. **collectors 架构（审查修正）**：4 个数据源 collector（主力资金流/龙虎榜/大宗/机构调研）照抄 fetch_income.py 重构范本（common.py 复用），走 datacenter 接口（RPT_MAIN_MONEY_FLOW / RPT_DAILYBILLBOARD_DETAILSNEW / RPT_BLOCKTRADE_DETAILS / RPT_ORG_SURVEYNEW）；concept_member 成分映射采集走 datacenter（RPT_F10_CORETHEME_BOARDTYPE）。**concept_daily 不采集**——由引擎本地聚合（用户确认，自己的权重）：板块日线 = 成分股等权（当日涨跌幅等权平均、成交额合计、上涨家数），`concept_member` + `stock_daily`（Batch2 扩展后含 amount）本地计算，作为计算层产物。
6. **引擎模块化（审查修正）**：compass-strategy 拆 `mod sepa`，**第一步先定义 compass-types 边界契约类型**（SepaQuery/SepaData/SepaRow/SepaDetails/SepaFactor/MarketThermometer/SepaIndicator——GUI 与 CLI 均依赖，必须先落地否则 Batch5 无法编译），再实现指标库（indicator/temperature/scoring/filter），入口 `run_sepa(query: &SepaQuery, reader: &ParquetReader, now) -> Result<SepaData, ScreenerError>`。
7. **GUI 按设计文档**（sepa-gui.md 已确认+审查修订）：DataCell::Score{value,max,inverted} + Rank 变体 + score_color + 独立 TabKind::Sepa + 单 Option<SepaData> 状态 + 第三条 AsyncDispatcher 通道 + 右侧详情面板 + 温度计顶条 + GUI 截断 TOP N。

## 批次依赖矩阵（审查要求显式化）

```
B5 (GUI #152) ← B3 契约类型+run_sepa (#147/149) ← B2 CrossSectionBar 扩展 (#145)
B4 (CLI #150, 脚本 #151) ← B3 run_sepa (#149) + B1 表存在 (#140-144) + B2 import-compass (#146)
B3 (引擎 #147-149) ← B2 字段扩展 (#145) + B2 表导入 (#146, concept_member 就绪)
B2 (数据 #145-146) ← B1 collectors 建表 (#140-144)
B1 (采集 #140-144) — 无依赖
```

## 覆盖率预算（审查要求补足，CI 门槛 ≥80%）

- B1：5 collector 文件 + 5 测试文件（TestRun stub session + TestImportToDolt 真实 temp Dolt），`--cov-fail-under=80` 现有门槛
- B3：指标库 800-1500 行为覆盖率稀释主风险——TDD 先行，边界用例强制（除零/空序列/窗口不足/平台期）；契约类型测试随类型定义
- B4：sepa.rs 写回路径（temp Dolt fixture 已有先例）+ CLI 解析测试；update-database.sh 幂等测试（scripts/tests/ 自测先例）
- B5：egui_kittest 面板渲染/交互 + 双 tab leaf 视觉断言（形状测试，禁目测）

## Scope IN

- Batch1：4 个数据源 collectors + concept_member 成分采集 + Dolt 新表（concept_member/capital_main_flow/dragon_list/block_trade/institution_survey）+ main.py 注册 + 测试
- Batch2：CrossSectionBar 扩展 4 字段 + fetch_cross_section SQL + 测试 + import-compass 5 新表 + data_updates 登记
- Batch3：compass-types 契约类型（SepaQuery/SepaData/SepaRow/SepaDetails/SepaFactor/MarketThermometer/SepaIndicator）→ mod sepa 指标库（MA/ATR/RS/VCP/量比/回撤）+ concept_daily 本地聚合（成分股等权）+ 市场温度计 + 五模块评分 + 过滤规则 + run_sepa 入口 + 测试
- Batch4：compass-data sepa CLI（score/temperature）+ 写回 Dolt 机制（DELETE+append 两段式）+ data_updates + update-database.sh（含 investment_data import 步骤）
- Batch5：GUI SEPA 面板（按 sepa-gui.md 全套）+ egui_kittest 测试 + 双 tab leaf 视觉断言
- 文档：kb/design/data-providers.md（CrossSectionBar 字段决策记录）、kb/design/ui.md（SEPA 面板）、kb/user/cli.md（sepa 命令）、kb/dev/testing.md（如新测试模式）、kb/user/config.md（如新增配置）

## Scope OUT (Must NOT have)

- 卖出系统、北向资金数据源、新闻政策 LLM、回测系统（均为独立后续 issue：卖出=不做、LLM=#153、回测=#154）
- 不新增任何 crate/UI 依赖（compass-data 新增 workspace 内 compass-strategy path 依赖除外；东财接口若需新 Python 依赖需单独评估）
- 不采集东财官方概念板块指数（concept_daily 用本地成分股等权聚合，用户确认）
- 不做历史批量回算（只算最新交易日，增量累积）
- 不做自动定时触发（update-database.sh 手动执行）
- 不引入 serde 到 SEPA 类型（无 TOML 持久化需求）

## Open questions

无——全部决策已锁定：grill 25 项 + 设计审查（S1/M1-M4）+ 计划双重审查（upsert 两段式、契约类型先落地、覆盖率预算、concept_daily 聚合）均已修订落地。

## Approval gate

status: awaiting-approval
<!-- 探索已穷尽（4 explore + Momus 计划审查 + Oracle 技术审查，两轮修订完成）。批准后写 .omo/plans/sepa-impl.md（决策完备执行计划，含每任务引用/验收/QA/commit + 显式依赖矩阵 + 批次映射表）。 -->
