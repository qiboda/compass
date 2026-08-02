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
| pending | #147 | engine: SEPA 契约类型 + 指标库 (MA/ATR/RS/VCP 纯函数) | #145 |
| pending | #148 | engine: concept_daily 聚合 + 市场温度计 | #145, #146, #147 |
| pending | #149 | engine: 五模块评分引擎 + 过滤规则 | #146, #147, #148 |

### Batch 4 — CLI + 脚本
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #150 | cli: compass-data sepa 子命令 (score/temperature) | #149 |
| pending | #151 | script: sepa_daily.sh 幂等每日选股脚本 | #146, #150 |

### Batch 5 — GUI
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #152 | gui: SEPA 评分面板扩展 | #150 |

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
- **Batch 2 进行中**：#145 CrossSectionBar 扩展、#146 import-compass 新表
- 前置风险：#138（flaky run_screener_emits_completion_log）master 已修 toast 测试但该测试未见修复——todo 6 验收时需验证；另发现既有 collector 数据丢失缺陷（fin_balance_sheet=1 行，CRITICAL review 发现，新 collector 已修、范本未修——建议独立 issue 跟踪）
