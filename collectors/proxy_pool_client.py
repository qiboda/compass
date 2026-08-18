"""Proxy pool client for collectors.

Wraps the local ``jhao104/proxy_pool`` HTTP API (default
``http://127.0.0.1:5010``) so collectors can implement proxy-first HTTPS
fetching:

- ``get_proxy`` asks the pool for a random HTTPS-capable proxy.
- An empty/unreachable pool is a *degradation*, never a hard error: the
  caller falls back to direct and this module records the event in
  ``proxy_pool_state.json`` (timestamp / pool count / degraded flag) and
  prints a loud warning once per ``ProxyPool`` instance.
- ``delete_proxy`` removes a bad proxy from the pool; API failures are
  logged but never raised.

This module intentionally does not import ``common`` to avoid a circular
dependency: ``common`` imports this module at module load.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

from curl_cffi import requests as _curl_requests

logger = logging.getLogger("compass_collectors.proxy_pool")

DEFAULT_API_URL = "http://127.0.0.1:5010"
PROXY_STATE_FILENAME = "proxy_pool_state.json"
DEFAULT_PROXY_MAX_ATTEMPTS = 3
_DEFAULT_CSV_DIR = "/data/compass-data/csv"
_API_TIMEOUT = 5.0

__all__ = [
    "DEFAULT_API_URL",
    "PROXY_STATE_FILENAME",
    "DEFAULT_PROXY_MAX_ATTEMPTS",
    "ProxyPool",
    "default_state_path",
    "proxy_enabled",
]


def proxy_enabled() -> bool:
    """Return whether the proxy layer is enabled.

    Set ``COMPASS_PROXY_DISABLE=1`` (or ``true``/``True``) to disable it
    entirely, which is useful for tests and local development without a pool.
    """
    return os.environ.get("COMPASS_PROXY_DISABLE", "").lower() not in ("1", "true")


def default_state_path() -> Path:
    """Return the default ``proxy_pool_state.json`` path.

    Lives next to the raw CSVs so operators can find it in the same place as
    progress files; ``COMPASS_CSV_DIR`` overrides it (test isolation).
    """
    base = os.environ.get("COMPASS_CSV_DIR", _DEFAULT_CSV_DIR)
    return Path(base) / PROXY_STATE_FILENAME


class ProxyPool:
    """Thin async client for the local proxy_pool API.

    All network calls go through ``_api_get`` (a sync hook) via
    ``asyncio.to_thread`` so the event loop is not blocked. Tests replace
    ``_api_get`` to simulate pool states without a live server.
    """

    def __init__(self, api_url: str | None = None, state_path: Path | None = None) -> None:
        base = api_url or os.environ.get("COMPASS_PROXY_API_URL") or DEFAULT_API_URL
        self.api_url = str(base).rstrip("/")
        self.state_path = Path(state_path) if state_path is not None else default_state_path()
        self._warned_empty = False

    def _api_get(self, path: str, params: dict[str, Any] | None = None) -> Any:
        """Perform one synchronous GET against the proxy_pool API."""
        resp = _curl_requests.get(f"{self.api_url}{path}", params=params, timeout=_API_TIMEOUT)
        resp.raise_for_status()
        return resp.json()

    async def get_proxy(self) -> str | None:
        """Return one ``IP:PORT`` proxy string, or None when the pool is empty.

        Empty/error responses degrade the request to direct: this writes the
        state file and prints the locked warning (once per instance).
        """
        try:
            data = await asyncio.to_thread(self._api_get, "/get/", {"type": "https"})
        except Exception as exc:
            await self._note_empty(f"proxy_pool API unreachable: {exc}")
            return None
        if not isinstance(data, dict):
            await self._note_empty("proxy_pool returned a non-object response")
            return None
        proxy = data.get("proxy")
        if isinstance(proxy, str) and proxy.strip():
            return proxy.strip()
        await self._note_empty("https pool empty")
        return None

    async def delete_proxy(self, proxy: str) -> None:
        """Ask the pool to evict a bad proxy. API failures are logged only."""
        try:
            await asyncio.to_thread(self._api_get, "/delete/", {"proxy": proxy})
        except Exception as exc:
            print(f"[proxy] WARN: failed to delete bad proxy {proxy}: {exc}", file=sys.stderr)
            logger.warning("delete_proxy failed for %s: %s", proxy, exc)

    async def pool_count(self) -> int:
        """Return the current pool count, or 0 when the API cannot be read."""
        try:
            data = await asyncio.to_thread(self._api_get, "/count/")
        except Exception:
            return 0
        if isinstance(data, dict):
            for key in ("count", "total"):
                value = data.get(key)
                if isinstance(value, int):
                    return value
                if isinstance(value, str) and value.isdigit():
                    return int(value)
            return 0
        if isinstance(data, bool):
            return int(data)
        if isinstance(data, (int, float)):
            return int(data)
        if isinstance(data, str) and data.isdigit():
            return int(data)
        return 0

    def record_state(self, pool_count: int, degraded: bool, reason: str = "") -> None:
        """Atomically write the degradation/state file."""
        payload = {
            "timestamp": datetime.now().isoformat(timespec="seconds"),
            "pool_count": int(pool_count),
            "degraded": bool(degraded),
            "reason": reason,
        }
        self.state_path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.state_path.with_name(f"{self.state_path.name}.{os.getpid()}.tmp")
        tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        os.replace(tmp, self.state_path)

    @staticmethod
    def proxy_spec(proxy: str) -> dict[str, str]:
        """Build the curl/requests ``proxies`` mapping for a ``IP:PORT`` value."""
        return {"http": f"http://{proxy}", "https": f"http://{proxy}"}

    async def _note_empty(self, reason: str) -> None:
        """Record an empty-pool degradation: state file + one loud warning."""
        count = await self.pool_count()
        self.record_state(pool_count=count, degraded=True, reason=reason)
        if not self._warned_empty:
            print(
                "[proxy] WARN/ERROR: https pool empty, falling back to direct",
                file=sys.stderr,
            )
            self._warned_empty = True
