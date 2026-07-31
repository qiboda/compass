---
name: /open-worktrees
description: 在独立 kitty 窗口中用 opencode 启动所有工作树区域。当用户说 "open worktrees"、"启动worktree" 或 "launch worktrees" 时触发。
---

# Open Worktrees

**⚠️ 先解绑当前 opencode session（MANDATORY）**——opencode 将每个工作树
目录映射为与 master 的*同一项目*。在 master 的 opencode 实例仍在运行时
在工作树中启动 `opencode` 会失败——master session 仍绑定着共享的项目。
在运行此 skill 之前先退出/停止当前 opencode 实例，然后从新终端运行。

调用时，运行脚本启动工作树区域：

```bash
scripts/open-worktrees.sh [name...]
```

不带参数时列出可用工作树。指定名称打开特定工作树：

```bash
scripts/open-worktrees.sh data gui
```

在 kitty 终端中，每个工作树在新窗口中打开。否则，脚本会为每个工作树
打印 `opencode` 命令——在独立终端中运行它们。
