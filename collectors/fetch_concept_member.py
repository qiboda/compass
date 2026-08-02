#!/usr/bin/env python3
"""A-share concept board membership collector (概念板块成分).

Independent module — uses common.py for shared infrastructure.

Report: RPT_F10_CORETHEME_BOARDTYPE (concept board members, EastMoney datacenter).
Unlike the financial-statement collectors, this one tracks *versions* rather
than per-reporting-period snapshots: every run fully replaces the previous
version (DELETE + full INSERT), so a symbol removed from a board disappears
from the table.  No incremental append by trading day.

Flow:
1. Fetch the concept board list (push2 clist, fs=m:90+t:3) → (board code, name).
2. For each board, fetch its members filtered by NEW_BOARD_CODE.
3. Write a CSV with raw EastMoney fields; import_to_dolt maps them into the
   concept_member schema (symbol gets the SH600519 prefix convention).
"""

import argparse
import asyncio
import random
import sys
from pathlib import Path

from common import (
    CFFI_SESSION,
    EM_BASE,
    EM_HEADERS,
    EM_MAX_RETRIES,
    AsyncSession,
    Throttle,
    dolt_sql,
    dolt_sql_csv,
    dolt_table_import,
    flatten_record,
    write_csv,
)

REPORT_NAME = "RPT_F10_CORETHEME_BOARDTYPE"
DOLT_TABLE = "concept_member"

# Concept board list — push2 clist (fs=m:90+t:3 selects concept boards).
BOARD_LIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"
BOARD_LIST_PAGE_SIZE = 100
BOARD_LIST_UT = "bd1d9ddb04089700cf9c27f6f7426281"
BOARD_HEADERS = {**EM_HEADERS, "Referer": "https://quote.eastmoney.com/center/boardlist.html"}

DDL = """\
CREATE TABLE concept_member (
    concept_code VARCHAR(20) NOT NULL,
    symbol       VARCHAR(20) NOT NULL,
    concept_name VARCHAR(50),
    update_date  DATE,
    PRIMARY KEY (concept_code, symbol)
)"""

# CSV columns written by run() — raw EastMoney fields; symbol/update_date
# are derived in import_to_dolt.
COLS = "SECUCODE, SECURITY_CODE, NEW_BOARD_CODE, BOARD_NAME"


async def fetch_board_list(
    session: CFFI_SESSION, throttle: Throttle,
) -> list[tuple[str, str]]:
    """Fetch all concept boards as (code, name) pairs from push2 clist.

    Response shape: ``{"data": {"total": N, "diff": [{"f12": "BK1169",
    "f14": "Kimi概念"}, ...]}}`` — paginated by ``pn``/``pz``.
    """
    boards: list[tuple[str, str]] = []
    page = 1
    while True:
        params = {
            "pn": page,
            "pz": BOARD_LIST_PAGE_SIZE,
            "po": "1",
            "np": "1",
            "ut": BOARD_LIST_UT,
            "fltt": "2",
            "invt": "2",
            "fid": "f12",
            "fs": "m:90+t:3",
            "fields": "f12,f14",
        }
        data = None
        for attempt in range(EM_MAX_RETRIES):
            try:
                await throttle.acquire()
                resp = await session.get(BOARD_LIST_URL, params=params, headers=BOARD_HEADERS)
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

        diff = ((data or {}).get("data") or {}).get("diff") or []
        total = int(((data or {}).get("data") or {}).get("total") or 0)
        boards.extend(
            (item["f12"], item["f14"])
            for item in diff
            if item.get("f12")
        )
        if not diff or len(boards) >= total:
            break
        page += 1

    return boards


async def fetch_board_members(
    session: CFFI_SESSION,
    throttle: Throttle,
    board_code: str,
    page_size: int,
) -> list[dict]:
    """Fetch all members of one concept board via NEW_BOARD_CODE filter.

    Note: this datacenter report requires double-quoted filter values
    (``(NEW_BOARD_CODE="BK1169")``); single quotes are ignored and return
    the full 90k-row table.  Hence the filter is built here rather than
    reusing ``fetch_paginated``.
    """
    all_records: list[dict] = []
    page = 1
    total_pages = 1

    while page <= total_pages:
        params = {
            "reportName": REPORT_NAME,
            "columns": "ALL",
            "filter": f'(NEW_BOARD_CODE="{board_code}")',
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


async def run(page_size: int = 100) -> Path:
    """Fetch concept board list, then per-board members into a CSV.

    Version-tracking semantics: the CSV always holds the *current* version
    (full replace at import time) — nothing is appended per trading day.
    """
    output_path = Path(f"{REPORT_NAME}.csv")

    print(f"Report: {REPORT_NAME}", file=sys.stderr)
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    total_records = 0

    async with AsyncSession(impersonate="chrome142") as session:
        try:
            boards = await fetch_board_list(session, throttle)
        except Exception as e:
            print(f"Board list fetch FAILED: {e}", file=sys.stderr)
            return output_path
        if not boards:
            print("No concept boards returned", file=sys.stderr)
            return output_path

        print(f"Boards: {len(boards)}", file=sys.stderr)
        all_records: list[dict] = []

        for i, (board_code, board_name) in enumerate(boards):
            print(
                f"[{i + 1}/{len(boards)}] {board_code} {board_name} ...",
                file=sys.stderr, end=" ", flush=True,
            )
            try:
                records = await fetch_board_members(session, throttle, board_code, page_size)
            except Exception as e:
                print(f"FAILED: {e}", file=sys.stderr)
                continue

            if records:
                all_records.extend(records)
                print(f"{len(records)} records", file=sys.stderr)
            else:
                print("empty", file=sys.stderr)
            total_records += len(records)

        write_csv(all_records, output_path)

    print(f"\nDone: {total_records} records → {output_path.resolve()}", file=sys.stderr)
    return output_path


def import_to_dolt(csv_path: Path | None = None) -> int:
    """Import the current version CSV, fully replacing any previous one.

    Version-tracking semantics: the previous table is renamed aside, a fresh
    ``concept_member`` is created and filled with the current members
    (``update_date = CURDATE()``), then the old version is dropped.  On
    INSERT failure the previous version is restored.
    """
    csv_path = csv_path or Path(f"{REPORT_NAME}.csv")
    print("[import concept_member]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0

    tmp_table = "_tmp_cm"
    if not dolt_table_import(tmp_table, csv_path):
        print("  Import failed", file=sys.stderr)
        return 0

    dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}_old")
    exists = dolt_sql_csv(
        f"SELECT COUNT(*) FROM information_schema.tables "
        f"WHERE table_name='{DOLT_TABLE}'"
    ).strip().split("\n")[-1].strip()
    if exists == "1":
        dolt_sql(f"RENAME TABLE {DOLT_TABLE} TO {tmp_table}_old")
    dolt_sql(DDL)

    sql = f"""
        INSERT INTO {DOLT_TABLE} (concept_code, symbol, concept_name, update_date)
        SELECT
            NEW_BOARD_CODE,
            CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
            BOARD_NAME,
            CURDATE()
        FROM {tmp_table}
    """
    result = dolt_sql(sql, timeout=600)
    if result.returncode != 0:
        print(f"  SQL error: {result.stderr}", file=sys.stderr)
        dolt_sql(f"DROP TABLE IF EXISTS {DOLT_TABLE}")
        old_exists = dolt_sql_csv(
            f"SELECT COUNT(*) FROM information_schema.tables "
            f"WHERE table_name='{tmp_table}_old'"
        ).strip().split("\n")[-1].strip()
        if old_exists == "1":
            dolt_sql(f"RENAME TABLE {tmp_table}_old TO {DOLT_TABLE}")
            print("  Rolled back to previous data", file=sys.stderr)
        dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}")
        return 0

    dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}")
    dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}_old")

    stdout = dolt_sql_csv(f"SELECT COUNT(*) FROM {DOLT_TABLE}")
    lines = stdout.strip().split("\n")
    total = int(lines[-1]) if len(lines) > 1 else 0

    dolt_sql(
        f"INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) "
        f"VALUES ('{DOLT_TABLE}', CURDATE(), 'EastMoney datacenter {REPORT_NAME}', {total}, CURDATE()) "
        f"ON DUPLICATE KEY UPDATE last_updated=CURDATE(), row_count={total}, "
        f"last_report_date=VALUES(last_report_date)"
    )
    print(f"  Done: {total} rows", file=sys.stderr)
    return total


if __name__ == "__main__":
    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share concept board members")
        p.add_argument("--page-size", type=int, default=100)
        args = p.parse_args()
        await run(page_size=args.page_size)

    asyncio.run(_main())
