#!/bin/bash
# Behavior tests for scripts/open-worktrees.sh (ref #96).
# Self-contained: creates its own temp git repo + worktree, never depends on
# the developer machine's .worktrees/ contents.
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

# --- temp git repo + worktree fixture (self-contained) ---
TMP="$(mktemp -d)"
REPO="$TMP/repo"
WT="$TMP/wt"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

git init -q "$REPO"
git -C "$REPO" config user.email test@compass.local
git -C "$REPO" config user.name Test
git -C "$REPO" commit --allow-empty -qm init
git -C "$REPO" worktree add -q -b feat/test-wt "$WT"

# 1. Syntax check
bash -n "$SCRIPT"
check "syntax valid" "true"

# 2. detect-terminal: either returns a terminal or fails cleanly (env-dependent,
#    never asserted as must-succeed on headless machines)
"$SCRIPT" --detect-terminal >/dev/null 2>&1
check "detect-terminal exit 0/1 (never crashes)" "[ \$? -le 1 ]"

# 3. --list: script lists OUR temp worktree when WT_DIR pointed at fixture.
#    The script hardcodes WT_DIR from its own path, so simulate by extracting
#    list_worktrees (and its has_worktree dependency) and overriding WT_DIR.
check "list_worktrees lists fixture wt" "bash -c '
    func=\$(sed -n \"/^has_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^list_worktrees()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    WT_DIR=$TMP
    list_worktrees | grep -qx wt
'"

# 4. No pgrep rejection: script must not refuse to start
check "no pgrep rejection (auto-launch)" "! grep -q 'pgrep -f \"opencode\"' '$SCRIPT'"

# 5. Uses setsid for detachment
check "uses setsid" "grep -q 'setsid' '$SCRIPT'"

# 6. No tmux dependency
check "no tmux dependency" "! grep -q 'tmux' '$SCRIPT'"

# 7. no-arg dry-run does NOT print usage (regression: "" case used to exit 1)
check "no-arg opens all (no usage error)" "! \"$SCRIPT\" --dry-run 2>&1 | grep -q '^usage:'"

# 8. dry-run for a nonexistent worktree exits 0 (skip path)
check "dry-run nonexistent exits 0" "\"$SCRIPT\" --dry-run definitely-not-a-worktree >/dev/null 2>&1"

# 9. --close with a real worktree: dry-run prints the removal commands.
#    Extract close_worktree + has_worktree, point WT_DIR at the fixture.
check "close dry-run lists removal" "bash -c '
    func=\$(sed -n \"/^has_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^close_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    WT_DIR=$TMP
    DRY_RUN=1
    close_worktree wt 2>&1 | grep -q \"git worktree remove\"
'"

# 10. --close + --dry-run flag combination (order-independent)
check "close+dry-run reversed order" "bash -c '
    func=\$(sed -n \"/^has_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^close_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    WT_DIR=$TMP
    DRY_RUN=1
    CLOSE=1
    close_worktree wt 2>&1 | grep -q \"git worktree remove\"
'"

# 11. Close skips nonexistent worktree (no crash)
check "close nonexistent is skip (exit 0)" "\"$SCRIPT\" --close --dry-run definitely-not-a-worktree >/dev/null 2>&1"

# 12. Injection safety on the REAL exec path (not just dry-run echo):
#     a stub `setsid` on PATH captures the argv passed to the terminal binary,
#     proving $dir arrives as a SINGLE argv element, never re-parsed by a shell.
#     Payload contains &, >, $(), quotes — all legal in git refnames.
check "injection-safe argv (real exec path)" "bash -c '
    mkdir -p $TMP/stub
    cat > $TMP/stub/setsid <<\"EOF\"
#!/bin/sh
printf \"%s\\n\" \"\$@\" >> $TMP/captured-argv
EOF
    chmod +x $TMP/stub/setsid
    cat > $TMP/stub/kitty <<\"EOF\"
#!/bin/sh
exit 0
EOF
    chmod +x $TMP/stub/kitty
    func=\$(sed -n \"/^launch_in_terminal()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    PATH=$TMP/stub:\$PATH
    dir=\"$TMP/wt/x&touch>/tmp/openwt-PWNED\"
    launch_in_terminal kitty \"\$dir\"
    grep -qx \"kitty\" $TMP/captured-argv
    grep -qx -- \"--directory\" $TMP/captured-argv
    grep -qx \"\$dir\" $TMP/captured-argv
    [ ! -e /tmp/openwt-PWNED ]
' 2>/dev/null"
rm -f /tmp/openwt-PWNED

# 13. launch_in_terminal returns 1 for unknown terminal (manual fallback),
#     and the open loop survives it (set -e guard) — verify call-site `|| true`.
check "unknown terminal falls back (exit 1)" "bash -c '
    func=\$(sed -n \"/^launch_in_terminal()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    DRY_RUN=1
    launch_in_terminal nosuchterm /tmp >/dev/null 2>&1
    [ \$? -eq 1 ]
'"
check "open loop guards launch with || true" "grep -q 'launch_in_terminal \"\$term\" \"\$dir\" || true' '$SCRIPT'"

# 14. No xdg-terminal-emulator backend remains (injection surface removed,
#     ref #96 round-2 security review: Debian-only command, argv not preserved)
check "no xdg-terminal-emulator branch" "! grep -q 'xdg-terminal-emulator' '$SCRIPT'"

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
