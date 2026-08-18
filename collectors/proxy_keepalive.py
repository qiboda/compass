#!/usr/bin/env python3
"""Keepalive daemon for the proxy_pool.

Runs in a loop and keeps the proxy_pool Redis hash warm by seeding from two
freeproxy sources every cycle:

- JSON snapshot (default GitHub raw ``proxies.json``): on success the raw
  payload is saved to ``--snapshot``; on failure (429/timeout/network) the
  local snapshot is used as a fallback.
- Realtime scrape via ``pyfreeproxy``.

Every sub-step is isolated with try/except: one broken source never crashes
the daemon. ``--once`` runs a single cycle and returns 0 (used by tests and
manual smoke runs).
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import fetch_freeproxy

DEFAULT_SNAPSHOT = "/tmp/freeproxy.json"

__all__ = ["main", "run_cycle", "run_json_cycle", "run_realtime_cycle"]


def _write_snapshot(snapshot: Path, payload: Any) -> None:
    """Persist the raw JSON payload for future fallback."""
    snapshot.parent.mkdir(parents=True, exist_ok=True)
    tmp = snapshot.with_name(f"{snapshot.name}.{time.time_ns()}.tmp")
    tmp.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    tmp.replace(snapshot)


def run_json_cycle(
    json_url: str,
    snapshot: Path,
    redis_url: str,
    table: str,
    limit: int,
) -> int:
    """Seed from the JSON source, falling back to the local snapshot.

    Returns the number of records written to Redis (0 on any failure).
    """
    payload: Any = None
    try:
        payload = fetch_freeproxy.fetch_json_payload(json_url)
        _write_snapshot(snapshot, payload)
        print(f"[keepalive] json source ok ({len(str(payload))} bytes)", file=sys.stderr)
    except Exception as exc:
        print(f"[keepalive] json source failed: {exc}", file=sys.stderr)
        if snapshot.exists():
            try:
                payload = json.loads(snapshot.read_text(encoding="utf-8"))
                print(f"[keepalive] using snapshot {snapshot}", file=sys.stderr)
            except Exception as snap_exc:
                print(f"[keepalive] snapshot read failed: {snap_exc}", file=sys.stderr)
                payload = None
        else:
            payload = None

    if payload is None:
        return 0
    records = fetch_freeproxy.records_from_json_data(payload, limit)
    if not records:
        print("[keepalive] json source produced no usable records", file=sys.stderr)
        return 0
    return fetch_freeproxy.write_to_redis(redis_url, table, records)


def run_realtime_cycle(
    redis_url: str,
    table: str,
    limit: int,
    sources: Iterable[str],
) -> int:
    """Seed from pyfreeproxy realtime sources.

    Returns the number of records written to Redis (0 on any failure).
    """
    try:
        records = fetch_freeproxy.fetch_realtime_proxies(limit, sources)
    except Exception as exc:
        print(f"[keepalive] realtime source failed: {exc}", file=sys.stderr)
        return 0
    if not records:
        print("[keepalive] realtime source produced no records", file=sys.stderr)
        return 0
    return fetch_freeproxy.write_to_redis(redis_url, table, records)


def run_cycle(
    json_url: str,
    snapshot: Path,
    redis_url: str,
    table: str,
    limit: int,
    sources: Iterable[str],
) -> tuple[int, int]:
    """Run one keepalive cycle; returns (json_written, realtime_written)."""
    json_written = 0
    realtime_written = 0
    try:
        json_written = run_json_cycle(json_url, snapshot, redis_url, table, limit)
    except Exception as exc:
        print(f"[keepalive] json cycle error: {exc}", file=sys.stderr)
    try:
        realtime_written = run_realtime_cycle(redis_url, table, limit, sources)
    except Exception as exc:
        print(f"[keepalive] realtime cycle error: {exc}", file=sys.stderr)
    return json_written, realtime_written


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Keep proxy_pool warm by seeding freeproxy json + realtime sources"
    )
    parser.add_argument(
        "--interval",
        type=int,
        default=600,
        help="seconds between cycles (default: %(default)s)",
    )
    parser.add_argument("--once", action="store_true", help="run one cycle and exit")
    parser.add_argument(
        "--json-url",
        default=fetch_freeproxy.DEFAULT_JSON_URL,
        help="proxies.json URL (default: %(default)s)",
    )
    parser.add_argument(
        "--snapshot",
        default=DEFAULT_SNAPSHOT,
        help="local snapshot fallback path (default: %(default)s)",
    )
    parser.add_argument(
        "--redis-url",
        default=fetch_freeproxy.DEFAULT_REDIS_URL,
        help="Redis URL used by proxy_pool (default: %(default)s)",
    )
    parser.add_argument(
        "--table",
        default=fetch_freeproxy.DEFAULT_TABLE,
        help="Redis hash table (default: %(default)s)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=fetch_freeproxy.DEFAULT_LIMIT,
        help="maximum proxies per source (default: %(default)s)",
    )
    parser.add_argument(
        "--realtime-sources",
        default=",".join(fetch_freeproxy.DEFAULT_REALTIME_SOURCES),
        help="comma-separated pyfreeproxy source names",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the keepalive loop (or a single ``--once`` cycle)."""
    args = _parse_args(argv)
    if args.limit < 0:
        print("fatal: --limit must be >= 0", file=sys.stderr)
        return 1
    if not args.once and args.interval <= 0:
        print("fatal: --interval must be > 0 unless --once is used", file=sys.stderr)
        return 2

    snapshot = Path(args.snapshot)
    sources = [s.strip() for s in args.realtime_sources.split(",") if s.strip()]

    while True:
        json_written, realtime_written = run_cycle(
            args.json_url,
            snapshot,
            args.redis_url,
            args.table,
            args.limit,
            sources,
        )
        print(
            f"[keepalive] cycle done: json={json_written} realtime={realtime_written}",
            file=sys.stderr,
        )
        if args.once:
            return 0
        time.sleep(args.interval)


if __name__ == "__main__":  # pragma: no cover - CLI entrypoint
    raise SystemExit(main())
