#!/usr/bin/env python3
"""A-share block trade collector (大宗交易).

Independent module — uses common.py for shared infrastructure.

Report:  RPT_DATA_BLOCKTRADE (EastMoney datacenter block-trade details),
         filter column TRADE_DATE. Default: START_YEAR onwards, one API call
         per calendar day (non-trading days simply return no records).
         Incremental via data_updates.last_report_date.

NOTE: the originally planned report name "RPT_BLOCKTRADE_DETAILS" is rejected
by the datacenter API (code 9501, 报表配置不存在); the live report is
RPT_DATA_BLOCKTRADE with fields DEAL_PRICE / DEAL_VOLUME / DEAL_AMT /
BUYER_NAME / SELLER_NAME / PREMIUM_RATIO.
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

REPORT_NAME = "RPT_DATA_BLOCKTRADE"
FILTER_COLUMN = "TRADE_DATE"
DOLT_TABLE = "block_trade"
START_YEAR = 2024

DDL = """\
CREATE TABLE block_trade (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    price DOUBLE NOT NULL,
    volume DOUBLE, amount DOUBLE,
    buyer VARCHAR(100), seller VARCHAR(100),
    premium_rate DOUBLE,
    update_date DATE,
    PRIMARY KEY (symbol, trade_date, price)
)"""

# INSERT column list (lowercase DDL names), values mapped from API fields
INSERT_COLS = (
    "symbol, trade_date, price, volume, amount, buyer, seller, premium_rate, update_date"
)


def _daily_dates(years: list[int]) -> list[str]:
    """All calendar days of the given years as ISO date strings.

    Deliberately not a trading calendar — weekends/holidays simply return no
    records from the API, and incremental runs filter these against
    last_report_date.
    """
    dates: list[str] = []
    for year in years:
        day = date(year, 1, 1)
        end = date(year, 12, 31)
        while day <= end:
            dates.append(day.isoformat())
            day += timedelta(days=1)
    return dates


async def run(
    years: list[int] | None = None,
    page_size: int = 100,
) -> Path:
    if years is None:
        years = list(range(START_YEAR, datetime.now().year + 1))

    output_path = Path(f"{REPORT_NAME}.csv")
    all_dates = _daily_dates(years)

    since = last_report_date(DOLT_TABLE)
    if since:
        print(f"Last trade date in Dolt: {since}, fetching only newer dates", file=sys.stderr)
        all_dates = [d for d in all_dates if d >= since]
        if not all_dates:
            print("No new trade dates to fetch.", file=sys.stderr)
            return output_path

    print(f"Report: {REPORT_NAME}", file=sys.stderr)
    print(
        f"Dates: {len(all_dates)} ({all_dates[0] if all_dates else 'none'}.."
        f"{all_dates[-1] if all_dates else 'none'})",
        file=sys.stderr,
    )
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    total_records = 0
    first_write = True

    async with AsyncSession(impersonate="chrome142") as session:
        for i, trade_date in enumerate(all_dates):
            print(
                f"[{i + 1}/{len(all_dates)}] {trade_date} ...",
                file=sys.stderr, end=" ", flush=True,
            )
            try:
                records = await fetch_paginated(
                    session, throttle, REPORT_NAME, FILTER_COLUMN, trade_date, page_size,
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
    print("[import block_trade]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0

    tmp_table = "_tmp_bt"
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

    symbol_expr = "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)"
    sql = f"""
        INSERT INTO {DOLT_TABLE} ({INSERT_COLS})
        SELECT
            {symbol_expr}, DATE(TRADE_DATE), DEAL_PRICE, DEAL_VOLUME, DEAL_AMT,
            BUYER_NAME, SELLER_NAME, PREMIUM_RATIO, CURDATE()
        FROM {tmp_table}
        WHERE {symbol_expr} IN (SELECT symbol FROM stock_basic)
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
        f"SELECT MAX(trade_date) FROM {DOLT_TABLE}"
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
        p = argparse.ArgumentParser(description="Fetch A-share block trade data")
        p.add_argument("--years", default="")
        p.add_argument("--page-size", type=int, default=100)
        args = p.parse_args()
        await run(
            years=[int(y) for y in args.years.split(",") if y] or None,
            page_size=args.page_size,
        )

    asyncio.run(_main())
