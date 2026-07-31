"""Integration tests for import_to_dolt() — temp Dolt + COMPASS_DATA_DIR.

Covers the table-replacement logic: first run vs rerun, and INSERT failure
rollback safety (the RENAME TABLE IF EXISTS bug: Dolt rejects that syntax,
so existence is checked via information_schema).
"""

import csv
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from fetch_balance_sheet import import_to_dolt  # noqa: E402

# Full 57-col header (API field names, ordered as in the DDL).
_HEADER = [
    "SECUCODE",
    "SECURITY_CODE",
    "INDUSTRY_CODE",
    "ORG_CODE",
    "SECURITY_NAME_ABBR",
    "INDUSTRY_NAME",
    "MARKET",
    "SECURITY_TYPE_CODE",
    "TRADE_MARKET_CODE",
    "DATE_TYPE_CODE",
    "REPORT_TYPE_CODE",
    "DATA_STATE",
    "NOTICE_DATE",
    "REPORT_DATE",
    "TOTAL_ASSETS",
    "FIXED_ASSET",
    "MONETARYFUNDS",
    "MONETARYFUNDS_RATIO",
    "ACCOUNTS_RECE",
    "ACCOUNTS_RECE_RATIO",
    "INVENTORY",
    "INVENTORY_RATIO",
    "TOTAL_LIABILITIES",
    "ACCOUNTS_PAYABLE",
    "ACCOUNTS_PAYABLE_RATIO",
    "ADVANCE_RECEIVABLES",
    "ADVANCE_RECEIVABLES_RATIO",
    "TOTAL_EQUITY",
    "TOTAL_EQUITY_RATIO",
    "TOTAL_ASSETS_RATIO",
    "TOTAL_LIAB_RATIO",
    "CURRENT_RATIO",
    "DEBT_ASSET_RATIO",
    "CASH_DEPOSIT_PBC",
    "CDP_RATIO",
    "LOAN_ADVANCE",
    "LOAN_ADVANCE_RATIO",
    "AVAILABLE_SALE_FINASSET",
    "ASF_RATIO",
    "LOAN_PBC",
    "LOAN_PBC_RATIO",
    "ACCEPT_DEPOSIT",
    "ACCEPT_DEPOSIT_RATIO",
    "SELL_REPO_FINASSET",
    "SRF_RATIO",
    "SETTLE_EXCESS_RESERVE",
    "SER_RATIO",
    "BORROW_FUND",
    "BORROW_FUND_RATIO",
    "AGENT_TRADE_SECURITY",
    "ATS_RATIO",
    "PREMIUM_RECE",
    "PREMIUM_RECE_RATIO",
    "SHORT_LOAN",
    "SHORT_LOAN_RATIO",
    "ADVANCE_PREMIUM",
    "ADVANCE_PREMIUM_RATIO",
]


def _make_row(secucode: str = "000001.SZ") -> list[str]:
    """Build a full 57-col row; only identity + TOTAL_ASSETS populated."""
    row = [""] * len(_HEADER)
    row[_HEADER.index("SECUCODE")] = secucode
    row[_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_HEADER.index("REPORT_DATE")] = "2024-12-31"
    row[_HEADER.index("TOTAL_ASSETS")] = "100"
    return row


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
        """Last line of dolt csv output (header row + data rows)."""
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerows(rows)

    def test_first_run_creates_table_and_imports(self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path) -> None:
        dolt_dir, dolt_sql_csv = dolt_env
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

    def test_rerun_replaces_table_without_duplicates(self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path) -> None:
        dolt_dir, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)

        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "1"

    def test_first_run_insert_failure_leaves_no_table_and_no_error(self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path) -> None:
        """Regression: first-run INSERT failure must not try to RENAME a
        nonexistent _old table (Dolt rejects RENAME TABLE IF EXISTS).

        Dropping stock_basic makes the WHERE ... IN (SELECT ...) subquery fail,
        which exercises the rollback path.
        """
        dolt_dir, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)

        assert rows == 0
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='fin_balance_sheet'"
        ))
        assert cnt == "0"
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='_tmp_bs_old'"
        ))
        assert cnt == "0"

    def test_rerun_insert_failure_rolls_back_previous_data(self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path) -> None:
        """Regression: rerun with a failing import must restore the previous table."""
        dolt_dir, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_balance_sheet")) == "1"
        assert self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet WHERE symbol='SZ000001'"
        )) == "100"
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='_tmp_bs_old'"
        ))
        assert cnt == "0"
