#!/usr/bin/env python3
"""A-share index daily collector — official indices & THS industry boards.

Independent module — uses common.py for shared infrastructure.

Sources:
- Official indices: EastMoney push2his ``kline/get`` (``klt=101``,
  ``fqt=0``; ``beg=0&end=20500000`` for new symbols, ``beg=last_date+1``
  for incremental); a Tencent ``newfqkline/get`` fallback covers EastMoney
  failure/empty klines and supplies 成交额 (index 8, 万元 → 元) for official
  index rows (issue #278/#286).
- Industry boards: 同花顺 (THS) 90 申万一级 industries (881xxx) — the list
  page ``q.10jqka.com.cn/thshy/`` (GBK) and per-year daily klines from
  ``d.10jqka.com.cn/v4/line/bk_881xxx/01/{year}.js`` (issue #283).

Two index classes (handoff decisions 1/5/7 + #283 D1):
- ``official`` — hardcoded whitelist of ~30 mainstream exchange indices
  (``secid={1|0}.{code}``, 1=SH / 0=SZ); a target whose kline response
  ``data.code`` does not match the whitelisted bare code is SKIPPED (the API
  may return a different index for a delisted/renamed code).
- ``industry`` — THS boards discovered from the thshy list page; klines via
  the per-year BK kline endpoint. A board whose kline is empty or fails is
  skipped for daily rows but KEEPS its index_basic entry (拉不到就跳过，不自算).

Incremental mode (decision 8 + issue #292): ``data_updates.last_report_date``
short-circuits when everything is already updated today. Per symbol, the
stored ``MAX(trade_date)`` drives a true incremental fetch: existing THS
boards only fetch MAX year→current year (a Dec-31 MAX starts the next year)
and rows ``<= MAX`` are filtered; official indices use EastMoney
``beg=MAX+1`` and a Tencent incremental pagination that stops at ``<= MAX``.
New symbols still backfill full history. The merge import (``INSERT IGNORE``
on PK (symbol, trade_date)) remains the idempotent landing. Rate limiting:
Throttle + host rotation (push2his main domain
falls back to numbered mirrors on empty/failed responses) + bounded 429
retries (handoff 调研).

Output: ``index_daily.csv`` (symbol, trade_date, index_type, open, close,
high, low, volume, amount, update_date) + ``index_basic.csv`` (symbol, name,
index_type) in csv_dir().
"""

import argparse
import asyncio
import csv
import json
import math
import random
import re
import sys
from datetime import date, timedelta
from pathlib import Path

from common import (
    AsyncSession,
    Progress,
    ProxyPool,
    Throttle,
    csv_dir,
    dolt_sql_csv,
    drop_name_en_mapping,
    import_replace_table,
    last_report_date,
    load_name_en_mapping,
    make_proxy_pool,
    proxy_get,
    write_csv,
)

DOLT_TABLE = "index_daily"
SOURCE = "EastMoney push2his kline + Tencent fallback + THS industry kline"

# push2his kline — primary host must stay the handoff-verified canonical URL;
# numbered mirrors are tried as fallback on empty/failed responses.
PUSH2HIS = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
PUSH2HIS_MIRRORS = (
    "https://91.push2his.eastmoney.com/api/qt/stock/kline/get",
    "https://79.push2his.eastmoney.com/api/qt/stock/kline/get",
)
KLINE_HOSTS = (PUSH2HIS, *PUSH2HIS_MIRRORS)

# Tencent fallback for official indices (issue #278/#286):
# web.ifzq.gtimg.cn/appstock/app/newfqkline/get supports count<=2000 and
# start-date pagination, and its 11-field day rows include 成交额 in 万元 at
# index 8 (the old fqkline/get rows have only 6 fields and no amount).
# Used when EastMoney fails or returns empty klines for an official whitelist
# target.
TENCENT_KLINE_URL = "https://web.ifzq.gtimg.cn/appstock/app/newfqkline/get"
_TENCENT_PAGE_SIZE = 2000
# 10 pages * 2000 bars ≈ 20k rows ceiling; A-share index full history is
# ~8.5k bars, so this is a generous safety cap against API misbehavior.
_TENCENT_MAX_PAGES = 10

# THS (同花顺) industry boards (issue #283 D1/D2): the list page is GBK
# HTML whose anchors embed every 881xxx code + display name (~140 raw rows,
# 50 duplicates → 90 unique); per-year klines come from the JSONP BK
# endpoint (2007..current year, ~20 requests per board).
THS_LIST_URL = "https://q.10jqka.com.cn/thshy/"
THS_KLINE_TPL = "https://d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js"
THS_FIRST_YEAR = 2007

HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    ),
    "Accept": "*/*",
    "Referer": "https://quote.eastmoney.com/",
}

# THS endpoints need their own Referer (10jqka hosts reject eastmoney one).
THS_HEADERS = {**HEADERS, "Referer": "https://q.10jqka.com.cn/"}

# Bounded retry budget per target: hosts × attempts. 30 official indices × 6
# + 2 THS list fetches × 6 = 192 requests worst case when everything 429s —
# must stay < 200 so an exhausted run terminates in bounded time.
#
# The THS per-year kline path is NOT covered by this budget: 90 industries ×
# ~20 years ≈ 1800 requests max. It has its own throttle (Throttle) plus a
# single retry per request (fetch_ths_kline), and the #277 fast-fail aborts
# after 5 consecutive failed industries — a 429 storm on THS therefore ends
# in bounded time via fast-fail, not via the <200 budget.
_MAX_HOSTS_TRIED = 2
_MAX_ATTEMPTS = 3

# Fast-fail threshold (issue #277): after this many consecutive failed targets
# (request failure or empty klines) the run aborts instead of spinning for hours
# on an anti-bot block. A success resets the counter.
_MAX_CONSECUTIVE_FAILURES = 5


def _abort_reason(count: int) -> str:
    """Build the fast-fail RuntimeError message for ``count`` consecutive failures."""
    return f"连续 {count} 个标的失败（疑似反爬或接口故障），终止采集"


def _bump_failure(consecutive_failures: int) -> tuple[int, str | None]:
    """Increment the consecutive-failure counter and return the abort reason.

    Returns ``(new_count, None)`` below the threshold, or ``(new_count,
    reason)`` when the run must abort after this failure.
    """
    count = consecutive_failures + 1
    if count >= _MAX_CONSECUTIVE_FAILURES:
        return count, _abort_reason(count)
    return count, None


def _persist_outputs(
    daily_records: list[dict[str, object]],
    basic_records: list[dict[str, object]],
    daily_path: Path,
    basic_path: Path,
) -> None:
    """Write the CSV outputs produced so far (shared by normal and abort paths).

    index_basic is (re)built on every non-short-circuited run whenever any
    basic record exists. The import is a merge (INSERT IGNORE on PK symbol),
    which never drops rows absent from the CSV, so rewriting the basic CSV
    cannot erase names already stored in Dolt (a THS list-page glitch that
    leaves only official rows stays harmless). Rebuilding on every run keeps
    the CSV mirror of ``index_basic`` in sync with the board whitelist —
    without it, a stale CSV resurrects Dolt rows deleted by the B1 cleanup
    (issue #283: 1000 EastMoney board rows were dropped from Dolt while an
    old CSV still listed them; the next incremental import re-inserted every
    deleted row). When no records exist at all, any stale CSV files are
    removed.
    """
    if daily_records:
        write_csv(daily_records, daily_path)
    if basic_records:
        write_csv(basic_records, basic_path)
    if not daily_records and not basic_records:
        daily_path.unlink(missing_ok=True)
        basic_path.unlink(missing_ok=True)

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


def max_trade_date(dolt_table: str, symbol: str) -> str | None:
    """Return the latest stored ``trade_date`` for one symbol, or None.

    None means the symbol has no rows (new symbol → full backfill) or the
    Dolt table/database is unavailable (degrade to full backfill rather than
    crash the collector). Single-quote escaping mirrors ``_dolt_close``.
    """
    if _dolt_dir_exists() is None:
        return None
    escaped = symbol.replace(chr(39), chr(39) * 2)
    try:
        stdout = dolt_sql_csv(
            f"SELECT DATE_FORMAT(MAX(trade_date), '%Y-%m-%d') "
            f"FROM {dolt_table} WHERE symbol = '{escaped}'"
        )
    except Exception:
        return None
    lines = stdout.strip().split("\n")
    if len(lines) < 2:
        return None
    value = lines[-1].strip()
    if value and value != "NULL":
        return value
    return None


def _parse_max_date(raw: str | None) -> date | None:
    """Parse a symbol's max trade_date into a date.

    Invalid/empty values are treated as None (new symbol → full backfill).
    A future-dated MAX (API/DB dirty data) is clamped to today so the
    collector treats the symbol as already up to date instead of attempting
    another full-history sweep.
    """
    if not raw:
        return None
    try:
        parsed = date.fromisoformat(raw)
    except ValueError:
        return None
    today = _today()
    return min(parsed, today)


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
    *,
    pool: "ProxyPool | None" = None,
) -> dict[str, object] | None:
    """GET ``params`` across ``hosts`` with bounded 429/error retries.

    Returns the parsed JSON body, or None when every host×attempt is
    exhausted. 429 waits then retries the same host (mirrors
    fetch_main_flow._fetch_page); an empty non-error response moves to the
    next host. Never raises on network failure — the caller treats None as
    "skip this target". ``pool`` enables proxy-first rotation.
    """
    for base in hosts[:_MAX_HOSTS_TRIED]:
        for attempt in range(_MAX_ATTEMPTS):
            try:
                await throttle.acquire()
                resp = await proxy_get(session, pool, base, params=params, headers=HEADERS)
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


async def fetch_ths_industry_list(
    session: AsyncSession,
    throttle: Throttle,
    *,
    pool: "ProxyPool | None" = None,
) -> list[tuple[str, str]]:
    """Fetch the THS industry list page and extract the unique 881xxx boards.

    The page is GBK-encoded HTML; each industry row is an anchor whose href
    embeds the 881xxx code and whose text is the display name. The raw page
    carries ~140 rows with ~50 duplicate codes — de-duplication yields the 90
    unique 申万一级 industries. Malformed hrefs and non-881xxx codes are
    rejected. Returns ``(code, name)`` pairs in page order, or [] when the
    page cannot be fetched/decoded (the run treats an empty universe as a
    no-op for boards, never a crash). ``pool`` enables proxy-first rotation.
    """
    # One retry with a short backoff — a transient 500/429 on the list page
    # must not silently empty the whole industry universe (review P2-2).
    last_exc: Exception | None = None
    for attempt in range(2):
        try:
            await throttle.acquire()
            resp = await proxy_get(session, pool, THS_LIST_URL, headers=THS_HEADERS)
            resp.raise_for_status()
            html = resp.content.decode("gbk", errors="replace")
            break
        except Exception as e:
            last_exc = e
            if attempt == 0:
                await asyncio.sleep(0.5 + random.uniform(0, 0.5))
    else:
        print(f"    FAILED ths list: {last_exc}", file=sys.stderr)
        return []
    boards: list[tuple[str, str]] = []
    seen: set[str] = set()
    # Live page anchors are /thshy/detail/code/881xxx/ (2026-08-16 实测);
    # the optional (?:detail/code/)? segment keeps test fixtures using the
    # bare /thshy/881xxx/ form working.
    for m in re.finditer(
        r'href="[^"]*?/thshy/(?:detail/code/)?(881\d{3})/"\s*[^>]*>([^<]+)</a>',
        html,
    ):
        code, name = m.group(1), m.group(2).strip()
        if code in seen:
            continue
        seen.add(code)
        boards.append((code, name))
    return boards


async def fetch_ths_kline(
    session: AsyncSession,
    throttle: Throttle,
    code: str,
    year: int,
    *,
    pool: "ProxyPool | None" = None,
) -> list[str] | None:
    """Fetch one year of THS industry daily klines for one 881xxx code.

    The endpoint returns a JSONP body whose ``data`` field is a
    ``;``-separated list of 11-field CSV rows. Every row keeps the first 7
    fields and is reordered from the THS column order
    (date, open, high, low, close, volume, amount) into the EastMoney order
    consumed by ``_kline_records`` (date, open, close, high, low, volume,
    amount) — the two sources are NOT column-identical (issue #283 实测
    2026-08-16; reusing the EM mapping as-is would silently swap high/close).

    Returns the normalized 7-field rows, [] for a structurally empty year, or
    None when the request/parse failed. The caller walks back through years on
    None (never truncating history) and counts the board as failed when
    nothing was collected. ``pool`` enables proxy-first rotation.
    """
    # One retry with a short backoff — a transient failure on one year must
    # not truncate the board history (review P1-1/P2-2); the caller still
    # distinguishes None (failure) from [] (empty year).
    last_exc: Exception | None = None
    body = ""
    for attempt in range(2):
        try:
            await throttle.acquire()
            resp = await proxy_get(
                session,
                pool,
                THS_KLINE_TPL.format(code=code, year=year),
                headers=THS_HEADERS,
            )
            resp.raise_for_status()
            body = resp.text
            break
        except Exception as e:
            last_exc = e
            if attempt == 0:
                await asyncio.sleep(0.5 + random.uniform(0, 0.5))
    else:
        print(f"    FAILED ths kline {code}/{year}: {last_exc}", file=sys.stderr)
        return None
    start, end = body.find("("), body.rfind(")")
    if start == -1 or end <= start:
        return None
    payload_text = body[start + 1 : end]
    # The live API wraps a JSON object (``{"data": "…"}``); a bare CSV body
    # (test fixture shape) parses as raw rows — accept both. A JSON object
    # without a CSV-string ``data`` field is NOT a valid kline carrier (e.g.
    # error/captcha/malformed), so it is a failed fetch (None), not an empty
    # year — otherwise anti-bot/API breakage would be silently treated as a
    # weekend no-op and bypass fast-fail.
    try:
        payload = json.loads(payload_text)
    except Exception:
        payload = None
    if isinstance(payload, dict):
        data = payload.get("data")
        if not isinstance(data, str):
            return None
    else:
        data = payload_text
    rows: list[str] = []
    for line in re.split(r"[;\n]", data):
        parts = line.split(",")
        if len(parts) < 7:
            continue
        # THS: date,open,high,low,close,volume,amount → EM: date,open,close,high,low,volume,amount.
        reordered = [
            _ths_date_iso(parts[0]),
            parts[1],
            parts[4],
            parts[2],
            parts[3],
            parts[5],
            parts[6],
        ]
        rows.append(",".join(reordered))
    return rows


def _ths_date_iso(cell: str) -> str:
    """Normalize the THS compact date (YYYYMMDD) to ISO (YYYY-MM-DD).

    THS kline rows carry 8-digit dates (``20260105``) while ``_kline_records``
    compares trade_date against ``today`` lexically in ISO form — the raw
    compact form compares greater than an ISO today (``0`` > ``-``) and every
    row would be dropped as "future-dated". ISO-looking cells pass through.
    """
    cell = cell.strip()
    if len(cell) == 8 and cell.isdigit():
        return f"{cell[:4]}-{cell[4:6]}-{cell[6:]}"
    return cell


async def fetch_kline(
    session: AsyncSession,
    throttle: Throttle,
    secid: str,
    last_date: str | None = None,
    *,
    pool: "ProxyPool | None" = None,
) -> tuple[list[str], str] | None:
    """Fetch daily klines for one secid.

    Returns (klines, data.code) on success, None when every host×attempt is
    exhausted. ``last_date is None`` keeps the legacy full-history window
    ``beg=0&end=20500000``; otherwise ``beg`` is ``last_date + 1 day`` in
    YYYYMMDD compact form so the API returns only the incremental window. The
    caller decides code-match validation (official indices validate, boards
    don't). ``pool`` enables proxy-first rotation.
    """
    if last_date is None:
        beg = "0"
    else:
        beg = (date.fromisoformat(last_date) + timedelta(days=1)).strftime("%Y%m%d")
    params = {
        "secid": secid,
        "klt": "101",
        "fqt": "0",
        "beg": beg,
        "end": "20500000",
        "lmt": "1000000",
        "fields1": "f1,f2,f3,f4,f5,f6",
        "fields2": "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61",
    }
    data = await _get_json(session, throttle, KLINE_HOSTS, params, pool=pool)
    payload = (data or {}).get("data") or {}
    klines = payload.get("klines") or []
    return (klines, str(payload.get("code") or "")) if data else None


def _tencent_code(secid: str) -> str:
    """Map an EastMoney secid to a Tencent symbol (1.→sh, 0.→sz + lower code)."""
    if not isinstance(secid, str) or "." not in secid:
        raise ValueError(f"invalid EastMoney secid: {secid!r}")
    market, code = secid.split(".", 1)
    if market not in ("1", "0"):
        raise ValueError(f"invalid EastMoney market prefix: {secid!r}")
    if len(code) != 6 or not code.isdigit():
        raise ValueError(f"invalid EastMoney code: {secid!r}")
    return ("sh" if market == "1" else "sz") + code.lower()


def _tencent_amount_yuan(row: list[object] | tuple[object, ...]) -> str:
    """Extract the 成交额 from a newfqkline/get day row as a yuan CSV cell.

    ``row[8]`` is 成交额 in 万元 (0-based); convert to yuan (×10000). Missing,
    empty, non-numeric, non-finite, negative or overflowing values degrade to
    ``"0"`` so the merge import never crashes on a malformed Tencent payload.
    """
    if len(row) <= 8:
        return "0"
    raw = str(row[8]).strip()
    if raw == "" or raw == "-":
        return "0"
    try:
        amount_wan = float(raw)
    except (TypeError, ValueError):
        return "0"
    if not math.isfinite(amount_wan):
        return "0"
    yuan = amount_wan * 10000.0
    if not math.isfinite(yuan) or yuan < 0:
        return "0"
    if yuan.is_integer():
        return str(int(yuan))
    return str(yuan)


async def _fetch_tencent_kline(
    session: AsyncSession,
    throttle: Throttle,
    secid: str,
    last_date: str | None = None,
    *,
    pool: "ProxyPool | None" = None,
) -> list[str] | None:
    """Fetch daily klines from Tencent for one official index.

    Paginates with count=2000, advancing the end date backwards until a
    short page or a bounded page cap. Returns klines in the same 7-field CSV
    format as EastMoney (date,open,close,high,low,volume,amount) with amount
    in yuan taken from newfqkline/get's 成交额 field (万元 × 10000), or None
    on any failure.

    With ``last_date`` set this is an incremental fetch: it keeps only rows
    strictly newer than ``last_date`` and stops paging as soon as a row
    ``<= last_date`` is seen. A structurally valid response that yields no new
    rows returns ``[]`` (successful no-op), which is distinct from ``None``
    (request/malformed failure). ``pool`` enables proxy-first rotation.
    """
    try:
        tcode = _tencent_code(secid)
    except ValueError:
        # A misconfigured whitelist entry should degrade to a fallback failure
        # instead of crashing the whole run.
        return None
    pages: list[list[str]] = []
    end_date = ""
    previous_min: str | None = None

    for _ in range(_TENCENT_MAX_PAGES):
        # The fourth param field is the end date; leaving it empty returns the
        # latest `count` bars, setting it to (previous earliest - 1 day) pulls
        # the next older page (verified against the live Tencent API).
        param = f"{tcode},day,,{end_date},{_TENCENT_PAGE_SIZE},qfq"
        data = await _get_json(session, throttle, (TENCENT_KLINE_URL,), {"param": param}, pool=pool)
        if data is None:
            return None
        data_section = data.get("data")
        payload = data_section.get(tcode) if isinstance(data_section, dict) else None
        rows = payload.get("day") if isinstance(payload, dict) else None
        if not isinstance(rows, list):
            # Structurally malformed response: treat the whole target as failed.
            return None

        page_klines: list[str] = []
        min_date: str | None = None
        boundary_hit = False
        valid_row_count = 0
        for row in rows:
            if not isinstance(row, (list, tuple)) or len(row) < 6:
                continue
            cells = [str(v) for v in row[:6]]
            date_cell = cells[0].strip()
            if not date_cell:
                continue
            valid_row_count += 1
            if last_date is not None and date_cell <= last_date:
                # This page overlaps the already-stored boundary. Tencent day
                # rows are ascending (oldest first), so later rows in the
                # same page may still be newer than last_date — keep scanning
                # and collect them, but do not paginate to an older page.
                boundary_hit = True
                continue
            if min_date is None or date_cell < min_date:
                min_date = date_cell
            page_klines.append(",".join([*cells, _tencent_amount_yuan(row)]))

        if rows and valid_row_count == 0:
            # Non-empty page with no structurally valid rows is a malformed
            # payload, not a valid empty increment — treat as failure.
            return None

        if boundary_hit:
            # Even an empty kept set is a valid incremental no-op: record the
            # (possibly empty) page so the merge below returns [] cleanly.
            pages.append(page_klines)
            break

        if not page_klines:
            # Empty or all-invalid page: no more data.
            break

        pages.append(page_klines)

        if len(rows) < _TENCENT_PAGE_SIZE:
            break  # last page

        # Advance backwards: next page's end date is the day before the
        # earliest bar seen in this page. Stop if no backward progress or the
        # date is malformed (degrade instead of crashing).
        if min_date is None:
            break
        try:
            next_end = (date.fromisoformat(min_date) - timedelta(days=1)).isoformat()
        except ValueError:
            break
        if previous_min is not None and min_date >= previous_min:
            break
        previous_min = min_date
        end_date = next_end

    # Pages were fetched newest-first; reverse so the merged history is
    # chronological ascending (oldest first). Deduplicate by trade_date as a
    # defensive guard against any page-boundary overlap.
    seen: set[str] = set()
    merged: list[str] = []
    for page in reversed(pages):
        for kline in page:
            trade_date = kline.split(",", 1)[0]
            if trade_date in seen:
                continue
            seen.add(trade_date)
            merged.append(kline)
    return merged


async def run() -> Path:
    """Fetch official indices + THS industry boards into two CSVs.

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

    print(
        "Report: index_daily/index_basic (EastMoney push2his + Tencent "
        "fallback + THS industry kline)",
        file=sys.stderr,
    )
    print(f"Output: {daily_path.resolve()} / {basic_path.resolve()}", file=sys.stderr)

    with Progress("index_daily", output_csv=daily_path) as progress:
        throttle = Throttle()
        pool = make_proxy_pool()
        daily_records: list[dict[str, object]] = []
        basic_records: list[dict[str, object]] = []
        consecutive_failures = 0
        abort_reason: str | None = None

        async with AsyncSession(impersonate="chrome142") as session:
            industries = await fetch_ths_industry_list(session, throttle, pool=pool)
            print(f"THS industries: {len(industries)}", file=sys.stderr)
            if not industries:
                print(
                    "WARNING: THS 行业列表为空（抓取失败或页面结构变化）——"
                    "本次仅采集官方指数，90 个行业未采",
                    file=sys.stderr,
                )
            progress.update(
                total_items=len(industries) + len(OFFICIAL_INDICES),
                message="Fetching THS industry and index klines",
            )

            # THS industries first (list order), official after — index_basic
            # order convention (GUI picker lists boards prominently).
            for i, (code, name) in enumerate(industries):
                if abort_reason is not None:
                    break
                symbol = f"BK{code}"
                basic_records.append(
                    {"symbol": symbol, "name": name, "index_type": "industry"}
                )
                print(
                    f"  [industry] {symbol} {name} ...",
                    file=sys.stderr, end=" ", flush=True,
                )
                # Per-symbol incremental window (issue #292): an existing board
                # starts at the year of its MAX(trade_date) (or the next year
                # when MAX is a Dec-31 snapshot) and filters out rows already
                # stored; a new board (None) still backfills 2007→current.
                # MAX == today means the symbol is already up to date → skip.
                max_raw = max_trade_date(DOLT_TABLE, symbol)
                max_dt = _parse_max_date(max_raw)
                if max_dt is not None and max_dt >= _today():
                    consecutive_failures = 0
                    print("up to date", file=sys.stderr)
                    progress.update(
                        completed=i + 1,
                        fetched_rows=len(daily_records),
                        current_item=symbol,
                        message=f"Skipped industry {symbol} {name}",
                    )
                    continue

                klines: list[str] = []
                saw_response = False
                fetch_failed = False
                if max_dt is None:
                    start_year = THS_FIRST_YEAR
                elif max_dt.month == 12 and max_dt.day == 31:
                    start_year = min(max_dt.year + 1, _today().year)
                    start_year = max(start_year, THS_FIRST_YEAR)
                else:
                    start_year = max(max_dt.year, THS_FIRST_YEAR)
                max_iso = max_dt.isoformat() if max_dt is not None else None

                # Per-year pagination, newest first. For a new board an EMPTY
                # year is the historical boundary (no older data) and stops
                # the loop; for an incremental board an empty year just means
                # no new rows in that year — older years in the window may
                # still hold rows newer than MAX, so keep walking back. A
                # request/parse FAILURE (None) is logged and the loop keeps
                # walking; a no-op is only accepted when every year in the
                # window responded successfully (otherwise a failed latest
                # year would be silently masked as "no new bars").
                for year in range(_today().year, start_year - 1, -1):
                    year_rows = await fetch_ths_kline(session, throttle, code, year, pool=pool)
                    if year_rows is None:
                        fetch_failed = True
                        print(
                            f"    year {year} fetch failed (kept going)",
                            file=sys.stderr,
                        )
                        continue
                    saw_response = True
                    if not year_rows:
                        if max_iso is None:
                            break
                        continue
                    if max_iso is not None:
                        kept = [
                            row for row in year_rows
                            if row.split(",", 1)[0] > max_iso
                        ]
                        if not kept:
                            continue
                        klines.extend(kept)
                    else:
                        klines.extend(year_rows)
                if klines and max_dt is not None and fetch_failed:
                    # Partial success in an incremental window is NOT a clean
                    # success: writing the successful years would advance
                    # MAX(trade_date) past a failed year and the missing bars
                    # would never be re-fetched. Discard the partial rows and
                    # count the board as failed so the next run retries the
                    # full window.
                    consecutive_failures, abort_reason = _bump_failure(consecutive_failures)
                    print("FAILED (partial year failure, rows discarded)", file=sys.stderr)
                elif not klines:
                    if max_dt is not None and saw_response and not fetch_failed:
                        # Weekend/halt/valid-empty increment: a successful no-op.
                        consecutive_failures = 0
                        print("no new bars", file=sys.stderr)
                    else:
                        consecutive_failures, abort_reason = _bump_failure(consecutive_failures)
                        print("FAILED (no klines)", file=sys.stderr)
                else:
                    daily_records.extend(
                        _kline_records(symbol, "industry", klines, _today())
                    )
                    print(f"{len(klines)} bars", file=sys.stderr)
                    consecutive_failures = 0
                progress.update(
                    completed=i + 1,
                    fetched_rows=len(daily_records),
                    current_item=symbol,
                    message=f"Fetched industry {symbol} {name}",
                )
                if abort_reason is not None:
                    break

            # Official indices: response data.code must echo the whitelisted code,
            # otherwise the API returned a different index (skip + log). A code
            # mismatch is neither a failure nor a success: it must not reset the
            # consecutive-failure counter (would mask a real block) nor count
            # toward it (would false-trigger on a delisted/renamed index).
            for j, target in enumerate(OFFICIAL_INDICES, start=len(industries)):
                if abort_reason is not None:
                    break
                symbol = f"SH{target['code']}" if target["secid"].startswith("1.") \
                    else f"SZ{target['code']}"
                print(
                    f"  [official] {target['secid']} {target['name']} ...",
                    file=sys.stderr, end=" ", flush=True,
                )
                # Per-symbol incremental window (issue #292): pass the stored
                # MAX(trade_date) so EastMoney/Tencent only fetch newer bars.
                # MAX == today (or a clamped future dirty value) means the
                # symbol is already up to date → skip.
                max_raw = max_trade_date(DOLT_TABLE, symbol)
                max_dt = _parse_max_date(max_raw)
                if max_dt is not None and max_dt >= _today():
                    consecutive_failures = 0
                    basic_records.append(
                        {"symbol": symbol, "name": target["name"], "index_type": "official"}
                    )
                    print("up to date", file=sys.stderr)
                    progress.update(
                        completed=j + 1,
                        fetched_rows=len(daily_records),
                        current_item=target["name"],
                        message=f"Skipped official {target['name']}",
                    )
                    continue

                last_date = max_dt.isoformat() if max_dt is not None else None
                result = await fetch_kline(
                    session, throttle, target["secid"], last_date=last_date, pool=pool
                )
                if result is None or not result[0]:
                    # EastMoney failed/empty → try Tencent fallback (issue #278).
                    source_label = "FAILED" if result is None else "empty (skipped)"
                    print(
                        f"{source_label} (eastmoney); trying tencent...",
                        file=sys.stderr,
                    )
                    tencent_klines = await _fetch_tencent_kline(
                        session, throttle, target["secid"], last_date=last_date, pool=pool
                    )
                    if last_date is not None:
                        # Incremental: a valid Tencent response with no new rows
                        # ([]) is a successful no-op, distinct from a
                        # request/malformed failure (None).
                        if tencent_klines is not None:
                            consecutive_failures = 0
                            basic_records.append(
                                {"symbol": symbol, "name": target["name"], "index_type": "official"}
                            )
                            daily_records.extend(
                                _kline_records(symbol, "official", tencent_klines, _today())
                            )
                            print(f"{len(tencent_klines)} bars (tencent)", file=sys.stderr)
                        else:
                            consecutive_failures, abort_reason = _bump_failure(consecutive_failures)
                            print("FAILED (eastmoney+tencent)", file=sys.stderr)
                    elif tencent_klines:
                        consecutive_failures = 0
                        basic_records.append(
                            {"symbol": symbol, "name": target["name"], "index_type": "official"}
                        )
                        daily_records.extend(
                            _kline_records(symbol, "official", tencent_klines, _today())
                        )
                        print(f"{len(tencent_klines)} bars (tencent)", file=sys.stderr)
                    else:
                        consecutive_failures, abort_reason = _bump_failure(consecutive_failures)
                        print("FAILED (eastmoney+tencent)", file=sys.stderr)
                else:
                    klines, code = result
                    # EastMoney echoes either the bare code ("000001") or the full
                    # symbol ("SH000001"); accept both, anything else is a different
                    # index (delisted/renamed code) — skip. A mismatch must NOT
                    # trigger the Tencent fallback (issue #278 semantics).
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
                if abort_reason is not None:
                    break

        if abort_reason is not None:
            _persist_outputs(
                daily_records, basic_records, daily_path, basic_path
            )
            raise RuntimeError(abort_reason)

        if not daily_records and not basic_records:
            _persist_outputs(
                daily_records, basic_records, daily_path, basic_path
            )
            raise RuntimeError(
                "No index data (rate-limited or empty) — "
                "aborting, no CSV written"
            )

        _persist_outputs(
            daily_records, basic_records, daily_path, basic_path
        )
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
    (official indexes) and by ``name`` against the ``industry`` section
    (THS industry boards, which carry BK symbols the index section does not
    cover). The symbol hit wins via COALESCE; a double LEFT JOIN keeps one
    row per CSV row (no inflation). Unmapped rows → NULL (GUI falls back to
    Chinese). A missing mapping degrades gracefully — the base import always
    lands.
    """
    print("[import index_basic]", file=sys.stderr)
    mapping = load_name_en_mapping()
    try:
        if mapping:
            joins = """
                LEFT JOIN _tmp_name_en m1
                  ON m1.section = 'index' AND m1.`key` = t.symbol
                LEFT JOIN _tmp_name_en m2
                  ON m2.section = 'industry' AND m2.`key` = t.name
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
            if (
                csv_close == 0.0
                or stored is None
                or abs(stored - csv_close) / csv_close <= _SAMPLE_TOLERANCE
            ):
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
    """Fetch a single close from Dolt (None when the row is absent).

    Single-quote escaping is defense-in-depth (security review P2): the
    values are CSV-derived and normally validated upstream, but the SELECT
    interpolates them raw.
    """
    try:
        out = dolt_sql_csv(
            "SELECT close FROM index_daily "
            f"WHERE symbol = '{symbol.replace(chr(39), chr(39) * 2)}' "
            f"AND trade_date = '{trade_date.replace(chr(39), chr(39) * 2)}'"
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
