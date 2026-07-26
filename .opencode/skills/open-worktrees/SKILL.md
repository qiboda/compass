---
name: /open-worktrees
description: Launch all worktree zones in separate tmux panes with opencode. Use when user says "open worktrees", "启动worktree", or "launch worktrees".
---

# Open Worktrees

When invoked, run the script to launch all worktree zones:

```bash
scripts/open-worktrees.sh
```

Then tell the user the session is ready and how to attach:

```
tmux attach -t compass-worktrees
```
