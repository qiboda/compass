---
name: issue-workflow
description: 管理完整的 issue 生命周期 — 单 issue 创建、epic/子 issue 分解、批次处理、批次关闭。用于本仓库的任何 issue 创建或管理。
---

# Issue Workflow — Issue 生命周期 Agent

## 角色

管理 compass 项目的完整 GitHub issue 生命周期。处理
单 issue 创建（从 compass-workflow 门禁第 2 步委派而来）、
使用 GitHub 原生子 issue 进行 epic 分解、通过计划文件
跟踪批次执行、以及 PR 合并后的批次关闭。

## 触发方式

- `/issue-workflow` 斜杠命令（用户发起）
- compass-workflow 预实现门禁第 2 步自动化（通过 `→ 调用 /issue-workflow`）

## 模式

本 skill 根据上下文以两种模式运行：

| 模式 | 触发条件 | 流程 |
|---|---|---|
| **单 issue** | compass-workflow 门禁第 2 步，单一 issue 需求 | 创建一个 issue → 展示 URL → 完成 |
| **Epic + 子 issues** | `/ulw-plan` 产生 2+ 任务批次且各批次有独立交付物 | 创建 epic → 批量创建子 issues → 计划中跟踪 → 批次关闭 |

## 工作流

### 阶段 0：确定模式

1. 阅读 grill-me 总结和 `/ulw-plan` 输出（`.omo/plans/<name>.md`）
2. 如果计划有 2+ 任务批次且各批次有独立交付物 → **Epic 模式**
3. 否则 → **单 issue 模式**

### 阶段 1A：单 issue 模式

1. 使用合适的模板创建 issue：
   ```sh
   gh issue create \
     --title "<title>" \
     --body-file /tmp/issue-body.md \
     --label "A-<area>,C-<category>"
   ```
2. 用 `gh issue view <N>` 验证
3. 如适用，在 `.omo/plans/<name>.md` 中记录 issue 编号
4. 将 issue URL 返回给调用方工作流

### 阶段 1B：Epic 模式 — 创建 epic

1. 首先创建 **epic**（父 issue）：
   ```sh
   gh issue create \
     --title "<epic title>" \
     --body-file /tmp/epic-body.md \
     --label "A-<area>,C-Feature"
   ```
   Epic body 包含：动机、范围概述、指向 `.omo/plans/<epic>.md` 的链接。

2. 用 `gh issue view <epic-N>` 验证

### 阶段 1B：Epic 模式 — 创建子 issues

1. 从 `.omo/plans/<epic>.md` 中提取每个任务批次中属于独立交付物的项目。

2. 对每个子 issue，使用 `--parent` 标志创建：
   ```sh
   gh issue create \
     --title "<sub-issue title>" \
     --body-file /tmp/sub-issue-body.md \
     --label "A-<area>,C-Feature" \
     --parent <epic-N>
   ```

3. 子 issue body 模板：
   ```markdown
   > **Parent**: #<epic-N>
   > **Plan**: .omo/plans/<epic-name>.md
   > **Batch**: <N>
   > **Depends on**: #<sub-X>, #<sub-Y>（如无则为 "—"）

   ## 描述
   <来自计划的任务描述>

   ## 验收标准
   <来自计划的验收标准>
   ```

4. 创建所有子 issues 后，更新 `.omo/plans/<epic>.md`：
   - 将每个任务行的 `Issue` 列填入子 issue 编号
   - 初始状态设为 `pending`
   - 记录依赖关系

### 阶段 2：批次跟踪

`.omo/plans/<epic>.md` 文件是权威的跟踪文档。其 `## Tasks` 章节使用 Markdown 表格：

```markdown
## Tasks

### Batch 1
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #12 | Implement XYZ | — |
| pending | #13 | Implement ABC | #12 |

### Batch 2
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #14 | Integration tests | #12, #13 |
```

状态值：`pending` | `in_progress` | `done`

**批次切换规则**：当 agent 完成当前批次的所有子 issues 后，必须：
1. 将所有已完成的子 issue 状态更新为 `done`
2. 向用户报告批次完成（列出：哪些子 issues 已完成、哪些 PR 已合并）
3. 等待用户确认后再开始下一批次
4. 确认后，将下一批次的子 issues 标记为 `in_progress` 并继续

### 阶段 3：执行期间新增子 issues

如果在实现过程中发现新的工作项：
1. 向计划文件的相应批次（或新批次）中添加任务行
2. 创建子 issue：
   ```sh
   gh issue create --title "..." --body-file /tmp/new-sub.md --label "..." --parent <epic-N>
   ```
3. 在计划表格中填入 Issue 列
4. 重新评估 DAG 依赖——被新子 issue 阻塞的项目在后者完成之前
   不进入 `in_progress`

### 阶段 4：批次关闭

在包含所有子 issue commit 的 PR 合并到 `master` 之后：

1. 关闭所有子 issues：
   ```sh
   gh issue close <sub-N1> <sub-N2> <sub-N3>
   ```

2. 在每个子 issue 上记录 PR：
   ```sh
   gh issue comment <sub-N> --body "Fixed by #<PR-N>"
   ```

3. 关闭 epic：
   ```sh
   gh issue close <epic-N>
   ```

4. 在 epic 上记录总结：
   ```sh
   gh issue comment <epic-N> --body "All sub-issues completed:
   - #<sub-N1>: <title>
   - #<sub-N2>: <title>
   Fixed by #<PR-N>"
   ```

## 输出格式

### 单 issue 模式
```
## Issue: #<N> — <title>
URL: https://github.com/qiboda/compass/issues/<N>
Labels: <labels>
```

### Epic 模式 — 创建
```
## Epic: #<epic-N> — <epic-title>
URL: https://github.com/qiboda/compass/issues/<epic-N>

### Sub-issues (Batch 1)
| # | Title | Depends On |
|---|-------|------------|
| #<sub-N1> | <title> | — |
| #<sub-N2> | <title> | #<sub-N1> |

### Sub-issues (Batch 2)
| # | Title | Depends On |
|---|-------|------------|
| #<sub-N3> | <title> | #<sub-N1>, #<sub-N2> |
```

### Epic 模式 — 批次完成
```
## Batch <N> Complete
Epic: #<epic-N>

Completed:
- #<sub-N1> <title> — merged in PR #<PR-N>
- #<sub-N2> <title> — merged in PR #<PR-N>

Pending (Batch <N+1>):
- #<sub-N3> <title> — blocked by: —

Proceed to next batch? (confirm to continue)
```

### Epic 模式 — 最终关闭
```
## Epic #<epic-N> Complete
All sub-issues closed. Epic closed.
PR: #<PR-N>
```

## 边界情况

| 场景 | 行为 |
|---|---|
| 计划有 2+ 批次但只有一个交付物 | 使用单 issue 模式（批次 ≠ 子 issues） |
| 计划无任务批次 | 使用单 issue 模式 |
| 子 issue 创建失败（网络/GitHub） | 重试一次；仍失败则报告错误及失败的 `gh` 命令 |
| Issue 编号 > 999（GitHub 自动编号） | 接受——GitHub 编号自动递增，无固定范围 |
| 用户想在批次启动后添加子 issue | 阶段 3——添加到计划、创建 issue、重新评估 DAG |
| DAG 有循环 | 报告错误——通过遍历 Depends On 链检测，在任何 issue 创建前中止 |
| Epic body 超出 GitHub 限制 | 将范围概述拆分到计划文件；epic body 引用计划获取详情 |
| 用户说"跳过批次确认，自动继续" | 照办——切换为自动模式；仍报告每个批次完成 |
| 已有 issue 需成为子 issue | `gh issue edit <parent> --add-sub-issue <existing-N>` |
| 子 issue 来自不同仓库 | GitHub 支持跨仓库子 issues——使用完整 URL：`gh issue create --parent https://github.com/owner/repo/issues/N` |

## 禁止事项

- **PR 合并前自动关闭 issues**——仅在合并到 `master` 后才关闭
- **跳过批次确认**——除非用户明确要求自动模式
- **删除计划文件条目**——只更新状态，绝不删除行
- **创建无父 issue 的子 issues**——始终使用 `--parent` 标志
- **在 commit 中使用 `fixes #N` 或 `closes #N`**——只用 `ref #N`（通过 `gh issue close` 手动关闭）
- **修改 compass-workflow skill**——issue-workflow 是同级协作，非替代
- **为非 feature/bugfix 工作创建 issues**——文档、lint、typo 修复完全跳过 issue 创建

## 与 compass-workflow 的协作

1. compass-workflow 门禁第 2 步显示 `→ 调用 /issue-workflow 创建/管理 issues`
2. 在门禁第 2 步，compass-workflow 调用 issue-workflow 创建 issue(s)
3. issue-workflow 决定单 issue 还是 epic 模式，创建 issues，返回结果
4. compass-workflow 对每个子 issue 独立执行门禁步骤 3-5b
5. 所有子 issues 完成后 → PR 合并 → issue-workflow 处理批次关闭
6. compass-workflow 处理 commit、test、review 和 push（不变）

## 参考资料

- `AGENTS.md` — 完整项目工作流和门禁规则
- `kb/dev/process.md` — 开发流程文档
- `kb/github/labels.md` — 标签分类体系
- `.omo/plans/` — 包含任务表格和 DAG 依赖的计划文件

## Issue Body 模板（子 Issue）

创建子 issue 时，body 文件（`/tmp/sub-issue-body.md`）遵循此模板：

```markdown
> **Parent**: #<epic-N>
> **Plan**: .omo/plans/<epic-name>.md
> **Batch**: <N>
> **Depends on**: #<sub-X>（如无则为 "—"）

## 描述
<任务描述>

## 验收标准
<验收标准>
```

## 计划文件任务表格格式

`.omo/plans/<epic>.md` 任务表格的规范格式：

```markdown
### Batch <N>
| Status | Issue | Task | Depends On |
|--------|-------|------|------------|
| pending | #<N> | <单行描述> | — |
| in_progress | #<N> | <单行描述> | #<X> |
| done | #<N> | <单行描述> | #<X>, #<Y> |
```

- `Status`：取值为 `pending`、`in_progress`、`done`
- `Issue`：GitHub issue 编号（带 `#` 前缀），如尚未创建则为空
- `Task`：来自计划的单行描述
- `Depends On`：逗号分隔的 issue 编号，如无则为 `—`

每个 worktree 每次只能有一个任务处于 `in_progress` 状态。
同一批次中的独立任务（无相互依赖）在使用并行子 agent 时
可以同时处于 `in_progress`。
