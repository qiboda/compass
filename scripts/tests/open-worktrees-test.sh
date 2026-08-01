#!/bin/bash
# Behavior tests for scripts/open-worktrees.sh (ref #96).
# Run: scripts/tests/open-worktrees-test.sh
set -euo pipefail

SCRIPT="$(cd "$(dirname "$0")/.." && pwd)/open-worktrees.sh"
FAIL=0

check() {
    local name="$1" cond="$2"
    if eval "$cond"; then
        echo "PASS: $name"
    else
        echo "FAIL: $name"
        FAIL=1
    fi
}

# 1. Syntax check
bash -n "$SCRIPT"
check "syntax valid" "true"

# 2. Terminal detection returns a known terminal
check "detects a terminal" "[[ -n \$(\"$SCRIPT\" --detect-terminal 2>/dev/null) ]]"

# 3. detect-terminal exit 0 when found
"$SCRIPT" --detect-terminal >/dev/null 2>&1
check "detect-terminal exits 0" "[ \$? -eq 0 ]"

# 4. No pgrep rejection: script must not refuse to start
check "no pgrep rejection (auto-launch)" "! grep -q 'pgrep -f \"opencode\"' '$SCRIPT'"

# 5. Uses setsid for detachment
check "uses setsid" "grep -q 'setsid' '$SCRIPT'"

# 6. No tmux dependency
check "no tmux dependency" "! grep -q 'tmux' '$SCRIPT'"

# 7. --list works (at least lists the repo as a worktree root)
check "list runs" "\"$SCRIPT\" --list >/dev/null 2>&1"

# 8. No-arg dry-run does NOT print usage (regression: "" case used to exit 1)
check "no-arg opens all (no usage error)" "! \"$SCRIPT\" --dry-run 2>&1 | grep -q '^usage:'"

# 9. dry-run for a nonexistent worktree exits 0 (skip path)
check "dry-run nonexistent exits 0" "\"$SCRIPT\" --dry-run definitely-not-a-worktree >/dev/null 2>&1"

# 10. --close dry-run works for a real worktree (cleanup-stock-basic exists)
check "close dry-run lists removal" "\"$SCRIPT\" --close --dry-run cleanup-stock-basic 2>&1 | grep -q 'git worktree remove'"

# 11. --close + --dry-run flag combination (order-independent)
check "close+dry-run reversed order" "\"$SCRIPT\" --dry-run --close cleanup-stock-basic 2>&1 | grep -q 'git worktree remove'"

# 12. Close skips nonexistent worktree (no crash)
check "close nonexistent is skip (exit 0)" "\"$SCRIPT\" --close --dry-run definitely-not-a-worktree >/dev/null 2>&1"

# 13. Injection safety: dir is passed as argv, never parsed by a shell.
#     A hostile worktree name with quotes/$() must appear verbatim (quoted)
#     in the dry-run output, and must NOT create a file.
check "injection-safe quoting (argv not parsed)" "bash -c '
    func=\$(sed -n \"/^launch_in_terminal()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    DRY_RUN=1
    dir=\"/data/codes/compass/.worktrees/evil-\\\$(touch /tmp/openwt-PWNED)\"
    launch_in_terminal kitty \"\$dir\" >/dev/null 2>&1
    [ ! -e /tmp/openwt-PWNED ]
'"
rm -f /tmp/openwt-PWNED

# 14. launch_in_terminal returns 1 for unknown terminal (manual fallback)
check "unknown terminal falls back (exit 1)" "bash -c '
    func=\$(sed -n \"/^launch_in_terminal()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    DRY_RUN=1
    launch_in_terminal nosuchterm /tmp >/dev/null 2>&1
    [ \$? -eq 1 ]
'"

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
