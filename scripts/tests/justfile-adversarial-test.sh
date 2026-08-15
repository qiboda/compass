#!/bin/bash
# Adversarial tests for the newly-introduced root `justfile` (ref #265).
#
# interface contract (issue #265):
#   run    = cargo run --bin compass                        (first recipe == default)
#   build  = cargo build
#   test   = cargo test
#   fmt    = cargo fmt
#   clippy = cargo clippy -- -D warnings
#   check  = fmt --check(fmt) + clippy + test, in that order (full gate combo)
#   import = cargo run --bin compass-data -- import
#   export = cargo run --bin compass-data -- export
#   backup = cargo run --bin compass-data -- backup
#
# adversarial attack dimensions (all against the declared contract):
#   1. SILENT WEAKENING / error path: every recipe's dry-run command must
#      contain the EXACT declared command. Presence of the full string (not
#      substring-of-shorter) catches `cargo clippy` losing `-- -D warnings`, a
#      `check` gate dropping fmt/clippy/test, etc.
#   2. DEFAULT-RECIPE DRIFT / boundary: `just -n` (no args) must resolve to the
#      first recipe == `run` == `cargo run --bin compass`, and must NOT leak any
#      `compass-data` pipeline command (a silently promoted import/export/backup
#      would be a default drift).
#      NOTE (validity): `just --list` sorts recipes alphabetically, so `run` is
#      NOT "first in --list". The correct default-recipe oracle is the dry-run
#      of the no-argument invocation itself — that is what just executes as the
#      first recipe. Asserting "run first in --list" would be a no-solution test
#      and is intentionally omitted.
#   3. RECIPE CONFLICT / duplicate & missing: `just --list` rc must be 0 (just
#      hard-errors on a redefined recipe name), and the exact recipe SET must
#      equal the declared 9 (no extra promotion risk, none missing).
#   4. ORDER of the `check` gate: `just -n check` must emit fmt before clippy
#      before test (line-index comparison catches a permuted dependency list).
#   5. FORMAT discipline: `just --fmt --check` must pass.
#   6. PERFORMANCE / resource: `just --list` must complete within 5s.
#   7. NO-JUSTFILE distinguishability: probe in a justfile-less dir must report
#      "no justfile" (rc!=0) — proving our RED root cause is the missing file,
#      not a broken assertion, and cleanly separable from assertion failures.
#
# Test are DRY-RUN ONLY: `just -n` / `just --list` / `just --fmt --check`.
# Nothing under test is actually executed (no cargo build/test/clippy/run).
#
# Run: bash scripts/tests/justfile-adversarial-test.sh
set -euo pipefail

REPO_ROOT=$(git -C "$(dirname "$0")/../.." rev-parse --show-toplevel 2>/dev/null || echo ".")

# --- We must run inside REPO_ROOT so `just` resolves OUR justfile, not some
#     parent/other directory's (path-safety: no false positive in a wrong dir).
cd "$REPO_ROOT"

FAIL=0
N=0

# Capture `just -n <args...>`: dry-run prints each command to STDERR. Returns rc
# in $DRY_RC and stderr in $DRY_ERR (stdout discarded; -n prints to stderr).
just_dryrun() {
    local err
    err=$(just -n "$@" 2>&1 >/dev/null) && DRY_RC=0 || DRY_RC=$?
    DRY_ERR=$err
}

# Capture `just <flag-ish> ...` (list / fmt): distinguish stdout vs stderr lazily
# by merging; caller filters.
just_raw() {
    local out rc
    out=$(just "$@") && rc=0 || rc=$?
    JUST_OUT=$out
    JUST_RC=$rc
}

# Whether the current dir has a justfile.
has_justfile() {
    [ -f "$REPO_ROOT/justfile" ] || [ -f "$REPO_ROOT/Justfile" ] || [ -f "$REPO_ROOT/justfile.default" ]
}

# verdict: $1=name $2=ok(0|1) $3=detail lines
verdict() {
    local name="$1" ok="$2" detail="${3:-}"
    N=$((N + 1))
    if [ "$ok" -eq 1 ]; then
        echo "PASS: $name"
    else
        echo "FAIL: $name"
        [ -z "$detail" ] || printf '%s\n' "$detail" | sed 's/^/    /'
        FAIL=1
    fi
}

# assert the exact declared command appears as a line in `just -n <recipe>`
# stderr (presence of the FULL string — catches silent weakening).
single_recipe_present() {  # $1=name $2=recipe $3=expected_exact_command
    local label="$1" recipe="$2" expected="$3"
    just_dryrun "$recipe"
    if [ "$DRY_RC" -eq 0 ] && printf '%s\n' "$DRY_ERR" | grep -Fxq "$expected"; then
        verdict "$label: '$recipe' dry-run == $expected" 1
    else
        verdict "$label: '$recipe' dry-run must contain exactly '$expected' (rc=$DRY_RC)" 0 \
"--- stderr ---
$DRY_ERR"
    fi
}

echo "=== 0. just available ==="
if ! command -v just >/dev/null 2>&1; then
    echo "FAIL: 'just' not on PATH (needed to run these tests)"
    exit 1
fi
verdict "just present on PATH" 1

echo ""
echo "=== 3. recipe existence / conflict / set (just --list) ==="
just_raw --list
if [ "$JUST_RC" -eq 0 ]; then
    verdict "just --list rc=0 (no duplicated recipe — just hard-errors on redefinition)" 1
else
    verdict "just --list rc must be 0 (duplicate recipe or parse error)" 0 "$JUST_OUT"
fi

LIST_LINES=$(printf '%s\n' "$JUST_OUT" | sed 's/^[[:space:]]*//' | grep '^[a-z][a-z0-9_-]*$' || true)
EXPECTED_SET="backup
build
check
clippy
export
fmt
import
run
test"
EXPECTED_COUNT=$(printf '%s\n' "$EXPECTED_SET" | wc -l)
GOT_COUNT=$(printf '%s\n' "$LIST_LINES" | sed '/^$/d' | wc -l)
GOT_SET=$(printf '%s\n' "$LIST_LINES" | sed '/^$/d' | sort -u)
SORTED_EXPECTED=$(printf '%s\n' "$EXPECTED_SET" | sort)

if [ "$JUST_RC" -eq 0 ] && [ "$GOT_COUNT" -eq "$EXPECTED_COUNT" ] && [ "$GOT_SET" = "$SORTED_EXPECTED" ]; then
    verdict "recipe set == declared 9, no extra, none missing, each unique ($GOT_COUNT recipes)" 1
else
    verdict "recipe set must be exactly the declared 9 (no dup, no extra, none missing)" 0 \
"--- declared ---
$SORTED_EXPECTED
--- from --list ($GOT_COUNT unique, rc=$JUST_RC) ---
$GOT_SET"
fi

echo ""
echo "=== 1. single-recipe exact command (silent-weakening guard) ==="
single_recipe_present "build"    build    "cargo build"
single_recipe_present "test"     test     "cargo test"
single_recipe_present "fmt"      fmt      "cargo fmt"
# clippy MUST keep `-- -D warnings` — presence of the full line catches a
# weakened `cargo clippy`.
single_recipe_present "clippy"   clippy   "cargo clippy -- -D warnings"
single_recipe_present "import"   import   "cargo run --bin compass-data -- import"
single_recipe_present "export"   export   "cargo run --bin compass-data -- export"
single_recipe_present "backup"   backup   "cargo run --bin compass-data -- backup"
single_recipe_present "run"      run      "cargo run --bin compass"

echo ""
echo "=== 2. default recipe == run (boundary: first recipe) ==="
just_dryrun
DRY_ERR_DEFAULT=$DRY_ERR
if [ "$DRY_RC" -eq 0 ] && [ "$DRY_ERR" = "cargo run --bin compass" ]; then
    verdict "'just -n' (no args) resolves to run ($DRY_ERR)" 1
else
    verdict "'just -n' (no args) must resolve to the first recipe == 'cargo run --bin compass'" 0 \
"--- got (rc=$DRY_RC) ---
$DRY_ERR"
fi
# default must NOT leak any compass-data pipeline command
if printf '%s\n' "$DRY_ERR" | grep -q 'compass-data'; then
    verdict "default recipe must not leak a compass-data pipeline command" 0 "$DRY_ERR"
else
    verdict "default recipe carries no compass-data pipeline command" 1
fi
# default and run must agree (only meaningful once a justfile resolves both)
just_dryrun run
DRY_ERR_RUN=$DRY_ERR
if [ -n "$DRY_ERR_DEFAULT" ] && [ -n "$DRY_ERR_RUN" ] && [ "$DRY_ERR_DEFAULT" = "$DRY_ERR_RUN" ]; then
    verdict "'just -n' default == 'just -n run'" 1
else
    if [ -z "$DRY_ERR_DEFAULT" ]; then
        verdict "'just -n' default == 'just -n run' (default unresolved: no justfile)" 0 "run=$DRY_ERR_RUN"
    else
        verdict "'just -n' default must equal 'just -n run'" 0 "default=$DRY_ERR_DEFAULT run=$DRY_ERR_RUN"
    fi
fi

echo ""
echo "=== 4. check gate = fmt --check + clippy + test, in order ==="
just_dryrun check
if [ "$DRY_RC" -ne 0 ]; then
    verdict "'just -n check' must resolve (rc=0)" 0 "$DRY_ERR"
else
    # Review fix (ref #265): all three line lookups must tolerate a missing
    # match (|| true) — under set -e -o pipefail a bare grep no-match would
    # silently abort the script with zero diagnostics instead of reaching the
    # clear FAIL verdict below when the check gate regresses.
    line_fmt=$(printf '%s\n' "$DRY_ERR" | grep -nFx 'cargo fmt -- --check' | head -1 | cut -d: -f1 || true)
    line_clippy=$(printf '%s\n' "$DRY_ERR" | grep -nFx 'cargo clippy -- -D warnings' | head -1 | cut -d: -f1 || true)
    line_test=$(printf '%s\n' "$DRY_ERR" | grep -nFx 'cargo test' | head -1 | cut -d: -f1 || true)
    if [ -n "$line_fmt" ] && [ -n "$line_clippy" ] && [ -n "$line_test" ] \
        && [ "$line_fmt" -lt "$line_clippy" ] && [ "$line_clippy" -lt "$line_test" ]; then
        verdict "'just -n check' gate order fmt($line_fmt) < clippy($line_clippy) < test($line_test)" 1
    else
        verdict "'just -n check' must run fmt BEFORE clippy BEFORE test; none may be dropped" 0 \
"--- dry-run (rc=$DRY_RC) ---
$DRY_ERR"
    fi
fi

echo ""
echo "=== 5. format discipline ==="
out=$(just --fmt --check 2>&1) && rc=0 || rc=$?
if [ "$rc" -eq 0 ]; then
    verdict "just --fmt --check passes" 1
else
    verdict "justfile must be properly formatted (just --fmt --check rc=0)" 0 "$out"
fi

echo ""
echo "=== 6. performance: 'just --list' must be fast (<= 5s) ==="
t0=$(date +%s)
timeout 5 just --list >/dev/null 2>&1 && prc=0 || prc=$?
t1=$(date +%s)
el=$((t1 - t0))
if [ "$prc" -eq 0 ] && [ "$el" -le 5 ]; then
    verdict "just --list completes in ${el}s (<=5s)" 1
else
    verdict "just --list must finish within 5s (took ${el}s, timeout rc=$prc)" 0
fi

echo ""
echo "=== 7. no-justfile distinguishability (RED root-cause probe) ==="
BLANK=$(mktemp -d)
trap 'rm -rf "$BLANK"' EXIT
(
    cd "$BLANK"
    just --list >/dev/null 2>err.txt && prc2=0 || prc2=$?
    if [ "$prc2" -ne 0 ] && grep -q 'no justfile found' err.txt; then
        echo "probe-ok"
    else
        echo "probe-bad rc=$prc2"
        cat err.txt
    fi
) | grep -q probe-ok
if [ "${PIPESTATUS[0]}" -eq 0 ]; then
    verdict "just in a justfile-less dir reports 'no justfile found' (rc!=0) — RED root cause is the missing file, not an assertion bug" 1
else
    verdict "probe: justfile-less dir must report 'no justfile found'" 0
fi

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL $N ADVERSARIAL CHECKS PASSED"
else
    echo "$N checks, SOME FAILED"
    exit "$FAIL"
fi
