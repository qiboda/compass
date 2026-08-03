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
    "SECUCODE", "SECURITY_CODE", "INDUSTRY_CODE", "ORG_CODE",
    "SECURITY_NAME_ABBR", "INDUSTRY_NAME", "MARKET", "SECURITY_TYPE_CODE",
    "TRADE_MARKET_CODE", "DATE_TYPE_CODE", "REPORT_TYPE_CODE", "DATA_STATE",
    "NOTICE_DATE", "REPORT_DATE",
    "TOTAL_ASSETS", "FIXED_ASSET", "MONETARYFUNDS", "MONETARYFUNDS_RATIO",
    "ACCOUNTS_RECE", "ACCOUNTS_RECE_RATIO", "INVENTORY", "INVENTORY_RATIO",
    "TOTAL_LIABILITIES", "ACCOUNTS_PAYABLE", "ACCOUNTS_PAYABLE_RATIO",
    "ADVANCE_RECEIVABLES", "ADVANCE_RECEIVABLES_RATIO",
    "TOTAL_EQUITY", "TOTAL_EQUITY_RATIO", "TOTAL_ASSETS_RATIO", "TOTAL_LIAB_RATIO",
    "CURRENT_RATIO", "DEBT_ASSET_RATIO", "CASH_DEPOSIT_PBC", "CDP_RATIO",
    "LOAN_ADVANCE", "LOAN_ADVANCE_RATIO", "AVAILABLE_SALE_FINASSET", "ASF_RATIO",
    "LOAN_PBC", "LOAN_PBC_RATIO", "ACCEPT_DEPOSIT", "ACCEPT_DEPOSIT_RATIO",
    "SELL_REPO_FINASSET", "SRF_RATIO", "SETTLE_EXCESS_RESERVE", "SER_RATIO",
    "BORROW_FUND", "BORROW_FUND_RATIO", "AGENT_TRADE_SECURITY", "ATS_RATIO",
    "PREMIUM_RECE", "PREMIUM_RECE_RATIO", "SHORT_LOAN", "SHORT_LOAN_RATIO",
    "ADVANCE_PREMIUM", "ADVANCE_PREMIUM_RATIO",
]


def _make_row(
    secucode: str = "000001.SZ",
    report_date: str = "2024-12-31",
    total_assets: str = "100",
) -> list[str]:
    """Build a full 57-col row with minimal data populated."""
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

    def test_incremental_merge_appends_preserving_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Incremental CSV appends to existing history instead of replacing it."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        # Incremental window: watermark refetch (same row) + new symbol + older period
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
        assert self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "100"
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

    def test_incremental_window_preserves_older_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: merge keeps pre-window rows; replace semantics wipes them."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row(), _make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 2

        # Incremental window shape: watermark refetch + a new symbol; 2023 history absent
        self._write_csv(csv_path, [_make_row(), _make_row(secucode="000002.SZ")])
        rows = import_to_dolt(csv_path)

        assert rows == 3  # RED pre-fix: replace returns 2 (2023-12-31 wiped)
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

    def test_restated_overlap_value_ignored_on_merge(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: restated value for an existing (symbol, report_date) must not clobber."""
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
        # RED pre-fix: replace overwrites with the restated "200"
        assert self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet "
            "WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "100"

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
        """RED: watermark row_count is the full table count, not the CSV batch size."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row(report_date="2023-12-31")])
        assert import_to_dolt(csv_path) == 1

        self._write_csv(csv_path, [_make_row()])
        rows = import_to_dolt(csv_path)

        assert rows == 2  # RED pre-fix: replace returns 1 (only the new window row)
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
        """On first-run insert failure the table exists but stays empty."""
        from fetch_balance_sheet import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)

        assert rows == 0
        # RED pre-fix: replace failure DROPs the whole table → COUNT(*) on missing
        # table returns "" instead of "0"
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
        assert self._last(dolt_sql_csv(
            "SELECT TOTAL_ASSETS FROM fin_balance_sheet WHERE symbol='SZ000001'"
        )) == "100"
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

        assert result.name == "RPT_DMSK_FN_BALANCE.csv"
        assert result.resolve().parent == tmp_path
        csv_path = tmp_path / "RPT_DMSK_FN_BALANCE.csv"
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

        assert result.name == "RPT_DMSK_FN_BALANCE.csv"

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

        assert result.name == "RPT_DMSK_FN_BALANCE.csv"

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

        assert result.name == "RPT_DMSK_FN_BALANCE.csv"

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

        assert result.name == "RPT_DMSK_FN_BALANCE.csv"

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
        assert result.name == "RPT_DMSK_FN_BALANCE.csv"

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
        stale = tmp_path / "RPT_DMSK_FN_BALANCE.csv"
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

        assert result.name == "RPT_DMSK_FN_BALANCE.csv"
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
        assert result.name == "RPT_DMSK_FN_BALANCE.csv"
        with open(tmp_path / "RPT_DMSK_FN_BALANCE.csv", encoding="utf-8-sig", newline="") as f:
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
        csv_path = tmp_path / "RPT_DMSK_FN_BALANCE.csv"
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
