#!/bin/bash
# update-database.sh — one-shot idempotent daily compass_data pipeline
# (auto-heal missing data, issue #308).
#
# Steps:
#   0. sync:        sync-investment-data.sh          (investment_data Dolt ← upstream)
#   1. market data: compass-data import              (investment_data Dolt → Parquet)
#   1b. verify:     check-stock-daily gaps           (missing trading day ⇒ hard fail)
#   2. collect:     compass-collectors sync         (all compass_data sources → Dolt, auto-heal gaps)
#   3. Dolt commit: collector tables                 (limited add, push; skipped when clean)
#   4. import:      import-compass 11 tables         (9 incremental/anchored + stock_basic/index_basic full)
#   5. backfill:    sepa backfill-dates              (compute missing derived SEPA dates)
#   6. compute:     sepa temperature + sepa score --top 50 (DELETE+append write-back)
#   7. Dolt commit: compute tables                   (limited add, push; skipped when clean)
#   8. print TOP50: reuse step 6 output, never recompute
#
# Idempotency: compass-collectors sync skips already-fetched dates (data_updates.
# last_report_date); the sepa CLI write-back is DELETE+append per trade_date; the
# Dolt commits are skipped when none of the allowlisted tables changed. The whole
# script can be re-run any day without double-counting or data loss.
#
# Usage:
#   scripts/update-database.sh
#
# Requires: dolt + cargo on PATH, DoltHub credentials (dolt creds ls),
#           local Dolt repos at the default paths below.
# Env overrides (used by scripts/tests/test-update-database.sh):
#   SEPA_INVESTMENT_DATA_DIR, SEPA_COMPASS_DATA_DIR, PARQUET_DIR

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
INVESTMENT_DATA_DIR="${SEPA_INVESTMENT_DATA_DIR:-/data/compass-data/investment_data}"
COMPASS_DATA_DIR="${SEPA_COMPASS_DATA_DIR:-/data/compass-data/compass_data}"
PARQUET_DIR="${PARQUET_DIR:-/data/compass-data/parquet_data}"
# Test hook: point at a fake sync script in the shell unit tests.
SYNC_INVESTMENT_SCRIPT="${SYNC_INVESTMENT_SCRIPT:-scripts/sync-investment-data.sh}"

# Pass the resolved absolute Dolt paths to child scripts/compass-collectors so a
# worktree without the gitignored repo-root symlink still works.
export COMPASS_INVESTMENT_DATA_DIR="${COMPASS_INVESTMENT_DATA_DIR:-$INVESTMENT_DATA_DIR}"
export SEPA_INVESTMENT_DATA_DIR="${SEPA_INVESTMENT_DATA_DIR:-$INVESTMENT_DATA_DIR}"

# Allowlisted table sets for the two Dolt commits (never `dolt add .`).
# The collector allowlist covers every compass_data table refreshed by the daily
# pipeline: stock_basic, the four financial tables, the SEPA time-series tables,
# and the index tables (index_basic is a side effect of index_daily import).
COLLECTOR_TABLES=(stock_basic fin_indicators fin_balance_sheet fin_income fin_cash_flow capital_main_flow dragon_list block_trade institution_survey index_daily index_basic)
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
        # dolt_commit_changed exit 1 on any dolt commit failure.
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

# --- 0. Sync investment_data upstream before touching the local Parquet ---
# The whole point of auto-heal is to pick up missed trading days; the upstream
# Dolt fetch is the source of truth for those days, so it must run first.
run_step 0 "sync investment_data upstream" "$PROJECT_ROOT" \
    bash "$SYNC_INVESTMENT_SCRIPT"

# --- 1. Market data: investment_data Dolt → Parquet main database ---
# Full (not --since) import: stock_daily.parquet is written as a single file via
# atomic rename, so a --since filter would DROP history. The local Dolt repo is
# kept incrementally fresh by step 0; re-running a full import is idempotent
# (identical regeneration).
run_step 1 "import market data (investment_data → Parquet)" "$PROJECT_ROOT" \
    cargo run --bin compass-data -- import

# --- 1b. Verify no SSE trading day is missing from stock_daily.parquet ---
# After a full import, any gap within [min, max] means the upstream data is
# incomplete or the import silently dropped rows. This is a hard failure —
# never continue with a hole in the OHLCV history (issue #308 decision 11/13).
if ! (cd "$PROJECT_ROOT" && cargo run --bin compass-data -- check-stock-daily \
        --dolt-dir "$INVESTMENT_DATA_DIR" --parquet-dir "$PARQUET_DIR"); then
    red "step 1b failed: stock_daily calendar gap check"
    exit 1
fi

# --- 2. Collect: all compass_data sources → Dolt ---
# compass-collectors sync is the single full-refresh entry: it fetches and imports
# stock_basic, all four financial tables, the SEPA time-series tables, and
# index_daily (index_basic is written as a side effect of index_daily import).
# Keeping one generation/import entry point means the shell no longer maintains a
# separate per-source fetch/import list.
run_step 2 "collect all compass_data sources (compass-collectors sync)" "$PROJECT_ROOT" \
    cargo run --bin compass-collectors -- sync

# --- 3. Dolt commit: collector tables ---
info "Step 3: Dolt commit collector tables"
dolt_commit_changed "$COMPASS_DATA_DIR" 3 "collector" "feat: sepa collectors data ref #139" \
    "${COLLECTOR_TABLES[@]}"

# --- 4. Import collector tables into Parquet ---
# Incremental anchor is per-table: each collector stores its own
# last_report_date in data_updates.  A missing/NULL anchor means that table has
# never been imported yet, so that table gets a full export instead of
# inheriting another table's anchor (a global MAX would let a newly added table
# skip its history).  stock_basic and index_basic are always full overwrites:
# stock_basic import ignores --since by design, and index_basic is a version
# snapshot.
info "Step 4: import collector tables into Parquet"
anchor_for() {
    local table="$1" raw="" date=""
    if ! raw="$(dolt --data-dir "$COMPASS_DATA_DIR" sql -r csv -q \
            "SELECT MAX(last_report_date) FROM data_updates WHERE table_name = '$table'")"; then
        red "step 4 failed: dolt sql anchor query for $table"
        exit 1
    fi
    date="$(printf '%s\n' "$raw" | tail -n 1 | tr -d '\r')"
    [ "$date" = "NULL" ] && date=""
    printf '%s' "$date"
}

for table in "${COLLECTOR_TABLES[@]}"; do
    if [ "$table" = "stock_basic" ] || [ "$table" = "index_basic" ]; then
        # Full-overwrite tables: stock_basic is authoritative (no --since in
        # import_compass), index_basic is a version snapshot.
        run_step 4 "import-compass --table $table (full overwrite)" "$PROJECT_ROOT" \
            cargo run --bin compass-data -- import-compass --table "$table"
        continue
    fi
    SINCE="$(anchor_for "$table")"
    if [ -n "$SINCE" ]; then
        run_step 4 "import-compass --table $table (incremental, since $SINCE)" "$PROJECT_ROOT" \
            cargo run --bin compass-data -- import-compass --table "$table" --since "$SINCE"
    else
        run_step 4 "import-compass --table $table (full, no anchor yet)" "$PROJECT_ROOT" \
            cargo run --bin compass-data -- import-compass --table "$table"
    fi
done
# --- 4b. Backfill missing derived SEPA compute dates ---
# sepa backfill-dates scans the Parquet trading calendar against the Dolt
# compute tables and computes every missing date (technical_factor /
# industry_factor / capital_factor / final_score / market_temperature).
# It is idempotent and strict: any failure aborts the whole run.
info "Step 4b: backfill missing SEPA dates"
if ! (cd "$PROJECT_ROOT" && cargo run --bin compass-data -- sepa backfill-dates); then
    red "step 4b failed: sepa backfill-dates"
    exit 1
fi

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
