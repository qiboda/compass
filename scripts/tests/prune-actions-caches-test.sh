#!/usr/bin/env bash
# =============================================================================
# Requirement acceptance tests — issue #353 (qiboda/compass)
#   "ci: 自动清理失效 rust-cache 缓存"
# Target: scripts/prune-actions-caches.sh (implemented in 24b805b; this suite
# runs against it — S17 is a regression guard added after multi-reviewer QA
# found the real caches API returns fractional-second timestamps).
# Location: scripts/tests/ (project shell-test convention — pairs with the
# target script; precedents: scripts/tests/test-update-database.sh 及
# check-coverage 等脚本配对的 *-test.sh / *-adversarial.sh)。
#
# Contract under test (per issue #353 design / BDD):
#   1. Sourcable: functions defined at top, bottom guard
#        `if [ "${BASH_SOURCE[0]}" = "$0" ]; then main "$@"; fi`.
#   2. select_deletions <caches_json_array>  (JSON array of {id, ref, key,
#      created_at, size_in_bytes}) prints one cache id per line (numeric asc):
#        - only entries with ref == "refs/heads/master" participate
#        - group key = token before the FIRST "-" of `key`
#          (rust-rust-Linux-x64-6ff13d87-8c853480 -> group "rust")
#        - per group: keep the entry with max created_at (lexicographic),
#          delete the rest
#        - created_at tie -> keep the larger id
#        - empty array -> no output
#   3. main: OWNER/REPO parsed from GITHUB_REPOSITORY (owner/repo), default
#      "qiboda/compass"; list_caches paginates
#        gh api "/repos/$OWNER/$REPO/actions/caches?per_page=100&page=<n>"
#      (total_count + actions_caches); then select; then per-id
#        gh api -X DELETE "/repos/$OWNER/$REPO/actions/caches/<id>"
#      continuing on failure, exiting non-zero at the end.
#   4. DRY_RUN non-empty: no DELETE issued; prints
#        [DRY-RUN] would delete <id> <key>
#      normal run prints [DELETE] <id> <key>.
#   5. bash `set -euo pipefail` compatible; jq available.
#
# BDD scenarios (S01..S16):
#   S01 script exists (RED gate: missing -> FAIL + exit 1)
#   S02 script parses (bash -n)
#   S03 prerequisite: jq available
#   S04 happy path: 2 groups x 2 caches (real issue keys, created_at differs)
#       -> exactly 2 deletions, one per group, correct ids, ascending
#   S05 all-hit: each group has exactly 1 cache -> no output
#   S06 non-master ref (refs/heads/dev-x, NEWEST) ignored
#   S07 created_at tie in a group -> keep larger id
#   S08 empty array -> no output
#   S09 single element -> no output
#   S10 sourcable: sourcing must NOT run main (no gh call, no output)
#   S11 DRY_RUN: prints [DRY-RUN] for both deletions, no DELETE call, rc 0
#   S12 GITHUB_REPOSITORY unset -> lists /repos/qiboda/compass/...
#   S13 GITHUB_REPOSITORY=acme/widgets -> lists /repos/acme/widgets/...
#   S14 main full chain, fake gh returns 2 pages (100+10 caches, 2 groups)
#       -> page=1 & page=2 list requests, 108 per-id DELETEs,
#          [DELETE] id set == select result (108 ids), rc 0
#   S15 DELETE failure on one id -> remaining DELETE still attempted,
#       final rc non-zero ("失败继续、最终非零退出")
#   S16 set -euo pipefail compatibility: source + select under strict mode
#
# Run: bash scripts/tests/prune-actions-caches-test.sh   (repo root)
# Env override: PRUNE_SCRIPT=<path> (green simulation against a scratch impl)
# =============================================================================
set -uo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../.." && pwd)"
SCRIPT="${PRUNE_SCRIPT:-$REPO_ROOT/scripts/prune-actions-caches.sh}"

PASS_N=0
FAIL_N=0

# ----------------------------------------------------------------------------
# assert helpers (self-contained; PASS/FAIL with caller line number)
# ----------------------------------------------------------------------------
assert_eq() { # $1 desc, $2 expected, $3 actual
    local desc="$1" exp="$2" got="$3" ln="${BASH_LINENO[0]}"
    if [ "$exp" = "$got" ]; then
        echo "  PASS: $desc [line $ln]"
        PASS_N=$((PASS_N + 1))
    else
        FAIL_N=$((FAIL_N + 1))
        echo "  FAIL: $desc [line $ln]"
        echo "        expected: [$(printf '%s' "$exp" | head -c 300)]"
        echo "        actual:   [$(printf '%s' "$got" | head -c 300)]"
    fi
}

assert_zero() { # $1 desc, $2 rc
    local desc="$1" rc="$2" ln="${BASH_LINENO[0]}"
    if [ "$rc" -eq 0 ]; then
        echo "  PASS: $desc [line $ln]"
        PASS_N=$((PASS_N + 1))
    else
        FAIL_N=$((FAIL_N + 1))
        echo "  FAIL: $desc [line $ln] (rc=$rc, expected 0)"
    fi
}

assert_nonzero() { # $1 desc, $2 rc
    local desc="$1" rc="$2" ln="${BASH_LINENO[0]}"
    if [ "$rc" -ne 0 ]; then
        echo "  PASS: $desc [line $ln] (rc=$rc)"
        PASS_N=$((PASS_N + 1))
    else
        FAIL_N=$((FAIL_N + 1))
        echo "  FAIL: $desc [line $ln] (rc=0, expected non-zero)"
    fi
}

assert_contains() { # $1 desc, $2 haystack, $3 needle
    local desc="$1" hay="$2" needle="$3" ln="${BASH_LINENO[0]}"
    if printf '%s' "$hay" | grep -Fq "$needle"; then
        echo "  PASS: $desc [line $ln]"
        PASS_N=$((PASS_N + 1))
    else
        FAIL_N=$((FAIL_N + 1))
        echo "  FAIL: $desc [line $ln]"
        echo "        needle:   [$needle]"
        echo "        haystack: [$(printf '%s' "$hay" | head -c 300)]"
    fi
}

# ----------------------------------------------------------------------------
# Test surroundings
# ----------------------------------------------------------------------------
TEST_TMP="$(mktemp -d)"
trap 'rm -rf "$TEST_TMP"' EXIT

FAKE_BIN="$TEST_TMP/bin"
mkdir -p "$FAKE_BIN"

# fake gh: records every call (one line per call, space-joined args) into
# $FAKE_GH_LOG; answers list calls from $FAKE_GH_RESPONSE_FILE (single page)
# or generates 2 pages when $FAKE_GH_TWO_PAGES=1; DELETE calls can be made to
# fail for one id via $FAKE_GH_FAIL_ID.
cat > "$FAKE_BIN/gh" <<'FAKEGH'
#!/bin/bash
printf '%s\n' "$*" >> "${FAKE_GH_LOG:?fake gh: FAKE_GH_LOG not set}"
if [ "${1:-}" != "api" ]; then
    echo "fake-gh: unexpected invocation: $*" >&2
    exit 1
fi
if [ "${2:-}" = "-X" ]; then
    # DELETE  /repos/<o>/<r>/actions/caches/<id>
    url="${4:-}"
    if [ -n "${FAKE_GH_FAIL_ID:-}" ] && printf '%s' "$url" | grep -Eq "caches/${FAKE_GH_FAIL_ID}\$"; then
        echo "fake-gh: HTTP 404 not found" >&2
        exit 1
    fi
    exit 0
fi
# list call: gh api "/repos/<o>/<r>/actions/caches?per_page=100&page=<n>"
url="${2:-}"
if [ -n "${FAKE_GH_TWO_PAGES:-}" ]; then
    case "$url" in
        *"page=1")
            printf '{"total_count": 110, "actions_caches": ['
            for i in $(seq 1000 1099); do
                [ "$i" -gt 1000 ] && printf ','
                printf '{"id": %s, "ref": "refs/heads/master", "key": "rust-rust-Linux-x64-6ff13d87-8c853480", "created_at": "2025-05-01T00:00:00Z", "size_in_bytes": 8900000000}' "$i"
            done
            printf ']}'
            echo
            ;;
        *"page=2")
            printf '{"total_count": 110, "actions_caches": ['
            for i in $(seq 2000 2009); do
                [ "$i" -gt 2000 ] && printf ','
                printf '{"id": %s, "ref": "refs/heads/master", "key": "bench-check-rust-Linux-x64-6ff13d87-8c853480", "created_at": "2025-05-02T00:00:00Z", "size_in_bytes": 8000000000}' "$i"
            done
            printf ']}'
            echo
            ;;
        *)
            echo "fake-gh: unexpected list url: $url" >&2
            exit 1
            ;;
    esac
    exit 0
fi
cat "${FAKE_GH_RESPONSE_FILE:?fake gh: FAKE_GH_RESPONSE_FILE not set}"
FAKEGH
chmod +x "$FAKE_BIN/gh"

# ----------------------------------------------------------------------------
# Fixtures (JSON compact; field set per contract: id/ref/key/created_at/size_in_bytes)
# ----------------------------------------------------------------------------
# Real issue #353 keys: rust job cache key rust-rust-Linux-x64-<envHash>-<hash>
#   (0b9fd15e = rustc 1.98.0, 6ff13d87 = rustc 1.98.1 — the stale ones)
# bench-check job key analogous: bench-check-rust-Linux-x64-<envHash>-<hash>
#   -> group token (first "-"-token): rust / bench.
JSON_HAPPY='[{"id":11,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":8900000000},{"id":12,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-04T00:00:00Z","size_in_bytes":8900000000},{"id":21,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2025-05-02T00:00:00Z","size_in_bytes":8000000000},{"id":22,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-03T00:00:00Z","size_in_bytes":8000000000}]'
JSON_ALLHIT='[{"id":1,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-03T00:00:00Z","size_in_bytes":1},{"id":2,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-03T00:00:00Z","size_in_bytes":1}]'
JSON_NONMASTER='[{"id":41,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":1},{"id":42,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-02T00:00:00Z","size_in_bytes":1},{"id":43,"ref":"refs/heads/dev-x","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-03T00:00:00Z","size_in_bytes":1}]'
JSON_TIE='[{"id":31,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":1},{"id":32,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":1},{"id":33,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2025-05-02T00:00:00Z","size_in_bytes":1},{"id":34,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-03T00:00:00Z","size_in_bytes":1}]'
JSON_EMPTY='[]'
JSON_SINGLE='[{"id":5,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":1}]'
JSON_MAIN4='[{"id":1,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":8900000000},{"id":2,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-02T00:00:00Z","size_in_bytes":8900000000},{"id":3,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2025-05-01T00:00:00Z","size_in_bytes":8000000000},{"id":4,"ref":"refs/heads/master","key":"bench-check-rust-Linux-x64-6ff13d87-8c853480","created_at":"2025-05-02T00:00:00Z","size_in_bytes":8000000000}]'

# ----------------------------------------------------------------------------
# Runners
# ----------------------------------------------------------------------------
# run_select: source the production script in a subshell and call
# select_deletions. Fake gh + DRY_RUN guard against a broken guard running
# main during source (must never touch the real repository caches).
run_select() { # $1 json, $2 log-tag; stdout = select output
    DRY_RUN=1 PATH="$FAKE_BIN:$PATH" \
        FAKE_GH_LOG="$TEST_TMP/gh_$2.log" \
        FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_$2.json" \
        bash -c 'source "$1" && select_deletions "$2"' _ "$SCRIPT" "$1" \
          2>"$TEST_TMP/sel_$2.err"
}

# ids of every DELETE actually issued by main (from fake gh log)
deleted_ids_from_log() { # $1 logfile
    grep '^api -X DELETE ' "$1" 2>/dev/null \
        | sed -n 's#.*/caches/\([0-9][0-9]*\)$#\1#p' | sort -n -u
}

# ids printed by main as [DELETE] <id> <key>
deleted_ids_from_stdout() { # $1 stdout file
    sed -n 's#^\[DELETE\] \([0-9][0-9]*\) .*#\1#p' "$1" | sort -n -u
}

# ----------------------------------------------------------------------------
# S01 — script exists (RED gate)
# ----------------------------------------------------------------------------
echo "== [S01] 生产脚本存在（RED 门禁）: $SCRIPT =="
if [ ! -f "$SCRIPT" ]; then
    echo "  FAIL: scripts/prune-actions-caches.sh 不存在（RED——实现尚未交付） [line ${LINENO}]"
    echo "        > 生产脚本按 issue #353 契约实现后，本文件将作为其验收测试运行。"
    echo "        > 其余 S02..S16 依赖该脚本，已跳过。"
    FAIL_N=$((FAIL_N + 1))
    echo ""
    echo "=== 汇总: PASS=$PASS_N FAIL=$FAIL_N (RED: 脚本缺失) ==="
    exit 1
fi
echo "  PASS: $SCRIPT 存在 [line ${LINENO}]"
PASS_N=$((PASS_N + 1))

# ----------------------------------------------------------------------------
# S02 — script parses
# ----------------------------------------------------------------------------
echo "== [S02] 脚本语法 (bash -n) =="
if bash -n "$SCRIPT" 2>"$TEST_TMP/syntax.err"; then
    echo "  PASS: bash -n $SCRIPT 通过 [line ${LINENO}]"
    PASS_N=$((PASS_N + 1))
else
    echo "  FAIL: bash -n $SCRIPT 报语法错误 [line ${LINENO}]"
    sed 's/^/        /' "$TEST_TMP/syntax.err"
    FAIL_N=$((FAIL_N + 1))
fi

# ----------------------------------------------------------------------------
# S03 — jq prerequisite
# ----------------------------------------------------------------------------
echo "== [S03] 前置：jq 可用 =="
if command -v jq >/dev/null 2>&1; then
    echo "  PASS: jq 可用 ($(jq --version 2>/dev/null || echo '?')) [line ${LINENO}]"
    PASS_N=$((PASS_N + 1))
else
    echo "  FAIL: jq 不在 PATH（契约前置缺失，无法验证） [line ${LINENO}]"
    FAIL_N=$((FAIL_N + 1))
fi

# ----------------------------------------------------------------------------
# S04 — happy path: 2 groups x 2 caches (real keys), created_at differs
#       rust {11 old, 12 new}, bench {21 old, 22 new} -> delete 11, 21
# ----------------------------------------------------------------------------
echo "== [S04] happy path: 2 组 x 2 份（真实 key；created_at 不同）-> 各保留最新 1 份 =="
out="$(run_select "$JSON_HAPPY" happy)"; rc=$?
assert_zero "S04 select_deletions 退出码 0" "$rc"
assert_eq "S04 输出恰好 2 个 id（11, 21，升序）" $'11\n21' "$out"

# ----------------------------------------------------------------------------
# S05 — all-hit: each group exactly 1 cache -> no output
# ----------------------------------------------------------------------------
echo "== [S05] 全命中：各组恰好 1 份 -> 无输出 =="
out="$(run_select "$JSON_ALLHIT" allhit)"; rc=$?
assert_zero "S05 select_deletions 退出码 0" "$rc"
assert_eq "S05 输出为空" "" "$out"

# ----------------------------------------------------------------------------
# S06 — non-master ref ignored, even if newest
# ----------------------------------------------------------------------------
echo "== [S06] 非 master ref（refs/heads/dev-x，最新）被忽略 =="
out="$(run_select "$JSON_NONMASTER" nonmaster)"; rc=$?
assert_zero "S06 select_deletions 退出码 0" "$rc"
assert_eq "S06 仅删 master 旧份 41（dev-x 的 43 不参与）" "41" "$out"

# ----------------------------------------------------------------------------
# S07 — created_at tie -> keep larger id (per group)
# ----------------------------------------------------------------------------
echo "== [S07] 同组同 created_at -> 保留 id 大者 =="
out="$(run_select "$JSON_TIE" tie)"; rc=$?
assert_zero "S07 select_deletions 退出码 0" "$rc"
assert_eq "S07 删 31（rust 同刻保留 32）与 33（bench 保留 34）" $'31\n33' "$out"

# ----------------------------------------------------------------------------
# S08 — empty array -> no output
# ----------------------------------------------------------------------------
echo "== [S08] 空数组 -> 无输出 =="
out="$(run_select "$JSON_EMPTY" empty)"; rc=$?
assert_zero "S08 select_deletions 退出码 0" "$rc"
assert_eq "S08 输出为空" "" "$out"

# ----------------------------------------------------------------------------
# S09 — single element -> no output
# ----------------------------------------------------------------------------
echo "== [S09] 单元素 -> 无输出 =="
out="$(run_select "$JSON_SINGLE" single)"; rc=$?
assert_zero "S09 select_deletions 退出码 0" "$rc"
assert_eq "S09 输出为空" "" "$out"

# ----------------------------------------------------------------------------
# S10 — sourcable: source must NOT run main (no gh call, no output)
# ----------------------------------------------------------------------------
echo "== [S10] source 复用：source 不触发 main =="
slog="$TEST_TMP/s10_gh.log"
: > "$slog"   # log must exist even if fake gh is never invoked (correct guard)
sout="$(DRY_RUN=1 PATH="$FAKE_BIN:$PATH" \
    FAKE_GH_LOG="$slog" FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_s10.json" \
    bash -c 'source "$1"; echo "SOURCED_OK"' _ "$SCRIPT" 2>"$TEST_TMP/s10.err")"; rc=$?
assert_zero "S10 source 退出码 0" "$rc"
assert_eq "S10 source 无意外输出" "SOURCED_OK" "$sout"
n_gh="$(grep -c '^api ' "$slog" 2>/dev/null || true)"
assert_eq "S10 source 期间零次 gh 调用" "0" "$n_gh"

# ----------------------------------------------------------------------------
# Fake gh single-page response for main-flow tests (4 caches, 2 groups).
# NOTE: must be the FULL GitHub API envelope {total_count, actions_caches} —
# the contract lists total_count + actions_caches, not a bare array.
printf '{"total_count": 4, "actions_caches": %s}\n' "$JSON_MAIN4" > "$TEST_TMP/fixture_main.json"

# ----------------------------------------------------------------------------
# S11 — DRY_RUN: print [DRY-RUN] ... for both deletions, no DELETE call, rc 0
# ----------------------------------------------------------------------------
echo "== [S11] DRY_RUN: 打印且不执行 DELETE =="
m11_log="$TEST_TMP/s11_gh.log"
m11_out="$TEST_TMP/s11_out.txt"
DRY_RUN=1 PATH="$FAKE_BIN:$PATH" \
    FAKE_GH_LOG="$m11_log" FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_main.json" \
    bash "$SCRIPT" >"$m11_out" 2>"$TEST_TMP/s11_err.txt"; rc=$?
assert_zero "S11 DRY_RUN 退出码 0" "$rc"
n_dry="$(grep -c '^\[DRY-RUN\] would delete ' "$m11_out" || true)"
assert_eq "S11 恰好 2 行 [DRY-RUN] would delete" "2" "$n_dry"
assert_contains "S11 行: [DRY-RUN] would delete 1 rust-rust-Linux-x64-0b9fd15e-8c853480" \
    "$(cat "$m11_out")" "[DRY-RUN] would delete 1 rust-rust-Linux-x64-0b9fd15e-8c853480"
assert_contains "S11 行: [DRY-RUN] would delete 3 bench-check-rust-Linux-x64-0b9fd15e-8c853480" \
    "$(cat "$m11_out")" "[DRY-RUN] would delete 3 bench-check-rust-Linux-x64-0b9fd15e-8c853480"
n_del="$(grep -c '^api -X DELETE ' "$m11_log" 2>/dev/null || true)"
assert_eq "S11 零次 DELETE 调用" "0" "$n_del"
n_list="$(grep -c '^api /repos/' "$m11_log" 2>/dev/null || true)"
assert_eq "S11 仅 1 次 list 调用（单页）" "1" "$n_list"

# ----------------------------------------------------------------------------
# S12 — GITHUB_REPOSITORY unset -> default qiboda/compass
# ----------------------------------------------------------------------------
echo "== [S12] GITHUB_REPOSITORY 缺省 -> qiboda/compass =="
m12_log="$TEST_TMP/s12_gh.log"
env -u GITHUB_REPOSITORY DRY_RUN=1 PATH="$FAKE_BIN:$PATH" \
    FAKE_GH_LOG="$m12_log" FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_main.json" \
    bash "$SCRIPT" >/dev/null 2>"$TEST_TMP/s12_err.txt"; rc=$?
assert_zero "S12 缺省仓库时退出码 0" "$rc"
assert_contains "S12 缺省解析为 qiboda/compass" "$(cat "$m12_log")" \
    "api /repos/qiboda/compass/actions/caches?per_page=100&page=1"

# ----------------------------------------------------------------------------
# S13 — GITHUB_REPOSITORY=acme/widgets -> parsed owner/repo
# ----------------------------------------------------------------------------
echo "== [S13] GITHUB_REPOSITORY 传入 -> acme/widgets =="
m13_log="$TEST_TMP/s13_gh.log"
env GITHUB_REPOSITORY=acme/widgets DRY_RUN=1 PATH="$FAKE_BIN:$PATH" \
    FAKE_GH_LOG="$m13_log" FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_main.json" \
    bash "$SCRIPT" >/dev/null 2>"$TEST_TMP/s13_err.txt"; rc=$?
assert_zero "S13 传入仓库时退出码 0" "$rc"
assert_contains "S13 解析为 acme/widgets" "$(cat "$m13_log")" \
    "api /repos/acme/widgets/actions/caches?per_page=100&page=1"

# ----------------------------------------------------------------------------
# S14 — main full chain: fake gh returns 2 pages (100 rust + 10 bench)
#       expect page=1 + page=2 list, 108 per-id DELETEs, [DELETE] set == select
# ----------------------------------------------------------------------------
echo "== [S14] main 全链路：2 页分页 + 108 条 DELETE，与 select 一致 =="
m14_log="$TEST_TMP/s14_gh.log"
m14_out="$TEST_TMP/s14_out.txt"
DRY_RUN= PATH="$FAKE_BIN:$PATH" FAKE_GH_TWO_PAGES=1 \
    FAKE_GH_LOG="$m14_log" \
    bash "$SCRIPT" >"$m14_out" 2>"$TEST_TMP/s14_err.txt"; rc=$?
assert_zero "S14 main 退出码 0" "$rc"
p1="$(grep -c 'per_page=100&page=1' "$m14_log" || true)"
p2="$(grep -c 'per_page=100&page=2' "$m14_log" || true)"
assert_eq "S14 发出 page=1 请求" "1" "$p1"
assert_eq "S14 发出 page=2 请求（分页）" "1" "$p2"
n_del="$(grep -c '^api -X DELETE ' "$m14_log" || true)"
assert_eq "S14 恰好 108 条 DELETE 请求" "108" "$n_del"
{
    for i in $(seq 1000 1098) $(seq 2000 2008); do echo "$i"; done
} | sort -n -u > "$TEST_TMP/s14_expected_ids.txt"
deleted_ids_from_log "$m14_log" > "$TEST_TMP/s14_actual_ids.txt"
if diff -u "$TEST_TMP/s14_expected_ids.txt" "$TEST_TMP/s14_actual_ids.txt" > "$TEST_TMP/s14_ids.diff" 2>&1; then
    echo "  PASS: S14 删除 id 集合与 select 一致（108 个） [line ${LINENO}]"
    PASS_N=$((PASS_N + 1))
else
    FAIL_N=$((FAIL_N + 1))
    echo "  FAIL: S14 删除 id 集合与 select 不一致（1099/2009 应保留、其余应删） [line ${LINENO}]"
    sed 's/^/        /' "$TEST_TMP/s14_ids.diff" | head -20
fi
n_del_out="$(grep -c '^\[DELETE\] ' "$m14_out" || true)"
assert_eq "S14 stdout 恰好 108 行 [DELETE]" "108" "$n_del_out"
deleted_ids_from_stdout "$m14_out" > "$TEST_TMP/s14_out_ids.txt"
if diff -u "$TEST_TMP/s14_expected_ids.txt" "$TEST_TMP/s14_out_ids.txt" > "$TEST_TMP/s14_out_ids.diff" 2>&1; then
    echo "  PASS: S14 [DELETE] id 集合与 select 一致 [line ${LINENO}]"
    PASS_N=$((PASS_N + 1))
else
    FAIL_N=$((FAIL_N + 1))
    echo "  FAIL: S14 [DELETE] id 集合与 select 不一致 [line ${LINENO}]"
    sed 's/^/        /' "$TEST_TMP/s14_out_ids.diff" | head -20
fi

# ----------------------------------------------------------------------------
# S15 — DELETE failure on id 1: remaining delete still attempted, rc != 0
# ----------------------------------------------------------------------------
echo "== [S15] DELETE 失败继续：id 1 失败，id 3 仍删除，最终非零退出 =="
m15_log="$TEST_TMP/s15_gh.log"
m15_out="$TEST_TMP/s15_out.txt"
env PATH="$FAKE_BIN:$PATH" DRY_RUN= FAKE_GH_FAIL_ID=1 \
    FAKE_GH_LOG="$m15_log" FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_main.json" \
    bash "$SCRIPT" >"$m15_out" 2>"$TEST_TMP/s15_err.txt"; rc=$?
assert_nonzero "S15 最终退出码非零" "$rc"
n_del="$(grep -c '^api -X DELETE ' "$m15_log" || true)"
assert_eq "S15 两条 DELETE 均被尝试（失败不中断）" "2" "$n_del"
assert_contains "S15 失败的 id 1 与成功的 id 3 均出现在 DELETE 日志" "$(cat "$m15_log")" \
    "/repos/qiboda/compass/actions/caches/3"
assert_contains "S15 stdout 含成功删除的 [DELETE] 3 ..." "$(cat "$m15_out")" \
    "[DELETE] 3 bench-check-rust-Linux-x64-0b9fd15e-8c853480"

# ----------------------------------------------------------------------------
# S16 — set -euo pipefail compatibility (source + select under strict mode)
# ----------------------------------------------------------------------------
echo "== [S16] set -euo pipefail 兼容 =="
s16_out="$(PATH="$FAKE_BIN:$PATH" DRY_RUN=1 \
    FAKE_GH_LOG="$TEST_TMP/s16_gh.log" \
    FAKE_GH_RESPONSE_FILE="$TEST_TMP/fixture_s16.json" \
    bash -c 'set -euo pipefail; source "$1"; select_deletions "$2"' _ "$SCRIPT" "$JSON_HAPPY" \
    2>"$TEST_TMP/s16.err")"; rc=$?
assert_zero "S16 严格模式下 source+select 退出码 0" "$rc"
assert_eq "S16 严格模式输出与 S04 一致" $'11\n21' "$s16_out"

# ----------------------------------------------------------------------------
# S17 — real GitHub API timestamp fidelity (regression; added after QA review
#       finding F1): the caches API returns created_at with microsecond
#       fractional seconds, e.g. 2026-09-03T15:53:48.535638000Z. The ISO
#       validation must accept them — a whole-seconds-only regex rejects every
#       real caches entry, silently turning the prune into a no-op.
# ----------------------------------------------------------------------------
echo "== [S17] 真实 API 时间戳（小数秒微秒精度）fidelity =="
JSON_FRACTIONAL='[{"id":1,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-0b9fd15e-8c853480","created_at":"2026-09-01T15:53:48.535638000Z","size_in_bytes":8900000000},{"id":2,"ref":"refs/heads/master","key":"rust-rust-Linux-x64-6ff13d87-8c853480","created_at":"2026-09-03T15:53:48.535638000Z","size_in_bytes":8900000000}]'
out="$(run_select "$JSON_FRACTIONAL" fractional)"; rc=$?
assert_zero "S17 小数秒时间戳 select_deletions 退出码 0" "$rc"
assert_eq "S17 小数秒时间戳仅删 stale 旧份 id 1（最新 id 2 保留）" "1" "$out"

# ----------------------------------------------------------------------------
echo ""
echo "=== 汇总: PASS=$PASS_N FAIL=$FAIL_N ==="
if [ "$FAIL_N" -eq 0 ]; then
    echo "ALL TESTS PASSED"
    exit 0
else
    echo "SOME TESTS FAILED"
    exit 1
fi
