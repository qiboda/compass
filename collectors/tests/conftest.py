"""Pytest configuration — add collectors dir to Python path, plus stub HTTP fixtures."""
from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


@pytest.fixture(autouse=True)
def _isolate_csv_dir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Point COMPASS_CSV_DIR at a per-test temp dir.

    Collectors now default their raw-CSV output to csv_dir()
    (/data/compass-data/csv in production). Without this isolation every
    run()/main() test would write into the real data directory; tests that
    explicitly set COMPASS_CSV_DIR (e.g. the #208 contract tests) override
    this value with their own monkeypatch.setenv.
    """
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))


class StubResponse:
    """Fake curl-cffi Response for unit-testing collector call-sites.

    Call-sites use ``resp.status_code`` (int), ``resp.raise_for_status()``
    (sync, raises on >= 400 or injected exception), and ``resp.json()``
    (sync, returns canned dict).
    """

    __slots__ = ("status_code", "_json", "_exc")

    def __init__(
        self,
        *,
        status_code: int = 200,
        json_data: dict[str, Any] | None = None,
        exc: Exception | None = None,
    ) -> None:
        self.status_code = status_code
        self._json = json_data
        self._exc = exc

    def raise_for_status(self) -> None:
        if self._exc is not None:
            raise self._exc
        if self.status_code >= 400:
            raise Exception(f"HTTP {self.status_code}")

    def json(self) -> dict[str, Any]:
        return self._json if self._json is not None else {}


class StubSession:
    """Async context-manager stub for ``curl_cffi.requests.AsyncSession``.

    Supports ``async with``, ``await stub.get(url, params=, headers=)``,
    and per-test canned-response injection via ``canned_responses`` or
    per-URL overrides in ``canned_responses`` dict.
    """

    def __init__(
        self,
        *,
        canned_responses: dict[str, StubResponse | dict[str, Any]] | None = None,
        status_code: int = 200,
        json_data: dict[str, Any] | None = None,
        exc: Exception | None = None,
    ) -> None:
        self._canned = canned_responses or {}
        self._status_code = status_code
        self._json_data = json_data
        self._exc = exc

    async def get(
        self, url: str, params: Any = None, headers: Any = None
    ) -> StubResponse:
        cfg = self._canned.get(url)
        if cfg is not None:
            if isinstance(cfg, StubResponse):
                return cfg
            return StubResponse(**cfg)  # type: ignore[arg-type]
        return StubResponse(
            status_code=self._status_code,
            json_data=self._json_data,
            exc=self._exc,
        )

    async def __aenter__(self) -> StubSession:
        return self

    async def __aexit__(self, *args: Any) -> None:
        pass


@pytest.fixture
def make_stub_session():
    """Factory fixture: returns a ``StubSession`` constructor.

    Usage::

        async def test_foo(make_stub_session):
            s = make_stub_session(json_data={"success": True, "result": {"data": [...], "pages": 1}})
            records = await fetch_paginated(s, Throttle(min_interval=0), ...)

    Keyword args mirror ``StubSession.__init__`` — ``status_code``,
    ``json_data``, ``exc``, ``canned_responses``.
    """
    return StubSession


class SyncStubResponse:
    """Fake requests.Response for sync (non-async) collector call-sites.

    Call-sites use ``resp.status_code`` (int), ``resp.raise_for_status()``
    (sync, raises on >= 400 or injected exception), ``resp.json()`` (canned
    dict), ``resp.content`` (bytes — xlsx zip payloads), ``resp.text`` (str —
    BSE JSONP body).
    """

    __slots__ = ("status_code", "_json", "_content", "_text", "_exc")

    def __init__(
        self,
        *,
        status_code: int = 200,
        json_data: dict[str, Any] | None = None,
        content: bytes = b"",
        text: str = "",
        exc: Exception | None = None,
    ) -> None:
        self.status_code = status_code
        self._json = json_data
        self._content = content
        self._text = text
        self._exc = exc

    def raise_for_status(self) -> None:
        if self._exc is not None:
            raise self._exc
        if self.status_code >= 400:
            raise Exception(f"HTTP {self.status_code}")

    def json(self) -> dict[str, Any]:
        return self._json if self._json is not None else {}

    @property
    def content(self) -> bytes:
        return self._content

    @property
    def text(self) -> str:
        return self._text


class SyncStubSession:
    """Sync stub for ``requests.Session`` — .get/.post return SyncStubResponse.

    Same injection API as StubSession: ``canned_responses`` keyed by URL
    (values are SyncStubResponse or kwargs dicts), per-test closure override
    (``stub.get = _get`` / ``stub.post = _post``). ``headers`` is a plain
    dict so ``session.headers.update(...)`` works; ``calls`` logs every
    (method, url, params/data) for assertion.
    """

    def __init__(
        self,
        *,
        canned_responses: dict[str, SyncStubResponse | dict[str, Any]] | None = None,
        status_code: int = 200,
        json_data: dict[str, Any] | None = None,
        content: bytes = b"",
        text: str = "",
        exc: Exception | None = None,
    ) -> None:
        self._canned = canned_responses or {}
        self._status_code = status_code
        self._json_data = json_data
        self._content = content
        self._text = text
        self._exc = exc
        self.headers: dict[str, Any] = {}
        self.calls: list[tuple[str, str, Any]] = []  # (method, url, params/data)

    def _dispatch(self, method: str, url: str, params: Any) -> SyncStubResponse:
        self.calls.append((method, url, params))
        cfg = self._canned.get(url)
        if cfg is not None:
            if isinstance(cfg, SyncStubResponse):
                return cfg
            return SyncStubResponse(**cfg)  # type: ignore[arg-type]
        return SyncStubResponse(
            status_code=self._status_code,
            json_data=self._json_data,
            content=self._content,
            text=self._text,
            exc=self._exc,
        )

    def get(self, url: str, params: Any = None, headers: Any = None,
            timeout: Any = None) -> SyncStubResponse:
        return self._dispatch("GET", url, params)

    def post(self, url: str, data: Any = None, headers: Any = None,
             timeout: Any = None) -> SyncStubResponse:
        return self._dispatch("POST", url, data)
