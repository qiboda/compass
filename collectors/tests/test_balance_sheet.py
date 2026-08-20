"""Tests for fetch_balance_sheet.py — import_to_dolt, run(), _main."""

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

# ── Reuse the existing Dolt test helper from test_import_to_dolt ──

_HEADER = [
    'SECUCODE', 'SECURITY_CODE', 'SECURITY_NAME_ABBR', 'ORG_CODE', 'ORG_TYPE', 'REPORT_DATE', 'REPORT_TYPE', 'REPORT_DATE_NAME',
    'SECURITY_TYPE_CODE', 'NOTICE_DATE', 'UPDATE_DATE', 'CURRENCY', 'ACCEPT_DEPOSIT_INTERBANK', 'ACCOUNTS_PAYABLE', 'ACCOUNTS_RECE', 'ACCRUED_EXPENSE',
    'ADVANCE_RECEIVABLES', 'AGENT_TRADE_SECURITY', 'AGENT_UNDERWRITE_SECURITY', 'AMORTIZE_COST_FINASSET', 'AMORTIZE_COST_FINLIAB', 'AMORTIZE_COST_NCFINASSET', 'AMORTIZE_COST_NCFINLIAB', 'APPOINT_FVTPL_FINASSET',
    'APPOINT_FVTPL_FINLIAB', 'ASSET_BALANCE', 'ASSET_OTHER', 'ASSIGN_CASH_DIVIDEND', 'AVAILABLE_SALE_FINASSET', 'BOND_PAYABLE', 'BORROW_FUND', 'BUY_RESALE_FINASSET',
    'CAPITAL_RESERVE', 'CIP', 'CONSUMPTIVE_BIOLOGICAL_ASSET', 'CONTRACT_ASSET', 'CONTRACT_LIAB', 'CONVERT_DIFF', 'CREDITOR_INVEST', 'CURRENT_ASSET_BALANCE',
    'CURRENT_ASSET_OTHER', 'CURRENT_LIAB_BALANCE', 'CURRENT_LIAB_OTHER', 'DEFER_INCOME', 'DEFER_INCOME_1YEAR', 'DEFER_TAX_ASSET', 'DEFER_TAX_LIAB', 'DERIVE_FINASSET',
    'DERIVE_FINLIAB', 'DEVELOP_EXPENSE', 'DIV_HOLDSALE_ASSET', 'DIV_HOLDSALE_LIAB', 'DIVIDEND_PAYABLE', 'DIVIDEND_RECE', 'EQUITY_BALANCE', 'EQUITY_OTHER',
    'EXPORT_REFUND_RECE', 'FEE_COMMISSION_PAYABLE', 'FIN_FUND', 'FINANCE_RECE', 'FIXED_ASSET', 'FIXED_ASSET_DISPOSAL', 'FVTOCI_FINASSET', 'FVTOCI_NCFINASSET',
    'FVTPL_FINASSET', 'FVTPL_FINLIAB', 'GENERAL_RISK_RESERVE', 'GOODWILL', 'HOLD_MATURITY_INVEST', 'HOLDSALE_ASSET', 'HOLDSALE_LIAB', 'INSURANCE_CONTRACT_RESERVE',
    'INTANGIBLE_ASSET', 'INTEREST_PAYABLE', 'INTEREST_RECE', 'INTERNAL_PAYABLE', 'INTERNAL_RECE', 'INVENTORY', 'INVEST_REALESTATE', 'LEASE_LIAB',
    'LEND_FUND', 'LIAB_BALANCE', 'LIAB_EQUITY_BALANCE', 'LIAB_EQUITY_OTHER', 'LIAB_OTHER', 'LOAN_ADVANCE', 'LOAN_PBC', 'LONG_EQUITY_INVEST',
    'LONG_LOAN', 'LONG_PAYABLE', 'LONG_PREPAID_EXPENSE', 'LONG_RECE', 'LONG_STAFFSALARY_PAYABLE', 'MINORITY_EQUITY', 'MONETARYFUNDS', 'NONCURRENT_ASSET_1YEAR',
    'NONCURRENT_ASSET_BALANCE', 'NONCURRENT_ASSET_OTHER', 'NONCURRENT_LIAB_1YEAR', 'NONCURRENT_LIAB_BALANCE', 'NONCURRENT_LIAB_OTHER', 'NOTE_ACCOUNTS_PAYABLE', 'NOTE_ACCOUNTS_RECE', 'NOTE_PAYABLE',
    'NOTE_RECE', 'OIL_GAS_ASSET', 'OTHER_COMPRE_INCOME', 'OTHER_CREDITOR_INVEST', 'OTHER_CURRENT_ASSET', 'OTHER_CURRENT_LIAB', 'OTHER_EQUITY_INVEST', 'OTHER_EQUITY_OTHER',
    'OTHER_EQUITY_TOOL', 'OTHER_NONCURRENT_ASSET', 'OTHER_NONCURRENT_FINASSET', 'OTHER_NONCURRENT_LIAB', 'OTHER_PAYABLE', 'OTHER_RECE', 'PARENT_EQUITY_BALANCE', 'PARENT_EQUITY_OTHER',
    'PERPETUAL_BOND', 'PERPETUAL_BOND_PAYBALE', 'PREDICT_CURRENT_LIAB', 'PREDICT_LIAB', 'PREFERRED_SHARES', 'PREFERRED_SHARES_PAYBALE', 'PREMIUM_RECE', 'PREPAYMENT',
    'PRODUCTIVE_BIOLOGY_ASSET', 'PROJECT_MATERIAL', 'RC_RESERVE_RECE', 'REINSURE_PAYABLE', 'REINSURE_RECE', 'SELL_REPO_FINASSET', 'SETTLE_EXCESS_RESERVE', 'SHARE_CAPITAL',
    'SHORT_BOND_PAYABLE', 'SHORT_FIN_PAYABLE', 'SHORT_LOAN', 'SPECIAL_PAYABLE', 'SPECIAL_RESERVE', 'STAFF_SALARY_PAYABLE', 'SUBSIDY_RECE', 'SURPLUS_RESERVE',
    'TAX_PAYABLE', 'TOTAL_ASSETS', 'TOTAL_CURRENT_ASSETS', 'TOTAL_CURRENT_LIAB', 'TOTAL_EQUITY', 'TOTAL_LIAB_EQUITY', 'TOTAL_LIABILITIES', 'TOTAL_NONCURRENT_ASSETS',
    'TOTAL_NONCURRENT_LIAB', 'TOTAL_OTHER_PAYABLE', 'TOTAL_OTHER_RECE', 'TOTAL_PARENT_EQUITY', 'TRADE_FINASSET', 'TRADE_FINASSET_NOTFVTPL', 'TRADE_FINLIAB', 'TRADE_FINLIAB_NOTFVTPL',
    'TREASURY_SHARES', 'UNASSIGN_RPOFIT', 'UNCONFIRM_INVEST_LOSS', 'USERIGHT_ASSET', 'ACCEPT_DEPOSIT_INTERBANK_YOY', 'ACCOUNTS_PAYABLE_YOY', 'ACCOUNTS_RECE_YOY', 'ACCRUED_EXPENSE_YOY',
    'ADVANCE_RECEIVABLES_YOY', 'AGENT_TRADE_SECURITY_YOY', 'AGENT_UNDERWRITE_SECURITY_YOY', 'AMORTIZE_COST_FINASSET_YOY', 'AMORTIZE_COST_FINLIAB_YOY', 'AMORTIZE_COST_NCFINASSET_YOY', 'AMORTIZE_COST_NCFINLIAB_YOY', 'APPOINT_FVTPL_FINASSET_YOY',
    'APPOINT_FVTPL_FINLIAB_YOY', 'ASSET_BALANCE_YOY', 'ASSET_OTHER_YOY', 'ASSIGN_CASH_DIVIDEND_YOY', 'AVAILABLE_SALE_FINASSET_YOY', 'BOND_PAYABLE_YOY', 'BORROW_FUND_YOY', 'BUY_RESALE_FINASSET_YOY',
    'CAPITAL_RESERVE_YOY', 'CIP_YOY', 'CONSUMPTIVE_BIOLOGICAL_ASSET_YOY', 'CONTRACT_ASSET_YOY', 'CONTRACT_LIAB_YOY', 'CONVERT_DIFF_YOY', 'CREDITOR_INVEST_YOY', 'CURRENT_ASSET_BALANCE_YOY',
    'CURRENT_ASSET_OTHER_YOY', 'CURRENT_LIAB_BALANCE_YOY', 'CURRENT_LIAB_OTHER_YOY', 'DEFER_INCOME_1YEAR_YOY', 'DEFER_INCOME_YOY', 'DEFER_TAX_ASSET_YOY', 'DEFER_TAX_LIAB_YOY', 'DERIVE_FINASSET_YOY',
    'DERIVE_FINLIAB_YOY', 'DEVELOP_EXPENSE_YOY', 'DIV_HOLDSALE_ASSET_YOY', 'DIV_HOLDSALE_LIAB_YOY', 'DIVIDEND_PAYABLE_YOY', 'DIVIDEND_RECE_YOY', 'EQUITY_BALANCE_YOY', 'EQUITY_OTHER_YOY',
    'EXPORT_REFUND_RECE_YOY', 'FEE_COMMISSION_PAYABLE_YOY', 'FIN_FUND_YOY', 'FINANCE_RECE_YOY', 'FIXED_ASSET_DISPOSAL_YOY', 'FIXED_ASSET_YOY', 'FVTOCI_FINASSET_YOY', 'FVTOCI_NCFINASSET_YOY',
    'FVTPL_FINASSET_YOY', 'FVTPL_FINLIAB_YOY', 'GENERAL_RISK_RESERVE_YOY', 'GOODWILL_YOY', 'HOLD_MATURITY_INVEST_YOY', 'HOLDSALE_ASSET_YOY', 'HOLDSALE_LIAB_YOY', 'INSURANCE_CONTRACT_RESERVE_YOY',
    'INTANGIBLE_ASSET_YOY', 'INTEREST_PAYABLE_YOY', 'INTEREST_RECE_YOY', 'INTERNAL_PAYABLE_YOY', 'INTERNAL_RECE_YOY', 'INVENTORY_YOY', 'INVEST_REALESTATE_YOY', 'LEASE_LIAB_YOY',
    'LEND_FUND_YOY', 'LIAB_BALANCE_YOY', 'LIAB_EQUITY_BALANCE_YOY', 'LIAB_EQUITY_OTHER_YOY', 'LIAB_OTHER_YOY', 'LOAN_ADVANCE_YOY', 'LOAN_PBC_YOY', 'LONG_EQUITY_INVEST_YOY',
    'LONG_LOAN_YOY', 'LONG_PAYABLE_YOY', 'LONG_PREPAID_EXPENSE_YOY', 'LONG_RECE_YOY', 'LONG_STAFFSALARY_PAYABLE_YOY', 'MINORITY_EQUITY_YOY', 'MONETARYFUNDS_YOY', 'NONCURRENT_ASSET_1YEAR_YOY',
    'NONCURRENT_ASSET_BALANCE_YOY', 'NONCURRENT_ASSET_OTHER_YOY', 'NONCURRENT_LIAB_1YEAR_YOY', 'NONCURRENT_LIAB_BALANCE_YOY', 'NONCURRENT_LIAB_OTHER_YOY', 'NOTE_ACCOUNTS_PAYABLE_YOY', 'NOTE_ACCOUNTS_RECE_YOY', 'NOTE_PAYABLE_YOY',
    'NOTE_RECE_YOY', 'OIL_GAS_ASSET_YOY', 'OTHER_COMPRE_INCOME_YOY', 'OTHER_CREDITOR_INVEST_YOY', 'OTHER_CURRENT_ASSET_YOY', 'OTHER_CURRENT_LIAB_YOY', 'OTHER_EQUITY_INVEST_YOY', 'OTHER_EQUITY_OTHER_YOY',
    'OTHER_EQUITY_TOOL_YOY', 'OTHER_NONCURRENT_ASSET_YOY', 'OTHER_NONCURRENT_FINASSET_YOY', 'OTHER_NONCURRENT_LIAB_YOY', 'OTHER_PAYABLE_YOY', 'OTHER_RECE_YOY', 'PARENT_EQUITY_BALANCE_YOY', 'PARENT_EQUITY_OTHER_YOY',
    'PERPETUAL_BOND_PAYBALE_YOY', 'PERPETUAL_BOND_YOY', 'PREDICT_CURRENT_LIAB_YOY', 'PREDICT_LIAB_YOY', 'PREFERRED_SHARES_PAYBALE_YOY', 'PREFERRED_SHARES_YOY', 'PREMIUM_RECE_YOY', 'PREPAYMENT_YOY',
    'PRODUCTIVE_BIOLOGY_ASSET_YOY', 'PROJECT_MATERIAL_YOY', 'RC_RESERVE_RECE_YOY', 'REINSURE_PAYABLE_YOY', 'REINSURE_RECE_YOY', 'SELL_REPO_FINASSET_YOY', 'SETTLE_EXCESS_RESERVE_YOY', 'SHARE_CAPITAL_YOY',
    'SHORT_BOND_PAYABLE_YOY', 'SHORT_FIN_PAYABLE_YOY', 'SHORT_LOAN_YOY', 'SPECIAL_PAYABLE_YOY', 'SPECIAL_RESERVE_YOY', 'STAFF_SALARY_PAYABLE_YOY', 'SUBSIDY_RECE_YOY', 'SURPLUS_RESERVE_YOY',
    'TAX_PAYABLE_YOY', 'TOTAL_ASSETS_YOY', 'TOTAL_CURRENT_ASSETS_YOY', 'TOTAL_CURRENT_LIAB_YOY', 'TOTAL_EQUITY_YOY', 'TOTAL_LIAB_EQUITY_YOY', 'TOTAL_LIABILITIES_YOY', 'TOTAL_NONCURRENT_ASSETS_YOY',
    'TOTAL_NONCURRENT_LIAB_YOY', 'TOTAL_OTHER_PAYABLE_YOY', 'TOTAL_OTHER_RECE_YOY', 'TOTAL_PARENT_EQUITY_YOY', 'TRADE_FINASSET_NOTFVTPL_YOY', 'TRADE_FINASSET_YOY', 'TRADE_FINLIAB_NOTFVTPL_YOY', 'TRADE_FINLIAB_YOY',
    'TREASURY_SHARES_YOY', 'UNASSIGN_RPOFIT_YOY', 'UNCONFIRM_INVEST_LOSS_YOY', 'USERIGHT_ASSET_YOY', 'OPINION_TYPE', 'OSOPINION_TYPE', 'LISTING_STATE',
]



def _make_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31",
    total_assets: str = "298944593920",  # 茅台 2024 年报实测值（元）
) -> list[str]:
    """Build a full 319-col F10 row with minimal data populated."""
    row = [""] * len(_HEADER)
    row[_HEADER.index("SECUCODE")] = secucode
    row[_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_HEADER.index("REPORT_DATE")] = report_date
    row[_HEADER.index("TOTAL_ASSETS")] = total_assets
    return row


# ── import_to_dolt tests (Dolt tempdir pattern) ──


class TestImportToDolt:
    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        """Init temp Dolt, point COMPASS_DATA_DIR at it. Returns (dir, dolt_sql_csv)."""
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
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)

        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "1"
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_balance_sheet'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

    def test_window_csv_replaces_table_full_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """A full-window CSV rebuilds the table under replace semantics."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        # Full-window CSV: watermark refetch (same row) + new symbol + older period
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
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "3"
        assert float(self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        ))) == 298944593920.0
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_balance_sheet WHERE symbol='SZ000002'"
        )) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_balance_sheet "
            "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
        )) == "1"
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_balance_sheet'"
        ).strip()
        assert "3" in row and "2024-12-31" in row

    def test_incremental_window_keeps_older_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Merge semantics: a window CSV appends/upserts without wiping history."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row(), _make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 2

        # Window CSV shape: watermark refetch + a new symbol; 2023 history absent
        self._write_csv(csv_path, [_make_row(), _make_row(secucode="000002.SZ")])
        rows = import_to_dolt(csv_path)

        assert rows == 3  # merge: 2023 history survives alongside 2024 + new symbol
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "3"
        assert self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_balance_sheet "
            "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
        )) == "1"
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_balance_sheet'"
        ).strip()
        assert "3" in row and "2024-12-31" in row

    def test_replace_overlap_uses_latest_csv_value(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Replace semantics: the latest CSV value wins for an existing (symbol, report_date)."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        # Same report_date re-fetched with a restated TOTAL_ASSETS + new symbol
        self._write_csv(
            csv_path, [_make_row(total_assets="200"), _make_row(secucode="000002.SZ")]
        )
        rows = import_to_dolt(csv_path)

        assert rows == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "2"
        # Replace: the fresh CSV overwrites the prior row with the restated value
        assert float(self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        ))) == 200.0

    def test_same_report_refetch_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Re-importing the identical CSV is idempotent — no duplicate rows."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)

        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "1"

    def test_merge_watermark_full_total_and_max_date(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Watermark row_count is the full table count after merge, not the CSV batch size."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(csv_path, [_make_row()])
        rows = import_to_dolt(csv_path)

        assert rows == 2  # merge: 2023 history remains + 2024 window row
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_balance_sheet'"
        ).strip()
        assert "2" in row and "2024-12-31" in row

    def test_csv_not_found_returns_zero(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        """When CSV does not exist, import_to_dolt returns 0 gracefully."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0

    def test_first_run_insert_failure_leaves_empty_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Merge first-run insert failure leaves the empty table (no data_updates row)."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)

        assert rows == 0
        # Merge failure does not drop the freshly created table; it stays empty.
        cnt = self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet"))
        assert cnt == "0"
        for tbl in ("_tmp_bs", "_tmp_bs_old"):
            cnt = self._last(dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables "
                f"WHERE table_name='{tbl}'"
            ))
            assert cnt == "0"
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM data_updates WHERE table_name='fin_balance_sheet'"
        ))
        assert cnt == "0"

    def test_rerun_insert_failure_preserves_prior_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """A failed re-run leaves previously imported rows and the watermark intact."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "1"
        assert float(self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet WHERE symbol='SZ000001'"
        ))) == 298944593920.0
        for tbl in ("_tmp_bs", "_tmp_bs_old"):
            cnt = self._last(dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables "
                f"WHERE table_name='{tbl}'"
            ))
            assert cnt == "0"
        row = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='fin_balance_sheet'"
        ).strip()
        assert "1" in row and "2024-12-31" in row


# ── run() tests (stub session + tmp_path) ──


class TestRun:
    async def test_run_writes_csv_with_header_and_rows(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() orchestrates build_dates→fetch_paginated→write_csv; assert CSV written."""
        from fetch_balance_sheet import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "name": "平安银行", "REPORT_DATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="FY", page_size=100)

        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"
        assert result.resolve().parent == tmp_path
        csv_path = tmp_path / "RPT_F10_FINANCE_GBALANCE.csv"
        assert csv_path.exists()

    async def test_run_single_period_no_reports_yields_csv_still(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Even with zero records, the output path is returned."""
        from fetch_balance_sheet import run  # noqa: E402

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

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="FY")

        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"

    async def test_run_with_explicit_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Exercises the explicit years branch."""
        from fetch_balance_sheet import run  # noqa: E402

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

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(years=[2023, 2024], periods="Q1")

        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"

    async def test_run_fetch_exception_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() catches and continues gracefully."""
        from fetch_balance_sheet import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            # First 4 calls raise — exhaust EM_MAX_RETRIES (4) → exception propagates to run()
            if call_count[0] <= 4:
                raise RuntimeError("simulated fetch error")
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {
                        "data": [{"code": "000001", "REPORT_DATE": "2024-12-31"}],
                        "pages": 1,
                    },
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(years=[2024], periods="Q1,Q2", page_size=100)

        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"

    async def test_run_default_years(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Call run() without years — triggers the `if years is None` default path."""
        from fetch_balance_sheet import run  # noqa: E402

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

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(periods="FY")

        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"

    async def test_run_incremental_since_date_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date returns a future date, run() returns early."""
        from fetch_balance_sheet import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_balance_sheet.last_report_date", lambda _tbl: "2099-12-31")

        result = await run(years=[2024], periods="FY")

        # Should return the output_path without writing CSV (no rows fetched)
        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"

    async def test_run_incremental_overwrites_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """run() overwrites a stale CSV from a previous run — history lives in Dolt."""
        from fetch_balance_sheet import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_balance_sheet.last_report_date", lambda _tbl: "2026-06-30")

        # Stale CSV left over from a previous full fetch
        stale = tmp_path / "RPT_F10_FINANCE_GBALANCE.csv"
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

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q2", page_size=100)

        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"
        with open(stale, encoding="utf-8-sig", newline="") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]

    async def test_run_incremental_window_starts_at_watermark(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Periods older than the watermark are skipped — only newer ones are fetched."""
        from fetch_balance_sheet import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_balance_sheet.last_report_date", lambda _tbl: "2026-06-30")

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

        with patch("fetch_balance_sheet.AsyncSession", return_value=stub):
            result = await run(years=[2026], periods="Q1,Q2", page_size=100)

        # 2026-03-31 < since is filtered; only 2026-06-30 is fetched
        assert call_count[0] == 1
        assert result.name == "RPT_F10_FINANCE_GBALANCE.csv"
        with open(tmp_path / "RPT_F10_FINANCE_GBALANCE.csv", encoding="utf-8-sig", newline="") as f:
            rows = list(csv.DictReader(f))
        assert rows == [{"code": "000001", "REPORT_DATE": "2026-06-30"}]


class TestImportToDoltEdgeCases:
    """Additional import_to_dolt coverage beyond what test_import_to_dolt.py covers."""

    def test_dolt_table_import_failure_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When dolt_table_import returns False, import_to_dolt returns 0 early."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        # Create a dummy CSV so the file-exists check passes
        csv_path = tmp_path / "RPT_F10_FINANCE_GBALANCE.csv"
        csv_path.write_text("header\n")

        # Patch at the common module level: post-GREEN fetch_balance_sheet no
        # longer holds a direct dolt_table_import binding. import_replace_table
        # calls dolt_table_import(tmp, csv, create_sql=create_sql).
        monkeypatch.setattr(
            "common.dolt_table_import",
            lambda _tbl, _pth, create_sql=None: False,
        )

        rows = import_to_dolt(csv_path)
        assert rows == 0


# ── __main__ block is covered via test_main.py dispatch tests ──
