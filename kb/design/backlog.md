# 需求池 (Backlog)

**愿景**: Compass 是一款 **local-first A-share 股票图表桌面应用** — 数据本地化，图表即时渲染。

需求池是**候选需求**的集中地：未成型想法（reflections 线索、灵感、产品观察）先沉淀到这里，
按优先级排序。选中做的时候才拆成 GitHub issue 执行（epic 或单 issue），执行细节交给 issue 跟踪，
不在池中展开。

## 使用方式

- `product` skill 每周一从需求池提出 3-5 个 sprint 候选（`/product brainstorm` 可手动触发）
- 选中候选 → 拆 GitHub issue（`/issue-workflow`）→ 按 PRE-IMPLEMENTATION GATE 执行
- 完成后从池中移除或标记，拆出的 issue 记录来源
- 优先级：`P1`（立即）/ `P2`（本周）/ `P3`（候补）

## 候选需求

| 优先级 | 需求 | 来源 | 状态 |
|---|---|---|---|
| P2 | collectors 定期刷新标志 `--refresh N`：REPORTDATE 增量无法检测已抓取期间的修订（如五粮液 2025Q1 revision），需周期性重抓 | reflections + issue #27 | 待拆 issue |
| P3 | collectors 增量修订检测：识别财报修订并触发局部重抓，替代定期全量刷新 | reflections（同上） | 待拆 issue |
| P3 | 数据质量监控：import 前后行数/日期范围校验，导入失败自动告警 | 产品观察 | 待拆 issue |

> 历史已完成的路线图条目见 `kb/dev/reflections.md`（事后反思按条目记录，不再维护独立的"已完成"清单）。
