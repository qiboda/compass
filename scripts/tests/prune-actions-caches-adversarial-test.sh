#!/usr/bin/env bash
# =============================================================================
# Adversarial RED tests for issue #353 (qiboda/compass):
#   scripts/prune-actions-caches.sh — auto-prune stale rust-cache entries
#
# Contract under test (from the approved plan; the production script does not
# exist yet — this file is the RED stage gate):
#   1. scripts/prune-actions-caches.sh is a bash script that can be `source`d:
#      library functions at the top, execution split via
#        if [ "${BASH_SOURCE[0]}" = "$0" ]; then main "$@"; fi
#   2. select_deletions <caches_json_array>:
#        - accepts one JSON array string; elements: id (number), ref (string),
#          key (string), created_at (ISO-8601 uppercase UTC string)
#        - stdout: one cache id per line, numerically ascending
#        - only entries with ref == "refs/heads/master" participate
#        - group key = token before the first '-' in `key`
#        - within a group: keep the entry with the largest created_at
#          (lexicographic == chronological for the ISO-8601 UTC format);
#          ties on created_at keep the largest id; the rest are deleted
#        - empty array -> empty stdout, exit 0; single-entry group -> nothing
#   3. main: list_caches paginates `gh api "/repos/$OWNER/$REPO/actions/caches?
#      per_page=100&page=<n>"` using total_count/actions_caches until
#      total_count is reached or an empty page appears; OWNER/REPO from
#      $GITHUB_REPOSITORY (owner/repo), default qiboda/compass; then
#      select_deletions; for each id: `gh api -X DELETE
#      "/repos/$OWNER/$REPO/actions/caches/<id>"`, a failed DELETE prints an
#      error and continues, final exit code non-zero if any failed
#      (fail-continue semantics).
#   4. DRY_RUN (non-empty = true): skip all DELETEs, print
#      "[DRY-RUN] would delete <id> <key>"; normal mode prints
#      "[DELETE] <id> <key>".
#   5. Compatible with `set -euo pipefail`; jq available.
#
# Attack dimensions covered (adversarial, NOT happy-path acceptance — that is
# skwy-requirement-test's job):
#   boundary values / invalid input / error paths / pagination data-source
#   mocking / performance / resource exhaustion / repeated-source state.
#
# Run from the repo root:
#   bash scripts/tests/prune-actions-caches-adversarial-test.sh
#
# Exit code 0 = all adversarial cases PASS; 1 = at least one FAIL (or RED:
# the production script is missing, which is exactly the expected state).
# Env override (same hook as the acceptance test): PRUNE_SCRIPT=<path> lets
# this suite run against a scratch implementation (green simulation).
#
# NOTE on contract extensions asserted here (flagged for the implementer):
#   * malformed top-level JSON / non-array JSON -> select_deletions must exit
#     non-zero (explicit failure; silently swallowing malformed data would
#     mask real API problems and is treated as a defect). This includes
#     EMPTY / whitespace-only input: jq treats it as "no program output"
#     with rc=0, so the caller must guard the empty-string case itself.
#   * per-entry malformed data (missing/non-numeric id, absent or null
#     ref/key/created_at, non-ISO created_at, non-object elements) -> the
#     entry is SKIPPED: it must never be deleted and must never displace the
#     legitimate newest entry of its group. No unknown id may be deleted.
#     id must be an integer >= 1: negative/zero ids are malformed too
#     (a negative id with the newest created_at must never win the group
#     competition and displace the legitimate newest entry).
#   * created_at accepts ISO-8601 with optional fractional seconds: the real
#     GitHub caches API returns microsecond precision, e.g.
#     2026-09-03T15:53:48.535638000Z. A whole-seconds-only regex rejects
#     every real cache entry and silently turns the prune into a no-op.
#   * main: a GITHUB_REPOSITORY value that is not a single "owner/repo"
#     (no slash, empty owner or repo, extra slashes) -> exit non-zero with
#     zero gh invocations.
#   * main: a list response that is not a well-formed object with a numeric
#     total_count (missing total_count / non-numeric) -> exit non-zero with
#     zero DELETEs. Never silently fall back to 0 (that would stop after
#     page 1 and under-collect).
#
# =============================================================================
set -uo pipefail

# Env override: PRUNE_SCRIPT=<path> (green simulation against a scratch impl)
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../.." && pwd)"
SCRIPT="${PRUNE_SCRIPT:-$REPO_ROOT/scripts/prune-actions-caches.sh}"
TEST_NAME="prune-actions-caches-adversarial"

FAILED=0
PASSED=0

PASS() {
    echo "PASS [${BASH_SOURCE[1]##*/}:${BASH_LINENO[0]}] $1"
    PASSED=$((PASSED + 1))
}
FAIL() {
    echo "FAIL [${BASH_SOURCE[1]##*/}:${BASH_LINENO[0]}] $1"
    FAILED=$((FAILED + 1))
}

# --- strict numeric comparison (non-negative ints, arbitrary length) --------
num_gt() { # returns 0 iff $1 > $2 (strictly)
    local a="$1" b="$2"
    a="${a#0}"; b="${b#0}"; a="${a:-0}"; b="${b:-0}"
    if [ "${#a}" -ne "${#b}" ]; then [ "${#a}" -gt "${#b}" ]; return; fi
    [ "$a" \> "$b" ]
}

# ---------------------------------------------------------------------------
# Planned adversarial cases (printed verbatim in RED pre-flight so the case
# inventory is visible even before the implementation exists)
# ---------------------------------------------------------------------------
plan_cases() {
    cat <<'EOF'
  P1 preflight_script_exists
  P2 preflight_sourcable_no_main_side_effects
  B1 edge_empty_array
  B2 edge_single_entry_single_group
  B3 edge_group_of_two
  B4 edge_five_groups_of_two
  B5 edge_hundred_same_group_reversed_ids
  B6 edge_created_at_tie_keep_max_id
  B7 edge_key_token_prefix_grouping
  B8 edge_key_without_dash_whole_token
  B9 edge_large_ids_near_2p53
  B10 edge_output_ascending_not_group_order
  B11 edge_created_at_fractional_seconds
  I1 invalid_missing_created_at_skipped
  I2 invalid_missing_id_skipped
  I3 invalid_missing_ref_skipped
  I4 invalid_null_fields_skipped
  I5 invalid_non_numeric_id_skipped
  I6 invalid_duplicate_ids_single_delete
  I7 invalid_wrong_refs_ignored
  I8 invalid_non_iso_created_at_never_misdeletes
  I9 invalid_malformed_json_exit_nonzero
  I10 invalid_json_non_object_elements
  I11 invalid_pretty_multiline_json_ok
  M1 main_pagination_150_two_pages
  M2 main_pagination_stop_on_empty_page
  M3 main_delete_failure_continues_then_nonzero
  M4 main_multiple_delete_failures_nonzero
  M5 main_list_failure_aborts_no_deletes
  M6 main_dry_run_skips_all_deletes_format
  M7 main_dry_run_truthy_zero_and_false
  M8 main_repo_parse_and_default_owner_repo
  M9 main_delete_output_format_normal_mode
  M10 main_invalid_repo_rejected
  M11 main_missing_total_count_aborts
  R1 perf_2000_entries_200_groups_sorted
  R2 oracle_python_crosscheck
  R3 resource_infinite_pagination_guard
  R4 resource_double_source_no_side_effects
EOF
}

PLANNED_COUNT=$(plan_cases | wc -l)

echo "======================================================================"
echo "ADVERSARIAL TESTS: $TEST_NAME (issue #353)"
echo "target: $SCRIPT"
echo "======================================================================"

# ---------------------------------------------------------------------------
# RED pre-flight: the production script does not exist yet. Missing = the
# RED state the plan requires; report every planned case as unrun and fail.
# ---------------------------------------------------------------------------
if [ ! -f "$SCRIPT" ]; then
    echo ""
    echo "RED: production script not found: $SCRIPT"
    echo "     Implementation is not present yet — every adversarial case"
    echo "     below is necessarily unmet (this is the expected RED state)."
    echo ""
    echo "Planned adversarial cases ($PLANNED_COUNT):"
    plan_cases
    echo ""
    echo "RESULT: RED — $PLANNED_COUNT planned cases unmet (script missing)"
    exit 1
fi

# ---------------------------------------------------------------------------
# Sandbox: fake `gh` in front of PATH. A real gh exists on this machine
# (/usr/bin/gh), so the fake is also the safety net: even if the production
# script's BASH_SOURCE guard is wrong and main() fires during `source`, the
# fake records the invocation instead of hitting GitHub.
# ---------------------------------------------------------------------------
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

FAKE_BIN="$TMP/bin"
mkdir -p "$FAKE_BIN"
cat > "$FAKE_BIN/gh" <<'FAKEGH'
#!/usr/bin/env bash
# Fake gh for adversarial tests. Configured via $FAKE_GH_DIR:
#   page-N.json   -> response body for `api .../actions/caches?...page=N`
#                    (missing file = empty page: total_count 0, no caches)
#   delete-fail   -> file with one failed DELETE id per line
#   fail-list     -> if present, list endpoint exits 1 (auth/network failure)
#   gh.log        -> one line per invocation (appended)
D="${FAKE_GH_DIR:?FAKE_GH_DIR is required}"
echo "$*" >> "$D/gh.log"
if [ "${1:-}" != "api" ]; then
    echo "fake gh: unsupported invocation: $*" >&2
    exit 1
fi
URL=""
for a in "$@"; do
    case "$a" in
        /repos/*) URL="$a" ;;
    esac
done
if [ -z "$URL" ]; then
    echo "fake gh: no API URL found in: $*" >&2
    exit 1
fi
if [[ "$URL" == */actions/caches/* ]]; then
    id="${URL##*/}"
    if [ -f "$D/delete-fail" ] && grep -qx "$id" "$D/delete-fail"; then
        echo "HTTP 500: DELETE failed for cache $id (fake)" >&2
        exit 1
    fi
    echo "{\"id\": $id}"
    exit 0
fi
if [ -f "$D/fail-list" ]; then
    echo "fake gh: HTTP 403 (list denied)" >&2
    exit 1
fi
page=1
# NB: must not match the substring "page=100" inside "per_page=100" —
# anchor on the query separator.
if [[ "$URL" =~ ([?&])page=([0-9]+) ]]; then
    page="${BASH_REMATCH[2]}"
fi
if [ -f "$D/page-$page.json" ]; then
    cat "$D/page-$page.json"
else
    echo '{"total_count": 0, "actions_caches": []}'
fi
exit 0
FAKEGH
chmod +x "$FAKE_BIN/gh"
export PATH="$FAKE_BIN:$PATH"

# --- tmp state shared by the call helpers -----------------------------------
CALL_RC=""
CALL_OUT=""
CALL_ERR=""

# stdout/rc pickled through files so `set -e` inside the target cannot
# kill the test harness and so we observe the real exit code.
call_select() { # $1 = path to a JSON array file
    local f="$1"
    ( set +e
      select_deletions "$(cat "$f")" > "$TMP/.sel.out" 2> "$TMP/.sel.err"
      echo $? > "$TMP/.sel.rc"
    )
    CALL_RC="$(cat "$TMP/.sel.rc")"
    CALL_OUT="$(cat "$TMP/.sel.out")"
    CALL_ERR="$(cat "$TMP/.sel.err")"
}

call_main() { # $1 = FAKE_GH_DIR; extra env (DRY_RUN / GITHUB_REPOSITORY) is
              # injected through MAIN_ENV inside the subshell
    local fdir="$1"
    # NOTE: main() ends with `exit` (contract: final non-zero exit). `exit` in
    # a function terminates the whole shell, so main must run inside a nested
    # subshell for its exit code to be observable.
    ( set +e
      unset DRY_RUN
      unset GITHUB_REPOSITORY
      eval "$MAIN_ENV"
      export FAKE_GH_DIR="$fdir"
      export PATH="$FAKE_BIN:$PATH"
      ( main ) > "$TMP/.main.out" 2> "$TMP/.main.err"
      echo $? > "$TMP/.main.rc"
    )
    CALL_RC="$(cat "$TMP/.main.rc")"
    CALL_OUT="$(cat "$TMP/.main.out")"
    CALL_ERR="$(cat "$TMP/.main.err")"
}

new_scenario() { # $1 = name -> prints a fresh FAKE_GH_DIR
    local s="$TMP/sc-$1"
    rm -rf "$s"
    mkdir -p "$s"
    : > "$s/gh.log"
    echo "$s"
}

# --- assertions -------------------------------------------------------------
assert_eq() { # desc expect actual
    local desc="$1" exp="$2" act="$3"
    if [ "$exp" = "$act" ]; then
        PASS "$desc"
    else
        FAIL "$desc (expected [<$exp>] got [<$act>])"
    fi
}

assert_sorted_uniq_numeric() { # desc output
    # every non-empty line must be a decimal integer in STRICTLY ascending
    # order, with no duplicate line (contract: id per line, numeric asc)
    local desc="$1" out="$2" prev="" line ok=1
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        if ! [[ "$line" =~ ^[0-9]+$ ]]; then
            FAIL "$desc: non-numeric output line <$line>"
            ok=0; break
        fi
        if [ -n "$prev" ]; then
            if num_gt "$line" "$prev"; then
                :
            elif [ "$line" = "$prev" ]; then
                FAIL "$desc: duplicate id $line in output"
                ok=0; break
            else
                FAIL "$desc: not strictly ascending: $prev -> $line"
                ok=0; break
            fi
        fi
        prev="$line"
    done <<< "$out"
    [ "$ok" = 1 ] && PASS "$desc: output numerically sorted, unique, all-numeric"
}

# json file helpers (python3 generates fixtures; jq stays production-only)
gen_pages() { # $1=dir $2=total $3=n_page1 $4=n_page2 $5=key_prefix
    python3 - "$1" "$2" "$3" "$4" "$5" <<'PY'
import json, os, sys
d, total, n1, n2, kp = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]

def arr(n, key, id_start):
    out = []
    for i in range(n):
        dt = f"2026-01-01T{i//60:02d}:{i%60:02d}:00Z"
        out.append({"id": id_start + i, "ref": "refs/heads/master",
                    "key": f"{key}-Linux-x64-{i:04x}", "created_at": dt})
    return out

p1 = arr(n1, kp, 1)
p2 = arr(n2, kp, 1 + n1)
with open(os.path.join(d, "page-1.json"), "w") as fh:
    json.dump({"total_count": total, "actions_caches": p1}, fh,
              separators=(",", ":"))
if n2 > 0:
    with open(os.path.join(d, "page-2.json"), "w") as fh:
        json.dump({"total_count": total, "actions_caches": p2}, fh,
                  separators=(",", ":"))
PY
}

# ===========================================================================
# Pre-flight
# ===========================================================================
echo ""
echo "== P1 preflight_script_exists =="
if [ -r "$SCRIPT" ]; then
    PASS "scripts/prune-actions-caches.sh exists and is readable"
else
    FAIL "scripts/prune-actions-caches.sh not readable"
fi

echo ""
echo "== P2 preflight_sourcable_no_main_side_effects =="
S0="$(new_scenario preflight)"
if grep -q 'BASH_SOURCE' "$SCRIPT"; then
    PASS "script contains a BASH_SOURCE guard (library/execution split)"
else
    FAIL "script has no BASH_SOURCE guard — sourcing it would run main()"
fi
# Sourcing must not invoke gh (guard wrong => fake log gets entries).
# fake gh is already first in PATH, so even a runaway main() is contained.
source "$SCRIPT" 2> "$TMP/.src.err" && :
set +e +u  # neutralize set -euo pollution the sourced library may introduce
if [ -s "$TMP/.src.err" ]; then
    FAIL "sourcing the script wrote to stderr: $(cat "$TMP/.src.err")"
else
    PASS "sourcing the script writes nothing to stderr"
fi
if [ "$(type -t select_deletions)" = "function" ]; then
    PASS "select_deletions is a function after sourcing"
else
    FAIL "select_deletions not defined after sourcing"
fi
if [ "$(type -t main)" = "function" ]; then
    PASS "main is a function after sourcing"
else
    FAIL "main not defined after sourcing"
fi
if [ -s "$S0/gh.log" ]; then
    FAIL "sourcing the script invoked gh (guard broken): $(cat "$S0/gh.log")"
else
    PASS "sourcing the script caused no gh invocations"
fi

# ===========================================================================
# A. Boundary values
# ===========================================================================

echo ""
echo "== B1 edge_empty_array =="
echo '[]' > "$TMP/b1.json"
call_select "$TMP/b1.json"
assert_eq "empty array -> empty output, exit 0" "0" "$CALL_RC"
assert_eq "empty array -> no deletion lines" "" "$CALL_OUT"

echo ""
echo "== B2 edge_single_entry_single_group =="
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-x","created_at":"2026-01-01T00:00:00Z"}]' > "$TMP/b2.json"
call_select "$TMP/b2.json"
assert_eq "single entry -> nothing to delete" "" "$CALL_OUT"

echo ""
echo "== B3 edge_group_of_two =="
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-02T00:00:00Z"}]' > "$TMP/b3.json"
call_select "$TMP/b3.json"
assert_eq "group of two keeps newest, deletes oldest" "1" "$CALL_OUT"

echo ""
echo "== B4 edge_five_groups_of_two =="
echo '[{"id":1,"ref":"refs/heads/master","key":"g0-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"g0-b","created_at":"2026-01-02T00:00:00Z"},{"id":3,"ref":"refs/heads/master","key":"g1-a","created_at":"2026-01-01T00:00:00Z"},{"id":4,"ref":"refs/heads/master","key":"g1-b","created_at":"2026-01-02T00:00:00Z"},{"id":5,"ref":"refs/heads/master","key":"g2-a","created_at":"2026-01-01T00:00:00Z"},{"id":6,"ref":"refs/heads/master","key":"g2-b","created_at":"2026-01-02T00:00:00Z"},{"id":7,"ref":"refs/heads/master","key":"g3-a","created_at":"2026-01-01T00:00:00Z"},{"id":8,"ref":"refs/heads/master","key":"g3-b","created_at":"2026-01-02T00:00:00Z"},{"id":9,"ref":"refs/heads/master","key":"g4-a","created_at":"2026-01-01T00:00:00Z"},{"id":10,"ref":"refs/heads/master","key":"g4-b","created_at":"2026-01-02T00:00:00Z"}]' > "$TMP/b4.json"
call_select "$TMP/b4.json"
assert_eq "five groups of two -> five deletions" "1
3
5
7
9" "$CALL_OUT"

echo ""
echo "== B5 edge_hundred_same_group_reversed_ids =="
# 100 entries in one group; created_at DESCENDS with id (id 100 oldest,
# id 1 newest). Keeping the newest MUST keep id 1, delete 2..100; output
# must be the remaining 99 ids in ASCENDING numeric order.
python3 - "$TMP/b5.json" <<'PY'
import json, sys
n = 100
out = []
for i in range(1, n + 1):
    # id 1 is NEWEST (k=99 -> ct 01:39), id 100 is OLDEST (k=0 -> ct 00:00):
    # created_at strictly descends with id, so kept entry = id 1,
    # deletion set = 2..100, which must come out ASCENDING.
    k = n - i
    ct = f"2026-06-01T{k//60:02d}:{k%60:02d}:00Z"
    out.append({"id": i, "ref": "refs/heads/master",
                "key": f"rust-Linux-x64-{i:04x}", "created_at": ct})
json.dump(out, open(sys.argv[1], "w"), separators=(",", ":"))
PY
call_select "$TMP/b5.json"
assert_sorted_uniq_numeric "100 same group, reversed id/ct order" "$CALL_OUT"
first_line="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | head -n1)"
last_line="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | tail -n1)"
line_count="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | wc -l)"
assert_eq "100-entry group deletes exactly 99" "99" "$line_count"
assert_eq "newest id 1 is kept (first output line is 2)" "2" "$first_line"
assert_eq "oldest id 100 is deleted (last line)" "100" "$last_line"
if printf '%s\n' "$CALL_OUT" | grep -qx '1'; then
    FAIL "id 1 (newest created_at) must be kept, but it appears in the deletion set"
else
    PASS "newest id 1 not in deletion set"
fi

echo ""
echo "== B6 edge_created_at_tie_keep_max_id =="
echo '[{"id":5,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":10,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-01T00:00:00Z"},{"id":7,"ref":"refs/heads/master","key":"rust-c","created_at":"2026-01-01T00:00:00Z"}]' > "$TMP/b6.json"
call_select "$TMP/b6.json"
assert_eq "identical created_at -> keep max id (10), delete 5,7" "5
7" "$CALL_OUT"
assert_sorted_uniq_numeric "tie-break output sorted" "$CALL_OUT"

echo ""
echo "== B7 edge_key_token_prefix_grouping =="
# key "rust-rust-Linux-x64-6ff13d87-8c853480" -> group "rust"; entries with
# different prefixes must be in different groups.
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-cargo-checksum","created_at":"2026-01-02T00:00:00Z"},{"id":3,"ref":"refs/heads/master","key":"node-rust-xyz","created_at":"2026-01-03T00:00:00Z"}]' > "$TMP/b7.json"
call_select "$TMP/b7.json"
# rust group: 1 (01-01) vs 2 (01-02) -> delete 1; node group single -> keep
assert_eq "prefix token grouping: rust group competes, node isolated" "1" "$CALL_OUT"

echo ""
echo "== B8 edge_key_without_dash_whole_token =="
# key without '-' has no separator; grouping must use the whole key as its
# own group (such keys are singletons unless duplicated).
echo '[{"id":1,"ref":"refs/heads/master","key":"rustcacheonly","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"nodecacheonly","created_at":"2026-01-01T00:00:00Z"},{"id":3,"ref":"refs/heads/master","key":"samekey","created_at":"2026-01-01T00:00:00Z"},{"id":4,"ref":"refs/heads/master","key":"samekey","created_at":"2026-01-02T00:00:00Z"}]' > "$TMP/b8.json"
call_select "$TMP/b8.json"
# rustcacheonly/nodecacheonly singletons -> kept; samekey pair -> delete id 3
assert_eq "no-dash keys group by whole key; duplicated key deletes older" "3" "$CALL_OUT"

echo ""
echo "== B9 edge_large_ids_near_2p53 =="
# ids below 2^53 (exact in jq/shell doubles) must round-trip exactly and
# compare numerically, not lexicographically.
echo '[{"id":999999999999999,"ref":"refs/heads/master","key":"zbig-a","created_at":"2026-01-01T00:00:00Z"},{"id":123456789012345,"ref":"refs/heads/master","key":"zbig-b","created_at":"2026-01-02T00:00:00Z"},{"id":9007199254740991,"ref":"refs/heads/master","key":"zbig-c","created_at":"2026-01-03T00:00:00Z"}]' > "$TMP/b9.json"
call_select "$TMP/b9.json"
assert_eq "large ids handled without precision/order loss" "123456789012345
999999999999999" "$CALL_OUT"

echo ""
echo "== B10 edge_output_ascending_not_group_order =="
# Output order must be numeric ascending globally, not group-processing order:
# group A deletes id 100, group B deletes id 2 -> output must be "2","100".
echo '[{"id":100,"ref":"refs/heads/master","key":"ga-a","created_at":"2026-01-01T00:00:00Z"},{"id":1,"ref":"refs/heads/master","key":"ga-b","created_at":"2026-01-02T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"gb-a","created_at":"2026-01-01T00:00:00Z"},{"id":99,"ref":"refs/heads/master","key":"gb-b","created_at":"2026-01-02T00:00:00Z"}]' > "$TMP/b10.json"
call_select "$TMP/b10.json"
assert_eq "output ascending (2 before 100) despite group order" "2
100" "$CALL_OUT"

echo ""
echo "== B11 edge_created_at_fractional_seconds =="
# GitHub Actions caches API returns microsecond-precision timestamps, e.g.
# 2026-09-03T15:53:48.535638000Z (regression F1: a whole-seconds-only ISO
# regex rejects every real cache entry -> silent no-op in production).
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-09-01T15:53:48.535638000Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-09-03T15:53:48.535638000Z"}]' > "$TMP/b11.json"
call_select "$TMP/b11.json"
assert_eq "fractional-second created_at accepted; stale id 1 deleted" "1" "$CALL_OUT"
assert_eq "fractional seconds -> exit 0" "0" "$CALL_RC"

# ===========================================================================
# B. Invalid input
# ===========================================================================

echo ""
echo "== I1 invalid_missing_created_at_skipped =="
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-02T00:00:00Z"},{"id":3,"ref":"refs/heads/master","key":"rust-c"}]' > "$TMP/i1.json"
call_select "$TMP/i1.json"
# Entry 3 has no created_at: skipped — never deleted, never treated as ""
# (which would defeat the legitimate newest entry).
assert_eq "missing created_at entry skipped (delete only id 1)" "1" "$CALL_OUT"
assert_eq "missing created_at -> exit 0" "0" "$CALL_RC"

echo ""
echo "== I2 invalid_missing_id_skipped =="
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-02T00:00:00Z"},{"ref":"refs/heads/master","key":"rust-c","created_at":"2026-01-03T00:00:00Z"}]' > "$TMP/i2.json"
call_select "$TMP/i2.json"
# id-less entry has no id to delete -> skip; must not crash on missing id.
assert_eq "missing id entry skipped (delete only id 1)" "1" "$CALL_OUT"
assert_eq "missing id -> exit 0" "0" "$CALL_RC"

echo ""
echo "== I3 invalid_missing_ref_skipped =="
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-02T00:00:00Z"},{"id":3,"key":"rust-c","created_at":"2026-01-03T00:00:00Z"}]' > "$TMP/i3.json"
call_select "$TMP/i3.json"
assert_eq "missing ref entry skipped (delete only id 1)" "1" "$CALL_OUT"

echo ""
echo "== I4 invalid_null_fields_skipped =="
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-02T00:00:00Z"},{"id":7,"ref":null,"key":"rust-c","created_at":"2026-01-03T00:00:00Z"},{"id":8,"ref":"refs/heads/master","key":null,"created_at":"2026-01-03T00:00:00Z"},{"id":9,"ref":"refs/heads/master","key":"rust-d","created_at":null},{"id":10,"ref":"refs/heads/master","key":"rust-e","created_at":"2026-01-03T00:00:00Z"}]' > "$TMP/i4.json"
call_select "$TMP/i4.json"
# null ref/key/created_at entries are malformed -> skipped, never deleted.
# Valid rust group: ids 1, 2, 10 -> keep 10 (newest), delete 1 and 2.
assert_eq "null fields skipped (delete only ids 1 and 2)" "1
2" "$CALL_OUT"
assert_eq "null fields -> exit 0" "0" "$CALL_RC"
if printf '%s\n' "$CALL_OUT" | grep -qxE '7|8|9'; then
    FAIL "null-field entry leaked into deletion set"
else
    PASS "null-field entries (7, 8, 9) never deleted"
fi

echo ""
echo "== I5 invalid_non_numeric_id_skipped =="
echo '[{"id":"abc","ref":"refs/heads/master","key":"rust-x","created_at":"2026-01-03T00:00:00Z"},{"id":1.5,"ref":"refs/heads/master","key":"rust-w","created_at":"2026-01-03T00:00:00Z"},{"id":true,"ref":"refs/heads/master","key":"rust-v","created_at":"2026-01-03T00:00:00Z"},{"id":13,"ref":"refs/heads/master","key":"rust-y","created_at":"2026-01-02T00:00:00Z"},{"id":14,"ref":"refs/heads/master","key":"rust-y","created_at":"2026-01-03T00:00:00Z"}]' > "$TMP/i5.json"
call_select "$TMP/i5.json"
# id "abc" / 1.5 / true are not integer ids -> skipped; 13 vs 14 -> delete 13.
assert_sorted_uniq_numeric "non-numeric ids never reach the output" "$CALL_OUT"
assert_eq "only the valid rust-y pair is evaluated (delete id 13)" "13" "$CALL_OUT"
# id <= 0 are malformed too: they must be skipped, never win their group
# (a negative id with the newest created_at must NOT displace the legitimate
# newest entry — regression from code review).
echo '[{"id":-5,"ref":"refs/heads/master","key":"rust-z","created_at":"2026-01-03T00:00:00Z"},{"id":0,"ref":"refs/heads/master","key":"rust-z","created_at":"2026-01-03T00:00:00Z"},{"id":1,"ref":"refs/heads/master","key":"rust-z","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-z","created_at":"2026-01-02T00:00:00Z"}]' > "$TMP/i5b.json"
call_select "$TMP/i5b.json"
assert_sorted_uniq_numeric "negative/zero ids: output safe" "$CALL_OUT"
assert_eq "negative/zero ids skipped; legitimate newest (2) kept -> delete 1" "1" "$CALL_OUT"

echo ""
echo "== I6 invalid_duplicate_ids_single_delete =="
# Same id appears twice with different created_at: a cache id must appear at
# most once in the deletion set (deduped even when both rows lose).
echo '[{"id":7,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-02T00:00:00Z"},{"id":8,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-01T00:00:00Z"},{"id":7,"ref":"refs/heads/master","key":"rust-c","created_at":"2026-01-03T00:00:00Z"},{"id":9,"ref":"refs/heads/master","key":"rust-d","created_at":"2026-01-04T00:00:00Z"}]' > "$TMP/i6.json"
call_select "$TMP/i6.json"
line_count="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | wc -l)"
assert_eq "duplicate id 7 deleted exactly once" "2" "$line_count"
assert_eq "duplicate id deduped: expected 7,8" "7
8" "$CALL_OUT"

echo ""
echo "== I7 invalid_wrong_refs_ignored =="
# Non-master refs must NEVER enter the deletion set, no matter how new they
# appear. Also covers refs/pull/N/merge, refs/tags and empty-string ref.
echo '[{"id":1,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"},{"id":2,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-01-02T00:00:00Z"},{"id":50,"ref":"refs/heads/dev-x","key":"rust-c","created_at":"2026-06-01T00:00:00Z"},{"id":51,"ref":"refs/pull/1/merge","key":"rust-d","created_at":"2026-06-01T00:00:00Z"},{"id":52,"ref":"refs/heads/release","key":"rust-e","created_at":"2026-06-01T00:00:00Z"},{"id":53,"ref":"","key":"rust-f","created_at":"2026-06-01T00:00:00Z"}]' > "$TMP/i7.json"
call_select "$TMP/i7.json"
assert_eq "non-master refs ignored (delete only id 1)" "1" "$CALL_OUT"
leaked=0
for bad in 50 51 52 53; do
    if printf '%s\n' "$CALL_OUT" | grep -qx "$bad"; then
        FAIL "ref entry $bad leaked into deletion set"
        leaked=1
    fi
done
[ "$leaked" = 0 ] && PASS "no non-master ref leaked into deletion set"

echo ""
echo "== I8 invalid_non_iso_created_at_never_misdeletes =="
# Non-ISO timestamps must never displace the legitimate newest entry of the
# group. Safe outcomes: skip the malformed entry (recommended) OR fail
# loudly — but never delete the true newest entry. Naive string comparison
# ("zzzz" > "2026...") would wrongly keep the malformed entry and delete
# the legitimate newest one.
echo '[{"id":100,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-06-01T00:00:00Z"},{"id":50,"ref":"refs/heads/master","key":"rust-b","created_at":"2026-05-01T00:00:00Z"},{"id":999,"ref":"refs/heads/master","key":"rust-c","created_at":"zzzz-not-a-date"},{"id":998,"ref":"refs/heads/master","key":"rust-d","created_at":"yesterday"},{"id":997,"ref":"refs/heads/master","key":"rust-e","created_at":"2026-06-01 10:00:00"}]' > "$TMP/i8.json"
call_select "$TMP/i8.json"
assert_sorted_uniq_numeric "non-ISO timestamps: output safe" "$CALL_OUT"
if printf '%s\n' "$CALL_OUT" | grep -qx '100'; then
    FAIL "legitimate newest entry (id 100) was deleted — malformed created_at displaced it"
else
    PASS "legitimate newest entry (id 100) not deleted despite malformed timestamps"
fi
# Skip semantics -> [50]; loud-failure semantics -> empty. Both acceptable;
# only the mis-deletion above is a defect.
if [ "$CALL_RC" != "0" ]; then
    PASS "non-ISO timestamps triggered explicit failure (also acceptable)"
elif [ "$CALL_OUT" = "50" ] || [ "$CALL_OUT" = "" ]; then
    PASS "non-ISO timestamps skipped; no mis-deletion"
else
    FAIL "unexpected deletion set with non-ISO timestamps: <$CALL_OUT>"
fi

echo ""
echo "== I9 invalid_malformed_json_exit_nonzero =="
printf '%s' '[{"id":1,"ref":"refs/heads/master"' > "$TMP/i9a.json"
call_select "$TMP/i9a.json"
if [ "$CALL_RC" != "0" ]; then
    PASS "truncated JSON -> non-zero exit (rc=$CALL_RC)"
else
    FAIL "truncated JSON silently accepted (rc=0) — must fail loudly"
fi
printf '%s' '[{"id":1},{"id":2},]' > "$TMP/i9b.json"
call_select "$TMP/i9b.json"
if [ "$CALL_RC" != "0" ]; then
    PASS "trailing-comma JSON -> non-zero exit (rc=$CALL_RC)"
else
    FAIL "trailing-comma JSON silently accepted (rc=0) — must fail loudly"
fi
printf '%s' '{"total_count":1,"actions_caches":[]}' > "$TMP/i9c.json"
call_select "$TMP/i9c.json"
if [ "$CALL_RC" != "0" ]; then
    PASS "non-array top-level JSON -> non-zero exit (rc=$CALL_RC)"
else
    FAIL "non-array top-level JSON silently accepted (rc=0) — must fail loudly"
fi
# Empty / whitespace-only JSON is malformed top-level data too. jq treats it
# as "no program output" with rc=0 — skipping the type check entirely — so
# the caller must guard the empty case itself (regression from code review).
for badge in 9d 9e; do
    if [ "$badge" = 9d ]; then
        printf '%s' '' > "$TMP/i$badge.json"
    else
        printf '%s' '   ' > "$TMP/i$badge.json"
    fi
    call_select "$TMP/i$badge.json"
    if [ "$CALL_RC" != "0" ]; then
        PASS "i$badge empty/blank JSON -> non-zero exit (rc=$CALL_RC)"
    else
        FAIL "i$badge empty/blank JSON silently accepted (rc=0) — must fail loudly"
    fi
done

echo ""
echo "== I10 invalid_json_non_object_elements =="
echo '[1,"x",null,{"id":5,"ref":"refs/heads/master","key":"rust-a","created_at":"2026-01-01T00:00:00Z"}]' > "$TMP/i10.json"
call_select "$TMP/i10.json"
# non-object elements are malformed -> skipped; the single valid entry is a
# group of one -> nothing deleted; no crash allowed.
assert_eq "non-object elements skipped; single valid entry kept" "" "$CALL_OUT"
assert_eq "non-object elements -> exit 0 (no crash)" "0" "$CALL_RC"

echo ""
echo "== I11 invalid_pretty_multiline_json_ok =="
# Pretty-printed multi-line JSON is still one JSON array string; a
# line-oriented implementation (while read) would get it wrong.
cat > "$TMP/i11.json" <<'EOF'
[
  {"id": 1, "ref": "refs/heads/master", "key": "rust-a", "created_at": "2026-01-01T00:00:00Z"},
  {"id": 2, "ref": "refs/heads/master", "key": "rust-b", "created_at": "2026-01-02T00:00:00Z"}
]
EOF
call_select "$TMP/i11.json"
assert_eq "pretty-printed multi-line JSON handled like single-line" "1" "$CALL_OUT"

# ===========================================================================
# C. main() integration with the fake `gh`
# ===========================================================================

echo ""
echo "== M1 main_pagination_150_two_pages =="
SM="$(new_scenario main1)"
gen_pages "$SM" 150 100 50 rust
MAIN_ENV="" call_main "$SM"
line_count="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | wc -l)"
# both pages use the "rust" prefix => one group of 150 -> keep 1, delete 149
assert_eq "150 entries (100+50, one group) -> 149 deletions" "149" "$line_count"
# every output line must be [DELETE] id key (normal mode)
bad="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | grep -Ev '^\[DELETE\] [0-9]+ ' | head -n1 || true)"
if [ -z "$bad" ]; then
    PASS "all output lines match [DELETE] <id> <key>"
else
    FAIL "unexpected output line: <$bad>"
fi
list_calls="$(grep -c 'actions/caches?.*page=' "$SM/gh.log" || true)"
assert_eq "pagination: exactly 2 list calls (150 => pages 1 and 2)" "2" "$list_calls"
if grep -q 'page=3' "$SM/gh.log"; then
    FAIL "fetched page 3 although total_count reached (wasted request)"
else
    PASS "no page=3 request after total_count reached"
fi
if grep -q 'per_page=100' "$SM/gh.log"; then
    PASS "list URL carries per_page=100"
else
    FAIL "list URL missing per_page=100"
fi
del_calls="$(grep -c 'DELETE.*actions/caches/' "$SM/gh.log" || true)"
assert_eq "one DELETE per deletion id" "149" "$del_calls"

echo ""
echo "== M2 main_pagination_stop_on_empty_page =="
SM="$(new_scenario main2)"
gen_pages "$SM" 100 100 0 rust
MAIN_ENV="" call_main "$SM"
list_calls="$(grep -c 'actions/caches?.*page=' "$SM/gh.log" || true)"
if [ "$list_calls" -ge 1 ] && [ "$list_calls" -le 2 ]; then
    PASS "pagination stops after the empty page (list calls=$list_calls)"
else
    FAIL "unexpected number of list calls: $list_calls"
fi
assert_eq "empty trailing page -> main exits cleanly" "0" "$CALL_RC"

echo ""
echo "== M3 main_delete_failure_continues_then_nonzero =="
SM="$(new_scenario main3)"
gen_pages "$SM" 3 3 0 rust     # one group of 3 -> keep id 3, delete ids 1,2
echo 1 > "$SM/delete-fail"     # DELETE id 1 fails (server error)
MAIN_ENV="" call_main "$SM"
if [ "$CALL_RC" != "0" ]; then
    PASS "delete failure propagates to non-zero exit (rc=$CALL_RC)"
else
    FAIL "delete failure swallowed — main exited 0 despite failed DELETE"
fi
if grep -q "DELETE.*actions/caches/2" "$SM/gh.log"; then
    PASS "DELETE id 2 still attempted after earlier failure (fail-continue)"
else
    FAIL "fail-continue broken: DELETE id 2 not attempted"
fi
if grep -q "DELETE.*actions/caches/3" "$SM/gh.log"; then
    FAIL "kept cache id 3 was deleted (selection/deletion mismatch)"
else
    PASS "kept cache id 3 not deleted"
fi
del_calls="$(grep -c 'DELETE.*actions/caches/' "$SM/gh.log" || true)"
assert_eq "both deletable ids attempted (1 failed, 1 succeeded)" "2" "$del_calls"
if printf '%s\n' "$CALL_OUT" "$CALL_ERR" | grep -qiE '(fail|error|500).*(cache|delete)|(cache|delete).*(fail|error)'; then
    PASS "failed DELETE reported (error visible in output/stderr)"
else
    FAIL "failed DELETE not reported: out=[$CALL_OUT] err=[$CALL_ERR]"
fi

echo ""
echo "== M4 main_multiple_delete_failures_nonzero =="
SM="$(new_scenario main4)"
gen_pages "$SM" 4 4 0 rust     # one group of 4 -> keep id 4, delete ids 1,2,3
printf '1\n3\n' > "$SM/delete-fail"
MAIN_ENV="" call_main "$SM"
if [ "$CALL_RC" != "0" ]; then
    PASS "multiple delete failures still end non-zero (rc=$CALL_RC)"
else
    FAIL "multiple delete failures swallowed — main exited 0"
fi
for id in 2 3; do
    if grep -q "DELETE.*actions/caches/$id" "$SM/gh.log"; then
        PASS "DELETE id $id attempted despite other failures"
    else
        FAIL "DELETE id $id not attempted"
    fi
done
if grep -q "DELETE.*actions/caches/4" "$SM/gh.log"; then
    FAIL "kept cache id 4 was deleted (selection/deletion mismatch)"
else
    PASS "kept cache id 4 not deleted"
fi
del_calls="$(grep -c 'DELETE.*actions/caches/' "$SM/gh.log" || true)"
assert_eq "all 3 deletable ids attempted (2 failed + 1 succeeded)" "3" "$del_calls"

echo ""
echo "== M5 main_list_failure_aborts_no_deletes =="
SM="$(new_scenario main5)"
gen_pages "$SM" 3 3 0 rust
: > "$SM/fail-list"
MAIN_ENV="" call_main "$SM"
if [ "$CALL_RC" != "0" ]; then
    PASS "list failure -> main exits non-zero (rc=$CALL_RC)"
else
    FAIL "list failure swallowed — main exited 0"
fi
del_calls="$(grep -c 'DELETE.*actions/caches/' "$SM/gh.log" || true)"
assert_eq "list failure -> zero DELETE calls (never delete on unknown state)" "0" "$del_calls"

echo ""
echo "== M6 main_dry_run_skips_all_deletes_format =="
SM="$(new_scenario main6)"
gen_pages "$SM" 3 3 0 rust
MAIN_ENV="DRY_RUN=1" call_main "$SM"
del_calls="$(grep -c 'DELETE.*actions/caches/' "$SM/gh.log" || true)"
assert_eq "DRY_RUN -> zero real DELETE calls" "0" "$del_calls"
bad="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | grep -Ev '^\[DRY-RUN\] would delete [0-9]+ ' | head -n1 || true)"
if [ -z "$bad" ]; then
    PASS "DRY_RUN output lines match [DRY-RUN] would delete <id> <key>"
else
    FAIL "DRY_RUN unexpected line: <$bad>"
fi
if [ "$CALL_RC" != "0" ]; then
    FAIL "DRY_RUN returned non-zero (rc=$CALL_RC) for a consistent dry run"
else
    PASS "DRY_RUN exits 0"
fi

echo ""
echo "== M7 main_dry_run_truthy_zero_and_false =="
# Contract: DRY_RUN non-empty means true — "0" and "false" are non-empty and
# must ALSO be treated as true (a `[ = "1" ]` comparison is a defect).
for v in 0 false; do
    SM="$(new_scenario m7-$v)"
    gen_pages "$SM" 3 3 0 rust
    MAIN_ENV="DRY_RUN=$v" call_main "$SM"
    del_calls="$(grep -c 'DELETE.*actions/caches/' "$SM/gh.log" || true)"
    assert_eq "DRY_RUN=$v (non-empty) -> zero DELETE calls" "0" "$del_calls"
    if printf '%s\n' "$CALL_OUT" | grep -q "\[DRY-RUN\] would delete"; then
        PASS "DRY_RUN=$v printed DRY-RUN lines"
    else
        FAIL "DRY_RUN=$v printed no DRY-RUN lines"
    fi
done

echo ""
echo "== M8 main_repo_parse_and_default_owner_repo =="
SM="$(new_scenario m8a)"
gen_pages "$SM" 1 1 0 rust
MAIN_ENV="GITHUB_REPOSITORY=custom-owner/custom-repo" call_main "$SM"
if grep -q '/repos/custom-owner/custom-repo/actions/caches' "$SM/gh.log"; then
    PASS "OWNER/REPO parsed from GITHUB_REPOSITORY"
else
    FAIL "GITHUB_REPOSITORY not honored: $(head -n1 "$SM/gh.log" 2>/dev/null)"
fi
SM="$(new_scenario m8b)"
gen_pages "$SM" 1 1 0 rust
MAIN_ENV="" call_main "$SM"   # GITHUB_REPOSITORY unset -> default
if grep -q '/repos/qiboda/compass/actions/caches' "$SM/gh.log"; then
    PASS "default OWNER/REPO qiboda/compass used when GITHUB_REPOSITORY unset"
else
    FAIL "default qiboda/compass not used: $(head -n1 "$SM/gh.log" 2>/dev/null)"
fi

echo ""
echo "== M9 main_delete_output_format_normal_mode =="
SM="$(new_scenario main9)"
gen_pages "$SM" 2 2 0 rust   # ids 1,2 same group -> delete 1, keep 2
MAIN_ENV="" call_main "$SM"
assert_eq "normal mode prints [DELETE] <id> <key>" "[DELETE] 1 rust-Linux-x64-0000" "$CALL_OUT"
assert_eq "normal mode exit 0" "0" "$CALL_RC"

echo ""
echo "== M10 main_invalid_repo_rejected =="
# GITHUB_REPOSITORY values that are not a single "owner/repo" must fail
# BEFORE any gh invocation (defensive: fail-closed on bad config).
for bad in "acme" "/repo" "owner/" "a/b/c" "owner//repo"; do
    SM="$(new_scenario "m10-${bad//\//-}")"
    gen_pages "$SM" 1 1 0 rust
    MAIN_ENV="GITHUB_REPOSITORY=$bad" call_main "$SM"
    if [ "$CALL_RC" != "0" ]; then
        PASS "GITHUB_REPOSITORY=$bad rejected (rc=$CALL_RC)"
    else
        FAIL "GITHUB_REPOSITORY=$bad accepted (must exit non-zero)"
    fi
    if [ -s "$SM/gh.log" ]; then
        FAIL "GITHUB_REPOSITORY=$bad still invoked gh: $(head -n1 "$SM/gh.log")"
    else
        PASS "GITHUB_REPOSITORY=$bad -> zero gh invocations"
    fi
done

echo ""
echo "== M11 main_missing_total_count_aborts =="
# A list response without a numeric total_count must abort before any DELETE:
# silently falling back to 0 would stop after page 1 and under-collect
# (regression from QA review F3).
SM="$(new_scenario m11)"
gen_pages "$SM" 1 1 0 rust
printf '%s' '{"actions_caches":[{"id":1,"ref":"refs/heads/master","key":"rust-Linux-x64-0000","created_at":"2026-01-01T00:00:00Z"}]}' > "$SM/page-1.json"
MAIN_ENV="" call_main "$SM"
if [ "$CALL_RC" != "0" ]; then
    PASS "list response without total_count aborts (rc=$CALL_RC)"
else
    FAIL "missing total_count accepted (rc=0) — must abort non-zero"
fi
if grep -q 'actions/caches/1' "$SM/gh.log" 2>/dev/null; then
    FAIL "DELETE was issued despite missing total_count"
else
    PASS "missing total_count -> zero DELETE invocations"
fi

# ===========================================================================
# D. Performance / resource exhaustion / state
# ===========================================================================

echo ""
echo "== R1 perf_2000_entries_200_groups_sorted =="
python3 - "$TMP/perf.json" <<'PY'
import json, sys
from datetime import datetime, timedelta
out = []
base = datetime(2026, 1, 1, 0, 0, 0)
for g in range(200):
    for i in range(10):
        ct = base + timedelta(days=g * 3, hours=i)
        out.append({"id": g * 10000 + i + 1, "ref": "refs/heads/master",
                    "key": f"g{g:03d}-runner-linux-{i:04x}",
                    "created_at": ct.strftime("%Y-%m-%dT%H:%M:%SZ")})
# large ids (below 2^53, exact in jq doubles) in one extra group of 2
out.append({"id": 999999999999999, "ref": "refs/heads/master", "key": "zbig-a",
            "created_at": "2026-06-01T00:00:00Z"})
out.append({"id": 123456789012345, "ref": "refs/heads/master", "key": "zbig-b",
            "created_at": "2026-06-02T00:00:00Z"})
json.dump(out, open(sys.argv[1], "w"), separators=(",", ":"))
PY
# Sourcing + calling inside guarded subshells with a hard timeout: a hang
# (infinite loop / O(n^2) blow-up) is a resource-exhaustion FAIL.
( set +e
  timeout 180 bash -c '
    source "$1" > /dev/null 2>&1 || exit 99
    select_deletions "$(cat "$2")" > "$3" 2> "$4"
    echo $? > "$5"
  ' _ "$SCRIPT" "$TMP/perf.json" "$TMP/.sel.out" "$TMP/.sel.err" "$TMP/.sel.rc"
  trc=$?
  if [ ! -f "$TMP/.sel.rc" ]; then
      # timeout killed the subshell before it could record rc (124)
      echo "$trc" > "$TMP/.sel.rc"
  fi
)
CALL_RC="$(cat "$TMP/.sel.rc")"
CALL_OUT="$(cat "$TMP/.sel.out")"
CALL_ERR="$(cat "$TMP/.sel.err")"
if [ "$CALL_RC" = "0" ]; then
    PASS "2000-entry selection completes (no hang/timeout)"
elif [ "$CALL_RC" = "124" ]; then
    FAIL "2000-entry selection hung (killed by 180s timeout — resource exhaustion)"
else
    FAIL "2000-entry selection failed (rc=$CALL_RC)"
fi
line_count="$(printf '%s\n' "$CALL_OUT" | sed '/^$/d' | wc -l)"
# 200 groups x 10 entries -> 200*9 deletions; extra 2-entry zbig group -> 1;
# total = 1800 + 1 = 1801
assert_eq "2000 entries / 201 groups -> 1801 deletions" "1801" "$line_count"
assert_sorted_uniq_numeric "2000-entry output sorted ascending, unique" "$CALL_OUT"

echo ""
echo "== R2 oracle_python_crosscheck =="
# Independent python3 oracle: fresh implementation of the contract from the
# plan, exercised on the SAME 2000-entry fixture. Any disagreement between
# the shell implementation and the oracle is a finding.
cat > "$TMP/oracle.py" <<'PY'
import json, re, sys
ISO = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$')
data = json.load(open(sys.argv[1]))
groups = {}
for e in data:
    if e.get("ref") != "refs/heads/master":
        continue
    if type(e.get("id")) is not int or e.get("id") < 1 \
       or type(e.get("key")) is not str \
       or type(e.get("created_at")) is not str or not ISO.match(e["created_at"]):
        continue
    tok = e["key"].split("-", 1)[0]
    groups.setdefault(tok, []).append(e)
deleted = set()
for es in groups.values():
    winner = max(es, key=lambda e: (e["created_at"], e["id"]))
    for e in es:
        if e is not winner:
            deleted.add(e["id"])
print("\n".join(str(i) for i in sorted(deleted)))
PY
orbit="$(python3 "$TMP/oracle.py" "$TMP/perf.json")"
mismatch="$(diff <(printf '%s\n' "$CALL_OUT") <(printf '%s\n' "$orbit") | head -n5 || true)"
if [ -z "$mismatch" ]; then
    PASS "select_deletions output identical to independent python3 oracle"
else
    FAIL "oracle mismatch (first diffs): $mismatch"
fi

echo ""
echo "== R3 resource_infinite_pagination_guard =="
# Server advertises total_count=100000 but page 2 onward is empty. A
# total_count-only loop would page forever; the contract's empty-page stop
# must terminate. Hard timeout: a hang is a FAIL (resource exhaustion).
SM="$(new_scenario main-r3)"
gen_pages "$SM" 100000 100 0 rust
MAIN_ENV="" timeout 60 bash -c '
    source "$1" > /dev/null 2>&1 || exit 98
    unset DRY_RUN; unset GITHUB_REPOSITORY
    export FAKE_GH_DIR="$2"; export PATH="$3:$PATH"
    main
' _ "$SCRIPT" "$SM" "$FAKE_BIN" > "$TMP/.r3.out" 2> "$TMP/.r3.err"
r3rc=$?
if [ "$r3rc" = "124" ]; then
    FAIL "pagination did not stop on the empty page (killed by timeout — infinite loop)"
else
    PASS "pagination terminated on the empty page despite inflated total_count (rc=$r3rc)"
fi
list_calls="$(grep -c 'actions/caches?.*page=' "$SM/gh.log" || true)"
if [ "$list_calls" -le 3 ]; then
    PASS "bounded number of list calls: $list_calls"
else
    FAIL "unbounded list calls: $list_calls (likely looping)"
fi

echo ""
echo "== R4 resource_double_source_no_side_effects =="
SM="$(new_scenario main-r4)"
before="$(wc -l < "$SM/gh.log" 2>/dev/null || echo 0)"
( set +e
  export FAKE_GH_DIR="$SM"
  export PATH="$FAKE_BIN:$PATH"
  source "$SCRIPT" > /dev/null 2>&1
  source "$SCRIPT" > /dev/null 2>&1
  if [ "$(type -t select_deletions)" != "function" ]; then exit 1; fi
  exit 0
)
srcrc=$?
if [ "$srcrc" != "0" ]; then
    FAIL "double sourcing the script broke the library functions"
else
    PASS "double sourcing keeps library functions intact"
fi
after="$(wc -l < "$SM/gh.log" 2>/dev/null || echo 0)"
if [ "$before" = "$after" ]; then
    PASS "double sourcing caused no gh side effects"
else
    FAIL "double sourcing invoked gh (side effects: $before -> $after lines)"
fi

# ===========================================================================
echo ""
echo "======================================================================"
if [ "$FAILED" -eq 0 ]; then
    echo "ALL $PASSED ADVERSARIAL CASES PASSED"
    exit 0
else
    echo "RESULT: $FAILED FAILED / $((FAILED + PASSED)) CASES — ADVERSARIAL GATE RED"
    exit 1
fi
