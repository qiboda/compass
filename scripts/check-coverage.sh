#!/usr/bin/env bash
# Enforce line-coverage thresholds from a `cargo llvm-cov --json` report.
#
# Checks the workspace total and each workspace crate (compass-core /
# compass-data / compass-i18n / compass / compass-strategy / compass-types /
# compass-ui) against per-target minimum line-coverage percentages. Exits 1 if
# any target is below its threshold or has no measured files.
#
# Thresholds (2026-08-12, ref #250, testability-based): compass-core,
# compass-data, compass-i18n, compass-strategy, compass-types and compass-ui
# are enforced at 95%; compass (GUI, event-loop/thread/interaction hard to
# test) at 90%; the workspace total at 93%.
#
# Usage:
#   scripts/check-coverage.sh [cov.json]
#
# Defaults: cov.json=cov.json (in CWD).
set -euo pipefail

COV_JSON="${1:-cov.json}"

# Per-target minimum line coverage. Keys must match the `check` calls below.
declare -A THRESHOLDS=(
    [workspace]=93
    [compass-core]=95
    [compass-data]=95
    [compass-i18n]=95
    [compass]=90
    [compass-strategy]=95
    [compass-types]=95
    [compass-ui]=95
)

if ! command -v jq >/dev/null 2>&1; then
    echo "ERROR: jq is required" >&2
    exit 2
fi
if [ ! -f "$COV_JSON" ]; then
    echo "ERROR: coverage report not found: $COV_JSON" >&2
    exit 2
fi

fail=0

check() {
    local label="$1" filter="$2"
    local threshold="${THRESHOLDS[$label]:-80}"
    local pct
    # ${filter} is injected as jq code (select(...) or .); $t is a jq variable.
    pct=$(jq -r --argjson t "$threshold" "
        [.data[0].files[] | ${filter} | .summary.lines] as \$l |
        if (\$l | length) == 0 then
            \"MISSING\"
        else
            ((\$l | map(.covered) | add) * 100 / (\$l | map(.count) | add))
        end
    " "$COV_JSON")

    case "$pct" in
        MISSING)
            echo "FAIL: $label — no files measured (0% effective)" >&2
            fail=1
            ;;
        *)
            if awk -v p="$pct" -v t="$threshold" 'BEGIN { exit !(p + 0 < t + 0) }'; then
                echo "FAIL: $label line coverage ${pct}% < ${threshold}%" >&2
                fail=1
            else
                echo "OK: $label line coverage ${pct}%"
            fi
            ;;
    esac
}

# Workspace total: every file in the report.
check "workspace" "."

# Per-crate: files under crates/<name>/ (report uses absolute paths).
check "compass-core" "select(.filename | contains(\"/crates/compass-core/\"))"
check "compass-data" "select(.filename | contains(\"/crates/compass-data/\"))"
check "compass-i18n" "select(.filename | contains(\"/crates/compass-i18n/\"))"
check "compass" "select(.filename | contains(\"/crates/compass/\"))"
check "compass-strategy" "select(.filename | contains(\"/crates/compass-strategy/\"))"
check "compass-types" "select(.filename | contains(\"/crates/compass-types/\"))"
check "compass-ui" "select(.filename | contains(\"/crates/compass-ui/\"))"

exit "$fail"
