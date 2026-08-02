---
name: compass-workflow
description: 强制执行 compass 项目工作流 — issue 驱动开发、文档同步、测试先行、逐步验证、提交纪律。用于本仓库的任何 feature、bugfix 或代码变更。
---

# Compass 工作流

本项目遵循严格的工作流。每次代码变更都必须执行以下规则。

---

## 🛑 触发：预实现门禁（立即执行）

**加载此 skill 的瞬间，你即进入门禁模式。**

**前置条件**：进入门禁之前，grill-me（第 0 步）必须已完成"shared understanding
reached"。如果尚未调用 `/grill-me`，请返回并先完成。

在创建任何 todos、读取任何源文件、编写任何代码之前——你必须向用户逐一确认以下检查清单。

```
🛑 预实现门禁

在继续之前，我将逐项检查门禁的每个步骤：

☐ 第 0 步 — GRILL-ME（前置条件）
   已达成 shared understanding
   → [必须确认]

☐ 第 0.5 步 — WORKTREE（grill 共识后立即判断，ref #138）
   需求是否需要 worktree？（feature/epic、2+ 模块、将产出 .omo/plans/*.md
   或 .omo/designs/*.md）
   → 需要则**立即创建并切换**（/worktree）：grill 共识达成后、产出任何 .omo
     文件之前。plan/design 直接在 worktree 内创建，随实现 PR 提交。
   → 不需要（单文件修复/纯 docs）→ 跳过，继续 master 工作区。
   → [必须展示 worktree 名称与 handoff 已写入]

☐ 第 1 步 — DESIGN（仅界面相关变更强制）
   涉及界面布局/视觉风格/交互效果的工作，先委派 ui-designer
   产出 .omo/designs/<feature>.md 设计方案，并经用户确认；
   确认后须将最终设计要点同步到 kb/design/ui.md（权威文档）
   → [必须展示设计方案要点 + 用户确认；纯逻辑/数据变更可跳过]

☐ 第 2 步 — ISSUE
   → 调用 /issue-workflow 创建/管理 issues
   → [必须向用户展示 issue URL，或 epic + 子 issue 列表]

☐ 第 3 步 — PLAN（仅单文件变更可跳过）
   计划 agent 已运行且已获批准
   → [必须展示计划摘要]

☐ 第 4 步 — TESTS（RED 阶段）
   → 调用 /test（qa skill）编写失败测试
   → [必须展示测试失败输出]

☐ 第 5a 步 — RUSTDOC
   → 调用 /rustdoc 验证 #[warn(missing_docs)] 合规
   → [必须展示 cargo doc --no-deps 无警告]

☐ 第 5b 步 — DOCS（kb/）
   → 调用 /docs 识别并更新 kb/ 文件
   → [必须列出文件清单]

☐ 第 5c 步 — 决策记录
   → 检查相关 kb/design/ 文件是否包含 ## 决策记录 章节
   → 如缺失，先补充再继续
```

**在上述所有门禁步骤（1-5c）完成并向用户展示之前，
严禁使用任何 edit/write/bash 工具进行实现。**

如果你发现自己在门禁未完成时就开始编写代码，立即停止，
回到第 0 步。这是硬性阻断——feature/bugfix 工作无例外。

### 例外（可跳过门禁的情况）

门禁不适用于：
- 纯文档变更
- Lint 修复
- Typo 修复
- 为已有代码添加测试

> ⚠️ **跳过门禁并不意味着跳过实现后审查。**
> 下方"实现后审查"章节适用于所有变更，
> 包括纯文档变更。门禁和审查是两个独立流程——
> 门禁是预实现阶段，审查是实现后阶段。

### 门禁完成信号

当所有步骤完成后，明确宣布：

```
✅ 门禁完成 — 进入实现阶段
```

只有此时才能创建 todos 并开始编辑文件。

---

## 规则（按优先级排序）

### 1. 文档同步（关键）

任何影响行为、公开 API、数据结构、配置或工作流的代码变更，必须在同一次 commit 中更新相关 `kb/` 文件和 `AGENTS.md`。

权威的「变更类型 → kb/ 文件」映射表见 `.opencode/skills/docs/SKILL.md` § Change → kb/ Mapping Table。速查：

| 变更类型 | 需更新的 kb/ 文件 |
|---|---|
| 新增数据源、API 调用、schema 变更 | `kb/design/data-providers.md` |
| 线程、管线、库变更 | `kb/design/architecture.md` |
| 符号格式、timeframe 映射 | `kb/design/symbols.md` |
| 测试框架、模式 | `kb/dev/testing.md` |
| 工作流、hooks、约定 | `kb/dev/process.md` |
| 项目级约定 | `AGENTS.md` |

### 2. 需求流程（关键）

编写任何 feature 或 bugfix 代码之前：
a) 调用 `/issue-workflow` 处理 issue 创建和管理
b) issue-workflow skill 决定单 issue 还是 epic/子 issue 模式
c) 确认 issue(s) 已创建且可见
d) 然后才实现

以下情况跳过：重构、文档、lint 修复、typo。

提交引用：`ref #N`（feat/fix），不使用 `fixes #N` / `closes #N`（避免自动关闭）。
Epic 工作中，每个 commit 引用其子 issue（`ref #<sub-N>`）。

**这适用于所有 commit——chore、docs、scripts 均无例外。**

### 3. 计划先行（`/ulw-plan`）

**多步工作不可妥协。** 以下情况必须运行 `/ulw-plan`：多步任务（2+ 模块）、架构变更、新增数据源、需求范围模糊。

计划 agent 生成 `.omo/plans/*.md` 文件，包含任务批次排序和验证门禁。不要跳过这一步自己口头描述计划——agent 的结构化输出才是批准的执行契约。

仅以下情况可跳过计划：真正的单文件修复、测试添加、文档更新。

### 4. 测试先行

Feature 和 bugfix 工作遵循 RED → GREEN → REFACTOR：
- 先写失败测试，确保因正确原因失败
- 然后实现
- 探索性变更可以先写代码后补测试
- 纯重构：先用特征测试锁定当前行为

### 5. 逐步验证

每次代码变更后：
- `cargo test` → 必须全部通过
- `lsp_diagnostics` 在变更文件上无错误

### 6. 提交前本地验证

```sh
cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

三者全部通过后才可 `git push`。

### 7. 禁止类型逃逸

- 生产代码中永不使用 `unwrap()` —— 使用 `.expect(msg)` 或正确的错误处理
- 永不使用 `as` 类型转换或类似手段压制类型错误

### 8. 分支策略

Feature 分支工作流：大部分工作在分支上进行，通过 PR 合并。
简单修复（typo、配置、单行变更）可直接提交到 master。

**Worktree 是分支策略的强制部分**：一旦创建 worktree（`git worktree add`），
后续实现工作必须完成交接闭环（add → 写 handoff → `scripts/open-worktrees.sh <name>`
启动会话）并在 worktree 内进行——master 上的实现继续即流程违规。
worktree 会话启动后第一步读取 `.omo/handoff.md` 获取上下文，剩余工作全部
由 worktree 内 agent 自主完成。master 只允许纯文档（docs/lint/typo/反思）直推。

开始实现前用 `git worktree list` + `git branch --contains HEAD` 确认所在分支；
不确认分支归属就不开始。

```
master  ●──●──●──●────────●  (主干，仅 docs/lint/typo/反思)
               \          /
feat/xxx      ●──●──●──┘   (worktree 分支，通过 PR 合并)
```

### 9. 标签强制

创建 GitHub issue 或 PR 时：
- 必须附加至少一个 **A-**（area，领域）和一个 **C-**（category，分类）标签。
- **D-**（difficulty，难度）、**P-**（priority，优先级）和 **S-**（status，状态）可选但建议添加。

完整分类体系见 `kb/github/labels.md`。

`gh issue create --label "C-Bug,A-Data"` 或 `gh pr create --label "C-Feature,A-GUI"`。

### 10. Sprint 节奏

使用 GitHub Milestones 进行每周 sprint 管理。周一（规划）→ 周日（回顾）。

- **周一**：规划 sprint —— 查看 open issues（需求池即 open issues，不再有 `backlog.md`），调用 `/product brainstorm` 获取 milestone 候选
- **周日**：回顾已完成工作，所有 issues 完成后关闭 milestone，调用 `/reflect`
- 手动触发：随时调用 `/product brainstorm` 获取新的候选

### 11. 摩擦记录（并入反思）

当用户纠正 AI 行为（矛盾、范围扩张、约束遗漏、方案偏离）时——
在写事后反思（`/reflect`）时一并记录，不再单独使用 friction 机制。

- 涵盖所有纠正性交互，不限于 grill-me 分歧
- 在 reflection 条目的 **User corrections** 小节记录"用户纠正了什么"
  （`friction.md` 机制已移除，历史摩擦条目见 `reflections.md` 末尾"历史摩擦记录"章节）
- 在纠正解决后提示用户记录，而非活跃工作期间

---

## 📋 可用 Skills

Compass 项目为特定工作流步骤提供以下 opencode skills：

| Skill | 斜杠命令 | 用途 | 门禁步骤 |
|---|---|---|---|
| ui-designer agent | `task(subagent_type="ui-designer")` | 界面布局/视觉风格/交互效果设计，产出 `.omo/designs/` 方案 | 第 1 步 — DESIGN |
| issue-workflow | `/issue-workflow` | 创建和管理 issues（单 issue + epic/子 issue） | 第 2 步 — ISSUE |
| qa（test） | `/test` | 编写失败测试（TDD/BDD）、测试覆盖 | 第 4 步 — TESTS |
| rustdoc | `/rustdoc` | 验证 `#![warn(missing_docs)]` 合规 | 第 5a 步 — RUSTDOC |
| docs | `/docs` | 识别并更新 kb/ 文件 | 第 5b 步 — DOCS |
| reflect | `/reflect` | 编写实现后反思（含 User corrections）+ 趋势分析 | 实现后 |

当门禁清单显示 `→ 调用 /<command>` 时，加载对应的 skill 并按其工作流执行。
每个 skill 的详细说明见 `.opencode/skills/<name>/SKILL.md`。

---

## 🔍 实现后审查（自动化）

实现完成后，运行自动化审查以在变更进入仓库前捕获问题。
旧的检查清单已被以下流程替代。

### 第 1 步：提交

先提交实现——始终如此。不要在提交前运行审查。

```
git add <files>
git commit -m "feat: description

ref #N"
```

### 第 2 步：运行审查

对当前变更触发 `/review-work`。审查会并行运行 5 个 agent：
目标验证、QA 执行、代码质量、安全审计和上下文挖掘。

**Epic 工作**：两层审查——
- **每个子 issue**：每个子 issue commit 后，审查该子 issue 的变更
- **PR 前**：所有子 issue 完成后，审查完整 PR diff 以发现集成问题

### 第 3 步：处理发现的问题

针对审查报告的每个问题：

| 问题类型 | 处理方式 |
|---|---|
| 与当前工作相关，影响 ≤3 个文件 | 直接自动修复 |
| 与当前工作无关 | 创建 GitHub issue（`gh issue create`） |
| 相关但影响 >3 个文件 | 创建 GitHub issue |

以审查 agent 的 `blocking_issues` 为主要输入。
范围内 = 本 PR/变更所涉及的文件和模块内的修复。

### 第 4 步：重新审查（最多 2 轮）

修复问题后，重新运行审查以验证修复正确。
如果在 2 轮后审查仍然报告阻塞问题，为剩余问题
创建 issues 并在 commit message 中注明。

### 第 5 步：完成

- 所有范围内问题已解决 → 提交，等待用户 push 指令
- **用户确认 push 后、执行 push 前**：调用 `/reflect` 编写实现后反思，
  反思 commit（含 `ref #N`）与实现代码**同批 push**，随 PR 合并落在 master
  ——不要在 push/合并后才写反思（ref #119 教训：合并后 issue 已关闭，
  commit-msg hook 拒绝 `ref #N`，反思 commit 只能摘 patch 单独直推）

### 第 6 步：Push 后关闭 issue（强制，勿忘）

**push 成功到达 `origin/master` 后**，必须完成 issue 收尾——这是流程的
一部分，不是可选项：

1. **追加完成 comment**（`gh issue comment <N>`，遵守 comments.md"永远追加"规范）：
   - 实现摘要 + 验收标准逐项状态（✅/⛔）
   - commit 列表（`git log --oneline origin/master@{1}..HEAD` 或等价范围）
   - 与 issue 原方案的偏差及原因（如方案被外部约束阻断、用户批准放弃）
2. **关闭 issue**（`gh issue close <N>`）——HARD BLOCK：只在 push 后关闭，
   push 前绝不关闭。
   - 单 issue：直接关闭
   - Epic：先关所有子 issues（每个注明 `Fixed by #<PR-N>`），再关 epic
     并在 epic 上记录总结 comment

> **教训来源**（ref #117）：agent 完成 push 后没有自动追加完成 comment 和
> 关闭 issue，用户提醒"需要comment"才补做。push 成功 ≠ 任务完成——issue
> 收尾（comment + close）必须作为强制步骤执行，不依赖用户提醒。

---

## 📝 反思记录

每次 feature 或 bugfix 实现后，调用 `/reflect`（reflect skill）
编写实现后反思并追加到 `kb/dev/reflections.md`。
这替代了之前的手工反思要求——reflect skill 负责
编写、格式和趋势分析。

**时机（强制）**：用户确认 push 后、执行 push 前——反思 commit 与实现
同批 push 随 PR 合并落在 master；不要在 push/合并后才写（ref #119 教训）。

完整反思工作流见 `.opencode/skills/reflect/SKILL.md`。

---

## 提交风格

- `feat:` / `fix:` / `test:` / `refactor:` / `docs:` / `chore:`
- 原子提交：每次提交一个逻辑单元
- 每个 commit 引用其 issue：`ref #N`（epic 工作引用子 issue）
- 一个 PR 可以包含多个子 issue commit（每个带有各自的 `ref #<sub-N>`）
- 仅在用户明确指令时推送（绝不自动推送）

## 代码风格

- Rust edition 2024、thiserror、async-trait、tracing
- 遵循所编辑文件中的现有约定
