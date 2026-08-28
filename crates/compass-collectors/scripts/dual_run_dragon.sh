#!/usr/bin/env bash
# Dual-run comparison for dragon_list: fetch the same date range with the
# Python collector and the Rust collector, then compare row count, seat_type
# distribution and aggregated amount fields. Date args are passed as argv to
# the Python helper, never interpolated into code.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
START="${1:-2026-08-27}"
END="${2:-$START}"
RUST_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

echo "== Rust =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" cargo run -p compass-collectors -- dragon --start "$START" --end "$END"

echo "== Python =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" uv run python - "$START" "$END" <<'PY'
import asyncio
import sys
from fetch_dragon import run

asyncio.run(run(start_date=sys.argv[1], end_date=sys.argv[2]))
PY
)

python3 - "$RUST_DIR" "$PY_DIR" <<'PY'
import csv
import sys
from pathlib import Path

rust_dir, py_dir = Path(sys.argv[1]), Path(sys.argv[2])

def load(path):
    if not path.exists():
        return []
    with open(path, encoding='utf-8-sig') as f:
        return list(csv.DictReader(f))

def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return v

def key(row):
    return (
        row.get("SECUCODE"),
        row.get("SECURITY_CODE"),
        row.get("TRADE_DATE"),
        row.get("SEAT_TYPE"),
        num(row.get("BUY_AMOUNT")),
        num(row.get("SELL_AMOUNT")),
        num(row.get("NET_AMOUNT")),
        int(float(row.get("INSTITUTION_FLAG") or 0)),
    )

r = sorted(load(rust_dir / "RPT_DAILYBILLBOARD_DETAILSNEW.csv"), key=key)
p = sorted(load(py_dir / "RPT_DAILYBILLBOARD_DETAILSNEW.csv"), key=key)
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
assert all(key(a) == key(b) for a, b in zip(r, p)), "dragon rows differ"
print(f"dual-run OK: {len(r)} dragon rows, keys match")
PY
