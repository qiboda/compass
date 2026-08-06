# Plan — fix/ci-hooks（#182 + #184）

## 背景

Worktree `fix/ci-hooks` 处理三个 CI 相关 open issues。**#189 已由 master 直推完成**
（`523e615`，排查卡固化），本 session 已补收尾（comment + close），不在本 PR 范围。
本 PR 处理 #182 与 #184，各一个 commit。

## 范围

| # | 变更 | 文件 | 类型 |
|---|---|---|---|
| #184 | write_back_result temp CSV 竞争修复（test-first） | `crates/compass-data/src/backtest.rs` | fix |
| #182 | pre-commit hook 追加 `cargo fmt --check` | `.githooks/pre-commit` | chore |

## 任务批次

### 批次 1 — #184 temp 文件竞争（test-first）

**问题**：`write_back_result` 的 temp CSV 路径
`compass_sepa_writeback/{end}_backtest_result.csv` 在多个测试间共享
（roundtrip 与 full_replace 都用 `end=2025-01-03`），nextest 并行时有竞争窗口
（CI 实测 1 次 FAIL：`write_back_result_full_replace` 断言 left:2 right:1，重跑自愈）。

**RED（先写失败测试）**：
- 新增确定性回归测试：以相同 `end` 连续两次调用 `write_back_result`（不同 dolt
  tempdir），断言 temp 目录中产生两个不同的 `{end}` 前缀 CSV 文件（唯一后缀）。
  修复前两次调用写同一路径 → 目录只有 1 个文件 → 断言失败（RED）。
- 该测试确定性触发（非 flaky），锁定"路径必须唯一"的契约。

**GREEN（修复）**：temp 文件名加唯一后缀（进程 PID + 单调计数器或时间戳），
例如 `{end}_backtest_result_{pid}_{seq}.csv`。生产路径一次调用无感知，测试
并行时互不干扰。

**验证**：`cargo test -p compass-data backtest` 全绿；flaky 根因消除。

### 批次 2 — #182 pre-commit 追加 cargo fmt --check

**变更**：`.githooks/pre-commit` 在现有 ruff 检查旁追加：

```sh
if ! cargo fmt --check; then
    echo "ERROR: cargo fmt --check found unformatted code. Run 'cargo fmt'."
    exit 1
fi
```

**验证**：
1. 故意改坏某文件格式 → commit 被 pre-commit 拒绝并提示
2. 恢复格式 → commit 通过
3. 既有 ruff / pre-push / commit-msg 行为不变

## 验证门禁

- `cargo test` 全绿（重点 `-p compass-data`）
- `cargo clippy -- -D warnings`、`cargo fmt --check`
- `lsp_diagnostics` 变更文件无错误
- pre-commit hook 手工触发验证（含失败分支）

## 提交

- commit 1：`fix(data): write_back_result temp CSV 路径加唯一后缀消除测试竞争` `ref #184`
- commit 2：`chore(hooks): pre-commit 追加 cargo fmt --check 拦截未格式化代码` `ref #182`

## 收尾

- commit 后各跑 /review-work（2 轮上限）
- 用户确认 push → /reflect 反思 commit → push → PR
- merge 后 issue 收尾：#184 comment（含自愈事实+隐患修复）、#182 comment，然后 close
