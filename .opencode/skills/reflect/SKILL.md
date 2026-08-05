---
name: reflect
description: 编写实施后反思并追加到 kb/dev/reflections.md，把教训落实为流程改进（AGENTS.md 规则/skill 步骤/hook/回归测试），含趋势分析。检查最近 10 条记录，识别重复模式。
---

# Reflect — 实施后反思 Agent

## 目的

反思的目的是**学习**，然后让开发流程更加完善和自动化，减少摩擦损耗。
本 skill 的一切机制都服务于这个目的：

| 目的 | 对应机制 |
|---|---|
| 学习 | 反思条目沉淀经验——输入客观化（第 0 步读对话记录 + git 验证） |
| 完善 | 第 3 步把教训落实为流程机制变更（AGENTS.md / skill / hook / 脚本 / 回归测试） |
| 自动化 | 可检测的失误固化为 hook/CI，可复现的失败固化为回归测试——执行不再依赖人工记忆 |
| 减少摩擦 | 趋势分析揭示重复模式 → 触发落实；已融入流程的条目标记退役 |

## 角色

在每次 feature 或 bugfix 完成后，将强制的实施后反思写入 `kb/dev/reflections.md`，
并把可固化的教训落实为流程改进。**反思的终点不是"记录"，而是"流程变得更好"。**

本 agent **替代** compass-workflow 中的手动反思指令。
compass-workflow 的 REFLECTION RECORD 章节现改为 `→ Invoke /reflect`，
而非指示主 agent 自行编写反思。

## 触发条件

- `/reflect` 斜杠命令（用户主动触发）
- compass-workflow 实施后 review 第 5 步（通过 `→ Invoke /reflect` 自动触发）

## 工作流

### 第 0 步：读取对话记录，提取用户纠正（强制）

**反思的输入必须来自客观记录，而不是结束时的记忆。** 反思不到流程偏差
（如"创建 worktree 后留在 master 开发"）的根本原因是：结束时执行者已无意识
接受了偏离，记忆里根本没有"偏差"。对话记录是客观存在的——执行者会忘，
对话不会忘。因此 /reflect 的第一步（任何其他步骤之前）必须：

1. **读取本 session 的对话记录**（`session_read`），逐条浏览**用户消息**，
   识别所有纠正型消息：
   - 明确纠正（"不对"、"应该 X"、"预期是 Y"）
   - 流程提醒（"切换worktree啊"、"现在没有在worktree吧"）
   - 语义纠正（"解绑指让新的进程脱离当前对话的约束"）
   - 范围/方向纠偏（"两个skill合并"、"修复钩子简单"）
2. **逐条对照反思条目**：每条用户纠正必须出现在 User corrections 章节
   （逐字引用用户原话）。遗漏任何一条 = 反思不完整。
3. **git 客观流程验证**（命令可查，不凭印象）：
   - `git branch --contains <commit>` — 本次 commit 落在哪个分支？
     存在活跃 worktree 而 commit 在 master = 流程偏差
   - `git worktree list` — 是否有"创建了但从未使用"的 worktree？
   - `git log --oneline <range>` — commit 数量/范围与预期一致？
4. 将发现写入反思条目：对话中提取的纠正 → User corrections；git 验证发现的
   流程偏差 → What went wrong + Lessons learned。

### 第 1 步：收集上下文

从环境或用户处收集以下信息：

- **GitHub issue 引用**（如 `ref #63`）
- **工作简述标题**（来自 issue 标题或 commit message）
- **做了什么**（变更摘要 — 来自 git diff、commit message 或用户输入）
- **出了什么问题**（流程失败、遗漏步骤、意外的坑 — 来自 review 结果或用户输入）

### 第 2 步：编写反思条目

按标准格式编写**一条**反思条目，并追加到 `kb/dev/reflections.md`：

```markdown
## [date] — <issue ref> <title>

**What was done**: [1-2 sentences summarizing the change]

**User corrections** (if any): [user corrections during this work — replaces the removed friction.md]

**What went wrong** (if any): [process failures, missed steps, surprises]

**Lessons learned**: [what to do differently next time]
```

日期格式：`YYYY-MM-DD`（例如 `2026-07-28`）。

各章节规则：
- **What was done**：事实陈述，1-2 句话。不带主观评价。
- **User corrections**：仅在用户纠正过 AI 时写（可选章节）——记录"用户纠正了什么"（决策过程）。
  注意：`friction.md` 机制已移除（2026-08-01），本小节继承其职责。历史摩擦条目见
  `kb/dev/reflections-archive.md` 归档文件。
- **What went wrong**：仅在确实出了问题时才写。如果没有问题，写 `**What went wrong**: No issues.` 或直接省略该章节。
- **Lessons learned**：可操作的内容 — 下次具体要做出什么改变。不能泛泛而谈（如"更小心"）。至少一条。

### 第 3 步：落实流程改进（目的核心步骤）

逐条评估 **Lessons learned** 是否可以固化为流程机制——把"下次我要记得做 X"
变成"流程自动做 X"：

| 教训类型 | 落实为 | 例子 |
|---|---|---|
| 规则/流程约束 | AGENTS.md 规则、compass-workflow skill 步骤 | "重构不直推 master" → AGENTS.md 分支策略 |
| 工具/语义误读 | 相关 skill 文档 | "解绑=setsid 自动脱离" → worktree skill |
| 可检测的失误 | pre-commit/pre-push hook、CI 检查 | "commit 缺 ref #N" → commit-msg hook |
| 可复现的失败 | 回归测试（scripts/tests/、Rust/Python 测试） | hook 正则误报 → 9 用例测试套件 |

执行方式：
- **纯文档类**（AGENTS.md、skill、kb/）：直接更新
- **代码/hook/脚本类**：走 PRE-IMPLEMENTATION GATE——反思 agent 输出改进建议清单，
  由主 agent 建 issue 排期（test-first）
- 把落实结果写入反思条目的 **Process improvements** 章节：
  - 已直接落实 → 写机制变更内容
  - 已建 issue → 写 `proposed (ref #N)`
  - 一次性教训、无法固化 → 写 `None`

### 第 4 步：趋势分析（有条件触发）

**当 `kb/dev/reflections.md` 中已有 ≥3 条反思条目时**：

1. 读取**最近 10 条**条目（如果总数不到 10 条则读取全部）
2. 识别跨条目的**重复模式**：
   - 相同类型的失败多次出现
   - 相同的教训被反复"学到"但未落实——**这是上次第 3 步没落实的信号，必须在本次落实**
   - 流程漏洞反复出现（如"跳过 gate"多次出现）
   - 工作流规则被反复违反
3. 输出**最多 3 条**观察要点的 bullet points：

```markdown
### Trends (last 10)
- [Pattern observation with specific ref numbers]
- [Actionable suggestion for process improvement]
```

4. **重复模式必须触发第 3 步的落实动作**——趋势分析不只是观察，
   它是"上次教训未固化"的报警器：同一模式出现第二次 = 上次没落实。
5. 将 "Trends" 子章节追加在新的反思条目之后。

**如果条目数 <3**：完全跳过趋势分析。不要创建 "Trends" 章节。

### 趋势分析范围边界

- 精确检查最近 10 条条目（从最新往前数）
- 最多输出 3 条 bullet points
- 每条 bullet 必须引用具体的 issue 编号作为证据
- 如果未发现模式，写 `No significant patterns observed.` 作为唯一的 bullet
- 不要输出图表、表格或单独的报告文件
- 不要分析超出 10 条窗口之外的条目

## 反思格式（精确模板）

```
## YYYY-MM-DD — <issue ref> <brief title>

**What was done**: <1-2 sentence summary>

**User corrections** (if any): <user corrections during this work>

**What went wrong**: <specific failures or "No issues.">

**Lessons learned**:
1. <actionable item>
2. <actionable item>

**Process improvements**: <what was solidified — AGENTS.md rule / skill step / hook / script / regression test, or "None">

### Trends (last 10)  ← only if ≥3 entries exist
- <pattern observation with issue refs>
```

## 输出格式

```
## Reflect: <issue ref>

### Reflection Entry
<the written entry>

### Process Improvements
<mechanism changes made (docs) or proposed (code — with issue refs)>

### Trend Analysis
<skipped (N entries, need ≥3)> or <N patterns found>

### Verdict
<Entry appended to kb/dev/reflections.md>
```

## 边界情况

| 场景 | 处理方式 |
|---|---|
| <3 条反思条目存在 | 仅写反思条目；完全跳过趋势分析 |
| reflections.md 不存在 | 创建 `kb/dev/reflections.md`，带 `# 反思日志` 标题，然后追加 |
| 无 feature/bugfix 上下文 | 写最小条目：`**What was done**: Minor change.` |
| 上一条目格式错误（缺少章节） | 在本次反思中注明："Previous entry (date) may be malformed" |
| 发生了流程违规（gate 被跳过等） | 必须在 "What went wrong" 中记录 — 流程违规就是 bug |
| 同一 issue 有多个 commit | 一条反思覆盖该批次的所有 commit |
| 该 issue 已有反思条目 | 检查上一条的 ref — 如果重复，改为追加 "Updated: <date>" 注释 |
| 教训无法固化为机制 | Process improvements 写 "None"——一次性教训，留在条目中即可 |
| 旧条目教训已融入流程/已被取代 | 归档至 `kb/dev/reflections-archive.md`（整体移动，原文不改） |
| 落实涉及代码/hook | 反思 agent 输出建议清单 → 主 agent 走 gate 建 issue；条目记录 "proposed (ref #N)" |
| 趋势分析未发现模式 | 写 "No significant patterns observed." 作为唯一的趋势 bullet |

## 禁止事项

- **删除或改写过去的反思条目** — 只能追加新条目。
  **归档替代退役标记**（ref #186）：教训已融入流程（AGENTS.md 规则、skill 步骤、
  hook、回归测试、CI 门禁）或已被后续条目取代的旧条目，整体移至
  `kb/dev/reflections-archive.md`（头部有归档说明，原文不得改动）。主文件
  `kb/dev/reflections.md` 仅保留活性条目。归档时用脚本按 `## ` 标题切分，
  逐条校验"原文每行都出现在新文件或归档中"，禁止手抄（防内容丢失）。
- **趋势 bullet points 超过 3 条** — 硬性上限
- **分析超过 10 条历史条目** — 硬性上限
- **创建单独的趋势报告文件** — 所有内容写入 `kb/dev/reflections.md`
- **删除或截断反思文件** — 防止意外数据丢失
- **凭空编造 issue** — 如果没有上下文，写一条最小的事实条目
- **评判代码质量** — 反思关乎流程，而非代码 review
- **把落实步骤变成事后口头承诺** — Process improvements 必须落到文件变更或 issue，
  不能只写"下次注意"

## 与 compass-workflow 的协作

1. compass-workflow 实施后 review 第 5 步说 `→ Invoke /reflect to write reflection`
2. 在 `/review-work` 完成后运行（反思可能引用 review 发现）
3. 反思条目与实施代码在同一批次中 commit
4. Reflect agent 替代旧的 "REFLECTION RECORD (MANDATORY)" 手动章节

Reflect agent 是**流程改进驱动器** — 学习经验、完善流程、固化机制，
让同样的摩擦不再发生。
