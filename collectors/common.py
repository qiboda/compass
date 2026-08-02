"""Shared utilities for EastMoney financial data collectors.

Provides throttled HTTP fetching, CSV output, date range building,
and state-file management — used by all collector modules.
"""

import asyncio
import csv
import os
import random
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, TypeAlias

from curl_cffi.requests import AsyncSession

# curl_cffi AsyncSession is generic over response type; pin to Any
CFFI_SESSION: TypeAlias = AsyncSession[Any]

__all__ = [
    "AsyncSession",
    "CFFI_SESSION",
    "Throttle",
    "build_dates",
    "dolt_sql",
    "dolt_sql_csv",
    "dolt_table_import",
    "fetch_paginated",
    "flatten_record",
    "import_replace_table",
    "last_report_date",
    "write_csv",
]

# ── API constants ───────────────────────────────────────────────

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
EM_PAGE_SIZE = 100

# Dolt directory — respects COMPASS_DATA_DIR env, defaults to /data/compass-data/compass_data
_DEFAULT_DOLT = Path("/data/compass-data/compass_data")


def dolt_dir() -> Path:
    """Resolve the Dolt data directory at call time (env override + testability)."""
    return Path(os.environ.get("COMPASS_DATA_DIR", str(_DEFAULT_DOLT)))


# ── Throttle ────────────────────────────────────────────────────

class Throttle:
    """Rate-limiter with jitter to avoid triggering API throttling."""

    def __init__(self, min_interval: float = EM_MIN_INTERVAL):
        self._min_interval = min_interval
        self._last: float = 0.0

    async def acquire(self) -> None:
        now = time.monotonic()
        since_last = now - self._last
        if since_last < self._min_interval:
            wait = self._min_interval - since_last + random.uniform(*EM_JITTER)
            await asyncio.sleep(wait)
        else:
            await asyncio.sleep(random.uniform(0, 0.15))
        self._last = time.monotonic()


# ── Dolt helpers ────────────────────────────────────────────────

def dolt_sql(sql: str, timeout: int = 300) -> subprocess.CompletedProcess[str]:
    """Run a Dolt SQL query against compass_data."""
    args = ["dolt", "--data-dir", str(dolt_dir()), "sql", "-q", sql]
    return subprocess.run(args, capture_output=True, text=True, timeout=timeout)


def dolt_sql_csv(sql: str, timeout: int = 300) -> str:
    """Run a Dolt SQL query and return stdout as text."""
    args = ["dolt", "--data-dir", str(dolt_dir()), "sql", "-r", "csv", "-q", sql]
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    return result.stdout


def dolt_table_import(table_name: str, csv_path: Path, timeout: int = 300) -> bool:
    """Import CSV into a Dolt table. Returns True on success."""
    csv_abs = csv_path.resolve()
    result = subprocess.run(
        [
            "dolt", "--data-dir", str(dolt_dir()),
            "table", "import", "-c", table_name, "--continue", str(csv_abs),
        ],
        capture_output=True, text=True, timeout=timeout,
    )
    if result.returncode != 0:
        print(f"  dolt import error: {result.stderr.strip()}", file=sys.stderr)
    return result.returncode == 0


def last_report_date(dolt_table: str) -> str:
    """Get last REPORT_DATE from data_updates table.

    Returns empty string if no record exists (triggers full fetch).
    """
    if not (dolt_dir() / ".dolt").exists():
        return ""
    stdout = dolt_sql_csv(
        f"SELECT last_report_date FROM data_updates WHERE table_name='{dolt_table}'"
    )
    lines = stdout.strip().split("\n")
    last = lines[-1].strip() if len(lines) > 1 else ""
    if last and last != "NULL":
        return last
    return ""


def _table_exists(table_name: str) -> str:
    """Count rows for a table in information_schema (last CSV cell, '0'/'1')."""
    stdout = dolt_sql_csv(
        f"SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{table_name}'"
    )
    lines = stdout.strip().split("\n")
    return lines[-1].strip() if len(lines) > 1 else "0"


def import_replace_table(
    csv_path: Path,
    tmp_name: str,
    ddl: str,
    insert_sql: str,
    dolt_table: str,
    source_label: str,
    last_report_expr: str,
) -> int:
    """Atomically replace ``dolt_table`` with the CSV content.

    Flow: CSV → temp table import → old table renamed aside → DDL creates the
    fresh table → INSERT SELECT fills it → on any failure the fresh table is
    dropped and the old one renamed back → on success both temp tables are
    dropped → data_updates gets a 5-column upsert (last_report_date from
    ``last_report_expr``). Returns the imported row count, or 0 when the CSV
    is missing or the import fails (previous table contents are preserved).
    """
    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0

    if not dolt_table_import(tmp_name, csv_path):
        print("  Import failed", file=sys.stderr)
        return 0

    old_name = f"{tmp_name}_old"
    dolt_sql(f"DROP TABLE IF EXISTS {old_name}")
    if _table_exists(dolt_table) == "1":
        dolt_sql(f"RENAME TABLE {dolt_table} TO {old_name}")

    created = dolt_sql(ddl).returncode == 0
    result = dolt_sql(insert_sql, timeout=600) if created else None
    if result is None or result.returncode != 0:
        if created:
            dolt_sql(f"DROP TABLE IF EXISTS {dolt_table}")
        if _table_exists(old_name) == "1":
            dolt_sql(f"RENAME TABLE {old_name} TO {dolt_table}")
            print("  Rolled back to previous data", file=sys.stderr)
        dolt_sql(f"DROP TABLE IF EXISTS {tmp_name}")
        return 0

    dolt_sql(f"DROP TABLE IF EXISTS {tmp_name}")
    dolt_sql(f"DROP TABLE IF EXISTS {old_name}")

    stdout = dolt_sql_csv(f"SELECT COUNT(*) FROM {dolt_table}")
    lines = stdout.strip().split("\n")
    total = int(lines[-1]) if len(lines) > 1 else 0
    last_val = (
        dolt_sql_csv(f"SELECT {last_report_expr} FROM {dolt_table}")
        .strip()
        .split("\n")[-1]
        .strip()
    )
    last_val = "NULL" if (not last_val or last_val == "NULL") else f"'{last_val}'"

    dolt_sql(
        f"INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) "
        f"VALUES ('{dolt_table}', CURDATE(), '{source_label}', {total}, {last_val}) "
        f"ON DUPLICATE KEY UPDATE last_updated=CURDATE(), row_count={total}, "
        f"last_report_date=VALUES(last_report_date)"
    )
    print(f"  Done: {total} rows", file=sys.stderr)
    return total


# ── Data fetching ───────────────────────────────────────────────

def flatten_record(item: dict[str, object]) -> dict[str, str | int | float]:
    """Flatten nested fields for CSV export."""
    record: dict[str, str | int | float] = {}
    for k, v in item.items():
        if v is None:
            record[k] = ""
        elif isinstance(v, (int, float, str)):
            record[k] = v
        else:
            record[k] = str(v)
    return record


async def fetch_paginated(
    session: CFFI_SESSION,
    throttle: Throttle,
    report_name: str,
    filter_column: str,
    report_date: str,
    page_size: int = EM_PAGE_SIZE,
) -> list[dict[str, str | int | float]]:
    """Fetch all pages for a single report period.

    Args:
        session: curl_cffi AsyncSession with TLS impersonation.
        throttle: Rate limiter instance.
        report_name: EastMoney report name (e.g. RPT_DMSK_FN_BALANCE).
        filter_column: Filter column name (REPORT_DATE or REPORTDATE).
        report_date: Date string to filter by (e.g. '2024-12-31').
        page_size: Records per page (max 100 for this API).
    """
    all_records: list[dict[str, str | int | float]] = []
    page = 1
    total_pages = 1

    while page <= total_pages:
        params = {
            "reportName": report_name,
            "columns": "ALL",
            "filter": f"({filter_column}='{report_date}')",
            "sortColumns": "SECURITY_CODE",
            "sortTypes": "1",
            "pageSize": page_size,
            "pageNumber": page,
            "source": "WEB",
            "client": "WEB",
        }

        data = None
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
                wait = min(2 ** attempt, 30) + random.uniform(0, 3)
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

        total_pages = min(result.get("pages", 1), 500)
        page += 1

    return all_records


# ── CSV output ──────────────────────────────────────────────────

def write_csv(
    records: list[dict[str, str | int | float]], filepath: Path, append: bool = False
) -> None:
    """Write records to CSV. Infers fieldnames from first record."""
    if not records:
        return
    mode = "a" if append and filepath.exists() else "w"
    write_header = not append or not filepath.exists()
    with open(filepath, mode, newline="", encoding="utf-8-sig") as f:
        writer = csv.DictWriter(f, fieldnames=list(records[0].keys()))
        if write_header:
            writer.writeheader()
        writer.writerows(records)


# ── Date builder ────────────────────────────────────────────────

_PERIOD_MAP: dict[str, str] = {
    "FY": "-12-31",
    "Q3": "-09-30",
    "Q2": "-06-30",
    "Q1": "-03-31",
}


def build_dates(years: list[int], periods: list[str]) -> list[str]:
    """Build sorted list of date strings for given years and period codes.

    Args:
        years: e.g. [2020, 2021, 2022]
        periods: e.g. ['Q1', 'Q2', 'Q3', 'FY']
    """
    dates: list[str] = []
    for p in periods:
        suffix = _PERIOD_MAP.get(p)
        if suffix:
            dates.extend(f"{y}{suffix}" for y in years)
    dates.sort()
    return dates
