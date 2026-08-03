"""Tests for fetch_cash_flow.py — import_to_dolt, run()."""

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

# 48-field header for RPT_DMSK_FN_CASHFLOW
_HEADER = [
    "SECUCODE", "SECURITY_CODE", "INDUSTRY_CODE", "ORG_CODE",
    "SECURITY_NAME_ABBR", "INDUSTRY_NAME", "MARKET", "SECURITY_TYPE_CODE",
    "TRADE_MARKET_CODE", "DATE_TYPE_CODE", "REPORT_TYPE_CODE", "DATA_STATE",
    "NOTICE_DATE", "REPORT_DATE",
    "NETCASH_OPERATE", "NETCASH_OPERATE_RATIO", "SALES_SERVICES", "SALES_SERVICES_RATIO",
    "PAY_STAFF_CASH", "PSC_RATIO", "NETCASH_INVEST", "NETCASH_INVEST_RATIO",
    "RECEIVE_INVEST_INCOME", "RII_RATIO", "CONSTRUCT_LONG_ASSET", "CLA_RATIO",
    "NETCASH_FINANCE", "NETCASH_FINANCE_RATIO", "CCE_ADD", "CCE_ADD_RATIO",
    "CUSTOMER_DEPOSIT_ADD", "CDA_RATIO", "DEPOSIT_IOFI_OTHER", "DIO_RATIO",
    "LOAN_ADVANCE_ADD", "LAA_RATIO", "RECEIVE_INTEREST_COMMISSION", "RIC_RATIO",
    "INVEST_PAY_CASH", "IPC_RATIO", "BEGIN_CCE", "BEGIN_CCE_RATIO",
    "END_CCE", "END_CCE_RATIO", "RECEIVE_ORIGIC_PREMIUM", "ROP_RATIO",
    "PAY_ORIGIC_COMPENSATE", "POC_RATIO",
]


def _make_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31",
    netcash_operate: str = "500",
) -> list[str]:
    row = [""] * len(_HEADER)
    row[_HEADER.index("SECUCODE")] = secucode
    row[_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_HEADER.index("REPORT_DATE")] = report_date
    row[_HEADER.index("NETCASH_OPERATE")] = netcash_operate
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
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "1"

        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_cash_flow'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

    @staticmethod
    def _table_exists(dolt_sql_csv: Callable[[str], str], table: str) -> bool:
        return (
            dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables "
                f"WHERE table_name='{table}'"
            ).strip().split("\n")[-1]
            == "1"
        )

    def test_incremental_merge_appends_preserving_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Incremental CSV merge appends rows, preserving prior history (PIN).

        Replace semantics observe identically here (CSV B fully re-supplies the
        overlap rows with identical values), so this passes both pre- and
        post-fix — it pins the merge contract against future regressions.
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

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
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "3"

        val = self._last(dolt_sql_csv(
            "SELECT NETCASH_OPERATE FROM fin_cash_flow "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        ))
        assert val == "500"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_cash_flow WHERE symbol='SZ000002'"
        )) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_cash_flow WHERE report_date='2023-12-31'"
        )) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_cash_flow'"
        )) == "3,2024-12-31"

    def test_incremental_window_preserves_older_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Incremental window (watermark refetch + new symbol) keeps old rows (RED).

        RED: pre-fix replace semantics wipe the 2023-12-31 row — CSV B replaces
        the whole table, so the second import returns 2 and the older period
        disappears instead of surviving.
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row(), _make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 2

        self._write_csv(csv_path, [_make_row(), _make_row(secucode="000002.SZ")])
        rows = import_to_dolt(csv_path)
        assert rows == 3
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "3"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_cash_flow "
            "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
        )) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_cash_flow'"
        )) == "3,2024-12-31"

    def test_restated_overlap_value_ignored_on_merge(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Restated overlap value is rejected on merge, old value kept (RED).

        RED: pre-fix replace accepts the restated 200 (CSV B wins wholesale),
        so the value assertion fails; merge must keep the original 500.
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(
            csv_path,
            [_make_row(netcash_operate="200"), _make_row(secucode="000002.SZ")],
        )
        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "2"
        val = self._last(dolt_sql_csv(
            "SELECT NETCASH_OPERATE FROM fin_cash_flow "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        ))
        assert val == "500"

    def test_same_report_refetch_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Refetching the same report period twice stays at one row (PIN)."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "1"

    def test_merge_watermark_full_total_and_max_date(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Watermark counts the full table and max report date (RED).

        RED: pre-fix replace writes row_count for the latest CSV only — the
        second import reports 1 row and row_count == 1, not the full-table 2.
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(csv_path, [_make_row()])
        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert self._last(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_cash_flow'"
        )) == "2,2024-12-31"

    def test_first_run_insert_failure_leaves_empty_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure leaves an empty table, no tmp residue (RED).

        RED: pre-fix replace drops the whole table on failure, so the
        COUNT(*) query errors and returns "" instead of "0"; merge must keep
        the (empty) table created by CREATE TABLE IF NOT EXISTS.
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "0"
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf")
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf_old")
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM data_updates WHERE table_name='fin_cash_flow'"
        )) == "0"

    def test_rerun_insert_failure_preserves_prior_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT preserves prior rows and watermark (PIN).

        Replace relies on RENAME rollback, merge on never touching the table —
        both keep the prior row, so this passes pre- and post-fix.
        """
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cf.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_cash_flow")) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT NETCASH_OPERATE FROM fin_cash_flow "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "500"
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf")
        assert not self._table_exists(dolt_sql_csv, "_tmp_cf_old")
        assert self._last(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_cash_flow'"
        )) == "1,2024-12-31"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_cash_flow import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_cash_flow import run  # noqa: E402

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

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="FY")

        assert result.name == "RPT_DMSK_FN_CASHFLOW.csv"
        csv_path = tmp_path / "RPT_DMSK_FN_CASHFLOW.csv"
        assert csv_path.exists()

    async def test_run_default_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Call run() without years — triggers the `if years is None` default path."""
        from fetch_cash_flow import run  # noqa: E402

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

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(periods="FY")

        assert result.name == "RPT_DMSK_FN_CASHFLOW.csv"

    async def test_run_incremental_since_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date returns a future date, run() returns early."""
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_cash_flow.last_report_date", lambda _tbl: "2099-12-31")

        result = await run(years=[2024], periods="FY")
        assert result.name == "RPT_DMSK_FN_CASHFLOW.csv"

    async def test_run_fetch_exception_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() catches and continues."""
        from fetch_cash_flow import run  # noqa: E402

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

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_DMSK_FN_CASHFLOW.csv"

    async def test_run_incremental_overwrites_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() overwrites a stale CSV — history lives in Dolt, not CSV (PIN).

        Direct answer to the "is the CSV overwritten?" question: every run
        rewrites the CSV from scratch (first write is mode="w"), so the CSV
        holds only the current fetch window. That is why Dolt must merge.
        """
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr(
            "fetch_cash_flow.last_report_date", lambda _tbl: "2026-06-30"
        )

        csv_path = tmp_path / "RPT_DMSK_FN_CASHFLOW.csv"
        with open(csv_path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(["code", "REPORT_DATE"])
            writer.writerow(["000001", "2024-12-31"])

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORT_DATE": "2026-06-30"}],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q2", page_size=100)

        assert result.name == "RPT_DMSK_FN_CASHFLOW.csv"
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]

    async def test_run_incremental_window_starts_at_watermark(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() fetches only dates >= watermark — window starts there (PIN).

        Pins the `d >= since` contract: the latest report period is always
        refetched, anything older is filtered out before any HTTP call.
        """
        from fetch_cash_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr(
            "fetch_cash_flow.last_report_date", lambda _tbl: "2026-06-30"
        )

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
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

        with patch("fetch_cash_flow.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_DMSK_FN_CASHFLOW.csv"
        assert call_count[0] == 1
        csv_path = tmp_path / "RPT_DMSK_FN_CASHFLOW.csv"
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]

