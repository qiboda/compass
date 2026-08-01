# 摩擦记录

记录 AI 行为偏差被用户纠正的时刻。与 `reflections.md`（实施后复盘）区分：
friction 记录决策过程中的卡点和纠正，reflection 记录实施后的教训。

**去重原则**：同一事件的 friction 与 reflection 各写一次即可——friction 聚焦"用户纠正了什么"（决策过程），技术细节/落地措施归 reflection。friction 条目如涉及技术教训，用一句话概括并指向对应 reflection 条目，不重复展开。

条目格式（与 `/friction` skill 一致）：

```markdown
## YYYY-MM-DD — <关联会话或issue>

**我的偏差**: <AI 做错了什么>

**你的纠正**: <用户如何纠正>

**教训**: <下次怎么避免>
```

---

## 2026-07-30 — #69 grill-me

**我的偏差**: 将摩擦记录的范围限定为 grill-me 中的分歧，认为只有 grill-me 场景需要记录。

**你的纠正**: 摩擦记录不应局限于 grill-me，应该是**任何「我做了/说了 X，你纠正为 Y」的场合**——包括执行方向偏离、意图误解、约束遗漏等所有纠正型交互。

**教训**: 不要被用户给出的例子锚定（anchoring bias）。用户举的例子是示意，不是边界定义。正确做法是追问范围边界，而非默认例子就是全部。

## 2026-07-30 — 三张财务报表管线 review 修复

**我的偏差**: review 发现「Dolt 不支持 `RENAME TABLE IF EXISTS`」等问题后，直接修改代码（`33f44ec`），没有先写失败测试锁定 bug。

**你的纠正**: 「发现问题首先应该写测试，而不是直接写代码」。TDD 的 RED→GREEN 流程：先写能复现 bug 的测试，确认它失败，再修代码让测试通过。

**教训**: 任何 bug 修复都必须先有失败测试。即使问题看起来"简单清楚"，跳过测试直接改代码就是违反 test-first 纪律——这会丢失回归保护，也无法证明修复真正有效。修复与测试应成对出现，测试先行。

## 2026-07-31 — worktree opencode 启动失败 (#76)

**我的偏差**: 创建 worktree（`fix-stock-basic-scope`）后直接在新终端启动 opencode，未先解绑当前 master 的 opencode session，导致 worktree 中的 opencode 启动失败。

**你的纠正**: 「开启新的 opencode，要先解绑当前的 opencode 的 session」。opencode 将 worktree 目录映射到与 master 相同的 project_id（`git_worktree` 关联），master 实例仍绑定该 project 的 session 时，worktree 新实例无法启动。该经验已写入 worktree skill 的 Post-Creation MANDATORY 步骤。

**教训**: 涉及 opencode/git 工具的跨目录操作，先确认工具对 worktree 的特殊处理（session/project 绑定模型），再执行启动动作。教训应沉淀到 skill 文档本身（而非仅 friction），确保后续所有 agent 在流程上不会重犯。

