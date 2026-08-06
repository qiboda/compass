# Plan — fix/ci-hooks（#182 + #184 + #194）

## 背景

Worktree `fix/ci-hooks` 处理四个 CI 相关 open issues。**#189 已由 master 直推完成**
（`523e615`，排查卡固化），已补收尾（comment + close），不在本 PR 范围。
本 PR 处理 #182、#184、#194，各一个 commit（#184 有 review 驱动的追加修复 commits）。

## 范围

| # | 变更 | 文件 | 类型 |
|---|---|---|---|
| #184 | write_back_result temp CSV 竞争修复（test-first）+ **sepa.rs 同类竞争一并修复** | `crates/compass-data/src/backtest.rs` `crates/compass-data/src/sepa.rs` | fix |
| #182 | pre-commit hook 追加 `cargo fmt --check` | `.githooks/pre-commit` | chore |
| #194 | 合并 Rust job + 分组缓存（6 份缓存 → 3 份） | `.github/workflows/ci.yml` `kb/dev/process.md` | chore |

> **范围扩展（review 后用户批准）**：Context 审查发现 sepa.rs `write_back` 用同一
> `compass_sepa_writeback` 目录 + 固定 `{date}_{file}` 路径（6+ 测试同日期并发写），
> 与 #184 同根因。用户决策：本 PR 顺带修复——提取共享 `stage_csv` helper
> （PID+seq 唯一后缀 + O_EXCL 防 symlink + EEXIST 重试），backtest/sepa 两处统一使用，
> import 后 `remove_file` 清理 temp 文件。回归测试相应重构为「清理契约 + 数据正确性」。

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
- commit 3（#184 review 修复）：`fix(data): temp 文件 O_EXCL + stage_csv 共享 helper + 清理` `ref #184`
- commit 4（#194）：`chore(ci): 合并 Rust job + 分组缓存（6→3 份）` `ref #194`

## 批次 3 — #194 CI job 合并 + 分组缓存

**问题**：GitHub Actions 缓存逼近 10GB 上限（9.53GB）。6 个 Rust job 各自
`prefix-key: ${{ github.job }}` → 6 份独立缓存（~10GB），同一 target 存 6 遍。

**方案**（用户已确认）：
- 合并为 1 个 Rust job + bench 独立：
  - `rust`：fmt + build + clippy + docs + nextest + coverage 顺序执行
    （同一 target 累积，save 一次；保留 Dolt + nextest + cargo-llvm-cov 安装）
  - bench 保留独立（release profile）
- 缓存 6 份 → 2 份（rust / bench）
- 保留 `save-if: master`；组内顺序执行无并行覆盖，组间独立 key 无竞争（ref #14 不重演）
- 保持 rust-cache 默认 add-rust-environment-hash-key（cargo.lock 变化时内置 restoreKey 前缀匹配仍复用旧缓存）

**测试策略**（CI 配置无法写失败测试）：push 分支触发 CI 观察；本地验证合并后
cargo 命令顺序执行正确性。

**验证**：master CI 全绿 + 缓存占用下降 + branch protection 同步（4 个新 check）。

> **方案迭代（用户两次确认）**：初始方案为 rust-check/rust-test 两组 + bench 独立
> （3 份缓存）；用户确认 fmt 并入 rust-check 后改为单 rust job + bench 独立
> （2 份缓存）。branch protection required checks 从 9 个旧名同步为 4 个新名。

## 收尾

- commit 后各跑 /review-work（2 轮上限）
- 用户确认 push → /reflect 反思 commit → push → PR
- merge 后 issue 收尾：#184 comment（含自愈事实+隐患修复）、#182 comment、#194 comment，然后 close
