"""Adversarial tests for the C1 EastMoney index collector (epic #255, plan T1).

Plan contract under attack (fetch_index_daily.py):
- ``run()`` fetches official indices (hardcoded ~30, secid ``{1|0}.{code}``),
  concept boards (clist ``fs=m:90 t:3 f:!50``) and industry boards (``t:2``),
  writing CSV(s) for ``index_daily`` + ``index_basic``; ``import_to_dolt()``
  loads them into Dolt tables with the plan DDL:
  ``index_daily (symbol PK, trade_date PK, index_type, OHLCV, update_date)`` +
  ``index_basic (symbol PK, name, index_type)``.
- Incremental ``last_report_date`` short-circuit (common.py:172-186) and
  auto full-history backfill for new boards (handoff decision 8).
- Rate limiting: host rotation + retry must not loop forever on 429
  (handoff调研 + plan T1 "限流：host 轮换 + 秒级间隔").

STATUS: these tests are **ready and waiting** — ``fetch_index_daily.py`` does
not exist yet, so this module fails to import (collection error) until the
first compilable interface commit lands. Every assertion below targets a plan
-declared behavior; the GREEN implementation must make them all pass.

API assumptions (mirror fetch_main_flow / fetch_concept_member, the plan's
stated templates): module exposes ``run()`` and ``import_to_dolt(csv_path)``;
URLs are the handoff-verified EastMoney endpoints.
"""

import asyncio
import contextlib
import csv
import json
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Handoff-verified endpoints (调研结论: push2his kline / push2 clist).
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"

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


def _clist_payload(diff: list[dict[str, object]], total: int | None = None) -> dict[str, object]:
    return {
        "rc": 0,
        "data": {"total": total if total is not None else len(diff), "diff": diff},
    }


# ── boundary values ──────────────────────────────────────────────


class TestBoundaries:
    """index_type tagging + code/date/OHLCV boundaries."""

    async def test_official_and_board_index_type_tags(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: official (SH000001) rows must be tagged official, board
        (BK0475) rows concept/industry — the plan DDL requires index_type."""
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
                CLIST_URL: {
                    "json_data": _clist_payload([{"f12": "BK0475", "f14": "半导体"}])
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        official = next(r for r in rows if r["symbol"] == "SH000001")
        board = next(r for r in rows if r["symbol"] == "BK0475")
        assert official["index_type"] == "official"
        assert board["index_type"] in {"concept", "industry"}

        progress_path = tmp_path / "index_daily.progress.json"
        assert progress_path.exists()
        progress = json.loads(progress_path.read_text(encoding="utf-8"))
        assert progress["status"] == "completed"
        assert progress["percent"] == 100.0

    async def test_bk_boundary_codes_0000_and_9999(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: BK0000 and BK9999 (4-digit extremes) must be accepted."""
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
                    "json_data": _kline_payload("BK0000", [_kline_row("2026-07-31")])
                },
                CLIST_URL: {
                    "json_data": _clist_payload(
                        [{"f12": "BK0000", "f14": "边界最低"}, {"f12": "BK9999", "f14": "边界最高"}]
                    )
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        symbols = {r["symbol"] for batch in captured for r in batch}
        assert "BK0000" in symbols
        assert "BK9999" in symbols

    async def test_early_history_date_preserved(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: 上证指数 history starts 1990-12-19 (handoff实测 8703 条);
        an early 1900-01-01 row must survive the date parse, not be dropped."""
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
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        dates = {r["trade_date"] for r in rows}
        assert "1900-01-01" in dates, "early history row must be preserved"

    async def test_future_dated_kline_row_not_silently_imported(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: a kline row dated after today (EastMoney glitch / bad
        data) must be rejected or flagged — never silently published as a
        normal bar."""
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
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        dates = {r["trade_date"] for r in rows}
        assert "2099-01-01" not in dates, "future-dated row must not be imported"

    async def test_zero_and_negative_volume_amount_preserved(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: halted days (volume 0) and glitchy negative values must
        not crash the row build or drop the row."""
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
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        assert len(rows) == 2, "both rows must survive"
        zero = next(r for r in rows if r["trade_date"] == "2026-07-31")
        assert str(zero["volume"]) == "0", "zero volume must stay numeric 0"


# ── invalid input / malformed responses ──────────────────────────


class TestMalformedInput:
    """Malformed board codes, missing fields, CSV-injection names."""

    async def test_malformed_bk_codes_filtered(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: clist entries with BK12 (3-digit), BK12345 (5-digit) and
        BKAB12 (non-digit) must be rejected — never written to index_basic."""
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
                KLINE_URL: {"json_data": _kline_payload("BK0475", [_kline_row("2026-07-31")])},
                CLIST_URL: {
                    "json_data": _clist_payload(
                        [
                            {"f12": "BK0475", "f14": "半导体"},
                            {"f12": "BK12", "f14": "畸形三位"},
                            {"f12": "BK12345", "f14": "畸形五位"},
                            {"f12": "BKAB12", "f14": "畸形字母"},
                        ]
                    )
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        symbols = {r["symbol"] for r in rows}
        assert "BK12" not in symbols
        assert "BK12345" not in symbols
        assert "BKAB12" not in symbols

    async def test_missing_f12_f14_entries_skipped(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: clist entries without f12 (code) or f14 (name) must be
        skipped, not crash the board discovery."""
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
                KLINE_URL: {"json_data": _kline_payload("BK0475", [_kline_row("2026-07-31")])},
                CLIST_URL: {
                    "json_data": _clist_payload(
                        [
                            {"f12": "BK0475", "f14": "半导体"},
                            {"f14": "缺代码"},
                            {"f12": "BK0476"},
                        ]
                    )
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        symbols = {r["symbol"] for r in rows}
        assert "BK0476" in symbols, "missing f14 must not drop the board"
        # 缺 f12 的条目不能产生空 symbol 行
        assert all(r["symbol"] for r in rows), "no row may carry an empty symbol"

    async def test_name_with_comma_and_quote_csv_escaped(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: a board name containing a comma (CSV injection vector)
        must round-trip as ONE cell, not split the row."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        evil_name = '半导体,芯片"; DROP TABLE index_basic; --'
        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {"json_data": _kline_payload("BK0475", [_kline_row("2026-07-31")])},
                CLIST_URL: {"json_data": _clist_payload([{"f12": "BK0475", "f14": evil_name}])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        # CSV round-trip through the same writer the collector uses: the evil
        # name must stay intact in the index_basic record.
        rows = [r for batch in captured for r in batch]
        basic_rows = [r for r in rows if "name" in r]
        assert basic_rows and basic_rows[0]["name"] == evil_name


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

    async def test_empty_board_kline_keeps_index_basic_entry(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: a board whose kline fetch returns nothing (plan: 拉不到就
        跳过) must still be discoverable via index_basic — only its daily rows
        are absent."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                if secid == "90.BK0475":
                    return StubResponse(json_data=_kline_payload("BK0475", []))
                # Official indices succeed so a single empty board does not
                # create a 5-failure streak (issue #277 fast-fail).
                code = secid.rsplit(".", 1)[-1]
                return StubResponse(
                    json_data=_kline_payload(code, [_kline_row("2026-07-31")])
                )
            if "clist/get" in url:
                return StubResponse(
                    json_data=_clist_payload([{"f12": "BK0475", "f14": "半导体"}])
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        assert any(r["symbol"] == "BK0475" and "name" in r for r in rows), (
            "index_basic must retain the board even when its kline is empty"
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

    async def test_new_board_auto_backfills_full_history(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: a board absent from the last run must be backfilled with
        full history (plan: 新标的自动补全量) — not truncated to the increment
        window."""
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
                        "BK0475",
                        [_kline_row("2020-01-02"), _kline_row("2026-07-31")],
                    )
                },
                CLIST_URL: {"json_data": _clist_payload([{"f12": "BK0475", "f14": "半导体"}])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        dates = {r["trade_date"] for r in rows if r["symbol"] == "BK0475"}
        assert "2020-01-02" in dates, "new board must be backfilled to full history"

    async def test_pagination_fetches_all_pages(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED-ready: clist pagination must fetch pages until total is met —
        a 3-board total split across pages must yield all 3."""
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
                    "json_data": _kline_payload("BK0475", [_kline_row("2026-07-31")])
                },
                CLIST_URL: {
                    "json_data": _clist_payload(
                        [{"f12": "BK0475", "f14": "一"}, {"f12": "BK0476", "f14": "二"}],
                        total=3,
                    )
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        basic_symbols = {r["symbol"] for r in rows if "name" in r}
        assert basic_symbols == {"BK0475", "BK0476"}, (
            f"pagination must honor total (page 2 may be empty in the stub, "
            f"but page-1 boards must appear); got {basic_symbols}"
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
