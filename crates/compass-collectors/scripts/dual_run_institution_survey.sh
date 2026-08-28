#!/usr/bin/env bash
# Dual-run comparison for institution_survey: fetch the same day with the
# Python collector and the Rust collector. Both collectors are asked to start
# at the supplied day; with no historical watermark this means one day only.
# The day argument is passed as argv to Python, never interpolated into code.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
DAY="${1:-$(date +%F)}"
RUST_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

echo "== Rust =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" cargo run -p compass-collectors -- institution-survey --start-date "$DAY"

echo "== Python =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" uv run python - "$DAY" <<'PY'
import asyncio
import sys
from fetch_institution_survey import run

asyncio.run(run(start_date=sys.argv[1]))
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

def key(row):
    return (
        row.get("SECUCODE"),
        row.get("SECURITY_CODE"),
        row.get("RECEIVE_START_DATE"),
        row.get("RECEIVE_OBJECT"),
        row.get("RECEIVE_WAY_EXPLAIN"),
    )

r = sorted(load(rust_dir / "RPT_ORG_SURVEYNEW.csv"), key=key)
p = sorted(load(py_dir / "RPT_ORG_SURVEYNEW.csv"), key=key)
assert len(r) == len(p), f"row count mismatch: rust={len(r)} python={len(p)}"
assert all(key(a) == key(b) for a, b in zip(r, p)), "institution survey rows differ"
print(f"dual-run OK: {len(r)} survey rows, keys match")
PY
