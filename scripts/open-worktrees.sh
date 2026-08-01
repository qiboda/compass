#!/bin/bash
# Open/close worktree zones in the OS default terminal, each running opencode.
#
# Open: starts opencode in the worktree directory, detached from the current
# process group (via setsid) so it keeps running after the launching session
# ends. No manual "unbind the current opencode session" step is required
# (ref #96).
#
# Close: stops any opencode whose cwd points at the worktree, then removes
# the worktree and its branch (ref #96). Use when opencode holds the worktree
# directory and `git worktree remove` would fail.
#
# Usage:
#   scripts/open-worktrees.sh            # open all worktrees
#   scripts/open-worktrees.sh gui data   # open specific ones
#   scripts/open-worktrees.sh --list     # list available worktrees
#   scripts/open-worktrees.sh --detect-terminal  # print detected terminal
#   scripts/open-worktrees.sh --dry-run [wt...]  # print commands without running
#   scripts/open-worktrees.sh --close [wt...]    # stop opencode + remove worktree

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

# ---------------------------------------------------------------------------
# Open: launch opencode in a worktree's terminal
#
# $dir is always passed as an argv element (never interpolated into a command
# string), so worktree names containing quotes or $() cannot inject commands.
# ---------------------------------------------------------------------------

launch_in_terminal() {
    local term="$1" dir="$2"

    # Path safety: dir may contain spaces/quotes; as an argv element (quoted
    # below) it is never re-parsed by a shell.
    case "$term" in
        kitty)
            if [ -n "${DRY_RUN:-}" ]; then
                echo "  setsid kitty --directory \"$dir\" -- opencode &"
            else
                setsid kitty --directory "$dir" -- opencode >/dev/null 2>&1 &
            fi
            ;;
        gnome-terminal)
            if [ -n "${DRY_RUN:-}" ]; then
                echo "  setsid gnome-terminal --working-directory=\"$dir\" -- bash -c 'opencode; exec bash' &"
            else
                setsid gnome-terminal --working-directory="$dir" -- bash -c 'opencode; exec bash' >/dev/null 2>&1 &
            fi
            ;;
        konsole)
            if [ -n "${DRY_RUN:-}" ]; then
                echo "  setsid konsole --workdir \"$dir\" -e bash -c 'opencode; exec bash' &"
            else
                setsid konsole --workdir "$dir" -e bash -c 'opencode; exec bash' >/dev/null 2>&1 &
            fi
            ;;
        xfce4-terminal)
            if [ -n "${DRY_RUN:-}" ]; then
                echo "  setsid xfce4-terminal --working-directory=\"$dir\" -- bash -c 'opencode; exec bash' &"
            else
                setsid xfce4-terminal --working-directory="$dir" -- bash -c 'opencode; exec bash' >/dev/null 2>&1 &
            fi
            ;;
        xterm)
            if [ -n "${DRY_RUN:-}" ]; then
                echo "  setsid xterm -e bash -c 'cd \"\$1\" && opencode; exec bash' _ \"$dir\" &"
            else
                setsid xterm -e bash -c 'cd "$1" && opencode; exec bash' _ "$dir" >/dev/null 2>&1 &
            fi
            ;;
        xdg-terminal-emulator)
            if [ -n "${DRY_RUN:-}" ]; then
                echo "  setsid xdg-terminal-emulator -e bash -c 'cd \"\$1\" && opencode; exec bash' _ \"$dir\" &"
            else
                setsid xdg-terminal-emulator -e bash -c 'cd "$1" && opencode; exec bash' _ "$dir" >/dev/null 2>&1 &
            fi
            ;;
        *)
            # No launcher for this terminal; print a manual command instead.
            echo "  cd $dir && opencode   # run manually (no launcher for $term)" >&2
            return 1
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Close: stop opencode holding the worktree, then remove worktree + branch
# ---------------------------------------------------------------------------

close_worktree() {
    local wt="$1"
    local dir="$WT_DIR/$wt"

    if ! has_worktree "$wt"; then
        echo "skip: $wt (not a worktree)" >&2
        return 0
    fi

    # 1. Stop any opencode whose cwd is this worktree directory.
    #    pgrep -f matches the script's own process too, but the cwd check
    #    below (readlink /proc/PID/cwd) confines the kill to real holders.
    local pids
    pids="$(pgrep -f 'opencode' || true)"
    for pid in $pids; do
        local cwd
        cwd="$(readlink "/proc/$pid/cwd" 2>/dev/null || true)"
        if [ "$cwd" = "$dir" ]; then
            echo "  stopping opencode (pid $pid) in $wt"
            if [ -z "${DRY_RUN:-}" ]; then
                kill "$pid" 2>/dev/null || true
            fi
        fi
    done

    # 2. Remove the worktree and its branch.
    local branch
    branch="$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
    if [ -n "${DRY_RUN:-}" ]; then
        echo "  git worktree remove --force \"$dir\""
        if [ -n "$branch" ] && [ "$branch" != "master" ]; then
            echo "  git branch -D $branch"
        fi
    else
        git worktree remove --force "$dir"
        if [ -n "$branch" ] && [ "$branch" != "master" ]; then
            git branch -D "$branch"
        fi
    fi
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

# Exclusive mode flags must be the first argument.
case "${1:-}" in
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
esac

# Combinable flags (--dry-run / --close) may appear anywhere; the first
# non-flag argument starts the worktree name list.
DRY_RUN=""
CLOSE=""
WANT=""

for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        --close) CLOSE=1 ;;
        --*) echo "usage: open-worktrees.sh [--list|--detect-terminal|--dry-run|--close] [worktree...]" >&2; exit 1 ;;
        *) WANT="$WANT $arg" ;;
    esac
done

# Worktrees to act on: explicit names, or all available (documented default).
if [ -z "$WANT" ]; then
    WANT="$(list_worktrees | tr '\n' ' ')"
fi

if [ -z "$WANT" ]; then
    echo "error: no worktrees found under $WT_DIR" >&2
    exit 1
fi

# --- Close mode ---
if [ -n "${CLOSE:-}" ]; then
    echo "# close: worktrees: $WANT"
    for wt in $WANT; do
        close_worktree "$wt"
    done
    echo ""
    echo "done. worktrees removed."
    exit 0
fi

# --- Open mode ---
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
    launch_in_terminal "$term" "$dir"
done

echo ""
echo "done. opencode sessions detached via setsid."
