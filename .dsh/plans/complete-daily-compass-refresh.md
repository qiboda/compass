# Plan — Complete Compass data daily refresh

## Issue
- #306 — [fix: sepa_daily.sh 应刷新完整 compass_data（含财务表与 stock_basic）](https://github.com/qiboda/compass/issues/306)
- Handoff: `.dsh/plans/handoff.md`
- Worktree: `fix/complete-daily-compass-refresh`

## Objective
把 `scripts/sepa_daily.sh` 从 SEPA-only 扩成完整 `compass_data` 每日刷新入口，
覆盖 `stock_basic` + 财务四表 + SEPA 表 + 指数表，并修正文档中不准确的描述。

## Locked decisions (from grill-me)
1. 扩展现有 `scripts/sepa_daily.sh`，不新建 update-all.sh。
2. 包含全部 `compass_data` 表（含 `stock_basic` 与财务四表）。
3. step 2 改用 `uv run python main.py sync`（单一入口，避免 shell 重复维护源列表）。
4. 同步扩展 Dolt allowlist 和 import-compass 表清单。
5. 不执行 `export`（DuckDB）。
6. 修复后运行脚本更新真实数据。

## Tasks

### Batch 1
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #306 | 更新 `scripts/sepa_daily.sh`：`COLLECTOR_TABLES` 扩至 11 张表、step 2 改用 `main.py sync`、step 4 覆盖 11 张表（stock_basic/index_basic 全量） | — |
| pending | #306 | 更新 `scripts/tests/test-sepa-daily.sh` 的 mock 契约（单次 sync 调用、11 表 import、allowlist/锚点边界） | — |
| pending | #306 | 文档同步：`.dsh/kb/user/cli.md` 每日流水线说明；全仓 grep `sepa_daily`/`每日一键` 引用点 | — |
| pending | #306 | 真实数据冒烟：运行修复后的 `scripts/sepa_daily.sh`，核对 Dolt/Parquet 各表与 data_updates | — |
| pending | #306 | 提交（含 `ref #306`）→ review → push → PR → 合并后 issue 收尾 | — |

## Test strategy
- 门禁 3.5：委派 `subagent_skwy_adversarial_test` 写对抗性测试（RED）：
  - sync 失败时 step 2 必须中止，不得继续 import-compass/step 5
  - `stock_basic` 必须全量覆盖，不得带 `--since`
  - 财务四表在无锚点时全量、有锚点时使用各自 `last_report_date`
  - dolt allowlist 不 stage 未列入表，不 `dolt add .`
  - 输出调用数必须与 11 表一致
- 门禁 4：委派 `subagent_skwy_requirement_test` 更新/补充需求验收测试（RED）：
  - step 2 恰好一次 `uv run python main.py sync`
  - step 4 恰好 11 次 import-compass 调用
  - `stock_basic` 与 `index_basic` 全量，其余按锚点
  - 脚本头部注释、文档描述同步
- 实现后：两批测试全部通过（GREEN）→ 独立 QA 复核 → review。

## Docs sync plan
基于变更文件：`scripts/sepa_daily.sh`、`scripts/tests/test-sepa-daily.sh`。

| 文档文件 | 原因 | 变更类型 |
|---|---|---|
| `.dsh/kb/user/cli.md` | 每日一键流水线描述需反映完整 compass_data 刷新 | 工作流/命令说明 |
| `.dsh/kb/dev/database.md` | sepa_daily.sh step2/4 与 11 表锚点说明需同步 | 数据管线 |
| `.dsh/kb/design/data-providers.md` | 决策记录表补充 #306 决策，保持设计文档与实现一致 | 数据管线 |

另需全仓 grep 以下标识符：`sepa_daily`、`每日一键流水线`、`5 sources`、`6 tables`。

## Final verification wave
- F1: commit message 含独立成行 `ref #306`；变更范围与计划一致（无超范围）。
- F2: `bash -n` + `scripts/tests/test-sepa-daily.sh` 全绿；review 无 P0/P1。
- F3: 真实数据冒烟成功（各表 row_count/data_updates 一致）。
- F4: 未改动非计划文件；文档覆盖所有引用点。

## Out of scope
- 不修改 `crates/compass-data`（import-compass 已支持 11 表）。
- 不执行 `export`（DuckDB）。
- 不新建 update-all.sh。
