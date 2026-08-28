#!/usr/bin/env bash
# Dual-run comparison for stock_basic (EastMoney): fetch one/few pages with
# the Python collector and the Rust collector, then compare row count and the
# identity/classification fields. Defaults to a bounded page slice so CI and
# local smoke do not fetch all 6000+ stocks.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
PAGE_SIZE="${1:-100}"
MAX_PAGES="${2:-1}"
RUST_CSV="$(mktemp --suffix=.csv)"
DATA_DIR="$(mktemp -d)"
PY_CSV="$(mktemp --suffix=.csv)"
trap 'rm -f "$RUST_CSV" "$PY_CSV"; rm -rf "$DATA_DIR"' EXIT

echo "== Rust =="
cargo run -p compass-collectors -- stock-basic --output "$RUST_CSV" --page-size "$PAGE_SIZE" --max-pages "$MAX_PAGES"

echo "== Python =="
(cd "$REPO_ROOT/collectors" && COMPASS_DATA_DIR="$DATA_DIR" uv run python fetch_stock_basic.py --page-size "$PAGE_SIZE" --max-pages "$MAX_PAGES" --output "$PY_CSV")

python3 - "$RUST_CSV" "$PY_CSV" <<'PY'
import csv
import sys
from pathlib import Path

rust_path, py_path = Path(sys.argv[1]), Path(sys.argv[2])

def load(path):
    if not path.exists():
        return []
    with open(path, encoding='utf-8-sig') as f:
        return list(csv.DictReader(f))

def key(row):
    return tuple((k, row.get(k)) for k in (
        "symbol", "ts_code", "f12", "f13", "f14", "f26",
        "f100", "f101", "f102", "f103", "f124",
        "f127", "f128", "f134", "f189", "f221",
    ))

r = sorted(load(rust_path), key=key)
p = sorted(load(py_path), key=key)
assert r and p, "stock_basic produced empty CSV on one side"
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
assert all(key(a) == key(b) for a, b in zip(r, p)), "stock_basic rows differ"
print(f"dual-run OK: {len(r)} stock_basic rows, keys match")
PY
