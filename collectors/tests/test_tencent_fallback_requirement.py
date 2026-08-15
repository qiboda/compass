"""Requirement acceptance tests for issue #278 — Tencent fallback for official indices.

Contract under test (issue #278 acceptance criteria, fetch_index_daily.py):

  C1. ``_tencent_code(secid)`` maps an EastMoney secid to the Tencent symbol:
      prefix ``1.`` → ``sh``, ``0.`` → ``sz`` + lowercase code
      (``1.000001`` → ``sh000001``, ``0.399001`` → ``sz399001``).
  C2. ``_fetch_tencent_kline(session, throttle, secid)`` fetches full history by
      paginating ``param=<code>,day,<start_date>,,<count>,qfq`` with count ≤ 2000;
      it keeps advancing the start_date until a short (<2000) page is seen and
      returns the merged round bars.
  C3. ``run()`` fetches official indices from EastMoney first; when a target's
      EastMoney kline fails or is empty it automatically falls back to Tencent.
      The CSV output format is unchanged — a Tencent bar (which carries no
      amount) is written with ``amount == 0``.
  C4. ``run()``'s Tencent segment is protected by the #277 consecutive-failure
      fast fail: 5 consecutive official indices failing on BOTH sources raise a
      ``RuntimeError`` mentioning "连续" and never request the remaining target.

The #277 machinery (``run()``, ``_bump_failure``, ``_MAX_CONSECUTIVE_FAILURES``,
``common.EM_MIN_INTERVAL``) is already GREEN. Issue #278 (Tencent source) is NOT
implemented yet, so every helper / fallback / amount test below fails with
AttributeError until GREEN.

STATUS: RED.
"""

import asyncio
import csv
import sys
from datetime import date, timedelta
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Handoff-verified endpoints.
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"
TENCENT_URL = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get"

# Tencent bars are 6 fields: date, open, close, high, low, volume (NO amount).
_TENCENT_PAGE_SIZE = 2000


def _tencent_row(day: str, close: float = 3000.0) -> list[str]:
    return [
        day,
        f"{close - 1}",
        f"{close}",
        f"{close + 1}",
        f"{close - 2}",
        "120000000",  # volume
    ]


def _tencent_payload(code: str, rows: list[list[str]]) -> dict[str, object]:
    """Shape of the Tencent fqkline/get response: data[code]["day"] = [[...]]. """
    return {"code": 0, "msg": "", "data": {code: {"day": rows}}}


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _pin_today(monkeypatch: pytest.MonkeyPatch, day: str = "2026-08-02") -> None:
    monkeypatch.setattr("fetch_index_daily._today", lambda: date.fromisoformat(day))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


def _env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
    # COMPASS_CSV_DIR is already pointed at tmp_path by conftest._isolate_csv_dir.


def _read_rows(path: Path) -> list[dict[str, str]]:
    with open(path, newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


# ── C1: secid → Tencent symbol mapping ────────────────────────────────────────


class TestTencentCodeMapping:
    """C1: _tencent_code maps EastMoney secid to a Tencent symbol."""

    def test_sh_secid_maps_to_sh_prefix(self) -> None:
        """RED: 1.000001 (上证指数) → sh000001."""
        from fetch_index_daily import _tencent_code  # noqa: E402

        assert _tencent_code("1.000001") == "sh000001"

    def test_sz_secid_maps_to_sz_prefix(self) -> None:
        """RED: 0.399001 (深证成指) → sz399001."""
        from fetch_index_daily import _tencent_code  # noqa: E402

        assert _tencent_code("0.399001") == "sz399001"

    def test_all_official_indices_have_mappable_secid(self) -> None:
        """RED: every OFFICIAL_INDICES secid maps to a clean sh/sz+code symbol."""
        from fetch_index_daily import OFFICIAL_INDICES, _tencent_code  # noqa: E402

        for t in OFFICIAL_INDICES:
            mapped = _tencent_code(t["secid"])
            assert mapped.startswith(("sh", "sz")), f"{t['secid']} → {mapped}"
            assert mapped[2:] == t["code"].lower(), f"{t['secid']} → {mapped}"


# ── C2: full-history pagination via start_date advance ────────────────────────


class TestTencentPagination:
    """C2: _fetch_tencent_kline loops count=2000 + advances start_date."""

    def _days(self, n: int, start: str = "2026-01-01") -> list[str]:
        d = date.fromisoformat(start)
        return [(d + timedelta(days=i)).isoformat() for i in range(n)]

    async def test_merges_pages_and_advances_start_date(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: page 1 returns exactly 2000 rows (full), page 2 returns fewer —
        the helper must (a) request page 1 with empty start_date, (b) advance
        start_date on the 2nd request, and (c) return ALL 2000+ rows merged."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        page1_days = self._days(_TENCENT_PAGE_SIZE, "2025-01-01")
        page2_days = self._days(50, "2026-06-13")  # calendar-following tail
        page1 = [_tencent_row(d) for d in page1_days]
        page2 = [_tencent_row(d) for d in page2_days]

        calls: list[dict] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            calls.append(params or {})
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                parts = param.split(",")
                start = parts[2] if len(parts) > 2 else ""
                if start == "":
                    return StubResponse(json_data=_tencent_payload(code, page1))
                return StubResponse(json_data=_tencent_payload(code, page2))
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        secid = "1.000001"
        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), secid)

        # Full merge: 2000 + 50 rows returned.
        assert len(klines) == _TENCENT_PAGE_SIZE + 50, (
            f"must merge both pages, got {len(klines)} rows"
        )
        # First request: start_date empty/absent (count ≤ 2000 caps the page).
        first = calls[0].get("param", "")
        assert first.startswith("sh000001,day,"), f"param {first!r}"
        assert ",,2000,qfq" in first or ",2000,qfq" in first, f"page1 count={first!r}"
        # Second request carries an advanced start_date (a real date follows
        # the last fetched date, not an empty window).
        second = calls[1].get("param", "")
        assert second.startswith("sh000001,day,"), f"param2 {second!r}"
        parts2 = second.split(",")
        start2 = parts2[2] if len(parts2) > 2 else ""
        assert start2 != "", f"2nd page must advance start_date, got {second!r}"

    async def test_single_short_page_returns_without_second_request(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """RED: when the first page is already short (<2000), no second request
        is made — full history fit in one page."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        rows = [_tencent_row(f"2026-07-{(i % 27) + 1:02d}") for i in range(100)]
        calls: list[dict] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            calls.append(params or {})
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                return StubResponse(
                    json_data=_tencent_payload(param.split(",")[0], rows)
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")
        assert len(klines) == 100
        assert len(calls) == 1, "a short first page must not page again"


# ── C3: EastMoney-fails → automatic Tencent fallback + amount default ────────


class TestTencentFallbackAndAmount:
    """C3: run() falls back to Tencent and writes amount 0 for official rows."""

    async def test_official_falls_back_to_tencent_writes_zero_amount(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: ONLY 1.000001 is in the whitelist via monkeypatch, its EastMoney
        kline fails, so the run must fetch from Tencent and write an official
        row whose amount is 0/empty and whose symbol is SH000001."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.OFFICIAL_INDICES",
            ({"secid": "1.000001", "code": "000001", "name": "上证指数"},),
        )

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                # One board makes boards non-empty so run() also writes
                # index_basic on this full run (official names ride along).
                return StubResponse(
                    json_data=_clist_payload([{"f12": "BK0475", "f14": "半导体"}])
                )
            if "ifzq.gtimg.cn" in url:  # Tencent → success
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                return StubResponse(json_data=_tencent_payload(code, [_tencent_row("2026-07-31", 3000.0)]))
            if "kline/get" in url:  # EastMoney push2his → fail (500)
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        # Official row landed in index_daily.csv with amount 0 and correct symbol.
        rows = _read_rows(tmp_path / "index_daily.csv")
        official = [r for r in rows if r["symbol"] == "SH000001"]
        assert official, f"SH000001 row must be written from the Tencent fallback, got {rows!r}"
        assert official[0]["amount"] in {"0", ""}, (
            f"Tencent rows carry no amount → must default to 0; got {official[0]['amount']!r}"
        )
        assert official[0]["trade_date"] == "2026-07-31"
        # index_basic must have the official entry.
        basic = _read_rows(tmp_path / "index_basic.csv")
        assert any(r["symbol"] == "SH000001" for r in basic), (
            "index_basic must list the official index"
        )

    async def test_eastmoney_empty_falls_back_to_tencent(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: EastMoney returns EMPTY klines (non-error) for the official
        target → must still fall back to Tencent and produce rows."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.OFFICIAL_INDICES",
            ({"secid": "0.399001", "code": "399001", "name": "深证成指"},),
        )

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:  # Tencent success
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                return StubResponse(json_data=_tencent_payload(code, [_tencent_row("2026-07-31", 9000.0)]))
            if "kline/get" in url:  # EastMoney empty klines
                return StubResponse(json_data=_kline_payload("399001", []))
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = _read_rows(tmp_path / "index_daily.csv")
        official = [r for r in rows if r["symbol"] == "SZ399001"]
        assert official, "empty EastMoney must trigger Tencent fallback for SZ399001"
        assert official[0]["amount"] in {"0", ""}, "amount must default to 0/empty"


# ── C4: Tencent segment protected by #277 fast-fail ───────────────────────────


class TestTencentFastFail:
    """C4: the Tencent segment is wired into #277 consecutive-failure fast-fail.

    The key #278-specific behavior: a target that FAILS on EastMoney but
    RECOVERS on Tencent must RESET the consecutive-failure counter (it is a
    success, not a failure). A target is only counted as a failure once BOTH
    sources are exhausted.
    """

    async def test_five_eastmoney_only_failures_abort_without_tencent(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: 6 official indices whitelisted; EastMoney fails for ALL of them
        but Tencent would succeed for the 5th. The #277 counter must only mark a
        target failed once Tencent was tried — so 4 EastMoney-only failures do
        NOT trigger, and the 5th succeeds on Tencent → no abort, all targets
        fetched. The current (no-tencent) implementation counts all 5 EastMoney
        failures consecutively and aborts → RED."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        # 5 official targets: first 4 fail on EastMoney, the 5th also fails on
        # EastMoney but SUCCEEDS on Tencent → it resets the streak.
        targets = tuple(
            {"secid": f"{'1' if i % 2 else '0'}.{100000 + i:06d}",
             "code": f"{100000 + i:06d}", "name": f"官方{i}"}
            for i in range(1, 7)
        )
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", targets)

        fetched_via_tencent: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                # Tencent succeeds for all → every official target is recoverable,
                # so the consecutive-failure counter must never reach 5.
                fetched_via_tencent.append(code)
                return StubResponse(
                    json_data=_tencent_payload(code, [_tencent_row("2026-07-31")])
                )
            if "kline/get" in url:  # EastMoney fails for every official target
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        # No RuntimeError expected — every target recovers via Tencent.
        daily_path = None
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await run()

        assert daily_path.exists(), "recoverable targets must let the run complete"
        assert len(fetched_via_tencent) == len(targets), (
            "every official target must fall back to Tencent; "
            f"got {len(fetched_via_tencent)}/{len(targets)}"
        )
        # The 5th (recovered) target's row must be written → counter reset.
        rows = _read_rows(tmp_path / "index_daily.csv")
        fifth = targets[4]
        fifth_symbol = f"{'SH' if fifth['secid'].startswith('1.') else 'SZ'}{fifth['code']}"
        assert any(r["symbol"] == fifth_symbol for r in rows), (
            "a Tencent-recovered official target must persist its daily row"
        )

    async def test_five_doublefail_officials_terminate(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: when Tencent ALSO fails (double-fail) for 5 consecutive official
        targets, run() must abort (连续… RuntimeError) and never fetch the 6th —
        proving the Tencent segment is inside the #277 fast-fail boundary."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        targets = tuple(
            {"secid": f"{'1' if i % 2 else '0'}.{100000 + i:06d}",
             "code": f"{100000 + i:06d}", "name": f"官方{i}"}
            for i in range(1, 7)
        )
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", targets)

        tracked: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                return StubResponse(status_code=500, json_data={})  # Tencent fail too
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                tracked.append(secid)
                return StubResponse(status_code=500, json_data={})  # EastMoney fail
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), f"must raise 连续…, got {str(e.value)!r}"
        # The 6th whitelisted target must never be requested.
        last = targets[-1]["secid"]
        assert last not in tracked, (
            "after 5 consecutive double-failures run() must not fetch the remaining target"
        )
        # The failing targets themselves must have been touched.
        assert targets[0]["secid"] in tracked, "the failing targets must be requested"
