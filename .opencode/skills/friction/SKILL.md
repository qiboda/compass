---
name: friction
description: 记录用户纠正 AI 行为偏差的摩擦时刻。自动检测纠正并追加到 kb/dev/friction.md。
---

# Friction — 纠正记录 Agent

## 角色

记录用户纠正 AI 行为的时刻 — 行为偏差、误解、遗漏的约束或锚定偏见。
将条目存储于 `kb/dev/friction.md`。仅追加。

本 agent 是**摩擦历史记录者** — 它记录 AI 犯了什么错，以免同样的错误重演。
与 `/reflect`（实施后反思）并列，但在工作**过程中**而非事后运行。

**去重原则**（2026-08-01 更新）：friction 与 reflection 服务于同一目的（流程优化），
同一事件的 friction 与 reflection **各写一次即可**：
- friction 聚焦"用户纠正了什么"（决策过程）——保持简短，一句话概括技术教训
- 技术细节/落地措施归同日 reflection 条目
- 若当日已有同批工作的 reflection 条目，摩擦内容可直接**并入其 `User corrections` 小节**，
  不另建 friction 条目（实例：2026-08-01 CI 修复批次）

## 触发条件

- **自动检测**：当用户反驳或推翻 AI 之前的输出 →
  提示用户："记录这次摩擦到 kb/dev/friction.md？"
- **手动触发**：`/friction` 斜杠命令 → 直接追加条目

## 工作流

### 第 1 步：检测纠正

当用户说出与 AI 之前输出矛盾、推翻、纠正或扩充的内容时，将其识别为纠正事件。

检测信号：
- 用户说"不是..." / "不对..." / "应该是..." / "不仅..."
- 用户对 AI 声明的范围给出反例
- 用户补充了一个 AI 遗漏的约束
- 用户改变了 AI 的方案方向

### 第 2 步：提示用户

在纠正已解决（达成新共识）之后，询问：

> 记录这次摩擦到 kb/dev/friction.md？

不要打断正在进行的任务流。在即时纠正处理完毕后再提问，而不是在处理过程中。

### 第 3 步：追加条目

如果用户确认，按以下模板追加到 `kb/dev/friction.md`：

```markdown
## YYYY-MM-DD — <关联会话或issue>

**我的偏差**: <what the AI got wrong>

**你的纠正**: <what the user corrected>

**教训**: <actionable lesson learned>
```

如果 `kb/dev/friction.md` 不存在，先创建文件，带 `# 摩擦记录` 标题和简要说明，然后追加条目。

### 第 4 步：拒绝

如果用户拒绝，尊重其选择。静默记录 "skipped" — 不要为同一纠正再次提示。

## 输出格式

```
## Friction: <session/issue context>

### Entry
<the appended entry>

### Verdict
<Entry appended to kb/dev/friction.md> or <User declined — skipped>
```

## 边界情况

| 场景 | 处理方式 |
|---|---|
| friction.md 不存在 | 创建文件带标题，然后追加 |
| 用户拒绝记录 | 静默尊重；记录 "skipped" |
| 同一纠正被检测到两次 | 跳过 — 不创建重复条目 |
| 同一轮对话中有多次纠正 | 分别记录；每次纠正一个条目 |
| 纠正发生在 grill-me 期间 | 正常记录 — grill-me 中的纠正也是有效摩擦 |
| 同日已有同批工作的 reflection 条目 | 并入其 `User corrections` 小节，不另建 friction 条目 |

## 禁止事项

- **修改过去的摩擦条目** — 只能追加新条目
- **记录设计决策** — 设计决策写入 `kb/design/` 的决策记录章节
- **与 reflect skill 重叠** — reflections = 实施后，friction = 工作中
- **打断正在进行的工作** — 在纠正解决后再提示，而非过程中
- **评判用户的纠正** — 如实记录，不带主观评价
- **创建 issue 或修改代码** — 仅读取并写入 friction.md

## 与 compass-workflow 的协作

1. compass-workflow 规则 11（摩擦记录）："当用户纠正 AI 行为 →
   暂停并建议通过 `/friction` 记录"
2. compass-workflow agent 检测到纠正后调用本 skill
3. 本 skill 独立处理记录流程
4. 摩擦条目不单独 commit — 随下一次 commit 一起提交

## 模板参考

`kb/dev/friction.md` 中的规范条目格式：

```markdown
## YYYY-MM-DD — <关联会话或issue>

**我的偏差**: <what the AI got wrong>

**你的纠正**: <what the user corrected>

**教训**: <actionable lesson>
```
