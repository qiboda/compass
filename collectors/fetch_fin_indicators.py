#!/usr/bin/env python3
"""Fetch A-share financial indicators from EastMoney datacenter API.

Pulls ALL fields (37) for all stocks, paginated by report date periods.

Usage:
    # Full historical (2000-now)
    uv run python fetch_fin_indicators.py

    # Specific years
    uv run python fetch_fin_indicators.py --years 2024,2025,2026

    # Incremental update (reads .state.json, fetches only new periods)
    uv run python fetch_fin_indicators.py --incremental

    # Balance sheet / income / cashflow
    uv run python fetch_fin_indicators.py --report-name RPT_DMSK_FN_BALANCE

State file: {report_name}.state.json caches last fetch.
Primary source for incremental mode is Dolt data_updates / table MAX(report_date).
CSV is temporary; Dolt is the source of truth.

KNOWN LIMITATION: Incremental mode uses REPORTDATE for filtering, which cannot
detect revisions to previously-fetched report periods.  A company may amend
historical reports (e.g. 五粮液 corrected 2025Q1-Q3 in April 2026) without
changing REPORTDATE.  To catch such revisions, a periodic full refresh of the
most recent 2-3 years is needed.
TODO: add --refresh N flag to refetch the last N years unconditionally.
"""

import argparse
import asyncio
import csv
import json
import random
import sys
import time
from datetime import datetime
from pathlib import Path

from curl_cffi.requests import AsyncSession

# ── Constants ───────────────────────────────────────────────────
EM_BASE = "https://datacenter-web.eastmoney.com/api/data/v1/get"
EM_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
)
EM_HEADERS = {
    "User-Agent": EM_UA,
    "Accept": "*/*",
    "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
    "Referer": "https://data.eastmoney.com/",
    "Sec-Ch-Ua": '"Chromium";v="142", "Google Chrome";v="142", "Not_A Brand";v="99"',
    "Sec-Ch-Ua-Mobile": "?0",
    "Sec-Ch-Ua-Platform": '"Windows"',
    "Sec-Fetch-Dest": "empty",
    "Sec-Fetch-Mode": "cors",
    "Sec-Fetch-Site": "same-site",
    "Connection": "keep-alive",
}

# Periods to fetch: all Q1/Q2/Q3/年报 dates
# We use REPORTDATE filters to batch by period
FY_DATES = [f"{y}-12-31" for y in range(2000, 2027)]
Q3_DATES = [f"{y}-09-30" for y in range(2000, 2027)]
Q2_DATES = [f"{y}-06-30" for y in range(2000, 2027)]
Q1_DATES = [f"{y}-03-31" for y in range(2000, 2027)]

# Rate limiting
EM_MIN_INTERVAL = 0.5
EM_JITTER = (0.1, 0.3)
EM_MAX_RETRIES = 4
EM_PAGE_SIZE = 100  # max for this API


def _last_report_date(report_name: str, state_path: Path) -> str:
    import subprocess

    dolt_dir = Path(__file__).resolve().parent.parent / "compass_data"
    if not (dolt_dir / ".dolt").exists():
        if state_path.exists():
            return json.loads(state_path.read_text()).get("last_report_date", "")
        return ""

    table = "fin_indicators" if report_name == "RPT_LICO_FN_CPD" else report_name
    result = subprocess.run(
        [
            "dolt",
            "--data-dir",
            str(dolt_dir),
            "sql",
            "-r",
            "csv",
            "-q",
            f"SELECT MAX(report_date) FROM {table}",
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode == 0:
        lines = result.stdout.strip().split("\n")
        last = lines[-1].strip() if len(lines) > 1 else ""
        if last and last != "NULL":
            return last

    if state_path.exists():
        return json.loads(state_path.read_text()).get("last_report_date", "")
    return ""


# ── Throttle ────────────────────────────────────────────────────


class Throttle:
    def __init__(self, min_interval: float = EM_MIN_INTERVAL):
        self._min_interval = min_interval
        self._last: float = 0.0

    async def acquire(self):
        now = time.monotonic()
        since_last = now - self._last
        if since_last < self._min_interval:
            wait = self._min_interval - since_last + random.uniform(*EM_JITTER)
            await asyncio.sleep(wait)
        else:
            await asyncio.sleep(random.uniform(0, 0.15))
        self._last = time.monotonic()


# ── Field flattener ─────────────────────────────────────────────


def flatten_record(item: dict) -> dict:
    """Flatten nested fields and normalize types for CSV export."""
    record = {}
    for k, v in item.items():
        if v is None:
            record[k] = ""
        elif isinstance(v, (int, float, str)):
            record[k] = v
        else:
            record[k] = str(v)
    return record


# ── Fetcher ─────────────────────────────────────────────────────


async def fetch_period(
    session: AsyncSession,
    throttle: Throttle,
    report_name: str,
    report_date: str,
    page_size: int = EM_PAGE_SIZE,
) -> list[dict]:
    """Fetch all pages for a single report period."""
    all_records = []
    page = 1
    total_pages = 1

    while page <= total_pages:
        params = {
            "reportName": report_name,
            "columns": "ALL",
            "filter": f"(REPORTDATE='{report_date}')",
            "sortColumns": "SECURITY_CODE",
            "sortTypes": "1",
            "pageSize": page_size,
            "pageNumber": page,
            "source": "WEB",
            "client": "WEB",
        }

        for attempt in range(EM_MAX_RETRIES):
            try:
                await throttle.acquire()
                resp = await session.get(EM_BASE, params=params, headers=EM_HEADERS)

                if resp.status_code == 429:
                    wait = 15 + random.uniform(0, 5)
                    print(f"    429, waiting {wait:.0f}s...", file=sys.stderr)
                    await asyncio.sleep(wait)
                    continue

                resp.raise_for_status()
                data = resp.json()
                break

            except Exception as e:
                wait = min(2**attempt, 30) + random.uniform(0, 3)
                if attempt < EM_MAX_RETRIES - 1:
                    print(
                        f"    retry {attempt + 1}/{EM_MAX_RETRIES} in {wait:.0f}s: {e}",
                        file=sys.stderr,
                    )
                    await asyncio.sleep(wait)
                else:
                    raise

        if not data.get("success"):
            print(f"    API error: {data.get('message', 'unknown')}", file=sys.stderr)
            break

        result = data.get("result")
        if result is None:
            break

        items = result.get("data", [])
        if not items:
            break

        for item in items:
            all_records.append(flatten_record(item))

        total_pages = min(result.get("pages", 1), 500)  # safety cap
        page += 1

    return all_records


# ── CSV writer ──────────────────────────────────────────────────


def write_csv(records: list[dict], filepath: Path, append: bool = False):
    """Write records to CSV. Infers fieldnames from first record on first write."""
    if not records:
        return

    mode = "a" if append and filepath.exists() else "w"
    write_header = not append or not filepath.exists()

    with open(filepath, mode, newline="", encoding="utf-8-sig") as f:
        writer = csv.DictWriter(f, fieldnames=list(records[0].keys()))
        if write_header:
            writer.writeheader()
        writer.writerows(records)


# ── Main ────────────────────────────────────────────────────────


async def main():
    parser = argparse.ArgumentParser(
        description="Fetch A-share financial indicators from EastMoney"
    )
    parser.add_argument(
        "--report-name",
        default="RPT_LICO_FN_CPD",
        help="EastMoney reportName (default: RPT_LICO_FN_CPD)",
    )
    parser.add_argument(
        "--years", default="", help="Comma-separated years to fetch (default: all 2000-2026)"
    )
    parser.add_argument("--output", default="", help="Output CSV path (default: {report_name}.csv)")
    parser.add_argument(
        "--periods",
        default="Q1,Q2,Q3,FY",
        help="Which quarters to fetch: Q1,Q2,Q3,FY (default: all)",
    )
    parser.add_argument(
        "--page-size",
        type=int,
        default=EM_PAGE_SIZE,
        help=f"Records per page (default: {EM_PAGE_SIZE})",
    )
    parser.add_argument(
        "--incremental",
        action="store_true",
        help="Only fetch new report periods. Checks Dolt data_updates first, falls back to .state.json",
    )
    args = parser.parse_args()

    report_name = args.report_name
    current_year = datetime.now().year

    # State file for incremental updates
    state_path = Path(f"{report_name}.state.json")

    # Build date list
    if args.years:
        years = [int(y.strip()) for y in args.years.split(",") if y.strip()]
    else:
        years = list(range(2020, current_year + 1))

    periods = [p.strip() for p in args.periods.split(",")]
    period_dates = {
        "FY": [f"{y}-12-31" for y in years],
        "Q3": [f"{y}-09-30" for y in years],
        "Q2": [f"{y}-06-30" for y in years],
        "Q1": [f"{y}-03-31" for y in years],
    }

    all_dates = []
    for period in periods:
        if period in period_dates:
            all_dates.extend(period_dates[period])

    # Sort dates so incremental mode works correctly
    all_dates.sort()

    # Handle incremental mode
    if args.incremental:
        since = _last_report_date(report_name, state_path)
        if since:
            print(f"Incremental: last data from {since}", file=sys.stderr)
            all_dates = [d for d in all_dates if d >= since]
        else:
            print("No prior data found, fetching full history.", file=sys.stderr)
        if not all_dates:
            print("No new report periods to fetch.", file=sys.stderr)
            return

    output_path = Path(args.output or f"{report_name}.csv")
    page_size = args.page_size

    print(f"Report: {report_name}", file=sys.stderr)
    print(
        f"Periods: {len(all_dates)} ({periods}, {all_dates[0] if all_dates else 'none'}..{all_dates[-1] if all_dates else 'none'})",
        file=sys.stderr,
    )
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(f"Page size: {page_size}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    total_records = 0
    max_report_date = ""
    first_write = not output_path.exists()  # appending if file already exists

    async with AsyncSession(impersonate="chrome142") as session:
        for i, report_date in enumerate(all_dates):
            date_label = report_date
            print(
                f"[{i + 1}/{len(all_dates)}] {date_label} ...", file=sys.stderr, end=" ", flush=True
            )

            try:
                records = await fetch_period(session, throttle, report_name, report_date, page_size)
            except Exception as e:
                print(f"FAILED: {e}", file=sys.stderr)
                continue

            if records:
                write_csv(records, output_path, append=not first_write)
                first_write = False
                max_report_date = max(max_report_date, report_date)
                print(f"{len(records)} records", file=sys.stderr)
            else:
                print("empty", file=sys.stderr)

            total_records += len(records)

    if max_report_date:
        state = {
            "last_report_date": max_report_date,
            "total_rows": total_records,
            "last_run": datetime.now().isoformat(),
        }
        state_path.write_text(json.dumps(state, indent=2))

    print(f"\nDone: {total_records} records → {output_path.resolve()}", file=sys.stderr)
    if state_path.exists():
        print(
            f"State: {state_path} → last_report_date={state_path.read_text().strip()[:80]}...",
            file=sys.stderr,
        )


if __name__ == "__main__":
    asyncio.run(main())
