"""Tests for fetch_dragon.py — import_to_dolt, run()."""

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

# Merged CSV header written by run() and consumed by import_to_dolt
_HEADER = [
    "SECUCODE",
    "SECURITY_CODE",
    "TRADE_DATE",
    "SEAT_TYPE",
    "BUY_AMOUNT",
    "SELL_AMOUNT",
    "NET_AMOUNT",
    "INSTITUTION_FLAG",
]


def _make_row(
    secucode: str = "000001.SZ",
    seat_type: str = "机构专用",
    trade_date: str = "2024-12-31",
    inst: str = "1",
) -> list[str]:
    return [
        secucode,
        secucode.split(".")[0],
        trade_date,
        seat_type,
        "1000",
        "400",
        "600",
        inst,
    ]


# ── import_to_dolt tests ──


class TestImportToDolt:
    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
            capture_output=True,
            text=True,
        )
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.name", "CI"],
            capture_output=True,
            text=True,
        )
        init = subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "init"],
            capture_output=True,
            text=True,
        )
        assert init.returncode == 0, init.stderr

        def dolt_sql_csv(sql: str) -> str:
            return subprocess.run(
                ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
                capture_output=True,
                text=True,
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
        from fetch_dragon import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "dragon.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM dragon_list")) == "1"

        # symbol prefix SH600519 format + seat_type/institution mapping
        seat = self._last(
            dolt_sql_csv("SELECT symbol, seat_type, institution_flag FROM dragon_list")
        )
        assert seat == "SZ000001,机构专用,1"

        # data_updates 5 columns: row_count, last_report_date, source
        up = self._last(
            dolt_sql_csv(
                "SELECT row_count, last_report_date, source FROM data_updates "
                "WHERE table_name='dragon_list'"
            )
        )
        assert up == "1,2024-12-31,EastMoney datacenter RPT_DAILYBILLBOARD_DETAILSNEW"

    def test_broker_seat_maps_to_branch_type(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Non-institution seats are imported as '营业部' with flag 0."""
        from fetch_dragon import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "dragon.csv"
        self._write_csv(csv_path, [_make_row(seat_type="营业部", inst="0")])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        seat = self._last(dolt_sql_csv("SELECT seat_type, institution_flag FROM dragon_list"))
        assert seat == "营业部,0"

    def test_rerun_replaces_table_without_duplicates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_dragon import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "dragon.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM dragon_list")) == "1"

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure drops the table cleanly."""
        from fetch_dragon import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "dragon.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        cnt = self._last(
            dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='dragon_list'"
            )
        )
        assert cnt == "0"

    def test_rerun_insert_failure_rolls_back(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores previous data."""
        from fetch_dragon import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "dragon.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM dragon_list")) == "1"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_dragon import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Stub session: run() merges BUY/SELL seat rows and writes the CSV."""
        from fetch_dragon import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {
                            "SECUCODE": "600519.SH",
                            "SECURITY_CODE": "600519",
                            "TRADE_DATE": "2024-12-30 00:00:00",
                            "OPERATEDEPT_NAME": "机构专用",
                            "BUY": 1000,
                            "SELL": 400,
                            "NET": 600,
                        },
                        {
                            "SECUCODE": "600519.SH",
                            "SECURITY_CODE": "600519",
                            "TRADE_DATE": "2024-12-30 00:00:00",
                            "OPERATEDEPT_NAME": "华泰证券股份有限公司广州云城东路证券营业部",
                            "BUY": 500,
                            "SELL": 300,
                            "NET": 200,
                        },
                        {
                            "SECUCODE": "600519.SH",
                            "SECURITY_CODE": "600519",
                            "TRADE_DATE": "2024-12-30 00:00:00",
                            "OPERATEDEPT_NAME": "深股通专用",
                            "BUY": 200,
                            "SELL": 100,
                            "NET": 100,
                        },
                    ],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_dragon.AsyncSession", return_value=stub):
            result = await run(start_date="2024-12-30", end_date="2024-12-30")

        assert result.name == "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        csv_path = tmp_path / "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        assert csv_path.exists()
        content = csv_path.read_text(encoding="utf-8-sig")
        assert "机构专用" in content
        assert "营业部" in content

        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        # BUY and SELL reports both return the same canned rows; dedupe keeps
        # exactly one aggregated row per seat type.
        inst = [r for r in rows if r["SEAT_TYPE"] == "机构专用"]
        assert len(inst) == 1
        assert inst[0]["INSTITUTION_FLAG"] == "1"
        assert float(inst[0]["BUY_AMOUNT"]) == 1000.0
        branch = [r for r in rows if r["SEAT_TYPE"] == "营业部"]
        assert len(branch) == 1
        assert branch[0]["INSTITUTION_FLAG"] == "0"
        link = [r for r in rows if r["SEAT_TYPE"] == "深股通专用"]
        assert len(link) == 1
        assert link[0]["INSTITUTION_FLAG"] == "0"

    async def test_run_incremental_resumes_after_last_date(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """start_date advances from last_report_date, fetching only newer days."""
        from fetch_dragon import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_dragon.last_report_date", lambda _tbl: "2024-12-29")

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {
                        "data": [
                            {
                                "SECUCODE": "600519.SH",
                                "SECURITY_CODE": "600519",
                                "TRADE_DATE": "2024-12-30 00:00:00",
                                "OPERATEDEPT_NAME": "机构专用",
                                "BUY": 1000,
                                "SELL": 400,
                                "NET": 600,
                            },
                        ],
                        "pages": 1,
                    },
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_dragon.AsyncSession", return_value=stub):
            result = await run(end_date="2024-12-30")

        assert result.name == "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        # Only the day after last_report_date is fetched: 2 reports x 1 day.
        assert call_count[0] == 2
        csv_path = tmp_path / "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        assert csv_path.exists()

    async def test_run_incremental_since_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date is in the future, run() returns early."""
        from fetch_dragon import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_dragon.last_report_date", lambda _tbl: "2099-12-31")

        result = await run()
        assert result.name == "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        assert not (tmp_path / "RPT_DAILYBILLBOARD_DETAILSNEW.csv").exists()

    async def test_run_default_start_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """No Dolt record and no explicit start: START_DATE after end returns early."""
        from fetch_dragon import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        result = await run(end_date="2019-12-31")
        assert result.name == "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        assert not (tmp_path / "RPT_DAILYBILLBOARD_DETAILSNEW.csv").exists()

    async def test_run_fetch_exception_aborts_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() aborts: raises and writes no CSV."""
        from fetch_dragon import run  # noqa: E402

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

        with patch("fetch_dragon.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run(start_date="2024-12-30", end_date="2024-12-31")

        assert not (tmp_path / "RPT_DAILYBILLBOARD_DETAILSNEW.csv").exists()

    async def test_run_fetch_exception_deletes_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """A failed run removes any stale CSV so import cannot publish old data."""
        from fetch_dragon import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stale = tmp_path / "RPT_DAILYBILLBOARD_DETAILSNEW.csv"
        stale.write_text("stale\n", encoding="utf-8")

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            raise RuntimeError("simulated fetch error")

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_dragon.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run(start_date="2024-12-30", end_date="2024-12-30")

        assert not stale.exists()
