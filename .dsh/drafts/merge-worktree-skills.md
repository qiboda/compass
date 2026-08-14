---
slug: merge-worktree-skills
status: awaiting-approval
intent: clear
review_required: false
pending-action: write .omo/plans/merge-worktree-skills.md
approach: 合并 worktree/open-worktrees skills 为单一权威源；改造 open-worktrees.sh 为自动启动（默认终端探测 + setsid 脱离）；同步 AGENTS.md/process.md 索引；单 commit ref #96
---

# Draft: merge-worktree-skills

## Components (topology ledger)
<!-- Lock the SHAPE before depth. One row per top-level component that can succeed or fail independently. -->
| id | outcome | status | evidence path |
|---|---|---|---|
| C1 | 合并 skill：`worktree/SKILL.md` 为唯一权威源，吸收启动职责；删除 `open-worktrees/SKILL.md` 及 `//open-worktrees` 命令 | active | .opencode/skills/{worktree,open-worktrees}/SKILL.md |
| C2 | 改造 `scripts/open-worktrees.sh`：删 pgrep 拒绝 precheck；探测默认终端（$TERMINAL→xdg-terminal-emulator→kitty/gnome-terminal/konsole/xfce4-terminal）；setsid 启动脱离进程组；去 tmux | active | scripts/open-worktrees.sh:14-69 |
| C3 | 同步索引：`AGENTS.md` skill 表（line 104）+ Worktrees 章节（line 207）；`kb/dev/process.md`（line 162-163） | active | AGENTS.md:104,207; kb/dev/process.md:162-163 |
| C4 | 测试：验证脚本自动启动行为（不再拒绝、默认终端探测、setsid 使用） | active | scripts/open-worktrees.sh (test section) |

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
| assumption | adopted default | rationale | reversible? |
|---|---|---|---|
| 探测链 `$TERMINAL` → `xdg-terminal-emulator` → 已知列表 | 从 xdg-terminal-emulator 开始，回退到 `$TERMINAL`，最后已知列表 | xdg-terminal-emulator 是 XDG 标准入口；已知列表兜底无该入口的系统 | yes |
| 脚本从 bash 工具调用，自身已脱离交互终端 | 脚本内 `setsid` 包裹终端启动命令（非必须——从 opencode bash 调用已是子进程，需显式脱离） | 对话结束杀死子进程树，必须 setsid 隔离 | yes |

## Findings (cited - path:lines)

1. 环境：Hyprland + kitty 是唯一已装终端；`$TERMINAL` 未设；无 `xdg-terminal-emulator`、gnome-terminal 等（`which` 全空）— bash 探查输出
2. `worktree/SKILL.md:53-61` — 步骤 2 将「解绑」描述为「退出当前 opencode 实例」，错误语义
3. `open-worktrees/SKILL.md:8-11` — 重复同一段「先退出/停止当前 opencode 实例」警告
4. `scripts/open-worktrees.sh:14-23` — `pgrep -f opencode` 发现实例即 `exit 1`，是「手动退出」模式的代码实现；`open_in_tmux()` (line 28-36) 用 tmux send-keys
5. `AGENTS.md:104` — skill 表含 `open-worktrees` 行（`//open-worktrees` 命令）；`AGENTS.md:207` — Worktrees 章节「解绑当前 opencode session」
6. `kb/dev/process.md:162-163` — 索引句「加载 /worktree skill 获取完整流程（…解绑 opencode session…）」
7. `kb/dev/reflections.md:268` — 历史经验记录（含「先解绑」表述）——历史记录，不改

## Decisions (with rationale)

| 决策 | 选项 | 选择 | 理由 | 排除原因 |
|---|---|---|---|---|
| skill 合并 | 保留两个 / 合并为一个 | 保留 `worktree`（含启动职责），删除 `open-worktrees` | 一个 worktree 生命周期一个 skill，减少维护面；open-worktrees 的「启动」是 worktree 生命周期的自然部分 | 两 skill 各自独立 → 机制描述重复（本次问题根源） |
| 「解绑」语义 | 手动退出 / 自动脱离 | 自动脱离（setsid 使新进程脱离当前进程组） | 对话结束新 opencode 进程不随之死亡；无需用户手动处理 | 手动退出要求用户干预，体验差且易忘 |
| 脚本启动方式 | tmux / 默认终端 | 探测 OS 默认终端开新窗口 | 不依赖特定终端模拟器/tmux；kitty 环境天然支持新窗口 | tmux 会话不是「默认终端」语义 |
| precheck | 拒绝启动 / 自动启动 | 删除 pgrep 拒绝，改为自动启动 | 拒绝逻辑与自动脱离语义矛盾 | 保留拒绝 → 仍要求用户手动退出 |

## Scope IN

1. 编辑 `worktree/SKILL.md`：吸收启动区域职责（原 open-worktrees 功能），重写步骤 2 为自动解绑（setsid），成为唯一机制权威源
2. 删除 `.opencode/skills/open-worktrees/SKILL.md`
3. 重写 `scripts/open-worktrees.sh`：默认终端探测 + setsid 自动启动 + 去 tmux + 删 pgrep 拒绝
4. 更新 `AGENTS.md`：skill 表删除 open-worktrees 行、Worktrees 章节「解绑」措辞改为自动启动
5. 更新 `kb/dev/process.md` line 162-163 索引句措辞
6. 脚本测试：`--dry-run`/单元测试验证探测链与启动行为
7. 单 commit `ref #96` → review → reflect

## Scope OUT (Must NOT have)

- 不改 `kb/dev/reflections.md`（历史记录）
- 不删除/重命名 `scripts/open-worktrees.sh`（保留文件名，`worktree` skill 引用它）
- 不引入新的脚本依赖（仅 bash 内置 + 标准探测）
- 不触碰 issue #80 的 `.worktrees/cleanup-stock-basic` 工作树
- 不实现任何 Rust/Python 代码变更

## Open questions

无（grill-me Q1-Q5 已锁定全部 owner-decision；其余为执行细节，探查已回答）

## Approval gate
status: awaiting-approval
<!-- When exploration is exhausted and unknowns are answered, set status: awaiting-approval. -->
<!-- That durable record is the loop guard: on a later turn read it and resume at the gate instead of re-running exploration. -->
