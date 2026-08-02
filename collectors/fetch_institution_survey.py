#!/usr/bin/env python3
"""A-share institution survey collector (机构调研).

Independent module — uses common.py for shared infrastructure.

Report:  RPT_ORG_SURVEYNEW, filter column NOTICE_DATE (announcement date;
the SURVEY_DATE column does not exist on this endpoint, verified 2026-08-02).
Per-survey rows carry RECEIVE_START_DATE (survey date), RECEIVE_OBJECT
(investigating institution) and RECEIVE_WAY_EXPLAIN (survey method).
Default: 2025-08 onwards (EastMoney keeps a rolling ~1 year window).
"""

import asyncio
import sys
from datetime import date, datetime, timedelta
from pathlib import Path

from common import (
    AsyncSession,
    Throttle,
    dolt_sql,
    dolt_sql_csv,
    dolt_table_import,
    fetch_paginated,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_ORG_SURVEYNEW"
FILTER_COLUMN = "NOTICE_DATE"
DOLT_TABLE = "institution_survey"
START_DATE = "2025-08-01"

DDL = """\
CREATE TABLE institution_survey (
    symbol      VARCHAR(20) NOT NULL,
    survey_date DATE NOT NULL,
    org_name    VARCHAR(100) NOT NULL,
    survey_type VARCHAR(20),
    update_date DATE,
    PRIMARY KEY (symbol, survey_date, org_name)
)"""

# API source columns mapped into DDL columns:
# org_name ← RECEIVE_OBJECT (investigating institution, e.g. 长信基金),
# survey_type ← RECEIVE_WAY_EXPLAIN (survey method, e.g. 电话会议)
TARGET_COLS = "org_name, survey_type"


async def run(
    start_date: str | None = None,
    page_size: int = 100,
) -> Path:
    output_path = Path(f"{REPORT_NAME}.csv")

    since = last_report_date(DOLT_TABLE)
    if since:
        # Incremental: resume the day after the last recorded survey date.
        since_dt = datetime.strptime(since, "%Y-%m-%d").date()
        start = (since_dt + timedelta(days=1)).isoformat()
    else:
        start = start_date or START_DATE

    today = date.today().isoformat()
    all_dates: list[str] = []
    d = date.fromisoformat(start)
    while d.isoformat() <= today:
        all_dates.append(d.isoformat())
        d += timedelta(days=1)

    if not all_dates:
        print("No new survey dates to fetch.", file=sys.stderr)
        return output_path

    print(f"Report: {REPORT_NAME}", file=sys.stderr)
    print(
        f"Dates: {len(all_dates)} ({all_dates[0]}..{all_dates[-1]})",
        file=sys.stderr,
    )
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    total_records = 0
    first_write = True

    async with AsyncSession(impersonate="chrome142") as session:
        for i, notice_date in enumerate(all_dates):
            print(
                f"[{i + 1}/{len(all_dates)}] {notice_date} ...",
                file=sys.stderr, end=" ", flush=True,
            )
            try:
                records = await fetch_paginated(
                    session, throttle, REPORT_NAME, FILTER_COLUMN, notice_date, page_size,
                )
            except Exception as e:
                print(f"FAILED: {e}", file=sys.stderr)
                continue

            if records:
                write_csv(records, output_path, append=not first_write)
                first_write = False
                print(f"{len(records)} records", file=sys.stderr)
            else:
                print("empty", file=sys.stderr)
            total_records += len(records)

    print(f"\nDone: {total_records} records → {output_path.resolve()}", file=sys.stderr)
    return output_path


def import_to_dolt(csv_path: Path | None = None) -> int:
    csv_path = csv_path or Path(f"{REPORT_NAME}.csv")
    print("[import institution survey]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0

    tmp_table = "_tmp_svy"
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

    # One stock can receive multiple institutions on the same survey date
    # (verified: duplicate (code, receive_start_date, receive_object) rows
    # exist upstream), so dedupe via GROUP BY on the composite PK.
    sql = f"""
        INSERT INTO {DOLT_TABLE} (symbol, survey_date, {TARGET_COLS}, update_date)
        SELECT
            CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
            RECEIVE_START_DATE,
            RECEIVE_OBJECT,
            MAX(RECEIVE_WAY_EXPLAIN),
            MAX(CURDATE())
        FROM {tmp_table}
        WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
              IN (SELECT symbol FROM stock_basic)
        GROUP BY 1, 2, 3
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
    last_rpt = dolt_sql_csv(
        f"SELECT MAX(survey_date) FROM {DOLT_TABLE}"
    ).strip().split("\n")[-1].strip()
    last_rpt_val = "NULL" if (not last_rpt or last_rpt == "NULL") else f"'{last_rpt}'"

    dolt_sql(
        f"INSERT INTO data_updates (table_name, last_updated, source, row_count, last_report_date) "
        f"VALUES ('{DOLT_TABLE}', CURDATE(), 'EastMoney datacenter {REPORT_NAME}', {total}, {last_rpt_val}) "
        f"ON DUPLICATE KEY UPDATE last_updated=CURDATE(), row_count={total}, "
        f"last_report_date=VALUES(last_report_date)"
    )
    print(f"  Done: {total} rows", file=sys.stderr)
    return total


if __name__ == "__main__":
    import argparse

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share institution surveys")
        p.add_argument("--start-date", default="")
        p.add_argument("--page-size", type=int, default=100)
        args = p.parse_args()
        await run(
            start_date=args.start_date or None,
            page_size=args.page_size,
        )

    asyncio.run(_main())
