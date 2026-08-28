#!/usr/bin/env bash
# Dual-run comparison for main_flow: fetch the latest-day snapshot with the
# Python collector and the Rust collector, then compare row count and key
# flow fields. Uses isolated CSV directories so neither side touches the
# production Dolt/csv dir.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
RUST_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

echo "== Rust =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" cargo run -p compass-collectors -- main-flow

echo "== Python =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" uv run python -c "
import asyncio
from fetch_main_flow import run
asyncio.run(run())
")

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
        row.get("symbol"),
        row.get("trade_date"),
        num(row.get("main_net_inflow")),
        num(row.get("main_net_inflow_rate")),
        num(row.get("super_large_net")),
        num(row.get("large_net")),
        num(row.get("medium_net")),
        num(row.get("small_net")),
    )

r = sorted(load(rust_dir / "RPT_MAIN_MONEY_FLOW.csv"), key=key)
p = sorted(load(py_dir / "RPT_MAIN_MONEY_FLOW.csv"), key=key)
assert r and p, "main_flow produced empty CSV on one side"
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
assert all(key(a) == key(b) for a, b in zip(r, p)), "main_flow rows differ"
print(f"dual-run OK: {len(r)} main_flow rows, keys match")
PY
