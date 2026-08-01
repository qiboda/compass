# 摩擦记录

记录 AI 行为偏差被用户纠正的时刻。与 `reflections.md`（实施后复盘）区分：
friction 记录决策过程中的卡点和纠正，reflection 记录实施后的教训。

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

## 2026-08-01 — CI 修复批次（#54/#75/#88/#92/#83）

**我的偏差**: ① 发现 opencode-ci-fix 的 /fix 触发链断裂后，提议在 ci-fix workflow 内直接内联执行 fix agent（方案 A：加 fix job 跑 anomalyco/opencode），设计较复杂；② 默认所有 CI 修复都必须先合并到 master 才能惠及其他分支，导致 PR 互相卡进程（#75 flaky 测试修复滞留在独立分支，所有后续分支 CI 连环挂）。

**你的纠正**: ① "感觉 github 修复和本地流程混在一起有点乱，目前不需要这么复杂，直接让 ci-fix 只提交 issue 就好"——简化到只保留自动建 issue，修复由人工接手；② "我们需要有从特定分支切一个新的分支的能力，来修复特定分支……才不会互相卡进程"——落地目标分支修复工作流（从 feature 分支切修复分支 → cherry-pick 回 → 直接 push）。

**教训**: ① 修复方案先问"最小可行是什么"，再考虑自动化——GITHUB_TOKEN 触发链断裂的根因是平台限制，与其绕路建复杂链路，不如砍掉自动修复回归人工（更简单可靠）；② CI 修复的传播路径要提前设计：master 级 bug（如 flaky 测试）应单独直推 master，feature 分支的问题应从该分支切修复分支，避免所有分支排队等 master——"目标分支修复工作流"已写入 worktree skill 和 kb/dev/process.md。
