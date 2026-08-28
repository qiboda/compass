#!/usr/bin/env bash
# Dual-run comparison for the block_trade pilot: fetch the same date with the
# Python collector and the Rust collector, then compare row count, date
# coverage and key numeric fields (floating-point text representation is
# normalized before comparison). Date args are passed as argv to Python.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
START="${1:-2026-08-27}"
END="${2:-$START}"
RUST_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

echo "== Rust =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" cargo run -p compass-collectors -- block-trade --start "$START" --end "$END"

echo "== Python =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" uv run python - "$START" "$END" <<'PY'
import asyncio
import sys
from fetch_block_trade import run

asyncio.run(run(start=sys.argv[1], end=sys.argv[2]))
PY
)

python3 - "$START" "$END" "$RUST_DIR" "$PY_DIR" <<'PY'
import csv
import sys
from pathlib import Path

start, end, rust_dir, py_dir = sys.argv[1], sys.argv[2], Path(sys.argv[3]), Path(sys.argv[4])

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
        num(row.get("DEAL_PRICE")),
        num(row.get("DEAL_VOLUME")),
        num(row.get("DEAL_AMT")),
        row.get("BUYER_NAME"),
        row.get("SELLER_NAME"),
        num(row.get("PREMIUM_RATIO")),
    )

r = load(rust_dir / "RPT_DATA_BLOCKTRADE.csv")
p = load(py_dir / "RPT_DATA_BLOCKTRADE.csv")
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
assert all(key(row_r) == key(row_p) for row_r, row_p in zip(r, p)), "key rows differ"
print(f"dual-run OK: {len(r)} rows, keys match")
PY
