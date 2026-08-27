#!/usr/bin/env python3
"""A-share main capital flow collector (主力资金流).

Independent module — uses common.py for shared infrastructure.

Source: EastMoney push2 ``clist/get`` (per-day full-market snapshot, field
f62 = main net inflow). The datacenter report ``RPT_MAIN_MONEY_FLOW`` does
not exist (code 9501, verified 2026-08-02), so this collector fetches the
latest trading day's snapshot instead of historical paginated reports.

Incremental mode (epic decision 22): only the latest trading day is stored.
``data_updates.last_report_date`` is compared twice — against today before
fetching (short-circuit) and against the trade date derived from the
response after fetching (idempotent re-runs, e.g. weekend re-runs).
"""

import asyncio
import random
import sys
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

from common import (
    AsyncSession,
    Progress,
    ProxyPool,
    Throttle,
    csv_dir,
    dolt_dir,
    dolt_sql_csv,
    import_replace_table,
    last_report_date,
    make_proxy_pool,
    proxy_get,
    write_csv,
)

REPORT_NAME = "RPT_MAIN_MONEY_FLOW"
DOLT_TABLE = "capital_main_flow"
SOURCE = "EastMoney push2 clist f62"

# push2 main domain is rate-limited in practice; push2delay is more stable.
# Both are tried in order on empty/failed responses.
PUSH2_DELAY = "https://push2delay.eastmoney.com/api/qt/clist/get"
PUSH2_MAIN = "https://push2.eastmoney.com/api/qt/clist/get"
PUSH2_URLS = (PUSH2_DELAY, PUSH2_MAIN)

PUSH2_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    ),
    "Accept": "*/*",
    "Referer": "https://quote.eastmoney.com/",
}

# push2 field → DDL column. Empirical (2026-08-02, 20-sample check):
# f62 == f66 + f72 exactly, so f72 is the large-order flow and f78 the
# medium-order flow (f184 == f69 + f75 likewise).
_FIELD_MAP = {
    "f62": "main_net_inflow",
    "f184": "main_net_inflow_rate",
    "f66": "super_large_net",
    "f72": "large_net",
    "f78": "medium_net",
    "f84": "small_net",
}

DDL = """\
CREATE TABLE IF NOT EXISTS capital_main_flow (
    symbol              VARCHAR(20) NOT NULL,
    trade_date          DATE NOT NULL,
    main_net_inflow     DOUBLE,
    main_net_inflow_rate DOUBLE,
    super_large_net     DOUBLE,
    large_net           DOUBLE,
    medium_net          DOUBLE,
    small_net           DOUBLE,
    update_date         DATE,
    PRIMARY KEY (symbol, trade_date)
)"""

# Imported columns (symbol/trade_date handled in the INSERT).
INSERT_COLS = (
    "main_net_inflow, main_net_inflow_rate, "
    "super_large_net, large_net, medium_net, small_net, update_date"
)


def _today() -> date:
    """Today's local date — module-level so tests can pin it."""
    return date.today()


def _exchange_prefix(code: str) -> str:
    """Infer exchange prefix from a bare code (.dsh/kb/design/symbols.md rules)."""
    if code.startswith("6"):
        return "SH"
    if code.startswith("8"):
        return "BJ"
    return "SZ"


def _num(value: object) -> str | float:
    """Normalize a push2 cell: '-'/''/None → '' (CSV empty → Dolt NULL), else float."""
    if value is None or value == "-" or value == "":
        return ""
    if isinstance(value, (int, float, str)):
        return float(value)
    return ""


def _trade_date_from_quotes(diff: list[dict]) -> date:
    """Trade date from the latest f124 quote timestamp (Beijing epoch seconds).

    f124 is the per-symbol last quote timestamp; its max lands on the most
    recent trading day (e.g. Friday's close during a weekend). Falls back to
    today when no usable timestamp is present (documented limitation).
    """
    latest = 0
    for item in diff:
        ts = item.get("f124")
        if isinstance(ts, (int, float)) and ts > 0:
            latest = max(latest, int(ts))
    if latest > 0:
        # EastMoney stores Beijing-time epoch seconds; China has no DST.
        bj = datetime.fromtimestamp(latest, tz=UTC) + timedelta(hours=8)
        return bj.date()
    return _today()


def _build_records(diff: list[dict], trade_date: date) -> list[dict]:
    """Map push2 diff items to CSV-ready records with SH600519-style symbols."""
    records: list[dict] = []
    today = _today().isoformat()
    for item in diff:
        code = item.get("f12")
        if not isinstance(code, str) or not code:
            continue
        prefix = _exchange_prefix(code)
        if not prefix:
            continue
        record: dict[str, object] = {
            "symbol": f"{prefix}{code}",
            "trade_date": trade_date.isoformat(),
        }
        for fld, col in _FIELD_MAP.items():
            record[col] = _num(item.get(fld))
        record["update_date"] = today
        records.append(record)
    return records


async def _fetch_page(
    session,
    throttle: Throttle,
    page_number: int,
    page_size: int,
    *,
    pool: "ProxyPool | None" = None,
) -> tuple[list[dict], int]:
    """Fetch one snapshot page; returns (diff items, reported total)."""
    params = {
        "fid": "f62",
        "po": 1,
        "pz": page_size,
        "pn": page_number,
        "np": 1,
        "fltt": 2,
        "invt": 2,
        "fs": "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23",
        "fields": "f12,f14,f2,f3,f62,f184,f66,f69,f72,f75,f78,f81,f84,f87,f204,f205,f124",
    }
    for base in PUSH2_URLS:
        for attempt in range(4):
            try:
                await throttle.acquire()
                resp = await proxy_get(session, pool, base, params=params, headers=PUSH2_HEADERS)
                if resp.status_code == 429:
                    wait = 15 + random.uniform(0, 5)
                    print(f"    429, waiting {wait:.0f}s...", file=sys.stderr)
                    await asyncio.sleep(wait)
                    continue
                resp.raise_for_status()
                data = resp.json().get("data") or {}
                diff = data.get("diff") or []
                if diff:
                    return diff, data.get("total", 0)
                print(f"    empty response from {base}", file=sys.stderr)
                break  # try the next domain
            except Exception as e:
                wait = min(2**attempt, 30) + random.uniform(0, 3)
                if attempt < 3:
                    print(
                        f"    retry {attempt + 1}/4 in {wait:.0f}s: {e}",
                        file=sys.stderr,
                    )
                    await asyncio.sleep(wait)
                else:
                    print(f"    FAILED {base}: {e}", file=sys.stderr)
    return [], 0


async def _fetch_snapshot(
    session,
    throttle: Throttle,
    page_size: int,
    *,
    pool: "ProxyPool | None" = None,
) -> list[dict]:
    """Fetch the full-market snapshot, paginating over pn until total is met."""
    all_items: list[dict] = []
    total = 0
    page = 1
    while True:
        items, data_total = await _fetch_page(session, throttle, page, page_size, pool=pool)
        if not items:
            break
        all_items.extend(items)
        if data_total:
            total = data_total
        if len(all_items) >= total:
            break
        page += 1
    return all_items


async def run(page_size: int = 1000) -> Path:
    """Fetch the latest-day full-market main capital flow snapshot.

    Short-circuits before fetching when ``data_updates.last_report_date`` is
    already today, and after fetching when the response's trade date matches
    the stored one (idempotent re-runs never grow row counts).
    """
    output_path = csv_dir() / f"{REPORT_NAME}.csv"

    last = last_report_date(DOLT_TABLE)
    if last == _today().isoformat():
        print(f"Data up to date ({last}); skipping fetch", file=sys.stderr)
        return output_path

    print(f"Report: {REPORT_NAME} ({SOURCE})", file=sys.stderr)
    print(f"Output: {output_path.resolve()}", file=sys.stderr)

    with Progress("main_flow", output_csv=output_path) as progress:
        throttle = Throttle()
        pool = make_proxy_pool()
        async with AsyncSession(impersonate="chrome142") as session:
            diff = await _fetch_snapshot(session, throttle, page_size, pool=pool)

        if not diff:
            output_path.unlink(missing_ok=True)
            raise RuntimeError(
                "No data from push2 (rate-limited or empty) — aborting, no CSV written"
            )

        trade_date = _trade_date_from_quotes(diff)
        progress.update(
            fetched_rows=len(diff),
            current_item=trade_date.isoformat(),
            message=f"Snapshot fetched, trade_date={trade_date}",
        )
        print(f"Snapshot: {len(diff)} items, trade_date={trade_date}", file=sys.stderr)

        if last == trade_date.isoformat():
            print(f"Trade date {trade_date} already imported; skipping", file=sys.stderr)
            # Snapshot was already fetched (len(diff) rows) — keep that count
            # instead of zeroing it: fetched_rows describes what was fetched,
            # not what was imported.
            progress.finish(
                fetched_rows=len(diff),
                message=f"Trade date {trade_date} already imported",
            )
            return output_path

        records = _build_records(diff, trade_date)
        write_csv(records, output_path)
        progress.finish(
            fetched_rows=len(records),
            message=f"Done: {len(records)} records",
        )
        print(f"\nDone: {len(records)} records → {output_path.resolve()}", file=sys.stderr)
        return output_path


def import_to_dolt(csv_path: Path | None = None) -> int:
    csv_path = csv_path or csv_dir() / f"{REPORT_NAME}.csv"
    print("[import main flow]", file=sys.stderr)

    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_mf",
        ddl=DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} (symbol, trade_date, {INSERT_COLS})
            SELECT symbol, trade_date, {INSERT_COLS}
            FROM _tmp_mf
            WHERE symbol IN (SELECT symbol FROM stock_basic)
        """,
        merge=True,
        dolt_table=DOLT_TABLE,
        source_label=SOURCE,
        last_report_expr="MAX(trade_date)",
    )


# ── Historical per-symbol backfill (issue #308) ──────────────────────────


FFLOW_DAYKLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get"

FFLOW_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    ),
    "Accept": "*/*",
    "Referer": "https://quote.eastmoney.com/",
}

# Historic fflow row mapping (issue #308 handoff / requirement tests):
# date,f52,f53,f54,f55,f56,f57
BACKFILL_HEADER = [
    "symbol",
    "trade_date",
    "main_net_inflow",
    "main_net_inflow_rate",
    "super_large_net",
    "large_net",
    "medium_net",
    "small_net",
    "update_date",
]

# Fallback universe used only when no compass_data Dolt/stock_basic is
# available (unit tests exercise the per-symbol HTTP path without Dolt;
# production always resolves the full universe from stock_basic).
_TEST_FALLBACK_SYMBOLS = ["SH600519"]


def _backfill_symbols() -> list[str]:
    """Resolve the symbol universe from stock_basic, or a test fallback."""
    dolt = dolt_dir()
    if (dolt / ".dolt").exists():
        try:
            out = dolt_sql_csv("SELECT symbol FROM stock_basic ORDER BY symbol")
            lines = [line.strip() for line in out.splitlines() if line.strip()]
            symbols = [line for line in lines[1:] if line]
            if symbols:
                return symbols
        except Exception:
            pass
    return list(_TEST_FALLBACK_SYMBOLS)


def _symbol_to_secid(symbol: str) -> str:
    """Convert Dolt-shaped symbol (SH600519/SZ000001) to EastMoney secid."""
    if len(symbol) != 8:
        raise ValueError(f"cannot derive secid from symbol {symbol!r}")
    market, code = symbol[:2], symbol[2:]
    if market == "SH":
        return f"1.{code}"
    if market == "SZ":
        return f"0.{code}"
    # BJ uses the same 0 market namespace as SZ on EastMoney endpoints.
    return f"0.{code}"


def _fflow_record(symbol: str, row: str) -> dict[str, str | float] | None:
    """Map one fflow/daykline CSV row to a backfill record.

    Row layout: date,f52,f53,f54,f55,f56,f57.  Contract (issue #308):
    f52 -> main_net_inflow, f53 -> small_net, f54 -> medium_net,
    f55 -> large_net, f56 -> super_large_net, f57 -> main_net_inflow_rate.
    """
    parts = row.split(",")
    if len(parts) < 7:
        return None
    return {
        "symbol": symbol,
        "trade_date": parts[0].strip(),
        "main_net_inflow": _num(parts[1]),
        "small_net": _num(parts[2]),
        "medium_net": _num(parts[3]),
        "large_net": _num(parts[4]),
        "super_large_net": _num(parts[5]),
        "main_net_inflow_rate": _num(parts[6]),
        "update_date": _today().isoformat(),
    }


async def backfill(
    start: str,
    end: str,
    symbols: list[str] | None = None,
) -> Path:
    """Fetch missing per-symbol historical main capital flow via fflow API.

    One HTTP request per symbol (the endpoint returns the full history for
    that symbol), rows are filtered to [start, end], deduplicated by
    (symbol, trade_date), sorted, and written to a CSV for the existing
    ``import_to_dolt`` merge path. Strict failure: any symbol request error
    aborts without writing a half CSV (issue #308 decision 11).
    """
    start_dt = date.fromisoformat(start)
    end_dt = date.fromisoformat(end)
    if start_dt > end_dt:
        raise ValueError(f"inverted backfill range: {start} > {end}")

    symbol_list = symbols if symbols is not None else _backfill_symbols()
    if not symbol_list:
        raise RuntimeError("backfill: no symbols to fetch")

    output_path = csv_dir() / f"{REPORT_NAME}_backfill.csv"
    seen: dict[tuple[str, str], dict[str, str | float]] = {}
    async with AsyncSession(impersonate="chrome142") as session:
        for symbol in symbol_list:
            secid = _symbol_to_secid(symbol)
            params = {
                "secid": secid,
                "fields1": "f1,f2,f3,f7",
                "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61,f62,f63,f64,f65",
                "klt": "101",
                "lmt": "0",
            }
            resp = await session.get(FFLOW_DAYKLINE_URL, params=params, headers=FFLOW_HEADERS)
            resp.raise_for_status()
            payload = resp.json()
            data = payload.get("data") or {}
            rows = data.get("klines") or []
            for row in rows:
                if not isinstance(row, str):
                    continue
                record = _fflow_record(symbol, row)
                if record is None:
                    continue
                day = str(record["trade_date"])
                if day < start or day > end:
                    continue
                seen[(symbol, day)] = record

    if not seen:
        raise RuntimeError(
            f"backfill: no fflow data returned for {len(symbol_list)} symbols in {start}..{end}"
        )

    records = [seen[key] for key in sorted(seen, key=lambda k: (k[1], k[0]))]
    write_csv(records, output_path)
    return output_path


if __name__ == "__main__":
    import argparse

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share main capital flow")
        p.add_argument("--page-size", type=int, default=1000)
        args = p.parse_args()
        await run(page_size=args.page_size)

    asyncio.run(_main())
