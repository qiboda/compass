"""Requirement-acceptance tests for issue #294 — collectors proxy_pool integration.

Contract under test (approved plan `.dsh/plans/collectors-proxy.md`):

  1. `collectors/proxy_pool_client.py` exposes `ProxyPool` with
     `get_proxy` / `delete_proxy` / `pool_count` / `record_state` /
     `proxy_spec`, plus `proxy_enabled` / `default_state_path`.
  2. `collectors/common.py` exposes `make_proxy_pool`, `proxy_get`,
     `proxy_post`, `proxy_get_sync`, `proxy_post_sync`, and
     `fetch_paginated(..., *, pool=None)`.
  3. Collectors use proxy-first: a usable https proxy is passed as
     per-request `proxies`; empty/unreachable pool falls back to direct
     with a visible warning + `proxy_pool_state.json`.
  4. A request exception through a proxy deletes that proxy and rotates to
     the next one; after the bounded attempts the request goes direct.
  5. `fetch_index_daily` keeps Tencent fallback; `fetch_stock_basic_official`
     uses sync proxy wrappers.
  6. `collectors/proxy_keepalive.py` runs dual-source cycles with snapshot
     fallback and `--once`.

STATUS: RED — these interfaces are not implemented yet; the file fails at
collection with ImportError/TypeError, which is the expected pre-implementation
state.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest

import common
import fetch_freeproxy
import fetch_index_daily
import fetch_stock_basic_official
import proxy_keepalive
import proxy_pool_client

# ── Test doubles ───────────────────────────────────────────────────────────


class FakeResponse:
    """Minimal curl_cffi-style response used by RecordingAsyncSession."""

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
    """Async session that records every call's kwargs (including proxies).

    ``exc_sequence`` is consumed per call; a non-None entry raises before
    returning a response, which lets tests model "first proxy fails, next
    succeeds".
    """

    def __init__(
        self,
        *,
        responses: dict[str, FakeResponse] | None = None,
        exc: Exception | None = None,
        exc_sequence: list[Exception | None] | None = None,
        default: FakeResponse | None = None,
    ) -> None:
        self.responses = responses or {}
        self.exc = exc
        self.exc_sequence = list(exc_sequence) if exc_sequence is not None else None
        self.default = default or FakeResponse()
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def _maybe_raise(self, url: str) -> None:
        if self.exc_sequence is not None:
            if self.exc_sequence:
                err = self.exc_sequence.pop(0)
                if err is not None:
                    raise err
            return
        if self.exc is not None and url not in self.responses:
            raise self.exc

    async def get(self, url: str, **kwargs: Any) -> FakeResponse:
        self.calls.append((url, kwargs))
        self._maybe_raise(url)
        return self.responses.get(url, self.default)

    async def post(self, url: str, **kwargs: Any) -> FakeResponse:
        self.calls.append((url, kwargs))
        self._maybe_raise(url)
        return self.responses.get(url, self.default)


class SyncFakeResponse:
    """Minimal requests-style response for RecordingSyncSession."""

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


class RecordingSyncSession:
    """Sync session that records every call's kwargs (including proxies).

    ``responses`` values may be a single response or a list of responses
    consumed in order per URL (used to terminate pagination loops).
    """

    def __init__(
        self, responses: dict[str, SyncFakeResponse | list[SyncFakeResponse]] | None = None
    ) -> None:
        self.responses = responses or {}
        self.calls: list[tuple[str, str, dict[str, Any]]] = []
        self._remaining: dict[str, list[SyncFakeResponse]] = {}
        for url, value in self.responses.items():
            if isinstance(value, list):
                self._remaining[url] = list(value)

    def _record(self, method: str, url: str, kwargs: dict[str, Any]) -> SyncFakeResponse:
        self.calls.append((method, url, kwargs))
        if url in self._remaining:
            seq = self._remaining[url]
            if seq:
                return seq.pop(0)
            return SyncFakeResponse()
        value = self.responses.get(url)
        if isinstance(value, SyncFakeResponse):
            return value
        return SyncFakeResponse()

    def get(self, url: str, **kwargs: Any) -> SyncFakeResponse:
        return self._record("GET", url, kwargs)

    def post(self, url: str, **kwargs: Any) -> SyncFakeResponse:
        return self._record("POST", url, kwargs)


def _pool_with(api_get: Any, state_path: Path) -> proxy_pool_client.ProxyPool:
    """Build a ProxyPool whose internal HTTP hook is replaced by ``api_get``."""
    pool = proxy_pool_client.ProxyPool(api_url="http://proxy.test:5010", state_path=state_path)
    pool._api_get = api_get  # type: ignore[method-assign]
    return pool


# ── proxy_pool_client: happy path + basic errors ──────────────────────────


async def test_get_proxy_returns_https_proxy(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        assert path == "/get/"
        assert params == {"type": "https"}
        return {"proxy": "1.2.3.4:8080", "https": True, "source": "freeproxy"}

    pool = _pool_with(api_get, tmp_path / "state.json")
    proxy = await pool.get_proxy()
    assert proxy == "1.2.3.4:8080"
    assert not (tmp_path / "state.json").exists()
    assert "https pool empty" not in capsys.readouterr().err


async def test_get_proxy_empty_pool_writes_state_and_warns(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"code": 0, "src": "no proxy"}
        if path == "/count/":
            return {"count": 0}
        raise AssertionError(f"unexpected path {path}")

    state_path = tmp_path / "proxy_pool_state.json"
    pool = _pool_with(api_get, state_path)
    assert await pool.get_proxy() is None
    assert "[proxy] WARN/ERROR: https pool empty, falling back to direct" in capsys.readouterr().err
    state = json.loads(state_path.read_text(encoding="utf-8"))
    assert state["degraded"] is True
    assert state["pool_count"] == 0
    assert state["timestamp"]


async def test_get_proxy_api_unreachable_falls_back_to_none(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        raise ConnectionError("boom")

    state_path = tmp_path / "proxy_pool_state.json"
    pool = _pool_with(api_get, state_path)
    assert await pool.get_proxy() is None
    assert "falling back to direct" in capsys.readouterr().err
    assert json.loads(state_path.read_text(encoding="utf-8"))["degraded"] is True


async def test_delete_proxy_calls_delete_and_swallows_api_failure(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    calls: list[tuple[str, dict[str, Any] | None]] = []

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        calls.append((path, params))
        if path == "/delete/":
            raise ConnectionError("delete failed")
        return {}

    pool = _pool_with(api_get, tmp_path / "state.json")
    await pool.delete_proxy("9.9.9.9:3128")  # must not raise
    assert any(path == "/delete/" and params == {"proxy": "9.9.9.9:3128"} for path, params in calls)
    assert "delete failed" in capsys.readouterr().err


async def test_pool_count_defensive_parsing(tmp_path: Path) -> None:
    pool = proxy_pool_client.ProxyPool(api_url="http://x", state_path=tmp_path / "s.json")

    pool._api_get = lambda path, params=None: {"count": 5}  # type: ignore[method-assign]
    assert await pool.pool_count() == 5

    pool._api_get = lambda path, params=None: 7  # type: ignore[method-assign]
    assert await pool.pool_count() == 7

    pool._api_get = lambda path, params=None: "12"  # type: ignore[method-assign]
    assert await pool.pool_count() == 12

    pool._api_get = lambda path, params=None: (_ for _ in ()).throw(ConnectionError())  # type: ignore[method-assign]
    assert await pool.pool_count() == 0


def test_proxy_spec_normalizes_http_scheme(tmp_path: Path) -> None:
    pool = proxy_pool_client.ProxyPool(api_url="http://x", state_path=tmp_path / "s.json")
    assert pool.proxy_spec("1.2.3.4:8080") == {
        "http": "http://1.2.3.4:8080",
        "https": "http://1.2.3.4:8080",
    }


def test_proxy_enabled_respects_disable_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("COMPASS_PROXY_DISABLE", raising=False)
    assert proxy_pool_client.proxy_enabled() is True
    monkeypatch.setenv("COMPASS_PROXY_DISABLE", "1")
    assert proxy_pool_client.proxy_enabled() is False


def test_make_proxy_pool_returns_none_when_disabled(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("COMPASS_PROXY_DISABLE", "1")
    assert common.make_proxy_pool() is None


def test_make_proxy_pool_returns_pool_with_csv_state_path(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.delenv("COMPASS_PROXY_DISABLE", raising=False)
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    pool = common.make_proxy_pool()
    assert pool is not None
    assert pool.state_path == tmp_path / "proxy_pool_state.json"


# ── common.proxy_get / proxy_post: happy path + basic errors ──────────────


async def test_proxy_get_without_pool_passes_no_proxies() -> None:
    session = RecordingAsyncSession()
    resp = await common.proxy_get(session, None, "https://example.com", params={"a": "1"})
    assert resp is session.default
    url, kwargs = session.calls[-1]
    assert url == "https://example.com"
    assert "proxies" not in kwargs
    assert kwargs["params"] == {"a": "1"}


async def test_proxy_get_with_proxy_passes_proxies(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingAsyncSession()
    await common.proxy_get(session, pool, "https://example.com", params={"a": "1"})
    url, kwargs = session.calls[-1]
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}
    assert kwargs["params"] == {"a": "1"}


async def test_proxy_get_empty_pool_falls_back_direct(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"code": 0, "src": "no proxy"}
        if path == "/count/":
            return {"count": 0}
        return {}

    state_path = tmp_path / "proxy_pool_state.json"
    pool = _pool_with(api_get, state_path)
    session = RecordingAsyncSession()
    resp = await common.proxy_get(session, pool, "https://example.com")
    assert resp is session.default
    url, kwargs = session.calls[-1]
    assert "proxies" not in kwargs
    assert "[proxy] WARN/ERROR: https pool empty, falling back to direct" in capsys.readouterr().err
    assert json.loads(state_path.read_text(encoding="utf-8"))["degraded"] is True


async def test_proxy_get_deletes_bad_proxy_and_uses_next(tmp_path: Path) -> None:
    state_path = tmp_path / "s.json"
    deleted: list[str] = []
    proxy_responses = iter([{"proxy": "bad:1"}, {"proxy": "good:2"}])

    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return next(proxy_responses)
        if path == "/delete/":
            deleted.append(params["proxy"])
            return {}
        return {}

    pool = _pool_with(api_get, state_path)
    session = RecordingAsyncSession(
        responses={
            "https://example.com": FakeResponse(json_data={"ok": True}),
        },
        exc_sequence=[ConnectionError("bad proxy"), None],
    )
    # First call raises (bad:1), second call succeeds (good:2).
    resp = await common.proxy_get(session, pool, "https://example.com")
    assert resp.json() == {"ok": True}
    assert deleted == ["bad:1"]
    proxies_used = [kwargs.get("proxies") for _, kwargs in session.calls]
    assert proxies_used[0] == {"http": "http://bad:1", "https": "http://bad:1"}
    assert proxies_used[1] == {"http": "http://good:2", "https": "http://good:2"}


async def test_proxy_get_all_proxies_fail_then_direct_succeeds(tmp_path: Path) -> None:
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
    session = RecordingAsyncSession(
        responses={"https://example.com": FakeResponse(json_data={"ok": True})},
        exc_sequence=[
            ConnectionError("bad proxy"),
            ConnectionError("bad proxy"),
            None,
        ],
    )
    resp = await common.proxy_get(
        session, pool, "https://example.com", max_proxy_attempts=2
    )
    assert resp.json() == {"ok": True}
    assert deleted == ["bad:1", "bad:1"]
    # Two proxy attempts raise, final direct attempt succeeds without proxies.
    assert session.calls[-1][1].get("proxies") is None


async def test_proxy_get_direct_also_fails_raises(tmp_path: Path) -> None:
    def api_get(path: str, params: dict[str, Any] | None = None) -> Any:
        if path == "/get/":
            return {"code": 0, "src": "no proxy"}
        return {}

    pool = _pool_with(api_get, tmp_path / "s.json")
    session = RecordingAsyncSession(exc=ConnectionError("direct failed"))
    with pytest.raises(ConnectionError, match="direct failed"):
        await common.proxy_get(session, pool, "https://example.com")


async def test_proxy_post_uses_proxies(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingAsyncSession()
    await common.proxy_post(session, pool, "https://example.com", data={"x": "y"})
    url, kwargs = session.calls[-1]
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}
    assert kwargs["data"] == {"x": "y"}


def test_proxy_get_sync_passes_proxies(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingSyncSession()
    common.proxy_get_sync(session, pool, "https://example.com", params={"a": "1"})
    method, url, kwargs = session.calls[-1]
    assert method == "GET"
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


def test_proxy_post_sync_passes_proxies(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingSyncSession()
    common.proxy_post_sync(session, pool, "https://example.com", data={"x": "y"})
    method, url, kwargs = session.calls[-1]
    assert method == "POST"
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


# ── fetch_paginated integration ───────────────────────────────────────────


async def test_fetch_paginated_uses_proxy_pool(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingAsyncSession(
        responses={
            common.EM_BASE: FakeResponse(
                json_data={"success": True, "result": {"data": [], "pages": 1}}
            )
        }
    )
    throttle = common.Throttle(min_interval=0)
    records = await common.fetch_paginated(
        session,
        throttle,
        "RPT_TEST",
        "REPORT_DATE",
        "2024-12-31",
        page_size=100,
        pool=pool,
    )
    assert records == []
    url, kwargs = session.calls[0]
    assert url == common.EM_BASE
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


# ── fetch_index_daily integration (Tencent fallback preserved) ────────────


async def test_index_daily_get_json_uses_proxy_pool(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingAsyncSession(
        responses={
            "https://push2his.eastmoney.com/api/qt/stock/kline/get": FakeResponse(
                json_data={"data": {"code": "000001", "klines": ["2024-01-01,1,2,3,4,5,6"]}}
            )
        }
    )
    throttle = common.Throttle(min_interval=0)
    data = await fetch_index_daily._get_json(
        session,
        throttle,
        ("https://push2his.eastmoney.com/api/qt/stock/kline/get",),
        {"secid": "1.000001"},
        pool=pool,
    )
    assert data is not None
    url, kwargs = session.calls[0]
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


async def test_index_daily_ths_list_uses_proxy_pool(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    html = (
        '<html><body><a href="/thshy/detail/code/881101/">银行</a>'
        '<a href="/thshy/detail/code/881121/">证券</a></body></html>'
    ).encode("gbk")
    session = RecordingAsyncSession(
        responses={
            fetch_index_daily.THS_LIST_URL: FakeResponse(content=html),
        }
    )
    throttle = common.Throttle(min_interval=0)
    boards = await fetch_index_daily.fetch_ths_industry_list(session, throttle, pool=pool)
    assert boards == [("881101", "银行"), ("881121", "证券")]
    url, kwargs = session.calls[0]
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


# ── fetch_stock_basic_official integration (sync) ─────────────────────────


def test_stock_basic_official_fetch_sse_uses_proxy_pool(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    session = RecordingSyncSession(
        responses={
            fetch_stock_basic_official.SSE_URL: SyncFakeResponse(
                json_data={"pageHelp": {"data": [{"PRODUCTCODE": "600000"}]}}
            )
        }
    )
    data = fetch_stock_basic_official.fetch_sse(session, pool=pool)
    assert data["pageHelp"]["data"]
    method, url, kwargs = session.calls[0]
    assert method == "GET"
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


def test_stock_basic_official_fetch_bse_uses_proxy_post(tmp_path: Path) -> None:
    pool = _pool_with(lambda path, params=None: {"proxy": "1.2.3.4:8080"}, tmp_path / "s.json")
    first_body = 'null([{"content": [{"xxzqdm": "830001"}], "totalPage": 1}])'
    second_body = 'null([{"content": [], "totalPage": 1}])'
    session = RecordingSyncSession(
        responses={
            fetch_stock_basic_official.BSE_LISTED_URL: SyncFakeResponse(text="ok"),
            fetch_stock_basic_official.BSE_API_URL: [
                SyncFakeResponse(text=first_body),
                SyncFakeResponse(text=second_body),
            ],
        }
    )
    rows = fetch_stock_basic_official.fetch_bse(session, pool=pool)
    assert rows == [{"xxzqdm": "830001"}]
    post_calls = [c for c in session.calls if c[0] == "POST"]
    assert post_calls
    method, url, kwargs = post_calls[0]
    assert kwargs["proxies"] == {"http": "http://1.2.3.4:8080", "https": "http://1.2.3.4:8080"}


# ── fetch_freeproxy refactor: snapshot support ────────────────────────────


def test_records_from_json_data_filters_to_records() -> None:
    payload = {
        "data": [
            {"ip": "1.2.3.4", "port": 8080, "protocol": "Http, Https", "country": "CN"},
            {"ip": "10.0.0.1", "port": 8080, "protocol": "Http"},  # private IP → dropped
            {"ip": "5.6.7.8", "port": 99999, "protocol": "Http"},  # invalid port → dropped
            "junk",
        ]
    }
    records = fetch_freeproxy.records_from_json_data(payload, limit=10)
    assert len(records) == 1
    assert records[0]["proxy"] == "1.2.3.4:8080"


def test_fetch_json_proxies_composes_payload_and_records(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = {"data": [{"ip": "1.2.3.4", "port": 8080, "protocol": "Http, Https"}]}
    monkeypatch.setattr(
        fetch_freeproxy, "fetch_json_payload", lambda url: payload
    )
    records = fetch_freeproxy.fetch_json_proxies("http://raw.example/proxies.json", limit=5)
    assert records[0]["proxy"] == "1.2.3.4:8080"


# ── proxy_keepalive: dual source + snapshot fallback + --once ─────────────


def test_keepalive_once_seeds_json_and_realtime(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    written: list[str] = []

    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        written.append(table)
        return len(records)

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: {"data": []})
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [{"proxy": "1.1.1.1:80"}])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [{"proxy": "2.2.2.2:80"}])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        [
            "--once",
            "--snapshot",
            str(tmp_path / "freeproxy.json"),
            "--redis-url",
            "redis://127.0.0.1:6379/0",
            "--table",
            "use_proxy",
        ]
    )
    assert rc == 0
    assert written == ["use_proxy", "use_proxy"]


def test_keepalive_json_failure_uses_snapshot(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    snapshot = tmp_path / "freeproxy.json"
    snapshot.write_text(json.dumps({"data": [{"ip": "3.3.3.3", "port": 8080, "protocol": "Http, Https"}]}), encoding="utf-8")
    written: list[list[dict[str, Any]]] = []

    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        written.append(records)
        return len(records)

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: (_ for _ in ()).throw(ConnectionError("raw 429")))
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", fetch_freeproxy.records_from_json_data)
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(snapshot), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0
    assert written and written[0][0]["proxy"] == "3.3.3.3:8080"


def test_keepalive_json_failure_without_snapshot_continues(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    def fake_write(redis_url: str, table: str, records: list[dict[str, Any]]) -> int:
        return len(records)

    monkeypatch.setattr(fetch_freeproxy, "fetch_json_payload", lambda url: (_ for _ in ()).throw(ConnectionError("raw 429")))
    monkeypatch.setattr(fetch_freeproxy, "records_from_json_data", lambda payload, limit: [])
    monkeypatch.setattr(fetch_freeproxy, "fetch_realtime_proxies", lambda limit, sources: [])
    monkeypatch.setattr(fetch_freeproxy, "write_to_redis", fake_write)

    rc = proxy_keepalive.main(
        ["--once", "--snapshot", str(tmp_path / "missing.json"), "--json-url", "http://raw.example/proxies.json"]
    )
    assert rc == 0
