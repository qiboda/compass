#!/bin/bash
# Open/close worktree zones in the OS default terminal, each running opencode.
#
# Open: starts opencode in the worktree directory, detached from the current
# process group (via setsid) so it keeps running after the launching session
# ends. No manual "unbind the current opencode session" step is required
# (ref #96).
#
# Close: stops any opencode whose cwd points at the worktree, closes the
# holding terminal window, then removes the worktree and its branch (ref #96).
# When the caller itself lives inside the worktree (e.g. --close issued from
# that worktree's own opencode session), cleanup is handed off to a
# setsid-detached subprocess so it survives the caller being killed (ref #104).
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
# Terminal detection: $TERMINAL → known terminal list
# ---------------------------------------------------------------------------

detect_terminal() {
    local candidate

    if [ -n "${TERMINAL:-}" ] && command -v "$TERMINAL" >/dev/null 2>&1; then
        echo "$TERMINAL"
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
        *)
            # No launcher for this terminal; print a manual command instead.
            echo "  cd $dir && opencode   # run manually (no launcher for $term)" >&2
            return 1
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Close: stop opencode holding the worktree, close its terminal, then remove
# worktree + branch
# ---------------------------------------------------------------------------

# Returns 0 if $1 is the calling process or one of its ancestors (walking the
# ppid chain via /proc/$pid/stat field 4). Guards the close kill loop against
# ever killing the caller itself (ref #104).
is_ancestor_of_self() {
    local target="$1" cur="$$"
    while [ "$cur" -gt 1 ] 2>/dev/null; do
        [ "$cur" = "$target" ] && return 0
        cur="$(awk '{print $4}' "/proc/$cur/stat" 2>/dev/null || echo 0)"
    done
    return 1
}

# Prints the pid of the terminal window hosting $1, walking its ppid chain.
# Matches per-window terminal binaries only — never gnome-terminal-server /
# xfce4-terminal daemon, whose kill would close unrelated windows (ref #104).
find_terminal_pid() {
    local pid="$1"
    local cur="$pid" cmdline base
    for _ in $(seq 1 16); do
        cmdline="$(tr '\0' ' ' < "/proc/$cur/cmdline" 2>/dev/null || true)"
        base="$(basename "${cmdline%% *}" 2>/dev/null || true)"
        case "$base" in
            kitty|konsole|xfce4-terminal|gnome-terminal|xterm)
                echo "$cur"
                return 0
                ;;
        esac
        cur="$(awk '{print $4}' "/proc/$cur/stat" 2>/dev/null || echo 0)"
        [ "$cur" -gt 1 ] 2>/dev/null || break
    done
    return 1
}

# Internal mode (ref #104): full close executed in a setsid-detached session,
# so it survives the caller — an opencode living inside the worktree — being
# killed. Re-enters this script as `--close-detached <wt> <dir>`.
launch_detached_cleanup() {
    local wt="$1" dir="$2"
    local log="$PROJECT_ROOT/logs/open-worktrees-close.log"
    mkdir -p "$PROJECT_ROOT/logs"
    setsid nohup "$0" --close-detached "$wt" "$dir" >>"$log" 2>&1 &
    echo "  cleanup continues in a detached session (log: $log)"
}

# Kills every opencode/holder whose cwd is $dir, closes their terminal
# windows, waits for exit, then removes the worktree and its branch.
detached_cleanup() {
    local wt="$1" dir="$2"
    local holders="" term_pids="" pid cwd term branch
    cd "$PROJECT_ROOT" || exit 1

    for pid in $(pgrep -f 'opencode' || true) $(pgrep -f 'bash-language-server' || true); do
        [ -n "$pid" ] || continue
        [ "$pid" = "$$" ] && continue
        cwd="$(readlink "/proc/$pid/cwd" 2>/dev/null || true)"
        [ "$cwd" = "$dir" ] || continue
        echo "  stopping opencode (pid $pid) in $wt"
        holders="$holders $pid"
        term="$(find_terminal_pid "$pid" || true)"
        kill "$pid" 2>/dev/null || true
        if [ -n "$term" ] && ! echo "$term_pids" | grep -q "$term"; then
            term_pids="$term_pids $term"
        fi
    done

    for _ in $(seq 1 15); do
        local alive=0
        for pid in $holders; do
            kill -0 "$pid" 2>/dev/null && alive=1
        done
        [ "$alive" -eq 0 ] && break
        sleep 1
    done
    for pid in $holders; do
        if kill -0 "$pid" 2>/dev/null; then
            echo "  force-killing $pid"
            kill -9 "$pid" 2>/dev/null || true
        fi
    done

    for term in $term_pids; do
        echo "  closing terminal (pid $term)"
        kill "$term" 2>/dev/null || true
    done

    branch="$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
    git -C "$PROJECT_ROOT" worktree remove --force "$dir"
    if [ -n "$branch" ] && [ "$branch" != "master" ] && [ "$branch" != "HEAD" ]; then
        git -C "$PROJECT_ROOT" branch -D "$branch"
    fi
}

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
    #    A holder on the caller's own ancestor chain means the caller runs
    #    inside this worktree — hand the whole close off to a detached
    #    session instead of killing ourselves mid-cleanup (ref #104).
    local holders="" pid cwd term seen_terms=""
    for pid in $(pgrep -f 'opencode' || true) $(pgrep -f 'bash-language-server' || true); do
        [ -n "$pid" ] || continue
        cwd="$(readlink "/proc/$pid/cwd" 2>/dev/null || true)"
        [ "$cwd" = "$dir" ] || continue
        if is_ancestor_of_self "$pid"; then
            echo "  caller runs inside $wt; handing off cleanup to a detached session"
            if [ -z "${DRY_RUN:-}" ]; then
                launch_detached_cleanup "$wt" "$dir"
            else
                echo "  (dry-run) would detach cleanup for $wt"
            fi
            return 0
        fi
        holders="$holders $pid"
    done

    # 2. Stop external holders and close their terminal windows.
    for pid in $holders; do
        echo "  stopping opencode (pid $pid) in $wt"
        if [ -z "${DRY_RUN:-}" ]; then
            term="$(find_terminal_pid "$pid" || true)"
            kill "$pid" 2>/dev/null || true
            if [ -n "$term" ] && ! echo "$seen_terms" | grep -q "$term"; then
                seen_terms="$seen_terms $term"
                echo "  closing terminal (pid $term)"
                kill "$term" 2>/dev/null || true
            fi
        fi
    done

    # 3. Remove the worktree and its branch.
    #    Detached-HEAD worktrees report "HEAD" from --abbrev-ref; never try to
    #    `git branch -D HEAD` (it fails) — just remove the worktree itself.
    #    git runs with -C PROJECT_ROOT: close may be invoked from any cwd.
    local branch
    branch="$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
    if [ -n "${DRY_RUN:-}" ]; then
        echo "  git worktree remove --force \"$dir\""
        if [ -n "$branch" ] && [ "$branch" != "master" ] && [ "$branch" != "HEAD" ]; then
            echo "  git branch -D $branch"
        fi
    else
        git -C "$PROJECT_ROOT" worktree remove --force "$dir"
        if [ -n "$branch" ] && [ "$branch" != "master" ] && [ "$branch" != "HEAD" ]; then
            git -C "$PROJECT_ROOT" branch -D "$branch"
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
    --close-detached)
        # Internal mode (ref #104): invoked by launch_detached_cleanup via
        # `setsid nohup $0 --close-detached <wt> <dir>` so the full close
        # survives the caller (an opencode inside the worktree) being killed.
        if [ "$#" -ne 3 ]; then
            echo "usage: open-worktrees.sh --close-detached <wt> <dir>" >&2
            exit 1
        fi
        detached_cleanup "$2" "$3"
        exit 0
        ;;
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
    # || true: an unknown/empty terminal returns 1 from launch_in_terminal
    # (manual command already printed); do not let set -e abort the loop.
    launch_in_terminal "$term" "$dir" || true
done

echo ""
echo "done. opencode sessions detached via setsid."
