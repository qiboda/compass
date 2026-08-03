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
    fetch_paginated,
    import_replace_table,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_DATA_BLOCKTRADE"
FILTER_COLUMN = "TRADE_DATE"
DOLT_TABLE = "block_trade"
START_YEAR = 2024

DDL = """\
CREATE TABLE IF NOT EXISTS block_trade (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    price DOUBLE NOT NULL,
    volume DOUBLE, amount DOUBLE,
    buyer VARCHAR(100), seller VARCHAR(100),
    premium_rate DOUBLE,
    update_date DATE,
    PRIMARY KEY (symbol, trade_date, price, volume, amount, buyer, seller)
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
        # Exclusive boundary: dates at or before the watermark are not
        # re-fetched; dates after today are never requested (the API returns
        # empty for future dates, which would scan half a year pointlessly).
        all_dates = [d for d in all_dates if since < d <= datetime.now().date().isoformat()]
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
    all_records: list[dict[str, str | int | float]] = []
    failure: str | None = None

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
                failure = f"{trade_date}: {e}"
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
    print("[import block_trade]", file=sys.stderr)

    symbol_expr = "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)"
    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_bt",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} ({INSERT_COLS})
            SELECT DISTINCT
                {symbol_expr}, DATE(TRADE_DATE), DEAL_PRICE, DEAL_VOLUME, DEAL_AMT,
                BUYER_NAME, SELLER_NAME, PREMIUM_RATIO, CURDATE()
            FROM _tmp_bt
            WHERE {symbol_expr} IN (SELECT symbol FROM stock_basic)
              AND DEAL_PRICE IS NOT NULL
        """,
        merge=True,
        dolt_table=DOLT_TABLE,
        source_label=f"EastMoney datacenter {REPORT_NAME}",
        last_report_expr="MAX(trade_date)",
    )


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
