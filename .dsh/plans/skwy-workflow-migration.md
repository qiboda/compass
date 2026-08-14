# skwy-workflow-migration - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->

**What you'll get:** 一套可跨项目复用的全局 opencode 技能组（skwy-workflow 系列 7 个技能 + 2 个 agent 定义），其中新增一个"找茬测试工程师"（skwy-adversarial-test）在计划批准后写刁钻但有效的对抗性测试；compass 本地清掉已迁走的技能目录并同步所有引用。

**Why this approach:** 技能去全局化（单一来源、多项目共用），compass 特有细节改为"项目在自身 AGENTS.md 定义"的引用点；找茬工程师与现有需求测试工程师并存分工，门禁新增第 3.5 步在实现前拦截缺陷。

**What it will NOT do:** 不迁移 product 技能与 ui-designer agent；不创建 rustdoc 全局技能（编译期强制已覆盖）；不改任何产品代码、数据管线、历史反思文档。

**Effort:** Large
**Risk:** Medium - 全局文件在 git 仓库外（无法靠 PR review 覆盖）+ 门禁 3.5 步是永久工作流变更，需要验证脚本兜底
**Decisions to sanity-check:** skwy-adversarial-test 的攻击边界（真实有效但极端）；现有 test agent 改名 skwy-requirement-test；脚本单一来源（compass 本地删除）

Your next move: 批准后由 worker 会话执行（$start-work）。

---

> TL;DR (machine): Large effort, Medium risk — 7 global skwy skills + 2 agents + gate 3.5 adversarial testing step + compass local cleanup + reference sync + executable verification.

## Scope
### Must have
- 7 个全局技能目录 `~/.config/opencode/skills/skwy-*/`：skwy-workflow（含门禁 3.5 步 + docs 整合 + kb 结构规范）、skwy-github-workflow（issue-workflow + comments.md）、skwy-git-workflow、skwy-requirement-test（qa 泛化改名）、skwy-adversarial-test（新）、skwy-reflect、skwy-worktree（含 scripts/open-worktrees.sh + tests）
- 2 个全局 agent 定义 `~/.config/opencode/agent/`：skwy-requirement-test.md、skwy-adversarial-test.md
- compass 本地删除：7 个技能目录（compass-workflow/docs/issue-workflow/qa/reflect/rustdoc/worktree）、.opencode/agent/test.md、scripts/open-worktrees.sh
- 引用同步：AGENTS.md、kb/dev/process.md、kb/design/ui.md、kb/github/{impl,fix}.md、.opencode/skills/product/SKILL.md、.opencode/agent/test.md（迁全局）
- 验证：技能/agent 可加载（可执行命令）、无残留旧名 grep（定义模式+排除）、open-worktrees 行为测试、新项目仅依赖全局技能组冒烟
- issue #210 body 同步（6→7 技能、skwy-test→skwy-requirement-test、+2 agents、+gate 3.5）
- kb/design 决策记录（handoff item 5）

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不迁移 product 技能、ui-designer agent；不创建 rustdoc 全局技能
- 不改 kb/dev/reflections*.md 历史内容、不改 kb/dev/testing.md 内容、不改 .githooks/ 逻辑
- 不修改任何 Rust/Python 产品代码、不改 Dolt/Parquet 数据管线
- 不迁移 /ulw-plan、/review-work、/grill-me（非本 epic 对象）
- skwy-adversarial-test 不写无解/无效测试（真实有效但场景极端）
- 不改 process.md L274（cargo-doc 约定行，非 rustdoc skill 引用——Metis 确认误读）

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after（迁移/文档类变更，验证以检查脚本为主）+ open-worktrees 行为测试（bash 测试脚本，已有）
- Evidence: .omo/evidence/skwy-workflow-migration/task-<N>.txt（每个 todo 的 QA 输出落盘）
- 关键可执行命令：
  - 技能可加载：`ls ~/.config/opencode/skills/skwy-*/SKILL.md` + `ls ~/.config/opencode/agent/skwy-*.md`（文件存在断言）
  - 无残留旧名（**锚定 slash-command 边界**，避免命中"分支/worktree""kb/dev/testing.md"等合法词）：`rg -n --hidden -g '!.git' -g '!target' -g '!.omo' -g '!node_modules' -g '!logs' -g '!.worktrees' -g '!reflections*.md' -e 'compass-workflow' -e 'issue-workflow' -e 'skwy-test' -e '/\b(reflect|test|worktree|docs|rustdoc)\b' -e '\.opencode/skills/(compass-workflow|docs|issue-workflow|qa|reflect|rustdoc|worktree)' AGENTS.md kb .opencode/agent .opencode/skills/product .opencode/plugins 2>/dev/null | rg -v '分支/worktree|chore/docs/test|test-sepa-daily'`（**scope 只含引用承载文件**——AGENTS.md/kb/.opencode/agent/.opencode/skills/product/.opencode/plugins；**排除 scripts/ 与 .githooks/**——这两个目录迁移后不可能含技能引用，且其中 `chore/docs/test`（pre-push 提示文本）、`test-sepa-daily.sh`（SEPA 测试脚本名）为合法内容，属于已验证误报源；`| rg -v` 追加白名单双保险（AGENTS.md L79"分支/worktree"属 SELF-CHECK 通用措辞）；`/\b...\b` 要求斜杠+词边界，不会命中 reflections.md/testing.md 文件名；`-g '!reflections*.md'` 排除历史反思文档；`-g '!.worktrees'` 排除嵌套 worktree）
  - open-worktrees 行为测试：`bash ~/.config/opencode/skills/skwy-worktree/scripts/tests/open-worktrees-test.sh`（在仓库内运行，fixture 指向临时目录）
  - 新项目冒烟：临时目录 + 最小 AGENTS.md，`opencode run` 触发各 skwy-* skill 加载

## Execution strategy
### Parallel execution waves
- **Wave 1**（决策记录与 issue 同步，2 todos，顺序）：todo 1-2
- **Wave 2**（5 个全局技能，并行）：todo 3-7
- **Wave 3**（2 技能 + 2 agents，并行）：todo 8-11
- **Wave 4**（本地清理 + AGENTS.md，顺序）：todo 12-13
- **Wave 5**（kb 引用同步，并行）：todo 14
- **Wave 6**（验证，顺序）：todo 15

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 (issue/handoff 同步) | — | 2 | — |
| 2 (kb/design 决策记录) | 1 | — | — |
| 3-7 (全局技能) | 1 | 10-11, 12-14 | 3-7 互相 |
| 8-9 (reflect/worktree) | 1 | 12-14 | 3-7, 10-11 |
| 10-11 (全局 agents) | 3, 6, 7 | 12-14 | 8-9 |
| 12 (本地删除) | 3-11 | 13 | — |
| 13 (AGENTS.md) | 12 | 14 | — |
| 14 (kb 引用同步) | 13 | 15 | — |
| 15 (验证) | 12-14 | F1-F4 | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

- [ ] 1. 同步 issue #210 body 与 .omo/handoff.md 至新决策（7 技能 + 2 agents + gate 3.5 + skwy-requirement-test 改名）
  What to do / Must NOT do: 用 `gh issue edit 210 --body-file` 更新 issue #210 body，**新 body 全文逐字如下**（验收标准含 agent 加载与 gate 3.5）：
  ```
  ## 背景
  compass 项目积累了 8 个 opencode 技能，其中大部分是通用开发流程，深度绑定 compass 项目（cargo/Dolt/A股/kb/ 具体文件）。目标：将通用技能抽取为全局技能组 skwy-workflow，使多个项目共用；compass 特有内容通过「项目自身 AGENTS.md/kb/ 定义」引用点保留。
  ## 决策（grill-me 已锁定）
  - 技能组形态：多个独立技能统一 `skwy-` 前缀，放 `~/.config/opencode/skills/`
  - 全局技能 7 个：`skwy-workflow` / `skwy-github-workflow` / `skwy-git-workflow` / `skwy-requirement-test` / `skwy-adversarial-test` / `skwy-reflect` / `skwy-worktree`
  - `skwy-workflow` = compass-workflow 泛化 + kb/ 结构规范 + docs 流程整合（docs 不单独成技能）+ **门禁新增第 3.5 步 Adversarial Tests**
  - `skwy-github-workflow` = issue-workflow + comments.md 合并
  - `skwy-git-workflow` = AGENTS.md Commit & Push 章节 + process.md git 规范
  - `skwy-requirement-test` = qa(test) 泛化改名：面向需求的验收测试
  - `skwy-adversarial-test` = 新增对抗性测试工程师：plan 批准后（门禁 3.5 步）写真实有效但场景刁钻的对抗性测试（RED→实现后必须 GREEN），与 skwy-requirement-test 并存分工
  - `skwy-reflect` = reflect 泛化；`skwy-worktree` = worktree + scripts/open-worktrees.sh（脚本随技能目录自包含）
  - 全局 agent 定义 2 个：`~/.config/opencode/agent/skwy-requirement-test.md`、`skwy-adversarial-test.md`
  - rustdoc 技能废弃（`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` 编译期强制已覆盖）；product 留在 compass 项目本地
  - 泛化原则：技能正文用 AGENTS.md/kb/.omo 通用约定，compass 特有细节改为「项目在自身 AGENTS.md 定义」的引用点
  ## compass 本地变更
  1. 删除 `.opencode/skills/` 下 7 个已迁走技能目录：compass-workflow、docs、issue-workflow、qa、reflect、rustdoc、worktree（保留 product）；删除 `.opencode/agent/test.md`（迁全局改名 skwy-requirement-test）；删除 `scripts/open-worktrees.sh` 与 `scripts/tests/open-worktrees-test.sh`（脚本随 skwy-worktree 技能自包含）
  2. AGENTS.md 引用更新：compass-workflow→skwy-workflow 等，斜杠命令 /reflect→/skwy-reflect 等，Skills 表新增 /skwy-adversarial-test
  3. kb/dev/process.md 等引用同步更新
  4. 门禁 5a 步 RUSTDOC 移除；5b 步 DOCS 改为 skwy-workflow 内嵌文档同步章节
  ## 验收标准
  - [ ] `~/.config/opencode/skills/` 下 7 个 skwy- 技能 + `~/.config/opencode/agent/` 下 2 个 agent 可被 opencode 发现加载
  - [ ] compass 本地已迁走 7 技能目录 + test.md + 脚本已删除，product/ui-designer 保留
  - [ ] AGENTS.md/kb 无残留旧技能名引用（grep 验证）
  - [ ] 全局技能文件随 skill 自包含（worktree 脚本在 skwy-worktree/scripts/ 下，绝对路径引用）
  - [ ] 门禁 3.5 步 Adversarial Tests 写入 skwy-workflow 技能（plan 批准后委派 skwy-adversarial-test 写对抗性测试，实现必须让 3.5+4 全绿）
  - [ ] 新项目可仅依赖全局技能组运行工作流（临时项目冒烟验证）
  标签：C-Feature, A-Docs
  ```
  更新 .omo/handoff.md：①「已锁定的 grill-me 决策」追加 9-13 条（D9 找茬工程师并存分工/纳入全局/直接写测试/内嵌门禁 3.5/独立 skwy-adversarial-test/真实有效极端边界/两批测试共存；D2 现有 test 改名 skwy-requirement-test）；② **同步更新 L15-21 全局技能名单（6→7，skwy-test→skwy-requirement-test + 新增 skwy-adversarial-test）与 L45-51 全局技能文件创建清单（7 目录）**——不留 skwy-test 残留；③ 技能组总览更新为 7 技能 + 2 agents。MUST NOT: 关闭 issue；不改其他文件。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2
  References: .omo/drafts/skwy-workflow-migration.md（D2/D9 决策）; issue #210 body（GitHub）; .omo/handoff.md L12-27（原决策 1-8）
  Acceptance criteria (agent-executable): `gh issue view 210 --json body` 输出含 "skwy-adversarial-test" 与 "skwy-requirement-test" 且不含 "skwy-test"；`gh issue view 210 --json body` 输出含 "7 个 skwy- 技能" 与 "第 3.5 步"；`grep -c 'skwy-adversarial-test' .omo/handoff.md` ≥ 3 且 `grep -c 'skwy-test' .omo/handoff.md` = 0
  QA scenarios: happy - 上述断言全命中；failure - 若 gh 命令失败（401/网络），检查 `~/.config/opencode/github-token` 并重试；若 handoff 残留 skwy-test，删除旧名单，Evidence .omo/evidence/skwy-workflow-migration/task-1.txt
  Commit: Y | docs(workflow): sync issue #210 and handoff to 7-skill plan with adversarial test engineer

- [ ] 2. kb/design 决策记录：写入技能组迁移设计决策（handoff item 5 / 门禁 5c）
  What to do / Must NOT do: **新建** `kb/design/workflow-skills.md`（若文件已存在则追加），内容追加 `## 决策记录` 表格，记录：技能去全局化（skwy- 前缀 7 技能 + 2 agents）、门禁 3.5 步对抗性测试、rustdoc 技能废弃、product 留本地、脚本单一来源。决策记录须自包含（what + why + why-not）。**不要**追加到 architecture.md（统一目标文件为 workflow-skills.md，避免验收路径歧义）。MUST NOT: 改动已有设计文档正文（仅新建/追加本文件）。
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: —
  References: AGENTS.md「决策记录」章节（所有 kb/design/ 文件 MUST 含 ## 决策记录）; .omo/drafts/skwy-workflow-migration.md D1-D11
  Acceptance criteria (agent-executable): `grep -c '## 决策记录' kb/design/workflow-skills.md` ≥ 1 且表格含 "skwy-adversarial-test" 行
  QA scenarios: happy - grep 命中；failure - 若追加到已有文件导致结构冲突，改用新建文件，Evidence .omo/evidence/skwy-workflow-migration/task-2.txt
  Commit: Y | docs(design): record skwy skill group migration decisions

- [ ] 3. 创建全局技能 skwy-workflow（门禁含 3.5 步 + docs 文档同步章节整合 + kb 结构规范）
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-workflow/SKILL.md`，泛化自 .opencode/skills/compass-workflow/SKILL.md（357 行）：保留门禁（含 PRE-IMPLEMENTATION GATE 表格，第 4 步 Tests 改为委派 skwy-requirement-test，新增**第 3.5 步 Adversarial Tests 委派 skwy-adversarial-test 写对抗性测试**）、12 条规则、实现后审查、反思记录；5a RUSTDOC 步骤移除（编译期强制已覆盖）；5b DOCS 改为内嵌「文档同步」章节（变更→kb 映射表改为「项目在自身 AGENTS.md/kb/ 定义」）；skill 名称/slash 命令全部改 skwy- 前缀（/skwy-github-workflow、/skwy-requirement-test、/skwy-adversarial-test、/skwy-reflect、/skwy-worktree）；ref # 历史引用改为泛化描述（不写 compass 具体 issue 号）；cargo 命令保留为通用示例（`cargo test && cargo clippy -- -D warnings && cargo fmt --check` 可作为 Rust 项目示例）；`.opencode/skills/<name>/SKILL.md` 路径改为 `~/.config/opencode/skills/<name>/SKILL.md`。MUST NOT: 写死 kb/dev/toolchain.md 等 compass 具体文件名（改为项目自引用点）；不包含 A股/Dolt 内容；不改动门禁 1-3 步结构。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 10, 12-14
  References: .opencode/skills/compass-workflow/SKILL.md（来源，357 行）; .opencode/skills/docs/SKILL.md（文档同步章节整合来源）; .omo/drafts/skwy-workflow-migration.md D6; AGENTS.md 门禁表（L64-70）
  Acceptance criteria (agent-executable): `grep -c '第 3.5 步' ~/.config/opencode/skills/skwy-workflow/SKILL.md` ≥ 1；`grep -c 'skwy-' ~/.config/opencode/skills/skwy-workflow/SKILL.md` ≥ 8；`grep -c 'compass-workflow\|/rustdoc\|kb/dev/toolchain' ~/.config/opencode/skills/skwy-workflow/SKILL.md` = 0
  QA scenarios: happy - 上述 grep 全命中；failure - 若残留 compass 文件名，修正为项目自引用点措辞，Evidence .omo/evidence/skwy-workflow-migration/task-3.txt
  Commit: N（文件在 ~/.config 仓库外，无 commit——创建状态写入 .omo/evidence/skwy-workflow-migration/task-3.txt 台账，随仓库 commit 引用说明；sha256sum 记录于 todo 15 验证）

- [ ] 4. 创建全局技能 skwy-github-workflow（issue-workflow + comments.md 合并，repo URL 参数化）
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-github-workflow/SKILL.md`，泛化自 .opencode/skills/issue-workflow/SKILL.md（287 行）+ 合并 kb/github/comments.md 的「永远追加」规范为独立章节：硬编码 `https://github.com/qiboda/compass/issues/<N>`（L166/L173）改为 `<owner>/<repo>` 占位符 + 「项目在自身 AGENTS.md 定义 repo 身份」；`master` 合并策略改为「项目默认分支」；A-/C- 标签约定保留为通用 GitHub 标签规范（注明标签分类由项目 AGENTS.md/kb 定义）；compass-workflow 引用（15+ 处）改 skwy-workflow；.omo/plans/ 路径保留（通用约定）。MUST NOT: 保留 qiboda/compass 硬编码；不引入 Dolt/cargo。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 10, 12-14
  References: .opencode/skills/issue-workflow/SKILL.md（来源）; kb/github/comments.md（合并来源）; .omo/drafts/skwy-workflow-migration.md D4
  Acceptance criteria (agent-executable): `grep -c 'qiboda\|compass' ~/.config/opencode/skills/skwy-github-workflow/SKILL.md` = 0；`grep -c '<owner>/<repo>' ~/.config/opencode/skills/skwy-github-workflow/SKILL.md` ≥ 1；`grep -c '永远追加' ~/.config/opencode/skills/skwy-github-workflow/SKILL.md` ≥ 1
  QA scenarios: happy - grep 命中；failure - 若有 qiboda 残留，替换为占位符，Evidence .omo/evidence/skwy-workflow-migration/task-4.txt
  Commit: N（~/.config 仓库外，无 commit——台账 + sha256sum 记录，同 todo 3）

- [ ] 5. 创建全局技能 skwy-git-workflow（AGENTS.md Commit & Push 章节 + process.md git 规范）
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-git-workflow/SKILL.md`，内容来自 AGENTS.md「Issue-Driven Commits」「Commit → Review」「Commit & Push」章节 + kb/dev/process.md git 部分：ref #N 约定（指向 OPEN issue）、Never auto-push、commit→review、push 前 rebase base、反思 commit 同批推送。泛化：ref # 具体案例移除，保留规则本身；pre-push hook 检测描述为「项目自身 hook」。MUST NOT: 写死 compass 具体 issue 号/PR 号；不复制 Dolt 相关流程。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 12-14
  References: AGENTS.md L198-250（Issue-Driven Commits / Commit & Push 章节）; kb/dev/process.md git 相关段落; .omo/drafts/skwy-workflow-migration.md D5
  Acceptance criteria (agent-executable): `grep -c 'ref #N' ~/.config/opencode/skills/skwy-git-workflow/SKILL.md` ≥ 1；`grep -c 'Never auto-push\|不要用.*push' ~/.config/opencode/skills/skwy-git-workflow/SKILL.md` ≥ 1；`grep -c 'ref #[0-9]' ~/.config/opencode/skills/skwy-git-workflow/SKILL.md` = 0
  QA scenarios: happy - grep 命中；failure - 若有具体 issue 号残留，移除，Evidence .omo/evidence/skwy-workflow-migration/task-5.txt
  Commit: N（~/.config 仓库外，无 commit——台账 + sha256sum 记录，同 todo 3）

- [ ] 6. 创建全局技能 skwy-requirement-test（qa 泛化改名：面向需求的验收测试）
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-requirement-test/SKILL.md`，泛化自 .opencode/skills/qa/SKILL.md（163 行）：frontmatter `name: skwy-requirement-test`；保留 TDD/BDD 骨架（阶段 0 设计/1 RED/2 GREEN/3 REFACTOR）+ 单元/集成测试组织；DuckDB/Dolt 测试模式（L96/L103）改为「项目在自身 kb/dev/testing.md 定义测试基础设施」引用点；compass 示例移除（DuckDbProvider::new_in_memory、A股 symbol 000001/600519、compass-core、cargo bench 路径）；kb/dev/testing.md 3 次引用（L20/L68/L109）改为项目引用点；禁止事项保留（不改依赖清单/不用 #[allow] 抑制）。定位描述强调「面向需求的验收测试，验证 plan/issue 承诺的功能契约」。MUST NOT: 包含 compass 特有测试示例；不含对抗性测试内容（那是 skwy-adversarial-test 的职责）。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 10, 12-14
  References: .opencode/skills/qa/SKILL.md（来源）; .omo/drafts/skwy-workflow-migration.md D2; AGENTS.md「test Agent（独立 QA）」章节（skill 与 agent 并存说明）
  Acceptance criteria (agent-executable): `grep -c 'name: skwy-requirement-test' ~/.config/opencode/skills/skwy-requirement-test/SKILL.md` ≥ 1；`grep -c '000001\|600519\|DuckDbProvider\|compass-core\|A-share\|egui' ~/.config/opencode/skills/skwy-requirement-test/SKILL.md` = 0；`grep -c 'kb/dev/testing.md' ~/.config/opencode/skills/skwy-requirement-test/SKILL.md` = 0
  QA scenarios: happy - grep 命中；failure - 若 compass 示例残留，移除并改为项目引用点措辞，Evidence .omo/evidence/skwy-workflow-migration/task-6.txt
  Commit: N（~/.config 仓库外，无 commit——台账 + sha256sum 记录，同 todo 3）

- [ ] 7. 创建全局技能 skwy-adversarial-test（找茬测试工程师：对抗性测试）
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-adversarial-test/SKILL.md`（新技能，无本地来源）：frontmatter `name: skwy-adversarial-test`；定位「对抗性测试工程师——想方设法让实现通不过」；触发场景：门禁第 3.5 步（plan 批准后）委派；攻击维度：边界值/错误路径/非法输入/并发竞态/性能退化/资源耗尽，针对 plan 声明的功能承诺；**RED/GREEN 契约（消除"实现前 vs 必须编译"悖论）**：委派时主 agent 传入 plan + issue + 仓库上下文；对抗性测试针对 plan 声明的**接口契约（公共 API 签名/行为承诺）**编写；若 plan 未声明接口细节，**两段式委派协议**：首次委派时 agent 判定 plan 无接口契约 → 返回 `DEFERRED（plan 未声明接口，待首个可编译接口 commit）` 状态并说明所需接口，主 agent 在实现产出第一个可编译接口 commit 后**携带 commit SHA 重新委派**，agent 再编写测试（RED = 断言失败而非编译错误；GREEN = 实现正确修复后断言通过）——技能文档必须写明此 DEFERRED 协议，避免 agent 空转或写无效测试。**硬约束**：测试必须真实有效（能编译、能运行、断言正确，仅针对已存在或 plan 声明的接口），实现正确修复后必须能通过，禁止无解/无效测试（grill 决策 6）；与 skwy-requirement-test 分工：需求验收测试验证功能契约（happy path + 基本错误），对抗性测试攻击极端场景，**不写需求验收职责内的测试**；测试基础设施（DuckDB/Dolt/框架）遵循项目 kb/dev/testing.md；输出格式：测试代码落盘 + RED 证据（失败输出）+ GREEN 验收。MUST NOT: 写无效/恶意测试；不替代 skwy-requirement-test 的职责；不含 compass 特定内容。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 10, 12-14
  References: .omo/drafts/skwy-workflow-migration.md D9（grill 决策 1-6）; AGENTS.md 门禁表（新增 3.5 步位置）; kb/dev/testing.md（测试基础设施引用点，作为项目侧文档）
  Acceptance criteria (agent-executable): `grep -c 'name: skwy-adversarial-test' ~/.config/opencode/skills/skwy-adversarial-test/SKILL.md` ≥ 1；`grep -c '真实有效\|必须能通过\|禁止无效' ~/.config/opencode/skills/skwy-adversarial-test/SKILL.md` ≥ 1；`grep -c '边界\|错误路径\|并发\|性能' ~/.config/opencode/skills/skwy-adversarial-test/SKILL.md` ≥ 3
  QA scenarios: happy - grep 命中；failure - 若技能描述缺少边界约束，补充硬约束条款，Evidence .omo/evidence/skwy-workflow-migration/task-7.txt
  Commit: N（~/.config 仓库外，无 commit——台账 + sha256sum 记录，同 todo 3）

- [ ] 8. 创建全局技能 skwy-reflect（反思泛化）
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-reflect/SKILL.md`，泛化自 .opencode/skills/reflect/SKILL.md（240 行）：保留目的/角色/触发/工作流（第 0 步 session_read 提取纠正/收集上下文/编写条目/落实流程改进/趋势分析）/反思格式模板/边界/禁止；kb/dev/reflections.md 引用（10+ 处）改为「项目在自身 AGENTS.md 定义反思文件」引用点；reflections-archive.md、friction.md 移除（L98）等 compass 历史改泛化描述；ref #206/#186/#63 具体 issue 引用移除；git 验证命令（branch --contains/worktree list/log）保留为通用；worktree 示例（L114）改 skwy-worktree。MUST NOT: 保留 compass 具体 issue 号/文件名。
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 12-14
  References: .opencode/skills/reflect/SKILL.md（来源）; .omo/drafts/skwy-workflow-migration.md D3
  Acceptance criteria (agent-executable): `grep -c 'kb/dev/reflections' ~/.config/opencode/skills/skwy-reflect/SKILL.md` = 0；`grep -c 'ref #[0-9]' ~/.config/opencode/skills/skwy-reflect/SKILL.md` = 0；`grep -c 'session_read' ~/.config/opencode/skills/skwy-reflect/SKILL.md` ≥ 1
  QA scenarios: happy - grep 命中；failure - 若具体引用残留，改泛化措辞，Evidence .omo/evidence/skwy-workflow-migration/task-8.txt
  Commit: N（~/.config 仓库外，无 commit——台账 + sha256sum 记录，同 todo 3）

- [ ] 9. 创建全局技能 skwy-worktree + 迁移 scripts/open-worktrees.sh + tests
  What to do / Must NOT do: 创建 `~/.config/opencode/skills/skwy-worktree/`：SKILL.md 泛化自 .opencode/skills/worktree/SKILL.md（204 行）——**9 处相对路径 `scripts/open-worktrees.sh`（L68/L106-110/L124-125/L201）改为绝对路径 `~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh`**；ref #96/#104/#138 改泛化；compass-workflow 引用（L161-166）改 skwy-workflow；cargo test/clippy/fmt 改为通用示例。复制 `scripts/open-worktrees.sh`（391 行，零硬编码路径）到 `~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh`，SELF 自引用（L57）改为 `SELF="$(dirname "$0")/open-worktrees.sh"` 或新绝对路径，**并同步更新 L17-22 usage 注释中的相对路径示例为绝对路径**（Oracle 审查发现：usage 文本在全局运行时产生误导）；复制 `scripts/tests/open-worktrees-test.sh` 到 `~/.config/opencode/skills/skwy-worktree/scripts/tests/`，L8 SCRIPT 路径更新。MUST NOT: 保留相对路径引用（SKILL.md 与脚本 usage 注释均查）；不修改脚本逻辑（仅路径）。
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 12-14
  References: .opencode/skills/worktree/SKILL.md（来源）; scripts/open-worktrees.sh（来源，391 行）; scripts/tests/open-worktrees-test.sh（来源）; .omo/drafts/skwy-workflow-migration.md D1; kb/dev/process.md L186-208（worktree 流程）
  Acceptance criteria (agent-executable): `grep -c 'scripts/open-worktrees.sh' ~/.config/opencode/skills/skwy-worktree/SKILL.md` = 9（全部为绝对路径前缀）；`grep -c '\./scripts\|^scripts/\|^#   scripts/' ~/.config/opencode/skills/skwy-worktree/SKILL.md ~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh` = 0；`ls ~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh` 存在；`bash -n ~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh` 语法通过
  QA scenarios: happy - 文件存在 + bash -n 通过；failure - 若 SKILL.md 残留相对路径引用，改为绝对路径，Evidence .omo/evidence/skwy-workflow-migration/task-9.txt
  Commit: N（~/.config 仓库外，无 commit——台账 + sha256sum 记录，同 todo 3）

- [ ] 10. 创建全局 agent 定义 skwy-requirement-test.md
  What to do / Must NOT do: 创建 `~/.config/opencode/agent/skwy-requirement-test.md`（需 mkdir -p ~/.config/opencode/agent/），泛化自 .opencode/agent/test.md：frontmatter `name: skwy-requirement-test` + description（面向需求的独立 QA agent）；prompt 保留独立读生产代码/独立写测试/独立验证的职责、三层兜底（只改测试范围/禁 cargo run/无提交权）；`compass-workflow 门禁第 4 步`（L2）改 `skwy-workflow 门禁第 4 步`；`qa` skill 引用（L25-27/L42）改 `skwy-requirement-test` skill；permissions 中的 compass 路径（`crates/**/tests/**`、`collectors/tests/**`）改为通用 `**/tests/**` + 「项目在自身 AGENTS.md 定义测试路径」；bash deny 保留（禁 cargo run/git 写）；**移除全部 compass 特有表述**：正文开头的 "A-share stock chart desktop application (Rust egui + Python collectors)" 项目介绍改为通用描述（「项目在自身 AGENTS.md 定义」），`kb/dev/testing.md` 提及（4+ 处）改为「项目在自身 AGENTS.md/kb/dev/testing.md 定义测试约定与覆盖率门槛」，覆盖率数字（95%/80%）、compass-core/compass-data 等改为「项目自身覆盖率门槛」引用点。MUST NOT: 保留 compass 特定路径或表述（A-share/egui/compass-core/覆盖率数字）；不包含对抗性测试职责。
  Parallelization: Wave 3 | Blocked by: 3, 6 | Blocks: 12-14
  References: .opencode/agent/test.md（来源）; ~/.config/opencode/skills/skwy-requirement-test/SKILL.md（依赖，todo 6）; AGENTS.md「test Agent（独立 QA）」章节; context7 确认 opencode 从 {agent,agents}/**/*.md 自动加载
  Acceptance criteria (agent-executable): `grep -c 'name: skwy-requirement-test' ~/.config/opencode/agent/skwy-requirement-test.md` ≥ 1；`grep -c 'crates/\|collectors/\|A-share\|egui\|compass-core\|compass-data\|kb/dev/testing.md\|95%\|80%' ~/.config/opencode/agent/skwy-requirement-test.md` = 0；`grep -c 'skwy-workflow' ~/.config/opencode/agent/skwy-requirement-test.md` ≥ 1
  QA scenarios: happy - grep 命中；failure - 若 compass 路径残留，改通用路径，Evidence .omo/evidence/skwy-workflow-migration/task-10.txt
  Commit: N（文件在 ~/.config 仓库外，无 commit；在 plan/evidence 台账记录）

- [ ] 11. 创建全局 agent 定义 skwy-adversarial-test.md
  What to do / Must NOT do: 创建 `~/.config/opencode/agent/skwy-adversarial-test.md`（新 agent，无本地来源）：frontmatter `name: skwy-adversarial-test` + description（对抗性测试工程师，门禁 3.5 步）；prompt 定义：读取 plan + 相关代码接口 → 编写对抗性测试（RED 预期失败）→ 报告失败证据 → 实现修复后验证 GREEN；硬约束（真实有效、实现正确修复后必须通过、禁止无效/恶意测试）；load_skills 建议 `["skwy-adversarial-test"]` 注入方法论；permissions 与 skwy-requirement-test 相同通用模式（`**/tests/**` + bash deny cargo run/git 写 + 无提交权）；「项目在自身 AGENTS.md/kb/dev/testing.md 定义测试基础设施」。MUST NOT: 允许写无效测试；不含 compass 特定内容。
  Parallelization: Wave 3 | Blocked by: 3, 7 | Blocks: 12-14
  References: ~/.config/opencode/skills/skwy-adversarial-test/SKILL.md（依赖，todo 7）; AGENTS.md「test Agent（独立 QA）」章节（三层兜底模式参考）; .omo/drafts/skwy-workflow-migration.md D9
  Acceptance criteria (agent-executable): `grep -c 'name: skwy-adversarial-test' ~/.config/opencode/agent/skwy-adversarial-test.md` ≥ 1；`grep -c '真实有效\|必须能通过' ~/.config/opencode/agent/skwy-adversarial-test.md` ≥ 1；`grep -c 'bash.*deny\|cargo run' ~/.config/opencode/agent/skwy-adversarial-test.md` ≥ 1
  QA scenarios: happy - grep 命中；failure - 若缺约束，补充硬约束条款，Evidence .omo/evidence/skwy-workflow-migration/task-11.txt
  Commit: N（文件在 ~/.config 仓库外，无 commit；在 plan/evidence 台账记录）

- [ ] 12. compass 本地清理：删除 7 技能目录 + .opencode/agent/test.md + scripts/open-worktrees.sh
  What to do / Must NOT do: `git rm -r .opencode/skills/compass-workflow .opencode/skills/docs .opencode/skills/issue-workflow .opencode/skills/qa .opencode/skills/reflect .opencode/skills/rustdoc .opencode/skills/worktree`；`git rm .opencode/agent/test.md`；`git rm scripts/open-worktrees.sh scripts/tests/open-worktrees-test.sh`（脚本已复制到全局，todo 9 完成后再删）。保留：.opencode/skills/product/、.opencode/agent/ui-designer.md。MUST NOT: 删除 product 或 ui-designer；删除前确认全局副本已就绪（todo 3-11 完成）。
  Parallelization: Wave 4 | Blocked by: 3-11 | Blocks: 13
  References: .omo/drafts/skwy-workflow-migration.md D8; .opencode/skills/ 目录清单（8 项）
  Acceptance criteria (agent-executable): `ls .opencode/skills/` 仅含 product；`ls .opencode/agent/` 仅含 ui-designer.md；`ls scripts/open-worktrees.sh scripts/tests/open-worktrees-test.sh 2>&1` 报不存在
  QA scenarios: happy - 目录清单符合预期；failure - 若删错（product/ui-designer 消失），git checkout 恢复，Evidence .omo/evidence/skwy-workflow-migration/task-12.txt
  Commit: Y（**与 todo 13 合并为同一 commit**——删除技能 + AGENTS.md 引用同步同批提交，避免中间态 AGENTS.md 指向已删技能；commit message 覆盖两项）| refactor(workflow): remove migrated local skills and sync AGENTS.md to global skwy group

- [ ] 13. AGENTS.md 引用同步（Skills 表 / 门禁表 3.5 步 / test Agent 章节 / 路径引用）
  What to do / Must NOT do: 更新 AGENTS.md：Skills 表（L125-132）改为 7 行 skwy- 技能 + product 行（新表：skwy-workflow /skwy-workflow、skwy-github-workflow /skwy-github-workflow、skwy-git-workflow /skwy-git-workflow、skwy-requirement-test /skwy-requirement-test、skwy-adversarial-test /skwy-adversarial-test、skwy-reflect /skwy-reflect、skwy-worktree /skwy-worktree、product /product）；门禁表（L64-70）新增「3.5 Adversarial Tests」行（委派 skwy-adversarial-test 写对抗性测试）+ 第 4 步 Tests 改委派 skwy-requirement-test + 5a RUSTDOC 行移除 + 5b Docs 改为 skwy-workflow 内嵌文档同步章节；「所有 skill 位于 .opencode/skills/<name>/SKILL.md」（L134）改为「全局技能位于 ~/.config/opencode/skills/<name>/SKILL.md（可被 OpenCode 自动发现）；项目本地技能位于 .opencode/skills/」；L193 `.opencode/skills/issue-workflow/SKILL.md` 改 skwy-github-workflow；test Agent 章节（L164-182）更新为双 agent 路由（3.5→skwy-adversarial-test、4→skwy-requirement-test）+ 改名引用；/reflect（L101/234/292）、/test（L178）、/issue-workflow（L64）、/worktree（L33）、/docs（L68）全部改 skwy- 新命令；L92/L116/L146/L285-286/L303 compass-workflow 改 skwy-workflow；L321/L325/L330 scripts/open-worktrees.sh 改全局绝对路径；文件顶部「可用技能」表行更新。MUST NOT: 改 AGENTS.md 其他章节内容（Dolt/数据管线/测试门槛等不相关部分）；不改 /review-work、/ulw-plan、/grill-me 引用。
  Parallelization: Wave 4 | Blocked by: 12 | Blocks: 14
  References: AGENTS.md L64-70（门禁表）、L125-132（Skills 表）、L134、L193、L164-182（test Agent 章节）、L101/L234/L292（/reflect）、L321-330（scripts 路径）; .omo/drafts/skwy-workflow-migration.md C4/D6/D8
  Acceptance criteria (agent-executable): `rg -n 'compass-workflow|/issue-workflow|/rustdoc|/docs|/reflect|/worktree|qa skill|\.opencode/skills/issue-workflow' AGENTS.md | rg -v '分支/worktree'` = 0 输出（排除 /skwy- 前缀新名与 /review-work /ulw-plan /grill-me 与 L79 分支/worktree 通用措辞）；`grep -c '3.5' AGENTS.md` ≥ 1（门禁 3.5 步行）；`grep -c 'skwy-adversarial-test' AGENTS.md` ≥ 3（Skills 表 + 门禁表 + test Agent 章节）
  QA scenarios: happy - 上述 grep 全命中且无旧名残留；failure - 若残留旧名，逐一修正；Evidence .omo/evidence/skwy-workflow-migration/task-13.txt
  Commit: Y（与 todo 12 合并为同一 commit，见 todo 12 Commit 行）

- [ ] 14. kb 引用同步：process.md / ui.md / github/{impl,fix}.md / product/SKILL.md
  What to do / Must NOT do: kb/dev/process.md：L14/L44 /issue-workflow 改 /skwy-github-workflow、L18-19/L234 /reflect 改 /skwy-reflect、L85 compass-workflow 改 skwy-workflow、L138/L186/L206-208 /worktree 改 /skwy-worktree、L42 `.opencode/skills/issue-workflow/SKILL.md` 改 `~/.config/opencode/skills/skwy-github-workflow/SKILL.md`、**L263 `.opencode/skills/docs/SKILL.md` § 变更→kb 映射表改 skwy-workflow 内嵌文档同步章节引用（Metis+Momus 双审查发现的遗漏引用点）**、L201 scripts/open-worktrees.sh 改全局绝对路径、L274 **不改**（cargo-doc 约定行，Momus 确认非 skill 引用）。kb/design/ui.md L4 compass-workflow 门禁第 1 步改 skwy-workflow。kb/github/impl.md L38 + fix.md L56 `.opencode/skills/docs/SKILL.md` § 变更→kb 映射表改 skwy-workflow 内嵌文档同步章节引用。.opencode/skills/product/SKILL.md L18/L100-102 compass-workflow 引用改 skwy-workflow（product 保留但引用必须更新）。MUST NOT: 改 reflections*.md 历史内容；改 L274；改 /ulw-plan /review-work /grill-me 引用；改 kb/dev/testing.md 内容。
  Parallelization: Wave 5 | Blocked by: 13 | Blocks: 15
  References: kb/dev/process.md（L14/L18-19/L42/L44/L85/L138/L186/L194/L201/L206-208/L274）; kb/design/ui.md L4; kb/github/impl.md L38; kb/github/fix.md L56; .opencode/skills/product/SKILL.md L18/L100-102; .omo/drafts/skwy-workflow-migration.md C4
  Acceptance criteria (agent-executable): `rg -n 'compass-workflow|/issue-workflow|/rustdoc|/docs|/reflect|/worktree|\.opencode/skills/(docs|issue-workflow)' kb/dev/process.md kb/design/ui.md kb/github/impl.md kb/github/fix.md .opencode/skills/product/SKILL.md` = 0 输出（排除 /skwy-*、/review-work、/ulw-plan、/grill-me）；`grep -c 'skwy' kb/dev/process.md` ≥ 5
  QA scenarios: happy - 无旧名残留；failure - 若残留，逐一修正；Evidence .omo/evidence/skwy-workflow-migration/task-14.txt
  Commit: Y | docs(workflow): sync kb references to skwy skill group

- [ ] 15. 验证：技能/agent 可加载 + 无残留旧名 grep + open-worktrees 行为测试 + 新项目冒烟 + 全局文件 sha256
  What to do / Must NOT do: 运行完整验证套件并落盘证据：① 技能文件存在断言（7 个 SKILL.md + 2 个 agent md + scripts 存在）；② 全仓无残留旧名 grep（**锚定模式 + scope 收窄到引用承载文件**：`rg -n --hidden -g '!.git' -g '!target' -g '!.omo' -g '!node_modules' -g '!logs' -g '!.worktrees' -g '!reflections*.md' -e 'compass-workflow' -e 'issue-workflow' -e 'skwy-test' -e '/\b(reflect|test|worktree|docs|rustdoc)\b' -e '\.opencode/skills/(compass-workflow|docs|issue-workflow|qa|reflect|rustdoc|worktree)' AGENTS.md kb .opencode/agent .opencode/skills/product .opencode/plugins 2>/dev/null | rg -v '分支/worktree|chore/docs/test|test-sepa-daily'`——scope 排除 scripts/.githooks（迁移后不可能含技能引用，且 `chore/docs/test`（.githooks/pre-push:118 提示文本）、`test-sepa-daily.sh`（scripts/tests/ SEPA 测试脚本）为合法内容已验证误报源），`| rg -v` 白名单双保险（AGENTS.md L79 分支/worktree 属 SELF-CHECK 通用措辞），`/\b...\b` 词边界保证不命中 reflections.md/testing.md 文件名）；③ `bash ~/.config/opencode/skills/skwy-worktree/scripts/tests/open-worktrees-test.sh`（在仓库内运行）；④ 新项目冒烟：`tmp=$(mktemp -d) && cd "$tmp" && git init -q && printf '# T\n' > AGENTS.md && opencode run "列出当前可用的 /skwy- 斜杠命令" 2>&1`，断言输出含 `/skwy-workflow`、`/skwy-adversarial-test`、`/skwy-requirement-test` 至少各一次（证明全局技能在全新项目可发现加载）；⑤ 全局文件 sha256 落盘：`sha256sum ~/.config/opencode/skills/skwy-*/SKILL.md ~/.config/opencode/skills/skwy-worktree/scripts/open-worktrees.sh ~/.config/opencode/agent/skwy-*.md > .omo/evidence/skwy-workflow-migration/global-files.sha256`——让 PR 可绑定全局状态（Oracle 审查发现：全局文件在仓库外，PR 需凭哈希核验）。MUST NOT: 修改生产代码；不 push（等待用户指令）；不改测试脚本逻辑。
  Parallelization: Wave 6 | Blocked by: 12-14 | Blocks: F1-F4
  References: .omo/plans/skwy-workflow-migration.md 验证策略章节; ~/.config/opencode/skills/skwy-*/（todo 3-9 产物）; ~/.config/opencode/agent/skwy-*.md（todo 10-11 产物）; kb/dev/reflections-archive.md（排除依据，历史记录保留旧名）
  Acceptance criteria (agent-executable): ① `for d in skwy-workflow skwy-github-workflow skwy-git-workflow skwy-requirement-test skwy-adversarial-test skwy-reflect skwy-worktree; do test -f ~/.config/opencode/skills/$d/SKILL.md; done` 全部通过；② 锚定 grep 输出为空（排除列表内文件）；③ open-worktrees-test.sh exit 0；④ 冒烟输出含 3 个 /skwy- 命令名；⑤ global-files.sha256 文件存在且含 10 行（7 SKILL + 1 script + 2 agent）
  QA scenarios: happy - 全部断言通过，证据落盘 .omo/evidence/skwy-workflow-migration/task-15.txt；failure - 任一断言失败，定位（技能缺失/残留旧名/脚本 bug/冒烟未识别全局技能）修复后重跑；**冒烟专用分支**：若 `opencode run` 失败先查认证（`opencode auth list`，若未登录按 ~/.config/opencode/github-token 流程补认证）与网络（api.opencode.ai 连通性），环境故障记录后重试，非技能问题不得视为残留旧名误报，Evidence .omo/evidence/skwy-workflow-migration/task-15.txt
  Commit: N（验证产物记录到 .omo/evidence/，无代码变更；如有修复走对应 todo 的 commit）

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit: 逐条核对 todos 1-15 完成状态 + evidence 落盘（.omo/evidence/skwy-workflow-migration/），对照 issue #210 验收标准（更新后）逐项核实
- [ ] F2. Code quality review: 全局技能/agent 文件内容审查（gate 3.5 语义与 RED/GREEN 契约、对抗性测试边界约束、泛化程度、无 compass 残留（A-share/egui/覆盖率数字/具体 kb 路径）、路径引用正确、绝对路径 vs 相对路径）；对照 .omo/evidence/skwy-workflow-migration/global-files.sha256 复核全局文件哈希与内容一致
- [ ] F3. Real manual QA: 执行 todo 15 验证套件（技能加载/无残留旧名/行为测试/新项目冒烟），确认全部通过
- [ ] F4. Scope fidelity: 确认 Scope OUT 全部遵守（product/ui-designer/reflections/测试门槛未动、无产品代码变更、L274 未改）

## Commit strategy
- 每个 todo 独立 commit，message 含 `ref #210`（issue 驱动提交纪律）
- **todos 1-2, 12-14 为仓库内变更**（issue body 同步经 gh 命令、决策记录/kb/AGENTS.md 文件）：每 todo 一个 commit（todo 12+13 合并为同一 commit，避免中间态 AGENTS.md 指向已删技能）
- **todos 3-11 只创建 ~/.config/opencode/ 仓库外文件**：无 commit，创建状态 + 文件内容逐字记录到 .omo/evidence/skwy-workflow-migration/task-<N>.txt 台账（含每个文件的 sha256，todo 15 统一复核）；台账文件随 12+13 合并 commit 一并提交
- **todo 15 验证产物**（task-15.txt、global-files.sha256）：随 12+13 合并 commit 之后的任意仓库 commit（或单独 evidence commit）提交，保证 PR 内可凭哈希核验全局状态
- **计划文件本身**：`.omo/plans/skwy-workflow-migration.md` 与 `.omo/evidence/` 台账随首个 commit（todo 1）一起 `git add` 提交（plan/evidence 属 .omo 已放行目录）
- 顺序：todo 1 → 2 → (3-9 并行) → (10-11 并行) → 12+13 → 14 → 15 → 等待用户 push 指令 → /skwy-reflect 反思 commit → push → issue 收尾
- push 前 rebase base（git fetch origin master）

## Success criteria
- [ ] issue #210 body 与 handoff 记录 7 技能 + 2 agents + gate 3.5 决策
- [ ] ~/.config/opencode/skills/ 下 7 个 skwy- 技能 + ~/.config/opencode/agent/ 下 2 个 agent 可被 opencode 发现
- [ ] compass 本地 7 技能目录 + test.md + scripts 脚本已删除，product/ui-designer 保留
- [ ] AGENTS.md + kb 引用无残留旧名（grep 验证通过）
- [ ] open-worktrees 行为测试通过（全局路径运行）
- [ ] 新项目可仅依赖全局技能组运行工作流（冒烟验证）
- [ ] 门禁 3.5 步 Adversarial Tests 写入 skwy-workflow 技能
