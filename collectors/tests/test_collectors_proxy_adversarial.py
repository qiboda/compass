"""Adversarial tests for issue #294 — collectors proxy_pool integration.

These tests attack the planned proxy layer beyond the happy path:
malformed API payloads, API outages, bad-proxy rotation exhaustion,
concurrent use, state-file atomicity, and keepalive resilience.

STATUS: RED — the interfaces are not implemented yet; collection fails with
ImportError/TypeError. They are expected to pass only after the implementation
lands.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

import pytest

import common
import fetch_freeproxy
import proxy_keepalive
import proxy_pool_client

# ── Test doubles ───────────────────────────────────────────────────────────


class FakeResponse:
    def __init__(
        self,
        *,
        json_data: dict[str, Any] | None = None,
        status_code: int = 200,
        content: bytes = b"",
        text: str = "",
    ) -> None:
        self._json = json_data if json_data is not None else {}
        self.status_code = status_code
        self._content = content
        self._text = text

    def raise_for_status(self) -> None:
        if self.status_code >= 400:
            raise RuntimeError(f"HTTP {self.status_code}")

    def json(self) -> dict[str, Any]:
        return self._json

    @property
    def content(self) -> bytes:
        return self._content

    @property
    def text(self) -> str:
        return self._text


class RecordingAsyncSession:
    def __init__(
        self,
        *,
        responses: dict[str, FakeResponse] | None = None,
        exc: Exception | None = None,
        default: FakeResponse | None = None,
    ) -> None:
        self.responses = responses or {}
        self.exc = exc
        self.default = default or FakeResponse()
        self.calls: list[tuple[str, dict[str, Any]]] = []

    async def get(self, url: str, **kwargs: Any) -> FakeResponse:
        self.calls.append((url, kwargs))
        if self.exc is not None and url not in self.responses:
            raise self.exc
        return self.responses.get(url, self.default)

    async def post(self, url: str, **kwargs: Any) -> FakeResponse:
        self.calls.append((url, kwargs))
        if self.exc is not None and url not in self.responses:
            raise self.exc
        return self.responses.get(url, self.default)


def _pool_with(api_get: Any, state_path: Path) -> proxy_pool_client.ProxyPool:
    pool = proxy_pool_client.ProxyPool(api_url="http://proxy.test:5010", state_path=state_path)
    pool._api_get = api_get  # type: ignore[method-assign]
    return pool


# ── Malformed /get/ payloads ──────────────────────────────────────────────


@pytest.mark.parametrize(
    "payload",
    [
        [],
        "no proxy",
        42,
        {"proxy": 123},
        {"proxy": ""},
        {"proxy": None},
        {"proxy": "   "},
    ],
)
async def test_get_proxy_malformed_payload_degrades(
    tmp_path: Path, payload: Any, capsys: pytest.CaptureFixture[str]
) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/count/":
            return {"count": 3}
        return payload

    state_path = tmp_path / "proxy_pool_state.json"
    pool = _pool_with(api_get, state_path)
    assert await pool.get_proxy() is None
    state = json.loads(state_path.read_text(encoding="utf-8"))
    assert state["degraded"] is True
    assert state["pool_count"] == 3
    assert "falling back to direct" in capsys.readouterr().err


async def test_get_proxy_empty_warning_printed_only_once_per_instance(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"code": 0}
        return {"count": 0}

    state_path = tmp_path / "proxy_pool_state.json"
    pool = _pool_with(api_get, state_path)
    await pool.get_proxy()
    await pool.get_proxy()
    err = capsys.readouterr().err
    assert err.count("[proxy] WARN/ERROR: https pool empty, falling back to direct") == 1


async def test_delete_proxy_passes_raw_proxy_string(tmp_path: Path) -> None:
    seen: list[tuple[str, dict[str, Any] | None]] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        seen.append((path, params))
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    await pool.delete_proxy("1.2.3.4:8080")
    assert ("/delete/", {"proxy": "1.2.3.4:8080"}) in seen


# ── Rotation exhaustion / edge semantics ───────────────────────────────────


async def test_proxy_get_raises_after_all_proxies_and_direct_fail(tmp_path: Path) -> None:
    state_path = tmp_path / "s.json"
    deleted: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"proxy": "bad:1"}
        if path == "/delete/":
            deleted.append(params["proxy"])
            return {}
        return {}

    pool = _pool_with(api_get, state_path)
    session = RecordingAsyncSession(exc=ConnectionError("always fails"))
    with pytest.raises(ConnectionError, match="always fails"):
        await common.proxy_get(session, pool, "https://example.com", max_proxy_attempts=3)
    assert len(deleted) == 3
    # 3 proxy attempts + 1 direct attempt = 4 calls; no infinite loop.
    assert len(session.calls) == 4


async def test_proxy_get_zero_attempts_means_direct_only(tmp_path: Path) -> None:
    get_calls: list[dict[str, Any] | None] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        get_calls.append(params)
        return {"proxy": "1.2.3.4:8080"}

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = RecordingAsyncSession()
    await common.proxy_get(session, pool, "https://example.com", max_proxy_attempts=0)
    assert get_calls == []  # never asked the pool
    assert session.calls[0][1].get("proxies") is None


async def test_proxy_get_pool_api_error_is_treated_as_empty_not_crash(tmp_path: Path) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        raise ConnectionError("pool api down")

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = RecordingAsyncSession()
    resp = await common.proxy_get(session, pool, "https://example.com")
    assert resp is session.default
    assert session.calls[0][1].get("proxies") is None


async def test_proxy_get_http_429_does_not_delete_proxy(tmp_path: Path) -> None:
    deleted: list[str] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"proxy": "1.2.3.4:8080"}
        if path == "/delete/":
            deleted.append(params["proxy"])
            return {}
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = RecordingAsyncSession(
        responses={"https://example.com": FakeResponse(status_code=429)}
    )
    resp = await common.proxy_get(session, pool, "https://example.com")
    assert resp.status_code == 429
    assert deleted == []


async def test_proxy_get_preserves_extra_kwargs(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingAsyncSession()
    await common.proxy_get(
        session,
        pool,
        "https://example.com",
        params={"a": 1},
        headers={"X-Test": "y"},
        timeout=12.5,
    )
    kwargs = session.calls[0][1]
    assert kwargs["params"] == {"a": 1}
    assert kwargs["headers"] == {"X-Test": "y"}
    assert kwargs["timeout"] == 12.5
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


async def test_proxy_get_overrides_caller_proxies_kwarg(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingAsyncSession()
    await common.proxy_get(
        session, pool, "https://example.com", proxies={"http": "http://evil:1"}
    )
    assert session.calls[0][1]["proxies"] == {
        "http": "http://1.2.3.4:8080",
        "https": "http://1.2.3.4:8080",
    }


# ── Concurrency ────────────────────────────────────────────────────────────


async def test_concurrent_proxy_get_each_gets_own_proxy(tmp_path: Path) -> None:
    state_path = tmp_path / "s.json"
    proxies = iter([{"proxy": "p1:1"}, {"proxy": "p2:2"}, {"proxy": "p3:3"}])

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return next(proxies)
        return {}

    pool = _pool_with(api_get, state_path)
    session = RecordingAsyncSession()

    async def one(url: str) -> None:
        await common.proxy_get(session, pool, url)

    await asyncio.gather(one("https://a"), one("https://b"), one("https://c"))
    used = [kwargs["proxies"]["http"] for _, kwargs in session.calls]
    assert sorted(used) == sorted(["http://p1:1", "http://p2:2", "http://p3:3"])


# ── State file robustness ──────────────────────────────────────────────────


def test_record_state_creates_parent_dirs_and_is_atomic(tmp_path: Path) -> None:
    state_path = tmp_path / "nested" / "proxy_pool_state.json"
    pool = proxy_pool_client.ProxyPool(api_url="http://x", state_path=state_path)
    pool.record_state(pool_count=2, degraded=True, reason="empty")
    assert state_path.exists()
    data = json.loads(state_path.read_text(encoding="utf-8"))
    assert data["pool_count"] == 2
    assert data["degraded"] is True
    assert "timestamp" in data


def test_record_state_many_writes_always_valid_json(tmp_path: Path) -> None:
    state_path = tmp_path / "proxy_pool_state.json"
    pool = proxy_pool_client.ProxyPool(api_url="http://x", state_path=state_path)
    for i in range(20):
        pool.record_state(pool_count=i, degraded=(i % 2 == 0), reason=f"r{i}")
        data = json.loads(state_path.read_text(encoding="utf-8"))
        assert data["pool_count"] == i


# ── fetch_freeproxy refactor adversarial ───────────────────────────────────


def test_records_from_json_data_handles_garbage_payloads() -> None:
    assert fetch_freeproxy.records_from_json_data(None, 10) == []
    assert fetch_freeproxy.records_from_json_data({}, 10) == []
    assert fetch_freeproxy.records_from_json_data({"data": "not-a-list"}, 10) == []
    assert fetch_freeproxy.records_from_json_data({"data": [None, 1, "x", {}]}, 10) == []


def test_records_from_json_data_rejects_injection_proxies() -> None:
    payload = {
        "data": [
            {"ip": "1.2.3.4\r\nX-Injected: 1", "port": 8080, "protocol": "Http"},
            {"ip": "1.2.3.4", "port": "8080\r\n", "protocol": "Http"},
            {"ip": "1.2.3.4@evil", "port": 8080, "protocol": "Http"},
            {"ip": "127.0.0.1", "port": 8080, "protocol": "Http"},
        ]
    }
    records = fetch_freeproxy.records_from_json_data(payload, limit=10)
    assert records == []


def test_records_from_json_data_respects_limit() -> None:
    payload = {
        "data": [
            {"ip": f"1.1.1.{i}", "port": 8080, "protocol": "Http, Https"}
            for i in range(1, 6)
        ]
    }
    records = fetch_freeproxy.records_from_json_data(payload, limit=2)
    assert len(records) == 2


# ── keepalive resilience ──────────────────────────────────────────────────


def test_keepalive_realtime_failure_does_not_crash(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    written: list[str] = []

    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        written.append(table)
        return 0

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: {"data": []})
    monkeypatch.setattr(
        fetch_freeproxy,
        "records_from_json_data",
        lambda payload, limit: [{"proxy": "1.1.1.1:80"}],
    )
    monkeypatch.setattr(
        fetch_freeproxy,
        "fetch_realtime_proxies",
        lambda limit, sources: (_ for _ in ()).throw(RuntimeError("realtime source down")),
    )
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(tmp_path / "f.json"), "--realtime-sources", "BrokenSource"]
    )
    assert rc == 0
    assert written == ["use_proxy"]


def test_keepalive_malformed_snapshot_continues_with_realtime(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    snapshot = tmp_path / "freeproxy.json"
    snapshot.write_text("{not valid json", encoding="utf-8")

    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        return 0

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: (_ for _ in ()).throw(ConnectionError("429")))
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [{"proxy": "2.2.2.2:80"}])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(snapshot), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0


def test_keepalive_cycle_functions_independently(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """A failure in one source must not prevent the other source from seeding."""
    calls: list[str] = []

    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        calls.append(table)
        return len(records)

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: (_ for _ in ()).throw(ConnectionError("json down")))
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [{"proxy": "2.2.2.2:80"}])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(tmp_path / "missing.json"), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0
    # JSON source had no snapshot → no json write; realtime still seeded.
    assert calls == ["use_proxy"]
