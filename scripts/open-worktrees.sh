#!/bin/bash
# Open worktree zones in the OS default terminal, each running opencode.
#
# The new opencode processes are started detached from the current process
# group (via setsid) so they keep running after the launching session ends.
# No manual "unbind the current opencode session" step is required (ref #96).
#
# Usage:
#   scripts/open-worktrees.sh           # open all worktrees
#   scripts/open-worktrees.sh gui data  # open specific ones
#   scripts/open-worktrees.sh --list    # list available worktrees
#   scripts/open-worktrees.sh --detect-terminal  # print detected terminal
#   scripts/open-worktrees.sh --dry-run # print commands without running

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WT_DIR="$PROJECT_ROOT/.worktrees"

# ---------------------------------------------------------------------------
# Terminal detection: $TERMINAL → xdg-terminal-emulator → known list
# ---------------------------------------------------------------------------

detect_terminal() {
    local candidate

    if [ -n "${TERMINAL:-}" ] && command -v "$TERMINAL" >/dev/null 2>&1; then
        echo "$TERMINAL"
        return 0
    fi

    if command -v xdg-terminal-emulator >/dev/null 2>&1; then
        echo "xdg-terminal-emulator"
        return 0
    fi

    for candidate in kitty gnome-terminal konsole xfce4-terminal xterm; do
        if command -v "$candidate" >/dev/null 2>&1; then
            echo "$candidate"
            return 0
        fi
    done

    return 1
}

# Build the command that opens a terminal at `dir` and runs `cmd` inside it.
# Emits nothing on failure (caller falls back to printing the raw command).
terminal_cmd() {
    local term="$1" dir="$2" cmd="$3"
    case "$term" in
        kitty)            echo "kitty --directory '$dir' -- $cmd" ;;
        gnome-terminal)   echo "gnome-terminal --working-directory='$dir' -- bash -c '$cmd; exec bash'" ;;
        konsole)          echo "konsole --workdir '$dir' -e bash -c '$cmd; exec bash'" ;;
        xfce4-terminal)   echo "xfce4-terminal --working-directory='$dir' -- bash -c '$cmd; exec bash'" ;;
        xterm)            echo "xterm -e bash -c 'cd \"$dir\" && $cmd; exec bash'" ;;
        xdg-terminal-emulator)
            # xdg-terminal-emulator does not accept a working dir; cd in a wrapper shell.
            echo "xdg-terminal-emulator -e bash -c 'cd \"$dir\" && $cmd; exec bash'" ;;
        *)                return 1 ;;
    esac
}

# ---------------------------------------------------------------------------
# Worktree helpers
# ---------------------------------------------------------------------------

has_worktree() {
    [ -d "$WT_DIR/$1" ] && git -C "$WT_DIR/$1" rev-parse --git-dir &>/dev/null
}

list_worktrees() {
    local name
    for name in "$WT_DIR"/*/; do
        [ -d "$name" ] || continue
        local base
        base="$(basename "$name")"
        has_worktree "$base" && echo "$base"
    done
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

ARG="${1:-}"
shift 2>/dev/null || true

case "$ARG" in
    --list)
        list_worktrees
        exit 0
        ;;
    --detect-terminal)
        if term="$(detect_terminal)"; then
            echo "$term"
            exit 0
        fi
        echo "no terminal detected" >&2
        exit 1
        ;;
    --dry-run)
        DRY_RUN=1
        WANT="$*"
        ;;
    "")
        echo "usage: open-worktrees.sh [--list|--detect-terminal|--dry-run] [worktree...]" >&2
        exit 1
        ;;
    *)
        WANT="$ARG $*"
        ;;
esac

# Worktrees to open: explicit args, or all available.
if [ -z "${WANT:-}" ]; then
    WANT="$(list_worktrees | tr '\n' ' ')"
fi

if [ -z "$WANT" ]; then
    echo "error: no worktrees found under $WT_DIR" >&2
    exit 1
fi

if [ -n "${DRY_RUN:-}" ]; then
    echo "# dry-run: would open worktrees: $WANT"
fi

if term="$(detect_terminal)"; then
    echo "terminal: $term"
else
    echo "warning: no terminal detected; printing commands for manual run" >&2
fi

for wt in $WANT; do
    if ! has_worktree "$wt"; then
        echo "skip: $wt (not a worktree)" >&2
        continue
    fi
    dir="$WT_DIR/$wt"
    # Setsid detaches each terminal process from this session's process group,
    # so the opencode inside survives this conversation ending (ref #96).
    if cmd="$(terminal_cmd "$term" "$dir" "opencode")"; then
        if [ -n "${DRY_RUN:-}" ]; then
            echo "  setsid bash -c '$cmd' &"
        else
            setsid bash -c "$cmd" &
        fi
    else
        echo "  cd $dir && opencode   # run manually (no launcher for $term)" >&2
    fi
done

echo ""
echo "done. opencode sessions detached via setsid."
