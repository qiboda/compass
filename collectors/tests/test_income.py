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


def _make_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31",
    parent_netprofit: str = "1000",
) -> list[str]:
    """Build a full 46-col row with minimal data populated."""
    row = [""] * len(_HEADER)
    row[_HEADER.index("SECUCODE")] = secucode
    row[_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_HEADER.index("REPORT_DATE")] = report_date
    row[_HEADER.index("PARENT_NETPROFIT")] = parent_netprofit
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
            "INSERT INTO stock_basic VALUES ('SZ000001'), ('SZ000002')"
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

    def test_incremental_merge_appends_preserving_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """CSV B (same row + new symbol + older period) appends to existing history."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(
            csv_path,
            [
                _make_row(),
                _make_row(secucode="000002.SZ"),
                _make_row(report_date="2023-12-31"),
            ],
        )
        rows = import_to_dolt(csv_path)
        assert rows == 3
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "3"
        assert self._last(dolt_sql_csv(
            "SELECT PARENT_NETPROFIT FROM fin_income "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "1000"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_income WHERE symbol='SZ000002'"
        )) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_income WHERE report_date='2023-12-31'"
        )) == "1"
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_income'"
        ).strip()
        assert "3" in row and "2024-12-31" in row

    def test_incremental_window_preserves_older_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Incremental-window CSV B must not erase periods older than the window."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row(), _make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 2

        self._write_csv(csv_path, [_make_row(), _make_row(secucode="000002.SZ")])
        rows = import_to_dolt(csv_path)
        assert rows == 3
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "3"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_income "
            "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
        )) == "1"
        row = dolt_sql_csv(
            "SELECT row_count FROM data_updates WHERE table_name='fin_income'"
        ).strip()
        assert "3" in row

    def test_restated_overlap_value_ignored_on_merge(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """A restated value for an existing (symbol, report_date) is ignored on merge."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(
            csv_path,
            [_make_row(parent_netprofit="200"), _make_row(secucode="000002.SZ")],
        )
        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "2"
        assert self._last(dolt_sql_csv(
            "SELECT PARENT_NETPROFIT FROM fin_income "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "1000"

    def test_same_report_refetch_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Re-importing the same CSV twice yields a single row (PK dedup)."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "1"

    def test_merge_watermark_full_total_and_max_date(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Watermark row_count is the full-table count, not just this CSV's rows."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(csv_path, [_make_row()])
        rows = import_to_dolt(csv_path)
        assert rows == 2
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_income'"
        ).strip()
        assert "2" in row and "2024-12-31" in row

    def test_first_run_insert_failure_leaves_empty_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure leaves the table present but empty (merge semantics)."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "0"
        for t in ("_tmp_inc", "_tmp_inc_old"):
            cnt = self._last(dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables "
                f"WHERE table_name='{t}'"
            ))
            assert cnt == "0"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM data_updates WHERE table_name='fin_income'"
        )) == "0"

    def test_rerun_insert_failure_preserves_prior_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT keeps prior rows and the watermark (merge semantics)."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT PARENT_NETPROFIT FROM fin_income "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "1000"
        for t in ("_tmp_inc", "_tmp_inc_old"):
            cnt = self._last(dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables "
                f"WHERE table_name='{t}'"
            ))
            assert cnt == "0"
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_income'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

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

    async def test_run_incremental_overwrites_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() overwrites a stale CSV; history lives in Dolt, not the CSV."""
        from fetch_income import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_income.last_report_date", lambda _tbl: "2026-06-30")

        stale = tmp_path / "RPT_DMSK_FN_INCOME.csv"
        stale.write_text("code,REPORT_DATE\n000001,2024-12-31\n", encoding="utf-8-sig")

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORT_DATE": "2026-06-30"}],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_income.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q2", page_size=100)

        assert result.name == "RPT_DMSK_FN_INCOME.csv"
        with open(stale, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]

    async def test_run_incremental_window_starts_at_watermark(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() refetches the watermark period but skips older ones (d >= since)."""
        from fetch_income import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_income.last_report_date", lambda _tbl: "2026-06-30")

        calls: list[str] = []

        async def _get(
            url: str, params: dict[str, str] | None = None, headers: object | None = None
        ) -> StubResponse:
            assert params is not None
            calls.append(params["filter"])
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {
                        "data": [{"code": "000001", "REPORT_DATE": "2026-06-30"}],
                        "pages": 1,
                    },
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_income.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_DMSK_FN_INCOME.csv"
        assert len(calls) == 1
        assert "(REPORT_DATE='2026-06-30')" in calls[0]
        with open(tmp_path / "RPT_DMSK_FN_INCOME.csv", newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]

