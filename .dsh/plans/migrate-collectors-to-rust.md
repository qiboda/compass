# Plan: migrate-collectors-to-rust — Python collectors 全量迁移到 Rust（epic #310）

- **Worktree**: `migrate-collectors-to-rust`（分支 `feat/migrate-collectors-to-rust`）
- **Epic**: https://github.com/qiboda/compass/issues/310
- **状态**: 待用户批准（2026-08-28 呈现）
- **前置**: handoff `.dsh/plans/handoff.md` + worktree 已启动 + epic/子 issue 已创建
- **用户 scope**: 本 session 完成门禁到计划批准；实现从批准后开始。

## 背景

用户原始诉求是“同步数据库的性能（用时）统计”；grill 中用户明确指示：
**“先将 python 迁移到 rust，然后再处理这个问题”**。因此本 epic 先把 Python
collectors 全量迁移到 Rust，消除 Python 采集层；时序统计作为后续独立工作，
不在本 epic 内。

## 已锁决策（grill-me 共识 + 用户确认）

| # | 决策 | 内容 |
|---|---|---|
| 1 | 范围 | 全量迁移 `collectors/` 下 Python（约 9,431 行），最终移除 Python 采集层 |
| 2 | 并行验证 | Rust 采集器与 Python 并行开发；dual-run 同数据对比等价后才切换 `update-database.sh`；全部批次完成后删除 Python |
| 3 | PR 结构 | **每个批次一个 PR**（用户确认覆盖仓库默认“一个 epic = 一个 PR”） |
| 4 | 新 crate | `crates/compass-collectors`，独立于 `compass-data` |
| 5 | HTTP/TLS | `wreq`（Chrome TLS 指纹/HTTP2，替代 Python `curl_cffi`）；`reqwest-impersonate` 为备份；**不静默降级到 reqwest** |
| 6 | 等价性 | dual-run + 迁移测试：对比 CSV/Dolt 行数、日期覆盖、关键字段 |
| 7 | 批次顺序 | 基础设施（HTTP/wreq、CSV、Dolt 写入、交易日历、progress）→ 一个简单 pilot（`block_trade`）→ 扩展到其余 |
| 8 | 切换前 | 保留 Python/tests；`update-database.sh` 继续跑 Python 直到等价 |
| 9 | 时序统计 | 本 epic 完成后单独处理 |

## 关键现状（2026-08-28 勘察）

- `collectors/` 共 ~9,431 Python 行；`main.py` 964、`common.py` 1343、`fetch_index_daily.py` 1304、`fetch_balance_sheet.py` 877、`fetch_cash_flow.py` 806、`fetch_stock_basic_official.py` 662、`fetch_income.py` 634、`fetch_main_flow.py` 464、`fetch_fin_indicators.py` 440、`fetch_freeproxy.py` 320、`fetch_dragon.py` 303、`check_proxy_pool.py` 280、`fetch_stock_basic.py` 258、`fetch_block_trade.py` 217、`fetch_institution_survey.py` 194、`proxy_keepalive.py` 199、`proxy_pool_client.py` 166。
- `collectors/common.py` 提供：`AsyncSession`（curl_cffi + chrome142）、`Throttle`、`fetch_paginated` / `fetch_by_update_date` / `fetch_incremental`、`write_csv` / `dedupe_csv` / `build_dates`、Dolt 子进程 SQL / `import_replace_table`、`last_report_date` / `set_last_report_date`、`trade_calendar` / `missing_dates`、`Progress` / `read_progress`、`ProxyPool` 集成。
- `collectors/main.py`：CLI `fetch <target>` / `import <target>` / `progress` / `sync` / `sync-investment` / `backfill`；`do_sync` 顺序 = auto-heal scan → stock_basic → fin_indicators → balance_sheet → income → cash_flow → dragon → block_trade → institution_survey → main_flow → index_daily → data_updates。
- 采集器数据源要点：
  - 龙虎榜：`RPT_BILLBOARD_DAILYDETAILSBUY` / `_SELL` 按日合并，Dolt `dragon_list` PK (symbol, trade_date, seat_type)。
  - 大宗交易：`RPT_DATA_BLOCKTRADE`，Dolt `block_trade` PK (symbol, trade_date, price, volume, amount, buyer, seller)，默认 2024+。
  - 机构调研：`RPT_ORG_SURVEYNEW`，Dolt `institution_survey` PK (symbol, survey_date, org_name)，需要宽字段避免截断。
  - 主力资金：EastMoney push2 `clist/get`（`RPT_MAIN_MONEY_FLOW` 不存在），只存最近交易日。
  - 财务：`RPT_F10_FINANCE_GBALANCE`（319 字段）/ `GINCOME`（203）/ `GCASHFLOW`（254）/ `RPT_LICO_FN_CPD`（fin_indicators 增量锚点）。
  - 指数日线：EastMoney push2his + Tencent 回退 + THS 行业板块（GBK、逐年 BK kline）。
  - stock_basic：EastMoney 全量 + 三大交易所官方源（SSE JSON / SZSE XLSX / BSE JSON）两条路径。
- Rust 侧现状：workspace 已有 `compass-core` / `compass-data` / `compass` 等；`compass-data` 已有 Dolt 子进程封装；依赖含 reqwest/rustls、serde、tokio、clap、tracing、chrono、duckdb、indicatif。**尚无 wreq**。

## 子 issue 分解

| 子 issue | 批次 | 内容 | 依赖 |
|---|---|---|---|
| [#311](https://github.com/qiboda/compass/issues/311) | B1 | `crates/compass-collectors` 搭建 + wreq HTTP/TLS 客户端 + 分页/节流 + 代理池客户端集成 | 无 |
| [#312](https://github.com/qiboda/compass/issues/312) | B1 | CSV / Dolt 写入 / data_updates / 交易日历 / missing_dates / progress 基础设施 | #311 |
| [#313](https://github.com/qiboda/compass/issues/313) | B2 | **pilot**：`block_trade` 迁移到 Rust | #311, #312 |
| [#314](https://github.com/qiboda/compass/issues/314) | B3 | dragon_list 迁移 | #313 |
| [#315](https://github.com/qiboda/compass/issues/315) | B3 | institution_survey 迁移 | #313 |
| [#316](https://github.com/qiboda/compass/issues/316) | B3 | main_flow 迁移 | #313 |
| [#317](https://github.com/qiboda/compass/issues/317) | B3 | stock_basic（EastMoney）迁移 | #313 |
| [#318](https://github.com/qiboda/compass/issues/318) | B4 | fin_indicators 迁移 | #311, #312 |
| [#319](https://github.com/qiboda/compass/issues/319) | B4 | balance_sheet 迁移 | #311, #312 |
| [#320](https://github.com/qiboda/compass/issues/320) | B4 | income 迁移 | #311, #312 |
| [#321](https://github.com/qiboda/compass/issues/321) | B4 | cash_flow 迁移 | #311, #312 |
| [#322](https://github.com/qiboda/compass/issues/322) | B5 | index_daily（官方 + THS + Tencent 回退）迁移 | #311, #312, #313 |
| [#323](https://github.com/qiboda/compass/issues/323) | B5 | stock_basic_official（三大交易所）迁移 | #311, #312 |
| [#324](https://github.com/qiboda/compass/issues/324) | B5 | 代理池工具迁移（proxy_pool_client / keepalive / freeproxy / check_proxy_pool） | #311 |
| [#325](https://github.com/qiboda/compass/issues/325) | B6 | `main.py` 编排 CLI（fetch/import/sync/progress/backfill/auto-heal）迁移 | 全部采集器 |
| [#326](https://github.com/qiboda/compass/issues/326) | B7 | 切换 `update-database.sh` 到 Rust + 移除 Python collectors + 文档 | #325 |

DAG（简）：`#311 → #312 → #313 → {#314,#315,#316,#317,#318,#319,#320,#321,#322,#323,#324} → #325 → #326`。

## 批次与 PR 计划

用户已确认：**每个批次一个 PR**，共 7 个批次（B1–B7）。每批 PR 内可以有多个 commit，
每个 commit 引用对应子 issue（独立成行 `ref #<sub-N>`）。PR 均在
`feat/migrate-collectors-to-rust` 上从当前 origin/master `5115c3f` 开始。

### B1 — 基础设施（PR 1）
- `crates/compass-collectors` crate 注册到 workspace；补 `wreq` 依赖。
- HTTP：wreq 客户端（chrome TLS/HTTP2）、`Throttle`、分页拉取、proxy 支持。
- 存储：CSV 输出/去重、Dolt 子进程 SQL、`import_replace_table`、data_updates 水位、
  `trade_calendar` / `missing_dates`、`Progress` / 状态。
- 测试：Rust 单测 + stub 网络；与 Python `common.py` 行为等价。
- 不切换任何生产脚本。

### B2 — Pilot（PR 2）
- `block_trade`：RPT_DATA_BLOCKTRADE 按日拉取 → CSV → Dolt `block_trade`。
- 迁移测试 + dual-run 对比（行数/日期/关键字段）。
- 验证基础设施可用后，后续采集器按同一模式扩展。

### B3 — 简单/日常采集器（PR 3）
- dragon_list、institution_survey、main_flow、stock_basic（EastMoney）。
- 每个模块：Rust 实现 + 迁移测试 + dual-run。
- 不切换生产脚本。

### B4 — 财务报表（PR 4）
- fin_indicators、balance_sheet、income、cash_flow。
- 共同点：财报周期、增量 UPDATE_DATE 锚点、大字段导入、Dolt upsert。
- 每个模块：Rust 实现 + 迁移测试 + dual-run。

### B5 — 复杂/特殊（PR 5）
- index_daily（多源、GBK、Tencent 回退、backfill）、stock_basic_official（官方源解析）、
  代理池工具（keepalive/freeproxy/check_proxy_pool）。
- 每个模块：Rust 实现 + 迁移测试 + 对应验证。

### B6 — 编排 CLI（PR 6）
- 移植 `main.py` 的完整 CLI，保持 `do_sync` 顺序与返回码。
- 提供 `compass-collectors` 统一二进制入口（或等价命令），供 `update-database.sh` 调用。
- 集成全部 Rust 采集器，CLI 级 dual-run。

### B7 — 切换与退役（PR 7）
- `scripts/update-database.sh` 第 2 步 `uv run python main.py sync` → Rust 入口。
- 删除/退役 `collectors/` Python 代码、`pyproject.toml`、`uv.lock`、Python tests。
- 更新文档与决策记录（见下）；CI/覆盖率配置同步。
- 已全部等价且切换后稳定，才执行本批。

## 每个采集器的 dual-run 验收矩阵

| 目标 | 对比维度 |
|---|---|
| block_trade | CSV/Dolt 行数、trade_date min/max（日期覆盖）、symbol 集合、price/volume/amount/buyer/seller/premium_rate 关键字段 |
| dragon_list | 行数、trade_date 覆盖、seat_type 分布、buy/sell/net 聚合、institution_flag |
| institution_survey | 行数、(symbol, survey_date, org_name) distinct、org_name 无截断/乱码 |
| main_flow | 行数、trade_date、symbol 集合、主力净流入关键字段 |
| stock_basic (EM) | 行数、symbol/name/industry/market/list_date |
| fin_indicators | 各 report 周期行数、symbol×REPORTDATE 覆盖、关键指标抽样 |
| balance_sheet | 行数、(symbol, report_date) 覆盖、关键字段抽样 |
| income | 行数、(symbol, report_date) 覆盖、关键字段抽样 |
| cash_flow | 行数、(symbol, report_date) 覆盖、关键字段抽样 |
| index_daily | index_daily / index_basic 行数、日期覆盖、官方指数 + THS 行业板块、Tencent 成交额回退 |
| stock_basic_official | 行数、12 列 schema、update_date、退市股覆盖 |

Dual-run 方法：
1. 同一期间（或同一批数据切片）分别跑 Python 与 Rust 采集器（CSV 落到隔离目录）。
2. 比对 CSV 行数/日期/关键字段；需要时再导入 Dolt 后比对行数与关键值。
3. 发现差异：先定位根因，再修 Rust 实现；不静默改 Python 或绕过。
4. 任何向主 `investment_data` Dolt 的写入必须遵循 AGENTS.md：完成后 `dolt commit + dolt push`；
   dual-run 阶段优先使用临时/测试 Dolt 或只写 CSV，避免污染主库。

## 测试策略

- 每个子 issue 先 RED：计划批准后按门禁委派 `subagent_skwy_adversarial_test`（3.5）和
  `subagent_skwy_requirement_test`（4）编写失败测试；首个可编译接口出现后再携带 SHA 重新委派。
- 实现阶段每个 batch：Rust 单测/集成测试 + 迁移的 Python 测试等价逻辑 + dual-run 数据验证。
- Rust 覆盖率按项目门槛（workspace 总 ≥93%，per-crate 按 `.dsh/kb/dev/testing.md` 阈值）；
  Python 在切换前保持 ≥95%。
- B7 切换后全量回归：`just check` + 真实 `update-database.sh` 同步冒烟。

## 文档同步（Gate 5b）

| 变更 | 文件 |
|---|---|
| 新采集 crate / 管线 / wreq 选型 | `.dsh/kb/design/architecture.md`（主）+ `.dsh/kb/design/data-providers.md`（如涉及 provider 侧） |
| 新 CLI（compass-collectors / update-database.sh 变化） | `.dsh/kb/user/cli.md`（主）+ `.dsh/kb/dev/process.md` |
| 测试模式（Rust 采集器 stub/dual-run） | `.dsh/kb/dev/testing.md` |
| 工作流/脚本变化 | `.dsh/kb/dev/process.md` |
| 用户侧重大变化 | `.dsh/kb/user/index.md`（pipe 概述） |

## 决策记录（Gate 5c）

实施时在 `.dsh/kb/design/architecture.md` 增加 `## 决策记录` 表格，至少包含：
`crates/compass-collectors` 独立 crate、`wreq` 选型（vs reqwest-impersonate / reqwest）、
每批一个 PR、dual-run 等价门槛、Python 退役时机。若新 crate 有独立设计文档，
同样带决策记录章节。

## 验证门禁（F-wave，最终批 B7 收尾时执行）

- F1 合规审计：所有 commit 独立成行 `ref #<sub-N>`，指向 OPEN 子 issue；F1 evidence
  在全部实现 commit 完成后一次性写，并自检 commit 计数与 HEAD 一致。
- F2 双 agent 审查：每个 batch commit 后 `subagent_review`；PR 前完整 diff 两层审查。
- F3 测试 + 覆盖率：Rust workspace 总 ≥93%（per-crate 阈值按测试文档）、Python ≥95%（切换前）。
- F4 scope fidelity：对照 epic 验收逐条核对，证据落盘 `.dsh/evidence/`，epic 总结评论记录所有完成子 issue。

## 风险与开放问题

1. **wreq 成熟度**：作为 Chrome TLS 指纹客户端可能遇到 API/编译/兼容问题；
   锁定决策明确“报告用户，不静默降级 reqwest”。若必须降级，需用户批准。
2. **端点兼容**：EastMoney/THS/交易所端点可能随 Rust HTTP 客户端行为变化；
   以 dual-run 数值对比为准，不凭“看起来像”判断。
3. **proxy 池**：Rust 侧 Redis/池交互、keepalive 守护进程需要确认与原 Python 行为一致；
   B5 子 issue 单独处理。
4. **Dolt 写入**：迁移导入路径可能触发大数据量 Dolt 写入；遵守写库后立即 commit+push。
5. **Python 退役**：B7 删除 Python 前必须完成全量 dual-run 并稳定运行；删除与 `update-database.sh`
   切换在同批但分步提交，便于回退。
6. **时序统计**：明确不在本 epic；需单独 issue/计划。

## 当前进度

- [x] 0. Worktree：已创建并注册，handoff 已读，分支与 origin/master 对齐（5115c3f）
- [x] 2. Issue：epic #310 + 16 个子 issue（#311–#326）已创建
- [x] 3. Plan：本文件已编写，用户已批准（2026-08-28）
- [ ] 3.5/4. 对抗性测试与需求测试 RED：B1/B2 首次委派返回 DEFERRED；首个可编译接口后的重新委派待补
- [x] 5b/5c. 文档与决策记录：B1 已同步 architecture.md/plan；决策记录已含 MIG-1..4
- [x] B1 基础设施：PR #327
- [x] B2 pilot block_trade：PR #328
- [x] B3 简单/日常采集器（dragon/institution_survey/main_flow/stock_basic）：本地实现 + 单元测试 + dual-run 通过，PR #329
- [x] B4 财务报表（fin_indicators/balance_sheet/income/cash_flow）：本地实现 + 单测 + 2026Q1 dual-run 全部通过，PR #330
- [x] B5 复杂/特殊（index_daily/stock_basic_official/proxy 工具）：本地实现 + 单测 + bounded dual-run/官方全量通过，PR #331
- [x] B6 编排 CLI：本地实现 + 单测 + CLI 冒烟 + review 两轮通过，PR #332
- [x] B7 切换/退役：PR #333

> B7 偏差记录（2026-08-29）：freeproxy `--source realtime` 未移植（用户接受 JSON-only）；
> 全量 update-database.sh 冒烟因外部 push2his 故障 + 1990+ SEPA 历史缺口改为有界验证。
