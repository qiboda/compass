"""Requirement-acceptance tests for collectors/fetch_freeproxy.py (issue #290)."""

from __future__ import annotations

import json
from typing import Any

import pytest

import fetch_freeproxy as mod


class _FakeResponse:
    def __init__(self, payload: dict[str, Any]) -> None:
        self._payload = payload

    def raise_for_status(self) -> None:
        pass

    def json(self) -> dict[str, Any]:
        return self._payload


class _FakeRedis:
    def __init__(self) -> None:
        self.hsets: list[tuple[str, str, str]] = []

    def hset(self, table: str, key: str, value: str) -> None:
        self.hsets.append((table, key, value))


def _sample_json_item(**overrides: Any) -> dict[str, Any]:
    item = {
        "ip": "1.2.3.4",
        "port": 8080,
        "protocol": "Http, Https",
        "country": "CN",
        "anonymity": "Elite",
        "speed": 100,
    }
    item.update(overrides)
    return item


def test_normalize_json_item_creates_redis_record() -> None:
    record = mod.normalize_json_item(_sample_json_item())
    assert record["proxy"] == "1.2.3.4:8080"
    assert record["https"] is False
    assert record["source"] == "freeproxy"
    assert record["region"] == "CN"
    assert record["anonymous"] == "Elite"
    assert record["fail_count"] == 0
    assert record["check_count"] == 0


def test_fetch_json_proxies_filters_http_and_limits(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = {
        "data": [
            _sample_json_item(ip="1.1.1.1", protocol="Http"),
            _sample_json_item(ip="2.2.2.2", protocol="Http, Https"),
            _sample_json_item(ip="3.3.3.3", protocol="Socks5"),
            _sample_json_item(ip="4.4.4.4", protocol="Http"),
        ]
    }
    monkeypatch.setattr(
        mod.curl_requests,
        "get",
        lambda url, timeout: _FakeResponse(payload),
    )
    records = mod.fetch_json_proxies("http://example.invalid/proxies.json", limit=2)
    proxies = [r["proxy"] for r in records]
    assert proxies == ["2.2.2.2:8080", "1.1.1.1:8080"]  # https-capable first


def test_fetch_json_proxies_prefers_cn_and_elite(monkeypatch: pytest.MonkeyPatch) -> None:
    payload = {
        "data": [
            _sample_json_item(ip="1.1.1.1", country="US", anonymity="Transparent"),
            _sample_json_item(ip="2.2.2.2", country="CN", anonymity="Elite"),
            _sample_json_item(ip="3.3.3.3", country="JP", anonymity="Elite"),
        ]
    }
    monkeypatch.setattr(
        mod.curl_requests,
        "get",
        lambda url, timeout: _FakeResponse(payload),
    )
    records = mod.fetch_json_proxies("http://example.invalid/proxies.json", limit=3)
    assert records[0]["proxy"] == "2.2.2.2:8080"


def test_write_to_redis_uses_hset(monkeypatch: pytest.MonkeyPatch) -> None:
    fake = _FakeRedis()
    monkeypatch.setattr(mod, "_new_redis_client", lambda url: fake)
    records = [
        {"proxy": "1.2.3.4:80", "https": False, "source": "freeproxy"},
        {"proxy": "5.6.7.8:443", "https": False, "source": "freeproxy"},
    ]
    written = mod.write_to_redis("redis://@localhost/0", "use_proxy", records)
    assert written == 2
    assert len(fake.hsets) == 2
    table, key, value = fake.hsets[0]
    assert table == "use_proxy"
    assert key == "1.2.3.4:80"
    assert json.loads(value)["proxy"] == "1.2.3.4:80"


def test_main_json_seeds(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    monkeypatch.setattr(
        mod,
        "fetch_json_proxies",
        lambda url, limit: [{"proxy": "1.2.3.4:80", "source": "freeproxy"}],
    )
    monkeypatch.setattr(mod, "write_to_redis", lambda url, table, records: 1)
    rc = mod.main(["--source", "json", "--limit", "1"])
    captured = capsys.readouterr()
    assert rc == 0
    assert "seeded 1 proxies" in captured.out


def test_main_realtime_seeds(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(
        mod,
        "fetch_realtime_proxies",
        lambda limit, sources: [{"proxy": "1.2.3.4:80", "source": "freeproxy"}],
    )
    monkeypatch.setattr(mod, "write_to_redis", lambda url, table, records: 1)
    rc = mod.main(["--source", "realtime", "--limit", "1"])
    captured = capsys.readouterr()
    assert rc == 0
    assert "seeded 1 proxies" in captured.out


def test_main_no_proxies_returns_1(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setattr(mod, "fetch_json_proxies", lambda url, limit: [])
    rc = mod.main(["--source", "json"])
    captured = capsys.readouterr()
    assert rc == 1
    assert "no proxies fetched" in captured.err
