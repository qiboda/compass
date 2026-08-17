"""Adversarial tests for issue #292 incremental index_daily sync.

Targets the plan-declared incremental contract only:
- ``max_trade_date(dolt_table, symbol)`` exists and degrades safely.
- ``fetch_kline(..., last_date=None)`` passes ``beg=<last_date+1>``.
- ``_fetch_tencent_kline(..., last_date=None)`` keeps ``> last_date`` rows,
  stops pagination at the first ``<= last_date`` row, returns ``[]`` for a
  valid no-new-row page and ``None`` on failure/malformed payloads.
- ``run()`` uses per-symbol MAX(trade_date) for THS year ranges, skips
  no-op symbols, and never falls back to full-history copies for stale/future
  MAX values.

These tests are intentionally adversarial: they must FAIL against the
pre-fix full-history implementation and pass only after the true incremental
implementation lands.
"""

import asyncio
import re
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest
from conftest import StubResponse

import fetch_index_daily as fid

TODAY = date(2026, 8, 17)
TODAY_ISO = TODAY.isoformat()

KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
THS_LIST_URL = "https://q.10jqka.com.cn/thshy/"
THS_KLINE_TPL = "https://d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js"
TENCENT_KLINE_URL = "https://web.ifzq.gtimg.cn/appstock/app/newfqkline/get"


def _ths_list_response(codes_names):
    anchors = "\n".join(
        f'<a href="http://q.10jqka.com.cn/thshy/{code}/">{name}</a>'
        for code, name in codes_names
    )
    resp = StubResponse(status_code=200)
    resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")
    return resp


def _ths_kline_response(code: str, year: int, rows: list[str]) -> StubResponse:
    resp = StubResponse(status_code=200)
    data = ";".join(rows)
    resp._text = f'quotebridge_v4_line_bk_{code}_01_{year}({{"data":"{data}"}})'
    return resp


def _ths_kline_row(day: str, close: int = 3000) -> str:
    return f"{day},{close - 1},{close + 1},{close - 2},{close},120000000,52000000000"


def _official_kline_payload(code: str, klines: list[str]) -> dict:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _official_kline_row(day: str) -> str:
    return f"{day},2999.0,3000.0,3001.0,2998.0,120000000,50000000000,1.5,0.5,1.0,0.5"


def _tencent_kline_payload(tcode: str, rows: list[list]) -> dict:
    return {"data": {tcode: {"day": rows}}}


def _tencent_row(day: str) -> list:
    # newfqkline/get day row; index 8 is 成交额 in 万元.
    return [day, 3000.0, 3001.0, 3002.0, 2998.0, 120000000, 500000, 1.5, 50.0, 1.0, 0.5]


@pytest.fixture(autouse=True)
def _pin_today(monkeypatch: pytest.MonkeyPatch) -> None:
    """Pin local date and skip the today short-circuit in every test."""
    monkeypatch.setattr("fetch_index_daily._today", lambda: TODAY)
    monkeypatch.setattr("fetch_index_daily.last_report_date", lambda _t: "2026-08-16")
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


class TestMaxTradeDateBoundaries:
    """MAX(trade_date) edge values: year boundary, today, future dirty data."""

    async def test_max_on_dec31_starts_next_year_not_same_year(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """A board whose last bar is 2025-12-31 must not re-request 2025;
        the incremental window starts in 2026 (next year)."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))

        requested_years: list[int] = []
        stub = make_stub_session(
            canned_responses={
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
                KLINE_URL: {
                    "json_data": _official_kline_payload(
                        "000001", [_official_kline_row("2026-08-14")]
                    )
                },
            }
        )
        original_get = stub.get

        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                code, year = int(m.group(1)), int(m.group(2))
                requested_years.append(year)
                # Never return an empty page, so the pre-fix full-history loop
                # walks all the way back to 2007 and is caught red-handed.
                return _ths_kline_response(code, year, [_ths_kline_row(f"{year}-01-02")])
            return await original_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        def fake_max_trade_date(table: str, symbol: str) -> str:
            return "2025-12-31"

        monkeypatch.setattr(fid, "max_trade_date", fake_max_trade_date)

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        assert requested_years, "run() must consult THS per-year kline URLs"
        assert min(requested_years) >= 2026, (
            "2025-12-31 MAX must start from the next year; "
            f"requested years {requested_years}"
        )
        assert 2025 not in requested_years, (
            "a fully-synced prior year must not be re-fetched"
        )

    async def test_max_today_skips_symbol_requests(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """MAX == today means no new data for that symbol: run() must not
        issue any EastMoney/Tencent/THS request for it."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))

        ths_requests: list[str] = []
        em_requests: list[str] = []
        stub = make_stub_session(
            canned_responses={THS_LIST_URL: _ths_list_response([("881101", "半导体")])}
        )
        original_get = stub.get

        async def _get(url, params=None, headers=None):
            if "/v4/line/" in url:
                ths_requests.append(url)
            if KLINE_URL in url:
                em_requests.append((url, params))
            return await original_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        def fake_max_trade_date(table: str, symbol: str) -> str:
            return TODAY_ISO

        monkeypatch.setattr(fid, "max_trade_date", fake_max_trade_date)

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        # The only permissible request is the THS list page itself.
        assert not em_requests, "official index with MAX=today must not be fetched"
        assert not ths_requests, (
            "industry with MAX=today must not fetch any per-year kline"
        )

    async def test_max_future_date_safe_degrade_no_full_backfill(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """API dirty MAX (future date) must never trigger the 2007 full
        backfill, must not crash, and must not hammer old pages."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))

        ths_years: list[int] = []
        em_params: list[dict] = []
        stub = make_stub_session(
            canned_responses={
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
                KLINE_URL: {
                    "json_data": _official_kline_payload(
                        "000001", [_official_kline_row("2026-08-14")]
                    )
                },
            }
        )
        original_get = stub.get

        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                ths_years.append(int(m.group(2)))
                code, year = int(m.group(1)), int(m.group(2))
                return _ths_kline_response(code, year, [_ths_kline_row(f"{year}-01-02")])
            if KLINE_URL in url:
                em_params.append(params)
            return await original_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        def fake_max_trade_date(table: str, symbol: str) -> str:
            return "2099-12-31"

        monkeypatch.setattr(fid, "max_trade_date", fake_max_trade_date)

        # Must not raise, even though every per-year stub returns data.
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        assert not em_params or all(
            p.get("beg") != "0" for p in em_params
        ), "future MAX must not trigger a full-history beg=0 request"
        if ths_years:
            assert min(ths_years) >= TODAY.year, (
                "future MAX must not request history before the current year; "
                f"years {ths_years}"
            )
        # A handful of current-year requests maximum; definitely not the full
        # 2007..current sweep that the old implementation performs.
        assert len(ths_years) <= 1, (
            f"future MAX must not repeatedly request old years; got {ths_years}"
        )


class TestMaxTradeDateInvalidInput:
    """max_trade_date() itself and run() with illegal helper return values."""

    async def test_helper_absent_is_red(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """The plan-declared helper must exist as a module-level function."""
        assert hasattr(fid, "max_trade_date"), (
            "plan declares max_trade_date(dolt_table, symbol) but it is missing"
        )

    async def test_helper_returns_none_when_dolt_query_fails(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Missing table/DB must degrade to None (new symbol -> full backfill),
        not raise."""
        def boom(_sql: str) -> str:
            raise RuntimeError("no such table")

        monkeypatch.setattr(fid, "dolt_sql_csv", boom)
        assert fid.max_trade_date("index_daily", "BK881101") is None

    @pytest.mark.parametrize("bad_value", ["not-a-date", "2026-13-99", ""])
    async def test_run_survives_invalid_max_trade_date(
        self,
        bad_value: str,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """Illegal MAX strings are consumed safely (full/current-year fallback)
        — run() must never crash while consulting per-symbol max dates."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))

        calls: list[str] = []
        stub = make_stub_session(
            canned_responses={
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
                KLINE_URL: {
                    "json_data": _official_kline_payload(
                        "000001", [_official_kline_row("2026-08-14")]
                    )
                },
            }
        )
        original_get = stub.get

        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                code, year = int(m.group(1)), int(m.group(2))
                return _ths_kline_response(code, year, [_ths_kline_row(f"{year}-01-02")])
            return await original_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        def fake_max_trade_date(table: str, symbol: str) -> str:
            calls.append((table, symbol))
            return bad_value

        monkeypatch.setattr(fid, "max_trade_date", fake_max_trade_date)

        # Pre-fix run() never calls max_trade_date; post-fix it must call it
        # and still survive an illegal return value.
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        assert calls, "run() must consult max_trade_date per symbol"

    async def test_run_uses_none_return_as_new_symbol(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """None (no rows in Dolt) must be treated as a new symbol: the helper
        is consulted and full backfill is still allowed (no crash)."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(fid, "OFFICIAL_INDICES", ())

        calls: list[str] = []
        years: list[int] = []
        stub = make_stub_session(
            canned_responses={THS_LIST_URL: _ths_list_response([("881101", "半导体")])}
        )
        original_get = stub.get

        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                code, year = int(m.group(1)), int(m.group(2))
                years.append(year)
                return _ths_kline_response(code, year, [_ths_kline_row(f"{year}-01-02")])
            return await original_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        def fake_max_trade_date(table: str, symbol: str) -> None:
            calls.append((table, symbol))
            return None

        monkeypatch.setattr(fid, "max_trade_date", fake_max_trade_date)

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        assert calls, "run() must consult max_trade_date even for new symbols"
        assert years, "new symbol full backfill must still request years"


class TestFetchKlineIncremental:
    """EastMoney fetch_kline last_date parameter and idempotency."""

    async def test_last_date_maps_to_beg_plus_one(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        circular = {}

        async def _get(url, params=None, headers=None):
            circular["params"] = dict(params or {})
            return StubResponse(
                status_code=200,
                json_data=_official_kline_payload("000001", [_official_kline_row("2026-08-14")]),
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        await fid.fetch_kline(stub, fid.Throttle(), "1.000001", last_date="2026-08-17")

        assert circular["params"]["beg"] == "20260818", (
            f"beg must be last_date+1, got {circular['params'].get('beg')}"
        )

    async def test_repeated_incremental_fetch_never_uses_beg_zero(
        self, make_stub_session
    ) -> None:
        """Running the same symbol twice with the same last_date is idempotent:
        neither call may request the full-history window (beg=0)."""
        captured: list[dict] = []

        async def _get(url, params=None, headers=None):
            captured.append(dict(params or {}))
            return StubResponse(
                status_code=200,
                json_data=_official_kline_payload("000001", [_official_kline_row("2026-08-14")]),
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        await fid.fetch_kline(stub, fid.Throttle(), "1.000001", last_date="2026-08-10")
        await fid.fetch_kline(stub, fid.Throttle(), "1.000001", last_date="2026-08-10")

        assert len(captured) == 2
        assert all(p["beg"] != "0" for p in captured), (
            f"incremental reruns must not request the full range; got {captured}"
        )
        assert captured[0]["beg"] == captured[1]["beg"] == "20260811"


class TestFetchTencentKlineIncremental:
    """Tencent fallback incremental pagination/error/empty semantics."""

    async def _tencent_getter(
        self,
        pages: list[list[list]],
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
    ):
        """Return (stub, requested_params) for a multi-page Tencent stub.

        Monkeypatches the page size down so a small first page is treated as
        full and the old implementation would paginate to a second page.
        """
        monkeypatch.setattr(fid, "_TENCENT_PAGE_SIZE", 2)
        monkeypatch.setattr(fid, "_TENCENT_MAX_PAGES", 10)
        page_iter = iter(pages)
        requested: list[str] = []

        async def _get(url, params=None, headers=None):
            requested.append(params["param"] if params else "")
            try:
                rows = next(page_iter)
            except StopIteration:
                rows = []
            return StubResponse(
                status_code=200,
                json_data=_tencent_kline_payload("sh000001", rows),
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        return stub, requested

    async def test_mixed_page_keeps_new_rows_and_stops_before_old_page(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """When a page mixes rows newer and older than last_date, only the new
        rows survive and pagination stops immediately (no second older page)."""
        first_page = [
            _tencent_row("2026-08-14"),
            _tencent_row("2026-08-10"),  # already <= last_date, boundary hit
        ]
        second_page = [_tencent_row("2026-07-01")]
        stub, requested = await self._tencent_getter(
            [first_page, second_page], make_stub_session, monkeypatch
        )

        result = await fid._fetch_tencent_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-08-12"
        )

        dates = [row.split(",", 1)[0] for row in (result or [])]
        assert dates == ["2026-08-14"], (
            "only the row newer than last_date must be kept, mixed page "
            f"returned {dates}"
        )
        assert len(requested) == 1, (
            "encountering an old row must stop pagination immediately; "
            f"requested {len(requested)} pages"
        )

    async def test_request_failure_returns_none(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        stub = make_stub_session(exc=RuntimeError("network down"))
        monkeypatch.setattr(fid, "_TENCENT_PAGE_SIZE", 2)

        result = await fid._fetch_tencent_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-08-12"
        )
        assert result is None

    async def test_malformed_rows_non_list_returns_none(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        async def _get(url, params=None, headers=None):
            return StubResponse(
                status_code=200,
                json_data={"data": {"sh000001": {"day": "not-a-list"}}},
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        monkeypatch.setattr(fid, "_TENCENT_PAGE_SIZE", 2)

        result = await fid._fetch_tencent_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-08-12"
        )
        assert result is None

    async def test_valid_empty_increment_returns_empty_list(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """A valid response whose rows are all <= last_date is a successful
        no-op: [] (not None, not an error)."""
        first_page = [
            _tencent_row("2026-08-10"),
            _tencent_row("2026-08-09"),
        ]
        stub, requested = await self._tencent_getter(
            [first_page, []], make_stub_session, monkeypatch
        )

        result = await fid._fetch_tencent_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-08-12"
        )

        assert result == [], (
            "valid empty incremental response must be [] to distinguish "
            "successful no-op from failure; got {result!r}"
        )
        assert len(requested) == 1, "empty increment must not paginate further"


class TestRunIncrementalPerformance:
    """No historical-year regressions / resource-exhaustion guards."""

    async def test_ths_incremental_does_not_fetch_pre_max_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """MAX=2026-06-30 means the window starts in 2026: years before 2026
        must never be requested (otherwise an incremental run degrades into a
        full 2007 backfill)."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(fid, "OFFICIAL_INDICES", ())

        ths_years: list[int] = []
        stub = make_stub_session(
            canned_responses={THS_LIST_URL: _ths_list_response([("881101", "半导体")])}
        )
        original_get = stub.get

        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                code, year = int(m.group(1)), int(m.group(2))
                ths_years.append(year)
                # Keep returning data to expose the old full-history sweep.
                return _ths_kline_response(code, year, [_ths_kline_row(f"{year}-01-02")])
            return await original_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]
        monkeypatch.setattr(fid, "max_trade_date", lambda _t, _s: "2026-06-30")

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        assert ths_years, "incremental run should still fetch at least one year"
        assert min(ths_years) >= 2026, (
            "must not request any year before MAX(trade_date)'s year; "
            f"got {sorted(ths_years)}"
        )
