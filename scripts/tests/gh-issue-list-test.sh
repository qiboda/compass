#!/bin/bash
# Adversarial tests for the batch gh issue list query in commit-msg/pre-push (ref #213).
#
# Target behavior (future state): the hooks fetch the set of OPEN issue numbers
# in ONE batch call —
#   open_set=$(unset GITHUB_TOKEN 2>/dev/null; gh issue list --repo qiboda/compass --state open --json number --limit 5000 --jq '.[].number' 2>/dev/null || echo "GH_FAIL")
# — then resolve every referenced issue locally (`echo "$open_set" | grep -qx "$n"`).
# Fail-closed: gh failure (GH_FAIL) or an empty open set rejects the commit/push.
#
# A fake `gh` injected on PATH records every invocation (CALL_START line + one
# arg per line) and answers ONLY `issue list` with canned numbers. Any other
# invocation — including the OLD per-issue `gh issue view` — exits 1, so an
# unrefactored hook is rejected and its recorded args mismatch the exact
# expected invocation. Assertions (all arg-for-arg, no substring relaxation):
#   * exact single invocation `gh issue list --repo qiboda/compass --state open
#     --json number --limit 5000 --jq '.[].number'`
#   * exactly ONE gh issue list call per commit-msg run / per push (batch, not
#     per-issue loop)
#   * open accepted; closed/missing rejected; gh failure rejected; empty set rejected
#   * both commit-msg and pre-push covered
#   * regression guard: neither hook may regress to the per-issue `gh issue view` loop
#
# Run: bash scripts/tests/gh-issue-list-test.sh
# HOOK_COMMIT_MSG / HOOK_PRE_PUSH env vars override the hook paths (used by the
# green-simulation run against scratch copies of the target implementation).
set -euo pipefail

REPO_ROOT=$(git -C "$(dirname "$0")/../.." rev-parse --show-toplevel 2>/dev/null || echo ".")
HOOK_COMMIT_MSG="${HOOK_COMMIT_MSG:-$REPO_ROOT/.githooks/commit-msg}"
HOOK_PRE_PUSH="${HOOK_PRE_PUSH:-$REPO_ROOT/.githooks/pre-push}"
[ -f "$HOOK_COMMIT_MSG" ] || { echo "FAIL: hook missing: $HOOK_COMMIT_MSG"; exit 1; }
[ -f "$HOOK_PRE_PUSH" ] || { echo "FAIL: hook missing: $HOOK_PRE_PUSH"; exit 1; }

FAIL=0
SCENARIO_N=0
LAST_LOG=""

TEST_TMP=$(mktemp -d)
trap 'rm -rf "$TEST_TMP"' EXIT

# --- fake gh: records invocations, answers only the batch issue list call ---
FAKE_BIN="$TEST_TMP/bin"
mkdir -p "$FAKE_BIN"
cat > "$FAKE_BIN/gh" <<'EOF'
#!/bin/bash
# fake gh for hook tests (ref #213). Records every invocation to $FAKE_GH_LOG:
# a CALL_START line followed by one arg per line. Only `issue list` is answered
# (cats $FAKE_GH_LIST_OUTPUT, exit $FAKE_GH_LIST_EXIT, default 0). Everything
# else — e.g. the OLD per-issue `issue view` — exits 1 (gh failure), which makes
# an unrefactored hook see MISSING and reject.
{
    echo "CALL_START"
    printf '%s\n' "$@"
} >> "${FAKE_GH_LOG:?}"
if [ "${1:-}" = "issue" ] && [ "${2:-}" = "list" ]; then
    if [ -n "${FAKE_GH_LIST_EXIT:-}" ] && [ "$FAKE_GH_LIST_EXIT" -ne 0 ]; then
        exit "$FAKE_GH_LIST_EXIT"
    fi
    cat "${FAKE_GH_LIST_OUTPUT:?}"
    exit 0
fi
exit 1
EOF
chmod +x "$FAKE_BIN/gh"

# --- fake cargo: pre-push's fmt/clippy/doc gates are not under test (ref #213)
cat > "$FAKE_BIN/cargo" <<'EOF'
#!/bin/bash
exit 0
EOF
chmod +x "$FAKE_BIN/cargo"

export PATH="$FAKE_BIN:$PATH"

# Expected exact batch invocation (target design, ref #213). Arg-for-arg compare
# of the whole recorded log — no substring relaxation.
EXPECTED_LIST_LOG='CALL_START
issue
list
--repo
qiboda/compass
--state
open
--json
number
--limit
5000
--jq
.[].number'

verdict() {  # $1=name $2=expect(0|nonzero) $3=rc $4=hook output
    local name="$1" expect="$2" rc="$3" out="$4"
    if [ "$expect" = "0" ]; then
        if [ "$rc" -eq 0 ]; then
            echo "PASS: $name (accepted)"
        else
            echo "FAIL: $name — expected accept (rc=0), got rc=$rc"
            printf '%s\n' "$out" | sed 's/^/    /'
            FAIL=1
        fi
    else
        if [ "$rc" -ne 0 ]; then
            echo "PASS: $name (rejected rc=$rc)"
        else
            echo "FAIL: $name — expected reject, got rc=0"
            FAIL=1
        fi
    fi
}

# --- commit-msg scenario: $1=name $2=msgtext $3=expect(0|nonzero)
#     $4=open_set_content $5=fake_list_exit(0|1) ---
check_commit_msg() {
    local name="$1" msgtext="$2" expect="$3" open_content="$4" list_exit="$5"
    SCENARIO_N=$((SCENARIO_N + 1))
    local n="$SCENARIO_N" rc=0 out
    printf '%s\n' "$msgtext" > "$TEST_TMP/msg.$n"
    printf '%s' "$open_content" > "$TEST_TMP/open.$n"
    export FAKE_GH_LOG="$TEST_TMP/log.$n" \
           FAKE_GH_LIST_OUTPUT="$TEST_TMP/open.$n" \
           FAKE_GH_LIST_EXIT="$list_exit"
    LAST_LOG="$FAKE_GH_LOG"
    if out=$(bash "$HOOK_COMMIT_MSG" "$TEST_TMP/msg.$n" 2>&1); then
        rc=0
    else
        rc=$?
    fi
    verdict "$name" "$expect" "$rc" "$out"
}

# --- pre-push scenario: $1=name $2=expect(0|nonzero) $3=open_set_content
#     $4=fake_list_exit(0|1)  $5.. = commit messages (base commit auto-made) ---
check_pre_push() {
    local name="$1" expect="$2" open_content="$3" list_exit="$4"
    shift 4
    SCENARIO_N=$((SCENARIO_N + 1))
    local n="$SCENARIO_N"
    local dir="$TEST_TMP/repo.$n" i=0 rc=0 out oid base_sha
    git init -q "$dir"
    git -C "$dir" config user.name test
    git -C "$dir" config user.email test@example.com
    echo base > "$dir/f"
    git -C "$dir" add f
    git -C "$dir" commit -q -m base
    base_sha=$(git -C "$dir" rev-parse HEAD)
    git -C "$dir" update-ref refs/remotes/origin/master "$base_sha"
    for m in "$@"; do
        i=$((i + 1))
        echo "change $i" > "$dir/f"
        git -C "$dir" add f
        git -C "$dir" commit -q -m "$m"
    done
    oid=$(git -C "$dir" rev-parse HEAD)
    printf '%s' "$open_content" > "$TEST_TMP/open.$n"
    export FAKE_GH_LOG="$TEST_TMP/log.$n" \
           FAKE_GH_LIST_OUTPUT="$TEST_TMP/open.$n" \
           FAKE_GH_LIST_EXIT="$list_exit"
    LAST_LOG="$FAKE_GH_LOG"
    if out=$(cd "$dir" && printf 'refs/heads/feature %s refs/remotes/origin/feature 0000000000000000000000000000000000000000\n' "$oid" | bash "$HOOK_PRE_PUSH" 2>&1); then
        rc=0
    else
        rc=$?
    fi
    verdict "$name" "$expect" "$rc" "$out"
}

# Exact whole-log comparison: exactly ONE recorded invocation and its args must
# equal the target batch call, arg for arg (order, flags, values — no substring).
check_exact_list_args() {
    local name="$1" log="$2" got
    got=$(cat "$log" 2>/dev/null || true)
    if [ "$got" = "$EXPECTED_LIST_LOG" ]; then
        echo "PASS: $name (exact single batch-list invocation)"
    else
        echo "FAIL: $name — hook did not issue the exact batch list call (arg-for-arg)"
        echo "  --- expected ---"
        printf '%s\n' "$EXPECTED_LIST_LOG" | sed 's/^/  /'
        echo "  --- recorded ($log) ---"
        printf '%s\n' "$got" | sed 's/^/  /'
        FAIL=1
    fi
}

# Count gh issue list calls: within a recorded invocation (CALL_START block),
# arg1 must be "issue" and arg2 "list". Anything else (old `issue view` loop)
# does not count.
check_one_batch_call() {
    local name="$1" log="$2" got
    got=$(awk '/^CALL_START$/{in_issue=0} $0=="issue"{in_issue=1; next} in_issue && $0=="list"{c++} END{print c+0}' "$log" 2>/dev/null || true)
    if [ "$got" -eq 1 ]; then
        echo "PASS: $name (exactly one gh issue list call)"
    else
        echo "FAIL: $name — expected exactly 1 gh issue list call, got $got"
        cat "$log" 2>/dev/null | sed 's/^/    /' || true
        FAIL=1
    fi
}

echo "=== commit-msg ==="

# a. OPEN issue accepted; b. exact single batch invocation.
check_commit_msg "commit-msg: open issue #213 accepted" $'fix: sync\n\nref #213' 0 '213' 0
check_exact_list_args "commit-msg: exact batch-list invocation (open case)" "$LAST_LOG"
check_one_batch_call "commit-msg: exactly one gh issue list call (open case)" "$LAST_LOG"

# c. Two OPEN issues in one commit — accepted via a SINGLE batch call.
check_commit_msg "commit-msg: two open issues accepted — one batch call" $'fix: multi\n\nref #213, #96' 0 '213
96' 0
check_exact_list_args "commit-msg: exact batch-list invocation (multi-issue)" "$LAST_LOG"
check_one_batch_call "commit-msg: exactly one gh issue list call (multi-issue)" "$LAST_LOG"

# d. Referenced issue absent from the open set (closed or nonexistent) — rejected.
check_commit_msg "commit-msg: issue not in open set rejected (closed/missing)" $'fix: x\n\nref #213' nonzero '96' 0

# e. Partial match — one of two refs not open — rejected (fail-closed on any).
check_commit_msg "commit-msg: partial match rejected (one of two not open)" $'fix: x\n\nref #213, #96' nonzero '213' 0

# f. gh failure (fake list exits 1) — rejected (fail-closed).
check_commit_msg "commit-msg: gh failure rejects (fail-closed)" $'fix: x\n\nref #213' nonzero '213' 1

# g. gh succeeds but open set empty — rejected (fail-closed).
check_commit_msg "commit-msg: empty open set rejects (fail-closed)" $'fix: x\n\nref #213' nonzero '' 0

echo "=== pre-push ==="

# h. OPEN issue accepted; exact single batch invocation.
check_pre_push "pre-push: open issue #213 accepted" 0 '213' 0 $'fix: one\n\nref #213'
check_exact_list_args "pre-push: exact batch-list invocation (open case)" "$LAST_LOG"
check_one_batch_call "pre-push: exactly one gh issue list call (open case)" "$LAST_LOG"

# i. 3 commits each with a ref — accepted and exactly ONE batch call for the push.
check_pre_push "pre-push: 3 issues across commits — one batch call" 0 '211
213
96' 0 $'fix: a\n\nref #213' $'fix: b\n\nref #96' $'fix: c\n\nref #211'
check_exact_list_args "pre-push: exact batch-list invocation (3 issues)" "$LAST_LOG"
check_one_batch_call "pre-push: exactly one gh issue list call (3 issues)" "$LAST_LOG"

# j. Issue not in open set — push rejected.
check_pre_push "pre-push: issue not open rejected" nonzero '96' 0 $'fix: one\n\nref #213'

# j2. Partial match — 3 commits, one of three refs not open — push rejected
#     (fail-closed on any ref, mirror of commit-msg scenario e).
check_pre_push "pre-push: partial match rejected (one of three not open)" nonzero '213
96' 0 $'fix: a\n\nref #213' $'fix: b\n\nref #96' $'fix: c\n\nref #211'

# k. gh failure — push rejected (fail-closed).
check_pre_push "pre-push: gh failure rejects (fail-closed)" nonzero '213' 1 $'fix: one\n\nref #213'

# l. Empty open set — push rejected (fail-closed).
check_pre_push "pre-push: empty open set rejects (fail-closed)" nonzero '' 0 $'fix: one\n\nref #213'

echo "=== regression guards: per-issue gh issue view loop must not return ==="

# m. Neither hook may regress to the per-issue `gh issue view` loop.
for hook in "$HOOK_COMMIT_MSG" "$HOOK_PRE_PUSH"; do
    if grep -qE 'gh issue view' "$hook"; then
        echo "FAIL: guard — $(basename "$hook") still contains per-issue 'gh issue view' (batch list required, ref #213)"
        FAIL=1
    else
        echo "PASS: guard — $(basename "$hook") has no 'gh issue view'"
    fi
done

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit "$FAIL"
fi
