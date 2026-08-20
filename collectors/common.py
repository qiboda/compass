"""Shared utilities for EastMoney financial data collectors.

Provides throttled HTTP fetching, CSV output, date range building,
and state-file management — used by all collector modules.
"""

import asyncio
import contextlib
import csv
import json
import logging
import os
import random
import re
import subprocess
import sys
import time
from datetime import date, datetime
from pathlib import Path
from typing import Any, TypeAlias

from curl_cffi.requests import AsyncSession

from proxy_pool_client import DEFAULT_PROXY_MAX_ATTEMPTS, ProxyPool, proxy_enabled

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
    "drop_name_en_mapping",
    "load_name_en_mapping",
    "name_en_mapping_path",
    "fetch_by_update_date",
    "fetch_incremental",
    "fetch_paginated",
    "flatten_record",
    "normalize_update_date",
    "update_date_anchor",
    "make_proxy_pool",
    "Progress",
    "progress_path",
    "proxy_get",
    "proxy_get_sync",
    "proxy_post",
    "proxy_post_sync",
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
EM_MIN_INTERVAL = 2.0
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

    Defensive: characters outside ``[A-Za-z0-9_.-]`` (path separators, spaces,
    etc.) are replaced with ``_``. Dots are kept, but since the name is always
    joined as a single filename segment under ``csv_dir()`` they can never act
    as directory separators — a hostile or buggy name cannot write outside
    ``csv_dir()``. Two distinct names may sanitize to the same path (e.g.
    ``a/b`` and ``a_b``); wired-in collectors use plain identifiers, so this
    is not a collision risk in practice.
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
        if self.total_items and self.total_items > 0:
            # Clamp to [0, 100]: a negative completed count (buggy caller)
            # must never leak a negative percent into the live-query file.
            # total_items <= 0 means no meaningful denominator → percent None.
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
        # PID-suffixed tmp avoids two same-named collector processes racing
        # on one tmp file; os.replace stays atomic for readers either way.
        tmp = self.path.with_name(f"{self.path.name}.{os.getpid()}.tmp")
        try:
            tmp.write_text(
                json.dumps(self._data(), ensure_ascii=False, indent=2),
                encoding="utf-8",
            )
            os.replace(tmp, self.path)
        except OSError as exc:
            # Progress is best-effort: a failing progress write (disk full,
            # permissions) must never crash the collector nor mask the
            # original fetch error via __exit__ → fail() re-write.
            logger.warning("progress write failed for %s: %s", self.name, exc)

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


# Name-en mapping CSV (epic #266 B1): section,key,value three-section table
# (index symbol → name_en / industry zh → industry_en).
# Path injection hook honoured by tests (ref #236): COMPASS_NAME_EN_MAPPING.
NAME_EN_MAPPING_ENV = "COMPASS_NAME_EN_MAPPING"
NAME_EN_MAPPING_TMP = "_tmp_name_en"
NAME_EN_MAPPING_DDL = (
    "CREATE TABLE _tmp_name_en (section VARCHAR(20), `key` VARCHAR(100), value VARCHAR(100))"
)


def name_en_mapping_path() -> Path:
    """Resolve the name-en mapping CSV path (env hook or checked-in default)."""
    env = os.environ.get(NAME_EN_MAPPING_ENV)
    if env:
        return Path(env)
    return Path(__file__).resolve().parent / "name_en_mapping.csv"


def load_name_en_mapping(tmp_name: str = NAME_EN_MAPPING_TMP) -> bool:
    """Stage the name-en mapping CSV into a Dolt temp table; True when loaded.

    Reads ``COMPASS_NAME_EN_MAPPING`` (tests) or the checked-in
    ``collectors/name_en_mapping.csv``; a missing file degrades to False and
    importers proceed with every en column NULL (epic #266 decision 4). The
    staging table ``_tmp_name_en`` carries ``(section, key, value)`` and is
    dropped by the caller after the JOIN. A stale staging table from a
    previous failed run is dropped first — otherwise the CREATE would fail
    and every en column would silently degrade to NULL (review P1-1).
    """
    path = name_en_mapping_path()
    if not path.exists():
        print(f"  name_en_mapping not found ({path}); en columns stay NULL", file=sys.stderr)
        return False
    dolt_sql(f"DROP TABLE IF EXISTS {tmp_name}")
    if not dolt_table_import(tmp_name, path, create_sql=NAME_EN_MAPPING_DDL):
        print("  name_en_mapping import failed; en columns stay NULL", file=sys.stderr)
        return False
    return True


def drop_name_en_mapping(tmp_name: str = NAME_EN_MAPPING_TMP) -> None:
    """Drop the name-en mapping staging table (caller cleanup)."""
    dolt_sql(f"DROP TABLE IF EXISTS {tmp_name}")


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


def normalize_update_date(value: object) -> str | None:
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


def update_date_anchor(
    report_name: str,
    state_path: Path,
    dolt_table: str | None = None,
) -> str:
    """Resolve the incremental UPDATE_DATE anchor for a report.

    Anchor = min(data_updates.last_updated (table for ``report_name`` or
    ``dolt_table``), state.json ``last_update_date``) — the EARLIER of the two
    sources, so a cross-day fetch/import or a standalone import can never push
    the anchor past rows updated in the fetch-import gap.

    - data_updates row with NULL/empty ``last_updated`` → that source missing
    - state.json lacking the ``last_update_date`` key (old single-key format)
      → that source missing
    - both sources missing → "" (caller decides fallback behavior)
    - anchor > today → clamped to today (no-update semantics)

    When ``dolt_table`` is provided it is used for the data_updates lookup
    (F10 tables: ``fin_balance_sheet`` etc.); otherwise the legacy mapping is
    kept (``RPT_LICO_FN_CPD`` → ``fin_indicators``, other report names use
    the report name as the Dolt table).
    """
    sources: list[str] = []
    if (dolt_dir() / ".dolt").exists():
        table = (
            dolt_table
            if dolt_table is not None
            else ("fin_indicators" if report_name == "RPT_LICO_FN_CPD" else report_name)
        )
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
        result = dolt_sql(insert_sql, timeout=3600) if created else None
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
        result = dolt_sql(insert_sql, timeout=3600) if created else None
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


# ── Proxy-first request wrappers (issue #294) ────────────────────


def make_proxy_pool() -> "ProxyPool | None":
    """Create a ``ProxyPool`` bound to the shared state file, or None when disabled."""
    if not proxy_enabled():
        return None
    return ProxyPool(state_path=csv_dir() / "proxy_pool_state.json")


async def proxy_get(
    session: CFFI_SESSION,
    pool: "ProxyPool | None",
    url: str,
    *,
    max_proxy_attempts: int = DEFAULT_PROXY_MAX_ATTEMPTS,
    **kwargs: Any,
) -> Any:
    """GET with proxy-first rotation.

    Tries up to ``max_proxy_attempts`` proxies from ``pool``; a request
    exception through a proxy deletes that proxy and moves to the next one.
    After the bounded attempts (or immediately when the pool is empty) the
    request is sent direct. HTTP status codes are returned to the caller and
    never treated as proxy failures.
    """
    if pool is None:
        return await session.get(url, **kwargs)
    attempts = max(0, int(max_proxy_attempts))
    last_exc: Exception | None = None
    for i in range(attempts + 1):
        proxy: str | None = None
        if i < attempts:
            proxy = await pool.get_proxy()
        try:
            request_kwargs = dict(kwargs)
            if proxy:
                request_kwargs["proxies"] = pool.proxy_spec(proxy)
            return await session.get(url, **request_kwargs)
        except Exception as exc:
            last_exc = exc
            if proxy:
                await pool.delete_proxy(proxy)
                continue
            raise
    assert last_exc is not None  # pragma: no cover - loop always returns or raises
    raise last_exc  # pragma: no cover


async def proxy_post(
    session: CFFI_SESSION,
    pool: "ProxyPool | None",
    url: str,
    *,
    max_proxy_attempts: int = DEFAULT_PROXY_MAX_ATTEMPTS,
    **kwargs: Any,
) -> Any:
    """POST variant of :func:`proxy_get`."""
    if pool is None:
        return await session.post(url, **kwargs)
    attempts = max(0, int(max_proxy_attempts))
    last_exc: Exception | None = None
    for i in range(attempts + 1):
        proxy: str | None = None
        if i < attempts:
            proxy = await pool.get_proxy()
        try:
            request_kwargs = dict(kwargs)
            if proxy:
                request_kwargs["proxies"] = pool.proxy_spec(proxy)
            return await session.post(url, **request_kwargs)
        except Exception as exc:
            last_exc = exc
            if proxy:
                await pool.delete_proxy(proxy)
                continue
            raise
    assert last_exc is not None  # pragma: no cover - loop always returns or raises
    raise last_exc  # pragma: no cover


def proxy_get_sync(
    session: Any,
    pool: "ProxyPool | None",
    url: str,
    *,
    max_proxy_attempts: int = DEFAULT_PROXY_MAX_ATTEMPTS,
    **kwargs: Any,
) -> Any:
    """Synchronous GET variant for ``requests.Session`` call-sites."""
    if pool is None:
        return session.get(url, **kwargs)
    attempts = max(0, int(max_proxy_attempts))
    last_exc: Exception | None = None
    for i in range(attempts + 1):
        proxy: str | None = None
        if i < attempts:
            # Sync context: call the sync hook directly; get_proxy is async,
            # so use a fresh event loop only when a live pool is configured.
            # Tests inject a stub whose ``get_proxy`` is async; keep this path
            # simple by expecting callers to pass an already-usable pool.
            proxy = _sync_get_proxy(pool)
        try:
            request_kwargs = dict(kwargs)
            if proxy:
                request_kwargs["proxies"] = pool.proxy_spec(proxy)
            return session.get(url, **request_kwargs)
        except Exception as exc:
            last_exc = exc
            if proxy:
                _sync_delete_proxy(pool, proxy)
                continue
            raise
    assert last_exc is not None  # pragma: no cover - loop always returns or raises
    raise last_exc  # pragma: no cover


def proxy_post_sync(
    session: Any,
    pool: "ProxyPool | None",
    url: str,
    *,
    max_proxy_attempts: int = DEFAULT_PROXY_MAX_ATTEMPTS,
    **kwargs: Any,
) -> Any:
    """Synchronous POST variant for ``requests.Session`` call-sites."""
    if pool is None:
        return session.post(url, **kwargs)
    attempts = max(0, int(max_proxy_attempts))
    last_exc: Exception | None = None
    for i in range(attempts + 1):
        proxy: str | None = None
        if i < attempts:
            proxy = _sync_get_proxy(pool)
        try:
            request_kwargs = dict(kwargs)
            if proxy:
                request_kwargs["proxies"] = pool.proxy_spec(proxy)
            return session.post(url, **request_kwargs)
        except Exception as exc:
            last_exc = exc
            if proxy:
                _sync_delete_proxy(pool, proxy)
                continue
            raise
    assert last_exc is not None  # pragma: no cover - loop always returns or raises
    raise last_exc  # pragma: no cover


def _sync_get_proxy(pool: "ProxyPool") -> str | None:
    """Run ``pool.get_proxy()`` from a synchronous call-site.

    The proxy_pool client is async-first; for the synchronous exchange
    collectors we need a small event-loop bridge. A dedicated loop per call is
    acceptable because these are rare local API calls, and the pool object has
    no cross-call async state that requires the same loop.
    """
    import asyncio

    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(pool.get_proxy())
    # Already inside a loop (rare in sync collectors): fall back to a direct
    # pool call without awaiting — tests use a synchronous stub via this hook.
    return _sync_get_proxy_fallback(pool)


def _sync_get_proxy_fallback(pool: "ProxyPool") -> str | None:
    """Best-effort sync proxy acquisition when no fresh loop is available."""
    try:
        # Tests monkeypatch ``_api_get`` with a plain sync callable; this
        # path keeps the sync wrappers testable without an event loop.
        data = pool._api_get("/get/", {"type": "https"})
    except Exception:
        return None
    if isinstance(data, dict):
        proxy = data.get("proxy")
        if isinstance(proxy, str) and proxy.strip():
            return proxy.strip()
    return None


def _sync_delete_proxy(pool: "ProxyPool", proxy: str) -> None:
    """Best-effort synchronous ``delete_proxy`` bridge."""
    import asyncio

    try:
        asyncio.get_running_loop()
    except RuntimeError:
        asyncio.run(pool.delete_proxy(proxy))
        return
    with contextlib.suppress(Exception):
        pool._api_get("/delete/", {"proxy": proxy})


async def fetch_paginated(
    session: CFFI_SESSION,
    throttle: Throttle,
    report_name: str,
    filter_column: str,
    report_date: str,
    page_size: int = EM_PAGE_SIZE,
    *,
    pool: "ProxyPool | None" = None,
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
                resp = await proxy_get(session, pool, EM_BASE, params=params, headers=EM_HEADERS)

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


async def fetch_by_update_date(
    session: CFFI_SESSION,
    throttle: Throttle,
    report_name: str,
    anchor: str,
    page_size: int = EM_PAGE_SIZE,
    *,
    pool: "ProxyPool | None" = None,
) -> list[dict[str, str | int | float]]:
    """Fetch all rows with UPDATE_DATE >= anchor (incremental revision detect).

    Single UPDATE_DATE filter instead of per-period REPORTDATE enumeration:
    catches revisions to any previously-fetched report period.  Sorted by
    UPDATE_DATE so pagination walks revisions in stable order; total_pages is
    capped at 500 and pagination progress is logged to stderr.
    """
    all_records: list[dict[str, str | int | float]] = []
    page = 1
    total_pages = 1

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

        data = None
        for attempt in range(EM_MAX_RETRIES):
            try:
                await throttle.acquire()
                resp = await proxy_get(session, pool, EM_BASE, params=params, headers=EM_HEADERS)

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


async def fetch_incremental(
    report_name: str,
    dolt_table: str,
    output_path: Path,
    state_path: Path,
    page_size: int = EM_PAGE_SIZE,
    initial_anchor: str = "2020-01-01",
    session_factory: Any = None,
    anchor_resolver: Any = None,
    fetch_fn: Any = None,
) -> int:
    """Run one UPDATE_DATE incremental fetch and write CSV/state (issue #299).

    Shared by the three F10 collectors (balance_sheet/income/cash_flow):
    resolves the anchor via :func:`update_date_anchor` (falling back to
    ``initial_anchor`` when both sources are missing), fetches every row with
    ``UPDATE_DATE>='{anchor}'``, writes the CSV (keep-LAST dedupe by
    ``(SECURITY_CODE, REPORT_DATE)``), and writes ``state_path`` when rows
    were returned.

    ``session_factory`` is the curl_cffi AsyncSession constructor (or a test
    fake); callers pass their module-level ``AsyncSession`` so tests can patch
    the module attribute as before. ``anchor_resolver`` and ``fetch_fn``
    default to :func:`update_date_anchor` / :func:`fetch_by_update_date`; the
    F10 modules pass their module-level imports so tests can monkeypatch them.

    Returns the number of records fetched (0 means an empty window or a
    fetch failure — the anchor/state is never advanced on empty results).
    """
    anchor = (anchor_resolver or update_date_anchor)(
        report_name, state_path, dolt_table=dolt_table
    )
    if not anchor:
        anchor = initial_anchor
        print(
            f"No prior anchor found; using UPDATE_DATE>='{anchor}' for one full-history pull",
            file=sys.stderr,
        )
    print(f"Report: {report_name}", file=sys.stderr)
    print(f"Update filter: UPDATE_DATE>='{anchor}' (ignores --years/--periods)", file=sys.stderr)
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    pool = make_proxy_pool()
    total_records = 0
    max_report_date = ""
    max_update_date = ""

    async with (session_factory or AsyncSession)(impersonate="chrome142") as session:
        print(
            f"[1/1] UPDATE_DATE>='{anchor}' ...", file=sys.stderr, end=" ", flush=True
        )
        try:
            records = await (fetch_fn or fetch_by_update_date)(
                session, throttle, report_name, anchor, page_size, pool=pool
            )
        except Exception as e:
            print(f"FAILED: {e}", file=sys.stderr)
            records = []

        if records:
            write_csv(records, output_path, append=False)
            total_records += len(records)
            for rec in records:
                rpt = str(rec.get("REPORT_DATE") or "")
                if rpt:
                    max_report_date = max(max_report_date, rpt)
                upd = normalize_update_date(rec.get("UPDATE_DATE"))
                if upd:
                    max_update_date = max(max_update_date, upd)
            dedupe_csv(output_path, date_col="REPORT_DATE")
            print(f"{len(records)} records", file=sys.stderr)
        else:
            print("empty", file=sys.stderr)

    if total_records > 0:
        state = {
            "last_report_date": max_report_date,
            "total_rows": total_records,
            "last_run": datetime.now().isoformat(),
        }
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

    return total_records


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


def dedupe_csv(path: Path, date_col: str = "REPORTDATE") -> None:
    """Dedupe a CSV file in place, keeping the LAST row per (SECURITY_CODE, <date_col>).

    Reads the whole file (utf-8-sig, BOM-safe) and rewrites it with the same
    header and column order. Keep-LAST means the final occurrence's values win
    for each PK; rows with distinct PKs keep their relative order. Empty files
    and files missing either PK column are left untouched (silent no-op).
    Rewriting is skipped when the file already has unique PKs.

    Called after every write so a revised row (same PK, newer UPDATE_DATE)
    overwrites the old one instead of duplicating it in the CSV. F10 collectors
    pass ``date_col="REPORT_DATE"`` because EastMoney F10 CSVs use the
    underscore form (issue #299).
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
            date_idx = header.index(date_col)
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
