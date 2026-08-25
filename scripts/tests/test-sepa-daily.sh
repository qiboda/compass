#!/bin/bash
# Tests for scripts/sepa_daily.sh (epic #139, issue #151).
# Mirrors the pre-push-ref-regex-test.sh precedent: a `bash -n` syntax gate plus
# behavioral assertions against mocked cargo/uv/dolt in a temp dir — no real
# network access, no real Dolt mutation, no data repos touched.
# Run: scripts/tests/test-sepa-daily.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SEPA_SCRIPT="$PROJECT_ROOT/scripts/sepa_daily.sh"

FAIL=0
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT

# --- 0. Syntax gate ---
echo "--- 0. syntax ---"
if bash -n "$SEPA_SCRIPT" 2>&1; then
    echo "PASS: bash -n sepa_daily.sh"
else
    echo "FAIL: bash -n sepa_daily.sh"
    exit 1
fi

# --- Harness: build a temp dir with mocked cargo/uv/dolt ---
# The mocks log every invocation to $FAKE_LOG; dolt status pops one block per
# call from $FAKE_STATUS_SEQ (blocks separated by "===" lines).
setup_fakes() {
    local t="$1"
    mkdir -p "$t/bin" "$t/repos/investment_data/.dolt" "$t/repos/compass_data/.dolt"

    cat > "$t/bin/dolt" <<'EOF'
#!/bin/bash
# Mock dolt: logs argv, emulates the subcommands sepa_daily.sh uses.
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
        if [ -n "${FAKE_DOLT_SQL_NULL:-}" ]; then
            printf 'd\nNULL\n'
        else
            printf 'd\n2026-07-31\n'
        fi
        exit 0
        ;;
    *) exit 0 ;;
esac
EOF

    cat > "$t/bin/cargo" <<'EOF'
#!/bin/bash
# Mock cargo: logs argv; optionally fails on the Nth cargo invocation.
echo "cargo $*" >> "${FAKE_LOG:?fake log unset}"
if [ -n "${FAKE_CARGO_FAIL_CALL:-}" ]; then
    n=$(grep -c '^cargo ' "$FAKE_LOG" || true)
    if [ "$n" -eq "$FAKE_CARGO_FAIL_CALL" ]; then
        echo "mock cargo: failing invocation $n ($*)" >&2
        exit 1
    fi
fi
exit 0
EOF

    cat > "$t/bin/uv" <<'EOF'
#!/bin/bash
# Mock uv: logs argv; optionally fails on the Nth uv invocation.
echo "uv $*" >> "${FAKE_LOG:?fake log unset}"
if [ -n "${FAKE_UV_FAIL_CALL:-}" ]; then
    n=$(grep -c '^uv ' "$FAKE_LOG" || true)
    if [ "$n" -eq "$FAKE_UV_FAIL_CALL" ]; then
        echo "mock uv: failing invocation $n ($*)" >&2
        exit 1
    fi
fi
exit 0
EOF

    chmod +x "$t/bin/dolt" "$t/bin/cargo" "$t/bin/uv"
}

# Run sepa_daily.sh against the mocked environment in $1; capture exit code.
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
        TMPDIR="$t" \
        "${envs[@]}" \
        bash "$SEPA_SCRIPT" > "$t/out.log" 2> "$t/err.log"
    local code=$?
    set -e
    echo "$code" > "$t/exit.code"
}

# Assert helpers — each prints PASS/FAIL and sets FAIL=1 on mismatch.
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

# Assert that line $a of a file comes before line $b (by line number).
assert_order() {
    local name="$1" file="$2" pat_a="$3" pat_b="$4"
    local la lb
    la=$(grep -n "$pat_a" "$file" 2>/dev/null | head -n1 | cut -d: -f1 || true)
    lb=$(grep -n "$pat_b" "$file" 2>/dev/null | head -n1 | cut -d: -f1 || true)
    if [ -n "$la" ] && [ -n "$lb" ] && [ "$la" -lt "$lb" ]; then
        echo "PASS: $name"
    else
        echo "FAIL: $name (line_a=$la line_b=$lb)"
        FAIL=1
    fi
}

# ---------------------------------------------------------------------------
# 1. Happy path: clean Dolt both times → no commit/push at all, full call chain
# ---------------------------------------------------------------------------
echo ""
echo "--- 1. happy path (no Dolt changes → commits skipped) ---"
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
assert_true "exit 0 on happy path" 'test "$(cat "$T1/exit.code")" = 0'
assert_true "step 1: import market data" \
    'grep -qx "cargo run --bin compass-data -- import" "$T1/calls.log"'
assert_true "step 2: fetch AND import 5 collector sources, one call each" \
    'grep -qx "uv run python main.py fetch main_flow" "$T1/calls.log" &&
     grep -qx "uv run python main.py fetch dragon" "$T1/calls.log" &&
     grep -qx "uv run python main.py fetch block_trade" "$T1/calls.log" &&
     grep -qx "uv run python main.py fetch institution_survey" "$T1/calls.log" &&
     grep -qx "uv run python main.py fetch index_daily" "$T1/calls.log" &&
     grep -qx "uv run python main.py import main_flow" "$T1/calls.log" &&
     grep -qx "uv run python main.py import dragon" "$T1/calls.log" &&
     grep -qx "uv run python main.py import block_trade" "$T1/calls.log" &&
     grep -qx "uv run python main.py import institution_survey" "$T1/calls.log" &&
     grep -qx "uv run python main.py import index_daily" "$T1/calls.log"'

assert_order "step 2: fetch/import are paired for main_flow" "$T1/calls.log" \
    "uv run python main.py fetch main_flow" "uv run python main.py import main_flow"
assert_order "step 2: fetch/import are paired for dragon" "$T1/calls.log" \
    "uv run python main.py fetch dragon" "uv run python main.py import dragon"
assert_order "step 2: fetch/import are paired for block_trade" "$T1/calls.log" \
    "uv run python main.py fetch block_trade" "uv run python main.py import block_trade"
assert_order "step 2: fetch/import are paired for institution_survey" "$T1/calls.log" \
    "uv run python main.py fetch institution_survey" "uv run python main.py import institution_survey"
assert_order "step 2: fetch/import are paired for index_daily" "$T1/calls.log" \
    "uv run python main.py fetch index_daily" "uv run python main.py import index_daily"
assert_order "step 2: index_daily pair follows institution_survey pair" "$T1/calls.log" \
    "uv run python main.py import institution_survey" "uv run python main.py fetch index_daily"

assert_true "step 4: 5 append tables with since anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table block_trade --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_daily --since 2026-07-31" "$T1/calls.log"'

assert_order "step 4: append imports in allowlist order (index_daily last)" "$T1/calls.log" \
    "import-compass --table capital_main_flow" "import-compass --table dragon_list"
assert_order "step 4: append imports in allowlist order (index_daily last)" "$T1/calls.log" \
    "import-compass --table dragon_list" "import-compass --table block_trade"
assert_order "step 4: append imports in allowlist order (index_daily last)" "$T1/calls.log" \
    "import-compass --table block_trade" "import-compass --table institution_survey"
assert_order "step 4: append imports in allowlist order (index_daily last)" "$T1/calls.log" \
    "import-compass --table institution_survey" "import-compass --table index_daily"

assert_true "step 5: temperature before score" \
    'grep -qx "cargo run --bin compass-data -- sepa temperature" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- sepa score --top 50" "$T1/calls.log"'
assert_order "step 5: temperature runs before score" "$T1/calls.log" \
    "sepa temperature" "sepa score --top 50"
assert_true "no dolt commit/push when nothing changed" \
    '! grep -qE "^dolt (add|commit|push) " "$T1/calls.log"'
assert_true "skip message shown for both commit steps" \
    'grep -q "skipping Dolt commit" "$T1/out.log"'
assert_true "done message" 'grep -q "Done." "$T1/out.log"'

# ---------------------------------------------------------------------------
# 2. Collector commit path: step 3 stages ONLY changed allowlisted tables
# ---------------------------------------------------------------------------
echo ""
echo "--- 2. collector tables changed → limited add + commit + push ---"
T2="$TMP_ROOT/t2"
mkdir -p "$T2"
setup_fakes "$T2"
cat > "$T2/status.seq" <<'EOF'
On branch main
Changes not staged for commit:
	modified:         capital_main_flow
	new table:        some_unrelated_table
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T2"
assert_true "exit 0" 'test "$(cat "$T2/exit.code")" = 0'
assert_true "add limited to changed collector tables (no add .)" \
    'grep -qx "dolt --data-dir $T2/repos/compass_data add capital_main_flow" "$T2/calls.log"'
assert_false "unrelated/new table never staged" \
    'grep -q "some_unrelated_table" "$T2/calls.log"'
assert_true "collector commit message with ref" \
    'grep -qx "dolt --data-dir $T2/repos/compass_data commit -m feat: sepa collectors data ref #139" "$T2/calls.log"'
assert_true "push origin main" \
    'grep -qx "dolt --data-dir $T2/repos/compass_data push origin main" "$T2/calls.log"'
assert_true "no compute commit on clean step 6" \
    '! grep -q "sepa scores" "$T2/calls.log"'

# ---------------------------------------------------------------------------
# 3. Compute commit path: second commit after scoring (decision 2/9)
# ---------------------------------------------------------------------------
echo ""
echo "--- 3. compute tables changed → second Dolt commit ---"
T3="$TMP_ROOT/t3"
mkdir -p "$T3"
setup_fakes "$T3"
cat > "$T3/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
Changes not staged for commit:
	modified:         final_score
	modified:         data_updates
	new table:        technical_factor
===
EOF
run_script "$T3"
assert_true "exit 0" 'test "$(cat "$T3/exit.code")" = 0'
assert_true "compute add limited to changed tables (allowlist order)" \
    'grep -qx "dolt --data-dir $T3/repos/compass_data add technical_factor final_score data_updates" "$T3/calls.log"'
assert_true "compute commit message with ref" \
    'grep -qx "dolt --data-dir $T3/repos/compass_data commit -m feat: sepa scores ref #139" "$T3/calls.log"'
assert_true "compute push origin main" \
    'grep -qx "dolt --data-dir $T3/repos/compass_data push origin main" "$T3/calls.log"'
assert_true "no collector commit on clean step 3" \
    '! grep -q "sepa collectors data" "$T3/calls.log"'

# ---------------------------------------------------------------------------
# 4. Failure: step 1 (import) fails → non-zero exit + red error
# ---------------------------------------------------------------------------
echo ""
echo "--- 4. step 1 failure aborts loudly ---"
T4="$TMP_ROOT/t4"
mkdir -p "$T4"
setup_fakes "$T4"
cat > "$T4/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T4" FAKE_CARGO_FAIL_CALL=1
assert_true "non-zero exit on step 1 failure" 'test "$(cat "$T4/exit.code")" != 0'
assert_true "error names step 1" 'grep -q "step 1 failed" "$T4/err.log"'
assert_false "no further steps after failure" 'grep -q "sepa temperature" "$T4/calls.log"'

# ---------------------------------------------------------------------------
# 5. Preflight: missing DoltHub credentials → abort before any step
# ---------------------------------------------------------------------------
echo ""
echo "--- 5. preflight: no credentials aborts ---"
T5="$TMP_ROOT/t5"
mkdir -p "$T5"
setup_fakes "$T5"
cat > "$T5/status.seq" <<'EOF'
===
EOF
run_script "$T5" FAKE_DOLT_CREDS_FAIL=1
assert_true "non-zero exit on missing credentials" 'test "$(cat "$T5/exit.code")" != 0'
assert_true "credentials error message" 'grep -q "no DoltHub credentials" "$T5/err.log"'
assert_false "no step ran" 'grep -q "^cargo " "$T5/calls.log"'

# ---------------------------------------------------------------------------
# 6. Preflight: compass_data not a Dolt repo → abort
# ---------------------------------------------------------------------------
echo ""
echo "--- 6. preflight: missing .dolt aborts ---"
T6="$TMP_ROOT/t6"
mkdir -p "$T6"
setup_fakes "$T6"
cat > "$T6/status.seq" <<'EOF'
===
EOF
run_script "$T6" SEPA_COMPASS_DATA_DIR="$T6/nonexistent"
assert_true "non-zero exit on missing .dolt" 'test "$(cat "$T6/exit.code")" != 0'
assert_true "not-a-Dolt-database error message" \
    'grep -q "is not a Dolt database" "$T6/err.log"'
assert_false "no step ran" 'grep -q "^cargo " "$T6/calls.log"'

# ---------------------------------------------------------------------------
# 7. Five-source contract: step 2 fetch/import index_daily as the last source,
#    header info says 5 sources, and step 4 produces 5 append imports
# ---------------------------------------------------------------------------
echo ""
echo "--- 7. five-source contract: index_daily fetch/import + 5 append imports ---"
T7="$TMP_ROOT/t7"
mkdir -p "$T7"
setup_fakes "$T7"
cat > "$T7/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T7"
assert_true "exit 0 on five-source happy path" 'test "$(cat "$T7/exit.code")" = 0'
assert_true "step 2: fetch AND import index_daily as a paired source" \
    'grep -qx "uv run python main.py fetch index_daily" "$T7/calls.log" &&
     grep -qx "uv run python main.py import index_daily" "$T7/calls.log"'
assert_true "step 2: exactly 5 fetches and 5 imports" \
    'test "$(grep -c "^uv run python main.py fetch " "$T7/calls.log")" -eq 5 &&
     test "$(grep -c "^uv run python main.py import " "$T7/calls.log")" -eq 5'
assert_order "step 2: fetch index_daily after institution_survey import" "$T7/calls.log" \
    "uv run python main.py import institution_survey" "uv run python main.py fetch index_daily"
assert_order "step 2: import index_daily after fetch index_daily" "$T7/calls.log" \
    "uv run python main.py fetch index_daily" "uv run python main.py import index_daily"
assert_true "script header comment says 5 sources (not 4)" \
    'grep -q "5 sources" "$SEPA_SCRIPT" && ! grep -q "4 sources" "$SEPA_SCRIPT"'
assert_true "step 4: SQL anchor query includes index_daily" \
    'grep "dolt .* sql" "$T7/calls.log" | grep -q "table_name IN" &&
     grep "table_name IN" "$T7/calls.log" | grep -q "index_daily"'
assert_true "step 4: exactly 5 import-compass calls" \
    'test "$(grep -c "cargo run --bin compass-data -- import-compass --table " "$T7/calls.log")" -eq 5'

# ---------------------------------------------------------------------------
# 8. Error path: step 2 fails at index_daily fetch → non-zero exit, no step 4/5
# ---------------------------------------------------------------------------
echo ""
echo "--- 8. step 2 index_daily failure aborts before step 4/5 ---"
T8="$TMP_ROOT/t8"
mkdir -p "$T8"
setup_fakes "$T8"
cat > "$T8/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T8" FAKE_UV_FAIL_CALL=9
assert_true "non-zero exit on index_daily step 2 failure" 'test "$(cat "$T8/exit.code")" != 0'
assert_true "error names step 2" 'grep -q "step 2 failed" "$T8/err.log"'
assert_true "earlier sources still fetched before failure" \
    'grep -qx "uv run python main.py fetch main_flow" "$T8/calls.log"'
assert_false "failed source's import not attempted after fetch failure" \
    'grep -q "uv run python main.py import index_daily" "$T8/calls.log"'
assert_false "no step 4/5 after failure" \
    'grep -q "import-compass" "$T8/calls.log" || grep -q "sepa temperature" "$T8/calls.log"'

# ---------------------------------------------------------------------------
# 9. Allowlist boundary: index_daily + existing collector both changed → staged
#    in allowlist order; unrelated/new table not staged; no `dolt add .`
# ---------------------------------------------------------------------------
echo ""
echo "--- 9. allowlist: index_daily plus existing collector staged in order ---"
T9="$TMP_ROOT/t9"
mkdir -p "$T9"
setup_fakes "$T9"
cat > "$T9/status.seq" <<'EOF'
On branch main
Changes not staged for commit:
	modified:         capital_main_flow
	modified:         index_daily
	new table:        some_unrelated_table
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T9"
assert_true "exit 0" 'test "$(cat "$T9/exit.code")" = 0'
assert_true "add limited to changed collector allowlist in order" \
    'grep -qx "dolt --data-dir $T9/repos/compass_data add capital_main_flow index_daily" "$T9/calls.log"'
assert_false "unrelated/new table never staged" \
    'grep -q "some_unrelated_table" "$T9/calls.log"'
assert_false "no dolt add ." 'grep -q "dolt .* add \." "$T9/calls.log"'
assert_true "collector commit + push happen" \
    'grep -qx "dolt --data-dir $T9/repos/compass_data commit -m feat: sepa collectors data ref #139" "$T9/calls.log" &&
     grep -qx "dolt --data-dir $T9/repos/compass_data push origin main" "$T9/calls.log"'
assert_true "no compute commit on clean step 6" \
    '! grep -q "sepa scores" "$T9/calls.log"'

# ---------------------------------------------------------------------------
# 10. Allowlist boundary: only index_daily changed → staged alone and committed
# ---------------------------------------------------------------------------
echo ""
echo "--- 10. allowlist: only index_daily changed ---"
T10="$TMP_ROOT/t10"
mkdir -p "$T10"
setup_fakes "$T10"
cat > "$T10/status.seq" <<'EOF'
On branch main
Changes not staged for commit:
	modified:         index_daily
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T10"
assert_true "exit 0" 'test "$(cat "$T10/exit.code")" = 0'
assert_true "index_daily staged alone" \
    'grep -qx "dolt --data-dir $T10/repos/compass_data add index_daily" "$T10/calls.log"'
assert_true "collector commit + push for index_daily only" \
    'grep -qx "dolt --data-dir $T10/repos/compass_data commit -m feat: sepa collectors data ref #139" "$T10/calls.log" &&
     grep -qx "dolt --data-dir $T10/repos/compass_data push origin main" "$T10/calls.log"'
assert_true "no other collector tables staged" \
    '! grep -qE "dolt .* add (capital_main_flow|dragon_list|block_trade|institution_survey)" "$T10/calls.log"'

# ---------------------------------------------------------------------------
# 11. Incremental anchor: data_updates query includes index_daily; index_daily
#     append import uses the same --since as all other allowlisted tables
# ---------------------------------------------------------------------------
echo ""
echo "--- 11. incremental anchor includes index_daily ---"
T11="$TMP_ROOT/t11"
mkdir -p "$T11"
setup_fakes "$T11"
cat > "$T11/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T11"
assert_true "exit 0" 'test "$(cat "$T11/exit.code")" = 0'
assert_true "sql anchor query includes index_daily" \
    'grep "dolt .* sql" "$T11/calls.log" | grep -q "table_name IN" &&
     grep "table_name IN" "$T11/calls.log" | grep -q "index_daily"'
assert_true "import-compass index_daily uses same since 2026-07-31" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_daily --since 2026-07-31" "$T11/calls.log"'
assert_true "all 5 append tables use same since anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table block_trade --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_daily --since 2026-07-31" "$T11/calls.log"'

# ---------------------------------------------------------------------------
# 12. Empty anchor: dolt sql returns NULL → full import path for all 5 tables,
#     including index_daily (no --since on NULL anchor)
# ---------------------------------------------------------------------------
echo ""
echo "--- 12. empty anchor NULL → full import for all 5 (incl. index_daily) ---"
T12="$TMP_ROOT/t12"
mkdir -p "$T12"
setup_fakes "$T12"
cat > "$T12/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T12" FAKE_DOLT_SQL_NULL=1
assert_true "exit 0 on empty anchor" 'test "$(cat "$T12/exit.code")" = 0'
assert_true "index_daily full import (no --since)" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_daily" "$T12/calls.log"'
assert_false "index_daily import must not include --since on NULL anchor" \
    'grep -q "cargo run --bin compass-data -- import-compass --table index_daily --since" "$T12/calls.log"'
assert_true "all 5 tables take full import path on NULL anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table block_trade" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_daily" "$T12/calls.log"'
assert_false "no table uses --since on NULL anchor" \
    'grep -q "import-compass --table .* --since" "$T12/calls.log"'

# ---------------------------------------------------------------------------
# 13. Basic error: step 2 fetch failure on an existing source (main_flow) stops
#     before that source's import and before any later source/step
# ---------------------------------------------------------------------------
echo ""
echo "--- 13. step 2 main_flow fetch failure aborts loudly ---"
T13="$TMP_ROOT/t13"
mkdir -p "$T13"
setup_fakes "$T13"
cat > "$T13/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T13" FAKE_UV_FAIL_CALL=1
assert_true "non-zero exit on main_flow fetch failure" 'test "$(cat "$T13/exit.code")" != 0'
assert_true "error names step 2" 'grep -q "step 2 failed" "$T13/err.log"'
assert_true "failed source fetch was attempted" \
    'grep -qx "uv run python main.py fetch main_flow" "$T13/calls.log"'
assert_false "failed source import not attempted after fetch failure" \
    'grep -q "uv run python main.py import main_flow" "$T13/calls.log"'
assert_false "later source not started after fetch failure" \
    'grep -q "uv run python main.py fetch dragon" "$T13/calls.log"'
assert_false "no step 4/5 after step 2 failure" \
    'grep -q "import-compass" "$T13/calls.log" || grep -q "sepa temperature" "$T13/calls.log"'

# ---------------------------------------------------------------------------
# 14. Basic error: step 2 import failure on an existing source (main_flow) still
#     stops the pipeline loudly before later sources and step 4/5
# ---------------------------------------------------------------------------
echo ""
echo "--- 14. step 2 main_flow import failure aborts loudly ---"
T14="$TMP_ROOT/t14"
mkdir -p "$T14"
setup_fakes "$T14"
cat > "$T14/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T14" FAKE_UV_FAIL_CALL=2
assert_true "non-zero exit on main_flow import failure" 'test "$(cat "$T14/exit.code")" != 0'
assert_true "error names step 2" 'grep -q "step 2 failed" "$T14/err.log"'
assert_true "fetch succeeded before import failure" \
    'grep -qx "uv run python main.py fetch main_flow" "$T14/calls.log"'
assert_true "failed source import was attempted" \
    'grep -qx "uv run python main.py import main_flow" "$T14/calls.log"'
assert_false "later source not started after import failure" \
    'grep -q "uv run python main.py fetch dragon" "$T14/calls.log"'
assert_false "no step 4/5 after step 2 failure" \
    'grep -q "import-compass" "$T14/calls.log" || grep -q "sepa temperature" "$T14/calls.log"'

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
