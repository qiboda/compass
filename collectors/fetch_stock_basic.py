#!/usr/bin/env python3
"""Fetch A-share stock basic info from EastMoney and output CSV.

Covers all 6000+ stocks: symbol, ts_code, name, industry, market, list_date.
Handles EastMoney anti-crawler: TLS fingerprint impersonation, rate limiting, retry.

Usage:
    uv run scripts/fetch_stock_basic.py                    # all stocks
    uv run scripts/fetch_stock_basic.py -o stock_basic.csv # custom output
    uv run scripts/fetch_stock_basic.py --page-size 500    # batch size
"""

import argparse
import asyncio
import csv
import math
import random
import sys
import time
from pathlib import Path

from curl_cffi.requests import AsyncSession

# ── EastMoney API ──────────────────────────────────────────────
EM_LIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"
EM_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
)
EM_HEADERS = {
    "User-Agent": EM_UA,
    "Accept": "*/*",
    "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
    "Referer": "https://quote.eastmoney.com/",
    "Sec-Ch-Ua": '"Chromium";v="142", "Google Chrome";v="142", "Not_A Brand";v="99"',
    "Sec-Ch-Ua-Mobile": "?0",
    "Sec-Ch-Ua-Platform": '"Windows"',
    "Sec-Fetch-Dest": "empty",
    "Sec-Fetch-Mode": "cors",
    "Sec-Fetch-Site": "same-site",
    "Connection": "keep-alive",
}

# Market filter: all A-shares (深市主板+创业板+沪市主板+科创板+北交所)
EM_FS = "m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:0+t:81"
# Fields: f12=code, f14=name, f100=industry, f102=market, f124=list_date(ts)
# stock_basic: classification-only fields. 行情→investment_data, 财务→fin_indicators
EM_FIELDS = (
    "f12,f13,f14,f26,"  # identity: code, market, name, list_date
    "f100,f101,f102,f103,"  # industry/region/notes
    "f127,f128,f134,"  # industry(alt), lead_stock, industry_member_count
    "f189,"  # list_date(alt)
    "f124,f221"  # timestamps
)

# Rate limiting: ~1 req/s + random jitter
EM_MIN_INTERVAL = 0.8
EM_JITTER = (0.1, 0.5)
EM_MAX_RETRIES = 4

# ── Exchange inference ─────────────────────────────────────────


def infer_exchange(code: str) -> str:
    """Infer exchange from 6-digit A-share code.

    Mirrors compass-core: symbol::infer_exchange_from_code()
    """
    if code.startswith("6"):
        return "SH"
    if code.startswith("8"):
        return "BJ"
    return "SZ"


def to_ts_code(code: str) -> str:
    """Build ts_code (e.g. 000001.SZ) from bare 6-digit code."""
    exchange = infer_exchange(code)
    suffix = {"SH": "SH", "SZ": "SZ", "BJ": "BJ"}[exchange]
    return f"{code}.{suffix}"


def to_symbol(code: str) -> str:
    """Build Dolt-native symbol (e.g. SZ000001) from bare 6-digit code."""
    exchange = infer_exchange(code)
    return f"{exchange}{code}"


def ts_to_date(ts: int | None) -> str:
    """Convert Unix timestamp (seconds) to YYYY-MM-DD."""
    if ts is None or ts <= 0:
        return ""
    return time.strftime("%Y-%m-%d", time.localtime(ts))


# ── Throttle ───────────────────────────────────────────────────


class Throttle:
    def __init__(self, min_interval: float = EM_MIN_INTERVAL):
        self._min_interval = min_interval
        self._last: float = 0.0

    async def acquire(self):
        now = time.monotonic()
        since_last = now - self._last
        if since_last < self._min_interval:
            wait = self._min_interval - since_last + random.uniform(*EM_JITTER)
            await asyncio.sleep(wait)
        else:
            await asyncio.sleep(random.uniform(0, 0.2))
        self._last = time.monotonic()


# ── Fetch logic ────────────────────────────────────────────────


async def fetch_page(
    session: AsyncSession,
    throttle: Throttle,
    page: int,
    page_size: int,
) -> list[dict]:
    """Fetch one page of stock basic data with retry."""
    params = {
        "pn": page,
        "pz": page_size,
        "po": "1",
        "np": "1",
        "fltt": "2",
        "invt": "2",
        "fid": "f3",
        "fs": EM_FS,
        "fields": EM_FIELDS,
        "ut": "bd1d9ddb04089700cf9c27f6f7426281",
    }

    for attempt in range(EM_MAX_RETRIES):
        try:
            await throttle.acquire()
            resp = await session.get(EM_LIST_URL, params=params, headers=EM_HEADERS)
            data = resp.json()
            items = data.get("data", {}).get("diff")
            if items is None:
                return []

            result = []
            for item in items:
                code = item.get("f12", "")
                if not code:
                    continue
                record = {"symbol": to_symbol(code), "ts_code": to_ts_code(code)}
                record.update(item)
                result.append(record)
            return result

        except Exception as e:
            wait = min(2**attempt, 30) + random.uniform(0, 2)
            if attempt < EM_MAX_RETRIES - 1:
                print(
                    f"  Retry {attempt + 1}/{EM_MAX_RETRIES} in {wait:.1f}s: {e}", file=sys.stderr
                )
                await asyncio.sleep(wait)
            else:
                raise

    return []


# ── Main ───────────────────────────────────────────────────────


async def main():
    parser = argparse.ArgumentParser(description="Fetch A-share stock basic info")
    parser.add_argument("-o", "--output", default="stock_basic.csv", help="Output CSV path")
    parser.add_argument("--page-size", type=int, default=100, help="Items per page")
    parser.add_argument("--max-pages", type=int, default=100, help="Max pages to fetch")
    parser.add_argument(
        "--resume", action="store_true", help="Resume from existing CSV instead of overwriting"
    )
    args = parser.parse_args()

    output_path = Path(args.output)
    page_size = args.page_size

    start_page = 1
    file_mode = "w"
    if args.resume and output_path.exists():
        with open(output_path, encoding="utf-8-sig") as f:
            existing = sum(1 for _ in f) - 1
        if existing > 0:
            start_page = max(1, existing // page_size)
            file_mode = "a"
            print(f"Resuming from page {start_page} ({existing} rows)", file=sys.stderr)

    print(f"Fetching stock basic info (page_size={page_size})...", file=sys.stderr)

    throttle = Throttle()
    total = 0

    async with AsyncSession(impersonate="chrome142") as session:
        # First page to get total count
        params1 = {
            "pn": 1,
            "pz": 1,
            "po": "1",
            "np": "1",
            "fltt": "2",
            "invt": "2",
            "fid": "f3",
            "fs": EM_FS,
            "fields": EM_FIELDS,
            "ut": "bd1d9ddb04089700cf9c27f6f7426281",
        }
        resp = await session.get(EM_LIST_URL, params=params1, headers=EM_HEADERS)
        data = resp.json()
        total_count = data.get("data", {}).get("total", 0)
        if total_count:
            print(f"  Total stocks reported: {total_count}", file=sys.stderr)

        total_pages = min(
            math.ceil(total_count / page_size) if total_count else args.max_pages, args.max_pages
        )

        writer = None
        with open(output_path, file_mode, newline="", encoding="utf-8-sig") as f:
            for page in range(start_page, total_pages + 1):
                items = await fetch_page(session, throttle, page, page_size)
                if not items:
                    print(f"  Page {page}: empty, stopping.", file=sys.stderr)
                    break

                if writer is None:
                    api_fields = sorted(
                        [k for k in items[0] if k not in ("symbol", "ts_code")],
                        key=lambda x: int(x[1:]) if x[1:].isdigit() else 0,
                    )
                    fieldnames = ["symbol", "ts_code"] + api_fields
                    writer = csv.DictWriter(f, fieldnames=fieldnames)
                    if file_mode == "w":
                        writer.writeheader()

                writer.writerows(items)
                total += len(items)

                progress = 100 * total / total_count if total_count else 0
                print(
                    f"  Page {page}/{total_pages} | {total} stashed | {progress:.0f}%",
                    file=sys.stderr,
                )

    print(f"\nDone: {total} stocks → {output_path.resolve()}", file=sys.stderr)


if __name__ == "__main__":
    asyncio.run(main())
