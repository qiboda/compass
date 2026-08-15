#!/usr/bin/env python3
"""A-share index daily collector — official indices, concept & industry boards.

Independent module — uses common.py for shared infrastructure.

Source: EastMoney push2his ``kline/get`` for daily bars (``klt=101``,
``fqt=0``, ``beg=0&end=20500000`` full history) and push2 ``clist/get``
(``fs=m:90 t:3`` / ``t:2`` ``f:!50``) for the concept/industry board list.

Three index classes (handoff decisions 1/5/7):
- ``official`` — hardcoded whitelist of ~30 mainstream exchange indices
  (``secid={1|0}.{code}``, 1=SH / 0=SZ); a target whose kline response
  ``data.code`` does not match the whitelisted bare code is SKIPPED (the API
  may return a different index for a delisted/renamed code).
- ``concept`` / ``industry`` — boards discovered from clist (``f12`` code +
  ``f14`` name); klines via ``secid=90.BKxxxx``. A board whose kline is empty
  or fails is skipped for daily rows but KEEPS its index_basic entry
  (decision 2/9: 拉不到就跳过，不自算).

Incremental mode (decision 8): ``data_updates.last_report_date`` is compared
against today before fetching (short-circuit); new boards/indices are fetched
with full history automatically because every fetch uses ``beg=0`` and the
merge import (``INSERT IGNORE`` on PK (symbol, trade_date)) dedupes the
overlap.  Rate limiting: Throttle + host rotation (push2his main domain
falls back to numbered mirrors on empty/failed responses) + bounded 429
retries (handoff 调研).

Output: ``index_daily.csv`` (symbol, trade_date, index_type, open, close,
high, low, volume, amount, update_date) + ``index_basic.csv`` (symbol, name,
index_type) in csv_dir().
"""

import argparse
import asyncio
import csv
import random
import sys
from datetime import date
from pathlib import Path

from common import (
    AsyncSession,
    Progress,
    Throttle,
    csv_dir,
    dolt_sql_csv,
    drop_name_en_mapping,
    import_replace_table,
    last_report_date,
    load_name_en_mapping,
    write_csv,
)

DOLT_TABLE = "index_daily"
SOURCE = "EastMoney push2his kline + push2 clist"

# push2his kline — primary host must stay the handoff-verified canonical URL;
# numbered mirrors are tried as fallback on empty/failed responses.
PUSH2HIS = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
PUSH2HIS_MIRRORS = (
    "https://91.push2his.eastmoney.com/api/qt/stock/kline/get",
    "https://79.push2his.eastmoney.com/api/qt/stock/kline/get",
)
KLINE_HOSTS = (PUSH2HIS, *PUSH2HIS_MIRRORS)

# Board list discovery (push2 clist) — concept ``t:3`` / industry ``t:2``.
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"
CLIST_FALLBACK = "https://push2.eastmoney.com/api/qt/clist/get"
CLIST_HOSTS = (CLIST_URL, CLIST_FALLBACK)
CLIST_PAGE_SIZE = 100

HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    ),
    "Accept": "*/*",
    "Referer": "https://quote.eastmoney.com/",
}

# Bounded retry budget per target: hosts × attempts. 30 official indices × 6
# + 2 clist fetches × 6 = 192 requests worst case when everything 429s —
# must stay < 200 so an exhausted run terminates in bounded time.
_MAX_HOSTS_TRIED = 2
_MAX_ATTEMPTS = 3

# Fast-fail threshold (issue #277): after this many consecutive failed targets
# (request failure or empty klines) the run aborts instead of spinning for hours
# on an anti-bot block. A success resets the counter.
_MAX_CONSECUTIVE_FAILURES = 5

# kline 11 fields: 日期,开盘,收盘,最高,最低,成交量,成交额,振幅,涨跌幅,涨跌额,换手率
_KLINE_FIELDS = (
    "trade_date",
    "open",
    "close",
    "high",
    "low",
    "volume",
    "amount",
)

# Hardcoded mainstream official index whitelist (akshare index_zh_em style,
# handoff decision 5).  secid prefix: 1=SH / 0=SZ; code is the bare API code
# the kline response must echo for the fetch to count as a success.
OFFICIAL_INDICES: tuple[dict[str, str], ...] = (
    {"secid": "1.000001", "code": "000001", "name": "上证指数"},
    {"secid": "1.000016", "code": "000016", "name": "上证50"},
    {"secid": "1.000010", "code": "000010", "name": "上证180"},
    {"secid": "1.000009", "code": "000009", "name": "上证380"},
    {"secid": "1.000015", "code": "000015", "name": "上证红利"},
    {"secid": "1.000038", "code": "000038", "name": "上证180金融"},
    {"secid": "1.000104", "code": "000104", "name": "中证全指能源"},
    {"secid": "1.000300", "code": "000300", "name": "沪深300"},
    {"secid": "1.000903", "code": "000903", "name": "中证100"},
    {"secid": "1.000905", "code": "000905", "name": "中证500"},
    {"secid": "1.000852", "code": "000852", "name": "中证1000"},
    {"secid": "1.000906", "code": "000906", "name": "中证800"},
    {"secid": "1.000922", "code": "000922", "name": "中证红利"},
    {"secid": "1.000985", "code": "000985", "name": "中证全指"},
    {"secid": "1.000688", "code": "000688", "name": "科创50"},
    {"secid": "1.000932", "code": "000932", "name": "中证消费"},
    {"secid": "1.000933", "code": "000933", "name": "中证医药"},
    {"secid": "1.000934", "code": "000934", "name": "中证金融"},
    {"secid": "1.000819", "code": "000819", "name": "有色金属"},
    {"secid": "1.000827", "code": "000827", "name": "中证环保"},
    {"secid": "0.399001", "code": "399001", "name": "深证成指"},
    {"secid": "0.399006", "code": "399006", "name": "创业板指"},
    {"secid": "0.399005", "code": "399005", "name": "中小100"},
    {"secid": "0.399106", "code": "399106", "name": "深证综指"},
    {"secid": "0.399107", "code": "399107", "name": "深证A指"},
    {"secid": "0.399108", "code": "399108", "name": "深证B指"},
    {"secid": "0.399330", "code": "399330", "name": "深证100"},
    {"secid": "0.399007", "code": "399007", "name": "深证300"},
    {"secid": "0.399013", "code": "399013", "name": "深市精选"},
    {"secid": "1.000919", "code": "000919", "name": "300价值"},
)

DAILY_DDL = """\
CREATE TABLE IF NOT EXISTS index_daily (
    symbol      VARCHAR(20) NOT NULL,
    trade_date  DATE NOT NULL,
    index_type  VARCHAR(20) NOT NULL,
    open        DOUBLE,
    close       DOUBLE,
    high        DOUBLE,
    low         DOUBLE,
    volume      DOUBLE,
    amount      DOUBLE,
    update_date DATE,
    PRIMARY KEY (symbol, trade_date)
)"""

BASIC_DDL = """\
CREATE TABLE IF NOT EXISTS index_basic (
    symbol      VARCHAR(20) NOT NULL PRIMARY KEY,
    name        VARCHAR(100),
    index_type  VARCHAR(20),
    name_en     VARCHAR(100)
)"""

DAILY_INSERT_COLS = (
    "symbol, trade_date, index_type, open, close, high, low, volume, amount, update_date"
)


def _today() -> date:
    """Today's local date — module-level so tests can pin it."""
    return date.today()


def _num(value: str) -> int | float | str:
    """Parse a kline numeric cell, preserving int-ness for exact CSV round-trips.

    '0' → 0 (int, so str(volume) == '0'); '2999.0'/'1e9' → float; empty/'-' →
    '' (CSV empty → Dolt NULL). Unparsable cells fall back to '' rather than
    crashing the row build.
    """
    v = value.strip()
    if not v or v == "-":
        return ""
    try:
        return int(v)
    except ValueError:
        pass
    try:
        return float(v)
    except ValueError:
        return ""


def _kline_records(
    symbol: str,
    index_type: str,
    klines: list[str],
    today: date,
) -> list[dict[str, object]]:
    """Map kline CSV rows (date,o,c,h,l,vol,amt,...) to daily records.

    Rows dated after ``today`` (API glitch / bad data) are dropped — never
    silently published as normal bars. Early history (e.g. 1990-12-19) is
    preserved.
    """
    records: list[dict[str, object]] = []
    today_iso = today.isoformat()
    for line in klines:
        parts = line.split(",")
        if len(parts) < 7:
            continue
        trade_date = parts[0].strip()
        if trade_date > today_iso:
            continue
        record: dict[str, object] = {
            "symbol": symbol,
            "trade_date": trade_date,
            "index_type": index_type,
        }
        for i, field in enumerate(_KLINE_FIELDS[1:], start=1):
            record[field] = _num(parts[i])
        # DAILY_INSERT_COLS / the index_daily DDL both reference update_date;
        # write_csv() infers the CSV header from this record's keys, so the
        # key MUST be present or the merge import fails with "column
        # update_date could not be found" (issue #273).
        record["update_date"] = today_iso
        records.append(record)
    return records


async def _get_json(
    session: AsyncSession,
    throttle: Throttle,
    hosts: tuple[str, ...],
    params: dict[str, str],
) -> dict[str, object] | None:
    """GET ``params`` across ``hosts`` with bounded 429/error retries.

    Returns the parsed JSON body, or None when every host×attempt is
    exhausted. 429 waits then retries the same host (mirrors
    fetch_main_flow._fetch_page); an empty non-error response moves to the
    next host. Never raises on network failure — the caller treats None as
    "skip this target".
    """
    for base in hosts[:_MAX_HOSTS_TRIED]:
        for attempt in range(_MAX_ATTEMPTS):
            try:
                await throttle.acquire()
                resp = await session.get(base, params=params, headers=HEADERS)
                if resp.status_code == 429:
                    wait = 15 + random.uniform(0, 5)
                    print(f"    429, waiting {wait:.0f}s...", file=sys.stderr)
                    await asyncio.sleep(wait)
                    continue
                resp.raise_for_status()
                data = resp.json()
                if data:
                    return data
                print(f"    empty response from {base}", file=sys.stderr)
                break  # try the next host
            except Exception as e:
                wait = min(2**attempt, 30) + random.uniform(0, 3)
                if attempt < _MAX_ATTEMPTS - 1:
                    print(
                        f"    retry {attempt + 1}/{_MAX_ATTEMPTS} in {wait:.0f}s: {e}",
                        file=sys.stderr,
                    )
                    await asyncio.sleep(wait)
                else:
                    print(f"    FAILED {base}: {e}", file=sys.stderr)
    return None


async def fetch_board_list(session: AsyncSession, throttle: Throttle) -> list[tuple[str, str, str]]:
    r"""Discover concept + industry boards from push2 clist.

    Returns ``(symbol, name, index_type)`` tuples for every valid BK code
    (``^BK\d{4}$``); malformed codes (BK12/BK12345/BKAB12) and entries
    without f12 are rejected. Boards listed in both the concept and industry
    queries keep their first (concept) classification — avoids double kline
    fetches. Paginates until ``total`` is met; a failed page breaks the
    pagination for that query (boards discovered so far are kept).
    """
    boards: list[tuple[str, str, str]] = []
    seen: set[str] = set()
    for fs, index_type in (("m:90 t:3 f:!50", "concept"), ("m:90 t:2 f:!50", "industry")):
        page = 1
        collected = 0
        while True:
            params = {
                "pn": str(page),
                "pz": str(CLIST_PAGE_SIZE),
                "po": "1",
                "np": "1",
                "fltt": "2",
                "invt": "2",
                "fid": "f12",
                "fs": fs,
                "fields": "f12,f14",
            }
            data = await _get_json(session, throttle, CLIST_HOSTS, params)
            diff = ((data or {}).get("data") or {}).get("diff") or []
            total = int(((data or {}).get("data") or {}).get("total") or 0)
            collected += len(diff)
            for item in diff:
                code = item.get("f12")
                if (
                    not isinstance(code, str)
                    or len(code) != 6
                    or not code.startswith("BK")
                    or not code[2:].isdigit()
                ):
                    continue
                if code in seen:
                    continue
                seen.add(code)
                name = item.get("f14") if isinstance(item.get("f14"), str) else ""
                boards.append((code, name, index_type))
            if not diff or total <= 0 or collected >= total or page >= 100:
                break
            page += 1
    return boards


async def fetch_kline(
    session: AsyncSession,
    throttle: Throttle,
    secid: str,
) -> tuple[list[str], str] | None:
    """Fetch full-history daily klines for one secid.

    Returns (klines, data.code) on success, None when every host×attempt is
    exhausted. ``beg=0&end=20500000`` fetches the whole history; the caller
    decides code-match validation (official indices validate, boards don't).
    """
    params = {
        "secid": secid,
        "klt": "101",
        "fqt": "0",
        "beg": "0",
        "end": "20500000",
        "lmt": "1000000",
        "fields1": "f1,f2,f3,f4,f5,f6",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
    }
    data = await _get_json(session, throttle, KLINE_HOSTS, params)
    payload = (data or {}).get("data") or {}
    klines = payload.get("klines") or []
    return (klines, str(payload.get("code") or "")) if data else None


async def run() -> Path:
    """Fetch official indices + concept/industry boards into two CSVs.

    Short-circuits before fetching when ``data_updates.last_report_date`` is
    already today (incremental, decision 8). Individual targets that fail or
    return empty klines are logged and normally skipped, but after
    ``_MAX_CONSECUTIVE_FAILURES`` consecutive failures the run aborts (issue
    #277): already-fetched records are written to CSV and a RuntimeError is
    raised instead of spinning on an anti-bot block. With zero usable records
    no (half-written) CSV is left behind and a RuntimeError is raised.
    """
    daily_path = csv_dir() / "index_daily.csv"
    basic_path = csv_dir() / "index_basic.csv"

    last = last_report_date(DOLT_TABLE)
    if last == _today().isoformat():
        print(f"Data up to date ({last}); skipping fetch", file=sys.stderr)
        return daily_path

    print("Report: index_daily/index_basic (EastMoney push2his + push2 clist)", file=sys.stderr)
    print(f"Output: {daily_path.resolve()} / {basic_path.resolve()}", file=sys.stderr)

    with Progress("index_daily", output_csv=daily_path) as progress:
        throttle = Throttle()
        daily_records: list[dict[str, object]] = []
        basic_records: list[dict[str, object]] = []
        consecutive_failures = 0
        abort_reason: str | None = None

        async with AsyncSession(impersonate="chrome142") as session:
            boards = await fetch_board_list(session, throttle)
            print(f"Boards: {len(boards)}", file=sys.stderr)
            progress.update(
                total_items=len(boards) + len(OFFICIAL_INDICES),
                message="Fetching board and index klines",
            )

            # Boards first (discovery order), official after — index_basic order
            # convention (GUI picker lists boards prominently).
            for i, (code, name, index_type) in enumerate(boards):
                if abort_reason is not None:
                    break
                basic_records.append(
                    {"symbol": code, "name": name, "index_type": index_type}
                )
                print(f"  [board] {code} {name} ...", file=sys.stderr, end=" ", flush=True)
                result = await fetch_kline(session, throttle, f"90.{code}")
                if result is None:
                    consecutive_failures += 1
                    print("FAILED", file=sys.stderr)
                elif not result[0]:
                    consecutive_failures += 1
                    print("empty (skipped)", file=sys.stderr)
                else:
                    klines, _code = result
                    daily_records.extend(
                        _kline_records(code, index_type, klines, _today())
                    )
                    print(f"{len(klines)} bars", file=sys.stderr)
                    consecutive_failures = 0
                progress.update(
                    completed=i + 1,
                    fetched_rows=len(daily_records),
                    current_item=code,
                    message=f"Fetched board {code} {name}",
                )
                if consecutive_failures >= _MAX_CONSECUTIVE_FAILURES:
                    abort_reason = (
                        f"连续 {consecutive_failures} 个标的失败"
                        "（疑似反爬或接口故障），终止采集"
                    )
                    break

            # Official indices: response data.code must echo the whitelisted code,
            # otherwise the API returned a different index (skip + log). A code
            # mismatch is neither a failure nor a success: it must not reset the
            # consecutive-failure counter (would mask a real block) nor count
            # toward it (would false-trigger on a delisted/renamed index).
            for j, target in enumerate(OFFICIAL_INDICES, start=len(boards)):
                if abort_reason is not None:
                    break
                print(
                    f"  [official] {target['secid']} {target['name']} ...",
                    file=sys.stderr, end=" ", flush=True,
                )
                result = await fetch_kline(session, throttle, target["secid"])
                if result is None:
                    consecutive_failures += 1
                    print("FAILED", file=sys.stderr)
                elif not result[0]:
                    consecutive_failures += 1
                    print("empty (skipped)", file=sys.stderr)
                else:
                    klines, code = result
                    symbol = f"SH{target['code']}" if target["secid"].startswith("1.") \
                        else f"SZ{target['code']}"
                    # EastMoney echoes either the bare code ("000001") or the full
                    # symbol ("SH000001"); accept both, anything else is a different
                    # index (delisted/renamed code) — skip.
                    if code != target["code"] and code != symbol:
                        print(f"code mismatch ({code!r}), skipped", file=sys.stderr)
                    else:
                        consecutive_failures = 0
                        basic_records.append(
                            {"symbol": symbol, "name": target["name"], "index_type": "official"}
                        )
                        daily_records.extend(
                            _kline_records(symbol, "official", klines, _today())
                        )
                        print(f"{len(klines)} bars", file=sys.stderr)
                progress.update(
                    completed=j + 1,
                    fetched_rows=len(daily_records),
                    current_item=target["name"],
                    message=f"Fetched official {target['name']}",
                )
                if consecutive_failures >= _MAX_CONSECUTIVE_FAILURES:
                    abort_reason = (
                        f"连续 {consecutive_failures} 个标的失败"
                        "（疑似反爬或接口故障），终止采集"
                    )
                    break

        if abort_reason is not None:
            if daily_records:
                write_csv(daily_records, daily_path)
            # index_basic is (re)built on full runs only (data_updates.last_report_date
            # empty) and only when the board universe was actually discovered — an
            # empty clist means the API glitched, and a boards-less basic table would
            # silently drop every board's name entry on the merge import. Incremental
            # runs publish the daily CSV alone; official names ride along on full runs.
            if not last and boards and basic_records:
                write_csv(basic_records, basic_path)
            if not daily_records and not basic_records:
                daily_path.unlink(missing_ok=True)
                basic_path.unlink(missing_ok=True)
            raise RuntimeError(abort_reason)

        if not daily_records and not basic_records:
            daily_path.unlink(missing_ok=True)
            basic_path.unlink(missing_ok=True)
            raise RuntimeError(
                "No index data from push2his/clist (rate-limited or empty) — "
                "aborting, no CSV written"
            )

        if daily_records:
            write_csv(daily_records, daily_path)
        # index_basic is (re)built on full runs only (data_updates.last_report_date
        # empty) and only when the board universe was actually discovered — an
        # empty clist means the API glitched, and a boards-less basic table would
        # silently drop every board's name entry on the merge import. Incremental
        # runs publish the daily CSV alone; official names ride along on full runs.
        if not last and boards and basic_records:
            write_csv(basic_records, basic_path)
        progress.finish(
            fetched_rows=len(daily_records),
            message=f"Done: {len(daily_records)} daily rows",
        )
        print(
            f"\nDone: {len(daily_records)} daily rows, {len(basic_records)} basic "
            f"rows → {daily_path.resolve()}, {basic_path.resolve()}",
            file=sys.stderr,
        )
        return daily_path


def _csv_has_valid_dates(csv_path: Path) -> bool:
    """True when every non-empty trade_date cell parses as an ISO date.

    Dolt's import pipeline (``--continue`` + ``INSERT IGNORE``) silently
    coerces/skips unparseable date cells, so a sabotage row would otherwise
    slip through as a partial insert. Validating here refuses the whole file
    before Dolt is touched — no half-written table (plan QA: 不写半截数据).
    """
    with open(csv_path, newline="", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        for row in reader:
            cell = (row.get("trade_date") or "").strip()
            if not cell:
                continue
            try:
                date.fromisoformat(cell)
            except ValueError:
                return False
    return True


def _import_index_daily(csv_path: Path) -> int:
    """Merge-import the index_daily CSV (incremental, PK (symbol, trade_date)).

    No ``WHERE symbol IN (SELECT symbol FROM stock_basic)`` filter — indices
    live outside stock_basic by design (ref #201 separation), so that stock
    filter would drop every row.
    """
    print("[import index_daily]", file=sys.stderr)
    if not csv_path.exists():
        print(f"  ERROR: {csv_path} not found. Run fetch first.", file=sys.stderr)
        return 0
    if not _csv_has_valid_dates(csv_path):
        print("  ERROR: invalid trade_date in CSV — import refused", file=sys.stderr)
        return 0
    return import_replace_table(
        csv_path=csv_path,
        tmp_name="_tmp_ixd",
        ddl=DAILY_DDL,
        insert_sql=f"""
            INSERT IGNORE INTO {DOLT_TABLE} ({DAILY_INSERT_COLS})
            SELECT {DAILY_INSERT_COLS}
            FROM _tmp_ixd
        """,
        merge=True,
        dolt_table=DOLT_TABLE,
        source_label=SOURCE,
        last_report_expr="MAX(trade_date)",
    )


def _import_index_basic(csv_path: Path) -> int:
    """Merge-import the index_basic CSV (PK symbol, names for the picker).

    Epic #266 B1 + review P1-1: when the name-en mapping is available the
    INSERT LEFT-JOINs it twice — by ``symbol`` against the ``index`` section
    (official indexes) and by ``name`` against the ``concept`` section
    (concept/industry boards, which carry BK symbols the index section does
    not cover). The symbol hit wins via COALESCE; a double LEFT JOIN keeps
    one row per CSV row (no inflation). Unmapped rows → NULL (GUI falls back
    to Chinese). A missing mapping degrades gracefully — the base import
    always lands.
    """
    print("[import index_basic]", file=sys.stderr)
    mapping = load_name_en_mapping()
    try:
        if mapping:
            joins = """
                LEFT JOIN _tmp_name_en m1
                  ON m1.section = 'index' AND m1.`key` = t.symbol
                LEFT JOIN _tmp_name_en m2
                  ON m2.section = 'concept' AND m2.`key` = t.name
            """
            insert_cols = "(symbol, name, index_type, name_en)"
            select_cols = (
                "t.symbol, t.name, t.index_type, COALESCE(m1.value, m2.value)"
            )
        else:
            joins = ""
            insert_cols = "(symbol, name, index_type)"
            select_cols = "t.symbol, t.name, t.index_type"
        return import_replace_table(
            csv_path=csv_path,
            tmp_name="_tmp_ixb",
            ddl=BASIC_DDL,
            insert_sql=f"""
                INSERT IGNORE INTO index_basic {insert_cols}
                SELECT {select_cols}
                FROM _tmp_ixb t
                {joins}
            """,
            merge=True,
            dolt_table="index_basic",
            source_label=SOURCE,
            last_report_expr="CURDATE()",
        )
    finally:
        drop_name_en_mapping()


def import_to_dolt(csv_path: Path | None = None) -> int:
    """Import the CSV(s) into Dolt; returns the index_daily row count.

    With an explicit ``csv_path`` only that file is imported (routed by
    filename); without one both CSVs are imported (daily result returned).
    After the daily import the most recent 3-5 day points are sampled and
    cross-checked against Dolt (decision 6: 增量后数值抽样核对，容差报警).
    """
    if csv_path is not None:
        if "index_basic" in csv_path.name:
            return _import_index_basic(csv_path)
        rows = _import_index_daily(csv_path)
        if rows > 0:
            _verify_recent_points(csv_path)
        return rows
    _import_index_basic(csv_dir() / "index_basic.csv")
    rows = _import_index_daily(csv_dir() / "index_daily.csv")
    if rows > 0:
        _verify_recent_points(csv_dir() / "index_daily.csv")
    return rows


# Decision 6: 增量后数值抽样核对 — the most recent 3 trading-day closes per
# symbol are compared between the just-imported CSV and the Dolt table. A
# mismatch beyond 0.5% (float rounding / duplicate-date drift) means the
# fetch or merge went wrong; this is a warn-only gate, never a failure.
_SAMPLE_DAYS = 3
_SAMPLE_TOLERANCE = 0.005


def _verify_recent_points(csv_path: Path) -> None:
    """Cross-check the newest closes in the CSV against Dolt (warn-only)."""
    if not csv_path.exists() or not (dolt_dir := _dolt_dir_exists()):
        return
    newest: dict[tuple[str, str], float] = {}
    with open(csv_path, newline="", encoding="utf-8-sig") as f:
        for row in csv.DictReader(f):
            symbol = (row.get("symbol") or "").strip()
            trade_date = (row.get("trade_date") or "").strip()
            close_raw = (row.get("close") or "").strip()
            if symbol and trade_date and close_raw:
                try:
                    close = float(close_raw)
                except ValueError:
                    continue
                newest[(symbol, trade_date)] = close
    if not newest:
        return
    # The most recent `_SAMPLE_DAYS` rows per symbol, ordered newest-first.
    sample: dict[str, list[tuple[str, float]]] = {}
    for (symbol, trade_date), close in sorted(
        newest.items(), key=lambda kv: (kv[0][0], kv[0][1]), reverse=True
    ):
        sample.setdefault(symbol, [])
        if len(sample[symbol]) < _SAMPLE_DAYS:
            sample[symbol].append((trade_date, close))
    for symbol, dates in sample.items():
        for trade_date, csv_close in dates:
            stored = _dolt_close(dolt_dir, symbol, trade_date)
            if stored is None or abs(stored - csv_close) / csv_close <= _SAMPLE_TOLERANCE:
                continue
            print(
                f"  [verify] {symbol} {trade_date}: CSV {csv_close} vs "
                f"Dolt {stored} ({(csv_close - stored) / stored:.2%}) — "
                f"beyond {_SAMPLE_TOLERANCE:.1%} tolerance",
                file=sys.stderr,
            )


def _dolt_dir_exists() -> Path | None:
    """Return the Dolt data dir when it exists, else None."""
    import os

    path = Path(os.environ.get("COMPASS_DATA_DIR", "/data/compass-data/compass_data"))
    return path if (path / ".dolt").exists() else None


def _dolt_close(dolt_dir: Path, symbol: str, trade_date: str) -> float | None:
    """Fetch a single close from Dolt (None when the row is absent)."""
    try:
        out = dolt_sql_csv(
            f"SELECT close FROM index_daily "
            f"WHERE symbol = '{symbol}' AND trade_date = '{trade_date}'"
        )
    except Exception:
        return None
    lines = out.strip().split("\n")
    if len(lines) < 2:
        return None
    try:
        return float(lines[-1])
    except ValueError:
        return None


if __name__ == "__main__":  # pragma: no cover — __main__ block, never executed under pytest

    async def _main() -> None:
        p = argparse.ArgumentParser(description="Fetch A-share index daily bars")
        p.add_argument("--import-after", action="store_true", help="import into Dolt after fetch")
        args = p.parse_args()
        await run()
        if args.import_after:
            import_to_dolt()

    asyncio.run(_main())
