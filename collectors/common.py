"""Shared utilities for EastMoney financial data collectors.

Provides throttled HTTP fetching, CSV output, date range building,
and state-file management — used by all collector modules.
"""

import asyncio
import csv
import json
import logging
import os
import random
import re
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any, TypeAlias

from curl_cffi.requests import AsyncSession

# curl_cffi AsyncSession is generic over response type; pin to Any
CFFI_SESSION: TypeAlias = AsyncSession[Any]

# Module logger: diagnostics (SQL errors, insert counts) go through logging so
# callers can route them; unconfigured roots fall back to stderr (basicConfig).
logger = logging.getLogger("compass_collectors")
if not logging.getLogger().handlers:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")

__all__ = [
    "AsyncSession",
    "CFFI_SESSION",
    "Throttle",
    "build_dates",
    "csv_dir",
    "dedupe_csv",
    "dolt_sql",
    "dolt_sql_csv",
    "dolt_table_import",
    "fetch_paginated",
    "flatten_record",
    "Progress",
    "progress_path",
    "read_progress",
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


# Raw CSV output directory — respects COMPASS_CSV_DIR env, defaults to
# /data/compass-data/csv (issue #208: keep raw fetched CSVs out of the Dolt
# repo working tree and out of the collectors source dir).
_DEFAULT_CSV = Path("/data/compass-data/csv")


def csv_dir() -> Path:
    """Resolve the raw CSV output directory at call time (env override + testability).

    Creates the directory (and parents) if missing, so collectors never crash
    with a bare FileNotFoundError on first write to a fresh COMPASS_CSV_DIR.
    """
    path = Path(os.environ.get("COMPASS_CSV_DIR", str(_DEFAULT_CSV)))
    path.mkdir(parents=True, exist_ok=True)
    return path


# ── Progress tracking ───────────────────────────────────────────


def progress_path(name: str) -> Path:
    """Return the JSON progress file path for a collector name.

    The file lives alongside the raw CSVs so a separate ``main.py progress``
    process can read it while the fetch is still running.

    Defensive: only plain filename characters (``[A-Za-z0-9_.-]``) are kept —
    anything else (path separators, ``..`` segments, spaces) is replaced with
    ``_`` so a hostile or buggy name can never write outside ``csv_dir()``.
    """
    safe = re.sub(r"[^A-Za-z0-9_.-]", "_", name)
    return csv_dir() / f"{safe}.progress.json"


def read_progress(name: str) -> dict[str, object] | None:
    """Read a collector's progress JSON, or None when absent/unreadable.

    A corrupt/partial file (e.g. a concurrent write) is treated as missing
    rather than crashing the query command.
    """
    path = progress_path(name)
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


class Progress:
    """Small JSON progress tracker for long-running collectors.

    Each update is written atomically to ``csv_dir()/<name>.progress.json`` so
    another process can query live progress without locking. The file is kept
    after completion with status ``completed`` or ``failed`` for post-run
    inspection.
    """

    def __init__(
        self,
        name: str,
        *,
        total_items: int | None = None,
        output_csv: str | Path | None = None,
        message: str = "starting",
    ) -> None:
        self.name = name
        self.path = progress_path(name)
        self.total_items = total_items
        self.output_csv = str(output_csv) if output_csv is not None else None
        self.completed_items = 0
        self.fetched_rows = 0
        self.current_item: str | None = None
        self.message = message
        self.status = "running"
        self.error: str | None = None
        self.started_at = datetime.now().isoformat(timespec="seconds")
        self.updated_at = self.started_at
        self._write()

    def _data(self) -> dict[str, object]:
        percent: float | None = None
        if self.total_items:
            # Clamp to [0, 100]: a negative completed count (buggy caller)
            # must never leak a negative percent into the live-query file.
            percent = round(min(max(self.completed_items, 0) / self.total_items * 100, 100.0), 2)
        return {
            "name": self.name,
            "status": self.status,
            "started_at": self.started_at,
            "updated_at": self.updated_at,
            "total_items": self.total_items,
            "completed_items": self.completed_items,
            "fetched_rows": self.fetched_rows,
            "current_item": self.current_item,
            "percent": percent,
            "message": self.message,
            "output_csv": self.output_csv,
            "error": self.error,
        }

    def _write(self) -> None:
        self.updated_at = datetime.now().isoformat(timespec="seconds")
        tmp = self.path.with_suffix(self.path.suffix + ".tmp")
        tmp.write_text(
            json.dumps(self._data(), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        os.replace(tmp, self.path)

    def update(
        self,
        *,
        completed: int | None = None,
        fetched_rows: int | None = None,
        current_item: str | None = None,
        message: str | None = None,
        total_items: int | None = None,
    ) -> None:
        if total_items is not None:
            self.total_items = total_items
        if completed is not None:
            self.completed_items = completed
        if fetched_rows is not None:
            self.fetched_rows = fetched_rows
        if current_item is not None:
            self.current_item = current_item
        if message is not None:
            self.message = message
        self._write()

    def finish(
        self,
        *,
        fetched_rows: int | None = None,
        message: str = "completed",
    ) -> None:
        self.status = "completed"
        if fetched_rows is not None:
            self.fetched_rows = fetched_rows
        if self.total_items is not None:
            self.completed_items = self.total_items
        self.message = message
        self.error = None
        self._write()

    def fail(self, error: str, *, message: str = "failed") -> None:
        self.status = "failed"
        self.error = str(error)
        self.message = message
        self._write()

    def __enter__(self) -> "Progress":
        return self

    def __exit__(self, exc_type: object, exc: object, tb: object) -> bool:
        if exc_type is not None:
            self.fail(str(exc) if exc else str(exc_type))
        return False


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


def dolt_table_import(
    table_name: str,
    csv_path: Path,
    timeout: int = 300,
    create_sql: str | None = None,
) -> bool:
    """Import CSV into a Dolt table. Returns True on success.

    With ``create_sql`` the table is created first with an explicit wide
    schema and the CSV is imported with ``-u`` (no type inference). Dolt's
    ``-c`` inference caps string columns at varchar(200) and truncates longer
    UTF-8 values mid-character, so long-text tables (e.g. institution survey
    org_name up to ~800 bytes) MUST pass ``create_sql``.
    """
    csv_abs = csv_path.resolve()
    if create_sql:
        create = subprocess.run(
            ["dolt", "--data-dir", str(dolt_dir()), "sql", "-q", create_sql],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if create.returncode != 0:
            print(f"  dolt create error: {create.stderr.strip()}", file=sys.stderr)
            return False
        mode = "-u"
    else:
        mode = "-c"
    result = subprocess.run(
        [
            "dolt",
            "--data-dir",
            str(dolt_dir()),
            "table",
            "import",
            mode,
            table_name,
            "--continue",
            str(csv_abs),
        ],
        capture_output=True,
        text=True,
        timeout=timeout,
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
    create_sql: str | None = None,
    merge: bool = False,
) -> int:
    """Import ``csv_path`` into ``dolt_table``.

    With ``merge=False`` (default) the table is atomically REPLACED: the CSV
    is staged in a temp table, the old table is renamed aside, a fresh table
    is created with ``ddl`` and filled via ``insert_sql``; any failure rolls
    back. With ``merge=True`` the CSV rows are upserted into the EXISTING
    table (created with ``ddl`` on first run), so incremental-window CSVs
    append to history instead of clobbering it. The caller's ``insert_sql``
    normally uses ``INSERT IGNORE INTO {dolt_table}`` with the PK deduping
    overlapping windows; a caller that must OVERWRITE revised rows (e.g.
    fin_indicators revision detection, issue #135) may pass an
    ``INSERT ... ON DUPLICATE KEY UPDATE`` statement instead — see
    ``main.py::_import_fin_indicators`` for the Dolt-2.2.3-compatible form
    (SELECT-side unique aliases + ODKU prefixless alias references; qualified
    source-column refs and ``VALUES()`` are rejected by Dolt).

    Flow (replace): CSV → optional wide temp create → old table renamed aside
    → DDL creates fresh table → INSERT SELECT fills → failure rolls back →
    data_updates upsert. Flow (merge): optional wide temp create → DDL
    CREATE IF NOT EXISTS → INSERT IGNORE/UPSERT SELECT → data_updates upsert.

    Returns the final full-table row count after import, or 0 when the CSV is
    missing or the import fails (previous table contents are preserved).
    In merge mode the number of rows actually inserted this run (the PK
    dedupes overlapping windows) is logged via ``logger.info``; failures log
    the SQL error via ``logger.error`` (never silently swallowed).
    """
    before_total = 0
    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0

    if not dolt_table_import(tmp_name, csv_path, create_sql=create_sql):
        print("  Import failed", file=sys.stderr)
        return 0

    if merge:
        before_total = int(
            dolt_sql_csv(f"SELECT COUNT(*) FROM {dolt_table}").strip().split("\n")[-1]
            if _table_exists(dolt_table) == "1"
            else 0
        )
        created = dolt_sql(ddl).returncode == 0
        result = dolt_sql(insert_sql, timeout=600) if created else None
        if result is None or result.returncode != 0:
            if result is not None and result.stderr:
                logger.error("  SQL error: %s", result.stderr.strip())
            dolt_sql(f"DROP TABLE IF EXISTS {tmp_name}")
            return 0
        dolt_sql(f"DROP TABLE IF EXISTS {tmp_name}")
    else:
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
        dolt_sql_csv(f"SELECT {last_report_expr} FROM {dolt_table}").strip().split("\n")[-1].strip()
    )
    last_val = "NULL" if (not last_val or last_val == "NULL") else f"'{last_val}'"

    dolt_sql(
        f"INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) "
        f"VALUES ('{dolt_table}', CURDATE(), '{source_label}', {total}, {last_val}) "
        f"ON DUPLICATE KEY UPDATE last_updated=CURDATE(), row_count={total}, "
        f"last_report_date=VALUES(last_report_date)"
    )
    if merge:
        inserted = max(total - before_total, 0)
        logger.info("  Done: %s rows (inserted %d this run)", dolt_table, inserted)
    else:
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


def dedupe_csv(path: Path) -> None:
    """Dedupe a CSV file in place, keeping the LAST row per (SECURITY_CODE, REPORTDATE).

    Reads the whole file (utf-8-sig, BOM-safe) and rewrites it with the same
    header and column order. Keep-LAST means the final occurrence's values win
    for each PK; rows with distinct PKs keep their relative order. Empty files
    and files missing either PK column are left untouched (silent no-op).
    Rewriting is skipped when the file already has unique PKs.

    Called after every write so a revised row (same PK, newer UPDATE_DATE)
    overwrites the old one instead of duplicating it in the CSV.
    """
    if not path.exists() or path.stat().st_size == 0:
        return
    with open(path, newline="", encoding="utf-8-sig") as f:
        reader = csv.reader(f)
        try:
            header = next(reader)
        except StopIteration:
            return
        try:
            code_idx = header.index("SECURITY_CODE")
            date_idx = header.index("REPORTDATE")
        except ValueError:
            return  # missing PK columns — leave the file untouched

        seen: dict[tuple[str, str], list[str]] = {}
        dupes = 0
        for row in reader:
            if not row or len(row) <= code_idx or len(row) <= date_idx:
                continue  # blank or malformed row — no key to dedupe on
            key = (row[code_idx], row[date_idx])
            if key in seen:
                dupes += 1
            seen[key] = row

    if dupes == 0:
        return

    with open(path, "w", newline="", encoding="utf-8-sig") as f:
        writer = csv.writer(f)
        writer.writerow(header)
        writer.writerows(seen.values())


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
