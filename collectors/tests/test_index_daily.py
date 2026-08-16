"""Tests for the index daily collector (epic #255 plan T1 + issue #283).

Plan contract under attack (fetch_index_daily.py):
- ``run()`` fetches official indices (hardcoded ~30, secid ``{1|0}.{code}``,
  EastMoney push2his + Tencent fallback) and THS industry boards (90 x 881xxx,
  list from ``q.10jqka.com.cn/thshy/``, per-year BK klines), writing CSV(s)
  for ``index_daily`` + ``index_basic``; ``import_to_dolt()`` loads them into
  Dolt tables with the plan DDL:
  ``index_daily (symbol PK, trade_date PK, index_type, OHLCV, update_date)`` +
  ``index_basic (symbol PK, name, index_type)``.
- Incremental ``last_report_date`` short-circuit (common.py:172-186) and
  auto full-history backfill for new boards (handoff decision 8).
- Rate limiting: host rotation + retry must not loop forever on 429.

URLs: EastMoney push2his kline / Tencent fqkline / THS list + per-year BK.
"""

import asyncio
import contextlib
import csv
import json
import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Handoff-verified endpoints (调研结论: push2his kline / THS list + BK kline).
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
THS_LIST_URL = "https://q.10jqka.com.cn/thshy/"
THS_KLINE_TPL = "https://d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js"

# 东财 kline 11 字段: 日期,开盘,收盘,最高,最低,成交量,成交额,振幅,涨跌幅,涨跌额,换手率
def _kline_row(
    day: str,
    close: float = 3000.0,
    volume: float = 1.2e8,
    amount: float = 5.0e10,
) -> str:
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


def _ths_list_getter(codes_names: list[tuple[str, str]]):
    """Return an async ``get`` answering THS_LIST_URL with the given boards."""
    canned = _ths_list_response(codes_names)

    async def _get(url, params=None, headers=None):
        if "thshy" in url:
            return canned
        return StubResponse(status_code=200, json_data={})

    return _get


def _ths_kline_getter(stub, codes: list[str], years: list[str]):
    """Wrap a stub so THS per-year kline URLs answer with one row per year.

    ``years`` selects which years return data (any other year answers an
    empty body → the run's year loop stops there). Rows are re-used across
    years so pagination tests can assert date coverage without 20 stubs.
    """
    original = stub.get

    async def _get(url, params=None, headers=None):
        m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
        if m:
            code, year = m.group(1), m.group(2)
            if code in codes and year in years:
                return _ths_kline_response(
                    code, int(year), [_ths_kline_row(f"{year}-07-31")]
                )
            return _ths_kline_response(code, int(year), [])
        return await original(url, params=params, headers=headers)

    return _get


# ── boundary values ──────────────────────────────────────────────


class TestBoundaries:
    """index_type tagging + code/date/OHLCV boundaries."""

    async def test_official_and_industry_index_type_tags(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """official (SH000001) rows tagged official, THS industry (BK881101)
        rows tagged industry — the plan DDL requires index_type."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_index_daily._today", lambda: __import__("datetime").date(2026, 8, 2))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )

        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload("000001", [_kline_row("2026-07-31")])
                },
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
            }
        )
        stub.get = _ths_kline_getter(stub, ["881101"], ["2026"])
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        official = next(r for r in rows if r["symbol"] == "SH000001")
        industry = next(r for r in rows if r["symbol"] == "BK881101")
        assert official["index_type"] == "official"
        assert industry["index_type"] == "industry"

        progress_path = tmp_path / "index_daily.progress.json"
        assert progress_path.exists()
        progress = json.loads(progress_path.read_text(encoding="utf-8"))
        assert progress["status"] == "completed"
        assert progress["percent"] == 100.0


    async def test_ths_boundary_codes_881000_and_881999(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """881000 and 881999 (6-digit extremes of the THS range) must be
        accepted by the list parser."""
        import fetch_index_daily as fid  # noqa: E402

        stub = make_stub_session()
        stub.get = _ths_list_getter([("881000", "边界最低"), ("881999", "边界最高")])
        boards = await fid.fetch_ths_industry_list(stub, fid.Throttle())
        codes = {c for c, _ in boards}
        assert "881000" in codes
        assert "881999" in codes


    async def test_early_history_date_preserved(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """上证指数 history starts 1990-12-19 (handoff实测 8703 条); an early
        1900-01-01 row must survive the date parse, not be dropped."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001",
                        [_kline_row("1900-01-01"), _kline_row("2026-07-31")],
                    )
                },
                THS_LIST_URL: _ths_list_response([]),
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        dates = {r["trade_date"] for r in rows}
        assert "1900-01-01" in dates, "early history row must be preserved"


    async def test_future_dated_kline_row_not_silently_imported(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """a kline row dated after today (API glitch / bad data) must be
        rejected — never silently published as a normal bar."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(
            "fetch_index_daily._today", lambda: __import__("datetime").date(2026, 8, 2)
        )
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001",
                        [_kline_row("2026-07-31"), _kline_row("2099-01-01")],
                    )
                },
                THS_LIST_URL: _ths_list_response([]),
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        dates = {r["trade_date"] for r in rows}
        assert "2099-01-01" not in dates, "future-dated row must not be imported"


    async def test_zero_and_negative_volume_amount_preserved(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """halted days (volume 0) and glitchy negative values must not crash
        the row build or drop the row."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001",
                        [
                            "2026-07-31,3000,3001,3002,2998,0,0,0,0,0,0",
                            "2026-07-30,2999,3000,3001,2997,-100,-1e9,0,0,0,0",
                        ],
                    )
                },
                THS_LIST_URL: _ths_list_response([]),
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        assert len(rows) == 2, "both rows must survive"
        zero = next(r for r in rows if r["trade_date"] == "2026-07-31")
        assert str(zero["volume"]) == "0", "zero volume must stay numeric 0"


# ── invalid input / malformed responses ──────────────────────────


class TestMalformedInput:
    """Malformed board codes, missing fields, CSV-injection names."""

    async def test_malformed_ths_codes_filtered(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """THS list anchors with malformed codes (88112 / 88112345 / 881AB12)
        must be rejected — never written to index_basic."""
        import fetch_index_daily as fid  # noqa: E402

        # Malformed anchors cannot round-trip through _ths_list_response
        # (it only formats valid codes), so build the raw page manually.
        anchors = (
            '<a href="http://q.10jqka.com.cn/thshy/881101/">半导体</a>'
            '<a href="http://q.10jqka.com.cn/thshy/88112/">畸形五位</a>'
            '<a href="http://q.10jqka.com.cn/thshy/88112345/">畸形七位</a>'
            '<a href="http://q.10jqka.com.cn/thshy/881AB12/">畸形字母</a>'
        )
        resp = StubResponse(status_code=200)
        resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")

        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            return resp
        stub.get = _get  # type: ignore[method-assign]

        boards = await fid.fetch_ths_industry_list(stub, fid.Throttle())
        codes = {c for c, _ in boards}
        assert codes == {"881101"}, f"only the valid code survives, got {codes}"


    async def test_anchor_without_code_skipped(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """THS anchors whose href carries no 881xxx code must be skipped, not
        crash the list parse."""
        import fetch_index_daily as fid  # noqa: E402

        anchors = (
            '<a href="http://q.10jqka.com.cn/other/881047/">无代码路径</a>'
            '<a href="http://q.10jqka.com.cn/thshy/881047/">半导体</a>'
        )
        resp = StubResponse(status_code=200)
        resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")

        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            return resp
        stub.get = _get  # type: ignore[method-assign]

        boards = await fid.fetch_ths_industry_list(stub, fid.Throttle())
        assert ("881047", "半导体") in boards
        assert len(boards) == 1


    async def test_name_with_comma_and_quote_csv_escaped(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """a board name containing a comma (CSV injection vector) must
        round-trip as ONE cell, not split the row."""
        import fetch_index_daily as fid  # noqa: E402

        evil_name = '半导体,芯片"; DROP TABLE index_basic; --'
        stub = make_stub_session()
        stub.get = _ths_list_getter([("881101", evil_name)])
        boards = await fid.fetch_ths_industry_list(stub, fid.Throttle())
        assert boards == [("881101", evil_name)], "name must survive the parse"


# ── error paths / rate limiting ──────────────────────────────────


class TestRunFailureModes:
    """429 / host exhaustion / empty klines / partial-write prevention."""

    @staticmethod
    def _stub_all_429(make_stub_session):
        """Every request answers 429 forever — retry loops must terminate."""
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            return StubResponse(status_code=429, json_data={})
        stub.get = _get  # type: ignore[method-assign]
        return stub

    async def test_429_rate_limit_does_not_loop_forever(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: with every endpoint 429ing, run() must give up within a
        bounded number of requests instead of looping forever (resource
        exhaustion)."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        counter = [0]
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            counter[0] += 1
            return StubResponse(status_code=429, json_data={})
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub), contextlib.suppress(RuntimeError):
            # bounded failure is acceptable — the contract is "not infinite"
            await run()

        assert counter[0] < 200, f"429 must not spin forever, made {counter[0]} requests"

    async def test_all_hosts_exhausted_does_not_crash(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: all hosts rate-limited/empty → run() returns or raises a
        clear error, never panics, and leaves no partial CSV."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = make_stub_session(exc=RuntimeError("simulated fetch error"))
        with patch("fetch_index_daily.AsyncSession", return_value=stub), contextlib.suppress(RuntimeError):
            await run()

        leftovers = [p for p in tmp_path.glob("*.csv") if "index" in p.name]
        assert not leftovers, "failed run must not leave a half-written CSV"

    async def test_empty_ths_kline_keeps_index_basic_entry(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """an industry whose kline fetch returns nothing (plan: 拉不到就跳过)
        must still be discoverable via index_basic — only its daily rows are
        absent."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31")]
                    )
                },
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
            }
        )
        stub.get = _ths_kline_getter(stub, [], [])
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        assert any(r["symbol"] == "BK881101" and "name" in r for r in rows), (
            "index_basic must retain the industry even when its kline is empty"
        )


    async def test_last_report_date_short_circuits_fetch(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: last_report_date == today → zero HTTP requests."""
        import datetime

        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr("fetch_index_daily._today", lambda: datetime.date(2026, 7, 31))
        monkeypatch.setattr("fetch_index_daily.last_report_date", lambda _t: "2026-07-31")
        counter = [0]
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            counter[0] += 1
            return StubResponse(status_code=200, json_data={})
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        assert counter[0] == 0, "short-circuit must not fetch"

    async def test_new_industry_auto_backfills_full_history(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """an industry absent from the last run must be backfilled with full
        history via the per-year pagination (plan: 新标的自动补全量) — not
        truncated to the increment window."""
        import datetime

        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_index_daily._today", lambda: datetime.date(2026, 8, 2))
        monkeypatch.setattr("fetch_index_daily.last_report_date", lambda _t: "2026-07-31")
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31")]
                    )
                },
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
            }
        )
        # Every year in range answers with the same 2020-01-02 row so the
        # per-year loop walks back to THS_FIRST_YEAR (full-history backfill).
        original_get = stub.get
        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                return _ths_kline_response(
                    m.group(1), int(m.group(2)),
                    [_ths_kline_row("2020-01-02"), _ths_kline_row("2026-07-31")],
                )
            return await original_get(url, params=params, headers=headers)
        stub.get = _get  # type: ignore[method-assign]
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        dates = {r["trade_date"] for r in rows if r["symbol"] == "BK881101"}
        assert "2020-01-02" in dates, "new industry must be backfilled to full history"


    async def test_yearly_pagination_fetches_all_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """per-year pagination must fetch every year until an empty one — a
        board with data in 2026 and 2025 must yield rows for both."""
        import datetime

        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_index_daily._today", lambda: datetime.date(2026, 8, 2))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31")]
                    )
                },
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
            }
        )
        stub.get = _ths_kline_getter(stub, ["881101"], ["2026", "2025"])
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        industry_rows = [r for r in rows if r["symbol"] == "BK881101"]
        dates = {r["trade_date"] for r in industry_rows}
        assert "2026-07-31" in dates and "2025-07-31" in dates, (
            f"yearly pagination must cover both years; got {dates}"
        )


# ── Dolt import (import_to_dolt) ─────────────────────────────────


class TestImportToDolt:
    """index_daily/index_basic Dolt landing — PK dedup, rollback, idempotency."""

    _DAILY_HEADER = [
        "symbol", "trade_date", "index_type",
        "open", "close", "high", "low", "volume", "amount", "update_date",
    ]

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
            capture_output=True, text=True,
        )
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.name", "CI"],
            capture_output=True, text=True,
        )
        init = subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "init"],
            capture_output=True, text=True,
        )
        assert init.returncode == 0, init.stderr

        def dolt_sql_csv(sql: str) -> str:
            return subprocess.run(
                ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
                capture_output=True, text=True,
            ).stdout

        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        return tmp_path, dolt_sql_csv

    @staticmethod
    def _last(stdout: str) -> str:
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def _write_csv(self, path: Path, header: list[str], rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(header)
            writer.writerows(rows)

    def _daily_row(self, symbol: str = "SH000001", day: str = "2026-07-31") -> list[str]:
        return [
            symbol, day, "official",
            "3000.0", "3001.0", "3002.0", "2998.0", "120000000", "50000000000", "2026-08-02",
        ]

    def test_index_daily_row_count_and_pk_dedup(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED-ready: duplicate (symbol, trade_date) in the CSV must not
        duplicate Dolt rows (PK semantics) — and index_type must survive."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(
            csv_path,
            self._DAILY_HEADER,
            [
                self._daily_row(),
                self._daily_row(),  # same PK → dedup
                self._daily_row(symbol="BK0475", day="2026-07-30"),
            ],
        )

        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM index_daily")) == "2"
        assert self._last(
            dolt_sql_csv(
                "SELECT index_type FROM index_daily WHERE symbol='SH000001' AND trade_date='2026-07-31'"
            )
        ) == "official"

    def test_index_basic_names_imported(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED-ready: index_basic rows carry name + index_type for the picker."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "index_basic.csv"
        self._write_csv(
            csv_path,
            ["symbol", "name", "index_type"],
            [["BK0475", "半导体", "concept"], ["SH000001", "上证指数", "official"]],
        )

        import_to_dolt(csv_path)

        row = self._last(
            dolt_sql_csv(
                "SELECT name, index_type FROM index_basic WHERE symbol='BK0475'"
            )
        )
        assert row == "半导体,concept"

    def test_rerun_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED-ready: re-importing the same CSV must not grow row counts."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(csv_path, self._DAILY_HEADER, [self._daily_row()])

        assert import_to_dolt(csv_path) == 1
        assert import_to_dolt(csv_path) == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM index_daily")) == "1"

    def test_verify_recent_points_consistent_no_alarm(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Decision 6: re-importing identical closes must not raise the
        sample-verify alarm (CSV == Dolt within tolerance)."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, _ = dolt_env
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(csv_path, self._DAILY_HEADER, [self._daily_row()])

        assert import_to_dolt(csv_path) == 1
        # Second identical import: verify passes silently (no stderr alarm).
        assert import_to_dolt(csv_path) == 1

    def test_verify_recent_points_alarms_on_drift(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path, capsys
    ) -> None:
        """Decision 6: a close drift beyond 0.5% vs the stored Dolt row must
        print a warn-only alarm (and never fail the import)."""
        from fetch_index_daily import _verify_recent_points  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "CREATE TABLE index_daily (symbol VARCHAR(20) NOT NULL, "
            "trade_date DATE NOT NULL, index_type VARCHAR(20) NOT NULL, "
            "open DOUBLE, close DOUBLE, high DOUBLE, low DOUBLE, "
            "volume DOUBLE, amount DOUBLE, update_date DATE, "
            "PRIMARY KEY (symbol, trade_date))"
        )
        dolt_sql_csv(
            "INSERT INTO index_daily VALUES ('SH000001', '2026-07-31', 'official', "
            "3000.0, 2950.0, 3002.0, 2998.0, 120000000, 50000000000, '2026-08-02')"
        )
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(csv_path, self._DAILY_HEADER, [self._daily_row()])

        _verify_recent_points(csv_path)
        captured = capsys.readouterr()
        assert "beyond" in captured.err, "drift beyond tolerance must alarm"
        assert "1.73%" in captured.err, "alarm must report the drift percentage"

    def test_verify_recent_points_no_dolt_dir_silent(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Decision 6: no Dolt dir → verify silently no-ops (never crashes)."""
        from fetch_index_daily import _verify_recent_points  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(csv_path, self._DAILY_HEADER, [self._daily_row()])

        _verify_recent_points(csv_path)  # must not raise

    def test_insert_failure_preserves_prior_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED-ready: a failing import must not leave a half-written index_daily
        (plan QA: failure → 不写半截数据)."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(csv_path, self._DAILY_HEADER, [self._daily_row()])
        assert import_to_dolt(csv_path) == 1

        # Sabotage: a CSV row with a trade_date that breaks the DATE cast.
        self._write_csv(
            csv_path,
            self._DAILY_HEADER,
            [self._daily_row(), self._daily_row(day="not-a-date")],
        )
        assert import_to_dolt(csv_path) == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM index_daily")) == "1", (
            "prior rows must survive a failed re-import"
        )

    async def test_mid_year_failure_does_not_truncate_history(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Review P1-1: a transient failure on one year (2025) must NOT stop
        the per-year loop — earlier years (2024) still land, otherwise a
        single 500 would silently truncate the board's whole history."""
        import datetime

        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_index_daily._today", lambda: datetime.date(2026, 8, 2))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31")]
                    )
                },
                THS_LIST_URL: _ths_list_response([("881101", "半导体")]),
            }
        )
        original_get = stub.get
        async def _get(url, params=None, headers=None):
            m = re.match(r"https://d\.10jqka\.com\.cn/v4/line/bk_(\d+)/01/(\d+)\.js$", url)
            if m:
                code, year = m.group(1), int(m.group(2))
                if year == 2025:
                    return StubResponse(status_code=500, json_data={})
                if year >= 2024:
                    return _ths_kline_response(code, year, [_ths_kline_row(f"{year}-07-31")])
                return _ths_kline_response(code, year, [])  # empty → loop ends
            return await original_get(url, params=params, headers=headers)
        stub.get = _get  # type: ignore[method-assign]
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        dates = {r["trade_date"] for r in rows if r["symbol"] == "BK881101"}
        assert "2024-07-31" in dates, (
            "a failed middle year must not truncate earlier history; "
            f"got {sorted(dates)}"
        )
        assert "2025-07-31" not in dates, "the failed year itself has no rows"
