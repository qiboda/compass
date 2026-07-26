#!/bin/bash
# Start Dolt SQL server for investment_data (read-only)
#
# Usage:
#   scripts/start-dolt-server.sh          # read-only, 0.0.0.0:3306
#   scripts/start-dolt-server.sh --rw     # allow writes
#   DOLT_PORT=3307 scripts/start-dolt-server.sh
#
# Connect from any MySQL-compatible GUI:
#   Host: localhost, Port: 3306, User: root, Password: (none)

set -euo pipefail

DATA_DIR="${DOLT_DATA_DIR:-investment_data}"
HOST="${DOLT_HOST:-0.0.0.0}"
PORT="${DOLT_PORT:-3306}"
READONLY="--readonly"

if [ "${1:-}" = "--rw" ]; then
    READONLY=""
    echo "WARNING: read-write mode enabled" >&2
fi

if ! command -v dolt &>/dev/null; then
    echo "error: dolt not found on PATH" >&2
    exit 1
fi

if [ ! -d "$DATA_DIR/.dolt" ]; then
    echo "error: $DATA_DIR is not a Dolt database (no .dolt/ found)" >&2
    exit 1
fi

if [ -n "$READONLY" ]; then
    MODE="read-only"
else
    MODE="read-write"
fi

echo "=== Dolt SQL Server ==="
echo "Data dir:  $DATA_DIR"
echo "Listening: $HOST:$PORT"
echo "Mode:      $MODE"
echo "Connect:   mysql -h 127.0.0.1 -P $PORT -u root"
echo "           (or any MySQL-compatible GUI)"
echo ""
echo "Press Ctrl+C to stop."

exec dolt sql-server \
    --data-dir "$DATA_DIR" \
    --host "$HOST" \
    --port "$PORT" \
    $READONLY
