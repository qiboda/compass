# sepa-impl - Work Plan（总览索引）

> **本文件已拆分**（用户要求拆分 + 更详细）：东方SEPA 实现计划分为 3 个独立执行计划，各自完整可审可执行。
> 本文件仅作索引，不再承载执行任务。

## 执行计划一览

| Plan | 范围 | 子 issue | 文件 | 依赖 |
|---|---|---|---|---|
| **1. 数据就绪层** | Batch 1+2：5 collectors + 数据层 + import-compass | #140-#146 | [sepa-collectors.md](./sepa-collectors.md) | 无上游 |
| **2. 评分引擎层** | Batch 3：契约类型 + 指标库 + 温度计 + 评分引擎 | #147-#149 | [sepa-engine.md](./sepa-engine.md) | plan 1 todo 6/7 |
| **3. 交付层** | Batch 4+5：CLI + 脚本 + GUI | #150-#152 | [sepa-delivery.md](./sepa-delivery.md) | plan 2 todo 8/10 |

## 配套文件

- `.omo/plans/sepa.md` — epic #139 生命周期跟踪表（issue-workflow，批次状态）
- `.omo/plans/sepa-collectors.md` — 执行计划 1（数据就绪）
- `.omo/plans/sepa-engine.md` — 执行计划 2（评分引擎）
- `.omo/plans/sepa-delivery.md` — 执行计划 3（交付层）
- `.omo/designs/sepa-gui.md` — GUI 设计方案（已确认 + 审查修订）
- `.omo/drafts/sepa-impl.md` — 规划过程 draft（决策记录）

## 关键锁定决策（跨 plan 共享，审查后修订）

| # | 决策 | 位置 |
|---|---|---|
| 1 | concept_daily 本地等权聚合，不采集官方板块指数 | plan1 todo 5/6, plan2 todo 9 |
| 2 | concept_member 全量覆盖导入（删除传播） | plan1 todo 7 |
| 3 | 题材公式分母恒 90（有/无 news 满分均 25） | plan2 todo 10 |
| 4 | 风险贡献 = −扣分合计×0.05 ∈ [−3.75,0] | plan2 todo 10 + sepa-gui.md |
| 5 | SEPA_WINDOW_DAYS = 550 | plan2 todo 10 |
| 6 | 温度计阈值常量（涨停 80/成交额 1.2 万亿） | plan2 todo 9 |
| 7 | 写回两段式 DELETE + import -a（不用 REPLACE INTO） | plan3 todo 11 |
| 8 | 脚本两段 Dolt commit（③ 采集表 + ⑥ 计算表） | plan3 todo 12 |
| 9 | GUI 进程内 run_sepa（不依赖 CLI）；TOP N 本地截断不回写 | plan3 todo 13 |
| 10 | 契约类型先落地（plan2 todo 8 第一步） | plan2 todo 8 |
| 11 | ParquetReader 读取原语授权（仅自身，禁改 DuckDbProvider） | plan1 todo 6 |
| 12 | 前置风险：#138 flaky 测试先修或豁免 | 各 plan Verification strategy |
| 13 | **concept_daily 表不建**：epic body 新表清单中的 concept_daily（采集层）已被决策 1 取代（引擎内存聚合）——三个 plan 均不建/不写该 Dolt 表，执行者勿按 epic 清单误建 | 全 plan |

## 执行顺序

```
plan 1 (sepa-collectors) → F1-F4 APPROVE → plan 2 (sepa-engine) → F1-F4 APPROVE → plan 3 (sepa-delivery) → F1-F4 APPROVE → PR/push → epic 收尾
```

每个 plan 独立 review（Momus 计划批评 + Oracle 技术验证），批准后执行；全部完成后一个 PR（.worktrees/sepa/）合并，epic #139 收尾关闭。
