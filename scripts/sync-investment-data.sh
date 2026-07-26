#!/bin/bash
# Sync investment_data: fetch latest from upstream (chenditc) and push to fork (skwy)
#
# Usage:
#   scripts/sync-investment-data.sh          # sync only, don't restart server
#   scripts/sync-investment-data.sh --restart # stop server, sync, restart server
#
# Requires: dolt on PATH, DoltHub credentials configured (dolt creds ls)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DATA_DIR="${PROJECT_ROOT}/investment_data"
UPSTREAM_REMOTE="origin"
UPSTREAM_URL="https://doltremoteapi.dolthub.com/chenditc/investment_data"
FORK_REMOTE="skwy"
FORK_URL="https://doltremoteapi.dolthub.com/skwy/investment_data"
RESTART_SERVER=false

if [ "${1:-}" = "--restart" ]; then
    RESTART_SERVER=true
fi

# --- helpers ---
red()   { echo -e "\033[31m$*\033[0m" >&2; }
green() { echo -e "\033[32m$*\033[0m"; }
info()  { echo -e "\033[36m>>> $*\033[0m"; }

# --- preflight ---
if ! command -v dolt &>/dev/null; then
    red "error: dolt not found on PATH"
    exit 1
fi

if ! dolt creds ls &>/dev/null 2>&1; then
    red "error: no DoltHub credentials — run 'dolt creds login' first"
    exit 1
fi

if [ ! -d "$DATA_DIR/.dolt" ]; then
    red "error: $DATA_DIR is not a Dolt database"
    exit 1
fi

cd "$PROJECT_ROOT"

# --- 1. Stop Dolt SQL server if restart requested ---
if $RESTART_SERVER; then
    info "Stopping Dolt SQL server..."
    PID=$(pgrep -f "dolt sql-server.*investment_data" || true)
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
        sleep 1
        # force kill if still running
        if kill -0 "$PID" 2>/dev/null; then
            kill -9 "$PID" 2>/dev/null || true
        fi
        green "  Server stopped (PID $PID)"
    else
        info "  No running server found"
    fi
fi

# --- 2. Ensure remotes are configured ---
info "Checking remotes..."

REMOTES=$(dolt --data-dir "$DATA_DIR" remote -v 2>/dev/null)

if echo "$REMOTES" | grep -q "^origin "; then
    info "  origin already configured"
else
    info "  Adding origin → $UPSTREAM_URL"
    dolt --data-dir "$DATA_DIR" remote add origin "$UPSTREAM_URL"
fi

if echo "$REMOTES" | grep -q "^$FORK_REMOTE "; then
    info "  $FORK_REMOTE already configured"
else
    info "  Adding $FORK_REMOTE → $FORK_URL"
    dolt --data-dir "$DATA_DIR" remote add "$FORK_REMOTE" "$FORK_URL"
fi

green "  Remotes OK"

# --- 3. Fetch latest from upstream ---
info "Fetching from $UPSTREAM_REMOTE ($UPSTREAM_URL)..."
dolt --data-dir "$DATA_DIR" fetch "$UPSTREAM_REMOTE"

# --- 4. Merge upstream into local master ---
info "Merging $UPSTREAM_REMOTE/master into master..."
dolt --data-dir "$DATA_DIR" checkout master 2>/dev/null || true

# Check if we are up to date
LOCAL_HASH=$(dolt --data-dir "$DATA_DIR" merge-base master "$UPSTREAM_REMOTE/master" 2>/dev/null || echo "")
UPSTREAM_HASH=$(dolt --data-dir "$DATA_DIR" merge-base "$UPSTREAM_REMOTE/master" "$UPSTREAM_REMOTE/master" 2>/dev/null || echo "")

if [ "$LOCAL_HASH" = "$UPSTREAM_HASH" ] && [ -n "$LOCAL_HASH" ]; then
    green "  Already up to date"
else
    dolt --data-dir "$DATA_DIR" pull "$UPSTREAM_REMOTE" master
    green "  Merged"
fi

# --- 5. Push to fork ---
info "Pushing to $FORK_REMOTE ($FORK_URL)..."
dolt --data-dir "$DATA_DIR" push "$FORK_REMOTE" master

green "Sync complete: $FORK_REMOTE/master == $UPSTREAM_REMOTE/master"

# --- 6. Restart server if requested ---
if $RESTART_SERVER; then
    info "Restarting Dolt SQL server..."
    nohup "$SCRIPT_DIR/start-dolt-server.sh" > /tmp/dolt-server.log 2>&1 &
    sleep 2
    if pgrep -f "dolt sql-server.*investment_data" > /dev/null; then
        green "  Server restarted (log: /tmp/dolt-server.log)"
    else
        red "  Server may have failed to start — check /tmp/dolt-server.log"
    fi
fi

echo ""
green "Done."
