# merge-worktree-skills - Work Plan

## TL;DR (For humans)

**What you'll get:** 两个 worktree skill（worktree + open-worktrees）合并为一个 `worktree` skill，启动脚本改为自动打开系统默认终端里的 opencode，无需手动退出当前对话。

**Why this approach:** 「解绑」的正确语义是新进程自动脱离当前对话的进程组（setsid），不是手动退出；合并 skill 使机制只写一处，消除四处重复表述的维护负担。

**What it will NOT do:** 不改 reflections.md 历史记录；不重命名脚本文件；不引入新依赖；不触碰 issue #80 的 cleanup-stock-basic 工作树。

**Effort:** Short
**Risk:** Low - 纯脚本 + 文档/skill 文件改动，无 Rust/Python 生产代码
**Decisions to sanity-check:** 默认终端探测链顺序；setsid 的使用位置（脚本内包裹 vs 调用侧）

Your next move: approve. Full execution detail follows below.

---

> TL;DR (machine): Short effort, Low risk — merge two worktree skills into one authoritative skill, rewrite open-worktrees.sh for auto-launch via detected default terminal + setsid detachment, sync AGENTS.md/process.md index lines.

## Scope
### Must have
- 合并 `worktree` + `open-worktrees` 两个 skill 为一个（保留 `worktree`，含创建/删除 + 启动区域职责），删除 `open-worktrees` skill 及 `//open-worktrees` 命令
- 重写 `scripts/open-worktrees.sh`：删 pgrep 拒绝 precheck；探测默认终端；setsid 自动启动脱离进程组；去 tmux 依赖
- 同步 `AGENTS.md`（skill 表 + Worktrees 章节）、`kb/dev/process.md` 索引句
- 脚本行为测试（不拒绝启动、探测链、setsid 使用）
- 单 commit `ref #96`

### Must NOT have (guardrails, anti-slop, scope boundaries)
- 不改 `kb/dev/reflections.md`（历史记录）
- 不重命名/删除 `scripts/open-worktrees.sh` 文件本身（保留文件名，skill 引用它）
- 不引入新脚本依赖（仅 bash 内置 + 标准探测命令）
- 不触碰 `.worktrees/cleanup-stock-basic`（issue #80 工作树）
- 不实现任何 Rust/Python 生产代码变更

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after（bash 脚本，用 bash -n 语法检查 + 探测链单元测试 + dry-run 行为断言）
- Evidence: .omo/evidence/merge-worktree-skills/

## Execution strategy
### Parallel execution waves
- Wave 1: C2 脚本重写 + C4 脚本测试（耦合，同一 todo）
- Wave 2: C1 skill 合并 + C3 文档同步（可并行，但都依赖 C2 的最终脚本接口确认）

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 (脚本重写+测试) | — | 2,3 | — |
| 2 (skill 合并) | 1 | 4 | 3 |
| 3 (AGENTS.md+process.md 同步) | 1 | 4 | 2 |
| 4 (全量验证+commit) | 2,3 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
- [ ] 1. scripts/open-worktrees.sh: 重写为默认终端探测 + setsid 自动启动，附带探测链测试
  What to do / Must NOT do: 删除 line 14-23 的 pgrep 拒绝 precheck 与 tmux 逻辑（line 28-36, 44-69）；新增：默认终端探测函数（$TERMINAL → xdg-terminal-emulator → kitty/gnome-terminal/konsole/xfce4-terminal 按序探测），为每个 worktree 在探测到的终端新窗口执行 `cd <wt> && opencode`，用 setsid 包裹终端启动命令使其脱离当前进程组；无探测到终端时打印命令让用户手动运行。Must NOT: 保留 tmux；引入新工具依赖；拒绝启动。
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2,3
  References: scripts/open-worktrees.sh:1-69（全文件重写）
  Acceptance criteria: `bash -n scripts/open-worktrees.sh` 通过；`scripts/open-worktrees.sh --detect-terminal` 输出探测结果；dry-run 模式打印将执行的命令且 exit 0
  QA scenarios: happy: 探测链返回 kitty 并构造启动命令；failure: 无终端可用时打印提示且不 crash。Evidence .omo/evidence/merge-worktree-skills/task-1.md
  Commit: N（与 2,3 合并为单 commit）
- [ ] 2. .opencode/skills/worktree/SKILL.md: 吸收 open-worktrees 启动职责，重写「解绑」步骤为自动解绑，成为唯一权威源；删除 open-worktrees/SKILL.md
  What to do / Must NOT do: 在 worktree SKILL.md 的创建后步骤 2 中，将「先解绑当前 opencode session（退出当前实例）」改写为「自动启动：运行 scripts/open-worktrees.sh，脚本探测默认终端并以 setsid 启动新 opencode，脱离当前对话进程组，对话结束新进程不受影响」；新增「启动工作树区域」章节（合并自 open-worktrees skill，含脚本用法 `scripts/open-worktrees.sh [name...]`）；删除 .opencode/skills/open-worktrees/SKILL.md 文件。Must NOT: 保留「手动退出当前实例」表述；复制机制描述到其他文件。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: .opencode/skills/worktree/SKILL.md:46-70, 139-143; .opencode/skills/open-worktrees/SKILL.md:1-26
  Acceptance criteria: grep「解绑当前 opencode session」在工作树 skill 中为 0 命中；grep「先退出/停止当前 opencode」全仓库 0 命中（reflections.md 除外）；open-worktrees/SKILL.md 文件不存在
  QA scenarios: happy: grep 验证旧表述清除；failure: 保留旧表述则 grep 命中报错。Evidence .omo/evidence/merge-worktree-skills/task-2.md
  Commit: N（与 1,3 合并为单 commit）
- [ ] 3. AGENTS.md + kb/dev/process.md: 同步 skill 表与 Worktrees 章节措辞
  What to do / Must NOT do: AGENTS.md line 104 删除 open-worktrees 行（skill 表）；line 207 将「解绑当前 opencode session」改为「自动启动工作树区域（见 worktree skill）」；kb/dev/process.md line 162-163 索引句去掉「解绑 opencode session」，改为「启动区域」。Must NOT: 复制机制细节；改动其他 kb 文件。
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: AGENTS.md:104, 204-208; kb/dev/process.md:156-167
  Acceptance criteria: AGENTS.md skill 表无 open-worktrees 行；grep「解绑」在 AGENTS.md/process.md 为 0 命中
  QA scenarios: happy: grep 验证；failure: 残留命中报错。Evidence .omo/evidence/merge-worktree-skills/task-3.md
  Commit: N（与 1,2 合并为单 commit）
- [ ] 4. 全量验证 + 单 commit ref #96
  What to do / Must NOT do: 运行 git diff 审查全部变更；`bash -n` 脚本；grep 旧表述清除；确认 open-worktrees skill 文件已删；`git add` 相关文件 → `git commit -m "refactor: merge worktree skills, auto-launch via default terminal\n\nref #96"`。Must NOT: 包含无关文件；commit 不引用 #96。
  Parallelization: Wave 3 | Blocked by: 2,3 | Blocks: —
  References: 全部变更文件
  Acceptance criteria: commit 成功且 message 含 `ref #96`；git status 干净
  QA scenarios: happy: commit 成功；failure: pre-push hook 校验 ref 引用。Evidence .omo/evidence/merge-worktree-skills/task-4.md
  Commit: Y | refactor(worktree): merge skills, auto-launch via default terminal

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
- [ ] F2. Code quality review
- [ ] F3. Real manual QA
- [ ] F4. Scope fidelity

## Commit strategy
- 单 commit：`refactor(worktree): merge skills, auto-launch via default terminal`，message 含 `ref #96`
- commit 后运行 `/review-work`，处理发现，再 `/reflect`

## Success criteria
- 旧「手动退出」语义在所有活跃文件（skill/AGENTS.md/process.md/脚本）中清零
- `open-worktrees` skill 删除，启动职责并入 `worktree` skill
- 脚本自动探测默认终端启动 opencode，setsid 脱离进程组
- 单 commit `ref #96` 落地
