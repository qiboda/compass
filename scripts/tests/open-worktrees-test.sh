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

# 8. dry-run for a nonexistent worktree exits 0 (skip path)
check "dry-run nonexistent exits 0" "\"$SCRIPT\" --dry-run definitely-not-a-worktree >/dev/null 2>&1"

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
