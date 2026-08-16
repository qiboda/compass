"""Fetch free proxies from CharlesPikachu/freeproxy and seed them into proxy_pool Redis.

Two sources are supported:

- ``--source json`` (default): download the daily ``proxies.json`` snapshot from
  the freeproxy repository.  This is fast and has no extra runtime dependency
  beyond the collectors environment.
- ``--source realtime``: use the ``pyfreeproxy`` library to scrape live proxy
  sources.  This requires ``pyfreeproxy`` (declared in ``collectors/pyproject.toml``).

The script normalizes proxy records and writes them into the Redis hash used by
proxy_pool (default table ``use_proxy``).  proxy_pool's scheduler then validates
HTTP/HTTPS availability, including the HTTPS validator patch from issue #290.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Iterable
from typing import Any

from curl_cffi import requests as curl_requests
from redis import Redis

__all__ = ["curl_requests", "main"]

DEFAULT_JSON_URL = (
    "https://raw.githubusercontent.com/CharlesPikachu/freeproxy/master/proxies.json"
)
DEFAULT_REDIS_URL = "redis://@127.0.0.1:6379/0"
DEFAULT_TABLE = "use_proxy"
DEFAULT_LIMIT = 300
DEFAULT_REALTIME_SOURCES = [
    "ProxiflyProxiedSession",
    "KuaidailiProxiedSession",
    "QiyunipProxiedSession",
    "TrustyTechProxiedSession",
]


def _is_http_protocol(protocol: str) -> bool:
    """Return True when a freeproxy protocol string includes HTTP(S)."""
    return "http" in protocol.lower()


def _score_item(item: dict[str, Any]) -> int:
    """Prefer HTTPS-capable, mainland-China and elite proxies for the first cut."""
    protocol = str(item.get("protocol", "")).lower()
    score = 0
    if "https" in protocol:
        score += 2
    if str(item.get("country", "")).upper() == "CN":
        score += 1
    if str(item.get("anonymity", "")).lower() == "elite":
        score += 1
    return score


def normalize_json_item(item: dict[str, Any]) -> dict[str, Any]:
    """Convert a freeproxy ``proxies.json`` entry into a proxy_pool Redis record."""
    proxy = f"{item['ip']}:{item['port']}"
    return {
        "proxy": proxy,
        "https": False,
        "fail_count": 0,
        "region": str(item.get("country", "")),
        "anonymous": str(item.get("anonymity", "")),
        "source": "freeproxy",
        "check_count": 0,
        "last_status": True,
        "last_time": "",
    }


def normalize_proxy_info(info: Any) -> dict[str, Any]:
    """Convert a pyfreeproxy ``ProxyInfo`` object into a proxy_pool Redis record."""
    proxy = str(getattr(info, "proxy", ""))
    if not proxy:
        raise ValueError("pyfreeproxy returned a proxy without a proxy string")
    return {
        "proxy": proxy,
        "https": False,
        "fail_count": 0,
        "region": str(getattr(info, "country", "") or ""),
        "anonymous": str(getattr(info, "anonymity", "") or ""),
        "source": "freeproxy",
        "check_count": 0,
        "last_status": True,
        "last_time": "",
    }


def fetch_json_proxies(url: str, limit: int) -> list[dict[str, Any]]:
    """Download and filter the freeproxy ``proxies.json`` snapshot."""
    resp: Any = curl_requests.get(url, timeout=30)
    resp.raise_for_status()
    payload = resp.json()
    data = payload.get("data", []) if isinstance(payload, dict) else []
    items = [
        item
        for item in data
        if isinstance(item, dict) and _is_http_protocol(str(item.get("protocol", "")))
    ]
    items.sort(key=_score_item, reverse=True)
    return [normalize_json_item(item) for item in items[:limit]]


def fetch_realtime_proxies(limit: int, sources: Iterable[str]) -> list[dict[str, Any]]:
    """Scrape live proxies with pyfreeproxy and normalize them for proxy_pool."""
    try:
        from freeproxy.modules import BuildProxiedSession
    except ImportError as exc:  # pragma: no cover - depends on environment
        raise RuntimeError(
            "pyfreeproxy is not installed; run `uv sync --project collectors`"
        ) from exc

    records: list[dict[str, Any]] = []
    for source in sources:
        try:
            session = BuildProxiedSession(
                {"max_pages": 1, "type": source, "disable_print": True}
            )
            proxies = session.refreshproxies()
        except Exception:
            continue
        for info in proxies:
            try:
                records.append(normalize_proxy_info(info))
            except ValueError:
                continue
            if len(records) >= limit:
                return records
    return records


def _new_redis_client(redis_url: str) -> Redis:
    """Create a Redis client with decoded responses."""
    return Redis.from_url(redis_url, decode_responses=True)


def write_to_redis(redis_url: str, table: str, records: Iterable[dict[str, Any]]) -> int:
    """Write proxy records into a Redis hash and return the number written."""
    client = _new_redis_client(redis_url)
    written = 0
    for record in records:
        proxy = record.get("proxy")
        if not proxy:
            continue
        client.hset(table, str(proxy), json.dumps(record, ensure_ascii=False))
        written += 1
    return written


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Seed freeproxy proxies into proxy_pool Redis"
    )
    parser.add_argument(
        "--source",
        choices=("json", "realtime"),
        default="json",
        help="proxy source: json snapshot (default) or pyfreeproxy realtime scrape",
    )
    parser.add_argument("--json-url", default=DEFAULT_JSON_URL, help="proxies.json URL")
    parser.add_argument(
        "--redis-url",
        default=DEFAULT_REDIS_URL,
        help="Redis URL used by proxy_pool (default: %(default)s)",
    )
    parser.add_argument(
        "--table", default=DEFAULT_TABLE, help="Redis hash table (default: %(default)s)"
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=DEFAULT_LIMIT,
        help="maximum number of proxies to seed (default: %(default)s)",
    )
    parser.add_argument(
        "--realtime-sources",
        default=",".join(DEFAULT_REALTIME_SOURCES),
        help="comma-separated pyfreeproxy source names for --source realtime",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the freeproxy -> proxy_pool seeding flow."""
    args = _parse_args(argv)
    if args.limit < 0:
        print("fatal: --limit must be >= 0", file=sys.stderr)
        return 1

    if args.source == "json":
        records = fetch_json_proxies(args.json_url, args.limit)
    else:
        sources = [s.strip() for s in args.realtime_sources.split(",") if s.strip()]
        records = fetch_realtime_proxies(args.limit, sources)

    if not records:
        print("no proxies fetched", file=sys.stderr)
        return 1

    written = write_to_redis(args.redis_url, args.table, records)
    print(f"seeded {written} proxies into {args.table} ({args.source})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
