---
name: /open-worktrees
description: Launch all worktree zones in separate kitty windows with opencode. Use when user says "open worktrees", "启动worktree", or "launch worktrees".
---

# Open Worktrees

**⚠️ 先解绑当前 opencode session（MANDATORY）** — opencode maps each worktree
directory to the *same project* as master. Launching `opencode` in a worktree
while the current (master) opencode instance is still running fails — the master
session still binds the shared project. Exit/quit the current opencode instance
before running this skill, then run it from a new terminal.

When invoked, run the script to launch worktree zones:

```bash
scripts/open-worktrees.sh [name...]
```

No args lists available worktrees. Specify names to open specific ones:

```bash
scripts/open-worktrees.sh data gui
```

In kitty terminals, each worktree opens in a new window. Otherwise, the script prints
`opencode` commands for each worktree — run them in separate terminals.
