# Epic: 东方SEPA 多因子选股系统 (epic #139)

> 计划文件 — 跟踪 epic #139 的子 issue 生命周期（issue-workflow 规范）。
> 设计决策 25 项详见 epic #139 body（grill-me 共识 2026-08-02）。

## Tasks

### Batch 1 — 采集层（Python collectors）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #140 | collector: 主力资金流采集 (capital_main_flow) — push2 快照（RPT_MAIN_MONEY_FLOW 不存在，已获批改 push2） | — |
| done | #141 | collector: 龙虎榜采集 (dragon_list) — BUY/SELL 席位报表（股票级报表无席位，已获批） | — |
| done | #142 | collector: 大宗交易采集 (block_trade) — RPT_DATA_BLOCKTRADE（计划名不存在，已获批） | — |
| done | #143 | collector: 机构调研采集 (institution_survey) — RECEIVE_* 映射（SURVEY_DATE 不存在，已获批） | — |
| done | #144 | collector: 概念板块成分采集 (concept_member) — push2 板块列表（无 datacenter 报表，已获批） | — |

### Batch 2 — 数据层
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #145 | data: CrossSectionBar 扩展 open/high/low/amount + 新表读取原语 | — |
| pending | #146 | data: import-compass 支持 5 张 SEPA 新表（concept_member 全量覆盖导入） | #140, #141, #142, #143, #144 |

### Batch 3 — 引擎
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #147 | engine: SEPA 契约类型 + 指标库 (MA/ATR/RS/VCP 纯函数) | #145 |
| done | #148 | engine: concept_daily 聚合 + 市场温度计 | #145, #146, #147 |
| done | #149 | engine: 五模块评分引擎 + 过滤规则 | #146, #147, #148 |

### Batch 4 — CLI + 脚本
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #150 | cli: compass-data sepa 子命令 (score/temperature) | #149 |
| done | #151 | script: sepa_daily.sh 幂等每日选股脚本 | #146, #150 |

### Batch 5 — GUI
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| done | #152 | gui: SEPA 评分面板扩展 | #150 |

### 后续（不在本 epic 批次）
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| open | #153 | follow-up: 行业新闻政策 LLM 自动分析 | — |
| open | #154 | follow-up: SEPA 量化回测系统 | — |

## 批次切换规则

- 完成当前批次所有子 issue 后：更新状态 → 向用户报告 → 等确认 → 下一批次
- 每个 commit 引用对应子 issue（`ref #<sub-N>`）；每个子 issue 一个 commit
- 一个 epic = 一个 PR = 一个 worktree

## 批次进度

- **Batch 1 完成（2026-08-02）**：5 个 collector 已提交（00cb0b6/4af9938/23ceae1/2e8f6fe/bc10a54 + 重构 e1bfed8）；东财接口实测修正 5 处（用户批准）；F3 真实冒烟通过（capital_main_flow 5536 行已 dolt commit+push）
- **Batch 2 完成（2026-08-02）**：#145 CrossSectionBar 9 字段 + 5 读取原语（4993364）、#146 import-compass 5 新表（31743a5）；#138 flaky 根治（6b842c3，set_global_default）
- **Batch 3 完成（2026-08-02）**：#147 契约类型+指标库（ef681e5）、#148 聚合+温度计（5b89f2f）、#149 五模块评分引擎（b479f66）+ 真实数据冒烟（fef1067）；F3 冒烟：温度计 38.1、87 涨停、TOP 分数 17.9-35.6；**数据维护事项**：stock_daily.parquet amount 列为 0（成交额分失真，需核对 Dolt 导入）
- **Batch 4+5 完成（2026-08-02）**：#150 sepa CLI + Dolt 写回（c525ad3）、#151 sepa_daily.sh（f01c1f9）、#152 SEPA GUI 面板（4a06cf5）；kb 文档同步（29696c5）；F3 端到端：sepa score 真实写回 5 计算表 + 幂等重跑行数不增 + Dolt 双段 commit 落 remote
- **F3 真实端到端修复（2026-08-03）**：5-way PR review 发现并修复 3 批代码缺陷——(a) 4505697：sepa CLI 写回 P0（`--top` 截断、temperature 清空 factor 表、决策 22 默认日期）；(b) 48f1d9f：block_trade PK 7 列 + survey DDL 加宽 + sepa_daily step2 循环；(c) 81f8e20：survey create_sql 宽表替代无效 alter_sql（dolt -c 固定 varchar(200) 字节截断）+ 4 时间序列表 merge 导入保历史（增量窗口不再覆盖完整历史）+ step2 补 import 环节；(d) acb236b：block_trade 增量窗口截断到今天。**F3 完整闭环**：真实采集 5 源（capital_main_flow 5536 / dragon_list 4197 / block_trade 18391 / institution_survey 40115 / concept_member 70459）→ import → 计算（温度计 40.7、TOP5 中际旭创 55.5）→ Dolt 双段 commit + push remote（`dolt log` 验证 collectors/scores 两 commit）；周日脏行清理（trade_date=2026-08-02 删除）；全量 627 Rust + 226 Python 测试绿 + ruff 干净
- **epic 收尾待办**：PR 创建前 rebase origin/master + PR 级 review + /reflect 反思 commit（push 前）+ 用户确认 push → 关闭 13 子 issue + epic #139
