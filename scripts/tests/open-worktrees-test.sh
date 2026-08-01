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
#    never asserted as must-succeed on headless machines). Guarded so a
#    nonzero exit cannot abort the suite via set -e.
set +e
"$SCRIPT" --detect-terminal >/dev/null 2>&1
DETECT_RC=$?
set -e
check "detect-terminal exit 0/1 (never crashes)" "[ \$DETECT_RC -le 1 ]"

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

# 11b. Detached-HEAD worktree: --close must not attempt `git branch -D HEAD`
#      (regression guard, ref #96 round-3 review). Create a second fixture
#      worktree checked out at a raw commit, close it via the real function.
check "close detached-HEAD skips branch -D" "bash -c '
    git -C $REPO worktree add -q --detach $TMP/wt-detached HEAD
    func=\$(sed -n \"/^has_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^close_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    WT_DIR=$TMP
    DRY_RUN=1
    out=\$(close_worktree wt-detached 2>&1)
    echo \"\$out\" | grep -q \"git worktree remove\"
    ! echo \"\$out\" | grep -q \"git branch -D HEAD\"
'"

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
    # launch_in_terminal backgrounds the stub setsid; wait for its argv write
    # before asserting (avoids a race between the async write and the greps).
    i=0
    while [ ! -s $TMP/captured-argv ] && [ \$i -lt 50 ]; do
        sleep 0.05
        i=\$((i + 1))
    done
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

# 16. Any cwd=$dir holder (caller OR external) must trigger a setsid-detached
#     cleanup — close_worktree must never kill in-process, because killing a
#     holder that is the caller would kill the script mid-cleanup. This holds
#     even when the holder is NOT an ancestor of the caller (ppid chains are
#     unreliable under opencode's bash tool, ref #104 round-2). Stub pgrep to
#     report an external child-process holder, stub setsid to capture argv.
check "close with holder detaches via setsid" "bash -c '
    set -e
    func=\$(sed -n \"/^has_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^find_terminal_pid()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^launch_detached_cleanup()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    func=\$(sed -n \"/^close_worktree()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    PROJECT_ROOT=$TMP
    WT_DIR=$TMP
    mkdir -p $TMP/stub
    cat > $TMP/stub/setsid <<\"EOF\"
#!/bin/sh
printf \"%s\\n\" \"\$@\" >> $TMP/detached-argv
EOF
    cat > $TMP/stub/git <<\"EOF\"
#!/bin/sh
printf \"GIT-STUB %s\\n\" \"\$@\" >> $TMP/git-argv
exit 0
EOF
    chmod +x $TMP/stub/setsid $TMP/stub/git
    PATH=$TMP/stub:\$PATH
    (cd $TMP/wt && sleep 30) &
    HOLDER=\$!
    pgrep() { echo \$HOLDER; }
    close_worktree wt 2>&1
    kill \$HOLDER 2>/dev/null || true
    i=0
    while [ ! -s $TMP/detached-argv ] && [ \$i -lt 50 ]; do
        sleep 0.05
        i=\$((i + 1))
    done
    [ -s $TMP/detached-argv ]
' 2>/dev/null"

# 17. find_terminal_pid: an ordinary process chain (init, pid 1) has no
#     terminal window to close (ref #104 terminal-close requirement).
check "find_terminal_pid rejects non-terminal chain" "bash -c '
    set -e
    func=\$(sed -n \"/^find_terminal_pid()/,/^}/p\" \"$SCRIPT\")
    eval \"\$func\"
    declare -F find_terminal_pid >/dev/null
    ! find_terminal_pid 1
'"

# 19. detached_cleanup must locate the holding terminal so its window can be
#     closed together with the opencode process (ref #104, user decision B).
check "detached cleanup locates holding terminal" "bash -c '
    body=\$(sed -n \"/^detached_cleanup()/,/^}/p\" \"$SCRIPT\")
    echo \"\$body\" | grep -q find_terminal_pid
'"

# 20. launch_detached_cleanup must hand the cleanup off via setsid (detached
#     session survives the caller being killed, ref #104).
check "close path detaches via setsid" "bash -c '
    body=\$(sed -n \"/^launch_detached_cleanup()/,/^}/p\" \"$SCRIPT\")
    echo \"\$body\" | grep -q setsid
'"

# 21. find_terminal_pid must NOT match xfce4-terminal (single-instance D-Bus
#     daemon whose process name is xfce4-terminal — killing it closes every
#     window) nor gnome-terminal-server (client-server daemon, ref #104).
check "terminal whitelist excludes daemons" "bash -c '
    body=\$(sed -n \"/^find_terminal_pid()/,/^}/p\" \"$SCRIPT\")
    echo \"\$body\" | grep -q \"kitty|konsole|gnome-terminal|xterm\"
    ! echo \"\$body\" | grep -q xfce4-terminal
    ! echo \"\$body\" | grep -q gnome-terminal-server
'"

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
