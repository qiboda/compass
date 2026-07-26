# 反思日志

属于项目书。记录每次功能与修复的事后反思——做了什么、哪里出错、下次怎么做。

旧的条目若已驱动具体流程改进（gate 硬阻、pre-push hook、自举测试等），则退役 — 教训已融入流程。

---

## 2026-07-25 — ref #16 fix: pre-push hook new-branch range scans only branch commits

**What was done**: Changed issue-reference validation in `.beads/hooks/pre-push` to use
`git merge-base origin/master` for new branches instead of scanning all reachable history.

**What went wrong**: `$local_oid` scanned entire commit history, flagging closed issues from old master commits.

**Lessons learned**: `git log $sha` without range prefix scans all ancestors — use `merge-base..$sha`.

---

## 2026-07-25 — chore: add worktree management skill

**What was done**: Created `worktree` skill, standardized conventions, cleaned orphan worktree.

**Lessons learned**: Process docs reference skills, don't duplicate them. Worktrees need cleanup discipline.

---

## 2026-07-26 — feat: 创建 compass_data Dolt 仓库 #25

**What was done**: 在 `investment_data` 同级创建 `compass_data` Dolt 仓库，含 `stock_basic` 和
`fin_indicators` 两张表，验证跨库 JOIN（ts_code 直连 ts_a_stock_list，symbol 直连
final_a_stock_eod_price）。

**What went wrong**: Dolt SQL 里 `||` 是逻辑 OR 而非字符串拼接 — 转用 `CONCAT()` 解决。
investment_data 的 exchange 值并非文档中的 `SHSE/BSE` 而是 `SSE/BSE`，跨库映射需以实际值为准。

**Lessons learned**: Dolt 方言和 MySQL 一致，`||` 不等于拼接；跨库查询需从父目录运行 `dolt sql` 无 `--data-dir`。
