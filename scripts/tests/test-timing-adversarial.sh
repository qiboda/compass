#!/bin/bash
# Adversarial RED tests for issue #334: sync database timing statistics.
#
# Scope: shell-layer only. The Rust `TimingWriter`/`TimingEvent` module is
# implemented and covered by its own Rust unit tests in this PR; this shell
# suite stays focused on the update-database.sh integration behavior.
#
# Attack dimensions (against the plan's declared commitments):
#   1. Timing must never block the main flow: bad COMPASS_TIMING_FILE /
#      unwritable SYNC_TIMING_DIR must still exit 0 + emit a stderr warning.
#   2. Bad JSONL from the Rust child must not break the main flow: the report
#      must still be produced from valid events; malformed lines only warn.
#   3. Failed steps are recorded without swallowing errors: a failing cargo
#      invocation must produce a `status:"failed"` step event in the JSONL,
#      the script still exits non-zero, and "step N failed" stays visible.
#   4. Timing is always-on by default but must not change success/failure
#      semantics or emit unexpected timing errors; temp JSONL is cleaned up.
#   5. File path / run_id uniqueness: two runs in the same second (different
#      PIDs) must produce two distinct final JSON files, never overwrite.
#   6. Shell pollution/injection: step name/status/source containing quotes,
#      spaces, shell metacharacters must still yield valid final JSON.
#
# Run: bash scripts/tests/test-timing-adversarial.sh
# Requires: jq, bash. No cargo/dolt/real network access; all mocked.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SEPA_SCRIPT="$PROJECT_ROOT/scripts/update-database.sh"

FAIL=0
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAIL=1; }
skip() { echo "SKIP: $1"; }

# ---------------------------------------------------------------------------
# 0. syntax gate for this test file and the production script
# ---------------------------------------------------------------------------
echo "--- 0. syntax ---"
if bash -n "$0" 2>&1; then
    echo "PASS: test-timing-adversarial.sh syntax"
else
    echo "FAIL: test-timing-adversarial.sh syntax"
    exit 1
fi
if bash -n "$SEPA_SCRIPT" 2>&1; then
    echo "PASS: bash -n update-database.sh"
else
    echo "FAIL: bash -n update-database.sh"
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "FAIL: jq is required by these timing tests"
    exit 1
fi

# ---------------------------------------------------------------------------
# Shared mock harness
# ---------------------------------------------------------------------------
# The mock cargo knows how to append a JSONL "collector" event to
# $COMPASS_TIMING_FILE when the variable is set, mimicking the future Rust
# collector subprocess.  It silently ignores write failures (the shell layer is
# what must warn), so a bad timing path never contaminates stderr and cannot
# produce a false PASS on the "shell timing warning" assertions.
#
# Env knobs:
#   FAKE_LOG                 invocation log for cargo/dolt/sync
#   FAKE_STATUS_SEQ          dolt status blocks separated by "==="
#   FAKE_STATUS_IDX          current dolt status block index
#   FAKE_CARGO_FAIL_CALL     Nth cargo invocation to fail
#   FAKE_COLLECTOR_EVENT     JSONL line to append (default valid collector line)
setup_fakes() {
    local t="$1"
    mkdir -p "$t/bin" "$t/repos/investment_data/.dolt" "$t/repos/compass_data/.dolt"

    cat > "$t/bin/dolt" <<'EOF'
#!/bin/bash
# Mock dolt: logs argv, emulates the subcommands update-database.sh uses.
echo "dolt $*" >> "${FAKE_LOG:?fake log unset}"
if [ "${1:-}" = "--data-dir" ]; then shift 2; fi
case "${1:-}" in
    creds)
        if [ -n "${FAKE_DOLT_CREDS_FAIL:-}" ]; then exit 1; fi
        exit 0
        ;;
    status)
        idx=$(cat "$FAKE_STATUS_IDX" 2>/dev/null || echo 0)
        awk -v n="$idx" 'BEGIN{RS="===\n"} NR==n+1{print}' "$FAKE_STATUS_SEQ" 2>/dev/null || true
        echo $((idx + 1)) > "$FAKE_STATUS_IDX"
        exit 0
        ;;
    sql)
        printf 'd\n2026-07-31\n'
        exit 0
        ;;
    *) exit 0 ;;
esac
EOF

    cat > "$t/bin/cargo" <<'EOF'
#!/bin/bash
# Mock cargo: logs argv; optionally fails on the Nth cargo invocation.
# Only compass-collectors sync performs real Rust timing reporting; mock that
# command by appending a fake collector JSONL event when COMPASS_TIMING_FILE is
# set.  Write failures are silent: they simulate an unreliable Rust timing sink
# and must not be confused with the shell warning.
echo "cargo $*" >> "${FAKE_LOG:?fake log unset}"
if [ -n "${COMPASS_TIMING_FILE:-}" ] && [[ "$*" == *"compass-collectors"*"-- sync"* ]]; then
    DEFAULT_COLLECTOR_EVENT='{"kind":"collector","source":"stock_basic","phase":"fetch","status":"success","duration_ms":42}'
    event="${FAKE_COLLECTOR_EVENT:-$DEFAULT_COLLECTOR_EVENT}"
    printf '%s\n' "$event" >> "$COMPASS_TIMING_FILE" 2>/dev/null || true
fi
if [ -n "${FAKE_CARGO_FAIL_CALL:-}" ]; then
    n=$(grep -c '^cargo ' "$FAKE_LOG" || true)
    if [ "$n" -eq "$FAKE_CARGO_FAIL_CALL" ]; then
        echo "mock cargo: failing invocation $n ($*)" >&2
        exit 1
    fi
fi
exit 0
EOF

    chmod +x "$t/bin/dolt" "$t/bin/cargo"

    cat > "$t/sync-fake.sh" <<'EOF'
#!/bin/bash
echo "sync-investment $*" >> "${FAKE_LOG:?fake log unset}"
exit 0
EOF
    chmod +x "$t/sync-fake.sh"
}

# Run update-database.sh against the mocked environment in $1; capture exit.
# Usage: run_script <tmpdir> [extra env assignments...]
run_script() {
    local t="$1"
    shift
    : > "$t/calls.log"
    echo 0 > "$t/status.idx"
    local envs=("$@")
    set +e
    env \
        PATH="$t/bin:$PATH" \
        FAKE_LOG="$t/calls.log" \
        FAKE_STATUS_IDX="$t/status.idx" \
        FAKE_STATUS_SEQ="$t/status.seq" \
        SEPA_INVESTMENT_DATA_DIR="$t/repos/investment_data" \
        SEPA_COMPASS_DATA_DIR="$t/repos/compass_data" \
        SYNC_INVESTMENT_SCRIPT="$t/sync-fake.sh" \
        TMPDIR="$t" \
        "${envs[@]}" \
        bash "$SEPA_SCRIPT" > "$t/out.log" 2> "$t/err.log"
    local code=$?
    set -e
    echo "$code" > "$t/exit.code"
}

assert_true() {
    local name="$1" cmd="$2"
    if eval "$cmd"; then
        echo "PASS: $name"
    else
        echo "FAIL: $name"
        FAIL=1
    fi
}

assert_false() {
    local name="$1" cmd="$2"
    if eval "$cmd"; then
        echo "FAIL: $name"
        FAIL=1
    else
        echo "PASS: $name"
    fi
}

assert_file_exists() {
    local name="$1" file="$2"
    if [ -f "$file" ]; then
        echo "PASS: $name ($file)"
    else
        echo "FAIL: $name (missing: $file)"
        FAIL=1
    fi
}

# ---------------------------------------------------------------------------
# Shared temp dirs
# ---------------------------------------------------------------------------
T_HAPPY="$TMP_ROOT/happy"
mkdir -p "$T_HAPPY"
setup_fakes "$T_HAPPY"
cat > "$T_HAPPY/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF

# ---------------------------------------------------------------------------
# Control: no timing intent preserves original success behavior.
# This is both a sanity check that the harness itself works and the
# "timing is always-on but must not disturb the main flow" regression guard
# (dimension 4): no unexpected timing warnings, no leftover temp JSONL.
# ---------------------------------------------------------------------------
echo ""
echo "--- control / 4. timing default: unchanged success behavior + temp cleanup ---"
T4="$TMP_ROOT/t4"
mkdir -p "$T4"
setup_fakes "$T4"
cat > "$T4/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T4" SYNC_TIMING_DIR="$T4/timings"
assert_true "default timing: exit 0 on happy path" 'test "$(cat "$T4/exit.code")" = 0'
assert_true "default timing: no unexpected timing warning on stderr" \
    '! grep -qiE "timing|timings|timing file|sync-timing" "$T4/err.log"'
assert_true "default timing: final JSON generated under SYNC_TIMING_DIR" \
    'test -n "$(find "$T4/timings" -maxdepth 1 -name "*.json" -print -quit 2>/dev/null)"'
assert_true "default timing: no leftover temp JSONL in run dir" \
    '! find "$T4" -maxdepth 2 -name "*.jsonl" -print -quit 2>/dev/null | grep -q .'
assert_true "default timing: pipeline still reaches Done" 'grep -q "Done." "$T4/out.log"'

# ---------------------------------------------------------------------------
# 1a. Timing must not block: COMPASS_TIMING_FILE parent directory missing
# ---------------------------------------------------------------------------
echo ""
echo "--- 1a. COMPASS_TIMING_FILE in nonexistent directory: main flow exit 0 + warning ---"
T1A="$TMP_ROOT/t1a"
mkdir -p "$T1A"
setup_fakes "$T1A"
cat > "$T1A/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T1A" COMPASS_TIMING_FILE="$T1A/no-such-dir/timing.jsonl"
assert_true "1a: all real steps succeed => exit 0" 'test "$(cat "$T1A/exit.code")" = 0'
assert_true "1a: stderr contains a visible timing warning" \
    'grep -qiE "warning" "$T1A/err.log" && grep -qiE "timing|timings|timing file|sync-timing" "$T1A/err.log"'
assert_true "1a: pipeline still reaches Done" 'grep -q "Done." "$T1A/out.log"'

# ---------------------------------------------------------------------------
# 1b. Timing must not block: COMPASS_TIMING_FILE exists but is unwritable
# ---------------------------------------------------------------------------
echo ""
echo "--- 1b. COMPASS_TIMING_FILE unwritable: main flow exit 0 + warning ---"
T1B="$TMP_ROOT/t1b"
mkdir -p "$T1B"
setup_fakes "$T1B"
cat > "$T1B/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
: > "$T1B/timing.jsonl"
chmod 444 "$T1B/timing.jsonl"
run_script "$T1B" COMPASS_TIMING_FILE="$T1B/timing.jsonl"
chmod 644 "$T1B/timing.jsonl" || true
assert_true "1b: all real steps succeed => exit 0" 'test "$(cat "$T1B/exit.code")" = 0'
assert_true "1b: stderr contains a visible timing warning" \
    'grep -qiE "warning" "$T1B/err.log" && grep -qiE "timing|timings|timing file|sync-timing" "$T1B/err.log"'
assert_true "1b: pipeline still reaches Done" 'grep -q "Done." "$T1B/out.log"'

# ---------------------------------------------------------------------------
# 1c. Timing must not block: SYNC_TIMING_DIR unwritable
# ---------------------------------------------------------------------------
echo ""
echo "--- 1c. SYNC_TIMING_DIR unwritable: main flow exit 0 + warning ---"
T1C="$TMP_ROOT/t1c"
mkdir -p "$T1C"
setup_fakes "$T1C"
cat > "$T1C/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
mkdir -p "$T1C/readonly"
chmod 555 "$T1C/readonly"
# Verify the readonly fixture actually works under this user; if not (e.g.
# root), skip the write-failure-specific assertion but still run the suite.
if touch "$T1C/readonly/probe" >/dev/null 2>&1; then
    skip "1c: readonly dir not effective under current user; skipping write-failure assertion"
    rm -f "$T1C/readonly/probe" >/dev/null 2>&1 || true
else
    run_script "$T1C" SYNC_TIMING_DIR="$T1C/readonly" COMPASS_TIMING_FILE="$T1C/timing.jsonl"
    assert_true "1c: all real steps succeed => exit 0" 'test "$(cat "$T1C/exit.code")" = 0'
    assert_true "1c: stderr contains a visible timing warning" \
        'grep -qiE "warning" "$T1C/err.log" && grep -qiE "timing|timings|timing file|sync-timing" "$T1C/err.log"'
    assert_true "1c: pipeline still reaches Done" 'grep -q "Done." "$T1C/out.log"'
    # Protect the main-flow semantics outside the readonly case too.
    chmod 755 "$T1C/readonly" || true
fi

# ---------------------------------------------------------------------------
# 2. Bad JSONL from the Rust child must not break the main flow
# ---------------------------------------------------------------------------
echo ""
echo "--- 2. bad JSONL (mock Rust child) => merge failure is only a warning, exit 0 ---"
T2="$TMP_ROOT/t2"
mkdir -p "$T2"
setup_fakes "$T2"
cat > "$T2/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
: > "$T2/timing.jsonl"
run_script "$T2" \
    COMPASS_TIMING_FILE="$T2/timing.jsonl" \
    SYNC_TIMING_DIR="$T2/timings" \
    FAKE_COLLECTOR_EVENT='this is { definitely not valid json'
assert_true "2: main flow still exits 0 despite bad JSONL" 'test "$(cat "$T2/exit.code")" = 0'
assert_true "2: bad JSONL triggers a visible timing/merge warning on stderr" \
    'grep -qiE "warning" "$T2/err.log" && grep -qiE "timing|timings|timing file|sync-timing|jsonl|merge" "$T2/err.log"'
assert_true "2: pipeline still reaches Done" 'grep -q "Done." "$T2/out.log"'
# The mock wrote the bad line into the JSONL; the shell must not treat the
# malformed child event as a hard failure.
assert_true "2: the bad JSONL child event was actually written (test is real)" \
    'grep -q "this is { definitely not valid json" "$T2/timing.jsonl"'

# ---------------------------------------------------------------------------
# 3. Failed cargo step records status:"failed" without swallowing the error
# ---------------------------------------------------------------------------
echo ""
echo "--- 3. failed step records status failed; script still exits non-zero ---"
T3="$TMP_ROOT/t3"
mkdir -p "$T3"
setup_fakes "$T3"
cat > "$T3/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
: > "$T3/timing.jsonl"
# cargo call #3 is exactly `compass-collectors -- sync` (happy-path call order:
# 1=import, 2=check-stock-daily, 3=sync).
run_script "$T3" \
    COMPASS_TIMING_FILE="$T3/timing.jsonl" \
    SYNC_TIMING_DIR="$T3/timings" \
    FAKE_CARGO_FAIL_CALL=3 \
    FAKE_COLLECTOR_EVENT='{"kind":"collector","source":"stock_basic","phase":"fetch","status":"failed","duration_ms":7}'
assert_true "3: script still exits non-zero on cargo failure" 'test "$(cat "$T3/exit.code")" != 0'
assert_true "3: original 'step 2 failed' error remains visible" 'grep -q "step 2 failed" "$T3/err.log"'
assert_true "3: failed step event recorded in JSONL as status failed" \
    'grep -q "\"kind\":\"step\"" "$T3/timing.jsonl" && grep -q "\"status\":\"failed\"" "$T3/timing.jsonl"'
assert_false "3: no later step runs after the failed sync" \
    'grep -q "sepa temperature" "$T3/calls.log"'

# ---------------------------------------------------------------------------
# 5. run_id uniqueness: two same-second runs must not overwrite each other
# ---------------------------------------------------------------------------
echo ""
echo "--- 5. run_id uniqueness: two same-second runs keep both JSON files ---"
T5="$TMP_ROOT/t5"
mkdir -p "$T5"
setup_fakes "$T5"
cat > "$T5/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
: > "$T5/timing.jsonl"
run_script "$T5" COMPASS_TIMING_FILE="$T5/timing.jsonl" SYNC_TIMING_DIR="$T5/timings"
# Second run, immediately, same second (different PID).
: > "$T5/timing2.jsonl"
run_script "$T5" COMPASS_TIMING_FILE="$T5/timing2.jsonl" SYNC_TIMING_DIR="$T5/timings"
assert_true "5: two distinct final JSON files exist after same-second runs" \
    'test "$(find "$T5/timings" -maxdepth 1 -name "*.json" 2>/dev/null | wc -l)" -ge 2'
if [ -d "$T5/timings" ] && [ "$(find "$T5/timings" -maxdepth 1 -name '*.json' 2>/dev/null | wc -l)" -ge 2 ]; then
    files=$(find "$T5/timings" -maxdepth 1 -name '*.json' 2>/dev/null | sort)
    assert_true "5: both final JSON filenames differ (no overwrite)" \
        'test "$(printf "%s\n" $files | sort -u | wc -l)" -ge 2'
fi
# Static contract: run_id must include a process-id source to disambiguate
# same-second runs.  Accept the plan's direct `$$` or common alternatives
# ($BASHPID / $PPID / an explicit pid variable).
assert_true "5: update-database.sh builds run_id with PID (static contract)" \
    'grep -qE "(run_id.*\$\$|RUN_ID.*\$\$|run_id.*BASHPID|run_id.*PPID|run_id.*pid|run_id.*PID)" "$SEPA_SCRIPT"'

# ---------------------------------------------------------------------------
# 6. Shell pollution/injection: special chars in events still valid JSON
# ---------------------------------------------------------------------------
echo ""
echo "--- 6. special characters in step/source/status remain valid JSON ---"
T6="$TMP_ROOT/t6"
mkdir -p "$T6"
setup_fakes "$T6"
cat > "$T6/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
: > "$T6/timing.jsonl"
# A single-quoted heredoc-free line keeps the raw special characters for the
# child to write.  The string contains double quotes (escaped for JSON),
# backslashes, spaces, $HOME, backticks and semicolons.
EVENT='{"kind":"collector","source":"stock_basic \"quoted\" $HOME `cmd`; A&B","phase":"fetch","status":"success","duration_ms":1}'
run_script "$T6" \
    COMPASS_TIMING_FILE="$T6/timing.jsonl" \
    SYNC_TIMING_DIR="$T6/timings" \
    FAKE_COLLECTOR_EVENT="$EVENT"
if [ -f "$T6/timings"/*.json ]; then
    final_json=$(find "$T6/timings" -maxdepth 1 -name '*.json' | head -n1)
    assert_true "6: final JSON parses with jq" 'jq -e . "$final_json" >/dev/null 2>&1'
    assert_true "6: special-char source survives in final JSON" \
        'jq -e ".collectors[] | select(.source == \"stock_basic \\\"quoted\\\" \$HOME \`cmd\`; A&B\")" "$final_json" >/dev/null 2>&1'
else
    fail "6: no final JSON generated; special-character event not merged"
fi

# ---------------------------------------------------------------------------
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL ADVERSARIAL TIMING SHELL TESTS PASSED"
else
    echo "SOME ADVERSARIAL TIMING SHELL TESTS FAILED (RED expected on un-implemented timing)"
    exit 1
fi
