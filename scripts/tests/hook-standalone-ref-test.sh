#!/bin/bash
# Tests for the standalone-line ref #N extraction (ref #211).
#
# Background: commit-msg and pre-push hooks validated ALL "ref #N" occurrences
# (inline narrative mentions included) against the OPEN-issue check. A narrative
# mention of a CLOSED issue — e.g. "ref #154 lesson: ..." — was wrongly rejected.
#
# Fix (grill-me confirmed): only a ref reference that sits on its OWN line
# (the line contains nothing but the ref reference) counts as the commit's
# issue reference and is OPEN-validated. Inline "ref #N" = narrative mention,
# not validated.
#
# These tests mirror the hooks' extraction regex without invoking the whole
# hook (which would need gh + network + real issue states).
#
# Run: scripts/tests/hook-standalone-ref-test.sh
set -euo pipefail

FAIL=0

# Mirrors the hook's extraction greps. Returns the issue numbers that a hook
# would OPEN-validate, one per line, sorted unique.
#
# NOTE: keep in sync with .githooks/commit-msg and .githooks/pre-push.
extract_refs() {
    printf '%s\n' "$1" \
        | grep -iE '^[[:space:]]*ref[[:space:]]+#[0-9]+([[:space:]]*,[[:space:]]*#[0-9]+)*[[:space:]]*$' \
        | grep -oE '#[0-9]+' \
        | tr -d '#' \
        | sort -u \
        || true
}

# Mirrors the hooks' existence check: at least one standalone ref line present.
has_standalone_ref() {
    printf '%s\n' "$1" | grep -qiE '^[[:space:]]*ref[[:space:]]+#[0-9]+([[:space:]]*,[[:space:]]*#[0-9]+)*[[:space:]]*$'
}

check_extract() {
    local name="$1" msg="$2" expect="$3"
    local got
    got=$(extract_refs "$msg")
    if [ "$got" = "$expect" ]; then
        echo "PASS: $name (refs='$got')"
    else
        echo "FAIL: $name (got='$got', expected='$expect')"
        FAIL=1
    fi
}

check_has() {
    local name="$1" msg="$2" expect="$3"
    if has_standalone_ref "$msg" && [ "$expect" = "yes" ]; then
        echo "PASS: $name (standalone ref present)"
    elif ! has_standalone_ref "$msg" && [ "$expect" = "no" ]; then
        echo "PASS: $name (no standalone ref)"
    else
        echo "FAIL: $name (expected standalone=$expect)"
        FAIL=1
    fi
}

# --- Core: narrative mentions are NOT extracted, standalone refs ARE ---

# 1. The reported friction: narrative mention of a CLOSED issue (#154) inline,
#    plus a real standalone ref #210 on its own line. Only #210 must be extracted.
check_extract "narrative ref #154 inline + standalone ref #210" \
    "fix: sync csv output contracts

Follow the pattern from ref #154 lesson: run real-data smoke first.
ref #210" "210"

# 2. Standalone ref at end of body (the canonical form) — extracted.
check_extract "standalone ref #96 after blank line" \
    "fix: something

ref #96" "96"

# 3. Multiple standalone refs on separate lines — all extracted, sorted unique.
check_extract "two standalone refs on separate lines" \
    "fix: thing

ref #210
ref #96" "210
96"

# 4. Multiple refs comma-separated on one standalone line — both extracted.
check_extract "comma-separated refs on one standalone line" \
    "fix: thing

ref #210, #211" "210
211"

# 5. Standalone ref with leading/trailing whitespace — still a standalone line.
check_extract "standalone ref padded with spaces" \
    "fix: thing

   ref #210   " "210"

# 6. Inline ref mid-sentence (narrative) WITHOUT any standalone ref — nothing extracted.
check_extract "inline narrative ref only, no standalone" \
    "fix: sync

this references ref #97 in the middle of a sentence" ""

# 7. Inline ref in parentheses (narrative) + standalone ref — only standalone extracted.
check_extract "parenthesized narrative ref + standalone ref" \
    "fix: sync

Review (ref #96) found an issue; see ref #154 too.
ref #210" "210"

# 8. 'ref' at line start followed by prose — NOT a standalone ref (line has more than the ref).
check_extract "ref at line start followed by prose" \
    "fix: thing

ref #210 something else" ""

# 9. Uppercase "REF #N" — case-insensitive match (hooks use grep -i).
check_extract "uppercase REF #210" \
    "fix: thing

REF #210" "210"

# 10. CRLF line endings — \r is [[:space:]] so the standalone line still matches.
check_extract "CRLF line endings" \
    "$(printf 'fix: thing\r\n\r\nref #210\r\n')" "210"

# 11. Duplicate standalone refs across lines — sort -u dedupes to one.
check_extract "duplicate refs deduped" \
    "fix: thing

ref #210
ref #210" "210"

# 12. Trailing comma after ref — NOT a standalone ref (group requires #N after comma).
check_extract "trailing comma after ref" \
    "fix: thing

ref #210," ""

# --- Existence check (has_standalone_ref) ---

check_has "canonical standalone ref present" \
    "fix: thing

ref #210" yes

check_has "only inline narrative ref — no standalone" \
    "fix: thing

Lessons from ref #154: don't do X." no

check_has "parenthesized narrative only — no standalone" \
    "fix: thing

(see ref #119)" no

# --- Mirror-drift guard: the exact regex must be present in both hook files ---

REPO_ROOT=$(git -C "$(dirname "$0")/../.." rev-parse --show-toplevel 2>/dev/null || echo ".")
STANDALONE_REGEX='^[[:space:]]*ref[[:space:]]+#[0-9]+([[:space:]]*,[[:space:]]*#[0-9]+)*[[:space:]]*$'

for hook in "$REPO_ROOT/.githooks/commit-msg" "$REPO_ROOT/.githooks/pre-push"; do
    if grep -Fq "$STANDALONE_REGEX" "$hook"; then
        echo "PASS: mirror-drift guard — regex present in $(basename "$hook")"
    else
        echo "FAIL: mirror-drift guard — regex MISSING from $(basename "$hook")"
        FAIL=1
    fi
done

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
