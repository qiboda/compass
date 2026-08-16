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
import ipaddress
import json
import sys
from collections.abc import Iterable
from typing import Any

from curl_cffi import requests as _curl_requests
from redis import Redis

__all__ = ["main"]

DEFAULT_JSON_URL = "https://raw.githubusercontent.com/CharlesPikachu/freeproxy/master/proxies.json"
DEFAULT_REDIS_URL = "redis://@127.0.0.1:6379/0"
DEFAULT_TABLE = "use_proxy"
DEFAULT_LIMIT = 300
DEFAULT_REALTIME_SOURCES = [
    "ProxiflyProxiedSession",
    "TrustyTechProxiedSession",
]


def _is_http_protocol(protocol: str) -> bool:
    """Return True when a freeproxy protocol string includes HTTP(S)."""
    return "http" in protocol.lower()


def _is_public_ip(host: str) -> bool:
    """Return True when ``host`` is a public, non-reserved IPv4/IPv6 address."""
    try:
        addr = ipaddress.ip_address(host)
    except ValueError:
        return False
    return not (
        addr.is_private
        or addr.is_loopback
        or addr.is_link_local
        or addr.is_multicast
        or addr.is_reserved
        or addr.is_unspecified
    )


def _safe_proxy(host: Any, port: Any) -> str | None:
    """Return ``host:port`` only when the values look like a safe public proxy."""
    if not isinstance(host, str) or not host:
        return None
    if not _is_public_ip(host):
        return None
    if any(ch.isspace() or ord(ch) < 32 for ch in host):
        return None
    if "/" in host or "@" in host:
        return None
    try:
        port_int = int(port)
    except (TypeError, ValueError):
        return None
    if not 1 <= port_int <= 65535:
        return None
    return f"{host}:{port_int}"


def _safe_proxy_str(proxy: str) -> str | None:
    """Validate a ``host:port`` proxy string before it is stored in Redis."""
    if not proxy:
        return None
    if any(ch.isspace() or ord(ch) < 32 for ch in proxy):
        return None
    if "/" in proxy or "@" in proxy:
        return None
    host, sep, port = proxy.rpartition(":")
    if not sep or not host:
        return None
    if not _is_public_ip(host):
        return None
    try:
        port_int = int(port)
    except ValueError:
        return None
    if not 1 <= port_int <= 65535:
        return None
    return f"{host}:{port_int}"


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
    proxy = _safe_proxy(item.get("ip"), item.get("port"))
    if proxy is None:
        raise ValueError("invalid proxy entry")
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
    raw_proxy = str(getattr(info, "proxy", ""))
    if "://" in raw_proxy:
        raw_proxy = raw_proxy.rsplit("://", 1)[-1]
    proxy = _safe_proxy_str(raw_proxy)
    if proxy is None:
        raise ValueError("pyfreeproxy returned an invalid proxy string")
    return {
        "proxy": proxy,
        "https": False,
        "fail_count": 0,
        "region": str(getattr(info, "country_code", "") or ""),
        "anonymous": str(getattr(info, "anonymity", "") or ""),
        "source": "freeproxy",
        "check_count": 0,
        "last_status": True,
        "last_time": "",
    }


def fetch_json_proxies(url: str, limit: int) -> list[dict[str, Any]]:
    """Download and filter the freeproxy ``proxies.json`` snapshot."""
    resp: Any = _curl_requests.get(url, timeout=30)
    resp.raise_for_status()
    payload = resp.json()
    data = payload.get("data") if isinstance(payload, dict) else None
    if not isinstance(data, list):
        data = []
    items = [
        item
        for item in data
        if isinstance(item, dict) and _is_http_protocol(str(item.get("protocol", "")))
    ]
    items.sort(key=_score_item, reverse=True)
    records: list[dict[str, Any]] = []
    for item in items[:limit]:
        try:
            records.append(normalize_json_item(item))
        except ValueError:
            continue
    return records


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
            session = BuildProxiedSession({"max_pages": 1, "type": source, "disable_print": True})
            proxies = session.refreshproxies()
        except Exception as exc:
            print(f"warning: realtime source {source} failed: {exc}", file=sys.stderr)
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
    try:
        for record in records:
            proxy = _safe_proxy_str(str(record.get("proxy", "")))
            if proxy is None:
                continue
            safe_record = dict(record)
            safe_record["proxy"] = proxy
            client.hset(table, proxy, json.dumps(safe_record, ensure_ascii=False))
            written += 1
    finally:
        client.close()
    return written


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Seed freeproxy proxies into proxy_pool Redis")
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

    try:
        if args.source == "json":
            records = fetch_json_proxies(args.json_url, args.limit)
        else:
            print(
                "warning: --source realtime makes outbound requests to untrusted "
                "third-party proxy sources; run it only in a sandboxed network",
                file=sys.stderr,
            )
            sources = [s.strip() for s in args.realtime_sources.split(",") if s.strip()]
            records = fetch_realtime_proxies(args.limit, sources)
    except Exception as exc:
        print(f"fatal: {exc}", file=sys.stderr)
        return 1

    if not records:
        print("no proxies fetched", file=sys.stderr)
        return 1

    try:
        written = write_to_redis(args.redis_url, args.table, records)
    except Exception as exc:
        print(f"fatal: {exc}", file=sys.stderr)
        return 1

    print(f"seeded {written} proxies into {args.table} ({args.source})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
