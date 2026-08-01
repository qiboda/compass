#!/bin/bash
# Tests for the pre-push hook's malformed-ref detection regex (ref #97).
# The hook lives in .githooks/pre-push (lines ~121-122); these tests mirror
# its two greps so a regression in word-boundary handling is caught without
# invoking the whole hook (which would run cargo/pytest).
# Run: scripts/tests/pre-push-ref-regex-test.sh
set -euo pipefail

FAIL=0

# Mirrors the hook's detection greps. grep exits 1 on no match — coerce to 0
# so the counting works under set -euo pipefail.
count_all()    { printf '%s\n' "$1" | grep -ciE '(^|[[:space:](])ref[[:space:]]+' || true; }
count_valid()  { printf '%s\n' "$1" | grep -ciE '(^|[[:space:](])ref[[:space:]]+#[0-9]+' || true; }

check() {
    local name="$1" msg="$2" expect_all="$3" expect_valid="$4"
    local a v
    a=$(count_all "$msg")
    v=$(count_valid "$msg")
    if [ "$a" -eq "$expect_all" ] && [ "$v" -eq "$expect_valid" ]; then
        echo "PASS: $name (all=$a valid=$v)"
    else
        echo "FAIL: $name (all=$a expected=$expect_all, valid=$v expected=$expect_valid)"
        FAIL=1
    fi
}

# 1. Legal ref #96 at line start — must count as valid, no malformed flag
check "legal ref #96 at line start" \
    "fix: something

ref #96" 1 1

# 2. Technical term --abbrev-ref followed by a word — must NOT count (regression: -ref was matched)
check "--abbrev-ref technical term ignored" \
    "ran 'git branch -D HEAD' (--abbrev-ref yields HEAD)" 0 0

# 3. --detect-terminal with ref-like segment — must NOT count
check "--detect-terminal technical term ignored" \
    "test 2 ran --detect-terminal unconditionally" 0 0

# 4. Mixed: term + legal ref — only the legal ref counts
check "term plus legal ref" \
    "ran --abbrev-ref yields HEAD

ref #96" 1 1

# 5. ref in parentheses followed by #N — counts as valid
check "ref #N in parentheses" \
    "Review (ref #96) found an issue" 1 1

# 6. 'refactored' / 'references' / 'refactor' — word fragment, must NOT count
check "refactored not counted" \
    "refactored the references in refactor mode" 0 0

# 6b. Regex/code fragment \<ref\> in prose — backslash prefix must NOT count
check "backslash-escaped ref fragment ignored" \
    "The check used \\<ref\\> word boundaries" 0 0

# 7. bare 'ref' at line end (no trailing space): neither regex counts it —
#    grep strips the newline, so [[:space:]]+ requires an inline space.
#    Matches old-hook behavior; not a regression.
check "bare ref line-end not counted (same as before)" \
    "some commit

ref

more text" 0 0

# 8. 'ref #97' mid-sentence — valid
check "ref #N mid-sentence" \
    "this references ref #97 in the middle" 1 1

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
