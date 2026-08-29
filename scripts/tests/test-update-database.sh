#!/bin/bash
# Tests for scripts/update-database.sh (issue #306).
# Mirrors the pre-push-ref-regex-test.sh precedent: a `bash -n` syntax gate plus
# behavioral assertions against mocked cargo/dolt in a temp dir — no real
# network access, no real Dolt mutation, no data repos touched.
# Run: scripts/tests/test-update-database.sh
#
# This suite carries the adversarial RED contract for issue #306:
#   - COLLECTOR_TABLES = all 11 compass_data tables, in declared order
#   - step 2 = exactly one `cargo run --bin compass-collectors -- sync`
#   - step 4 = exactly 11 import-compass calls
#   - stock_basic and index_basic always full (never --since)
#   - financial four tables use per-table last_report_date anchors
#   - non-financial incremental tables preserve per-table anchor behavior
#   - sync failure aborts before step 4/5/6/7
#   - dolt commits only COLLECTOR_TABLES/COMPUTE_TABLES, never `dolt add .`
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SEPA_SCRIPT="$PROJECT_ROOT/scripts/update-database.sh"

FAIL=0
TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TMP_ROOT"' EXIT

# --- 0. Syntax gate ---
echo "--- 0. syntax ---"
if bash -n "$SEPA_SCRIPT" 2>&1; then
    echo "PASS: bash -n update-database.sh"
else
    echo "FAIL: bash -n update-database.sh"
    exit 1
fi

# --- Harness: build a temp dir with mocked cargo/dolt ---
# The mocks log every invocation to $FAKE_LOG; dolt status pops one block per
# call from $FAKE_STATUS_SEQ (blocks separated by "===" lines).
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
        if [ -n "${FAKE_DOLT_SQL_FAIL:-}" ]; then
            echo "mock dolt sql: failing" >&2
            exit 1
        fi
        if [ -n "${FAKE_DOLT_SQL_NULL:-}" ]; then
            printf 'd\nNULL\n'
        elif [ -n "${FAKE_DOLT_SQL_MIXED_NULL:-}" ]; then
            # stock_basic/index_basic/index_daily have no anchor yet; all other
            # collector tables already have one. (stock_basic/index_basic are
            # never queried by a correct implementation; this guards against
            # accidentally querying/using an anchor for them.)
            if printf '%s\n' "$*" | grep -qE "table_name = '(index_daily|stock_basic|index_basic)'"; then
                printf 'd\nNULL\n'
            else
                printf 'd\n2026-07-31\n'
            fi
        elif [ -n "${FAKE_DOLT_SQL_DISTINCT:-}" ]; then
            # Per-table anchors with distinct dates, to prove each table uses
            # its own anchor rather than a shared global MAX.
            case "$*" in
                *"fin_indicators"*) printf 'd\n2026-07-28\n' ;;
                *"fin_balance_sheet"*) printf 'd\n2026-07-29\n' ;;
                *"fin_income"*) printf 'd\n2026-08-04\n' ;;
                *"fin_cash_flow"*) printf 'd\n2026-08-05\n' ;;
                *"capital_main_flow"*) printf 'd\n2026-07-30\n' ;;
                *"dragon_list"*) printf 'd\n2026-07-31\n' ;;
                *"block_trade"*) printf 'd\n2026-08-01\n' ;;
                *"institution_survey"*) printf 'd\n2026-08-02\n' ;;
                *"index_daily"*) printf 'd\n2026-08-03\n' ;;
                *) printf 'd\nNULL\n' ;;
            esac
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

    chmod +x "$t/bin/dolt" "$t/bin/cargo"

    # Fake investment-data sync: run_script points update-database.sh at this
    # file via SYNC_INVESTMENT_SCRIPT so no real upstream Dolt fetch happens.
    cat > "$t/sync-fake.sh" <<'EOF'
#!/bin/bash
echo "sync-investment $*" >> "${FAKE_LOG:?fake log unset}"
exit 0
EOF
    chmod +x "$t/sync-fake.sh"
}

# Run update-database.sh against the mocked environment in $1; capture exit code.
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
        SYNC_TIMING_DIR="$t/timings" \
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
# 1. Happy path: clean Dolt both times → no commit/push at all, single sync,
#    11 table import chain
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
assert_true "step 2: exactly one compass-collectors sync (single collector entry point)" \
    'test "$(grep -c "^cargo run --bin compass-collectors -- sync" "$T1/calls.log")" -eq 1'
assert_true "step 2: no per-source fetch remains" \
    '! grep -q "cargo run --bin compass-collectors -- fetch " "$T1/calls.log"'
assert_true "step 2: no per-source import remains" \
    '! grep -q "cargo run --bin compass-collectors -- import " "$T1/calls.log"'
assert_order "step 2: sync runs before step 4 first import-compass" "$T1/calls.log" \
    "cargo run --bin compass-collectors -- sync" "import-compass --table stock_basic"
assert_true "step 0: sync-investment-data runs before import" \
    'grep -q "^sync-investment" "$T1/calls.log"'
assert_order "step 0: investment sync before market import" "$T1/calls.log" \
    "sync-investment" "cargo run --bin compass-data -- import"
assert_true "step 1b: stock_daily gap check runs after import" \
    'grep -q "cargo run --bin compass-data -- check-stock-daily" "$T1/calls.log"'
assert_order "step 1b: gap check after import before sync" "$T1/calls.log" \
    "cargo run --bin compass-data -- import" "check-stock-daily"
assert_order "step 1b: gap check before compass-collectors sync" "$T1/calls.log" \
    "check-stock-daily" "cargo run --bin compass-collectors -- sync"

assert_true "step 4: 11 table exports (stock_basic + index_basic full + 9 anchored)" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_indicators --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_balance_sheet --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_income --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_cash_flow --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table block_trade --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_daily --since 2026-07-31" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T1/calls.log"'
assert_true "step 4: index_basic is a full overwrite (no --since)" \
    '! grep -q "import-compass --table index_basic --since" "$T1/calls.log"'
assert_true "step 4: stock_basic is a full overwrite (no --since)" \
    '! grep -q "import-compass --table stock_basic --since" "$T1/calls.log"'

assert_order "step 4: append imports follow COLLECTOR_TABLES order" "$T1/calls.log" \
    "import-compass --table stock_basic" "import-compass --table fin_indicators"
assert_order "step 4: financial four in declared order" "$T1/calls.log" \
    "import-compass --table fin_indicators" "import-compass --table fin_balance_sheet"
assert_order "step 4: financial four in declared order" "$T1/calls.log" \
    "import-compass --table fin_balance_sheet" "import-compass --table fin_income"
assert_order "step 4: financial four in declared order" "$T1/calls.log" \
    "import-compass --table fin_income" "import-compass --table fin_cash_flow"
assert_order "step 4: fin_cash_flow before capital_main_flow" "$T1/calls.log" \
    "import-compass --table fin_cash_flow" "import-compass --table capital_main_flow"
assert_order "step 4: append imports in allowlist order" "$T1/calls.log" \
    "import-compass --table capital_main_flow" "import-compass --table dragon_list"
assert_order "step 4: append imports in allowlist order" "$T1/calls.log" \
    "import-compass --table dragon_list" "import-compass --table block_trade"
assert_order "step 4: append imports in allowlist order" "$T1/calls.log" \
    "import-compass --table block_trade" "import-compass --table institution_survey"
assert_order "step 4: append imports in allowlist order" "$T1/calls.log" \
    "import-compass --table institution_survey" "import-compass --table index_daily"
assert_order "step 4: index_basic full overwrite follows index_daily" "$T1/calls.log" \
    "import-compass --table index_daily" "import-compass --table index_basic"

assert_true "step 5: temperature before score" \
    'grep -qx "cargo run --bin compass-data -- sepa temperature" "$T1/calls.log" &&
     grep -qx "cargo run --bin compass-data -- sepa score --top 50" "$T1/calls.log"'
assert_order "step 5: temperature runs before score" "$T1/calls.log" \
    "sepa temperature" "sepa score --top 50"
assert_true "step 4b: sepa backfill-dates runs" \
    'grep -qx "cargo run --bin compass-data -- sepa backfill-dates" "$T1/calls.log"'
assert_order "step 4b: backfill after last import-compass" "$T1/calls.log" \
    "import-compass --table index_basic" "sepa backfill-dates"
assert_order "step 4b: backfill before temperature" "$T1/calls.log" \
    "sepa backfill-dates" "sepa temperature"
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
# 7. Single-sync + 11-table contract: step 2 exactly one sync, step 4 exactly
#    11 import-compass calls, COLLECTOR_TABLES contains all 11 tables
# ---------------------------------------------------------------------------
echo ""
echo "--- 7. single-sync and 11-table contract ---"
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
assert_true "exit 0 on new happy path" 'test "$(cat "$T7/exit.code")" = 0'
assert_true "step 2: exactly one sync, no fetch/import source loop" \
    'test "$(grep -c "^cargo run --bin compass-collectors -- sync" "$T7/calls.log")" -eq 1 &&
     test "$(grep -c "^cargo run --bin compass-collectors -- fetch " "$T7/calls.log")" -eq 0 &&
     test "$(grep -c "^cargo run --bin compass-collectors -- import " "$T7/calls.log")" -eq 0'
assert_true "step 4: exactly 11 import-compass calls" \
    'test "$(grep -c "cargo run --bin compass-data -- import-compass --table " "$T7/calls.log")" -eq 11'
assert_true "step 4: stock_basic full overwrite first" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T7/calls.log"'
assert_true "step 4: index_basic full overwrite last" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T7/calls.log"'
assert_true "COLLECTOR_TABLES declares all 11 compass_data tables in order" \
    'test "$(grep "^COLLECTOR_TABLES=" "$SEPA_SCRIPT")" = "COLLECTOR_TABLES=(stock_basic fin_indicators fin_balance_sheet fin_income fin_cash_flow capital_main_flow dragon_list block_trade institution_survey index_daily index_basic)"'

# ---------------------------------------------------------------------------
# 8. Error path: sync (step 2) fails → non-zero exit, no step 4/5/6/7
# ---------------------------------------------------------------------------
echo ""
echo "--- 8. step 2 compass-collectors sync failure aborts before step 4/5 ---"
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
run_script "$T8" FAKE_CARGO_FAIL_CALL=3
assert_true "non-zero exit on sync failure" 'test "$(cat "$T8/exit.code")" != 0'
assert_true "sync call was attempted" \
    'grep -qx "cargo run --bin compass-collectors -- sync" "$T8/calls.log"'
assert_true "error names step 2" 'grep -q "step 2 failed" "$T8/err.log"'
assert_false "no step 4/5 after sync failure" \
    'grep -q "import-compass" "$T8/calls.log" || grep -q "sepa temperature" "$T8/calls.log"'

# ---------------------------------------------------------------------------
# 9. Allowlist boundary: index_daily + index_basic plus existing collector changed
#    → staged in allowlist order; unrelated/new table not staged; no `dolt add .`
# ---------------------------------------------------------------------------
echo ""
echo "--- 9. allowlist: index_daily + index_basic plus existing collector staged ---"
T9="$TMP_ROOT/t9"
mkdir -p "$T9"
setup_fakes "$T9"
cat > "$T9/status.seq" <<'EOF'
On branch main
Changes not staged for commit:
	modified:         capital_main_flow
	modified:         index_daily
	modified:         index_basic
	new table:        some_unrelated_table
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T9"
assert_true "exit 0" 'test "$(cat "$T9/exit.code")" = 0'
assert_true "add limited to changed collector allowlist in order" \
    'grep -qx "dolt --data-dir $T9/repos/compass_data add capital_main_flow index_daily index_basic" "$T9/calls.log"'
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
# 11. Incremental anchor: per-table data_updates queries for all 9 anchored
#     tables; all use their own 2026-07-31 default; stock_basic/index_basic full
# ---------------------------------------------------------------------------
echo ""
echo "--- 11. per-table incremental anchor: 9 anchored tables since, 2 full ---"
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
assert_true "per-table anchor query for financial table fin_indicators" \
    'grep "dolt .* sql" "$T11/calls.log" | grep -q "table_name = .fin_indicators"'
assert_true "per-table anchor query for financial table fin_cash_flow" \
    'grep "dolt .* sql" "$T11/calls.log" | grep -q "table_name = .fin_cash_flow"'
assert_true "per-table anchor query for partial table index_daily" \
    'grep "dolt .* sql" "$T11/calls.log" | grep -q "table_name = .index_daily"'
assert_true "stock_basic is not anchor-queried (always full)" \
    '! grep "dolt .* sql" "$T11/calls.log" | grep -q "table_name = .stock_basic"'
assert_true "index_basic is not anchor-queried (always full)" \
    '! grep "dolt .* sql" "$T11/calls.log" | grep -q "table_name = .index_basic"'
assert_true "financial four use same default since anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_indicators --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_balance_sheet --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_income --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_cash_flow --since 2026-07-31" "$T11/calls.log"'
assert_true "all 5 incremental partial tables use same since anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table block_trade --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey --since 2026-07-31" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_daily --since 2026-07-31" "$T11/calls.log"'
assert_true "stock_basic and index_basic stay full overwrite even with anchors" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T11/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T11/calls.log" &&
     ! grep -q "import-compass --table stock_basic --since" "$T11/calls.log" &&
     ! grep -q "import-compass --table index_basic --since" "$T11/calls.log"'

# ---------------------------------------------------------------------------
# 12. Empty anchor: no anchors → full import for all 11 tables
# ---------------------------------------------------------------------------
echo ""
echo "--- 12. empty anchor NULL → full import for all 11 ---"
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
assert_true "financial four full import (no --since) on NULL anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_indicators" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_balance_sheet" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_income" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_cash_flow" "$T12/calls.log"'
assert_true "partial index_daily full import (no --since) on NULL anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_daily" "$T12/calls.log"'
assert_true "all 11 tables take full import path on NULL anchor" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_indicators" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_balance_sheet" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_income" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table fin_cash_flow" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table block_trade" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_daily" "$T12/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T12/calls.log"'
assert_false "no table uses --since on NULL anchor" \
    'grep -q "import-compass --table .* --since" "$T12/calls.log"'

# ---------------------------------------------------------------------------
# 12b. Mixed anchor: index_daily is null (full), partial/financial have anchors,
#      stock_basic/index_basic always full
# ---------------------------------------------------------------------------
echo ""
echo "--- 12b. mixed anchor: null-index_daily full, others incremental, 2 always-full ---"
T12B="$TMP_ROOT/t12b"
mkdir -p "$T12B"
setup_fakes "$T12B"
cat > "$T12B/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T12B" FAKE_DOLT_SQL_MIXED_NULL=1
assert_true "exit 0 on mixed anchors" 'test "$(cat "$T12B/exit.code")" = 0'
assert_true "financial table still uses incremental --since when anchor present" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_indicators --since 2026-07-31" "$T12B/calls.log"'
assert_true "existing partial table still uses incremental --since" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow --since 2026-07-31" "$T12B/calls.log"'
assert_true "index_daily with no own anchor uses full import" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_daily" "$T12B/calls.log" &&
     ! grep -q "import-compass --table index_daily --since" "$T12B/calls.log"'
assert_true "stock_basic always full overwrite" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T12B/calls.log" &&
     ! grep -q "import-compass --table stock_basic --since" "$T12B/calls.log"'
assert_true "index_basic always full overwrite" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T12B/calls.log" &&
     ! grep -q "import-compass --table index_basic --since" "$T12B/calls.log"'

# ---------------------------------------------------------------------------
# 12c. dolt sql anchor failure: a real query error must abort loudly, not
#      silently degrade to a full import
# ---------------------------------------------------------------------------
echo ""
echo "--- 12c. dolt sql anchor failure aborts loudly ---"
T12C="$TMP_ROOT/t12c"
mkdir -p "$T12C"
setup_fakes "$T12C"
cat > "$T12C/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T12C" FAKE_DOLT_SQL_FAIL=1
assert_true "non-zero exit on dolt sql failure" 'test "$(cat "$T12C/exit.code")" != 0'
assert_true "error names step 4 anchor query" \
    'grep -q "step 4 failed: dolt sql anchor query" "$T12C/err.log"'
# A correctly fixed pipeline imports the always-full stock_basic before the
# first anchor query, so zero import-compass is no longer the right contract.
# The real adversarial demand is: no anchored/partial/financial table import may
# be attempted after the anchor query fails (the pipeline must stop there).
assert_false "no anchored import after anchor query failure" \
    'grep -q "import-compass --table fin_indicators" "$T12C/calls.log" ||
     grep -q "import-compass --table fin_balance_sheet" "$T12C/calls.log" ||
     grep -q "import-compass --table fin_income" "$T12C/calls.log" ||
     grep -q "import-compass --table fin_cash_flow" "$T12C/calls.log" ||
     grep -q "import-compass --table capital_main_flow" "$T12C/calls.log" ||
     grep -q "import-compass --table dragon_list" "$T12C/calls.log" ||
     grep -q "import-compass --table block_trade" "$T12C/calls.log" ||
     grep -q "import-compass --table institution_survey" "$T12C/calls.log" ||
     grep -q "import-compass --table index_daily" "$T12C/calls.log"'

# ---------------------------------------------------------------------------
# 12d. Distinct per-table anchors: each of the 9 anchored tables uses its own
#      data_updates.last_report_date; stock_basic/index_basic always full
# ---------------------------------------------------------------------------
echo ""
echo "--- 12d. distinct per-table anchors used independently ---"
T12D="$TMP_ROOT/t12d"
mkdir -p "$T12D"
setup_fakes "$T12D"
cat > "$T12D/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T12D" FAKE_DOLT_SQL_DISTINCT=1
assert_true "exit 0 on distinct anchors" 'test "$(cat "$T12D/exit.code")" = 0'
assert_true "fin_indicators uses its own 2026-07-28" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_indicators --since 2026-07-28" "$T12D/calls.log"'
assert_true "fin_balance_sheet uses its own 2026-07-29" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_balance_sheet --since 2026-07-29" "$T12D/calls.log"'
assert_true "fin_income uses its own 2026-08-04" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_income --since 2026-08-04" "$T12D/calls.log"'
assert_true "fin_cash_flow uses its own 2026-08-05" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table fin_cash_flow --since 2026-08-05" "$T12D/calls.log"'
assert_true "capital_main_flow uses its own 2026-07-30" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table capital_main_flow --since 2026-07-30" "$T12D/calls.log"'
assert_true "dragon_list uses its own 2026-07-31" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table dragon_list --since 2026-07-31" "$T12D/calls.log"'
assert_true "block_trade uses its own 2026-08-01" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table block_trade --since 2026-08-01" "$T12D/calls.log"'
assert_true "institution_survey uses its own 2026-08-02" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table institution_survey --since 2026-08-02" "$T12D/calls.log"'
assert_true "index_daily uses its own 2026-08-03" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_daily --since 2026-08-03" "$T12D/calls.log"'
assert_true "stock_basic and index_basic full even when others have distinct anchors" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T12D/calls.log" &&
     grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T12D/calls.log" &&
     ! grep -q "import-compass --table stock_basic --since" "$T12D/calls.log" &&
     ! grep -q "import-compass --table index_basic --since" "$T12D/calls.log"'

# ---------------------------------------------------------------------------
# 13. Adversarial: sync is the sole step-2 command — a failure at the third cargo
#     call (the sync) must stop the whole pipeline before import-compass
# ---------------------------------------------------------------------------
echo ""
echo "--- 13. sync failure (third cargo call = Rust sync) aborts before any import-compass ---"
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
run_script "$T13" FAKE_CARGO_FAIL_CALL=3
assert_true "non-zero exit on sync failure" 'test "$(cat "$T13/exit.code")" != 0'
assert_true "the failed cargo invocation is the Rust sync, not a fetch/import" \
    'grep -qx "cargo run --bin compass-collectors -- sync" "$T13/calls.log" &&
     ! grep -q "compass-collectors -- fetch" "$T13/calls.log" &&
     ! grep -q "compass-collectors -- import" "$T13/calls.log"'
assert_true "error names step 2" 'grep -q "step 2 failed" "$T13/err.log"'
assert_false "no step 4/5 after sync failure" \
    'grep -q "import-compass" "$T13/calls.log" || grep -q "sepa temperature" "$T13/calls.log"'
assert_true "exactly one sync attempt (no retry)" \
    'test "$(grep -c "^cargo run --bin compass-collectors -- sync" "$T13/calls.log")" -eq 1'

# ---------------------------------------------------------------------------
# 14. Adversarial: dolt collector commit must cover the full 11-table allowlist
#     (including stock_basic and financial four), and must never `dolt add .`
# ---------------------------------------------------------------------------
echo ""
echo "--- 14. dolt collector commit allowlist covers all 11 tables ---"
T14="$TMP_ROOT/t14"
mkdir -p "$T14"
setup_fakes "$T14"
cat > "$T14/status.seq" <<'EOF'
On branch main
Changes not staged for commit:
	modified:         stock_basic
	modified:         fin_indicators
	modified:         fin_balance_sheet
	modified:         fin_income
	modified:         fin_cash_flow
	modified:         capital_main_flow
	modified:         dragon_list
	modified:         block_trade
	modified:         institution_survey
	modified:         index_daily
	modified:         index_basic
	new table:        some_unrelated_table
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T14"
assert_true "exit 0" 'test "$(cat "$T14/exit.code")" = 0'
assert_true "collector add includes all 11 tables in allowlist order" \
    'grep -qx "dolt --data-dir $T14/repos/compass_data add stock_basic fin_indicators fin_balance_sheet fin_income fin_cash_flow capital_main_flow dragon_list block_trade institution_survey index_daily index_basic" "$T14/calls.log"'
assert_false "unrelated/new table never staged" \
    'grep -q "some_unrelated_table" "$T14/calls.log"'
assert_false "no dolt add ." 'grep -q "dolt .* add \." "$T14/calls.log"'
assert_true "collector commit + push happen" \
    'grep -qx "dolt --data-dir $T14/repos/compass_data commit -m feat: sepa collectors data ref #139" "$T14/calls.log" &&
     grep -qx "dolt --data-dir $T14/repos/compass_data push origin main" "$T14/calls.log"'
assert_true "no compute commit on clean step 6" \
    '! grep -q "sepa scores" "$T14/calls.log"'

# ---------------------------------------------------------------------------
# 15. Adversarial: dolt sql must NEVER be issued for stock_basic or index_basic;
#     those two are full-coverage tables by contract, so an anchor query for them
#     is an implementation bug that could make a full-coverage table incremental.
# ---------------------------------------------------------------------------
echo ""
echo "--- 15. full-coverage tables never anchor-queried ---"
T15="$TMP_ROOT/t15"
mkdir -p "$T15"
setup_fakes "$T15"
cat > "$T15/status.seq" <<'EOF'
On branch main
nothing to commit, working tree clean
===
On branch main
nothing to commit, working tree clean
===
EOF
run_script "$T15" FAKE_DOLT_SQL_DISTINCT=1
assert_true "exit 0 on distinct anchors" 'test "$(cat "$T15/exit.code")" = 0'
assert_false "no dolt sql anchor query for stock_basic" \
    'grep "dolt .* sql" "$T15/calls.log" | grep -q "table_name = .stock_basic"'
assert_false "no dolt sql anchor query for index_basic" \
    'grep "dolt .* sql" "$T15/calls.log" | grep -q "table_name = .index_basic"'
assert_true "stock_basic full (no --since)" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table stock_basic" "$T15/calls.log" &&
     ! grep -q "import-compass --table stock_basic --since" "$T15/calls.log"'
assert_true "index_basic full (no --since)" \
    'grep -qx "cargo run --bin compass-data -- import-compass --table index_basic" "$T15/calls.log" &&
     ! grep -q "import-compass --table index_basic --since" "$T15/calls.log"'

# ---------------------------------------------------------------------------
# 16. Requirement: script header comments must describe the complete compass_data
#     daily refresh, not the old 5-source / 6-table SEPA-only pipeline.
#     This is a static contract from plan item 5: the header/comment must reflect
#     "complete compass_data refresh (not 5 sources / 6 tables)".
# ---------------------------------------------------------------------------
echo ""
echo "--- 16. script header reflects complete compass_data refresh ---"
assert_false "script header no longer says collectors fetch 5 sources" \
    'grep -n "collectors fetch 5 sources" "$SEPA_SCRIPT"'
assert_false "script header no longer says import-compass 6 tables" \
    'grep -n "import-compass 6 tables" "$SEPA_SCRIPT"'
assert_false "script header no longer says (5 incremental" \
    'grep -n "(5 incremental" "$SEPA_SCRIPT"'
# Positive contract: the header must describe the full table set.  We accept the
# most direct formulations a correct fix might use; the old header has none of
# them, so this RED is real and will GREEN after the documented sync.
assert_true "script header mentions all-11 / complete compass_data refresh" \
    'grep -E "(complete compass_data|all 11 compass_data|11 compass_data tables|11 tables|11 张表|11 个表|全部 11|完整.*compass_data|full compass_data)" "$SEPA_SCRIPT"'
assert_true "script header reflects the single compass-collectors sync entry point" \
    'grep -E "(main\.py sync|single.*sync|one.*sync|collect.*sync)" "$SEPA_SCRIPT"'
assert_true "script header still mentions the 11 table import step" \
    'grep -E "(11 table|11 tables|11 张|11 个|all 11|全部 11)" "$SEPA_SCRIPT"'

# ---------------------------------------------------------------------------
# 17. Requirement: `.dsh/kb/user/cli.md` daily pipeline description must match the
#     complete refresh (plan item 7 — doc sync is implementation scope, but a
#     stable static assertion is possible).
# ---------------------------------------------------------------------------
echo ""
echo "--- 17. user cli.md daily pipeline description matches complete refresh ---"
CLI_DOC="$PROJECT_ROOT/.dsh/kb/user/cli.md"
assert_true "cli.md exists before static check" 'test -f "$CLI_DOC"'
# Old text that must not remain in the daily-pipeline description.
assert_false "cli.md no longer describes 5 time-series + index_basic only" \
    'grep -q "5 个时序表增量" "$CLI_DOC"'
assert_false "cli.md no longer says 5 sources / 6 tables" \
    'grep -qE "5 sources|6 tables|5 个时序表" "$CLI_DOC"'
# Positive contract: the daily pipeline description must cover the full set
# (stock_basic and financial tables plus index tables).
assert_true "cli.md daily pipeline covers stock_basic" \
    'grep -q "stock_basic" "$CLI_DOC"'
assert_true "cli.md daily pipeline covers financial tables" \
    'grep -qE "fin_indicators|财务四表|财务表" "$CLI_DOC"'
assert_true "cli.md daily pipeline reflects 11-table / full refresh" \
    'grep -qE "11 张表|11 tables|11 个表|全部 11|all 11|完整.*刷新|完整.*compass_data|compass_data 每日刷新|daily.*compass_data" "$CLI_DOC"'

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
