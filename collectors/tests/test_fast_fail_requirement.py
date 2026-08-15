"""Requirement acceptance tests for issue #277 — consecutive-failure fast fail.

Contract under test (issue #277 acceptance criteria, fetch_index_daily.py +
common.py):

  C1. ``run()`` keeps a consecutive-failure counter; **5 consecutive failed
      targets** (FAILED = all host×attempt exhausted, or empty ``klines``)
      terminates the run immediately, without requesting the remaining targets.
  C2. Before terminating, the daily/basic records already fetched are **written
      to CSV** (keep-resumable), then a ``RuntimeError`` is raised whose message
      mentions "连续 N 个标的失败（疑似反爬或接口故障）".
  C3. Fail/success interleaving does NOT trigger (a success resets the counter).
  C4. Boundary: 4 consecutive failures do NOT terminate; the 5th triggers it.
  C5. ``common.EM_MIN_INTERVAL == 2.0`` (global rate limit widened).
  C6. Coverage: consecutive-termination + CSV keep + interleave-no-kill +
      boundary (4 no / 5 yes) + rate-limit assertion.

Failure definition: a target "fails" when ``_get_json`` returns None (500 →
``raise_for_status`` → every host×attempt exhausted) OR the kline response
carries empty ``klines``.  Both count toward consecutive failures.

STATUS: RED — the current implementation has no consecutive-failure counter
(it just prints FAILED/empty and continues), and ``EM_MIN_INTERVAL = 0.5``.
Every terminating/raising/interval test below therefore fails until GREEN.
"""

import asyncio
import csv
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
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"

# 东财 kline 11 字段: 日期,开盘,收盘,最高,最低,成交量,成交额,振幅,涨跌幅,涨跌额,换手率
def _kline_row(day: str = "2026-07-31", close: float = 3000.0) -> str:
    return (
        f"{day},{close - 1},{close},{close + 1},{close - 2},"
        f"120000000,50000000000,1.5,0.5,1.0,0.5"
    )


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _board(code: str, name: str) -> dict[str, object]:
    return {"f12": code, "f14": name}


# secid → behavior key used by the custom stub.get dispatcher.
_OK = "ok"
_FAIL = "fail"   # HTTP 500 → _get_json returns None (all hosts×attempts exhausted)
_EMPTY = "empty"  # non-error but empty klines → counted as a failure too


def _make_stub(make_stub_session, boards: list[tuple[str, str]], kline: dict[str, str]):
    """Build an AsyncSession stub whose ``get`` dispatches per target.

    ``boards`` is the THS list page content (881xxx code, name) and ``kline``
    maps a bare 881xxx code → {_OK, _FAIL, _EMPTY}. Unknown codes (e.g. the
    official indices in scenarios with no early terminate) default to _OK so
    they never contribute a spurious failure. ``tracked`` records every THS
    code that was actually requested (in request order, including retry
    repeats) so tests can assert a remaining target was never reached.

    Returns ``(stub, tracked)``.
    """
    tracked: list[str] = []
    stub = make_stub_session()

    async def _get(url, params=None, headers=None):
        if "d.10jqka.com.cn" in url:  # THS per-year kline
            m = re.search(r"bk_(\d+)/01/", url)
            code = m.group(1) if m else ""
            if code:
                tracked.append(code)
            kind = kline.get(code, _OK)
            if kind == _FAIL:
                return StubResponse(status_code=500, json_data={})
            if kind == _EMPTY:
                return _ths_empty_response()
            return _ths_kline_response(code, 2026, [_ths_kline_row()])
        if "thshy" in url:
            anchors = "\n".join(
                f'<a href="http://q.10jqka.com.cn/thshy/{code}/">{name}</a>'
                for code, name in boards
            )
            resp = StubResponse(status_code=200)
            resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")
            return resp
        if "kline/get" in url:
            secid = (params or {}).get("secid", "")
            code = secid.split(".")[-1]
            return StubResponse(json_data=_kline_payload(code, [_kline_row()]))
        return StubResponse(status_code=200, json_data={})

    stub.get = _get  # type: ignore[method-assign]
    return stub, tracked


def _ths_kline_row(day: str = "2026-07-31", close: float = 3000.0) -> str:
    """One 7-field THS row: date,open,high,low,close,volume,amount."""
    return f"{day},{close - 1},{close + 1},{close - 2},{close},120000000,52000000000"


def _ths_kline_response(code: str, year: int, rows: list[str]) -> StubResponse:
    """JSONP per-year THS kline body."""
    resp = StubResponse(status_code=200)
    data = ";".join(rows)
    resp._text = f'quotebridge_v4_line_bk_{code}_01_{year}({{"data":"{data}"}})'
    return resp


def _ths_empty_response() -> StubResponse:
    return _ths_kline_response("000000", 2026, [])


def _pin_today(monkeypatch: pytest.MonkeyPatch) -> None:
    """Pin ``_today`` to a fixed date and no-op all asyncio.sleep (no real waits)."""
    monkeypatch.setattr("fetch_index_daily._today", lambda: date(2026, 8, 2))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


def _env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    """Isolate data/CSV dirs; no Dolt dir → last_report_date=='' → full fetch."""
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
    # COMPASS_CSV_DIR is already pointed at tmp_path by conftest._isolate_csv_dir.


def _read_rows(path: Path) -> list[dict[str, str]]:
    with open(path, newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


# ── C1/C6: consecutive failures terminate + stop requesting remaining ────────


class TestConsecutiveFailureTerminates:
    """5 consecutive failed targets terminate run() and never touch the rest."""

    async def test_five_request_failures_terminate_before_remaining_target(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: 6 boards discovered; boards 1-5 fail with HTTP 500, board 6 would
        succeed. run() must raise after the 5th failure and never request board 6."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [(f"881{i:03d}", f"B{i}") for i in range(101, 107)]  # BK1101..BK1106
        kline = {
            f"881{i:03d}": (_FAIL if i <= 105 else _OK) for i in range(101, 107)
        }
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), f"message must mention 连续, got {str(e.value)!r}"
        assert "疑似反爬" in str(e.value), "message must hint anti-bot, got {str(e.value)!r}"
        # The remaining (would-succeed) target after the 5th failure is never fetched.
        assert "881106" not in tracked, (
            "after 5 consecutive failures run() must stop before the remaining target"
        )
        assert "881101" in tracked, "the failing targets must themselves be requested"

    async def test_five_empty_klines_failures_terminate_before_remaining_target(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: empty (non-error) klines also count as a failure — 5 consecutive
        empty boards terminate and never reach the 6th."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [(f"881{i:03d}", f"B{i}") for i in range(111, 117)]  # BK1111..BK1116
        kline = {
            f"881{i:03d}": (_EMPTY if i <= 115 else _OK) for i in range(111, 117)
        }
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "empty klines failures must also fast-fail"
        assert "881116" not in tracked, (
            "5 empty-klines failures must stop before the remaining target"
        )


# ── C2: CSV preserved + RuntimeError raised on termination ───────────────────


class TestCsvPreservedOnTerminate:
    """Records already fetched are written to CSV before the RuntimeError."""

    async def test_termination_writes_fetched_rows_then_raises(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: one board succeeds (daily row), then 5 consecutive 500-failures.
        run() must (a) raise RuntimeError and (b) have persisted the successful
        board's rows to index_daily.csv before raising (keep-resumable). Uses the
        REAL run() write path (write_csv NOT patched)."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        # First board succeeds, the next 5 fail consecutively.
        ok = ("881201", "赢家板")
        fail_boards = [(f"881{i:03d}", f"F{i}") for i in range(202, 207)]  # BK1202..BK1206
        boards = [ok, *fail_boards]
        kline = {
            "881201": _OK,
            **{f"881{i:03d}": _FAIL for i in range(202, 207)},
        }
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "termination must still raise 连续... RuntimeError"

        # The successful board's daily rows must be on disk (keep-resumable).
        daily_path = tmp_path / "index_daily.csv"
        assert daily_path.exists(), "index_daily.csv must exist even though run() raised"
        rows = _read_rows(daily_path)
        symbols = {r["symbol"] for r in rows}
        assert "BK881201" in symbols, (
            f"records fetched before termination must be written to CSV; got symbols {symbols}"
        )
        # index_basic keeps the discovered boards (including the failed streak).
        basic_path = tmp_path / "index_basic.csv"
        assert basic_path.exists(), "index_basic.csv must be preserved for resumability"
        basic_symbols = {r["symbol"] for r in _read_rows(basic_path)}
        assert "BK881201" in basic_symbols and "BK881205" in basic_symbols, (
            "index_basic must retain discovered boards past the abort point"
        )

    async def test_incremental_abort_writes_daily_but_not_basic(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Incremental run (last_report_date non-empty) + 5 consecutive failures:
        abort still writes the fetched daily CSV, but does NOT rebuild
        index_basic (same gate as the normal incremental path)."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.last_report_date", lambda _tbl: "2026-07-31"
        )

        ok = ("881201", "赢家板")
        fail_boards = [(f"881{i:03d}", f"F{i}") for i in range(202, 207)]
        boards = [ok, *fail_boards]
        kline = {
            "881201": _OK,
            **{f"881{i:03d}": _FAIL for i in range(202, 207)},
        }
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "incremental abort must still raise 连续... RuntimeError"
        daily_path = tmp_path / "index_daily.csv"
        assert daily_path.exists(), "incremental abort must keep the fetched daily CSV"
        symbols = {r["symbol"] for r in _read_rows(daily_path)}
        assert "BK881201" in symbols, "daily rows fetched before the streak must be persisted"
        basic_path = tmp_path / "index_basic.csv"
        assert not basic_path.exists(), (
            "incremental abort must not rebuild index_basic (same gate as normal incremental path)"
        )


# ── C3/C4/C6: interleave-resets + boundary (4 no / 5 yes) ────────────────────


class TestInterleaveAndBoundary:
    """A success resets the counter; exactly 5 consecutive failures trigger."""

    async def test_interleaved_failures_do_not_terminate(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Guard (currently GREEN): F,F,S,F,F,F,S,S — max streak 3, so run() must
        NOT terminate and must fetch every board. This guard will fail if a GREEN
        over-aggressively counts total or mis-resets the counter."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        # B1,F B2,F B3,S B4,F B5,F B6,F B7,S B8,S  → max consecutive streak = 3.
        order = [_FAIL, _FAIL, _OK, _FAIL, _FAIL, _FAIL, _OK, _OK]
        boards = [(f"881{i:03d}", f"B{i}") for i in range(301, 309)]  # BK1301..BK1308
        kline = {f"881{i:03d}": order[i - 301] for i in range(301, 309)}
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        daily_path = None
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await run()  # must NOT raise

        # All boards fetched — no early termination despite 5 total failures.
        for i in range(301, 309):
            assert f"881{i:03d}" in tracked, (
                "interleaved failures must never stop the run (success resets counter)"
            )
        assert daily_path.exists(), "a completed run must materialize the daily CSV"

    async def test_four_consecutive_failures_do_not_terminate(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Guard (currently GREEN): 4 consecutive failures must NOT terminate —
        only the 5th triggers. B1-B4 fail, B5 succeeds → counter hits 4 then
        resets; run() completes normally."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [(f"881{i:03d}", f"B{i}") for i in range(401, 407)]  # BK1401..BK1406
        kline = {
            f"881{i:03d}": (_FAIL if i <= 404 else _OK) for i in range(401, 407)
        }
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await run()  # must NOT raise at 4 consecutive failures

        assert "881405" in tracked, (
            "the counter must not trigger at 4 — the 5th target still gets fetched"
        )
        assert daily_path.exists(), "run() completes and writes the daily CSV"

    async def test_five_consecutive_failures_terminate(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: B1-B5 fail consecutively — the 5th triggers termination before B6."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [(f"881{i:03d}", f"B{i}") for i in range(501, 507)]  # BK1501..BK1506
        kline = {
            f"881{i:03d}": (_FAIL if i <= 505 else _OK) for i in range(501, 507)
        }
        stub, tracked = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "5th consecutive failure must raise 连续... RuntimeError"
        assert "881506" not in tracked, (
            "the 5th failure stops the run before the remaining (success) target"
        )


# ── C5: global rate-limit widened ─────────────────────────────────────────────


class TestRateLimitWidened:
    """common.EM_MIN_INTERVAL must be 2.0 (issue #277 acceptance C5)."""

    def test_em_min_interval_is_two_seconds(self) -> None:
        """RED: the global EastMoney throttle interval must be widened to 2.0s."""
        import common  # noqa: E402
        import fetch_fin_indicators  # noqa: E402
        import fetch_stock_basic  # noqa: E402

        assert common.EM_MIN_INTERVAL == 2.0, (
            f"EM_MIN_INTERVAL must be 2.0 per issue #277, got {common.EM_MIN_INTERVAL!r}"
        )
        assert fetch_fin_indicators.EM_MIN_INTERVAL == 2.0, (
            "fetch_fin_indicators local EM_MIN_INTERVAL must also be 2.0 "
            "(global rate-limit decision)"
        )
        assert fetch_stock_basic.EM_MIN_INTERVAL == 2.0, (
            "fetch_stock_basic local EM_MIN_INTERVAL must also be 2.0 "
            "(global rate-limit decision)"
        )
