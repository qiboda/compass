#!/bin/bash
# sepa_daily.sh — one-shot idempotent SEPA daily pipeline (epic #139, issue #151).
#
# Seven steps:
#   1. market data:  compass-data import           (investment_data Dolt → Parquet)
#   2. collect:      collectors fetch 5 sources    (EastMoney → compass_data Dolt)
#   3. Dolt commit:  collector tables              (limited add, push; skipped when clean)
#   4. import:       import-compass 4 append tables + concept_member --overwrite
#   5. compute:      sepa temperature + sepa score --top 50 (DELETE+append write-back)
#   6. Dolt commit:  compute tables                (limited add, push; skipped when clean)
#   7. print TOP50:  reuse step 5 output, never recompute
#
# Idempotency: the collectors skip already-fetched dates (data_updates.
# last_report_date); the sepa CLI write-back is DELETE+append per trade_date; the
# Dolt commits are skipped when none of the allowlisted tables changed. The whole
# script can be re-run any day without double-counting or data loss.
#
# Usage:
#   scripts/sepa_daily.sh
#
# Requires: dolt + uv + cargo on PATH, DoltHub credentials (dolt creds ls),
#           local Dolt repos at the default paths below.
# Env overrides (used by scripts/tests/test-sepa-daily.sh):
#   SEPA_INVESTMENT_DATA_DIR, SEPA_COMPASS_DATA_DIR

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
INVESTMENT_DATA_DIR="${SEPA_INVESTMENT_DATA_DIR:-/data/compass-data/investment_data}"
COMPASS_DATA_DIR="${SEPA_COMPASS_DATA_DIR:-/data/compass-data/compass_data}"

# Allowlisted table sets for the two Dolt commits (never `dolt add .`).
COLLECTOR_TABLES=(capital_main_flow dragon_list block_trade institution_survey concept_member)
COMPUTE_TABLES=(technical_factor industry_factor capital_factor final_score market_temperature data_updates)

# --- helpers ---
red()   { echo -e "\033[31m$*\033[0m" >&2; }
green() { echo -e "\033[32m$*\033[0m"; }
info()  { echo -e "\033[36m>>> $*\033[0m"; }

# Run a pipeline step from directory $3; on failure print "step N failed" and
# abort loudly (no silent failures).
run_step() {
    local n="$1" desc="$2" dir="$3"
    shift 3
    info "Step $n: $desc"
    if ! (cd "$dir" && "$@"); then
        red "step $n failed: $desc"
        exit 1
    fi
}

# Stage and commit only the allowlisted tables that actually changed, then push.
# Called with: <data_dir> <step> <label> <commit_msg> <tables...>
dolt_commit_changed() {
    local data_dir="$1" step="$2" label="$3" msg="$4"
    shift 4
    local status_out="" changed=() t
    if ! status_out="$(dolt --data-dir "$data_dir" status 2>&1)"; then
        red "step $step failed: dolt status"
        exit 1
    fi
    for t in "$@"; do
        if grep -qE "(^|[[:space:]])${t}([[:space:]]|$)" <<<"$status_out"; then
            changed+=("$t")
        fi
    done
    if [ "${#changed[@]}" -eq 0 ]; then
        green "  No $label table changes — skipping Dolt commit"
        return 0
    fi
    info "  Staging changed $label tables: ${changed[*]}"
    if ! dolt --data-dir "$data_dir" add "${changed[@]}"; then
        red "step $step failed: dolt add ${changed[*]}"
        exit 1
    fi
    if ! dolt --data-dir "$data_dir" commit -m "$msg"; then
        red "step $step failed: dolt commit"
        exit 1
    fi
    if ! dolt --data-dir "$data_dir" push origin main; then
        red "step $step failed: dolt push origin main"
        exit 1
    fi
    green "  Committed & pushed: $msg"
}

# --- preflight ---
if ! command -v dolt &>/dev/null; then
    red "error: dolt not found on PATH"
    exit 1
fi

if ! command -v uv &>/dev/null; then
    red "error: uv not found on PATH (required by collectors)"
    exit 1
fi

if ! command -v cargo &>/dev/null; then
    red "error: cargo not found on PATH"
    exit 1
fi

if ! dolt creds ls &>/dev/null 2>&1; then
    red "error: no DoltHub credentials — run 'dolt creds login' first"
    exit 1
fi

if [ ! -d "$INVESTMENT_DATA_DIR/.dolt" ]; then
    red "error: $INVESTMENT_DATA_DIR is not a Dolt database"
    exit 1
fi

if [ ! -d "$COMPASS_DATA_DIR/.dolt" ]; then
    red "error: $COMPASS_DATA_DIR is not a Dolt database"
    exit 1
fi

cd "$PROJECT_ROOT"

# --- 1. Market data: investment_data Dolt → Parquet main database ---
# Full (not --since) import: stock_daily.parquet is written as a single file via
# atomic rename, so a --since filter would DROP history. The local Dolt repo is
# already kept incrementally fresh by scripts/sync-investment-data.sh; re-running
# a full import is idempotent (identical regeneration).
run_step 1 "import market data (investment_data → Parquet)" "$PROJECT_ROOT" \
    cargo run --bin compass-data -- import

# --- 2. Collect: EastMoney data → compass_data Dolt ---
# Each fetcher short-circuits when data_updates.last_report_date already covers
# the day, so re-runs add nothing.
run_step 2 "collect EastMoney data (5 sources)" "$PROJECT_ROOT/collectors" \
    uv run python main.py fetch main_flow dragon block_trade institution_survey concept_member

# --- 3. Dolt commit: collector tables ---
info "Step 3: Dolt commit collector tables"
dolt_commit_changed "$COMPASS_DATA_DIR" 3 "collector" "feat: sepa collectors data ref #139" \
    "${COLLECTOR_TABLES[@]}"

# --- 4. Import collector tables into Parquet ---
# Incremental anchor: the newest last_report_date across the collector tables —
# the collectors only append rows after it, so importing from there forward is a
# complete, minimal window (import_append_table merges prefer-new into the
# existing parquet). Empty on first run → full export.
info "Step 4: import collector tables into Parquet"
SINCE=""
if SINCE_RAW="$(dolt --data-dir "$COMPASS_DATA_DIR" sql -r csv -q \
        "SELECT MAX(last_report_date) FROM data_updates WHERE table_name IN ('capital_main_flow','dragon_list','block_trade','institution_survey')" 2>/dev/null)"; then
    SINCE="$(printf '%s\n' "$SINCE_RAW" | tail -n 1 | tr -d '\r')"
    [ "$SINCE" = "NULL" ] && SINCE=""
fi

for table in capital_main_flow dragon_list block_trade institution_survey; do
    if [ -n "$SINCE" ]; then
        run_step 4 "import-compass --table $table (incremental)" "$PROJECT_ROOT" \
            cargo run --bin compass-data -- import-compass --table "$table" --since "$SINCE"
    else
        run_step 4 "import-compass --table $table (full, no anchor yet)" "$PROJECT_ROOT" \
            cargo run --bin compass-data -- import-compass --table "$table"
    fi
done
# concept_member is a versioned mapping (not a date-partitioned feed): full
# overwrite (DELETE+rewrite semantics in import_concept_member).
run_step 4 "import-compass --table concept_member (full overwrite)" "$PROJECT_ROOT" \
    cargo run --bin compass-data -- import-compass --table concept_member --overwrite

# --- 5. Compute: market temperature + TOP50, write back to Dolt ---
# The CLI write-back is DELETE-by-trade_date + append, so re-running the same day
# is idempotent. The score table is teed to a log so step 7 can reuse it without
# recomputing.
info "Step 5: compute — market temperature + TOP50 scores"
SCORE_LOG="${TMPDIR:-/tmp}/sepa_top50_$(date +%Y%m%d).txt"
if ! (cd "$PROJECT_ROOT" && cargo run --bin compass-data -- sepa temperature); then
    red "step 5 failed: sepa temperature"
    exit 1
fi
if ! (cd "$PROJECT_ROOT" && cargo run --bin compass-data -- sepa score --top 50 2>&1 | tee "$SCORE_LOG"); then
    red "step 5 failed: sepa score --top 50"
    exit 1
fi

# --- 6. Dolt commit: compute tables ---
# Mandatory second commit — otherwise the compute-table changes stay in the
# working tree and never reach the remote (epic #139 decision 2/9).
info "Step 6: Dolt commit compute tables"
dolt_commit_changed "$COMPASS_DATA_DIR" 6 "compute" "feat: sepa scores ref #139" \
    "${COMPUTE_TABLES[@]}"

# --- 7. Print TOP50 (reuse step 5 output — never recompute) ---
info "Step 7: TOP50 list"
if [ -s "$SCORE_LOG" ]; then
    green "TOP50 printed by 'sepa score --top 50' in step 5; saved copy: $SCORE_LOG"
else
    green "TOP50 printed by 'sepa score --top 50' in step 5 above."
fi

echo ""
green "Done. Next: cargo run --bin compass (GUI) to view the SEPA panel."
