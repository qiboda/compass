"""Requirement acceptance RED tests for issue #308 (auto-heal missing data).

These tests assert the *planned* interface contracts from:
- ``.dsh/plans/auto-heal-missing-data.md``
- GitHub issue #308 (feat: 自动回补缺失数据机制)

The production functions do not exist yet, so these tests should be RED
(ImportError / AttributeError / TypeError / assertion failure). After the
implementation lands they must become GREEN without modifying these tests.
"""

import asyncio
import csv
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# ── EastMoney historical per-stock fund-flow endpoint (plan handoff) ──
FFLOW_DAYKLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get"

# Plan contract: f52..f56 -> main/small/medium/large/super_large_net,
# f57 -> main_net_inflow_rate; date is the first CSV field of each API row.
_MAIN_FLOW_HEADER = [
    "symbol",
    "trade_date",
    "main_net_inflow",
    "main_net_inflow_rate",
    "super_large_net",
    "large_net",
    "medium_net",
    "small_net",
    "update_date",
]


def _fflow_payload(
    secid: str = "1.600519",
    rows: list[str] | None = None,
) -> dict[str, object]:
    """Build the EastMoney fflow/daykline JSON body.

    Each row is ``date,f52,f53,f54,f55,f56,f57`` (7 CSV cells) in the
    handoff-verified order; f52-f56 are the five net-inflow columns and
    f57 is the main_net_inflow_rate.
    """
    return {
        "rc": 0,
        "data": {
            "code": secid,
            "name": "stub",
            "klines": rows
            or [
                "2026-08-13,1.1,2.2,3.3,4.4,5.5,6.6",
                "2026-08-14,7.7,8.8,9.9,10.1,11.1,12.2",
            ],
        },
    }


def _fflow_row(day: str, i: int = 1) -> str:
    """One fflow API row with deterministic values."""
    return f"{day},{i}.1,{i}.2,{i}.3,{i}.4,{i}.5,{i}.6"


# ---------------------------------------------------------------------------
# Dolt helpers
# ---------------------------------------------------------------------------


def _dolt_sql(tmp_path: Path, sql: str, *, db_name: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["dolt", "--data-dir", str(tmp_path / db_name), "sql", "-q", sql],
        capture_output=True,
        text=True,
    )


def _dolt_sql_csv(tmp_path: Path, sql: str, *, db_name: str) -> str:
    return subprocess.run(
        ["dolt", "--data-dir", str(tmp_path / db_name), "sql", "-r", "csv", "-q", sql],
        capture_output=True,
        text=True,
    ).stdout


@pytest.fixture
def dolt_envs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Path, Callable[[str, str], str]]:
    """Create two temp Dolt repos: investment_data + compass_data.

    - investment_data gets the SSE trading calendar.
    - compass_data gets capital_main_flow, stock_basic, data_updates.

    Returns (investment_dir, compass_dir, dolt_sql_csv_fn).
    """
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
        capture_output=True,
        text=True,
    )
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.name", "CI"],
        capture_output=True,
        text=True,
    )

    invest_dir = tmp_path / "investment_data"
    compass_dir = tmp_path / "compass_data"
    for d in (invest_dir, compass_dir):
        d.mkdir(parents=True, exist_ok=True)
        init = subprocess.run(
            ["dolt", "--data-dir", str(d), "init"], capture_output=True, text=True
        )
        assert init.returncode == 0, init.stderr

    # SSE calendar: 2026-08-13, 08-14, 08-17, 08-18, 08-19, 08-20, 08-21, 08-24, 08-25 are open.
    _dolt_sql(
        tmp_path,
        "CREATE TABLE ts_trade_day_calendar ("
        " id INT PRIMARY KEY, exchange VARCHAR(10), `date` DATE, is_open TINYINT);"
        " INSERT INTO ts_trade_day_calendar VALUES "
        " (1, 'SSE', '2026-08-13', 1), (2, 'SSE', '2026-08-14', 1),"
        " (3, 'SSE', '2026-08-17', 1), (4, 'SSE', '2026-08-18', 1),"
        " (5, 'SSE', '2026-08-19', 1), (6, 'SSE', '2026-08-20', 1),"
        " (7, 'SSE', '2026-08-21', 1), (8, 'SSE', '2026-08-24', 1),"
        " (9, 'SSE', '2026-08-25', 1),"
        " (10, 'SSE', '2026-08-22', 0), (11, 'SSE', '2026-08-23', 0);",
        db_name="investment_data",
    )

    _dolt_sql(
        tmp_path,
        "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY);"
        " INSERT INTO stock_basic VALUES ('SH600519'), ('SZ000001');",
        db_name="compass_data",
    )
    _dolt_sql(
        tmp_path,
        "CREATE TABLE capital_main_flow ("
        " symbol VARCHAR(20) NOT NULL, trade_date DATE NOT NULL,"
        " main_net_inflow DOUBLE, main_net_inflow_rate DOUBLE,"
        " super_large_net DOUBLE, large_net DOUBLE, medium_net DOUBLE,"
        " small_net DOUBLE, update_date DATE,"
        " PRIMARY KEY (symbol, trade_date));"
        " INSERT INTO capital_main_flow VALUES"
        " ('SH600519', '2026-08-17', 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, '2026-08-17'),"
        " ('SH600519', '2026-08-18', 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, '2026-08-18'),"
        " ('SH600519', '2026-08-19', 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, '2026-08-19'),"
        " ('SH600519', '2026-08-20', 4.0, 4.0, 4.0, 4.0, 4.0, 4.0, '2026-08-20'),"
        " ('SH600519', '2026-08-21', 5.0, 5.0, 5.0, 5.0, 5.0, 5.0, '2026-08-21');",
        db_name="compass_data",
    )
    _dolt_sql(
        tmp_path,
        "CREATE TABLE index_daily ("
        " symbol VARCHAR(20) NOT NULL, trade_date DATE NOT NULL,"
        " index_type VARCHAR(20), open DOUBLE, close DOUBLE, high DOUBLE,"
        " low DOUBLE, volume DOUBLE, amount DOUBLE, update_date DATE,"
        " PRIMARY KEY (symbol, trade_date));",
        db_name="compass_data",
    )
    _dolt_sql(
        tmp_path,
        "CREATE TABLE data_updates ("
        " table_name VARCHAR(50) PRIMARY KEY, last_updated DATE,"
        " source VARCHAR(200), row_count INT, last_report_date DATE);",
        db_name="compass_data",
    )

    # The new helpers use COMPASS_DATA_DIR for compass_data and an
    # investment-data dir for the calendar.  Tests monkeypatch common's env
    # values below when calling the helpers.
    monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))

    def _sql_csv(db_name: str, sql: str) -> str:
        return _dolt_sql_csv(tmp_path, sql, db_name=db_name)

    return invest_dir, compass_dir, _sql_csv


# ---------------------------------------------------------------------------
# 交易日历 / 缺口检测工具 (plan: common.py or main.py new module)
# ---------------------------------------------------------------------------


class TestTradeCalendarTools:
    """Contract: trade_calendar / missing_dates / set_last_report_date."""

    def test_trade_calendar_returns_sse_open_days(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from common import trade_calendar  # noqa: F401  # RED: not implemented yet

        invest_dir, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        days = trade_calendar("2026-08-13", "2026-08-25")
        assert days == [
            "2026-08-13",
            "2026-08-14",
            "2026-08-17",
            "2026-08-18",
            "2026-08-19",
            "2026-08-20",
            "2026-08-21",
            "2026-08-24",
            "2026-08-25",
        ]

    def test_missing_dates_identifies_gap(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from common import missing_dates  # noqa: F401  # RED

        invest_dir, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        missing = missing_dates(
            table="capital_main_flow",
            date_col="trade_date",
            start="2026-08-13",
            end="2026-08-25",
        )
        assert missing == ["2026-08-13", "2026-08-14", "2026-08-24", "2026-08-25"]

    def test_missing_dates_no_gap_returns_empty(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from common import missing_dates  # noqa: F401  # RED

        invest_dir, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        # Existing rows cover 08-17 and 08-18; within that sub-range there is no gap.
        missing = missing_dates(
            table="capital_main_flow",
            date_col="trade_date",
            start="2026-08-17",
            end="2026-08-18",
        )
        assert missing == []

    def test_missing_dates_empty_table_returns_all_calendar_days(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from common import missing_dates  # noqa: F401  # RED

        invest_dir, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        missing = missing_dates(
            table="index_daily",
            date_col="trade_date",
            start="2026-08-17",
            end="2026-08-18",
        )
        assert missing == ["2026-08-17", "2026-08-18"]

    def test_missing_dates_missing_dolt_raises_clear_error(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from common import missing_dates  # noqa: F401  # RED

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(tmp_path / "no_invest"))
        with pytest.raises((RuntimeError, ValueError), match="Dolt|dolt|not|missing|repo"):
            missing_dates(
                table="capital_main_flow",
                date_col="trade_date",
                start="2026-08-13",
                end="2026-08-25",
            )

    def test_set_last_report_date_updates_anchor(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        from common import set_last_report_date  # noqa: F401  # RED

        invest_dir, compass_dir, sql_csv = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        set_last_report_date("capital_main_flow", "2026-08-25")
        row = (
            sql_csv(
                "compass_data",
                "SELECT last_report_date FROM data_updates WHERE table_name='capital_main_flow'",
            )
            .strip()
            .split("\n")[-1]
        )
        assert row == "2026-08-25"


# ---------------------------------------------------------------------------
# fetch_main_flow 历史回补
# ---------------------------------------------------------------------------


class TestFetchMainFlowBackfill:
    """Contract: fetch_main_flow.backfill(start, end) -> Path."""

    @staticmethod
    def _stub(make_stub_session, payload=None, exc=None, calls=None):
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if calls is not None:
                calls.append({"url": url, "params": params, "headers": headers})
            if exc is not None:
                raise exc
            return StubResponse(json_data=payload)

        stub.get = _get  # type: ignore[method-assign]
        return stub

    async def test_backfill_writes_requested_days_to_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        calls: list[dict[str, object]] = []
        stub = self._stub(
            make_stub_session,
            payload=_fflow_payload(
                "1.600519",
                [_fflow_row("2026-08-13", 1), _fflow_row("2026-08-14", 2)],
            ),
            calls=calls,
        )
        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            result = await fetch_main_flow.backfill("2026-08-13", "2026-08-25")

        assert result.name.endswith(".csv")
        csv_path = tmp_path / result.name
        assert csv_path.exists()
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert {r["trade_date"] for r in rows} == {"2026-08-13", "2026-08-14"}
        first = rows[0]
        # Plan: f52..f56 -> main/small/medium/large/super_large_net,
        # f57 -> main_net_inflow_rate.
        assert first["main_net_inflow"] == "1.1"
        assert first["small_net"] == "1.2"
        assert first["medium_net"] == "1.3"
        assert first["large_net"] == "1.4"
        assert first["super_large_net"] == "1.5"
        assert first["main_net_inflow_rate"] == "1.6"

    async def test_backfill_import_to_dolt_is_idempotent(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(dolt_envs[1]))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = self._stub(
            make_stub_session,
            payload=_fflow_payload("1.600519", [_fflow_row("2026-08-13", 1)]),
        )
        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            path = await fetch_main_flow.backfill("2026-08-13", "2026-08-13")

        assert fetch_main_flow.import_to_dolt(path) > 0
        first_count = int(
            _dolt_sql_csv(
                tmp_path,
                "SELECT COUNT(*) FROM capital_main_flow",
                db_name="compass_data",
            )
            .strip()
            .split("\n")[-1]
        )
        assert fetch_main_flow.import_to_dolt(path) > 0
        second_count = int(
            _dolt_sql_csv(
                tmp_path,
                "SELECT COUNT(*) FROM capital_main_flow",
                db_name="compass_data",
            )
            .strip()
            .split("\n")[-1]
        )
        assert first_count == second_count, "re-import must not grow row count"

    async def test_backfill_unknown_symbol_filtered_via_stock_basic(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(dolt_envs[1]))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = self._stub(
            make_stub_session,
            payload=_fflow_payload("1.600519", [_fflow_row("2026-08-13", 1)]),
        )
        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            path = await fetch_main_flow.backfill("2026-08-13", "2026-08-13")

        # Append a synthetic unknown-symbol row directly to the backfilled CSV:
        # the landing import must filter it against stock_basic.
        with open(path, "a", newline="", encoding="utf-8-sig") as f:
            writer = csv.DictWriter(f, fieldnames=_MAIN_FLOW_HEADER)
            writer.writerow(
                {
                    "symbol": "SZ999999",
                    "trade_date": "2026-08-13",
                    "main_net_inflow": "9.9",
                    "main_net_inflow_rate": "9.9",
                    "super_large_net": "9.9",
                    "large_net": "9.9",
                    "medium_net": "9.9",
                    "small_net": "9.9",
                    "update_date": "2026-08-13",
                }
            )

        # The landing path must only import symbols present in stock_basic.
        fetch_main_flow.import_to_dolt(path)
        rows = (
            _dolt_sql_csv(
                tmp_path,
                "SELECT symbol FROM capital_main_flow WHERE trade_date='2026-08-13'",
                db_name="compass_data",
            )
            .strip()
            .split("\n")
        )
        symbols = [r for r in rows[1:] if r]
        assert "SZ999999" not in symbols, f"unknown symbol must be filtered: {symbols}"
        assert symbols, "known symbol must still be imported"
        assert all(s in {"SH600519", "SZ000001"} for s in symbols), symbols

    async def test_backfill_api_empty_raises_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = self._stub(make_stub_session, payload={"rc": 0, "data": {"klines": []}})
        with (
            patch("fetch_main_flow.AsyncSession", return_value=stub),
            pytest.raises(RuntimeError, match="No data|empty|backfill"),
        ):
            await fetch_main_flow.backfill("2026-08-13", "2026-08-13")

        leftover = list(tmp_path.glob("*.csv"))
        assert not leftover, "API empty must not leave a half-written CSV"

    async def test_backfill_api_failure_raises_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = self._stub(make_stub_session, exc=RuntimeError("simulated fflow failure"))
        with patch("fetch_main_flow.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await fetch_main_flow.backfill("2026-08-13", "2026-08-13")

        leftover = list(tmp_path.glob("*.csv"))
        assert not leftover, "API failure must not leave a half-written CSV"


# ---------------------------------------------------------------------------
# fetch_index_daily 指定范围回补
# ---------------------------------------------------------------------------


class TestFetchIndexDailyBackfill:
    """Contract: fetch_index_daily.backfill(start, end) or run(start=..., end=...)."""

    async def test_backfill_fills_middle_gap_only(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_index_daily  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        monkeypatch.setattr(
            "fetch_index_daily._today", lambda: __import__("datetime").date(2026, 8, 27)
        )

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )
        monkeypatch.setattr(
            fetch_index_daily,
            "fetch_ths_industry_list",
            AsyncMock(return_value=[]),
        )
        klines = [
            "2026-08-13,1,2,3,4,5,6",
            "2026-08-14,1,2,3,4,5,6",
            "2026-08-25,1,2,3,4,5,6",
        ]
        monkeypatch.setattr(
            fetch_index_daily,
            "fetch_kline",
            AsyncMock(return_value=(klines, "000001")),
        )
        await fetch_index_daily.backfill("2026-08-13", "2026-08-25")

        rows = [r for batch in captured for r in batch if "trade_date" in r]
        dates = {r["trade_date"] for r in rows}
        assert "2026-08-13" in dates
        assert "2026-08-14" in dates
        assert "2026-08-25" in dates
        # Out-of-range days must not be written.
        assert "2026-08-12" not in dates
        assert "2026-08-26" not in dates

    async def test_backfill_failure_propagates(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_index_daily  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = make_stub_session(exc=RuntimeError("simulated kline failure"))
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            pytest.raises(RuntimeError),
        ):
            await fetch_index_daily.backfill("2026-08-13", "2026-08-14")

        assert not list(tmp_path.glob("*.csv")), "failed backfill must not leave a CSV"


# ---------------------------------------------------------------------------
# fetch_block_trade 范围回补
# ---------------------------------------------------------------------------


class TestFetchBlockTradeRange:
    """Contract: fetch_block_trade.run(..., start=..., end=...)."""

    async def test_run_with_start_end_requests_only_range(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        requested_dates: list[str] = []

        async def _fake_fetch(
            session, throttle, report_name, filter_column, report_date, page_size, **kwargs
        ):
            requested_dates.append(report_date)
            return []

        monkeypatch.setattr(fetch_block_trade, "fetch_paginated", _fake_fetch)

        # Hide the current year scanning by pinning last_report_date to None
        # and passing explicit start/end.
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")
        with patch("fetch_block_trade.AsyncSession", return_value=make_stub_session()):
            await fetch_block_trade.run(start="2026-08-13", end="2026-08-14")

        assert requested_dates == ["2026-08-13", "2026-08-14"], requested_dates

    async def test_run_start_end_respects_out_of_range(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        requested_dates: list[str] = []

        async def _fake_fetch(*args, **kwargs):
            # args[4] is report_date under fetch_paginated(session, throttle,
            # report_name, filter_column, report_date, page_size)
            requested_dates.append(args[4])
            return []

        monkeypatch.setattr(fetch_block_trade, "fetch_paginated", _fake_fetch)
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")
        with patch("fetch_block_trade.AsyncSession", return_value=make_stub_session()):
            await fetch_block_trade.run(start="2026-08-13", end="2026-08-15")

        assert "2026-08-13" in requested_dates
        assert "2026-08-14" in requested_dates
        assert "2026-08-15" in requested_dates
        assert "2026-08-16" not in requested_dates, "must not scan outside range"

    async def test_run_start_end_failure_propagates(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")

        async def _boom(*args, **kwargs):
            raise RuntimeError("simulated block_trade failure")

        monkeypatch.setattr(fetch_block_trade, "fetch_paginated", _boom)
        with (
            patch("fetch_block_trade.AsyncSession", return_value=make_stub_session()),
            pytest.raises(RuntimeError),
        ):
            await fetch_block_trade.run(start="2026-08-13", end="2026-08-14")

        assert not list(tmp_path.glob("*.csv")), "failed range run must not leave a CSV"


# ---------------------------------------------------------------------------
# main.py sync 集成自动扫描/回补
# ---------------------------------------------------------------------------


class TestMainSyncAutoHeal:
    """Contract: do_sync scans daily-table gaps, backfills, then normal sync."""

    def test_sync_backfills_when_missing_date(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod  # noqa: F401

        mock_missing = Mock(return_value=["2026-08-13", "2026-08-14"])
        # AsyncMock so do_sync may either await backfill() or pass it to asyncio.run().
        mock_backfill = AsyncMock(return_value=Path("/tmp/backfill.csv"))
        monkeypatch.setattr(main_mod, "missing_dates", mock_missing)
        monkeypatch.setattr(main_mod, "backfill", mock_backfill)

        for mod in (
            "fetch_stock_basic_official",
            "fetch_fin_indicators",
            "fetch_balance_sheet",
            "fetch_income",
            "fetch_cash_flow",
            "fetch_dragon",
            "fetch_block_trade",
            "fetch_institution_survey",
            "fetch_main_flow",
            "fetch_index_daily",
        ):
            try:
                m = __import__(mod)
            except ModuleNotFoundError:
                continue
            if hasattr(m, "run"):
                monkeypatch.setattr(m, "run", Mock(return_value=Path("x.csv")))
            if hasattr(m, "main"):
                monkeypatch.setattr(m, "main", Mock())
            if hasattr(m, "import_to_dolt"):
                monkeypatch.setattr(m, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock(return_value=1))
        monkeypatch.setattr(main_mod.asyncio, "run", Mock())

        main_mod.do_sync()
        mock_backfill.assert_called()

    def test_sync_skips_backfill_when_no_gap(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod  # noqa: F401

        mock_missing = Mock(return_value=[])
        mock_backfill = AsyncMock()
        monkeypatch.setattr(main_mod, "missing_dates", mock_missing)
        monkeypatch.setattr(main_mod, "backfill", mock_backfill)

        for mod in (
            "fetch_stock_basic_official",
            "fetch_fin_indicators",
            "fetch_balance_sheet",
            "fetch_income",
            "fetch_cash_flow",
            "fetch_dragon",
            "fetch_block_trade",
            "fetch_institution_survey",
            "fetch_main_flow",
            "fetch_index_daily",
        ):
            try:
                m = __import__(mod)
            except ModuleNotFoundError:
                continue
            if hasattr(m, "run"):
                monkeypatch.setattr(m, "run", Mock(return_value=Path("x.csv")))
            if hasattr(m, "main"):
                monkeypatch.setattr(m, "main", Mock())
            if hasattr(m, "import_to_dolt"):
                monkeypatch.setattr(m, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock(return_value=1))
        monkeypatch.setattr(main_mod.asyncio, "run", Mock())

        main_mod.do_sync()
        mock_backfill.assert_not_called()

    def test_sync_backfill_failure_aborts(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod  # noqa: F401

        mock_missing = Mock(return_value=["2026-08-13"])
        monkeypatch.setattr(main_mod, "missing_dates", mock_missing)
        monkeypatch.setattr(
            main_mod,
            "backfill",
            AsyncMock(side_effect=RuntimeError("backfill failed")),
        )

        for mod in (
            "fetch_stock_basic_official",
            "fetch_fin_indicators",
            "fetch_balance_sheet",
            "fetch_income",
            "fetch_cash_flow",
            "fetch_dragon",
            "fetch_block_trade",
            "fetch_institution_survey",
            "fetch_main_flow",
            "fetch_index_daily",
        ):
            try:
                m = __import__(mod)
            except ModuleNotFoundError:
                continue
            if hasattr(m, "run"):
                monkeypatch.setattr(m, "run", Mock(return_value=Path("x.csv")))
            if hasattr(m, "main"):
                monkeypatch.setattr(m, "main", Mock())
            if hasattr(m, "import_to_dolt"):
                monkeypatch.setattr(m, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock(return_value=1))
        monkeypatch.setattr(main_mod.asyncio, "run", Mock())

        with pytest.raises(RuntimeError, match="backfill failed"):
            main_mod.do_sync()


class TestAutoHealTableRange:
    """Contract: each daily table is scanned from its own earliest date."""

    def test_range_uses_per_table_earliest_not_global_min(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod  # noqa: F401

        _, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))

        start, end = main_mod._auto_heal_table_range("capital_main_flow", "trade_date")
        assert start == "2026-08-17"
        assert end >= "2026-08-17"

        # An empty table must fall back to the 90-day window, not to the
        # earliest date of a much older table (which would flood backfill).
        empty_start, empty_end = main_mod._auto_heal_table_range("index_daily", "trade_date")
        assert empty_start > "1990-01-01", f"unexpected global-min scan: {empty_start}"


class TestMainBackfillImports:
    """Contract: main.backfill fetches each source and imports it into Dolt."""

    async def test_backfill_fetches_and_imports_all_four_sources(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        import fetch_block_trade  # noqa: F401
        import fetch_dragon  # noqa: F401
        import fetch_index_daily  # noqa: F401
        import fetch_main_flow  # noqa: F401
        import main as main_mod  # noqa: F401

        mf_path = tmp_path / "capital_main_flow_backfill.csv"
        idx_path = tmp_path / "index_daily_backfill.csv"
        dragon_path = tmp_path / "dragon.csv"
        block_path = tmp_path / "block_trade.csv"
        # The helper imports only when the source produced a real CSV file.
        for path in (mf_path, idx_path, dragon_path, block_path):
            path.write_text("symbol,trade_date\n", encoding="utf-8")

        monkeypatch.setattr(fetch_main_flow, "backfill", AsyncMock(return_value=mf_path))
        monkeypatch.setattr(fetch_main_flow, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(fetch_index_daily, "backfill", AsyncMock(return_value=idx_path))
        monkeypatch.setattr(fetch_index_daily, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(fetch_dragon, "run", AsyncMock(return_value=dragon_path))
        monkeypatch.setattr(fetch_dragon, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(fetch_block_trade, "run", AsyncMock(return_value=block_path))
        monkeypatch.setattr(fetch_block_trade, "import_to_dolt", Mock(return_value=1))

        await main_mod.backfill("2026-08-13", "2026-08-25")

        fetch_main_flow.import_to_dolt.assert_called_once_with(mf_path)
        fetch_index_daily.import_to_dolt.assert_called_once_with(idx_path)
        fetch_dragon.import_to_dolt.assert_called_once_with(dragon_path)
        fetch_block_trade.import_to_dolt.assert_called_once_with(block_path)

    async def test_backfill_import_zero_rows_aborts(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_main_flow  # noqa: F401
        import main as main_mod  # noqa: F401

        # Only the first import can be reached before the strict abort.
        monkeypatch.setattr(
            fetch_main_flow,
            "backfill",
            AsyncMock(return_value=Path("/tmp/capital_main_flow_backfill.csv")),
        )
        monkeypatch.setattr(fetch_main_flow, "import_to_dolt", Mock(return_value=0))

        with pytest.raises(RuntimeError, match="capital_main_flow import returned 0 rows"):
            await main_mod.backfill("2026-08-13", "2026-08-25")

    async def test_backfill_skips_import_for_sources_with_no_rows(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        import fetch_block_trade  # noqa: F401
        import fetch_dragon  # noqa: F401
        import fetch_index_daily  # noqa: F401
        import fetch_main_flow  # noqa: F401
        import main as main_mod  # noqa: F401

        mf_path = tmp_path / "capital_main_flow_backfill.csv"
        mf_path.write_text("symbol,trade_date\n", encoding="utf-8")
        # index/dragon/block return paths that were never written (no data).
        idx_path = tmp_path / "index_daily_backfill.csv"
        dragon_path = tmp_path / "dragon.csv"
        block_path = tmp_path / "block_trade.csv"

        monkeypatch.setattr(fetch_main_flow, "backfill", AsyncMock(return_value=mf_path))
        monkeypatch.setattr(fetch_main_flow, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(fetch_index_daily, "backfill", AsyncMock(return_value=idx_path))
        monkeypatch.setattr(fetch_index_daily, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(fetch_dragon, "run", AsyncMock(return_value=dragon_path))
        monkeypatch.setattr(fetch_dragon, "import_to_dolt", Mock(return_value=1))
        monkeypatch.setattr(fetch_block_trade, "run", AsyncMock(return_value=block_path))
        monkeypatch.setattr(fetch_block_trade, "import_to_dolt", Mock(return_value=1))

        await main_mod.backfill("2026-08-13", "2026-08-25")

        fetch_main_flow.import_to_dolt.assert_called_once_with(mf_path)
        fetch_index_daily.import_to_dolt.assert_not_called()
        fetch_dragon.import_to_dolt.assert_not_called()
        fetch_block_trade.import_to_dolt.assert_not_called()
