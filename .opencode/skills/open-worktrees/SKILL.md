---
name: /open-worktrees
description: Launch all worktree zones in separate kitty windows with opencode. Use when user says "open worktrees", "启动worktree", or "launch worktrees".
---

# Open Worktrees

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
