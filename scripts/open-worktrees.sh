#!/bin/bash
# Open all worktree zones in separate tmux panes with opencode.
#
# Usage:
#   scripts/open-worktrees.sh          # open all three
#   scripts/open-worktrees.sh gui      # open specific one

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SESSION="compass-worktrees"
WT_DIR="$PROJECT_ROOT/.worktrees"

# --- helpers ---
has_worktree() { [ -d "$WT_DIR/$1" ] && git -C "$WT_DIR/$1" rev-parse --git-dir &>/dev/null; }

open_in_tmux() {
    local name="$1"
    if ! has_worktree "$name"; then
        echo "skip: $name (not a worktree)" >&2
        return
    fi
    tmux send-keys -t "$SESSION:0" "cd $WT_DIR/$name && opencode" Enter
    echo "opened: $name"
}

if [ "${1:-}" != "" ]; then
    WANT="$1"
else
    WANT="gui data infra"
fi

# --- start tmux session if not already running ---
if tmux has-session -t "$SESSION" 2>/dev/null; then
    echo "session $SESSION already exists"
else
    FIRST=$(echo "$WANT" | awk '{print $1}')
    if ! has_worktree "$FIRST"; then
        echo "error: no valid worktree found" >&2
        exit 1
    fi
    tmux new-session -d -s "$SESSION" -c "$WT_DIR/$FIRST"
    echo "started session: $SESSION"
fi

# --- open each worktree in its own pane ---
COUNT=0
for wt in $WANT; do
    COUNT=$((COUNT + 1))
    if [ $COUNT -gt 1 ]; then
        tmux split-window -t "$SESSION:0" -c "$WT_DIR/$wt"
        tmux select-layout -t "$SESSION:0" tiled
    fi
    open_in_tmux "$wt"
done

echo ""
echo "attach: tmux attach -t $SESSION"
