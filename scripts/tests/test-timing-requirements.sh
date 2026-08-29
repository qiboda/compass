#!/bin/bash
# Requirement acceptance RED tests for issue #334 (db-sync timing stats).
#
# Scope: shell-level acceptance assertions against `scripts/update-database.sh`.
# These tests are deliberately written BEFORE the timing implementation exists;
# they must FAIL (RED) on the current production script because the script does
# not yet generate timing JSON / print a timing summary / record failures.
#
# The 6 requirement acceptance points from the approved plan:
#   1. `update-database.sh` writes a final JSON file at
#      `$SYNC_TIMING_DIR/YYYY-MM-DD-<run_id>.json` (default dir =
#      `$PROJECT_ROOT/logs/sync-timings`).
#   2. Final JSON contains run metadata (id/date/started_at/finished_at/
#      total_ms/status), `steps`, and `summary`.
#   3. A mocked `compass-collectors sync` that appends one collector JSONL to
#      `$COMPASS_TIMING_FILE` is reflected in the final JSON `collectors` array.
#   4. The console prints a human-readable timing summary (e.g. contains
#      "total" or "Timing").
#   5. Robustness: non-writable SYNC_TIMING_DIR / non-writable
#      COMPASS_TIMING_FILE still leaves the main flow exit 0 with a warning on
#      stderr.
#   6. Failure steps are recorded: a mocked failing cargo call leaves the
#      corresponding step `status:"failed"` in the final JSON while the script
#      still returns non-zero (original hard-fail behaviour preserved).
#
# The suite uses mocked cargo/dolt/date/sync-investment on PATH and temp dirs;
# it never touches real Dolt repos or network.
#
# Run: bash scripts/tests/test-timing-requirements.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SEPA_SCRIPT="$PROJECT_ROOT/scripts/update-database.sh"

FAIL=0
TMP_ROOT="$(mktemp -d)"
DEFAULT_TIMING_DIR="$PROJECT_ROOT/logs/sync-timings"
# Clean any timing JSON that this suite may generate under the project root
# (only files matching the run-id naming pattern are removed, to be safe).
cleanup() {
    rm -rf "$TMP_ROOT"
    if [ -d "$DEFAULT_TIMING_DIR" ]; then
        find "$DEFAULT_TIMING_DIR" -maxdepth 1 -type f \
            -name '*[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9][0-9][0-9]-[0-9]*.json' \
            -delete 2>/dev/null || true
    fi
}
trap cleanup EXIT

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAIL=1; }

# Assert helper used throughout the suite.
assert_true() {
    local name="$1" cmd="$2"
    if eval "$cmd"; then
        pass "$name"
    else
        fail "$name"
    fi
}

# --- 0. Syntax gate (script must exist; assertions must fail for logic, not "file missing") ---
echo "--- 0. syntax/precondition gate ---"
if [ ! -f "$SEPA_SCRIPT" ]; then
    echo "FAIL: production script scripts/update-database.sh missing"
    exit 1
fi
if bash -n "$SEPA_SCRIPT" 2>&1; then
    echo "PASS: bash -n update-database.sh"
else
    echo "FAIL: bash -n update-database.sh"
    exit 1
fi

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAIL=1; }

# ---------------------------------------------------------------------------
# Mock harness
# ---------------------------------------------------------------------------
# `setup_fakes <tmpdir>` creates:
#   - mocked `cargo`: logs argv; if FAKE_CARGO_FAIL_CALL is set, fails the Nth
#     cargo invocation; when the invocation is `compass-collectors -- sync`,
#     it appends a collector JSONL event to $COMPASS_TIMING_FILE.
#   - mocked `dolt`: logs argv; emulates the subcommands the pipeline uses.
#   - fake `sync-investment-data.sh` via SYNC_INVESTMENT_SCRIPT.
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
# Mock cargo: logs argv.  Optionally fails on the Nth cargo invocation
# (FAKE_CARGO_FAIL_CALL).  When the invocation is compass-collectors sync, it
# appends one fake collector JSONL event to $COMPASS_TIMING_FILE, mirroring
# the plan's Rust collector timing contract.
echo "cargo $*" >> "${FAKE_LOG:?fake log unset}"
if [ -n "${FAKE_CARGO_FAIL_CALL:-}" ]; then
    n=$(grep -c '^cargo ' "$FAKE_LOG" || true)
    if [ "$n" -eq "$FAKE_CARGO_FAIL_CALL" ]; then
        echo "mock cargo: failing invocation $n ($*)" >&2
        exit 1
    fi
fi
# Plan contract: when running compass-collectors sync and COMPASS_TIMING_FILE is
# set, the Rust collector appends one structured collector JSONL event.
if [[ "$*" == *"compass-collectors"*"-- sync"* ]] && [ -n "${COMPASS_TIMING_FILE:-}" ]; then
    printf '%s\n' '{"kind":"collector","source":"stock_basic","phase":"fetch","status":"success","duration_ms":1}' >> "$COMPASS_TIMING_FILE"
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

# Run update-database.sh against the mocked environment in $1, with optional
# extra env assignments.  Captures exit code and stdout/stderr files.
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

# Return the single final JSON path under a given SYNC_TIMING_DIR, matching
# YYYY-MM-DD-<run_id>.json at depth 1.
find_final_json() {
    local dir="$1"
    find "$dir" -maxdepth 1 -type f -name '*.json' 2>/dev/null | head -n1 || true
}

# Assert final JSON exists and return its path via stdout (caller captures).
assert_json_exists() {
    local name="$1" dir="$2"
    local json
    json=$(find_final_json "$dir")
    if [ -z "$json" ]; then
        fail "$name (no JSON file under $dir)" >&2
        echo ""
    elif [ ! -f "$json" ]; then
        fail "$name (path is not a regular file: $json)" >&2
        echo ""
    else
        pass "$name ($json)" >&2
        echo "$json"
    fi
}

# ---------------------------------------------------------------------------
# 1. Happy path + default SYNC_TIMING_DIR:
#    update-database.sh runs, generates final JSON under the default
#    $PROJECT_ROOT/logs/sync-timings, filename YYYY-MM-DD-<run_id>.json.
# ---------------------------------------------------------------------------
echo ""
echo "--- 1. final JSON generated at default SYNC_TIMING_DIR ---"
T1="$TMP_ROOT/t1"
mkdir -p "$T1"
setup_fakes "$T1"
cat > "$T1/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T1"
# The happy path must exit 0.
assert_true "exit 0 on happy path" 'test "$(cat "$T1/exit.code")" = 0'
# Default SYNC_TIMING_DIR contract must be the project-root logs/sync-timings.
DEFAULT_JSON=$(assert_json_exists "default timing JSON under $DEFAULT_TIMING_DIR" "$DEFAULT_TIMING_DIR")
if [ -n "$DEFAULT_JSON" ]; then
    assert_true "final JSON filename matches YYYY-MM-DD-<run_id>.json" \
        'basename "$DEFAULT_JSON" | grep -Eq "^[0-9]{4}-[0-9]{2}-[0-9]{2}-[0-9]{8}-[0-9]{6}-[0-9]+\.json$"'
fi
# Also assert env-override path works (second part of requirement 1).
echo "--- 1b. SYNC_TIMING_DIR env override honored ---"
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
RUN_SCRIPT="$T1B"
run_script "$RUN_SCRIPT" SYNC_TIMING_DIR="$T1B/timings"
OVERRIDE_JSON=$(assert_json_exists "env-override timing JSON under $T1B/timings" "$T1B/timings")
if [ -n "$OVERRIDE_JSON" ]; then
    assert_true "env-override JSON path under SYNC_TIMING_DIR" \
        'case "$OVERRIDE_JSON" in "$T1B/timings/"*) true;; *) false;; esac'
fi

# ---------------------------------------------------------------------------
# 2. Final JSON schema: run metadata + steps + summary.
# ---------------------------------------------------------------------------
echo ""
echo "--- 2. final JSON contains run/steps/summary ---"
# Reuse T1B override path if it exists, otherwise fall back to T1 default.
SCHEMA_JSON=""
if [ -n "$OVERRIDE_JSON" ] && [ -f "$OVERRIDE_JSON" ]; then
    SCHEMA_JSON="$OVERRIDE_JSON"
elif [ -n "$DEFAULT_JSON" ] && [ -f "$DEFAULT_JSON" ]; then
    SCHEMA_JSON="$DEFAULT_JSON"
fi
if [ -z "$SCHEMA_JSON" ]; then
    fail "cannot validate schema: no final JSON generated"
else
    assert_true "run object present" 'jq -e ".run" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "run.id present" 'jq -e ".run.id | type == \"string\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "run.date present" 'jq -e ".run.date | type == \"string\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "run.started_at present" 'jq -e ".run.started_at | type == \"string\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "run.finished_at present" 'jq -e ".run.finished_at | type == \"string\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "run.total_ms present" 'jq -e ".run.total_ms | type == \"number\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "run.status present" 'jq -e ".run.status | type == \"string\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "steps array present" 'jq -e ".steps | type == \"array\"" "$SCHEMA_JSON" >/dev/null 2>&1'
    assert_true "summary object present" 'jq -e ".summary | type == \"object\"" "$SCHEMA_JSON" >/dev/null 2>&1'
fi

# ---------------------------------------------------------------------------
# 3. Mocked compass-collectors sync appends collector JSONL to
#    $COMPASS_TIMING_FILE; final JSON collectors array contains that event.
# ---------------------------------------------------------------------------
echo ""
echo "--- 3. collector JSONL from COMPASS_TIMING_FILE folded into final JSON ---"
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
run_script "$T3" SYNC_TIMING_DIR="$T3/timings" COMPASS_TIMING_FILE="$T3/collector.jsonl"
COLLECTOR_JSON=$(assert_json_exists "collector test final JSON" "$T3/timings")
COLLECTOR_JSONL="$T3/collector.jsonl"
if [ -f "$COLLECTOR_JSONL" ]; then
    assert_true "collector JSONL was written to COMPASS_TIMING_FILE" \
        'grep -q "stock_basic" "$COLLECTOR_JSONL"'
else
    fail "collector JSONL not written to $COMPASS_TIMING_FILE"
fi
if [ -z "$COLLECTOR_JSON" ]; then
    fail "collector test final JSON missing; cannot validate collectors array"
else
    assert_true "collectors array present in final JSON" \
        'jq -e ".collectors | type == \"array\"" "$COLLECTOR_JSON" >/dev/null 2>&1'
    assert_true "collector event folded into final JSON" \
        'jq -e "[.collectors[] | select(.source == \"stock_basic\" and .phase == \"fetch\" and .status == \"success\" and .duration_ms == 1)] | length == 1" "$COLLECTOR_JSON" >/dev/null 2>&1'
fi

# ---------------------------------------------------------------------------
# 4. Console human-readable timing summary (e.g. "total" or "Timing").
# ---------------------------------------------------------------------------
echo ""
echo "--- 4. console timing summary ---"
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
assert_true "console summary contains 'total' or 'Timing'" \
    'grep -Eiq "(total|timing)" "$T4/out.log"'

# ---------------------------------------------------------------------------
# 5. Robustness: non-writable SYNC_TIMING_DIR and non-writable
#    COMPASS_TIMING_FILE must not break the main flow; stderr must warn.
# ---------------------------------------------------------------------------
echo ""
echo "--- 5a. non-writable SYNC_TIMING_DIR still exit 0 + warning ---"
T5A="$TMP_ROOT/t5a"
mkdir -p "$T5A"
setup_fakes "$T5A"
cat > "$T5A/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
# Two non-writable variants: a chmod-555 dir (works for non-root) and a
# structurally impossible path under /proc (works even when running as root).
# Both must leave the main pipeline exit 0 and print a timing warning.
for T5A_DIR in "$T5A/unwritable-timings" "/proc/1/nonexistent-timings"; do
    if [ "$T5A_DIR" = "$T5A/unwritable-timings" ]; then
        mkdir -p "$T5A_DIR"
        chmod 555 "$T5A_DIR"
    fi
    run_script "$T5A" SYNC_TIMING_DIR="$T5A_DIR"
    assert_true "exit 0 despite non-writable SYNC_TIMING_DIR ($T5A_DIR)" \
        'test "$(cat "$T5A/exit.code")" = 0'
    assert_true "stderr warning names timing for non-writable SYNC_TIMING_DIR ($T5A_DIR)" \
        'grep -Eiq "timing" "$T5A/err.log" && grep -Eiq "(warning|fail|error|cannot|unable|denied)" "$T5A/err.log"'
done

echo ""
echo "--- 5b. non-writable COMPASS_TIMING_FILE still exit 0 + warning ---"
T5B="$TMP_ROOT/t5b"
mkdir -p "$T5B"
setup_fakes "$T5B"
cat > "$T5B/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
# Point COMPASS_TIMING_FILE at a path inside a non-writable directory (chmod
# 555 for non-root; /proc path as a structurally non-writable fallback).
for T5B_FILE in "$T5B/ro-dir/collector.jsonl" "/proc/1/nonexistent/collector.jsonl"; do
    if [ "$T5B_FILE" = "$T5B/ro-dir/collector.jsonl" ]; then
        mkdir -p "$T5B/ro-dir"
        chmod 555 "$T5B/ro-dir"
    fi
    run_script "$T5B" SYNC_TIMING_DIR="$T5B/timings" COMPASS_TIMING_FILE="$T5B_FILE"
    assert_true "exit 0 despite non-writable COMPASS_TIMING_FILE ($T5B_FILE)" \
        'test "$(cat "$T5B/exit.code")" = 0'
    # The warning must come from the timing layer, not merely the mock's own
    # file-redirect error (which would include the literal file path but not a
    # timing/warning framing).  Require "timing" plus a warning/error framing.
    assert_true "stderr warns on collector timing write failure ($T5B_FILE)" \
        'grep -Eiq "timing" "$T5B/err.log" && grep -Eiq "(warning|fail|error|cannot|unable|denied)" "$T5B/err.log"'
done

# ---------------------------------------------------------------------------
# 6. Failure steps recorded: a mocked failing cargo call leaves the
#    corresponding step status:"failed" in the final JSON while the script
#    still returns non-zero.
# ---------------------------------------------------------------------------
echo ""
echo "--- 6. failed step recorded as failed + non-zero exit ---"
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
# Make the third cargo invocation fail: call 1 = import, call 2 = check-stock-daily,
# call 3 = compass-collectors sync.  The sync mock will try to append to
# COMPASS_TIMING_FILE, but the cargo script fails *before* returning 0, so if the
# implementation is correct the shell must still record a failed step event.
run_script "$T6" SYNC_TIMING_DIR="$T6/timings" COMPASS_TIMING_FILE="$T6/collector.jsonl" FAKE_CARGO_FAIL_CALL=3
FAIL_JSON=$(assert_json_exists "failed-run final JSON" "$T6/timings")
assert_true "non-zero exit on failed pipeline step" \
    'test "$(cat "$T6/exit.code")" != 0'
if [ -z "$FAIL_JSON" ]; then
    fail "cannot validate failed step: no final JSON under $T6/timings"
else
    assert_true "run.status is failed" \
        'jq -e ".run.status == \"failed\"" "$FAIL_JSON" >/dev/null 2>&1'
    assert_true "at least one step has status failed" \
        'jq -e "[.steps[] | select(.status == \"failed\")] | length >= 1" "$FAIL_JSON" >/dev/null 2>&1'
    assert_true "the failed step is the compass-collectors sync (step 2)" \
        'jq -e "[.steps[] | select(.status == \"failed\" and .step == 2)] | length >= 1" "$FAIL_JSON" >/dev/null 2>&1'
fi

# ---------------------------------------------------------------------------
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TIMING REQUIREMENT TESTS PASSED"
else
    echo "SOME TIMING REQUIREMENT TESTS FAILED"
    exit 1
fi
