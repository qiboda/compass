---
slug: ci-gate-to-merge
status: approved
intent: clear
review_required: true
approved-by: user (grill-me 决策 + "按 handoff 契约推进")
approach: 删除 pre-push hook 的 master CI 检查；配置 GitHub branch protection 强制 9 个 status check；同步 kb/dev/process.md
---

# Plan: ci-gate-to-merge

修复 issue **#172**：pre-push hook 的 master CI 检查造成死锁（修复 CI 的 PR 无法
push），改为 branch protection 强制 PR merge 的 CI 门槛。

## Components (topology ledger)

| id | outcome | status | evidence path |
|---|---|---|---|
| hook | `.githooks/pre-push` master CI 检查块（第 9-31 行）删除，push 不再被 master CI 状态拦截 | planned | `.githooks/pre-push` |
| protection | master branch protection 强制 9 个 status check（strict=true），CI 未全绿 merge 按钮禁用 | planned | GitHub API `PUT branches/master/protection` |
| docs | `kb/dev/process.md` push gate 清单删除 CI 健康项、补充 branch protection merge 门槛说明 | planned | `kb/dev/process.md:88-103` |
| test | `scripts/tests/pre-push-no-ci-check-test.sh` 行为测试断言 hook 不再含 master CI 检查 | planned | `scripts/tests/pre-push-no-ci-check-test.sh` |

## Decisions (grill-me 锁定 + draft 批准)

1. **删除 hook CI 检查块**（grill Q1）：push 阶段 master CI 状态与本次代码质量无关；
   CI 验证职责移到 merge 侧。push 保留 fmt/clippy/doc/ref #N。
2. **branch protection 强制 status check**（grill Q2）：GitHub 原生机制，CI 未全绿
   merge 按钮直接禁用。
3. **只限制 merge 不拦直推**（grill Q3）：不启用 enforce_admins/禁止直接 push，
   docs/lint/typo/反思类直推照常。
4. **9 个 check 全列**：与 ci.yml job name 精确匹配（Build/Clippy/Format/Docs/
   Bench (compile)/Test (nextest)/Coverage (llvm-cov)/Python Lint/Python Test），
   漏配会导致合法 PR 也无法合并。
5. **strict=true**：落后 master 的 PR 必须先更新才能 merge，防止 base 过期合并。

## Open assumptions (announced defaults)

| assumption | adopted default | rationale | reversible? |
|---|---|---|---|
| check 名单 | ci.yml 全部 9 个 job name | branch protection 需精确匹配 check 名 | 是 |
| strict 模式 | required_status_checks[strict]=true | 防止 base 过期合并 | 是 |
| 不拦直推 | 不启用 enforce_admins / 禁止直接 push | docs 直推保留 | 是 |
| 测试脚本超出 handoff"只改"列表 | 新增 `scripts/tests/pre-push-no-ci-check-test.sh` | gate Step 4 强制行为测试；ref #97 已建立 hook 测试先例 | 是 |

## Findings (cited - path:lines)

- pre-push hook 第 0 步（`.githooks/pre-push:14-31`）用 `gh run list --branch master`
  检查 master CI，failure 则 `has_error=1` 拦截 push
- ci.yml 有 9 个 job，name 分别是 Build/Clippy/Format/Docs/Bench (compile)/
  Test (nextest)/Coverage (llvm-cov)/Python Lint/Python Test
- `kb/dev/process.md:93` push gate 清单第 1 项即 "CI 健康：master 上的最新 CI 运行必须通过"；
  `:102-103` 手动 pre-push checklist 含 rebase + fmt/clippy/doc + ref 检查（无 CI 项——手动清单需核对）
- AGENTS.md Push 段落（:197-199）仅 rebase + 反思要求，无 CI 健康引用 → 无需修改
- 死锁实证：master CI 失败即 #169 → 修复 PR #170 被 hook 拦截，需 --no-verify 绕行
  （toolchain.md「pre-push hook 拦截修复失败 CI 的 PR（死锁）」排查卡）
- 仓库权限 admin:true（`gh api repos/qiboda/compass --jq '.permissions'`）
- GitHub 无现成 branch protection（404 Branch not protected）
- hook 测试先例：`scripts/tests/pre-push-ref-regex-test.sh`（ref #97）

## Execution batches

### Batch 1 — 测试（RED，gate Step 4）

1. 新增 `scripts/tests/pre-push-no-ci-check-test.sh`：
   - 断言 `.githooks/pre-push` 不再包含 `gh run list --branch master` / `CI_STATUS` /
     "Fix CI before pushing" 字样
   - 断言 hook 仍保留 fmt/clippy/doc/ref #N 检查（防止误删）
   - 预期 RED：当前 hook 含 CI 块 → 断言失败
2. 运行测试脚本，记录失败输出

### Batch 2 — 实现

3. `.githooks/pre-push`：删除第 14-31 行（CI 检查块），更新头部注释
   （移除第 4-5 行 "0. Latest CI on master..." 及编号调整）
4. `kb/dev/process.md:88-103`：删除 push gate 清单第 1 项 CI 健康；补充 branch
   protection merge 门槛说明（CI 未全绿 merge 按钮禁用，master 直推不受限）；
   核对手动 pre-push checklist 删除 CI 残留

### Batch 3 — 验证

5. 运行 `scripts/tests/pre-push-no-ci-check-test.sh` → GREEN
6. 运行 `scripts/tests/pre-push-ref-regex-test.sh` → 确认 ref 检测未破坏
7. `bash -n .githooks/pre-push` 语法校验

### Batch 4 — Branch protection（仓库级配置，随本 PR 一并执行）

8. `gh api repos/qiboda/compass/branches/master/protection -X PUT`：
   - `required_status_checks`: strict=true, contexts=9 个 job name
   - `enforce_admins`: false
   - 其余保护项（reviews/linear-history/PRs）不启用
9. 验证：`gh api repos/qiboda/compass/branches/master/protection --jq` 检查
   contexts 与 strict 值

### Batch 5 — Commit & Review

10. Commit（`ref #172`）—— 本 PR 的 push 本身就是"hook 不再拦修复 CI 的 PR"的验证
11. `/review-work`（5 并行 agent）
12. 按 review 结果修复（最多 2 轮）

### Batch 6 — 收尾（push 后）

13. 用户确认 push → `/reflect` 写反思 commit（同批 push）
14. push 成功 → merge PR → master CI 全绿 → 追加完成 comment + 关闭 #172

## Scope IN

- `.githooks/pre-push` 删除 master CI 检查块（第 14-31 行）+ 头部注释同步
- GitHub API 配置 master branch protection（strict=true + 9 个 contexts）
- `kb/dev/process.md` push gate 清单同步
- `scripts/tests/pre-push-no-ci-check-test.sh`（行为测试，gate Step 4 强制）
- `.omo/plans/ci-gate-to-merge.md` + `.omo/handoff.md`（过程归档，随 PR 提交）
- `.omo/drafts/ci-gate-to-merge.md` 状态更新为 approved

## Scope OUT (Must NOT have)

- 不启用 enforce_admins（保留 docs 直推）
- 不启用 require_pull_request_reviews / required_linear_history 等其他保护项
- 不改 ci.yml 本身
- 不做 #171（modal）实现——仅已建 issue
- 不改 pre-push 的 fmt/clippy/doc/ref 检查
- 不改 AGENTS.md（无 CI 健康引用）

## Verification gates

- [ ] RED：`scripts/tests/pre-push-no-ci-check-test.sh` 在删除前失败
- [ ] GREEN：删除后测试通过
- [ ] `scripts/tests/pre-push-ref-regex-test.sh` 仍全过
- [ ] `bash -n .githooks/pre-push` 通过
- [ ] branch protection API 返回 contexts 与 strict=true 正确
- [ ] 本 PR push 不被 hook 拦截（自验证）
