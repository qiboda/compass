#!/usr/bin/env bash
# Bounded dual-run for index_daily: compare one official EastMoney kline and
# the THS industry list through the exact same paths used by the full run.
# Full index_daily (90 THS industries x ~20 years) is intentionally not run
# here; these probes validate the EastMoney/TLS/GBK/parsing paths end to end.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"
SECID="${1:-1.000001}"
RUST_DIR="$(mktemp -d)"
PY_DIR="$(mktemp -d)"
DATA_DIR="$(mktemp -d)"
trap 'rm -rf "$RUST_DIR" "$PY_DIR" "$DATA_DIR"' EXIT

echo "== Rust probe $SECID =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 \
  cargo run -p compass-collectors -- index-daily-probe --secid "$SECID" --output "$RUST_DIR/probe.csv"

echo "== Python probe $SECID =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 \
  uv run python - "$SECID" "$PY_DIR/probe.csv" <<'PY'
import asyncio
import sys
from pathlib import Path
from common import AsyncSession, Throttle
from fetch_index_daily import fetch_kline

async def main():
    secid, out = sys.argv[1], sys.argv[2]
    async with AsyncSession(impersonate="chrome142") as session:
        throttle = Throttle()
        result = await fetch_kline(session, throttle, secid)
        assert result is not None, f"fetch_kline returned None for {secid}"
        klines, _code = result
        Path(out).write_text("\n".join(klines), encoding="utf-8")

asyncio.run(main())
PY
)

python3 - "$RUST_DIR/probe.csv" "$PY_DIR/probe.csv" <<'PY'
import sys
from pathlib import Path

r = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
p = Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
assert r and p, "probe returned empty on one side"
assert len(r) == len(p), f"kline count mismatch: rust={len(r)} python={len(p)}"
assert r == p, "kline rows differ"
print(f"dual-run OK: {len(r)} index klines match for probe")
PY

echo "== Rust THS industries =="
COMPASS_CSV_DIR="$RUST_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 \
  cargo run -p compass-collectors -- index-daily-industries-probe --output "$RUST_DIR/industries.csv"

echo "== Python THS industries =="
(cd "$REPO_ROOT/collectors" && COMPASS_CSV_DIR="$PY_DIR" COMPASS_DATA_DIR="$DATA_DIR" COMPASS_PROXY_DISABLE=1 \
  uv run python - "$PY_DIR/industries.csv" <<'PY'
import asyncio
import sys
from pathlib import Path
from common import AsyncSession, Throttle
from fetch_index_daily import fetch_ths_industry_list

async def main():
    out = sys.argv[1]
    async with AsyncSession(impersonate="chrome142") as session:
        throttle = Throttle()
        industries = await fetch_ths_industry_list(session, throttle)
        Path(out).write_text("\n".join(f"{c},{n}" for c, n in industries), encoding="utf-8")

asyncio.run(main())
PY
)

python3 - "$RUST_DIR/industries.csv" "$PY_DIR/industries.csv" <<'PY'
import sys
from pathlib import Path

r = Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
p = Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
assert r and p, "THS industries probe returned empty on one side"
assert len(r) == len(p), f"THS industry count mismatch: rust={len(r)} python={len(p)}"
assert r == p, "THS industry list differs"
print(f"dual-run OK: {len(r)} THS industries match")
PY
