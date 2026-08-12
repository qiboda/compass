#!/usr/bin/env python3
"""Fetch A-share financial indicators from EastMoney datacenter API.

Pulls ALL fields (37) for all stocks, paginated by report date periods.

Usage:
    # Full historical (2020-now)
    uv run python fetch_fin_indicators.py

    # Specific years
    uv run python fetch_fin_indicators.py --years 2024,2025,2026

    # Incremental update (UPDATE_DATE anchor, detects revisions)
    uv run python fetch_fin_indicators.py --incremental

    # Balance sheet / income / cashflow
    uv run python fetch_fin_indicators.py --report-name RPT_DMSK_FN_BALANCE

State file: {report_name}.state.json caches last fetch.

Incremental mode (RPT_LICO_FN_CPD only) fetches every row whose UPDATE_DATE is
>= the anchor = min(data_updates.last_updated, state.json last_update_date) —
this catches revisions to previously-fetched report periods (a company may
amend historical reports without changing REPORTDATE).  --years/--periods are
ignored in incremental mode: the anchor filter crosses report periods.
A NULL/absent anchor falls back to full REPORTDATE enumeration.

KNOWN BEHAVIOR: the anchor can stall at the max UPDATE_DATE ever seen (pre-
stamped future dates included), re-fetching that small window every run —
the Dolt UPSERT import is idempotent so this is safe and expected; do NOT
"optimize" it into advancing to CURDATE().  Rows deleted/withdrawn on the API
side do not propagate to Dolt (UPSERT can overwrite but never delete).
"""

import argparse
import asyncio
import csv
import json
import random
import re
import sys
import time
from datetime import date, datetime
from pathlib import Path

from curl_cffi.requests import AsyncSession

from common import csv_dir, dedupe_csv, dolt_dir, dolt_sql_csv

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


def _normalize_update_date(value: object) -> str | None:
    """Normalize an API UPDATE_DATE to its YYYY-MM-DD date prefix.

    Handles time-suffixed values ("2026-08-05 00:00:00") and slash / zero-
    padded variants ("2026/08/13", "2026-8-3").  Returns None for empty,
    missing, or unparseable values so callers can SKIP them (they must never
    crash the filter construction or the state max computation).
    """
    if value is None:
        return None
    s = str(value).strip()
    if not s:
        return None
    m = re.match(r"^(\d{4})[-/](\d{1,2})[-/](\d{1,2})", s)
    if not m:
        return None
    year, month, day = m.groups()
    return f"{year}-{int(month):02d}-{int(day):02d}"


def _update_anchor(report_name: str, state_path: Path) -> str:
    """Resolve the incremental UPDATE_DATE anchor for a report.

    Anchor = min(data_updates.last_updated (table for ``report_name``),
    state.json ``last_update_date``) — the EARLIER of the two sources, so a
    cross-day fetch/import or a standalone import can never push the anchor
    past rows updated in the fetch-import gap.

    - data_updates row with NULL/empty ``last_updated`` → that source missing
    - state.json lacking the ``last_update_date`` key (old single-key format)
      → that source missing
    - both sources missing → "" (triggers full REPORTDATE enumeration)
    - anchor > today → clamped to today (no-update semantics)

    Read via common.dolt_dir()/dolt_sql_csv (COMPASS_DATA_DIR env-aware) —
    deliberately NOT the repo-relative ``_last_report_date`` path, which has
    never resolved on any checkout.
    """
    sources: list[str] = []
    if (dolt_dir() / ".dolt").exists():
        table = "fin_indicators" if report_name == "RPT_LICO_FN_CPD" else report_name
        stdout = dolt_sql_csv(
            f"SELECT last_updated FROM data_updates WHERE table_name='{table}'"
        )
        lines = stdout.strip().split("\n")
        last = lines[-1].strip() if len(lines) > 1 else ""
        if last and last != "NULL":
            sources.append(last)

    if state_path.exists():
        try:
            state = json.loads(state_path.read_text())
        except (json.JSONDecodeError, OSError):
            state = {}
        last_update = state.get("last_update_date")
        if last_update:
            sources.append(str(last_update))

    if not sources:
        return ""
    anchor = min(sources)
    today = date.today().isoformat()
    return today if anchor > today else anchor


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


async def fetch_by_update_date(
    session: AsyncSession,
    throttle: Throttle,
    report_name: str,
    anchor: str,
    page_size: int = EM_PAGE_SIZE,
) -> list[dict]:
    """Fetch all rows with UPDATE_DATE >= anchor (incremental revision detect).

    Single UPDATE_DATE filter instead of per-period REPORTDATE enumeration:
    catches revisions to any previously-fetched report period.  Sorted by
    UPDATE_DATE so pagination walks revisions in stable order; total_pages is
    capped at 500 and pagination progress is logged to stderr.
    """
    all_records = []
    page = 1
    total_pages = 1
    data: dict | None = None

    while page <= total_pages:
        params = {
            "reportName": report_name,
            "columns": "ALL",
            "filter": f"(UPDATE_DATE>='{anchor}')",
            "sortColumns": "UPDATE_DATE",
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

        if data is None:
            print("    No data returned", file=sys.stderr)
            break
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
        if page > 1 or total_pages > 1:
            print(f"    page {page}/{total_pages}", file=sys.stderr)
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
        "--years", default="", help="Comma-separated years to fetch (default: all 2020-now)"
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

    # Handle incremental mode: RPT_LICO_FN_CPD uses the UPDATE_DATE time anchor
    # (detects revisions to any report period; ignores --years/--periods).
    # Other report names keep the old REPORTDATE-window behavior.
    cpd = report_name == "RPT_LICO_FN_CPD"
    anchor = ""
    if args.incremental:
        if cpd:
            anchor = _update_anchor(report_name, state_path)
            if anchor:
                print(f"Incremental: UPDATE_DATE>='{anchor}'", file=sys.stderr)
            else:
                print("No prior data found, fetching full history.", file=sys.stderr)
        else:
            since = _last_report_date(report_name, state_path)
            if since:
                print(f"Incremental: last data from {since}", file=sys.stderr)
                all_dates = [d for d in all_dates if d >= since]
            else:
                print("No prior data found, fetching full history.", file=sys.stderr)
            if not all_dates:
                print("No new report periods to fetch.", file=sys.stderr)
                return

    output_path = Path(args.output) if args.output else csv_dir() / f"{report_name}.csv"
    page_size = args.page_size

    print(f"Report: {report_name}", file=sys.stderr)
    if anchor:
        print(
            f"Update filter: UPDATE_DATE>='{anchor}' (ignores --years/--periods)",
            file=sys.stderr,
        )
    else:
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
    max_update_date = ""
    first_write = not output_path.exists()  # appending if file already exists

    async with AsyncSession(impersonate="chrome142") as session:
        if anchor:
            print(
                f"[1/1] UPDATE_DATE>='{anchor}' ...", file=sys.stderr, end=" ", flush=True
            )
            try:
                records = await fetch_by_update_date(
                    session, throttle, report_name, anchor, page_size
                )
            except Exception as e:
                print(f"FAILED: {e}", file=sys.stderr)
                records = []

            if records:
                write_csv(records, output_path, append=not first_write)
                first_write = False
                total_records += len(records)
                for rec in records:
                    rpt = rec.get("REPORTDATE") or ""
                    if rpt:
                        max_report_date = max(max_report_date, rpt)
                    upd = _normalize_update_date(rec.get("UPDATE_DATE"))
                    if upd:
                        max_update_date = max(max_update_date, upd)
                dedupe_csv(output_path)
                print(f"{len(records)} records", file=sys.stderr)
            else:
                print("empty", file=sys.stderr)
        else:
            for i, report_date in enumerate(all_dates):
                date_label = report_date
                print(
                    f"[{i + 1}/{len(all_dates)}] {date_label} ...",
                    file=sys.stderr, end=" ", flush=True
                )

                try:
                    records = await fetch_period(
                        session, throttle, report_name, report_date, page_size
                    )
                except Exception as e:
                    print(f"FAILED: {e}", file=sys.stderr)
                    continue

                if records:
                    write_csv(records, output_path, append=not first_write)
                    first_write = False
                    max_report_date = max(max_report_date, report_date)
                    for rec in records:
                        upd = _normalize_update_date(rec.get("UPDATE_DATE"))
                        if upd:
                            max_update_date = max(max_update_date, upd)
                    dedupe_csv(output_path)
                    print(f"{len(records)} records", file=sys.stderr)
                else:
                    print("empty", file=sys.stderr)

                total_records += len(records)

    if total_records > 0:
        state = {
            "last_report_date": max_report_date,
            "total_rows": total_records,
            "last_run": datetime.now().isoformat(),
        }
        if args.incremental and cpd:
            if not max_update_date:
                # All fetched rows had empty/missing UPDATE_DATE: preserve the
                # previous anchor (never advance, never regress).
                try:
                    prev = (
                        json.loads(state_path.read_text()).get("last_update_date", "")
                        if state_path.exists()
                        else ""
                    )
                except (json.JSONDecodeError, OSError):
                    prev = ""
                max_update_date = prev
            today = date.today().isoformat()
            if max_update_date > today:
                max_update_date = today
            state["last_update_date"] = max_update_date
        state_path.write_text(json.dumps(state, indent=2))

    print(f"\nDone: {total_records} records → {output_path.resolve()}", file=sys.stderr)
    if state_path.exists():
        print(
            f"State: {state_path} → last_report_date={state_path.read_text().strip()[:80]}...",
            file=sys.stderr,
        )


if __name__ == "__main__":  # pragma: no cover — __main__ block, never executed under pytest
    asyncio.run(main())
