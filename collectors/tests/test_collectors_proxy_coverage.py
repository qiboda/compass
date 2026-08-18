"""Coverage-completion tests for issue #294 proxy layer.

These tests target branches that the RED suites did not exercise directly
(real ``_api_get`` hook, sync-loop fallbacks, keepalive fatal guards,
freeproxy safety helpers) so the Python coverage gate (>=95%) stays green.
"""

from __future__ import annotations

import sys
import types
from pathlib import Path
from typing import Any

import pytest

import common
import fetch_freeproxy
import proxy_keepalive
import proxy_pool_client

# ── Small fakes ─────────────────────────────────────────────────────────────


class _FakeCurlResponse:
    def __init__(self, payload: Any) -> None:
        self._payload = payload

    def raise_for_status(self) -> None:
        pass

    def json(self) -> Any:
        return self._payload


class _FakeAsyncSession:
    """Async session that can raise on selected calls (sequence-based)."""

    def __init__(
        self,
        *,
        exc_sequence: list[Exception | None] | None = None,
        response: Any = None,
    ) -> None:
        self.exc_sequence = list(exc_sequence) if exc_sequence is not None else None
        self.response = response
        self.calls: list[tuple[str, dict[str, Any]]] = []

    async def get(self, url: str, **kwargs: Any) -> Any:
        self.calls.append((url, kwargs))
        if self.exc_sequence is not None:
            err = self.exc_sequence.pop(0) if self.exc_sequence else None
            if err is not None:
                raise err
        return self.response

    async def post(self, url: str, **kwargs: Any) -> Any:
        return await self.get(url, **kwargs)


class _FakeSyncSession:
    """Sync session with sequence-based failure control."""

    def __init__(self, exc_sequence: list[Exception | None] | None = None) -> None:
        self.exc_sequence = list(exc_sequence) if exc_sequence is not None else None
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def get(self, url: str, **kwargs: Any) -> Any:
        self.calls.append((url, kwargs))
        if self.exc_sequence is not None:
            err = self.exc_sequence.pop(0) if self.exc_sequence else None
            if err is not None:
                raise err
        return object()

    def post(self, url: str, **kwargs: Any) -> Any:
        return self.get(url, **kwargs)


def _pool_with(api_get: Any, state_path: Path) -> proxy_pool_client.ProxyPool:
    pool = proxy_pool_client.ProxyPool(api_url="http://proxy.test:5010", state_path=state_path)
    pool._api_get = api_get  # type: ignore[method-assign]
    return pool


# ── proxy_pool_client coverage ──────────────────────────────────────────────


def test_default_state_path_uses_env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    assert proxy_pool_client.default_state_path() == tmp_path / "proxy_pool_state.json"


def test_api_get_real_hook(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    seen: list[tuple[str, dict[str, Any] | None]] = []

    def fake_get(url: str, params: dict[str, Any] | None = None, **kwargs: Any) -> _FakeCurlResponse:
        seen.append((url, params))
        return _FakeCurlResponse({"ok": True})

    monkeypatch.setattr(proxy_pool_client._curl_requests, "get", fake_get)
    pool = proxy_pool_client.ProxyPool(api_url="http://proxy.test:5010", state_path=tmp_path / "s.json")
    assert pool._api_get("/get/", {"type": "https"}) == {"ok": True}
    assert seen[0][0] == "http://proxy.test:5010/get/"
    assert seen[0][1] == {"type": "https"}


async def test_pool_count_additional_shapes(tmp_path: Path) -> None:
    pool = proxy_pool_client.ProxyPool(api_url="http://x", state_path=tmp_path / "s.json")

    pool._api_get = lambda path, params=None: {"count": "9"}  # type: ignore[method-assign]
    assert await pool.pool_count() == 9

    pool._api_get = lambda path, params=None: True  # type: ignore[method-assign]
    assert await pool.pool_count() == 1

    pool._api_get = lambda path, params=None: {}  # type: ignore[method-assign]
    assert await pool.pool_count() == 0

    pool._api_get = lambda path, params=None: "not-a-number"  # type: ignore[method-assign]
    assert await pool.pool_count() == 0


# ── common proxy wrappers coverage ──────────────────────────────────────────


async def test_proxy_post_without_pool_direct(tmp_path: Path) -> None:
    session = _FakeAsyncSession(response=object())
    resp = await common.proxy_post(session, None, "https://example.com", data={"x": 1})
    assert resp is not None
    url, kwargs = session.calls[0]
    assert "proxies" not in kwargs
    assert kwargs["data"] == {"x": 1}


async def test_proxy_post_bad_proxy_deletes_and_retries(tmp_path: Path) -> None:
    deleted: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"proxy": "bad:1"}
        if path == "/delete/":
            deleted.append(params["proxy"])
            return {}
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = _FakeAsyncSession(
        exc_sequence=[ConnectionError("bad"), None],
        response=object(),
    )
    await common.proxy_post(session, pool, "https://example.com", data={"x": 1})
    assert deleted == ["bad:1"]
    assert session.calls[0][1]["proxies"]["http"] == "http://bad:1"
    assert session.calls[1][1]["proxies"]["http"] == "http://bad:1"


def test_proxy_get_sync_bad_proxy_deletes_and_retries(tmp_path: Path) -> None:
    deleted: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"proxy": "bad:1"}
        if path == "/delete/":
            deleted.append(params["proxy"])
            return {}
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = _FakeSyncSession(exc_sequence=[ConnectionError("bad"), None])
    common.proxy_get_sync(session, pool, "https://example.com")
    assert deleted == ["bad:1"]
    assert session.calls[0][1]["proxies"]["http"] == "http://bad:1"
    assert session.calls[1][1]["proxies"]["http"] == "http://bad:1"


def test_proxy_post_sync_bad_proxy_deletes_and_retries(tmp_path: Path) -> None:
    deleted: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"proxy": "bad:1"}
        if path == "/delete/":
            deleted.append(params["proxy"])
            return {}
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = _FakeSyncSession(exc_sequence=[ConnectionError("bad"), None])
    common.proxy_post_sync(session, pool, "https://example.com")
    assert deleted == ["bad:1"]
    assert session.calls[0][1]["proxies"]["http"] == "http://bad:1"
    assert session.calls[1][1]["proxies"]["http"] == "http://bad:1"


async def test_sync_get_proxy_inside_running_loop_uses_fallback(tmp_path: Path) -> None:
    """Running-loop path: _sync_get_proxy must use _sync_get_proxy_fallback."""
    seen: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        seen.append(path)
        if path == "/get/":
            return {"proxy": "1.2.3.4:8080"}
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    # Inside an async test there is a running loop, so the fallback path runs.
    assert common._sync_get_proxy(pool) == "1.2.3.4:8080"
    assert "/get/" in seen


async def test_sync_get_proxy_fallback_error_returns_none(tmp_path: Path) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        raise ConnectionError("down")

    pool = _pool_with(api_get, tmp_path / "s.json")
    assert common._sync_get_proxy_fallback(pool) is None


async def test_sync_delete_proxy_inside_running_loop_uses_fallback(tmp_path: Path) -> None:
    seen: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        seen.append(path)
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    common._sync_delete_proxy(pool, "1.2.3.4:8080")
    assert "/delete/" in seen


async def test_fetch_paginated_no_data_breaks(tmp_path: Path) -> None:
    class _NoneJsonResponse:
        status_code = 200

        def raise_for_status(self) -> None:
            pass

        def json(self) -> None:
            return None

    session = _FakeAsyncSession(response=_NoneJsonResponse())
    throttle = common.Throttle(min_interval=0)
    records = await common.fetch_paginated(
        session, throttle, "RPT_TEST", "REPORT_DATE", "2024-12-31", page_size=100
    )
    assert records == []


def test_dedupe_csv_empty_or_missing_file_returns(tmp_path: Path) -> None:
    missing = tmp_path / "missing.csv"
    common.dedupe_csv(missing)  # must not raise
    empty = tmp_path / "empty.csv"
    empty.write_text("", encoding="utf-8")
    common.dedupe_csv(empty)  # must not raise


def test_dedupe_csv_skips_malformed_row(tmp_path: Path) -> None:
    path = tmp_path / "rows.csv"
    path.write_text(
        "SECURITY_CODE,REPORTDATE\n"
        "short\n"
        "000001,2024-12-31\n",
        encoding="utf-8",
    )
    common.dedupe_csv(path)  # must not raise on the malformed row
    assert "000001,2024-12-31" in path.read_text(encoding="utf-8")


# ── proxy_keepalive coverage ────────────────────────────────────────────────


def test_keepalive_json_empty_records_no_write(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    written: list[str] = []

    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        written.append(table)
        return 0

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: {"data": []})
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(tmp_path / "f.json"), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0
    assert written == []


def test_keepalive_json_cycle_error_caught(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        raise ConnectionError("redis down")

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: {"data": []})
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [{"proxy": "1.1.1.1:80"}])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(tmp_path / "f.json"), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0


def test_keepalive_realtime_cycle_error_caught(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        raise ConnectionError("redis down")

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: {"data": []})
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [{"proxy": "2.2.2.2:80"}])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(tmp_path / "f.json"), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0


def test_keepalive_negative_limit_fatal() -> None:
    assert proxy_keepalive.main(["--once", "--limit", "-1"]) == 1


def test_keepalive_interval_zero_fatal() -> None:
    assert proxy_keepalive.main(["--interval", "0"]) == 2


def test_keepalive_loop_sleeps(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    def fake_run_cycle(*args: Any, **kwargs: Any) -> tuple[int, int]:
        return 0, 0

    def fake_sleep(seconds: float) -> None:
        raise SystemExit("slept")

    monkeypatch.setattr(proxy_keepalive, "run_cycle", fake_run_cycle)
    monkeypatch.setattr(proxy_keepalive.time, "sleep", fake_sleep)
    with pytest.raises(SystemExit, match="slept"):
        proxy_keepalive.main(["--interval", "1"])


# ── fetch_freeproxy safety helpers coverage ────────────────────────────────


def test_safe_proxy_rejects_bad_host_and_port() -> None:
    assert fetch_freeproxy._safe_proxy(None, 80) is None
    assert fetch_freeproxy._safe_proxy("", 80) is None
    assert fetch_freeproxy._safe_proxy("10.0.0.1", 80) is None  # private
    assert fetch_freeproxy._safe_proxy("1.2.3.4\r\n", 80) is None  # control char
    assert fetch_freeproxy._safe_proxy("1.2.3.4@evil", 80) is None  # @
    assert fetch_freeproxy._safe_proxy("1.2.3.4/24", 80) is None  # /
    assert fetch_freeproxy._safe_proxy("1.2.3.4", "abc") is None  # bad port
    assert fetch_freeproxy._safe_proxy("1.2.3.4", 70000) is None  # out of range
    assert fetch_freeproxy._safe_proxy("1.2.3.4", 8080) == "1.2.3.4:8080"


def test_safe_proxy_str_rejects_bad_values() -> None:
    assert fetch_freeproxy._safe_proxy_str("") is None
    assert fetch_freeproxy._safe_proxy_str("1.2.3.4\r\n:80") is None
    assert fetch_freeproxy._safe_proxy_str("1.2.3.4@evil:80") is None
    assert fetch_freeproxy._safe_proxy_str("1.2.3.4") is None  # no port
    assert fetch_freeproxy._safe_proxy_str("1.2.3.4:abc") is None
    assert fetch_freeproxy._safe_proxy_str("1.2.3.4:70000") is None
    assert fetch_freeproxy._safe_proxy_str("1.2.3.4:8080") == "1.2.3.4:8080"


def test_normalize_proxy_info_invalid_raises() -> None:
    info = types.SimpleNamespace(proxy="no-port")
    with pytest.raises(ValueError, match="invalid proxy"):
        fetch_freeproxy.normalize_proxy_info(info)


def test_fetch_realtime_proxies_source_failure_and_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake_module = types.ModuleType("freeproxy.modules")

    class _FakeBuild:
        def __init__(self, cfg: dict[str, Any]) -> None:
            self.cfg = cfg

        def refreshproxies(self) -> list[Any]:
            if self.cfg.get("type") == "Broken":
                raise RuntimeError("source down")
            return [
                types.SimpleNamespace(proxy="bad-no-port"),
                types.SimpleNamespace(proxy="1.2.3.4:8080"),
            ]

    fake_module.BuildProxiedSession = _FakeBuild
    monkeypatch.setitem(sys.modules, "freeproxy.modules", fake_module)

    # First source fails → warning + continue; second source yields one valid
    # record and the limit=1 early-return fires.
    records = fetch_freeproxy.fetch_realtime_proxies(1, ["Broken", "Good"])
    assert [r["proxy"] for r in records] == ["1.2.3.4:8080"]


def test_fetch_freeproxy_main_fatal_on_fetch_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def boom(url: str, limit: int) -> list[dict[str, Any]]:
        raise ConnectionError("github 429")

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_proxies", boom)
    assert fetch_freeproxy.main(["--source", "json"]) == 1


def test_fetch_freeproxy_main_fatal_on_write_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def fake_records(url: str, limit: int) -> list[dict[str, Any]]:
        return [{"proxy": "1.2.3.4:8080"}]

    def boom(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        raise ConnectionError("redis down")

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_proxies", fake_records)
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", boom)
    assert fetch_freeproxy.main(["--source", "json"]) == 1
