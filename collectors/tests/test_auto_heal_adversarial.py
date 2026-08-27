"""Adversarial RED tests for issue #308 (auto-heal missing data).

These tests attack the *declared* interfaces/behaviours from
``.dsh/plans/auto-heal-missing-data.md``. They are intentionally destructive /
boundary / error / idempotency / resource tests, distinct from the requirement
acceptance tests in ``test_auto_heal_requirement.py``. They must remain RED
until the implementation is correct, and become GREEN without being edited.

Targeted plan contracts:
- ``common.trade_calendar(start, end) -> list[str]``
- ``common.missing_dates(table, date_col, start, end) -> list[str]``
- ``common.set_last_report_date(table, date) -> None``
- ``fetch_main_flow.backfill(start, end) -> Path``
- ``fetch_index_daily.backfill(start, end) -> Path`` (or run range)
- ``fetch_block_trade.run(start=..., end=...)``
- ``main.do_sync`` auto-heal scanning (strict failure)
"""

from __future__ import annotations

import asyncio
import csv
import subprocess
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch

import pytest
from conftest import StubResponse  # noqa: E402

# Since production functions do not yet exist, import errors are valid RED.
# The tests are written against the planned API and become GREEN when the
# implementation lands.
FFLOW_DAYKLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/fflow/daykline/get"

MAIN_FLOW_HEADER = [
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
    return f"{day},{i}.1,{i}.2,{i}.3,{i}.4,{i}.5,{i}.6"


# ---------------------------------------------------------------------------
# Temp Dolt fixtures (independent from requirement file)
# ---------------------------------------------------------------------------


def _dolt_sql(tmp_path: Path, sql: str, *, db_name: str) -> None:
    r = subprocess_run(["dolt", "--data-dir", str(tmp_path / db_name), "sql", "-q", sql])
    assert r.returncode == 0, r.stderr


def _dolt_sql_csv(tmp_path: Path, sql: str, *, db_name: str) -> str:
    return subprocess_run(
        ["dolt", "--data-dir", str(tmp_path / db_name), "sql", "-r", "csv", "-q", sql]
    ).stdout


def subprocess_run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, capture_output=True, text=True)


@pytest.fixture
def dolt_envs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Path, Callable[[str, str], str]]:
    """Create temp investment_data + compass_data Dolt repos for adversarial tests.

    Returns (investment_dir, compass_dir, sql_csv_fn).
    """
    subprocess_run(["dolt", "config", "--global", "--add", "user.email", "adv@compass.local"])
    subprocess_run(["dolt", "config", "--global", "--add", "user.name", "Adversarial"])

    invest_dir = tmp_path / "investment_data"
    compass_dir = tmp_path / "compass_data"
    for d in (invest_dir, compass_dir):
        d.mkdir(parents=True, exist_ok=True)
        r = subprocess_run(["dolt", "--data-dir", str(d), "init"])
        assert r.returncode == 0, r.stderr

    # A realistic SSE calendar including weekend/holiday rows.
    _dolt_sql(
        tmp_path,
        "CREATE TABLE ts_trade_day_calendar (id INT PRIMARY KEY, exchange VARCHAR(10),"
        " `date` DATE, is_open TINYINT);"
        " INSERT INTO ts_trade_day_calendar VALUES"
        " (1, 'SSE', '2026-08-13', 1), (2, 'SSE', '2026-08-14', 1),"
        " (3, 'SZSE', '2026-08-14', 1),"
        " (4, 'SSE', '2026-08-15', 0), (5, 'SSE', '2026-08-16', 0),"
        " (6, 'SSE', '2026-08-17', 1), (7, 'SSE', '2026-08-18', 1),"
        " (8, 'SSE', '2026-08-19', 1), (9, 'SSE', '2026-08-20', 1),"
        " (10, 'SSE', '2026-08-21', 1), (11, 'SSE', '2026-08-22', 0),"
        " (12, 'SSE', '2026-08-23', 0), (13, 'SSE', '2026-08-24', 1),"
        " (14, 'SSE', '2026-08-25', 1);",
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
        " ('SH600519', '2026-08-18', 2.0, 2.0, 2.0, 2.0, 2.0, 2.0, '2026-08-18');",
        db_name="compass_data",
    )
    _dolt_sql(
        tmp_path,
        "CREATE TABLE data_updates ("
        " table_name VARCHAR(50) PRIMARY KEY, last_updated DATE,"
        " source VARCHAR(200), row_count INT, last_report_date DATE);",
        db_name="compass_data",
    )

    monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))

    def _sql_csv(db_name: str, sql: str) -> str:
        return _dolt_sql_csv(tmp_path, sql, db_name=db_name)

    return invest_dir, compass_dir, _sql_csv


# ---------------------------------------------------------------------------
# common.trade_calendar — boundary / illegal / calendar-quality attacks
# ---------------------------------------------------------------------------


class TestTradeCalendarAdversarial:
    def test_empty_calendar_raises_not_silently_empty(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import subprocess

        d = tmp_path / "empty_invest"
        d.mkdir(parents=True, exist_ok=True)
        subprocess.run(["dolt", "--data-dir", str(d), "init"], capture_output=True)
        subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(d),
                "sql",
                "-q",
                "CREATE TABLE ts_trade_day_calendar ("
                " id INT PRIMARY KEY, exchange VARCHAR(10), `date` DATE, is_open TINYINT)",
            ],
            capture_output=True,
        )
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(d))

        from common import trade_calendar  # noqa: F401

        with pytest.raises((RuntimeError, ValueError), match="empty|no.*calendar|calendar"):
            trade_calendar("2026-08-13", "2026-08-25")

    def test_start_after_end_raises(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, _, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))

        from common import trade_calendar  # noqa: F401

        with pytest.raises((RuntimeError, ValueError), match="start|end|range|inverted"):
            trade_calendar("2026-08-25", "2026-08-13")

    def test_illegal_date_string_raises(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, _, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))

        from common import trade_calendar  # noqa: F401

        with pytest.raises((RuntimeError, ValueError), match="date|format|invalid"):
            trade_calendar("not-a-date", "2026-08-25")
        with pytest.raises((RuntimeError, ValueError), match="date|format|invalid"):
            trade_calendar("2026-08-13", "2026/08/25")

    def test_cross_year_boundary_sorted_and_only_open(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import subprocess

        d = tmp_path / "cross_invest"
        d.mkdir(parents=True, exist_ok=True)
        subprocess.run(["dolt", "--data-dir", str(d), "init"], capture_output=True)
        subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(d),
                "sql",
                "-q",
                "CREATE TABLE ts_trade_day_calendar (id INT PRIMARY KEY,"
                " exchange VARCHAR(10), `date` DATE, is_open TINYINT);"
                " INSERT INTO ts_trade_day_calendar VALUES"
                " (1, 'SSE', '2026-12-31', 1), (2, 'SSE', '2027-01-01', 1),"
                " (3, 'SSE', '2027-01-02', 0), (4, 'SSE', '2027-01-04', 1);",
            ],
            capture_output=True,
        )
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(d))

        from common import trade_calendar  # noqa: F401

        days = trade_calendar("2026-12-31", "2027-01-04")
        assert days == ["2026-12-31", "2027-01-01", "2027-01-04"]
        assert days == sorted(days)

    def test_duplicates_weekends_and_other_exchanges_are_normalized(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, _, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))

        from common import trade_calendar  # noqa: F401

        days = trade_calendar("2026-08-13", "2026-08-25")
        # The fixture includes a duplicate 2026-08-14 (SZSE row) and closed
        # weekend rows; only unique SSE open days may be returned.
        assert days == sorted(set(days)), "calendar must return unique sorted days"
        assert "2026-08-15" not in days
        assert "2026-08-22" not in days
        assert "2026-08-14" in days


# ---------------------------------------------------------------------------
# common.missing_dates — boundary / quality / table-attack tests
# ---------------------------------------------------------------------------


class TestMissingDatesAdversarial:
    def test_calendar_missing_raises(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        import subprocess

        invest = tmp_path / "no_cal_invest"
        invest.mkdir(parents=True, exist_ok=True)
        subprocess.run(["dolt", "--data-dir", str(invest), "init"], capture_output=True)
        # No ts_trade_day_calendar table at all.
        compass = tmp_path / "no_cal_compass"
        compass.mkdir(parents=True, exist_ok=True)
        subprocess.run(["dolt", "--data-dir", str(compass), "init"], capture_output=True)
        subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(compass),
                "sql",
                "-q",
                "CREATE TABLE capital_main_flow (symbol VARCHAR(20), trade_date DATE)",
            ],
            capture_output=True,
        )
        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass))
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest))

        from common import missing_dates  # noqa: F401

        with pytest.raises((RuntimeError, ValueError), match="calendar|Dolt|dolt|table|missing"):
            missing_dates(
                table="capital_main_flow",
                date_col="trade_date",
                start="2026-08-13",
                end="2026-08-25",
            )

    def test_missing_target_table_raises(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import subprocess

        invest = tmp_path / "target_invest"
        invest.mkdir(parents=True, exist_ok=True)
        subprocess.run(["dolt", "--data-dir", str(invest), "init"], capture_output=True)
        subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(invest),
                "sql",
                "-q",
                "CREATE TABLE ts_trade_day_calendar (id INT PRIMARY KEY,"
                " exchange VARCHAR(10), `date` DATE, is_open TINYINT);"
                " INSERT INTO ts_trade_day_calendar VALUES"
                " (1, 'SSE', '2026-08-13', 1), (2, 'SSE', '2026-08-14', 1);",
            ],
            capture_output=True,
        )
        compass = tmp_path / "target_compass"
        compass.mkdir(parents=True, exist_ok=True)
        subprocess.run(["dolt", "--data-dir", str(compass), "init"], capture_output=True)

        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass))
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest))

        from common import missing_dates  # noqa: F401

        with pytest.raises(
            (RuntimeError, ValueError), match="table|Dolt|dolt|missing|does not exist"
        ):
            missing_dates(
                table="capital_main_flow",
                date_col="trade_date",
                start="2026-08-13",
                end="2026-08-14",
            )

    def test_start_after_end_raises(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, _, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))

        from common import missing_dates  # noqa: F401

        with pytest.raises((RuntimeError, ValueError), match="start|end|range|inverted"):
            missing_dates(
                table="capital_main_flow",
                date_col="trade_date",
                start="2026-08-25",
                end="2026-08-13",
            )

    def test_illegal_dates_raise(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, _, _ = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))

        from common import missing_dates  # noqa: F401

        with pytest.raises((RuntimeError, ValueError), match="date|format|invalid"):
            missing_dates(
                table="capital_main_flow",
                date_col="trade_date",
                start="banana",
                end="2026-08-25",
            )

    def test_duplicate_existing_dates_do_not_create_false_gaps(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, compass_dir, sql_csv = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        # Fixture already has SH600519 on 08-17/08-18. Add SZ000001 rows for
        # the same dates (different PKs), making each existing date appear on
        # multiple rows. A naive "SELECT DISTINCT date" or raw date union must
        # not treat these as gaps.
        _dolt_sql(
            Path(compass_dir).parent,
            "INSERT INTO capital_main_flow VALUES"
            " ('SZ000001', '2026-08-17', 1, 1, 1, 1, 1, 1, '2026-08-17'),"
            " ('SZ000001', '2026-08-18', 1, 1, 1, 1, 1, 1, '2026-08-18');",
            db_name="compass_data",
        )

        from common import missing_dates  # noqa: F401

        missing = missing_dates(
            table="capital_main_flow",
            date_col="trade_date",
            start="2026-08-17",
            end="2026-08-18",
        )
        assert missing == [], "existing dates must not appear as missing due to duplicates"


# ---------------------------------------------------------------------------
# common.set_last_report_date — anchor regression / creation
# ---------------------------------------------------------------------------


class TestSetLastReportDateAdversarial:
    def test_does_not_regress_anchor(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, compass_dir, sql_csv = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))
        # Pre-seed a newer anchor.
        _dolt_sql(
            Path(compass_dir).parent,
            "INSERT INTO data_updates (table_name, last_updated, source, row_count,"
            " last_report_date) VALUES ('capital_main_flow', CURDATE(), 'stub', 2,"
            " '2026-08-25')",
            db_name="compass_data",
        )

        from common import set_last_report_date  # noqa: F401

        # Attempting to set an older date after a newer one must not regress.
        set_last_report_date("capital_main_flow", "2026-08-13")
        row = (
            sql_csv(
                "compass_data",
                "SELECT last_report_date FROM data_updates WHERE table_name='capital_main_flow'",
            )
            .strip()
            .split("\n")[-1]
        )
        assert row == "2026-08-25", f"anchor must not regress, got {row}"

    def test_creates_anchor_when_no_row_exists(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        invest_dir, compass_dir, sql_csv = dolt_envs
        monkeypatch.setenv("COMPASS_INVESTMENT_DATA_DIR", str(invest_dir))

        from common import set_last_report_date  # noqa: F401

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
# fetch_main_flow.backfill — range / data quality / strict failure / idempotency
# ---------------------------------------------------------------------------


class TestFetchMainFlowBackfillAdversarial:
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

    async def test_start_after_end_raises_without_requests(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        calls: list[dict[str, object]] = []
        stub = self._stub(make_stub_session, payload=_fflow_payload(), calls=calls)

        with (
            patch("fetch_main_flow.AsyncSession", return_value=stub),
            pytest.raises((RuntimeError, ValueError), match="start|end|range|inverted"),
        ):
            await fetch_main_flow.backfill("2026-08-25", "2026-08-13")

        assert calls == [], "start>end must abort before any network request"

    async def test_api_duplicate_dates_are_deduped(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        dup_payload = _fflow_payload(
            "1.600519",
            [
                "2026-08-13,1.1,2.2,3.3,4.4,5.5,6.6",
                "2026-08-13,9.9,8.8,7.7,6.6,5.5,4.4",
                "2026-08-14,1.1,2.2,3.3,4.4,5.5,6.6",
            ],
        )
        stub = self._stub(make_stub_session, payload=dup_payload)

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            path = await fetch_main_flow.backfill("2026-08-13", "2026-08-14")

        with open(path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        dates = [r["trade_date"] for r in rows]
        assert dates == sorted(set(dates)), f"duplicate dates must be deduped: {dates}"
        assert len(dates) == 2

    async def test_api_out_of_order_dates_are_sorted(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        payload = _fflow_payload(
            "1.600519",
            [
                "2026-08-14,7.7,8.8,9.9,10.1,11.1,12.2",
                "2026-08-13,1.1,2.2,3.3,4.4,5.5,6.6",
                "2026-08-17,2.2,3.3,4.4,5.5,6.6,7.7",
            ],
        )
        stub = self._stub(make_stub_session, payload=payload)

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            path = await fetch_main_flow.backfill("2026-08-13", "2026-08-17")

        with open(path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        dates = [r["trade_date"] for r in rows]
        assert dates == sorted(dates), f"dates must be in ascending order: {dates}"

    async def test_missing_or_dash_or_none_values_do_not_crash(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        payload = _fflow_payload(
            "1.600519",
            [
                "2026-08-13,1.1,-,,,5.5,6.6",
                "2026-08-14,7.7,8.8,9.9,,,",
            ],
        )
        stub = self._stub(make_stub_session, payload=payload)

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            path = await fetch_main_flow.backfill("2026-08-13", "2026-08-14")

        with open(path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert len(rows) == 2
        # '--'/'None'/missing cells must not crash; the missing channels are
        # allowed to be empty strings in the CSV.
        for row in rows:
            assert any(
                row[col] in ("", "-", "None") for col in ("small_net", "medium_net", "large_net")
            ), row
        assert rows[0]["trade_date"] == "2026-08-13"

    async def test_multiple_symbols_one_request_each(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
    ) -> None:

        import fetch_main_flow  # noqa: F401

        invest_dir, compass_dir, _ = dolt_envs
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        calls: list[dict[str, object]] = []
        # stock_basic fixture has exactly two symbols: SH600519 and SZ000001.
        payload = _fflow_payload(
            "1.600519",
            ["2026-08-13,1.1,2.2,3.3,4.4,5.5,6.6"],
        )
        stub = self._stub(make_stub_session, payload=payload, calls=calls)

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            await fetch_main_flow.backfill("2026-08-13", "2026-08-13")

        # The handoff contract says: "每股票一次请求，约 6000 次" — one request
        # per symbol. With two symbols in stock_basic, exactly two requests
        # must be issued, never per-day or repeated per-symbol.
        assert len(calls) == 2, f"expected one request per symbol, got {len(calls)}: {calls}"

    async def test_partial_symbol_failure_aborts_strictly_no_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        failures = 0

        async def _fail_second(_url, params=None, headers=None):
            nonlocal failures
            failures += 1
            if failures == 2:
                raise RuntimeError("second symbol failed")
            return StubResponse(json_data=_fflow_payload())

        stub = make_stub_session()
        stub.get = _fail_second  # type: ignore[method-assign]
        with (
            patch("fetch_main_flow.AsyncSession", return_value=stub),
            pytest.raises(RuntimeError, match="failed|abort|symbol"),
        ):
            await fetch_main_flow.backfill(
                "2026-08-13",
                "2026-08-13",
                symbols=["SH600519", "SZ000001"],
            )

        leftover = list(tmp_path.glob("*.csv"))
        assert not leftover, "partial symbol failure must not leave a half-written CSV"

    async def test_repeated_backfill_same_range_is_idempotent(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_main_flow  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        payload = _fflow_payload(
            "1.600519",
            [_fflow_row("2026-08-13", 1), _fflow_row("2026-08-14", 2)],
        )
        stub = self._stub(make_stub_session, payload=payload)

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            p1 = await fetch_main_flow.backfill("2026-08-13", "2026-08-14")
            p2 = await fetch_main_flow.backfill("2026-08-13", "2026-08-14")

        def _count(p: Path) -> int:
            with open(p, newline="", encoding="utf-8-sig") as f:
                return sum(1 for _ in csv.DictReader(f))

        assert _count(p1) == _count(p2), "same backfill re-run must not grow row count"

    async def test_import_does_not_regress_last_report_date(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
    ) -> None:
        import fetch_main_flow  # noqa: F401

        invest_dir, compass_dir, sql_csv = dolt_envs
        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        # Seed an existing newer anchor (e.g. 08-25 already imported).
        _dolt_sql(
            Path(compass_dir).parent,
            "INSERT INTO data_updates (table_name, last_updated, source, row_count,"
            " last_report_date) VALUES ('capital_main_flow', CURDATE(), 'stub', 2,"
            " '2026-08-25')",
            db_name="compass_data",
        )

        stub = self._stub(
            make_stub_session,
            payload=_fflow_payload("1.600519", [_fflow_row("2026-08-13", 1)]),
        )
        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            path = await fetch_main_flow.backfill("2026-08-13", "2026-08-13")

        fetch_main_flow.import_to_dolt(path)
        row = (
            sql_csv(
                "compass_data",
                "SELECT last_report_date FROM data_updates WHERE table_name='capital_main_flow'",
            )
            .strip()
            .split("\n")[-1]
        )
        assert row == "2026-08-25", f"backfill must not regress anchor to {row}"

    def test_backfill_symbols_query_failure_is_strict(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_main_flow  # noqa: F401

        _, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))
        # A missing stock_basic table must not silently fall back to the single
        # test symbol in production (issue #308 decision 11).
        _dolt_sql(compass_dir.parent, "DROP TABLE stock_basic", db_name="compass_data")
        with pytest.raises(RuntimeError, match="stock_basic"):
            fetch_main_flow._backfill_symbols()

    def test_backfill_symbols_empty_universe_is_strict(
        self,
        dolt_envs: tuple[Path, Path, Callable[[str, str], str]],
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_main_flow  # noqa: F401

        _, compass_dir, _ = dolt_envs
        monkeypatch.setenv("COMPASS_DATA_DIR", str(compass_dir))
        _dolt_sql(compass_dir.parent, "DELETE FROM stock_basic", db_name="compass_data")
        with pytest.raises(RuntimeError, match="no symbols"):
            fetch_main_flow._backfill_symbols()


# ---------------------------------------------------------------------------
# fetch_index_daily.backfill — range / pollution / strict failure
# ---------------------------------------------------------------------------


class TestFetchIndexDailyBackfillAdversarial:
    async def test_start_after_end_raises(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_index_daily  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        stub = make_stub_session()
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            pytest.raises((RuntimeError, ValueError), match="start|end|range|inverted"),
        ):
            await fetch_index_daily.backfill("2026-08-25", "2026-08-13")

    async def test_empty_range_does_not_write(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_index_daily  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

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
        monkeypatch.setattr(
            fetch_index_daily,
            "fetch_kline",
            AsyncMock(return_value=([], "")),
        )
        # No trade days between two consecutive weekends is unnatural; use
        # a single calendar date that is not in Parquet/API — the key
        # adversarial demand is that an empty range must not write a CSV.
        await fetch_index_daily.backfill("2026-08-22", "2026-08-22")

        assert captured == [], "empty/range must not write any CSV records"

    async def test_failure_does_not_pollute_preexisting_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_index_daily  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        # Pre-create an existing daily CSV that must survive a failed backfill.
        existing = tmp_path / "index_daily.csv"
        existing.write_text(
            "symbol,trade_date,index_type,open,close,high,low,volume,amount,update_date\n"
            "SH000001,2026-08-13,official,1,2,3,4,5,6,2026-08-13\n",
            encoding="utf-8-sig",
        )
        before = existing.read_text(encoding="utf-8-sig")

        stub = make_stub_session(exc=RuntimeError("backfill failure"))
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            pytest.raises(RuntimeError),
        ):
            await fetch_index_daily.backfill("2026-08-13", "2026-08-14")

        assert existing.read_text(encoding="utf-8-sig") == before, (
            "failed backfill must not pollute existing CSV"
        )

    async def test_failure_midway_leaves_no_temp_or_partial(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_index_daily  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        # First symbol succeeds, second fails — backfill must abort strictly.
        monkeypatch.setattr(
            fetch_index_daily,
            "fetch_ths_industry_list",
            AsyncMock(return_value=[]),
        )
        calls = {"n": 0}

        async def _kline(*_args, **_kwargs):
            calls["n"] += 1
            if calls["n"] >= 2:
                raise RuntimeError("simulated mid-backfill failure")
            return (["2026-08-13,1,2,3,4,5,6,7"], "000001")

        monkeypatch.setattr(
            fetch_index_daily,
            "fetch_kline",
            AsyncMock(side_effect=_kline),
        )
        with pytest.raises(RuntimeError):
            await fetch_index_daily.backfill("2026-08-13", "2026-08-13")

        # No CSV / progress / temp leftovers.
        assert not list(tmp_path.glob("*.csv")), "partial backfill left CSV"
        assert not list(tmp_path.glob("*.tmp")), "partial backfill left temp files"
        assert not list(tmp_path.glob("*.progress.json")), "partial backfill left progress"


# ---------------------------------------------------------------------------
# fetch_block_trade.run(start,end) — empty/invalid/error/resource
# ---------------------------------------------------------------------------


class TestFetchBlockTradeRangeAdversarial:
    async def test_start_after_end_aborts(  # noqa: E301
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")
        requested: list[str] = []

        async def _fake_fetch(*args, **kwargs):
            requested.append(args[4])
            return []

        monkeypatch.setattr(fetch_block_trade, "fetch_paginated", _fake_fetch)
        with (
            patch("fetch_block_trade.AsyncSession", return_value=make_stub_session()),
            pytest.raises((RuntimeError, ValueError), match="start|end|range|inverted"),
        ):
            await fetch_block_trade.run(start="2026-08-25", end="2026-08-13")

        assert requested == [], "invalid range must abort before any network request"

    async def test_empty_years_with_no_range_does_not_write(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")

        stub = make_stub_session()
        with patch("fetch_block_trade.AsyncSession", return_value=stub):
            await fetch_block_trade.run(years=[])

        assert not list(tmp_path.glob("*.csv")), "empty years must not produce CSV"

    async def test_network_failure_mid_range_aborts_no_csv_or_temp(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")

        async def _boom(*args, **kwargs):
            raise RuntimeError("simulated block_trade network failure")

        monkeypatch.setattr(fetch_block_trade, "fetch_paginated", _boom)
        with (
            patch("fetch_block_trade.AsyncSession", return_value=make_stub_session()),
            pytest.raises(RuntimeError),
        ):
            await fetch_block_trade.run(start="2026-08-13", end="2026-08-14")

        assert not list(tmp_path.glob("*.csv")), "failed run must not leave CSV"
        assert not list(tmp_path.glob("*.tmp")), "failed run must not leave temp files"

    async def test_invalid_date_string_aborts_before_network(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        import fetch_block_trade  # noqa: F401

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        monkeypatch.setattr(fetch_block_trade, "last_report_date", lambda _t: "")

        stub = make_stub_session()
        with (
            patch("fetch_block_trade.AsyncSession", return_value=stub),
            pytest.raises((RuntimeError, ValueError), match="date|format|invalid"),
        ):
            await fetch_block_trade.run(start="not-a-date", end="2026-08-14")


# ---------------------------------------------------------------------------
# main.do_sync auto-heal — strict failure / no-partial / resource
# ---------------------------------------------------------------------------


class TestMainSyncAutoHealAdversarial:
    def _patch_sync_modules(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import main as main_mod  # noqa: F401

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
            for attr in ("run", "main", "import_to_dolt"):
                if hasattr(m, attr):
                    if attr == "run":
                        monkeypatch.setattr(m, attr, Mock(return_value=Path("x.csv")))
                    elif attr == "main":
                        monkeypatch.setattr(m, attr, Mock())
                    else:
                        monkeypatch.setattr(m, attr, Mock(return_value=1))
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock(return_value=1))
        monkeypatch.setattr(main_mod.asyncio, "run", Mock())

    def test_missing_dates_error_aborts_before_any_sync_step(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import main as main_mod  # noqa: F401

        self._patch_sync_modules(monkeypatch)
        monkeypatch.setattr(
            main_mod,
            "missing_dates",
            Mock(side_effect=RuntimeError("calendar unavailable")),
        )
        monkeypatch.setattr(main_mod, "backfill", AsyncMock())
        monkeypatch.setattr(main_mod, "set_last_report_date", Mock())

        with pytest.raises(RuntimeError, match="calendar unavailable"):
            main_mod.do_sync()

        main_mod.backfill.assert_not_called()
        # The normal sync must not have run either.
        main_mod._import_stock_basic.assert_not_called()

    def test_backfill_failure_stops_before_normal_collectors(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import main as main_mod  # noqa: F401

        self._patch_sync_modules(monkeypatch)
        monkeypatch.setattr(main_mod, "missing_dates", Mock(return_value=["2026-08-13"]))
        monkeypatch.setattr(
            main_mod,
            "backfill",
            AsyncMock(side_effect=RuntimeError("backfill failed")),
        )
        monkeypatch.setattr(main_mod, "set_last_report_date", Mock())

        with pytest.raises(RuntimeError, match="backfill failed"):
            main_mod.do_sync()

        # Strict abort: no normal collector fetch/import may start.
        main_mod._import_stock_basic.assert_not_called()
        main_mod._import_fin_indicators.assert_not_called()

    def test_no_gap_does_not_touch_anchor(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import main as main_mod  # noqa: F401

        self._patch_sync_modules(monkeypatch)
        monkeypatch.setattr(main_mod, "missing_dates", Mock(return_value=[]))
        monkeypatch.setattr(main_mod, "backfill", AsyncMock())
        monkeypatch.setattr(main_mod, "set_last_report_date", Mock())

        main_mod.do_sync()

        main_mod.backfill.assert_not_called()
        main_mod.set_last_report_date.assert_not_called()

    def test_no_stock_basic_aborts_instead_of_silent_noop(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import main as main_mod  # noqa: F401

        self._patch_sync_modules(monkeypatch)
        # A missing stock_basic table must be a hard failure, not a silent
        # "nothing to filter" success.  We model stock_basic as absent by
        # making the import fail.
        monkeypatch.setattr(main_mod, "missing_dates", Mock(return_value=[]))
        monkeypatch.setattr(
            main_mod,
            "_import_stock_basic",
            Mock(side_effect=RuntimeError("stock_basic import failed")),
        )

        with pytest.raises(RuntimeError, match="stock_basic"):
            main_mod.do_sync()
