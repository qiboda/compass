---
name: worktree
description: 管理 PR 开发的 git 工作树。用于创建、列出或删除 .worktrees/ 下的工作树。当用户说 "worktree"、"切一个worktree" 或需要 PR 工作空间时触发。
---

# Worktree

Git 工作树为 PR 开发提供隔离的工作目录。
每个工作树是单个 PR 的**临时工作空间**——开发开始时创建，PR 合并后删除。

## 约定

所有工作树位于 `.worktrees/<name>/` 下（已 gitignore）。分支命名：`feat/<short-description>` 或 `fix/<short-description>`。

```
.worktrees/
├── fix-candle-rendering/   # 修复蜡烛图渲染的 PR
└── add-sector-filter/      # 添加板块筛选的 PR
```

| 工作树路径 | 用途 |
|---|---|
| `.worktrees/<name>` | 临时 PR 工作空间——每个 PR 一个 |
| `.worktrees/<name>` | 短期存在：PR 创建时建立，合并后删除 |

## 命令

### 创建

```bash
# 从 master 切 PR 分支（默认场景）
git worktree add -b feat/<name> .worktrees/<name> master

# 从目标分支切修复分支（场景：修复特定 PR 分支的 CI/测试，不依赖 master 先行合并）
git worktree add -b fix/<name> .worktrees/<name> <target-branch>
```

**规则**：
- `<name>` = 与 PR 匹配的 kebab-case 短名（例如 `fix-candle-rendering`、`add-sector-filter`）
- 默认基于 `master`——PR 合并回 master
- **目标分支场景**：当 CI 失败在某个 feature/PR 分支（如 `fix/ci-fix-issue-only`）时，
  从**该分支**切修复分支（`<target-branch>`），修复后 merge/cherry-pick 回目标分支，
  **目标分支直接 push**（不经 PR 到 master）——目标分支自身的 PR 负责最终进入 master
- 绝不在 `.worktrees/` 之外创建工作树

**创建后步骤（MANDATORY）**——每次 `git worktree add` 之后：

1. **运行 `/handoff`** 保存当前对话上下文：
   - handoff 文件写入 `.worktrees/<name>/.omo/handoff.md`
   - 内容包含：已做出的决策、下一步计划、相关设计上下文
   - 如果 `/handoff` 命令不可用，使用 `write` 工具创建 handoff 文件

2. **⚠️ 先解绑当前 opencode session（MANDATORY）**——在打开工作树中的新 opencode 之前：
   - **原因**：opencode 将工作树目录识别为与 master 的*同一项目*
     （通过 `git_worktree` 关联，`~/.local/share/opencode/opencode.db` 中相同的 `project_id`）。
     当前在 master 中运行的 opencode 实例仍然*绑定*着该项目的 session，因此在工作树中启动新的
     `opencode` 会失败。
   - **做法**：先释放当前 session 绑定——例如退出当前 opencode
     实例（或停止/退出其 session）使项目解除绑定，*然后*在工作树中启动新的
     opencode。
   - 不要跳过此步骤。当 master session 仍处于绑定状态时，打开工作树的 opencode 会失败。

3. **告知用户**在工作树中打开新的 opencode session（仅在步骤 2 之后）：
   ```
   工作树已就绪。请在新终端中继续：
       cd .worktrees/<name> && opencode
   ```
   新的 opencode session 将自动读取 `.omo/handoff.md` 获取上下文。

4. **当前 session 留在 master 中**——不要在当前 session 中 `cd` 进入工作树。

### 列出

```bash
git worktree list
```

### 删除（PR 合并后）

PR 合并后，清理：

```bash
# 删除工作树及其分支
git worktree remove .worktrees/<name> --force
git branch -D feat/<name>
```

### 清理孤立目录

删除 `.worktrees/` 下不是活跃 git 工作树的目录：

```bash
for d in .worktrees/*/; do
  name=$(basename "$d")
  if ! git worktree list | grep -q ".worktrees/$name"; then
    echo "orphan: $d"
    rm -rf "$d"
  fi
done
```

## 与 compass-workflow 集成

当 `compass-workflow` skill 同时加载时：
- 工作树中的每个 PR 都经过 gate 流程（issue → plan → tests → docs）
- 质量检查（`cargo test`、`cargo clippy`、`cargo fmt`）在工作树内运行
- 推送到 PR 分支（`feat/<name>`），创建 PR，通过 GitHub 合并

## 示例：创建 PR 工作树

```bash
# 用户："切一个fix candle的worktree"
# → 触发此 skill，然后：
git worktree add -b feat/fix-candle-rendering .worktrees/fix-candle-rendering master
```

## 示例：从目标分支切修复分支

```bash
# 场景：PR 分支 fix/ci-fix-issue-only 的 CI 失败（flaky 测试，issue #75），
# 修复不依赖 master 先合并——从目标分支切修复分支
# 用户："从 fix/ci-fix-issue-only 切一个修 flaky 测试的 worktree"
git worktree add -b fix/deterministic-aggregation-test .worktrees/deterministic-aggregation-test fix/ci-fix-issue-only
```

修复完成后（`git commit` 到 `fix/<name>`），合并回目标分支：

```bash
# 方式 A：merge（保留修复分支历史）
git checkout fix/ci-fix-issue-only && git merge fix/deterministic-aggregation-test

# 方式 B：cherry-pick（单 commit 修复，推荐——引用清晰）
git checkout fix/ci-fix-issue-only && git cherry-pick <fix-commit-sha>

# 目标分支直接 push（不经 PR 到 master）——其自身 PR 负责最终合并
git push origin fix/ci-fix-issue-only
```

**然后**（同一回合，`git worktree add` 成功后立即执行）：

1. 运行 `/handoff` → 将当前上下文写入 `.worktrees/fix-candle-rendering/.omo/handoff.md`
2. **解绑当前 opencode session**（见上方创建后步骤 2，MANDATORY）
3. 告知用户：`cd .worktrees/fix-candle-rendering && opencode`

工作树是临时的——PR 合并后清理。
