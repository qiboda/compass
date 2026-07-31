#!/usr/bin/env python3
"""A-share income statement collector (利润表).

Independent module — uses common.py for shared infrastructure.

Report:  RPT_DMSK_FN_INCOME, 46 fields, filter column REPORT_DATE.
Default: 2020 onwards, all four quarterly periods.
"""

import asyncio
import sys
from datetime import datetime
from pathlib import Path

from common import (
    AsyncSession,
    Throttle,
    build_dates,
    dolt_sql,
    dolt_sql_csv,
    dolt_table_import,
    fetch_paginated,
    last_report_date,
    write_csv,
)

REPORT_NAME = "RPT_DMSK_FN_INCOME"
FILTER_COLUMN = "REPORT_DATE"
DOLT_TABLE = "fin_income"
START_YEAR = 2020

DDL = """\
CREATE TABLE fin_income (
    symbol              VARCHAR(20) NOT NULL,
    report_date         DATE NOT NULL,
    SECUCODE            VARCHAR(20),
    SECURITY_CODE       VARCHAR(10),
    INDUSTRY_CODE       VARCHAR(10),
    ORG_CODE            VARCHAR(20),
    SECURITY_NAME_ABBR  VARCHAR(100),
    INDUSTRY_NAME       VARCHAR(50),
    MARKET              VARCHAR(10),
    SECURITY_TYPE_CODE  VARCHAR(20),
    TRADE_MARKET_CODE   VARCHAR(20),
    DATE_TYPE_CODE      VARCHAR(10),
    REPORT_TYPE_CODE    VARCHAR(10),
    DATA_STATE          TINYINT,
    NOTICE_DATE         DATE,
    PARENT_NETPROFIT    DOUBLE,
    TOTAL_OPERATE_INCOME DOUBLE,
    TOTAL_OPERATE_COST  DOUBLE,
    TOE_RATIO           DOUBLE,
    OPERATE_COST        DOUBLE,
    OPERATE_EXPENSE     DOUBLE,
    OPERATE_EXPENSE_RATIO DOUBLE,
    SALE_EXPENSE        DOUBLE,
    MANAGE_EXPENSE      DOUBLE,
    FINANCE_EXPENSE     DOUBLE,
    OPERATE_PROFIT      DOUBLE,
    TOTAL_PROFIT        DOUBLE,
    INCOME_TAX          DOUBLE,
    OPERATE_INCOME      DOUBLE,
    INTEREST_NI         DOUBLE,
    INTEREST_NI_RATIO   DOUBLE,
    FEE_COMMISSION_NI   DOUBLE,
    FCN_RATIO           DOUBLE,
    OPERATE_TAX_ADD     DOUBLE,
    MANAGE_EXPENSE_BANK DOUBLE,
    FCN_CALCULATE       DOUBLE,
    INTEREST_NI_CALCULATE DOUBLE,
    EARNED_PREMIUM      DOUBLE,
    EARNED_PREMIUM_RATIO DOUBLE,
    INVEST_INCOME       DOUBLE,
    SURRENDER_VALUE     DOUBLE,
    COMPENSATE_EXPENSE  DOUBLE,
    TOI_RATIO           DOUBLE,
    OPERATE_PROFIT_RATIO DOUBLE,
    PARENT_NETPROFIT_RATIO DOUBLE,
    DEDUCT_PARENT_NETPROFIT DOUBLE,
    DPN_RATIO           DOUBLE,
    PRIMARY KEY (symbol, report_date)
)"""

COLS = (
    "SECUCODE, SECURITY_CODE, INDUSTRY_CODE, ORG_CODE, "
    "SECURITY_NAME_ABBR, INDUSTRY_NAME, MARKET, SECURITY_TYPE_CODE, TRADE_MARKET_CODE, "
    "DATE_TYPE_CODE, REPORT_TYPE_CODE, DATA_STATE, NOTICE_DATE, "
    "PARENT_NETPROFIT, TOTAL_OPERATE_INCOME, TOTAL_OPERATE_COST, TOE_RATIO, "
    "OPERATE_COST, OPERATE_EXPENSE, OPERATE_EXPENSE_RATIO, SALE_EXPENSE, "
    "MANAGE_EXPENSE, FINANCE_EXPENSE, OPERATE_PROFIT, TOTAL_PROFIT, INCOME_TAX, "
    "OPERATE_INCOME, INTEREST_NI, INTEREST_NI_RATIO, FEE_COMMISSION_NI, FCN_RATIO, "
    "OPERATE_TAX_ADD, MANAGE_EXPENSE_BANK, FCN_CALCULATE, INTEREST_NI_CALCULATE, "
    "EARNED_PREMIUM, EARNED_PREMIUM_RATIO, INVEST_INCOME, SURRENDER_VALUE, "
    "COMPENSATE_EXPENSE, TOI_RATIO, OPERATE_PROFIT_RATIO, "
    "PARENT_NETPROFIT_RATIO, DEDUCT_PARENT_NETPROFIT, DPN_RATIO"
)


async def run(
    years: list[int] | None = None,
    periods: str = "Q1,Q2,Q3,FY",
    page_size: int = 100,
) -> Path:
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
    csv_path = csv_path or Path(f"{REPORT_NAME}.csv")
    print("[import income]", file=sys.stderr)

    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0

    tmp_table = "_tmp_inc"
    if not dolt_table_import(tmp_table, csv_path):
        print("  Import failed", file=sys.stderr)
        return 0

    dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}_old")
    dolt_sql(f"RENAME TABLE {DOLT_TABLE} TO {tmp_table}_old")
    dolt_sql(DDL)

    sql = f"""
        INSERT INTO {DOLT_TABLE} (symbol, report_date, {COLS})
        SELECT
            CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
            REPORT_DATE, {COLS}
        FROM {tmp_table}
        WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
              IN (SELECT symbol FROM stock_basic)
    """
    result = dolt_sql(sql, timeout=600)
    if result.returncode != 0:
        print(f"  SQL error: {result.stderr}", file=sys.stderr)

    dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}")
    dolt_sql(f"DROP TABLE IF EXISTS {tmp_table}_old")

    stdout = dolt_sql_csv(f"SELECT COUNT(*) FROM {DOLT_TABLE}")
    lines = stdout.strip().split("\n")
    total = int(lines[-1]) if len(lines) > 1 else 0
    last_rpt = dolt_sql_csv(
        f"SELECT MAX(report_date) FROM {DOLT_TABLE}"
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
        p = argparse.ArgumentParser(description="Fetch A-share income statement")
        p.add_argument("--years", default="")
        p.add_argument("--periods", default="Q1,Q2,Q3,FY")
        args = p.parse_args()
        await run(
            years=[int(y) for y in args.years.split(",") if y] or None,
            periods=args.periods,
        )

    asyncio.run(_main())
