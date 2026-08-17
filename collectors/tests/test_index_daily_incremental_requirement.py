"""Requirement acceptance tests for issue #292 — index_daily true incremental sync.

Plan contract under test (``.dsh/plans/index-daily-incremental.md``):

1. ``fetch_index_daily.max_trade_date(dolt_table, symbol) -> str | None``
   queries the Dolt ``index_daily`` max ``trade_date`` for a symbol.
2. ``fetch_kline(..., last_date: str | None = None)``:
   - ``last_date is None`` → ``params["beg"] == "0"`` (full history);
   - otherwise ``params["beg"]`` is ``(last_date + 1 day).strftime('%Y%m%d')``.
3. ``_fetch_tencent_kline(..., last_date: str | None = None)``:
   - incremental mode starts from the newest page;
   - stops paging once a row ``<= last_date`` is seen in the current page;
   - keeps only rows ``> last_date``;
   - a valid response with no new rows returns ``[]``, not ``None``.
4. ``run()`` per-symbol incremental behavior:
   - existing THS board (max_trade_date = e.g. 2026-07-31) only requests
     years from MAX year to current year (not 2025/2007 ...);
   - new THS board (max_trade_date = None) still requests
     current year → 2007 full backfill;
   - existing THS board with a valid empty/no-new response (weekend/halt)
     does not raise and does not bump the consecutive-failure fast-fail;
   - official EastMoney incremental receives ``beg = last_date + 1``;
   - official Tencent incremental stops at ``<= last_date`` and keeps new rows;
   - global ``last_report_date == today`` still performs zero HTTP requests.

RED note: the new interface does not exist yet in fetch_index_daily.py, so
tests calling ``fetch_kline(..., last_date=...)`` /
``_fetch_tencent_kline(..., last_date=...)`` fail with TypeError, and direct
``max_trade_date`` tests fail with AttributeError. Run-level tests create the
not-yet-implemented module function via ``raising=False`` so the current
non-incremental behavior is exercised and fails on the behavioral assertions.
"""

import asyncio
import re
import sys
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Handoff-verified endpoints.
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
THS_LIST_URL = "https://q.10jqka.com.cn/thshy/"
THS_KLINE_TPL = "https://d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js"
TENCENT_KLINE_URL = "https://web.ifzq.gtimg.cn/appstock/app/newfqkline/get"


# ── helpers copied from test_index_daily.py (self-contained file) ──


def _kline_row(
    day: str,
    close: float = 3000.0,
    volume: float = 1.2e8,
    amount: float = 5.0e10,
) -> str:
    """EastMoney kline 11-field CSV row."""
    return (
        f"{day},{close - 1},{close},{close + 1},{close - 2},"
        f"{volume},{amount},1.5,0.5,1.0,0.5"
    )


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _ths_list_response(codes_names: list[tuple[str, str]]) -> StubResponse:
    """GBK-encoded THS list page: one anchor per (881xxx code, display name)."""
    anchors = "\n".join(
        f'<a href="http://q.10jqka.com.cn/thshy/{code}/">{name}</a>'
        for code, name in codes_names
    )
    resp = StubResponse(status_code=200)
    resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")
    return resp


def _ths_kline_response(code: str, year: int, rows: list[str]) -> StubResponse:
    """JSONP per-year THS kline body (rows in THS order: date,o,h,l,c,v,amt)."""
    resp = StubResponse(status_code=200)
    data = ";".join(rows)
    resp._text = f'quotebridge_v4_line_bk_{code}_01_{year}({{"data":"{data}"}})'
    return resp


def _ths_kline_row(day: str, close: float = 3000.0) -> str:
    """One 7-field THS row: date,open,high,low,close,volume,amount."""
    return f"{day},{close - 1},{close + 1},{close - 2},{close},120000000,52000000000"


def _tencent_response(tcode: str, rows: list[list[object]]) -> StubResponse:
    """Tencent newfqkline/get JSON body for one code."""
    return StubResponse(status_code=200, json_data={"data": {tcode: {"day": rows}}})


def _record_stub(
    make_stub_session,
    *,
    ths_boards: list[tuple[str, str]] | None = None,
    ths_rows_by_year: dict[int, list[str]] | None = None,
    em_json: dict[str, object] | None = None,
    tencent_json: dict[str, object] | None = None,
):
    """Build a capturing StubSession for run()-level tests.

    Returns ``(stub, calls)`` where ``calls`` is ``[(url, params), ...]``.
    ``em_json=None`` makes EastMoney return an empty body so the Tencent
    fallback path is exercised.
    """
    ths_boards = ths_boards or []
    ths_rows_by_year = ths_rows_by_year or {}
    calls: list[tuple[str, dict | None]] = []
    stub = make_stub_session()

    async def _get(url, params=None, headers=None):
        calls.append((url, params))
        if "thshy" in url:
            return _ths_list_response(ths_boards)
        m = re.match(
            r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url
        )
        if m:
            code, year = m.group(1), m.group(2)
            return _ths_kline_response(code, int(year), ths_rows_by_year.get(int(year), []))
        if url.startswith(KLINE_URL):
            if em_json is None:
                return StubResponse(status_code=200, json_data={})
            return StubResponse(status_code=200, json_data=em_json)
        if url.startswith(TENCENT_KLINE_URL):
            if tencent_json is None:
                return StubResponse(status_code=200, json_data={})
            return StubResponse(status_code=200, json_data=tencent_json)
        return StubResponse(status_code=200, json_data={})

    stub.get = _get  # type: ignore[method-assign]
    return stub, calls


def _prepare_run_env(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    *,
    today: date = date(2026, 8, 2),
    last_report: str = "2026-07-31",
) -> None:
    """Shared run() isolation: temp dir, stale last_report_date, no real Dolt."""
    monkeypatch.chdir(tmp_path)
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
    monkeypatch.setattr("fetch_index_daily._today", lambda: today)
    monkeypatch.setattr("fetch_index_daily.last_report_date", lambda _t: last_report)
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


# ── max_trade_date interface ─────────────────────────────────────


class TestMaxTradeDateContract:
    def test_max_trade_date_interface_exists(self) -> None:
        """Plan requires a module-level max_trade_date(dolt_table, symbol)."""
        import fetch_index_daily as fid  # noqa: E402

        assert callable(fid.max_trade_date), "max_trade_date must be defined"


# ── fetch_kline last_date contract ───────────────────────────────


class TestFetchKlineLastDate:
    async def test_fetch_kline_last_date_none_beg_is_zero(
        self, make_stub_session
    ) -> None:
        import fetch_index_daily as fid  # noqa: E402

        seen: list[dict] = []
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            seen.append(params)
            return StubResponse(
                status_code=200,
                json_data={"data": {"code": "000001", "klines": []}},
            )
        stub.get = _get  # type: ignore[method-assign]

        result = await fid.fetch_kline(
            stub, fid.Throttle(), "1.000001", last_date=None
        )
        assert result is not None
        assert seen[0]["beg"] == "0"

    async def test_fetch_kline_last_date_beg_is_next_day_compact(
        self, make_stub_session
    ) -> None:
        import fetch_index_daily as fid  # noqa: E402

        seen: list[dict] = []
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            seen.append(params)
            return StubResponse(
                status_code=200,
                json_data={"data": {"code": "000001", "klines": []}},
            )
        stub.get = _get  # type: ignore[method-assign]

        result = await fid.fetch_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-07-31"
        )
        assert result is not None
        assert seen[0]["beg"] == "20260801"


# ── _fetch_tencent_kline last_date contract ──────────────────────


class TestFetchTencentKlineLastDate:
    async def test_incremental_stops_at_last_date_and_keeps_new_rows(
        self, make_stub_session
    ) -> None:
        import fetch_index_daily as fid  # noqa: E402

        calls: list[dict | None] = []
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            calls.append(params)
            return _tencent_response(
                "sh000001",
                [
                    ["2026-07-31", "3000", "3001", "3002", "2998",
                     "120000000", "0", "0", "50000000"],
                    ["2026-07-30", "2999", "3000", "3001", "2997",
                     "119000000", "0", "0", "49000000"],
                ],
            )
        stub.get = _get  # type: ignore[method-assign]

        rows = await fid._fetch_tencent_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-07-30"
        )
        dates = {r.split(",", 1)[0] for r in rows}
        assert dates == {"2026-07-31"}, (
            "only rows strictly after last_date must be kept; got {dates}"
        )
        assert len(calls) == 1, "paging must stop at the <= last_date row"

    async def test_incremental_valid_no_new_rows_returns_empty_list(
        self, make_stub_session
    ) -> None:
        import fetch_index_daily as fid  # noqa: E402

        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            return _tencent_response(
                "sh000001",
                [
                    ["2026-07-30", "2999", "3000", "3001", "2997",
                     "119000000", "0", "0", "49000000"],
                    ["2026-07-29", "2998", "2999", "3000", "2996",
                     "118000000", "0", "0", "48000000"],
                ],
            )
        stub.get = _get  # type: ignore[method-assign]

        rows = await fid._fetch_tencent_kline(
            stub, fid.Throttle(), "1.000001", last_date="2026-07-31"
        )
        assert rows == [], "valid empty increment must be [] not None"


# ── run() per-symbol incremental behavior ─────────────────────────


class TestRunIncremental:
    async def test_existing_ths_board_only_requests_max_year_to_current_year(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """An existing board with MAX=2026-07-31 must not request 2025/2007."""
        import fetch_index_daily as fid  # noqa: E402

        _prepare_run_env(monkeypatch, tmp_path)
        monkeypatch.setattr(fid, "OFFICIAL_INDICES", ())
        monkeypatch.setattr(
            fid,
            "max_trade_date",
            lambda dolt_table, symbol: "2026-07-31" if symbol == "BK881101" else None,
            raising=False,
        )
        stub, calls = _record_stub(
            make_stub_session,
            ths_boards=[("881101", "半导体")],
            ths_rows_by_year={2026: [_ths_kline_row("2026-08-01")]},
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        requested_years = [
            int(m.group(2))
            for url, _ in calls
            if (m := re.match(
                r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$",
                url,
            ))
        ]
        assert requested_years == [2026], (
            "existing THS board must only request MAX year→current year; "
            f"got {requested_years}"
        )

    async def test_new_ths_board_backfills_full_history(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """A board with no prior rows (max_trade_date=None) fetches 2007..now."""
        import fetch_index_daily as fid  # noqa: E402

        _prepare_run_env(monkeypatch, tmp_path)
        monkeypatch.setattr(fid, "OFFICIAL_INDICES", ())
        monkeypatch.setattr(
            fid,
            "max_trade_date",
            lambda dolt_table, symbol: None,
            raising=False,
        )
        all_years = list(range(2007, 2027))
        stub, calls = _record_stub(
            make_stub_session,
            ths_boards=[("881101", "半导体")],
            ths_rows_by_year={
                year: [_ths_kline_row("2020-01-02"), _ths_kline_row(f"{year}-07-31")]
                for year in all_years
            },
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        requested_years = [
            int(m.group(2))
            for url, _ in calls
            if (m := re.match(
                r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$",
                url,
            ))
        ]
        assert sorted(requested_years) == all_years, (
            "new THS board must backfill 2007..current year; "
            f"got {sorted(requested_years)}"
        )

    async def test_existing_ths_no_new_rows_does_not_trigger_fast_fail(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Weekend/halt empty increments are successes, not fast-fail bumps."""
        import fetch_index_daily as fid  # noqa: E402

        _prepare_run_env(monkeypatch, tmp_path)
        board_codes = [f"88110{i}" for i in range(5)]
        monkeypatch.setattr(fid, "OFFICIAL_INDICES", ())
        monkeypatch.setattr(
            fid,
            "max_trade_date",
            lambda dolt_table, symbol: (
                "2026-07-31" if symbol.startswith("BK881") else None
            ),
            raising=False,
        )
        stub, _ = _record_stub(
            make_stub_session,
            ths_boards=[(code, f"板块{i}") for i, code in enumerate(board_codes)],
            ths_rows_by_year={},  # valid empty THS responses, no new rows
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()  # must not raise RuntimeError after 5 same-status boards

    async def test_official_eastmoney_incremental_beg_is_last_date_plus_one(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() must pass last_date to EastMoney fetch_kline."""
        import fetch_index_daily as fid  # noqa: E402

        _prepare_run_env(monkeypatch, tmp_path)
        monkeypatch.setattr(
            fid,
            "OFFICIAL_INDICES",
            ({"secid": "1.000001", "code": "000001", "name": "上证指数"},),
        )
        monkeypatch.setattr(
            fid,
            "max_trade_date",
            lambda dolt_table, symbol: (
                "2026-07-31" if symbol == "SH000001" else None
            ),
            raising=False,
        )
        stub, calls = _record_stub(
            make_stub_session,
            em_json=_kline_payload("000001", [_kline_row("2026-08-01")]),
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        em_params = [
            params
            for url, params in calls
            if url.startswith(KLINE_URL) and params is not None
        ]
        assert em_params and em_params[0]["beg"] == "20260801", (
            "EastMoney incremental request must set beg to last_date+1; "
            f"got {em_params[:1]}"
        )

    async def test_official_tencent_incremental_stops_and_keeps_new_rows(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() Tencent fallback must filter out rows <= last_date."""
        import fetch_index_daily as fid  # noqa: E402

        _prepare_run_env(monkeypatch, tmp_path)
        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        monkeypatch.setattr(
            fid,
            "OFFICIAL_INDICES",
            ({"secid": "1.000001", "code": "000001", "name": "上证指数"},),
        )
        monkeypatch.setattr(
            fid,
            "max_trade_date",
            lambda dolt_table, symbol: (
                "2026-07-30" if symbol == "SH000001" else None
            ),
            raising=False,
        )
        tencent_rows = [
            ["2026-07-31", "3000", "3001", "3002", "2998",
             "120000000", "0", "0", "50000000"],
            ["2026-07-30", "2999", "3000", "3001", "2997",
             "119000000", "0", "0", "49000000"],
        ]
        stub, calls = _record_stub(
            make_stub_session,
            em_json=None,  # EastMoney empty -> Tencent fallback
            tencent_json=_tencent_response("sh000001", tencent_rows).json(),
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        rows = [
            r
            for batch in captured
            for r in batch
            if r.get("trade_date") and r["symbol"] == "SH000001"
        ]
        dates = {r["trade_date"] for r in rows}
        assert dates == {"2026-07-31"}, (
            "Tencent incremental must keep only rows > last_date; got {dates}"
        )
        tencent_calls = [
            params
            for url, params in calls
            if url.startswith(TENCENT_KLINE_URL) and params is not None
        ]
        assert len(tencent_calls) == 1, (
            "Tencent incremental must stop paging at <= last_date row"
        )

    async def test_global_last_report_date_today_still_zero_requests(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """The existing whole-table short-circuit must not regress."""
        import fetch_index_daily as fid  # noqa: E402

        _prepare_run_env(
            monkeypatch,
            tmp_path,
            today=date(2026, 7, 31),
            last_report="2026-07-31",
        )
        counter = [0]
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            counter[0] += 1
            return StubResponse(status_code=200, json_data={})
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await fid.run()

        assert counter[0] == 0, "last_report_date == today must not fetch"
