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

# 203-field header for RPT_F10_FINANCE_GINCOME (F10 full income statement,
# generated from .omo/evidence/financial-f10/f10_columns.json GINCOME.fields)
_HEADER = [
    "SECUCODE", "SECURITY_CODE", "SECURITY_NAME_ABBR", "ORG_CODE", "ORG_TYPE", "REPORT_DATE",
    "REPORT_TYPE", "REPORT_DATE_NAME", "SECURITY_TYPE_CODE", "NOTICE_DATE", "UPDATE_DATE",
    "CURRENCY", "TOTAL_OPERATE_INCOME", "TOTAL_OPERATE_INCOME_YOY", "OPERATE_INCOME",
    "OPERATE_INCOME_YOY", "INTEREST_INCOME", "INTEREST_INCOME_YOY", "EARNED_PREMIUM",
    "EARNED_PREMIUM_YOY", "FEE_COMMISSION_INCOME", "FEE_COMMISSION_INCOME_YOY",
    "OTHER_BUSINESS_INCOME", "OTHER_BUSINESS_INCOME_YOY", "TOI_OTHER", "TOI_OTHER_YOY",
    "TOTAL_OPERATE_COST", "TOTAL_OPERATE_COST_YOY", "OPERATE_COST", "OPERATE_COST_YOY",
    "INTEREST_EXPENSE", "INTEREST_EXPENSE_YOY", "FEE_COMMISSION_EXPENSE",
    "FEE_COMMISSION_EXPENSE_YOY", "RESEARCH_EXPENSE", "RESEARCH_EXPENSE_YOY", "SURRENDER_VALUE",
    "SURRENDER_VALUE_YOY", "NET_COMPENSATE_EXPENSE", "NET_COMPENSATE_EXPENSE_YOY",
    "NET_CONTRACT_RESERVE", "NET_CONTRACT_RESERVE_YOY", "POLICY_BONUS_EXPENSE",
    "POLICY_BONUS_EXPENSE_YOY", "REINSURE_EXPENSE", "REINSURE_EXPENSE_YOY", "OTHER_BUSINESS_COST",
    "OTHER_BUSINESS_COST_YOY", "OPERATE_TAX_ADD", "OPERATE_TAX_ADD_YOY", "SALE_EXPENSE",
    "SALE_EXPENSE_YOY", "MANAGE_EXPENSE", "MANAGE_EXPENSE_YOY", "ME_RESEARCH_EXPENSE",
    "ME_RESEARCH_EXPENSE_YOY", "FINANCE_EXPENSE", "FINANCE_EXPENSE_YOY", "FE_INTEREST_EXPENSE",
    "FE_INTEREST_EXPENSE_YOY", "FE_INTEREST_INCOME", "FE_INTEREST_INCOME_YOY",
    "ASSET_IMPAIRMENT_LOSS", "ASSET_IMPAIRMENT_LOSS_YOY", "CREDIT_IMPAIRMENT_LOSS",
    "CREDIT_IMPAIRMENT_LOSS_YOY", "TOC_OTHER", "TOC_OTHER_YOY", "FAIRVALUE_CHANGE_INCOME",
    "FAIRVALUE_CHANGE_INCOME_YOY", "INVEST_INCOME", "INVEST_INCOME_YOY", "INVEST_JOINT_INCOME",
    "INVEST_JOINT_INCOME_YOY", "NET_EXPOSURE_INCOME", "NET_EXPOSURE_INCOME_YOY", "EXCHANGE_INCOME",
    "EXCHANGE_INCOME_YOY", "ASSET_DISPOSAL_INCOME", "ASSET_DISPOSAL_INCOME_YOY",
    "ASSET_IMPAIRMENT_INCOME", "ASSET_IMPAIRMENT_INCOME_YOY", "CREDIT_IMPAIRMENT_INCOME",
    "CREDIT_IMPAIRMENT_INCOME_YOY", "OTHER_INCOME", "OTHER_INCOME_YOY", "OPERATE_PROFIT_OTHER",
    "OPERATE_PROFIT_OTHER_YOY", "OPERATE_PROFIT_BALANCE", "OPERATE_PROFIT_BALANCE_YOY",
    "OPERATE_PROFIT", "OPERATE_PROFIT_YOY", "NONBUSINESS_INCOME", "NONBUSINESS_INCOME_YOY",
    "NONCURRENT_DISPOSAL_INCOME", "NONCURRENT_DISPOSAL_INCOME_YOY", "NONBUSINESS_EXPENSE",
    "NONBUSINESS_EXPENSE_YOY", "NONCURRENT_DISPOSAL_LOSS", "NONCURRENT_DISPOSAL_LOSS_YOY",
    "EFFECT_TP_OTHER", "EFFECT_TP_OTHER_YOY", "TOTAL_PROFIT_BALANCE", "TOTAL_PROFIT_BALANCE_YOY",
    "TOTAL_PROFIT", "TOTAL_PROFIT_YOY", "INCOME_TAX", "INCOME_TAX_YOY", "EFFECT_NETPROFIT_OTHER",
    "EFFECT_NETPROFIT_OTHER_YOY", "EFFECT_NETPROFIT_BALANCE", "EFFECT_NETPROFIT_BALANCE_YOY",
    "UNCONFIRM_INVEST_LOSS", "UNCONFIRM_INVEST_LOSS_YOY", "NETPROFIT", "NETPROFIT_YOY",
    "PRECOMBINE_PROFIT", "PRECOMBINE_PROFIT_YOY", "CONTINUED_NETPROFIT", "CONTINUED_NETPROFIT_YOY",
    "DISCONTINUED_NETPROFIT", "DISCONTINUED_NETPROFIT_YOY", "PARENT_NETPROFIT",
    "PARENT_NETPROFIT_YOY", "MINORITY_INTEREST", "MINORITY_INTEREST_YOY",
    "DEDUCT_PARENT_NETPROFIT", "DEDUCT_PARENT_NETPROFIT_YOY", "NETPROFIT_OTHER",
    "NETPROFIT_OTHER_YOY", "NETPROFIT_BALANCE", "NETPROFIT_BALANCE_YOY", "BASIC_EPS",
    "BASIC_EPS_YOY", "DILUTED_EPS", "DILUTED_EPS_YOY", "OTHER_COMPRE_INCOME",
    "OTHER_COMPRE_INCOME_YOY", "PARENT_OCI", "PARENT_OCI_YOY", "MINORITY_OCI", "MINORITY_OCI_YOY",
    "PARENT_OCI_OTHER", "PARENT_OCI_OTHER_YOY", "PARENT_OCI_BALANCE", "PARENT_OCI_BALANCE_YOY",
    "UNABLE_OCI", "UNABLE_OCI_YOY", "CREDITRISK_FAIRVALUE_CHANGE",
    "CREDITRISK_FAIRVALUE_CHANGE_YOY", "OTHERRIGHT_FAIRVALUE_CHANGE",
    "OTHERRIGHT_FAIRVALUE_CHANGE_YOY", "SETUP_PROFIT_CHANGE", "SETUP_PROFIT_CHANGE_YOY",
    "RIGHTLAW_UNABLE_OCI", "RIGHTLAW_UNABLE_OCI_YOY", "UNABLE_OCI_OTHER", "UNABLE_OCI_OTHER_YOY",
    "UNABLE_OCI_BALANCE", "UNABLE_OCI_BALANCE_YOY", "ABLE_OCI", "ABLE_OCI_YOY",
    "RIGHTLAW_ABLE_OCI", "RIGHTLAW_ABLE_OCI_YOY", "AFA_FAIRVALUE_CHANGE",
    "AFA_FAIRVALUE_CHANGE_YOY", "HMI_AFA", "HMI_AFA_YOY", "CASHFLOW_HEDGE_VALID",
    "CASHFLOW_HEDGE_VALID_YOY", "CREDITOR_FAIRVALUE_CHANGE", "CREDITOR_FAIRVALUE_CHANGE_YOY",
    "CREDITOR_IMPAIRMENT_RESERVE", "CREDITOR_IMPAIRMENT_RESERVE_YOY", "FINANCE_OCI_AMT",
    "FINANCE_OCI_AMT_YOY", "CONVERT_DIFF", "CONVERT_DIFF_YOY", "ABLE_OCI_OTHER",
    "ABLE_OCI_OTHER_YOY", "ABLE_OCI_BALANCE", "ABLE_OCI_BALANCE_YOY", "OCI_OTHER", "OCI_OTHER_YOY",
    "OCI_BALANCE", "OCI_BALANCE_YOY", "TOTAL_COMPRE_INCOME", "TOTAL_COMPRE_INCOME_YOY",
    "PARENT_TCI", "PARENT_TCI_YOY", "MINORITY_TCI", "MINORITY_TCI_YOY", "PRECOMBINE_TCI",
    "PRECOMBINE_TCI_YOY", "EFFECT_TCI_BALANCE", "EFFECT_TCI_BALANCE_YOY", "TCI_OTHER",
    "TCI_OTHER_YOY", "TCI_BALANCE", "TCI_BALANCE_YOY", "ACF_END_INCOME", "ACF_END_INCOME_YOY",
    "OPINION_TYPE",
]

# F10 text columns (VARCHAR in DDL) — REPORT_DATE excluded: it maps to the
# PK column `report_date DATE NOT NULL`.
_VARCHAR_FIELDS = {
    "SECUCODE", "SECURITY_CODE", "SECURITY_NAME_ABBR", "ORG_CODE", "ORG_TYPE",
    "REPORT_TYPE", "REPORT_DATE_NAME", "SECURITY_TYPE_CODE", "NOTICE_DATE",
    "UPDATE_DATE", "CURRENCY", "OPINION_TYPE",
}

_HEADER_IDX = {f: i for i, f in enumerate(_HEADER)}

# Moutai FY2024 reference values (yuan units), locked by
# test_moutai_2024_values_units (±1%).
MOUTAI_TOTAL_OPERATE_INCOME = "174144069958.25"
MOUTAI_BASIC_EPS = "68.64"


def _make_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31 00:00:00",
    parent_netprofit: str = "1000",
) -> list[str]:
    """Build a full 203-col F10 row with every field populated.

    Numeric fields get reasonable values; TOTAL_OPERATE_INCOME / BASIC_EPS
    carry the Moutai FY2024 reference values (yuan units) so any imported
    row exercises the full 203-column path and the unit assertions.
    """
    row = ["1"] * len(_HEADER)
    row[_HEADER_IDX["SECUCODE"]] = secucode
    row[_HEADER_IDX["SECURITY_CODE"]] = secucode.split(".")[0]
    row[_HEADER_IDX["SECURITY_NAME_ABBR"]] = "TEST CO LTD"
    row[_HEADER_IDX["ORG_CODE"]] = "ORG001"
    row[_HEADER_IDX["REPORT_DATE"]] = report_date
    row[_HEADER_IDX["REPORT_DATE_NAME"]] = "2024 Annual"
    row[_HEADER_IDX["NOTICE_DATE"]] = "2025-04-30 00:00:00"
    row[_HEADER_IDX["UPDATE_DATE"]] = "2025-04-30 00:00:00"
    row[_HEADER_IDX["CURRENCY"]] = "CNY"
    row[_HEADER_IDX["OPINION_TYPE"]] = "标准无保留意见"
    row[_HEADER_IDX["PARENT_NETPROFIT"]] = parent_netprofit
    row[_HEADER_IDX["TOTAL_OPERATE_INCOME"]] = MOUTAI_TOTAL_OPERATE_INCOME
    row[_HEADER_IDX["BASIC_EPS"]] = MOUTAI_BASIC_EPS
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

    def test_ddl_covers_all_f10_fields(self) -> None:
        """DDL/COLS must cover every F10 GINCOME field except REPORT_DATE (→ PK)."""
        from fetch_income import COLS, DDL  # noqa: E402

        cols = [c.strip() for c in COLS.split(",")]
        assert len(cols) == len(_HEADER) - 1 == 202
        assert cols == [f for f in _HEADER if f != "REPORT_DATE"]

        assert "PRIMARY KEY (symbol, report_date)" in DDL
        for field in cols:
            line = next(
                (ln for ln in DDL.splitlines() if ln.strip().startswith(field + " ")),
                None,
            )
            assert line is not None, f"{field} missing from DDL"
            expect = "VARCHAR(100)" if field in _VARCHAR_FIELDS else "DOUBLE"
            assert expect in line, f"{field}: expected {expect}, got {line.strip()}"

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

    def test_moutai_2024_values_units(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Moutai FY2024 units: revenue ≈ 174.14bn CNY, EPS ≈ 68.64 (±1%)."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        dolt_sql_csv("INSERT INTO stock_basic VALUES ('SH600519')")
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row(secucode="600519.SH")])

        assert import_to_dolt(csv_path) == 1
        out = dolt_sql_csv(
            "SELECT TOTAL_OPERATE_INCOME, BASIC_EPS FROM fin_income "
            "WHERE symbol='SH600519' AND report_date='2024-12-31'"
        )
        lines = out.strip().split("\n")
        assert lines[0].split(",") == ["TOTAL_OPERATE_INCOME", "BASIC_EPS"]
        total, eps = lines[1].split(",")
        assert float(total) == pytest.approx(174144069958.25, rel=0.01)
        assert float(eps) == pytest.approx(68.64, rel=0.01)

    def test_refetch_full_csv_rebuilds_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """A full-history CSV refetch rebuilds the table; every row present."""
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
                _make_row(report_date="2023-12-31 00:00:00"),
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

    def test_partial_window_replaces_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """A partial-window CSV replaces the whole table (rebuild semantics)."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row(), _make_row(report_date="2023-12-31 00:00:00")])
        assert import_to_dolt(csv_path) == 2

        self._write_csv(csv_path, [_make_row(), _make_row(secucode="000002.SZ")])
        rows = import_to_dolt(csv_path)
        assert rows == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "2"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_income WHERE symbol='SZ000002'"
        )) == "1"
        # periods outside the new CSV are dropped (replace, not append)
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_income "
            "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
        )) == "0"
        row = dolt_sql_csv(
            "SELECT row_count FROM data_updates WHERE table_name='fin_income'"
        ).strip()
        assert "2" in row

    def test_restated_value_wins_on_replace(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """A restated value in the new CSV replaces the previously stored one."""
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
        )) == "200"

    def test_same_report_refetch_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Re-importing the same single-row CSV twice yields a single row."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_income")) == "1"

    def test_replace_watermark_full_total_and_max_date(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Watermark row_count is the final table count and max report date."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row(report_date="2023-12-31 00:00:00")])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(csv_path, [_make_row()])
        rows = import_to_dolt(csv_path)
        assert rows == 1
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_income'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure drops the fresh table (nothing to roll back to)."""
        from fetch_income import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "inc.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='fin_income'"
        )) == "0"
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
        """Rerun with failing INSERT rolls back to the previous table and watermark."""
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

        assert result.name == "RPT_F10_FINANCE_GINCOME.csv"
        csv_path = tmp_path / "RPT_F10_FINANCE_GINCOME.csv"
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

        assert result.name == "RPT_F10_FINANCE_GINCOME.csv"

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
        assert result.name == "RPT_F10_FINANCE_GINCOME.csv"

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

        assert result.name == "RPT_F10_FINANCE_GINCOME.csv"

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

        stale = tmp_path / "RPT_F10_FINANCE_GINCOME.csv"
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

        assert result.name == "RPT_F10_FINANCE_GINCOME.csv"
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

        assert result.name == "RPT_F10_FINANCE_GINCOME.csv"
        assert len(calls) == 1
        assert "(REPORT_DATE='2026-06-30')" in calls[0]
        with open(tmp_path / "RPT_F10_FINANCE_GINCOME.csv", newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]
