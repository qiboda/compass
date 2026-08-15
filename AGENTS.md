# AGENTS.md — compass

A-share 股票图表桌面应用（egui）。数据管线以本地 Dolt `investment_data` 为**主数据源**
（18M+ 行，6000+ 标的）。GUI 只读本地 Parquet 文件（DuckDB 查询），**无在线回退**。
Python collectors 抓取数据写入 Dolt（财务数据来自 EastMoney；stock_basic 来自三大交易所官网）。

**项目书** = 本项目所有规则与知识文件的统称，包括 `AGENTS.md` 和 `.dsh/kb/` 目录下所有文件。

**默认对话语言：中文。** 所有回答、解释、讨论默认使用中文，代码注释和提交信息按惯例使用英文。

---

## 品质准则

精益求精，追求完美。每一行代码、每一次提交、每一个决策，都应以最高标准衡量。容不得将就、凑合、差不多。

- 代码不行就重构，不要留着凑合
- 设计不对就推翻，不要叠加补丁
- 流程有漏洞就堵，不要绕过去
- **禁止依赖视觉表现来 debug**：UI 问题必须用客观证据定位（代码逻辑、测试断言、日志、像素采样），不靠"看起来对不对"猜
- **agent 可自行完善项目书**：发现重复摩擦或可预防的失误时，agent 有权在 AGENTS.md / `.dsh/kb/` 中添加或修订规则以改善自身行为——规则变更随当次 commit 提交并在 commit message 中说明理由（ref #N）
- **问题处理闭环（强制）**：执行中遇到**任何**异常（工具失败、命令报错、配置错误、数据不一致、流程障碍、输出可疑）时，**禁止静默绕过或静默降级**——包括"改用替代工具""忽略错误继续""跳过步骤""换个说法糊弄过去"。必须依次完成：
  1. **感知**：停下，识别这是问题，不把绕行当解决
  2. **诊断**：用客观证据定位根因（日志、环境变量、复现实验、对比验证），不猜
  3. **处理**：修复根因；仅在确认根因无法修复时允许 fallback，且必须在记录中说明
  4. **记录**：根因与排查路径沉淀到 `.dsh/kb/dev/toolchain.md`（问题排查卡）或 reflections.md，使其可复用

  绕行本身就是违规，无论结果多顺利。本规则覆盖 MCP 401、编译错误、测试失败、hook 拒绝等一切异常（ref #159）。

---

## ⚡ GRILL-ME FIRST (ALWAYS)

**每次用户消息都必须先加载 `/grill-me` 再回应。** 无任何例外。

grill-me 访谈必须达到 "shared understanding reached" 才能进行任何其他操作——
包括读文件、分类请求、创建 todos、写代码。

**Grill-me 完成后 → 任何 feature 或 bugfix 工作必须进入下面的 PRE-IMPLEMENTATION GATE。
Grill-me 是第 0 步；gate 是第 1-5c 步。不要因为 grill-me 已达成共识就跳过 gate。**

**子代理（delegated agents）例外**：子代理收到的是主 agent 的委托任务而非用户
消息——不强制触发 grill-me。子代理自行判断任务是否需要澄清：有设计歧义时自行
选择是否发起 grill 式澄清；无论是否选择，都将判断与结果报告主 agent，由主 agent
汇总给用户。

---

## 🛑 PRE-IMPLEMENTATION GATE (任何代码变更前必读)

**本 gate 适用于所有代码变更。** 唯一例外：
- 纯文档变更（typo、格式、补充说明）
- Cargo fmt / clippy 修复（CI 已覆盖）
- 注释或字符串中的 trivial typo

**除此之外的一切 —— feature、bugfix、重构、新命令、CI 变更、hooks、脚本、
依赖更新 —— 必须走 gate。**

动手改任何文件之前，向用户逐条 verbalize 以下步骤并确认完成：

| Step | 动作 | 所需证据 |
|---|---|---|
| **0.5. Worktree** | 需求是否需要 worktree？（feature/epic、2+ 模块、将产出 `.dsh/plans/*.md` 或 `.dsh/designs/*.md`）→ 需要则**立即创建并切换**（`/skwy-worktree`），plan/design 直接在 worktree 内创建；不需要则跳过 | worktree 名称 + `.dsh/handoff.md` 已写入 |
| **1. Design** | 涉及界面设计时：委派 `ui-designer` 产出 `.dsh/designs/<feature>.md` 方案并经用户确认；纯逻辑/数据变更可跳过 | 展示方案要点 + 用户确认 |
| **2. Issue** | 调用 `/skwy-github-workflow` 创建/管理 issue | 向用户展示 issue URL |
| **3. Plan** | 涉及 2+ 模块时运行 `/ulw-plan` agent 直到批准 | `.dsh/plans/*.md` 文件创建 + 用户批准 |
| **3.5. Adversarial Tests** | 委派 `skwy-adversarial-test` 写对抗性测试（RED；plan 无接口契约时返回 DEFERRED，首个可编译接口 commit 后携带 SHA 重新委派） | 测试失败输出 / DEFERRED 记录 |
| **4. Tests** | 委派 `skwy-requirement-test` 写失败测试（需求验收 RED） | 测试失败输出 |
| **5b. Docs** | 按 `skwy-workflow` 技能内嵌「文档同步」章节确定哪些 `.dsh/kb/` 文件需更新 | 向用户列出文件清单 |
| **5c. 决策记录** | 检查相关 `.dsh/kb/design/` 文件是否含 `## 决策记录` 章节 | 缺失则补齐后再继续 |

**任何一步未完成即 STOP。不实现。不创建 todos。不改文件。**

### SELF-CHECK（强制 —— 每次代码编辑前问自己这 5 个问题）

1. **"这项工作有 GitHub issue 吗？"** — 没有就 NOW 创建。
2. **"我的 commit message 包含 `ref #N` 吗？"** — 没有就加。
3. **"我先写了失败测试吗？"** — 没有就先写再实现。
4. **"我更新了相关 .dsh/kb/ 文件吗？"** — 没有就确定文件并更新。
5. **"当前工作在正确的分支/worktree 上吗？"** — 存在活跃 worktree 时
   （`git worktree list`），实现工作必须在 worktree 内进行；master 只允许
   docs/lint/typo/反思类提交直推。不确认分支归属就不开始。

这 5 个问题不是可选的。它们是最低标准。跳过任何一个就是违反工作流。

**Test-first 不可妥协**：任何 bugfix 或 feature 变更必须从能复现问题的失败测试开始
（RED），再做让它通过的修复（GREEN）。适用于 Python（`collectors/tests/`）、
Rust（`#[cfg(test)]`）以及本仓库所有语言。先写修复再写失败测试是反模式 ——
见 `.dsh/kb/dev/reflections-archive.md` 历史摩擦记录章节（test-first 教训）。

### HARD BLOCK

本 gate 不可妥协。加载 `skwy-workflow` skill 时会再次提醒此 gate。
如果发现自己没完成这些步骤就在写代码，即违反工作流——立即停止，
`git stash` 或 revert，回到第 0 步。

**流程违规本身就是 bug。** 跳过 gate 的工作无论代码质量如何都是不完整的。
在 reflections 中记录违规。

### 实现后：Reflection Record

每次 feature/bugfix 完成后，调用 `/skwy-reflect`（skwy-reflect skill）写事后反思，
追加到 `.dsh/kb/dev/reflections.md`。

**反思时机（强制）**：在**用户确认 push 之后、执行 push 之前**编写并提交
反思 commit——反思随当前 PR 一起推送合并，天然落在 master 上。不要在
push/合并后才写：届时 issue 可能已关闭（commit-msg hook 拒绝已关闭 issue
的 `ref #N`），且反思 commit 只能脱离 PR 单独直推（ref #119 教训：
合并后反思被迫 reopen issue + 摘 patch 移 master）。

这是强制要求 —— 反思 commit 与实现代码同批 push。

---

## Workflow (MANDATORY)

所有 **feature** 和 **bugfix** 工作 MUST 加载 `skwy-workflow` skill。
它强制执行：issue 驱动开发、doc-sync、test-first、分步验证、commit 纪律。

**加载 skill 后**：立即按上面的 PRE-IMPLEMENTATION GATE 检查清单走一遍，一步不跳。

### Available Skills

| Skill | Slash Command | 用途 |
|---|---|---|
| `skwy-workflow` | `/skwy-workflow` | 强制执行 issue 驱动开发、doc-sync、test-first、分步验证、commit 纪律 |
| `skwy-github-workflow` | `/skwy-github-workflow` | 创建和管理 issues（单 issue + epic/sub-issue 分解与批量关闭） |
| `skwy-git-workflow` | `/skwy-git-workflow` | git 提交纪律（ref #N、Never auto-push、commit→review、push 前 rebase） |
| `skwy-requirement-test` | `/skwy-requirement-test` | 面向需求的验收测试（TDD/BDD、测试覆盖） |
| `skwy-adversarial-test` | `/skwy-adversarial-test` | 对抗性测试工程师（门禁 3.5 步，刁钻但真实有效的对抗性测试） |
| `skwy-reflect` | `/skwy-reflect` | 写事后反思（含 User corrections + 趋势分析） |
| `skwy-worktree` | `/skwy-worktree` | 管理 PR 开发的 git worktrees（创建/删除/启动区域） |
| `product` | `/product` | Sprint 候选分析（只读，milestone 提议） |
| `subagent-compile` | `/subagent-compile` | 委派 subagent 时的编译权限分级——subagent 允许 `cargo check`，禁止重型编译（test/clippy/build） |

所有 skill 位于 `~/.config/opencode/skills/<name>/SKILL.md`（全局技能组，可被
OpenCode 自动发现）；项目本地技能位于 `.dsh/skills/`。无需注册。

**强制加载（MANDATORY）**：上表所有全局 skills 在其对应场景触发时**必须加载**
（`/skwy-workflow` 等斜杠命令或 `skill` 工具），无例外——grill-me 每次用户
消息强制加载（见上），skwy-workflow 对 feature/bugfix 强制加载，其余 skills
按其描述的使用场景强制加载。加载后立即按其指引执行，一步不跳。

### ui-designer Agent（界面设计）

`~/.config/opencode/agent/ui-designer.md` 定义了界面设计 agent **`ui-designer`**
（只读，全局技能组，OpenCode 自动发现），负责 GUI 布局、视觉风格与交互效果
设计，输出设计方案到 `.dsh/designs/<feature>.md`。

**路由规则（强制）**：任何涉及界面设计的工作 —— 布局、视觉风格、交互效果、
动画、hover/快捷键/反馈状态 —— 主 agent 必须先委派 `ui-designer` 产出
设计方案，再由实现 agent 按方案落地。`ui-designer` 不写源码，只输出方案。
该环节即 skwy-workflow 预实现门禁的 **第 1 步 DESIGN**：方案产出后须向
用户展示要点并获确认，方可进入后续步骤。纯逻辑/数据变更可跳过此步。

**设计方案留档**：`.dsh/designs/` 下的设计方案文件必须随实现一并提交（
`.gitignore` 已放行该目录），作为**过程归档**。

**最终版沉淀 .dsh/kb/（强制）**：设计经用户确认后，最终设计要点必须同步到
`.dsh/kb/design/ui.md` —— 这是 UI 设计的**权威文档**，与代码同步维护。
`.dsh/designs/` 仅归档原始方案；一切 UI 设计决策以 `.dsh/kb/design/ui.md` 为准。

**agent 模型配置（ref #200）**：`.opencode/agent/*.md` 的 frontmatter 不写
`model:` 字段——模型属于运行时配置，写死在 agent 职责定义中会在全局 provider
迁移后留下 stale 引用（ui-designer 曾硬编码 `deepseek/deepseek-v4-flash`，
而该 provider 无此模型）。agent 默认继承全局 `model`；需要给某 agent 指定
非默认模型时，在 `opencode.json` 的 `agent` 段集中配置。

### 测试 Agent（独立 QA，双 agent 并存）

`~/.config/opencode/agent/skwy-requirement-test.md` 定义了需求验收测试 agent
**`skwy-requirement-test`**（独立 QA 角色）；`~/.config/opencode/agent/skwy-adversarial-test.md`
定义了对抗性测试 agent **`skwy-adversarial-test`**（找茬测试工程师）。

**与 `skwy-requirement-test` skill 的区别（关键）**：skill 注入测试**方法论**（怎么测——
框架/内存数据库/tempdir/覆盖率门槛，由项目在自身 .dsh/kb/dev/testing.md 定义）；agent 提供
**认知独立**（谁来独立判断测什么）。skill 由主 agent 加载执行，主 agent 的
上下文盲区 skill 同样看不到；agent 以独立上下文读代码、独立写测试、独立跑
验证，能发现主 agent 注意不到的测试缺口。二者**并存**：委派 `skwy-requirement-test`
agent 时以 `load_skills=["skwy-requirement-test"]` 注入方法论。

**两个 agent 分工**：
- `skwy-requirement-test`：**面向需求的验收测试**——验证 plan/issue 声明的功能契约
  （happy path + 基本错误路径）
- `skwy-adversarial-test`：**对抗性测试**——攻击极端场景（边界/错误路径/非法输入/
  并发/性能退化/资源耗尽），真实有效但刁钻，实现正确修复后必须能通过

**路由规则（强制）**：
- 门禁第 3.5 步 ADVERSARIAL TESTS（RED）：plan 批准后委派 `skwy-adversarial-test`
  写对抗性测试（plan 无接口契约时返回 DEFERRED，首个可编译接口 commit 后携带
  SHA 重新委派）
- 门禁第 4 步 TESTS（RED）：委派 `skwy-requirement-test` 独立写需求验收失败测试
  （而非主 agent 自己加载 skill 写）
- 实现后独立验证：主 agent 完成 GREEN 后，委派 `skwy-requirement-test` 做独立
  QA 复核——验证者与实现者分离（独立验证原则）
- `/skwy-requirement-test`、`/skwy-adversarial-test` 手动触发同样走对应 agent

**职责边界**：独立读生产代码/写测试（RED）/跑验证/判定覆盖缺口/审查主 agent 已写
测试。**不写生产实现代码**。Rust 单测 `#[cfg(test)]` 内嵌源文件，路径权限无法细分
——用三层兜底保证独立验证：① 指令约束只改 `mod tests` 内；② bash 禁 `cargo run`/
git 写操作；③ 无提交权，改动经主 agent 审查后提交。

### Epic & Sub-Issue Workflow

跨多模块的大型需求分解为 **epic**（父 issue）+ **sub-issues**（子 issue）
（GitHub 原生 sub-issue）。关键规则：一个 epic = 一个 PR（每个 sub-issue 一个
commit，`ref #<sub-N>`）、一个 worktree、按依赖 DAG 分批处理（手动切换批次）、
合并后批量关闭。计划文件（`.dsh/plans/<epic>.md`）跟踪状态。

完整子 issue 生命周期见 `~/.config/opencode/skills/skwy-github-workflow/SKILL.md`。

### Issue-Driven Commits

**每个 commit 必须引用 GitHub issue。** 无例外 —— 包括 chores、docs、scripts。
pre-push hook 拒绝没有 `ref #N` 的 commit。

**`ref #N` 必须指向 OPEN issue，且必须独立成行。** commit-msg / pre-push hook
只把**独立成行的 `ref #N`**（该行除 ref 引用外无其他内容）当作 issue 引用提取
并校验状态，指向已关闭/合并 issue 直接拒绝。一行可逗号分隔引用多个 issue
（如 `ref #26, #27`，全部校验）。**行内 `ref #N`（如 "ref #154
教训：…"）视为叙述性提及，不参与校验**——叙述性提及已关闭/合并 issue（如讲解
历史背景）可以直接写 `ref #N` 或 `#N`（ref #211 修复了叙述性提及被误伤的摩擦）。

epic 工作的每个 commit 引用其子 issue（`ref #<sub-N>`）。

```
feat: add thing

ref #26
```

### Commit → Review (MANDATORY)

每次 commit 后必须 review。无例外。

1. **Commit**: stage 变更、写含 `ref #N` 的描述性消息、commit。
2. **Review**: 对已提交变更运行 `/review-work`（5 个并行 agent：goal、quality、security、QA、context）。
3. **Fix**: review 发现问题就修复并重新 commit（最多 2 轮）。

Docs、lint 修复、typo、trivial chores 可跳过。

### Commit & Push

Commit 和 push 是**两个独立操作**。不要用 `&&` 串联。

**Commit**: 直接执行，不需要向用户申请确认。提交是 agent 的职责，按流程 commit 后自动 review。

**HARD BLOCK: Never auto-push.** 等用户明确说 "push" / "推送" 才 push。
**Follow the user's exact words.** "commit" 只表示 commit；"push" 只表示 push。

**Push 前必须 rebase base 分支**：push 前先 `git fetch origin <base>`，若分支落后 base（`git log HEAD..origin/<base>` 非空），先 `git rebase origin/<base>` 解决冲突后再 push。禁止携带过期 base 的提交直接 push——rebase 冲突在 push 后更难收拾（force-push 需小心、远端已带缺陷 commit）。

**Push 前必须提交反思**：用户确认 push 后，先调用 `/skwy-reflect` 写反思并提交（ref #119），再执行 push——反思 commit 与实现同批推送，随 PR 合并落在 master（见「实现后：Reflection Record」）。

完整 push gate 清单见 `.dsh/kb/dev/process.md`。

### Issue Lifecycle

**HARD BLOCK: 只在 push 后关闭 issue。** issue 只有在修复到达
`origin/master` 后才算 "done"。commit 后不要关闭 —— 等 push 成功。

**PR 内的 bug 不建独立 issue。** PR 未合并前，属于该 PR 内容范围的问题
（实现缺陷、冒烟测试发现的问题）直接在 PR 内修复，commit 引用 PR 对应的
epic/issue（`ref #<N>`），不创建新 issue；issue 收尾时在完成 comment 中
一并记录。仅当问题独立于 PR 范围、或 PR 已合并后才走正常 issue 流程。

**push 成功后的强制收尾（勿忘，勿等用户提醒）**：追加完成 comment
（`gh issue comment <N>`——实现摘要 + 验收状态 + commit 列表 + 方案偏差及原因，
遵守 comments.md"永远追加"规范），然后关闭 issue。push 成功 ≠ 任务完成
——issue 收尾是流程的一部分（ref #117 曾因 agent 遗漏被用户提醒）。

**收尾前必须核实实现存在（禁止过度声称）**：完成 comment 中每一项"已实现/已
落地"的功能，都必须先用代码证据核实（grep 实现、测试断言、运行验证），不能凭
记忆或 issue 关联性声称。关联 issue（如从 epic 砍出的独立 issue）若未实现，保持
OPEN 并注明依赖就绪，绝不随 epic 一并关闭（ref #119 曾过度声称 #121/#122
已落地，实际未实现，已发布更正）。

**Plan/批次完成声明同理（ref #174 教训）**：宣布"plan 执行完毕"前必须逐条核对
plan 的 Final verification wave（F1 合规审计 / F2 审查 / F3 测试+覆盖率 / F4 scope
fidelity）并回写台账——evidence 落盘（`.dsh/evidence/`）、台账勾选、epic 两层
审查（子 issue 级 + PR 级完整 diff）是完成定义的一部分，不是可选项。"实现 commit
全部提交" ≠ "plan 完成"；未核即声明即过度声称。

**F1 evidence 与 HEAD 一致性自检（ref #181 教训）**：F-wave evidence 必须在
**全部实现 commit 完成后一次性写**——中途写必然过期（ref #181 曾在 9 commits
时写 F1 声称全部 ref，实际完成 14 commits，构成"过期声称"）。写 evidence 时
自检三件事：

1. **时机**：evidence 写于实现收尾后（全部实现 commit 已提交），不随实现
   中途落盘
2. **commit 计数可复核**：evidence 声称的 commit 数与实际一致
   （`git log <base>..HEAD --format=%B` 逐条核对 ref 引用计数）
3. **中途写必须补正**：如不得已中途写了 evidence（如分批次记录），收尾时
   必须补正为最终状态（commit 数/日期与 HEAD 一致），并注明补正记录

**Epic close**: PR 合并到 master 后，先关闭所有 sub-issues，再关闭 epic。
在 epic 上记录总结 comment 列出所有完成的 sub-issues。

完整 issue lifecycle 见 `.dsh/kb/dev/process.md`，Bevy 风格 A-/C-/D-/P-/S- 标签
分类见 `.dsh/kb/github/labels.md`。最低要求：一个 A- 和一个 C- 标签。

### Scope Discipline

**绝不静默改变已计划的方案。** 如果外部约束（库 bug、API 不兼容、缺 crate）
阻塞了已确认的实现方案，不要通过改变 feature 设计来绕过。
向用户提出该问题并请求决策。

grill-me 决策和已批准的 plan 构成契约。任何偏离 —— 即使是务实的 workaround ——
都需要用户先批准。

---

## Sprint 规划

使用 GitHub Milestones 进行每周 sprint 管理（周一规划 / 周日回顾，周末为核心开发窗口）。
`product` skill 每周一扫描代码库和 open issues，提出 3-5 个候选需求；`/product brainstorm`
可随时手动触发。Sprint 节奏由 `skwy-workflow` skill 的 Sprint Rhythm 规则强制执行。

## 摩擦记录（并入反思）

任何「AI 行为偏差被用户纠正」的场合（grill-me 分歧、执行方向偏离、意图误解、约束遗漏等），
在写事后反思时记录到 `reflections.md` 条目的 **User corrections** 小节。
**/skwy-reflect 必须读取本 session 对话记录（`session_read`）逐条提取用户纠正，
逐字引用原话——不凭记忆，对话记录是客观存在的**（执行者会忘，对话不会忘）；
同时用 git 命令客观验证流程（commit 分支归属、worktree 是否创建未用）。
`friction.md` 机制已移除（2026-08-01）——历史摩擦条目见 `.dsh/kb/dev/reflections-archive.md` 归档文件。已融入流程的反思条目归档至 `.dsh/kb/dev/reflections-archive.md`（主文件 `.dsh/kb/dev/reflections.md` 仅保留活性条目）。**反思文件超过 500 行自动归档一次（skwy-reflect 第 5 步）**：值得处理的条目建 issue 后归档、已处理的直接归档、剩余的保留待下次归档时重新检阅；归档后仍超 500 行则交用户判断（ref #238）。

## 决策记录

所有 `.dsh/kb/design/` 下的设计文档 MUST 包含 `## 决策记录` 章节，自包含地记录
关键设计决策的 **what + why + why-not**。

- **格式**: 表格 `| 决策 | 选项 | 选择 | 理由 | 排除原因 |`
- **保障**: `skwy-workflow` PRE-IMPLEMENTATION GATE Step 5c 检查是否存在
- **自包含**: 决策记录不依赖外部引用（如 friction.md），所有理由直接写在设计文档内

---

## Worktrees

PR 开发使用 git worktrees，位于 `.worktrees/<name>/`（gitignored），每个 worktree 对应
一个 PR/epic，合并后清理。**创建时机（强制）**：需求经 grill-me 确认是需要 worktree 的
工作（feature/epic、2+ 模块、将产出 `.dsh/plans/*.md` 或 `.dsh/designs/*.md`）时，
**grill 共识达成后立即创建并切换**——后续的 design/issue/plan/review/实现全部在
worktree 内进行，**plan/design 等 .omo 产出文件直接在 worktree 内创建**，随实现 PR
一并提交。**禁止**在 master 工作区先产出 plan/design 再等开 worktree 迁移——git
worktree 是独立 checkout，master 工作区的 untracked 文件不会出现在 worktree 中
（ref #138 教训：SEPA 曾全程在 master 规划、plan/design 成 untracked，最后需手动迁移）。

**主 session 的职责仅为确定用途 + 命名**：将用途简述、
对应 issue URL 与已锁定决策写入 `.worktrees/<name>/.dsh/handoff.md`，然后运行
`~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh <name>` 自动启动工作树区域（探测默认终端 + setsid
脱离进程组，无需手动解绑当前 session）。剩余工作（设计/计划/实现/commit/PR）
全部由 worktree 内的 agent 自主完成——worktree 会话启动后**第一步读取
`.dsh/handoff.md`** 获取上下文契约。worktree 创建后其原始分支（master）可能继续
推进，**worktree 会话启动后先同步原始分支**（`git fetch origin master && git rebase origin/master`，
冲突解决后再开始），避免基于过期基点开发。opencode 仍占用目录无法删除时用
`~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh --close <name>` 终止并清理
（从 worktree 内执行时自动转为 detached 清理，含关闭承载终端窗口，见 `.dsh/kb/dev/process.md`）。
**加载 `/skwy-worktree` skill 获取完整流程****（含 post-creation MANDATORY 步骤与清理）。

**强制规则**：worktree 一旦创建，后续实现工作必须在 worktree 内完成交接闭环
（add → 写 handoff → `~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh <name>` 启动），master 上不再继续实现。
master 只允许 docs/lint/typo/反思类提交直推；存在活跃 worktree 时实现类提交
落在 master 即流程违规，在 reflections 中记录。
**用户明确指示直推时的知情同意**：用户指示实现类提交直推 master（如"在master
提交吧"）时，agent 必须先明示该指示与 worktree 规则的冲突并确认用户知情
（"这违反 worktree 规则，按你的指示在 master 提交并在 reflections 记录，确认？"），
获得确认后才执行（ref #211 教训：静默照办 = 用户不知情的流程偏离）。

## Knowledge base

详细文档在 `.dsh/kb/` 下，按四部分组织。**AGENTS.md 是索引，不是重复** — 细节只在 .dsh/kb/ 中，绝不在此复制。

| 文件 | 内容 |
|---|---|
| `.dsh/kb/design/architecture.md` | 系统总览、crate 关系、线程模型、数据管线、存储策略、库选型 |
| `.dsh/kb/design/data-providers.md` | Provider trait 体系、DuckDbProvider/ParquetReader、错误处理、DDL |
| `.dsh/kb/design/backtest.md` | SEPA 历史回测 — 架构、组合模拟/基准代理口径、绩效指标、决策记录 |
| `.dsh/kb/design/symbols.md` | A 股市场分段、符号约定、交换所推断、timeframe 映射 |
| `.dsh/kb/design/ui.md` | UI 设计权威文档 — 设计系统、布局结构、交互规范（最终版；`.dsh/designs/` 仅归档） |
| `.dsh/kb/design/ui-widgets.md` | UI 组件使用规范权威文档 — 24 个组件 × 8 字段模板（用途/适用场景/变体/API/示例/反模式/相关组件/测试锚点）、三层组织、状态所有权、偏差跟踪 |
| `.dsh/kb/design/workflow-skills.md` | skwy- 技能组设计决策（issue #210）— 全局技能迁移范围、门禁 3.5 步、脚本自包含等 |
| `.dsh/kb/dev/testing.md` | rstest + tokio::test 模式、内存 DuckDB、Dolt 测试库、benchmark/Tracy |
| `.dsh/kb/dev/process.md` | 开发流程、命令、配置、调试、重置 |
| `.dsh/kb/dev/database.md` | 数据库开发信息 — Dolt 查询/同步/提交、Parquet/DuckDB 生成、布局 |
| `.dsh/kb/dev/toolchain.md` | 工具链问题排查卡 — 执行中遇到并解决的问题，按症状/根因/排查路径/修复/验证沉淀 |
| `.dsh/kb/dev/reflections.md` | 事后反思（活性条目）— 做了什么、哪里出错、教训（User corrections） |
| `.dsh/kb/dev/reflections-archive.md` | 反思归档 — 教训已融入流程/已被取代的历史条目 + 历史摩擦记录 |
| `.dsh/kb/user/index.md` | 用户总览 — Compass 是什么、快速开始、前置条件 |
| `.dsh/kb/user/gui.md` | 图表应用 — 界面、控件、数据流、股票代码 |
| `.dsh/kb/user/cli.md` | 数据管线 — import/import-compass/export/backup、工作流、排障 |
| `.dsh/kb/user/config.md` | 配置参考 — 全部选项、默认值、示例 |
| `.dsh/kb/github/labels.md` | Issue/PR 标签分类 — Bevy 风格 C/A/D/P/S 前缀 |
| `.dsh/kb/github/comments.md` | 评论规范 — 永远追加，绝不修改 |
| `.dsh/kb/github/ask.md` | GitHub bot 角色 — /ask 只读问答（工作流按路径加载，勿改） |
| `.dsh/kb/github/fix.md` | GitHub bot 角色 — /fix 修 bug（工作流按路径加载，勿改） |
| `.dsh/kb/github/impl.md` | GitHub bot 角色 — /impl 实现功能（工作流按路径加载，勿改） |
| `.dsh/kb/github/pr-review.md` | GitHub bot 角色 — /review 代码审查（工作流按路径加载，勿改） |
| `.dsh/kb/github/ci-fix.md` | GitHub bot 角色 — CI 失败诊断（工作流按路径加载，勿改） |

### 变更类型 → .dsh/kb/ 文件映射表

文档同步（`skwy-workflow` 技能内嵌「文档同步」章节，门禁 5b 步）时，按此表
确定需更新的 .dsh/kb/ 文件——这是本项目的「项目自身定义的映射表」引用点。

| 变更类型 | 主要 .dsh/kb/ 文件 | 次要 .dsh/kb/ 文件 |
|---|---|---|
| 新增数据源、API 调用、schema 变更 | `.dsh/kb/design/data-providers.md` | `.dsh/kb/design/architecture.md`（如涉及管线变更） |
| 线程、管线、库变更 | `.dsh/kb/design/architecture.md` | — |
| 符号格式、timeframe 映射 | `.dsh/kb/design/symbols.md` | — |
| 测试框架、测试模式 | `.dsh/kb/dev/testing.md` | — |
| 工作流、hook、约定 | `.dsh/kb/dev/process.md` | `AGENTS.md`（如项目级别） |
| 新增 CLI 命令或 flag 变更 | `.dsh/kb/user/cli.md` | `.dsh/kb/dev/process.md`（调试章节） |
| GUI 布局、控件变更 | `.dsh/kb/user/gui.md` | `.dsh/kb/design/ui.md`（布局/视觉/交互设计变更）+ `.dsh/kb/design/architecture.md`（如涉及线程变更） |
| 配置项新增/变更 | `.dsh/kb/user/config.md` | — |
| 重大功能（用户侧） | `.dsh/kb/user/index.md` | 相关 design + GUI/CLI 文件 |
| 项目级别约定 | `AGENTS.md` | `.dsh/kb/dev/process.md` |
| OpenCode skill 或 agent 变更 | `AGENTS.md` | `.dsh/kb/dev/process.md`（OpenCode 工作流章节） |
| 标签约定 | `.dsh/kb/github/labels.md` | — |
| 评论约定 | `.dsh/kb/github/comments.md` | — |

## Setup

- **Rust edition 2024** — 需要 Rust ≥1.85。当前工具链：1.97.1。
- **mold 链接器** — Linux 构建使用 mold（`.cargo/config.toml`，`-fuse-ld=/usr/bin/mold`）。Ubuntu: `sudo apt install mold clang`。缺失时编译失败。
- **GUI app** — 需要显示服务器（X11/Wayland）。`scripts/run.sh` 一键启动（或 `cargo run --bin compass`）。
- 日志写入 `logs/compass.log`（每日轮转）。
- 配置在 `~/.config/compass/config.toml`（缺省回退默认值）。见 `.dsh/kb/user/config.md`。

## Commands

```sh
cargo build
scripts/run.sh                # 一键启动 GUI 图表窗口（前台，Ctrl+C 退出）
cargo run --bin compass       # 等价手动方式（需 X11/Wayland）
cargo run --bin compass-data -- <subcommand>  # 数据管线 CLI
cargo test                   # 单元 + 集成测试
cargo fmt
cargo clippy
RUST_LOG=debug scripts/run.sh # 详细日志
```

### compass-data CLI 速查

```sh
cargo run --bin compass-data -- import                    # Dolt investment_data → Parquet（全量直写，推荐）
cargo run --bin compass-data -- import --since 20260725   # ⚠️ 日期过滤直写：仅导出 since 后数据并覆盖全文件，非追加（慎用，见 .dsh/kb/dev/toolchain.md）
cargo run --bin compass-data -- import-compass --table stock_basic  # Dolt compass_data → Parquet（--since 有 merge）
cargo run --bin compass-data -- export                    # Parquet → DuckDB
cargo run --bin compass-data -- backup                    # Parquet → 百度云
```

`import-compass`/`export` 默认 merge/skip，`--overwrite` 覆盖；`import` 总是全量直写（`--since` 仅过滤并覆盖，不是增量追加）。
完整选项见 `.dsh/kb/user/cli.md`。

## Architecture & Data providers

- **架构**: `.dsh/kb/design/architecture.md` — 线程模型、数据管线、schema、源码布局、库选型
- **数据提供者**: `.dsh/kb/design/data-providers.md` — DuckDB、Dolt、ParquetReader、DataError
- **符号约定**: `.dsh/kb/design/symbols.md` — 市场分段、交换所推断、timeframe 映射

**Priority**: Dolt `investment_data` (local) 是主数据源。GUI 数据访问全部本地 — 无在线回退。

### compass_data Dolt 仓库 — 每次数据变更后 commit & push（所有路径）

`/data/compass-data/compass_data` 是 Dolt 仓库（remote:
`doltremoteapi.dolthub.com/skwy/compass_data`）。**任何路径修改该库**
（import、re-import、schema 变更、data_updates 更新、SEPA 采集、CLI/程序
写回如 `sepa backtest` 的 `backtest_result`）都必须**及时**提交并推送——写库
操作完成后立即收尾，禁止让数据滞留工作区留待"以后再说"（ref #190 教训：
`sepa backtest` 写回后未 commit，backtest_result 384 行滞留工作区一天）：

```sh
cd /data/compass-data/compass_data
dolt status                            # 确认变更范围
dolt add <table>...                    # or `dolt add .`
dolt commit -m "feat: ..."             # describe the data change
dolt push origin main
dolt status                            # 确认工作区干净、与 origin 同步
```

**程序写回路径同样受约束**：任何 Rust/Python 代码向 `compass_data` 写表
后，流程必须在同一 session 内执行 `dolt commit` + `dolt push`（手动命令
或内置到 CLI 的收尾步骤），不得只写数据不提交。`dolt status` 非干净
（working tree 有变更）即视为流程违规，在 reflections 中记录。

完整 Dolt 操作指南（含跨库查询示例、investment_data 同步流程）见 `.dsh/kb/dev/database.md`。

## Parquet schema & Config

- **Parquet 主数据库结构** 与 **DuckDB DDL**: `.dsh/kb/design/data-providers.md`（Schema 章节）
- **配置参考**: `.dsh/kb/user/config.md`（全部选项 + 默认值 + 示例）

## Testing

见 `.dsh/kb/dev/testing.md` — rstest + tokio::test 模式、内存 DuckDB、Dolt 测试库、benchmark、Tracy 分析。

**覆盖率门槛（CI 强制，低于阈值 CI 失败）**：Rust workspace 总 **93%**，per-crate 阈值按可测试性设定——纯逻辑/serde 可测的 compass-core / compass-data / compass-i18n / compass-strategy / compass-types / compass-ui **95%**，GUI 主程序 compass（事件循环/线程/交互难测）**90%**（`cargo llvm-cov --json` + `scripts/check-coverage.sh` 内嵌阈值表校验）；Python collectors `--cov=.` 全量计入 ≥95%（`--cov-fail-under=95`）。GUI 用 egui_kittest 无头集成测试，Python 用 stub AsyncSession 模拟网络。详见 `.dsh/kb/dev/testing.md` 覆盖率章节。

## API reference

类型级 API 参考见 `cargo doc --open`（`#![warn(missing_docs)]` 强制所有 pub 项带 `///` 注释）。
egui-charts 用法示例见 `.dsh/kb/user/gui.md` 与 `cargo doc`。
