"""Tests for fetch_institution_survey.py — import_to_dolt, run()."""

import asyncio
import csv
import subprocess
import sys
from collections.abc import Callable
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# CSV columns consumed by import_to_dolt (subset of RPT_ORG_SURVEYNEW fields)
_HEADER = [
    "SECUCODE", "SECURITY_CODE", "RECEIVE_START_DATE", "RECEIVE_OBJECT",
    "RECEIVE_WAY_EXPLAIN",
]


def _make_row(
    secucode: str = "000001.SZ",
    receive_start: str = "2025-08-28 00:00:00",
    receive_object: str = "长信基金",
    receive_way: str = "电话会议",
) -> list[str]:
    return [secucode, secucode.split(".")[0], receive_start, receive_object, receive_way]


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
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "survey.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey")) == "1"

        row = self._last(dolt_sql_csv(
            "SELECT symbol, survey_date, org_name, survey_type, update_date "
            "FROM institution_survey"
        ))
        assert row == f"SZ000001,2025-08-28,长信基金,电话会议,{date.today().isoformat()}"

        upd = self._last(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='institution_survey'"
        ))
        assert upd == "1,2025-08-28"

    def test_rerun_replaces_table_without_duplicates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "survey.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey")) == "1"

    def test_duplicate_pk_rows_deduped(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Same (symbol, survey_date, org_name) with different ways dedupes to one row."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "survey.csv"
        # Real-world case: 000001 has two rows for one receive date, same org
        self._write_csv(
            csv_path,
            [
                _make_row(receive_object="境内投资者", receive_way="路演活动,实地调研"),
                _make_row(receive_object="境内投资者", receive_way="路演活动,实地会议"),
            ],
        )

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey")) == "1"

    def test_empty_receive_start_date_row_filtered_out(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """CSV rows with empty RECEIVE_START_DATE (NULL in tmp table) are skipped
        by the WHERE guard — without it the NOT NULL survey_date PK fails."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "survey.csv"
        self._write_csv(csv_path, [_make_row(receive_start=""), _make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey")) == "1"
        org = self._last(dolt_sql_csv("SELECT org_name FROM institution_survey"))
        assert org == "长信基金"

    def test_ddl_survey_type_width_50(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """survey_type column is VARCHAR(50) to fit long method descriptions."""
        import fetch_institution_survey  # noqa: E402

        assert "survey_type VARCHAR(50)" in fetch_institution_survey.DDL

    async def test_run_to_import_round_trip(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """run() output CSV feeds import_to_dolt: real Dolt table gets the rows."""
        from fetch_institution_survey import import_to_dolt, run  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{
                        "SECUCODE": "000001.SZ",
                        "SECURITY_CODE": "000001",
                        "RECEIVE_START_DATE": "2025-08-28 00:00:00",
                        "RECEIVE_OBJECT": "长信基金",
                        "RECEIVE_WAY_EXPLAIN": "电话会议",
                    }],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_institution_survey.AsyncSession", return_value=stub):
            result = await run(start_date="2025-08-28", page_size=100)

        csv_path = tmp_path / result.name
        assert csv_path.exists()

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey")) == "1"
        symbol = self._last(dolt_sql_csv("SELECT symbol FROM institution_survey"))
        assert symbol == "SZ000001"

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure drops the table cleanly."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "survey.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='institution_survey'"
        ))
        assert cnt == "0"

    def test_rerun_insert_failure_rolls_back(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores previous data."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "survey.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM institution_survey")) == "1"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_institution_survey import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_institution_survey import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{
                        "SECUCODE": "000001.SZ",
                        "SECURITY_CODE": "000001",
                        "RECEIVE_START_DATE": "2025-08-28 00:00:00",
                        "RECEIVE_OBJECT": "长信基金",
                        "RECEIVE_WAY_EXPLAIN": "电话会议",
                    }],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_institution_survey.AsyncSession", return_value=stub):
            result = await run(start_date="2025-08-28")

        assert result.name == "RPT_ORG_SURVEYNEW.csv"
        csv_path = tmp_path / "RPT_ORG_SURVEYNEW.csv"
        assert csv_path.exists()

        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            row = next(csv.DictReader(f))
        assert row["SECUCODE"] == "000001.SZ"
        assert row["SECURITY_CODE"] == "000001"
        assert row["RECEIVE_START_DATE"].startswith("2025-08-28")

    async def test_run_default_start_date(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Call run() without start_date — triggers the default start path."""
        from fetch_institution_survey import run  # noqa: E402

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

        with patch("fetch_institution_survey.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_ORG_SURVEYNEW.csv"

    async def test_run_incremental_since_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date is in the future, run() returns early."""
        from fetch_institution_survey import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr(
            "fetch_institution_survey.last_report_date", lambda _tbl: "2099-12-31"
        )

        result = await run()
        assert result.name == "RPT_ORG_SURVEYNEW.csv"

    async def test_run_fetch_exception_aborts_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() aborts: raises and writes no CSV."""
        from fetch_institution_survey import run  # noqa: E402

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

        with patch("fetch_institution_survey.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run(start_date="2025-08-28", page_size=100)

        assert not (tmp_path / "RPT_ORG_SURVEYNEW.csv").exists()
