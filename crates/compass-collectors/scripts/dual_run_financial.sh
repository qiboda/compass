#!/usr/bin/env bash
# Dual-run comparison for B4 financial collectors. Usage:
#   dual_run_financial.sh <module> [years] [periods]
# Modules: fin_indicators, balance_sheet, income, cash_flow.
# Defaults to one quarter of the current year to keep the smoke bounded.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
MOD="${1:-balance_sheet}"
YEARS="${2:-$(date +%Y)}"
PERIODS="${3:-Q1}"

RUST_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

case "$MOD" in
  fin_indicators)
    PY_SCRIPT="fetch_fin_indicators.py"
    RUST_CMD="fin-indicators"
    REPORT="RPT_LICO_FN_CPD"
    ;;
  balance_sheet)
    PY_SCRIPT="fetch_balance_sheet.py"
    RUST_CMD="balance-sheet"
    REPORT="RPT_F10_FINANCE_GBALANCE"
    ;;
  income)
    PY_SCRIPT="fetch_income.py"
    RUST_CMD="income"
    REPORT="RPT_F10_FINANCE_GINCOME"
    ;;
  cash_flow)
    PY_SCRIPT="fetch_cash_flow.py"
    RUST_CMD="cash-flow"
    REPORT="RPT_F10_FINANCE_GCASHFLOW"
    ;;
  *)
    echo "unknown module: $MOD" >&2
    exit 2
    ;;
esac

echo "== Rust $MOD =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 cargo run -p compass-collectors -- "$RUST_CMD" --years "$YEARS" --periods "$PERIODS"

echo "== Python $MOD =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 uv run python "$PY_SCRIPT" --years "$YEARS" --periods "$PERIODS")

python3 - "$REPORT" "$RUST_DIR" "$PY_DIR" <<'PY'
import csv
import sys
from pathlib import Path

report, rust_dir, py_dir = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])

def load(dir):
    path = dir / f"{report}.csv"
    if not path.exists():
        return []
    with open(path, encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))

def key(row):
    return (
        row.get("SECUCODE"),
        row.get("SECURITY_CODE"),
        row.get("REPORTDATE") or row.get("REPORT_DATE"),
    )

def canon(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return (v or "").strip()

def canon_row(row):
    return tuple(canon(row[k]) for k in sorted(row))

r = sorted(load(rust_dir), key=key)
p = sorted(load(py_dir), key=key)
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
assert all(key(a) == key(b) for a, b in zip(r, p)), "financial key rows differ"
assert all(canon_row(a) == canon_row(b) for a, b in zip(r, p)), "financial values differ"
print(f"dual-run OK: {len(r)} {report} rows, keys and values match")
PY
