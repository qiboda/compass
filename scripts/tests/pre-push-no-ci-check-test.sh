#!/bin/bash
# Tests for the pre-push hook's removal of the master-CI check (ref #172).
# The hook lives in .githooks/pre-push; this test asserts the CI-status block
# is GONE (it deadlocked fix-PR pushes — see .dsh/kb/dev/toolchain.md) while the
# fmt/clippy/doc/ref quality gates are PRESERVED (no accidental over-deletion).
# Follows the pre-push-ref-regex-test.sh precedent: a bash-only behavior test
# that mirrors the hook file instead of invoking it (which would run cargo).
# Run: scripts/tests/pre-push-no-ci-check-test.sh
set -euo pipefail

HOOK=".githooks/pre-push"
FAIL=0

[ -f "$HOOK" ] || { echo "FAIL: $HOOK missing"; exit 1; }

# --- 1. master-CI check block must be REMOVED -------------------------------
# Any of these markers means the deadlock-inducing CI check survived.
check_removed() {
    local name="$1" pattern="$2"
    if grep -qE "$pattern" "$HOOK"; then
        echo "FAIL: $name — marker still present: $pattern"
        FAIL=1
    else
        echo "PASS: $name (removed)"
    fi
}

check_removed "no gh run list --branch master" \
    'gh run list.*--branch master'
check_removed "no bare gh run usage (CI check's only source)" \
    'gh run'
check_removed "no CI_STATUS variable" \
    'CI_STATUS'
check_removed "no 'Fix CI before pushing' hint" \
    'Fix CI before pushing'
check_removed "no 'checking latest CI on master' echo" \
    'checking latest CI on master'

# --- 2. quality gates must be PRESERVED --------------------------------------
check_present() {
    local name="$1" pattern="$2"
    if grep -qE "$pattern" "$HOOK"; then
        echo "PASS: $name (preserved)"
    else
        echo "FAIL: $name — expected marker missing: $pattern"
        FAIL=1
    fi
}

check_present "cargo fmt gate"      'cargo fmt --check'
check_present "cargo clippy gate"   'cargo clippy -- -D warnings'
check_present "cargo doc gate"      'cargo doc --no-deps'
check_present "ref #N validation"   'gh issue list'
check_present "has_error exit path" 'has_error -ne 0'

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
