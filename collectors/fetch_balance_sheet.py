#!/usr/bin/env python3
"""A-share balance sheet collector (资产负债表).

Independent module — uses common.py for shared infrastructure.

Report:  RPT_DMSK_FN_BALANCE, 57 fields, filter column REPORT_DATE.
Default: 2020 onwards, all four quarterly periods.

Usage:
    uv run python fetch_balance_sheet.py              # full run (2020-now)
    uv run python fetch_balance_sheet.py --years 2024,2025
    uv run python fetch_balance_sheet.py --incremental
"""

import asyncio
import sys
from datetime import datetime
from pathlib import Path

from common import (
    AsyncSession,
    Throttle,
    build_dates,
    fetch_paginated,
    import_replace_table,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_DMSK_FN_BALANCE"
FILTER_COLUMN = "REPORT_DATE"  # underscore — different from RPT_LICO_FN_CPD
DOLT_TABLE = "fin_balance_sheet"
START_YEAR = 2020

DDL = """\
CREATE TABLE IF NOT EXISTS fin_balance_sheet (
    symbol            VARCHAR(20) NOT NULL,
    report_date       DATE NOT NULL,
    SECUCODE          VARCHAR(20),
    SECURITY_CODE     VARCHAR(10),
    INDUSTRY_CODE     VARCHAR(10),
    ORG_CODE          VARCHAR(20),
    SECURITY_NAME_ABBR VARCHAR(100),
    INDUSTRY_NAME     VARCHAR(50),
    MARKET            VARCHAR(10),
    SECURITY_TYPE_CODE VARCHAR(20),
    TRADE_MARKET_CODE VARCHAR(20),
    DATE_TYPE_CODE    VARCHAR(10),
    REPORT_TYPE_CODE  VARCHAR(10),
    DATA_STATE        TINYINT,
    NOTICE_DATE       DATE,
    TOTAL_ASSETS      DOUBLE,
    FIXED_ASSET       DOUBLE,
    MONETARYFUNDS     DOUBLE,
    MONETARYFUNDS_RATIO DOUBLE,
    ACCOUNTS_RECE     DOUBLE,
    ACCOUNTS_RECE_RATIO DOUBLE,
    INVENTORY         DOUBLE,
    INVENTORY_RATIO   DOUBLE,
    TOTAL_LIABILITIES DOUBLE,
    ACCOUNTS_PAYABLE  DOUBLE,
    ACCOUNTS_PAYABLE_RATIO DOUBLE,
    ADVANCE_RECEIVABLES DOUBLE,
    ADVANCE_RECEIVABLES_RATIO DOUBLE,
    TOTAL_EQUITY      DOUBLE,
    TOTAL_EQUITY_RATIO DOUBLE,
    TOTAL_ASSETS_RATIO DOUBLE,
    TOTAL_LIAB_RATIO  DOUBLE,
    CURRENT_RATIO     DOUBLE,
    DEBT_ASSET_RATIO  DOUBLE,
    CASH_DEPOSIT_PBC  DOUBLE,
    CDP_RATIO         DOUBLE,
    LOAN_ADVANCE      DOUBLE,
    LOAN_ADVANCE_RATIO DOUBLE,
    AVAILABLE_SALE_FINASSET DOUBLE,
    ASF_RATIO         DOUBLE,
    LOAN_PBC          DOUBLE,
    LOAN_PBC_RATIO    DOUBLE,
    ACCEPT_DEPOSIT    DOUBLE,
    ACCEPT_DEPOSIT_RATIO DOUBLE,
    SELL_REPO_FINASSET DOUBLE,
    SRF_RATIO         DOUBLE,
    SETTLE_EXCESS_RESERVE DOUBLE,
    SER_RATIO         DOUBLE,
    BORROW_FUND       DOUBLE,
    BORROW_FUND_RATIO DOUBLE,
    AGENT_TRADE_SECURITY DOUBLE,
    ATS_RATIO         DOUBLE,
    PREMIUM_RECE      DOUBLE,
    PREMIUM_RECE_RATIO DOUBLE,
    SHORT_LOAN        DOUBLE,
    SHORT_LOAN_RATIO  DOUBLE,
    ADVANCE_PREMIUM   DOUBLE,
    ADVANCE_PREMIUM_RATIO DOUBLE,
    PRIMARY KEY (symbol, report_date)
)"""

COLS = (
    "SECUCODE, SECURITY_CODE, INDUSTRY_CODE, ORG_CODE, "
    "SECURITY_NAME_ABBR, INDUSTRY_NAME, MARKET, SECURITY_TYPE_CODE, TRADE_MARKET_CODE, "
    "DATE_TYPE_CODE, REPORT_TYPE_CODE, DATA_STATE, NOTICE_DATE, "
    "TOTAL_ASSETS, FIXED_ASSET, MONETARYFUNDS, MONETARYFUNDS_RATIO, "
    "ACCOUNTS_RECE, ACCOUNTS_RECE_RATIO, INVENTORY, INVENTORY_RATIO, "
    "TOTAL_LIABILITIES, ACCOUNTS_PAYABLE, ACCOUNTS_PAYABLE_RATIO, "
    "ADVANCE_RECEIVABLES, ADVANCE_RECEIVABLES_RATIO, "
    "TOTAL_EQUITY, TOTAL_EQUITY_RATIO, TOTAL_ASSETS_RATIO, TOTAL_LIAB_RATIO, "
    "CURRENT_RATIO, DEBT_ASSET_RATIO, CASH_DEPOSIT_PBC, CDP_RATIO, "
    "LOAN_ADVANCE, LOAN_ADVANCE_RATIO, AVAILABLE_SALE_FINASSET, ASF_RATIO, "
    "LOAN_PBC, LOAN_PBC_RATIO, ACCEPT_DEPOSIT, ACCEPT_DEPOSIT_RATIO, "
    "SELL_REPO_FINASSET, SRF_RATIO, SETTLE_EXCESS_RESERVE, SER_RATIO, "
    "BORROW_FUND, BORROW_FUND_RATIO, AGENT_TRADE_SECURITY, ATS_RATIO, "
    "PREMIUM_RECE, PREMIUM_RECE_RATIO, SHORT_LOAN, SHORT_LOAN_RATIO, "
    "ADVANCE_PREMIUM, ADVANCE_PREMIUM_RATIO"
)


async def run(
    years: list[int] | None = None,
    periods: str = "Q1,Q2,Q3,FY",
    page_size: int = 100,
) -> Path:
    """Fetch balance sheet data and write to CSV.

    Returns the path to the generated CSV file.
    """
    if years is None:
        years = list(range(START_YEAR, datetime.now().year + 1))

    output_path = Path(f"{REPORT_NAME}.csv")
    period_list = [p.strip() for p in periods.split(",")]
    all_dates = build_dates(years, period_list)

    since = last_report_date(DOLT_TABLE)
    if since:
        print(f"Last report date in Dolt: {since}, fetching only newer periods", file=sys.stderr)
        all_dates = [d for d in all_dates if d >= since]
        if not all_dates:
            print("No new report periods to fetch.", file=sys.stderr)
            return output_path

    print(f"Report: {REPORT_NAME}", file=sys.stderr)
    print(
        f"Periods: {len(all_dates)} ({periods}, "
        f"{all_dates[0] if all_dates else 'none'}..{all_dates[-1] if all_dates else 'none'})",
        file=sys.stderr,
    )
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    total_records = 0
    first_write = True

    async with AsyncSession(impersonate="chrome142") as session:
        for i, report_date in enumerate(all_dates):
            print(
                f"[{i + 1}/{len(all_dates)}] {report_date} ...",
                file=sys.stderr, end=" ", flush=True,
            )
            try:
                records = await fetch_paginated(
                    session, throttle, REPORT_NAME, FILTER_COLUMN, report_date, page_size,
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
    """Import the fetched CSV into Dolt fin_balance_sheet (merge semantics).

    Rows are INSERT IGNORE'd into the existing table, deduped by the PK
    (symbol, report_date), so incremental-window CSVs append to history
    instead of clobbering it (ref #160).
    """
    csv_path = csv_path or Path(f"{REPORT_NAME}.csv")
    print("[import balance_sheet]", file=sys.stderr)

    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_bs",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} (symbol, report_date, {COLS})
            SELECT
                CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
                REPORT_DATE, {COLS}
            FROM _tmp_bs
            WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
                  IN (SELECT symbol FROM stock_basic)
        """,
        dolt_table=DOLT_TABLE,
        source_label=f"EastMoney datacenter {REPORT_NAME}",
        last_report_expr="MAX(report_date)",
        merge=True,
    )


# Standalone entry point (also callable via main.py)
if __name__ == "__main__":
    import argparse

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share balance sheet")
        p.add_argument("--years", default="")
        p.add_argument("--periods", default="Q1,Q2,Q3,FY")
        args = p.parse_args()
        await run(
            years=[int(y) for y in args.years.split(",") if y] or None,
            periods=args.periods,
        )

    asyncio.run(_main())
