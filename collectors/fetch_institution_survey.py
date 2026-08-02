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
    fetch_paginated,
    import_replace_table,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_ORG_SURVEYNEW"
FILTER_COLUMN = "NOTICE_DATE"
DOLT_TABLE = "institution_survey"
START_DATE = "2025-08-01"

DDL = """\
CREATE TABLE IF NOT EXISTS institution_survey (
    symbol      VARCHAR(20) NOT NULL,
    survey_date DATE NOT NULL,
    org_name    VARCHAR(1000) NOT NULL,
    survey_type VARCHAR(300),
    update_date DATE,
    PRIMARY KEY (symbol, survey_date, org_name)
)"""

# API source columns mapped into DDL columns:
# org_name ← RECEIVE_OBJECT (investigating institution, e.g. 长信基金),
# survey_type ← RECEIVE_WAY_EXPLAIN (survey method, e.g. 电话会议)
INSERT_COLS = "org_name, survey_type"


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
    all_records: list[dict[str, str | int | float]] = []
    failure: str | None = None

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
                failure = f"{notice_date}: {e}"
                print(f"FAILED: {e}", file=sys.stderr)
                break

            all_records.extend(records)
            print(f"{len(records)} records" if records else "empty", file=sys.stderr)

    if failure is not None:
        output_path.unlink(missing_ok=True)
        raise RuntimeError(f"Fetch aborted at {failure} — no CSV written")

    write_csv(all_records, output_path)
    print(
        f"\nDone: {len(all_records)} records → {output_path.resolve()}",
        file=sys.stderr,
    )
    return output_path


def import_to_dolt(csv_path: Path | None = None) -> int:
    csv_path = csv_path or Path(f"{REPORT_NAME}.csv")
    print("[import institution survey]", file=sys.stderr)

    # One stock can receive multiple institutions on the same survey date
    # (verified: duplicate (code, receive_start_date, receive_object) rows
    # exist upstream). Dedup groups on the FULL composite key (symbol,
    # survey_date, org_name): the group key is the ASCII-safe
    # HEX(RECEIVE_OBJECT) joined with the already-derived s/d columns, so
    # distinct events of the same org (different symbol/date) each survive.
    # The temp table is created with an explicit wide schema: dolt's CSV type
    # inference caps strings at varchar(200) and truncates longer UTF-8
    # values mid-character (org_name up to ~800 bytes), so the inferred width
    # silently corrupts the data before any post-import ALTER could widen it.
    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_svy",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} (symbol, survey_date, {INSERT_COLS}, update_date)
            SELECT MAX(s), MAX(d), MAX(o), MAX(st), MAX(u) FROM (
                SELECT
                    CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE) AS s,
                    DATE(RECEIVE_START_DATE) AS d,
                    RECEIVE_OBJECT AS o,
                    RECEIVE_WAY_EXPLAIN AS st,
                    CURDATE() AS u,
                    HEX(RECEIVE_OBJECT) AS gk
                FROM _tmp_svy
                WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
                      IN (SELECT symbol FROM stock_basic)
                  AND RECEIVE_START_DATE IS NOT NULL
            ) t
            GROUP BY s, d, gk
        """,
        create_sql=(
            "CREATE TABLE _tmp_svy ("
            "SECUCODE VARCHAR(20), SECURITY_CODE VARCHAR(20), "
            "RECEIVE_START_DATE DATETIME, RECEIVE_OBJECT VARCHAR(1000), "
            "RECEIVE_WAY_EXPLAIN VARCHAR(500))"
        ),
        merge=True,
        dolt_table=DOLT_TABLE,
        source_label=f"EastMoney datacenter {REPORT_NAME}",
        last_report_expr="MAX(survey_date)",
    )


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
