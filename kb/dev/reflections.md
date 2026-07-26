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
