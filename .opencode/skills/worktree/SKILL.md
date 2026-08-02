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

## 创建时机（MANDATORY）

**需求经 grill-me 确认是需要 worktree 的工作（feature/epic、2+ 模块、将产出
`.omo/plans/*.md` 或 `.omo/designs/*.md`）时，grill 共识达成后立即创建并切换。**
后续的 design/issue/plan/review/实现全部在 worktree 内进行，plan/design 等 .omo
产出文件**直接在 worktree 内创建**，随实现 PR 一并提交。

**禁止**在 master 工作区先产出 plan/design 再等开 worktree 迁移——git worktree 是
独立 checkout，master 工作区的 **untracked 文件不会出现在 worktree 中**（ref #138
教训：SEPA 曾全程在 master 规划，plan/design 成 untracked，最后需手动迁移）。

判断口诀：**一旦确定"这次要产出 .omo 文件"→ 先开 worktree 再写文件**。

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

1. **确定用途 + 命名（主 session 唯一职责）**：
   - 主 session 只需确认 worktree 的用途（一句话：做什么、对应哪个 issue）并命名
   - `<name>` 即 kebab-case 短名，与 PR 匹配
   - 将用途简述写入 `.worktrees/<name>/.omo/handoff.md`（含对应 issue URL / 已锁定的 grill-me 决策）
   - 使用 `write` 工具创建 handoff 文件（不再依赖 `/handoff` 命令）

2. **自动启动工作树区域（ref #96）**——无需手动解绑当前 opencode session：
   - 运行 `scripts/open-worktrees.sh [name...]`，脚本探测 OS 默认终端
     （`$TERMINAL` → kitty/gnome-terminal/konsole/xfce4-terminal/xterm）
     并在新终端窗口中启动 `opencode`
   - 脚本通过 `setsid` 启动新进程，使其**脱离当前对话的进程组**——当前
     对话结束，新 opencode 窗口不会随之关闭；无需用户手动退出当前实例
   - 无探测到终端时，脚本打印手动运行命令

3. **剩余工作全部移交 worktree agent**——主 session 不再参与：
   - 后续的 grill-me 延续、设计（ui-designer）、计划（ulw-plan）、实现、
     commit、PR 全部由 worktree 内的 agent 自主完成
   - worktree agent 从 handoff 文件读取用途/决策/issue 上下文后自行推进
   - **所有 .omo 产出（`.omo/plans/*.md`、`.omo/designs/*.md`）直接在 worktree
     内创建**——master 工作区不产出这些文件（untracked 文件不会跨 checkout 迁移，
     ref #138）
   - 主 session 创建后即结束该任务的参与，不做实现、不跟进

4. **当前 session 留在 master 中**——不要在当前 session 中 `cd` 进入工作树。

## worktree 会话启动规则（MANDATORY）

**worktree 内的 opencode 会话启动后，第一步必须读取
`.omo/handoff.md`**（worktree 根目录，即 `.worktrees/<name>/.omo/handoff.md`；
若存在）——这是主 session
移交的上下文契约，包含：用途简述、对应 issue URL、已锁定的
grill-me 决策、下一步计划。读取 handoff 之后才允许开始任何工作
（grill-me 延续、设计、计划、实现）。

- handoff 缺失时：先向用户询问该 worktree 的用途与目标 issue，
  再补写 handoff 文件（放入 `.omo/handoff.md`），然后开始工作
- handoff 内容与用户当前指示冲突时：以用户当前指示为准，
  并更新 handoff 文件反映最新决策

### 启动/关闭工作树区域

在 OS 默认终端中打开 worktree 区域的 opencode 会话（`setsid` 自动脱离进程组，
对话结束新会话不随之关闭，ref #96）：

```bash
scripts/open-worktrees.sh            # 打开所有 worktree
scripts/open-worktrees.sh gui data   # 打开指定 worktree
scripts/open-worktrees.sh --list     # 列出可用 worktree
scripts/open-worktrees.sh --dry-run [wt...]  # 打印将执行的命令，不实际启动
scripts/open-worktrees.sh --close [wt...]    # 终止 opencode + 删除 worktree 与分支
```

**`--close`（ref #96, #104）**：当 opencode 仍在 worktree 目录中运行时，
`git worktree remove` 因目录被进程占用而失败。`--close` 先终止 cwd 指向该
worktree 的 opencode 进程、关闭其承载终端窗口（每窗口终端可靠；client-server
终端如 gnome-terminal 尽力而为，xfce4-terminal 因单实例守护进程不尝试），
再 `git worktree remove --force` + `git branch -D`，一次完成退出与清理。
**当该 worktree 存在运行中的持有进程**（包括从 worktree 内部执行时的调用者
自身——例如在该 worktree 的 opencode 会话里运行 `--close`），清理自动交给
`setsid` 脱离会话的子进程（日志 `logs/open-worktrees-close.log`），
调用者被终止后清理仍会完成：

```bash
scripts/open-worktrees.sh --close cleanup-stock-basic   # 关闭指定区域
scripts/open-worktrees.sh --close                        # 关闭所有区域
```

探测链：`$TERMINAL` → kitty/gnome-terminal/konsole/xfce4-terminal/xterm。
无探测到终端时脚本打印手动运行命令。

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

1. 将用途简述（一句话 + 对应 issue URL + 已锁定决策）写入 `.worktrees/fix-candle-rendering/.omo/handoff.md`
2. 运行 `scripts/open-worktrees.sh fix-candle-rendering` 自动启动（setsid 脱离进程组，无需手动解绑）
3. 告知用户：worktree 区域已在默认终端中打开，后续工作由 worktree 内的 agent 自主完成

工作树是临时的——PR 合并后清理。
