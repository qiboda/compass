"""Integration tests for import_to_dolt() — temp Dolt + COMPASS_DATA_DIR.

Covers the merge/upsert import semantics (issue #299): first run vs rerun,
INSERT failure safety. With merge=True (upsert), a first-run INSERT failure
leaves the freshly created table empty (no data_updates row); a rerun failure
preserves the previous table contents.
"""

import csv
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from fetch_balance_sheet import COLS, import_to_dolt  # noqa: E402

# Full 319-col F10 header (API field names): COLS (318 data fields, JSON
# order) plus REPORT_DATE, which is mapped to the report_date PK column.
# Derived from the implementation so it tracks schema changes automatically.
_HEADER = [c.strip() for c in COLS.split(",")] + ["REPORT_DATE"]


def _make_row(secucode: str = "000001.SZ") -> list[str]:
    """Build a full 319-col F10 row; only identity + TOTAL_ASSETS populated."""
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

    def test_first_run_insert_failure_leaves_empty_table(self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path) -> None:
        """Merge semantics: first-run INSERT failure leaves the empty table,
        with no temp-table residue (no old table to restore).
        """
        dolt_dir, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)

        assert rows == 0
        # merge: freshly created table stays (empty); no old-table rename exists
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM fin_balance_sheet"
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
