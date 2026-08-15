#!/bin/bash
# Requirement acceptance tests for the justfile introduced in issue #265.
#
# Contract (issue #265, "core complete set"): a `justfile` at the repository
# root exposes the following recipes, whose command mappings must match the
# AGENTS.md "Commands" section exactly:
#   run     (default recipe, first recipe)  = cargo run --bin compass
#   build                                    = cargo build
#   test                                     = cargo test
#   fmt                                      = cargo fmt
#   clippy                                   = cargo clippy -- -D warnings
#   check                                    = fmt fmt-check + clippy + test gate
#   import                                   = cargo run --bin compass-data -- import
#   export                                   = cargo run --bin compass-data -- export
#   backup                                   = cargo run --bin compass-data -- backup
#
# Acceptance criteria (issue #265): justfile exists and `just --fmt --check`
# passes; `just` (no args) runs the GUI-starting command; `just --list` shows
# all recipes; every recipe command matches AGENTS.md Commands; `check`
# combines the full gate; regression coverage for recipe existence + mapping.
#
# Test approach (read-only — NO cargo/target operations, no GUI, no build):
#   * `just --fmt --check`   — validates justfile formatting only
#   * `just --list`          — enumerates recipes only
#   * `just -n <recipe>`     — dry-run: prints the command(s) that would run
#   * `just -n` (no recipe)  — dry-run of the default (first) recipe
# The dry-run (`-n`) output goes to STDERR (verified empirically against
# just 1.58.0), so assertions capture 2>&1. Assertions are exact (full-line
# grep -Fx), not substring-relaxed.
#
# Run: bash scripts/tests/justfile-test.sh
set -euo pipefail

REPO_ROOT=$(git -C "$(dirname "$0")/../.." rev-parse --show-toplevel 2>/dev/null || echo ".")
JUSTFILE="$REPO_ROOT/justfile"
JUST="${JUST:-just}"

FAIL=0

check() {   # $1=name $2=expect(0|nonzero) $3=rc
    local name="$1" expect="$2" rc="$3"
    if [ "$expect" = "0" ]; then
        if [ "$rc" -eq 0 ]; then echo "PASS: $name"; else echo "FAIL: $name — expected rc=0, got rc=$rc"; FAIL=1; fi
    else
        if [ "$rc" -ne 0 ]; then echo "PASS: $name (rc=$rc)"; else echo "FAIL: $name — expected reject (nonzero), got rc=0"; FAIL=1; fi
    fi
}

# dry_run: run `just -n <recipe>` and require its STDOUT+STDERR to exactly equal
# the expected multi-line output (full-line exact, no substring relaxation).
# $1=name $2=recipe(or empty for default) $3=expected-output
dry_run() {
    local name="$1" recipe="$2" expected="$3" got rc
    if [ -z "$recipe" ]; then
        got=$("$JUST" -f "$JUSTFILE" -n 2>&1) || rc=$?
    else
        got=$("$JUST" -f "$JUSTFILE" -n "$recipe" 2>&1) || rc=$?
    fi
    rc=${rc:-0}
    if [ "$rc" -eq 0 ] && [ "$got" = "$expected" ]; then
        echo "PASS: $name"
    else
        echo "FAIL: $name — dry-run mismatch"
        echo "  rc=$rc, expected:"
        printf '%s\n' "$expected" | sed 's/^/    /'
        echo "  got:"
        printf '%s\n' "$got" | sed 's/^/    /'
        FAIL=1
    fi
}

# Exactly one recipe name present in `just --list`.
# $1=name $2=recipe
recipe_in_list() {
    local name="$1" recipe="$2" list
    list=$("$JUST" -f "$JUSTFILE" --list 2>&1) || list="JUSTFILE_UNAVAILABLE (rc=$?)"
    if printf '%s\n' "$list" | grep -Fxq "    $recipe"; then
        echo "PASS: $name"
    else
        echo "FAIL: $name — recipe '$recipe' missing from 'just --list'"
        printf '%s\n' "$list" | sed 's/^/    /'
        FAIL=1
    fi
}

echo "=== issue #265: justfile requirement acceptance (RED — justfile not yet created) ==="

# --- happy path: `just --list` exposes all 9 recipes ---
for r in run build test fmt clippy check import export backup; do
    recipe_in_list "list: recipe '$r' present" "$r"
done

# --- default recipe: `just -n` (no args) runs the GUI command only ---
dry_run "default: \`just -n\` runs 'cargo run --bin compass'" "" "cargo run --bin compass"

# --- per-recipe exact command mapping (full-line exact) ---
dry_run "run: cargo run --bin compass" run "cargo run --bin compass"
dry_run "build: cargo build" build "cargo build"
dry_run "test: cargo test" test "cargo test"
dry_run "fmt: cargo fmt" fmt "cargo fmt"
dry_run "clippy: cargo clippy -- -D warnings" clippy "cargo clippy -- -D warnings"
dry_run "import: cargo run --bin compass-data -- import" import "cargo run --bin compass-data -- import"
dry_run "export: cargo run --bin compass-data -- export" export "cargo run --bin compass-data -- export"
dry_run "backup: cargo run --bin compass-data -- backup" backup "cargo run --bin compass-data -- backup"

# --- check: combines full gate in order fmt --check → clippy → test ---
dry_run "check: full gate (fmt check → clippy → test)" check "$(printf 'cargo fmt -- --check\ncargo clippy -- -D warnings\ncargo test')"

# --- `just --fmt --check` passes (formatting valid) ---
if "$JUST" -f "$JUSTFILE" --fmt --check >/dev/null 2>&1; then
    echo "PASS: \`just --fmt --check\` passes"
else
    echo "FAIL: \`just --fmt --check\` fails — justfile formatting invalid or absent"
    FAIL=1
fi

# --- basic error path: no justfile in a temp dir → `just --list` must fail ---
TMPDIR_X=$(mktemp -d)
if (cd "$TMPDIR_X" && "$JUST" --list >/dev/null 2>&1); then
    echo "FAIL: error-path — \`just --list\` in dir without justfile unexpectedly succeeded"
    FAIL=1
else
    echo "PASS: error-path — \`just --list\` fails in dir without justfile (distinguishes absent vs missing recipe)"
fi
rm -rf "$TMPDIR_X"

# --- boundary: `just --list` reveals >= 9 recipes (no silent merge/comment-out) ---
list=$("$JUST" -f "$JUSTFILE" --list 2>&1) || list="JUSTFILE_UNAVAILABLE (rc=$?)"
n=$(printf '%s\n' "$list" | grep -cE '^    .+$' || true)
if [ "$n" -ge 9 ]; then
    echo "PASS: \`just --list\` shows $n recipes (>= 9)"
else
    echo "FAIL: \`just --list\` shows $n recipes, expected >= 9 (recipes merged or commented out)"
    printf '%s\n' "$list" | sed 's/^/    /'
    FAIL=1
fi

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "ALL TESTS PASSED"
else
    echo "SOME TESTS FAILED"
    exit 1
fi
