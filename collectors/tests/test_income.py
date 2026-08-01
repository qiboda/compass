"""Tests for fetch_income.py — import_to_dolt, run()."""

import asyncio
import csv
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# 46-field header for RPT_DMSK_FN_INCOME
_HEADER = [
    "SECUCODE", "SECURITY_CODE", "INDUSTRY_CODE", "ORG_CODE",
    "SECURITY_NAME_ABBR", "INDUSTRY_NAME", "MARKET", "SECURITY_TYPE_CODE",
    "TRADE_MARKET_CODE", "DATE_TYPE_CODE", "REPORT_TYPE_CODE", "DATA_STATE",
    "NOTICE_DATE", "REPORT_DATE",
    "PARENT_NETPROFIT", "TOTAL_OPERATE_INCOME", "TOTAL_OPERATE_COST", "TOE_RATIO",
    "OPERATE_COST", "OPERATE_EXPENSE", "OPERATE_EXPENSE_RATIO", "SALE_EXPENSE",
    "MANAGE_EXPENSE", "FINANCE_EXPENSE", "OPERATE_PROFIT", "TOTAL_PROFIT", "INCOME_TAX",
    "OPERATE_INCOME", "INTEREST_NI", "INTEREST_NI_RATIO", "FEE_COMMISSION_NI", "FCN_RATIO",
    "OPERATE_TAX_ADD", "MANAGE_EXPENSE_BANK", "FCN_CALCULATE", "INTEREST_NI_CALCULATE",
    "EARNED_PREMIUM", "EARNED_PREMIUM_RATIO", "INVEST_INCOME", "SURRENDER_VALUE",
    "COMPENSATE_EXPENSE", "TOI_RATIO", "OPERATE_PROFIT_RATIO",
    "PARENT_NETPROFIT_RATIO", "DEDUCT_PARENT_NETPROFIT", "DPN_RATIO",
]


def _make_row(secucode: str = "000001.SZ") -> list[str]:
    row = [""] * len(_HEADER)
    row[_HEADER.index("SECUCODE")] = secucode
    row[_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_HEADER.index("REPORT_DATE")] = "2024-12-31"
    row[_HEADER.index("PARENT_NETPROFIT")] = "1000"
    return row


# ── import_to_dolt tests ──


class TestImportToDolt:
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
            "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY); "
            "INSERT INTO stock_basic VALUES ('SZ000001')"
        )
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

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerows(rows)

    def test_first_run_creates_table_and_imports(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "1"

        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_income'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

    def test_rerun_replaces_table_without_duplicates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "1"

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure drops the table cleanly."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='fin_income'"
        ))
        assert cnt == "0"

    def test_rerun_insert_failure_rolls_back(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores previous data."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "1"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_income import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_income import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORT_DATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_income.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="FY")

        assert result.name == "RPT_DMSK_FN_INCOME.csv"
        csv_path = tmp_path / "RPT_DMSK_FN_INCOME.csv"
        assert csv_path.exists()

    async def test_run_default_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Call run() without years — triggers the `if years is None` default path."""
        from fetch_income import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [], "pages": 1},
            }
        )

        with patch("fetch_income.AsyncSession", return_value=stub):
            result = await run(periods="FY")

        assert result.name == "RPT_DMSK_FN_INCOME.csv"

    async def test_run_incremental_since_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date returns a future date, run() returns early."""
        from fetch_income import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_income.last_report_date", lambda _tbl: "2099-12-31")

        result = await run(years=[2024], periods="FY")
        assert result.name == "RPT_DMSK_FN_INCOME.csv"

    async def test_run_fetch_exception_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() catches and continues."""
        from fetch_income import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            if call_count[0] <= 4:
                raise RuntimeError("simulated fetch error")
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [], "pages": 1},
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_income.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_DMSK_FN_INCOME.csv"

