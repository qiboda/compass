#!/bin/bash
# Adversarial RED tests for issue #308: `scripts/sepa_daily.sh` rename to
# `scripts/update-database.sh` and the new auto-heal pipeline steps.
#
# These are intentionally destructive / error-path / rename-consistency tests:
#  - old name must be fully removed (no compatibility entry)
#  - new name must exist and be executable
#  - sync-investment-data.sh is the mandatory step 0 (missing => hard fail)
#  - stock_daily gap check is a hard failure, never silent/degraded
#  - Dolt commit failure propagates (no silent skip)
#  - idempotent re-run must not duplicate (static contract checks; the dynamic
#    mock harness lives in the renamed test-sepa-daily.sh after implementation)
#
# Run: bash scripts/tests/test-update-database-adversarial.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
NEW_SCRIPT="$PROJECT_ROOT/scripts/update-database.sh"
OLD_SCRIPT="$PROJECT_ROOT/scripts/sepa_daily.sh"
FAIL=0

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAIL=1; }

echo "--- 1. rename: old name removed, new name present/executable ---"
if [ -e "$OLD_SCRIPT" ]; then
    fail "old scripts/sepa_daily.sh must not exist (no compatibility entry)"
else
    pass "old scripts/sepa_daily.sh is gone"
fi
if [ -f "$NEW_SCRIPT" ] && [ -x "$NEW_SCRIPT" ]; then
    pass "scripts/update-database.sh exists and is executable"
else
    fail "scripts/update-database.sh missing or not executable"
fi

echo "--- 2. whole-repo old-name references (excluding .git/worktree internals) ---"
# The plan says the rename is thorough: no old-name references may remain in
# source, docs, or tests (a test file may legitimately assert the old name is
# gone; we skip the assertion itself by excluding files whose content only
# checks for absence).
if grep -rIl "sepa_daily\.sh" "$PROJECT_ROOT" \
        --exclude-dir=.git --exclude-dir=target --exclude-dir=tests \
        --exclude-dir=plans --exclude-dir=designs --exclude-dir=evidence \
        --exclude=handoff.md --exclude=reflections-archive.md \
        --exclude=test-update-database-adversarial.sh \
        2>/dev/null | grep -q .; then
    fail "repo still references old scripts/sepa_daily.sh"
else
    pass "no active source references old scripts/sepa_daily.sh"
fi

echo "--- 3. step 0: sync-investment-data.sh is the mandatory first pipeline step ---"
# `update-database.sh` must invoke scripts/sync-investment-data.sh before any
# cargo/import/collect step. This is a static contract; dynamic mock coverage
# belongs to the renamed test-sepa-daily.sh after the rename lands.
if [ -f "$NEW_SCRIPT" ] && grep -q "sync-investment-data\.sh" "$NEW_SCRIPT"; then
    pass "update-database.sh references sync-investment-data.sh"
else
    fail "update-database.sh does not reference scripts/sync-investment-data.sh"
fi

if [ -f "$NEW_SCRIPT" ]; then
    # sync-investment must be step 0: it must appear before the first cargo import.
    sync_line=$(grep -n "sync-investment-data\.sh" "$NEW_SCRIPT" | head -n1 | cut -d: -f1 || true)
    import_line=$(grep -n "cargo run --bin compass-data -- import" "$NEW_SCRIPT" | head -n1 | cut -d: -f1 || true)
    if [ -n "$sync_line" ] && [ -n "$import_line" ] && [ "$sync_line" -lt "$import_line" ]; then
        pass "sync-investment-data.sh runs before market import (step 0)"
    else
        fail "sync-investment-data.sh is not clearly before the import step"
    fi
else
    fail "cannot check step order: update-database.sh missing"
fi

echo "--- 4. sync-investment failure must be a hard abort ---"
if [ -f "$NEW_SCRIPT" ]; then
    # The script must not continue when the investment-data sync fails.
    if grep -qE "(sync-investment-data\.sh.*(exit|fail)|step 0 failed|run_step 0)" "$NEW_SCRIPT"; then
        pass "sync-investment failure path aborts loudly"
    else
        fail "sync-investment failure path is missing or silent"
    fi
else
    fail "cannot check sync failure path: update-database.sh missing"
fi

echo "--- 5. stock_daily gap check is a hard failure ---"
if [ -f "$NEW_SCRIPT" ]; then
    if grep -q "stock_daily" "$NEW_SCRIPT" && grep -qE "(exit 1|failed:.*stock_daily|stock_daily.*fail)" "$NEW_SCRIPT"; then
        pass "stock_daily gap check is a hard failure, not a warning"
    else
        fail "stock_daily gap check is missing or not hard-failing"
    fi
else
    fail "cannot check stock_daily gap check: update-database.sh missing"
fi

echo "--- 6. Dolt commit failure propagates ---"
if [ -f "$NEW_SCRIPT" ]; then
    # dolt_commit_changed/run_step style failures must exit non-zero.
    if grep -qE "(dolt commit.*exit 1|step [0-9]+ failed: dolt commit|dolt_commit_changed.*exit 1)" "$NEW_SCRIPT"; then
        pass "Dolt commit failure propagates"
    else
        fail "Dolt commit failure is not made a hard error"
    fi
else
    fail "cannot check Dolt commit failure: update-database.sh missing"
fi

echo "--- 7. idempotent re-run contract visible in script ---"
if [ -f "$NEW_SCRIPT" ]; then
    if grep -qE "(INSERT IGNORE|DELETE.*trade_date|idempotent|skip.*already|data up to date|max\(trade_date\))" "$NEW_SCRIPT"; then
        pass "script documents/idempotent constructs present"
    else
        fail "no visible idempotency mechanism in update-database.sh"
    fi
else
    fail "cannot check idempotency: update-database.sh missing"
fi

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL ADVERSARIAL SHELL TESTS PASSED"
else
    echo "SOME ADVERSARIAL SHELL TESTS FAILED"
    exit 1
fi
