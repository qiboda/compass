# Handoff: 财务三表按 UPDATE_DATE 增量抓取

## 用途
实现 GitHub issue #299：财务三表（balance_sheet / income / cash_flow）从按 `REPORT_DATE` 全市场最新报告期增量，改为按 `UPDATE_DATE` 增量抓取，捕获历史修订并减少全量拉取。

## Issue
https://github.com/qiboda/compass/issues/299

## 已锁定的 grill-me 决策（2026-08-20）
- **无 anchor 时的行为**：不再回退到按 `REPORT_DATE` 全量枚举 / 全量 replace。无 anchor（首次运行/无 state/无 data_updates）时，直接用固定起始日 `2020-01-01` 走 `UPDATE_DATE>='2020-01-01'` 增量路径（相当于一次拉取全部历史更新）。
- 其余方案以 issue #299 body 为准：三表增量 fetch 复用 `fetch_fin_indicators.py::fetch_by_update_date` / `_update_anchor` / `_normalize_update_date` 逻辑；导入改为 `import_replace_table(merge=True)` + `INSERT ... ON DUPLICATE KEY UPDATE`（参照 `main.py::_import_fin_indicators`），确保修订行覆盖旧值；state.json 记录 `last_update_date` + `last_report_date`。
- 本次为 feature 工作，需走完整 gate：worktree → plan → RED tests → docs。

## 下一步（worktree 会话第一步）
1. 先同步原始分支：`git fetch origin master && git rebase origin/master`（如落后）。
2. 读取 `.dsh/plans/handoff.md`（本文件）确认上下文。
3. 走 skwy-workflow 门禁：第 3 步 PLAN（在 worktree 内创建 `.dsh/plans/*.md` 并向用户展示批准）→ 第 3.5/4 步 RED tests（subagent_skwy_adversarial_test + subagent_skwy_requirement_test）→ 第 5b docs → 实现 → 冒烟 → commit/review/PR。

## 备注
- 本 worktree 的 `.dsh/plans/handoff.md` 在 master 上是被 git 跟踪的旧文件（内容原为 #292 index_daily）；本次已覆盖为 #299 上下文。提交 PR 时需评估是否将 handoff 更新纳入 PR（历史惯例不明确）。
