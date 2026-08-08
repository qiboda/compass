#!/usr/bin/env python3
"""A-share dragon-tiger list (龙虎榜) seat collector.

Independent module — uses common.py for shared infrastructure.

Seat-level daily billboard details are sourced from the EastMoney datacenter
BUY/SELL seat reports (RPT_BILLBOARD_DAILYDETAILSBUY / _SELL), fetched per
TRADE_DATE. The stock-level billboard list report RPT_DAILYBILLBOARD_DETAILSNEW
carries no per-seat breakdown — it is kept as REPORT_NAME for the data_updates
source label. BUY and SELL records are merged (deduping seats that appear in
both top lists) and aggregated to one row per (symbol, trade_date, seat_type).
seat_type is classified from the returned seat name: '机构专用' is the
institution seat (institution_flag=1); exchange-link seats keep their name;
all other seats are brokerage branches ('营业部').
"""

import asyncio
import sys
from datetime import datetime, timedelta
from pathlib import Path

from common import (
    AsyncSession,
    Throttle,
    csv_dir,
    fetch_paginated,
    import_replace_table,
    last_report_date,
    write_csv,
)

# Stock-level billboard list report (no seat data) — used as the source label.
REPORT_NAME = "RPT_DAILYBILLBOARD_DETAILSNEW"
# Seat-level daily billboard details (buy/sell seat lists per stock and day).
BUY_REPORT_NAME = "RPT_BILLBOARD_DAILYDETAILSBUY"
SELL_REPORT_NAME = "RPT_BILLBOARD_DAILYDETAILSSELL"
FILTER_COLUMN = "TRADE_DATE"
DOLT_TABLE = "dragon_list"
START_DATE = "2020-01-01"

DDL = """\
CREATE TABLE IF NOT EXISTS dragon_list (
    symbol              VARCHAR(20) NOT NULL,
    trade_date          DATE NOT NULL,
    seat_type           VARCHAR(10) NOT NULL,
    buy_amount          DOUBLE,
    sell_amount         DOUBLE,
    net_amount          DOUBLE,
    institution_flag    TINYINT,
    update_date         DATE,
    PRIMARY KEY (symbol, trade_date, seat_type)
)"""

# Imported columns (symbol/trade_date/update_date handled in the INSERT).
INSERT_COLS = "SEAT_TYPE, BUY_AMOUNT, SELL_AMOUNT, NET_AMOUNT, INSTITUTION_FLAG"


def _next_day(date_str: str) -> str:
    """Return the next calendar day as YYYY-MM-DD."""
    day = datetime.strptime(date_str, "%Y-%m-%d").date()
    return (day + timedelta(days=1)).isoformat()


def _as_float(value: object) -> float:
    """Parse a numeric field; empty/None becomes 0.0."""
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str) and value != "":
        try:
            return float(value)
        except ValueError:
            pass
    return 0.0


def _seat_type(operatedept_name: str) -> tuple[str, int]:
    """Classify a seat from its EastMoney name.

    Returns (seat_type, institution_flag). '机构专用' is the institution
    seat; exchange-link seats (深股通专用/沪股通专用) keep their name;
    all other seats are brokerage branches ('营业部').
    """
    if operatedept_name == "机构专用":
        return "机构专用", 1
    if operatedept_name.endswith("专用"):
        return operatedept_name, 0
    return "营业部", 0


def _merge_seats(
    records: list[dict[str, str | int | float]],
) -> list[dict[str, str | int | float]]:
    """Merge BUY/SELL seat records into one row per (symbol, trade_date, seat_type).

    The same seat can appear in both the buy and sell top lists with identical
    amounts — those duplicates are dropped, then amounts are summed per seat
    type (multiple institution entries share the name '机构专用').
    """
    seen: set[tuple[object, ...]] = set()
    raw: list[dict[str, str | int | float]] = []
    for r in records:
        key = (
            r.get("SECUCODE"),
            str(r.get("TRADE_DATE") or "")[:10],
            r.get("OPERATEDEPT_NAME"),
            r.get("BUY"),
            r.get("SELL"),
            r.get("NET"),
        )
        if key in seen:
            continue
        seen.add(key)
        raw.append(r)

    by_key: dict[tuple[object, ...], dict[str, str | int | float]] = {}
    for r in raw:
        day = str(r.get("TRADE_DATE") or "")[:10]
        seat_type, inst = _seat_type(str(r.get("OPERATEDEPT_NAME") or ""))
        key = (str(r.get("SECUCODE") or ""), day, seat_type)
        entry = by_key.get(key)
        if entry is None:
            entry = {
                "SECUCODE": str(r.get("SECUCODE") or ""),
                "SECURITY_CODE": str(r.get("SECURITY_CODE") or ""),
                "TRADE_DATE": day,
                "SEAT_TYPE": seat_type,
                "BUY_AMOUNT": 0.0,
                "SELL_AMOUNT": 0.0,
                "NET_AMOUNT": 0.0,
                "INSTITUTION_FLAG": 0,
            }
            by_key[key] = entry
        entry["BUY_AMOUNT"] = float(entry["BUY_AMOUNT"]) + _as_float(r.get("BUY"))
        entry["SELL_AMOUNT"] = float(entry["SELL_AMOUNT"]) + _as_float(r.get("SELL"))
        entry["NET_AMOUNT"] = float(entry["NET_AMOUNT"]) + _as_float(r.get("NET"))
        entry["INSTITUTION_FLAG"] = max(int(entry["INSTITUTION_FLAG"]), inst)
    return list(by_key.values())


async def run(
    start_date: str | None = None,
    end_date: str | None = None,
    page_size: int = 100,
) -> Path:
    """Fetch daily billboard seat data into a CSV.

    Dates advance day by day (weekends/holidays return empty from the API —
    no trading calendar is generated); the incremental start comes from the
    last imported TRADE_DATE recorded in data_updates.

    Args:
        start_date: First day to fetch (YYYY-MM-DD). Defaults to the day
            after last_report_date, or START_DATE on first run.
        end_date: Last day to fetch (YYYY-MM-DD). Defaults to today.
        page_size: Records per API page.
    """
    output_path = csv_dir() / f"{REPORT_NAME}.csv"
    end_date = end_date or datetime.now().strftime("%Y-%m-%d")

    since = last_report_date(DOLT_TABLE)
    if since:
        print(f"Last trade date in Dolt: {since}, fetching only newer days", file=sys.stderr)
        start_date = _next_day(since)
    elif start_date is None:
        start_date = START_DATE

    if start_date > end_date:
        print("No new trading days to fetch.", file=sys.stderr)
        return output_path

    print(f"Report: {REPORT_NAME} ({BUY_REPORT_NAME}/{SELL_REPORT_NAME})", file=sys.stderr)
    print(f"Range: {start_date}..{end_date}", file=sys.stderr)
    print(f"Output: {output_path.resolve()}", file=sys.stderr)
    print(file=sys.stderr)

    throttle = Throttle()
    all_records: list[dict[str, str | int | float]] = []
    day = start_date
    failure: str | None = None

    async with AsyncSession(impersonate="chrome142") as session:
        while day <= end_date:
            print(f"[{day}] ...", file=sys.stderr, end=" ", flush=True)
            try:
                buy = await fetch_paginated(
                    session,
                    throttle,
                    BUY_REPORT_NAME,
                    FILTER_COLUMN,
                    day,
                    page_size,
                )
                sell = await fetch_paginated(
                    session,
                    throttle,
                    SELL_REPORT_NAME,
                    FILTER_COLUMN,
                    day,
                    page_size,
                )
                records = _merge_seats(buy + sell)
            except Exception as e:
                failure = f"{day}: {e}"
                print(f"FAILED: {e}", file=sys.stderr)
                break

            all_records.extend(records)
            print(f"{len(records)} records" if records else "empty", file=sys.stderr)
            day = _next_day(day)

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
    """Import the fetched CSV into the Dolt dragon_list table.

    Replaces the table atomically: previous data is renamed aside, the new
    table is created from the CSV (symbols filtered to stock_basic), and any
    INSERT failure rolls back to the previous data.
    """
    csv_path = csv_path or csv_dir() / f"{REPORT_NAME}.csv"
    print("[import dragon_list]", file=sys.stderr)

    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_dr",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} (symbol, trade_date, {INSERT_COLS}, update_date)
            SELECT
                CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE),
                TRADE_DATE, {INSERT_COLS}, CURDATE()
            FROM _tmp_dr
            WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE)
                  IN (SELECT symbol FROM stock_basic)
        """,
        merge=True,
        dolt_table=DOLT_TABLE,
        source_label=f"EastMoney datacenter {REPORT_NAME}",
        last_report_expr="MAX(trade_date)",
    )


if __name__ == "__main__":
    import argparse

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share dragon-tiger list seats")
        p.add_argument("--start", default="")
        p.add_argument("--end", default="")
        p.add_argument("--page-size", type=int, default=100)
        args = p.parse_args()
        await run(
            start_date=args.start or None,
            end_date=args.end or None,
            page_size=args.page_size,
        )

    asyncio.run(_main())
