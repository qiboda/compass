#!/usr/bin/env bash
# Dual-run comparison for stock_basic_official: fetch the three-exchange
# official stock lists with Python and Rust, then compare row count and all
# 12 canonical columns. Uses isolated COMPASS_CSV_DIR / COMPASS_DATA_DIR.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
RUST_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

echo "== Rust =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 \
  cargo run -p compass-collectors -- stock-basic-official

echo "== Python =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 \
  uv run python fetch_stock_basic_official.py)

python3 - "$RUST_DIR" "$PY_DIR" <<'PY'
import csv
import sys
from pathlib import Path

rust_dir, py_dir = Path(sys.argv[1]), Path(sys.argv[2])

def load(dir):
    path = dir / "stock_basic_official.csv"
    if not path.exists():
        return []
    with open(path, encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))

def canon(v):
    return (v or "").strip()

def key(row):
    return (
        canon(row.get("symbol")),
        canon(row.get("ts_code")),
        canon(row.get("code")),
    )

r = sorted(load(rust_dir), key=key)
p = sorted(load(py_dir), key=key)
assert r and p, "stock_basic_official produced empty CSV on one side"
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
for ar, ap in zip(r, p):
    for col in sorted(ar):
        if col not in ap:
            continue
        vr, vp = canon(ar.get(col)), canon(ap.get(col))
        if vr != vp:
            raise AssertionError(f"{col} differs for {ar['code']}: rust={vr!r} python={vp!r}")
print(f"dual-run OK: {len(r)} stock_basic_official rows, 12-column values match")
PY
