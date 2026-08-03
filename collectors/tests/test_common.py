"""Unit tests for common.py — shared collector infrastructure."""

import asyncio
import csv
import io
import subprocess
import sys
import time
from collections.abc import Callable
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

from common import (  # noqa: E402
    Throttle,
    build_dates,
    dolt_dir,
    fetch_paginated,
    flatten_record,
    last_report_date,
    write_csv,
)


class TestBuildDates:
    def test_single_year_all_periods(self) -> None:
        dates = build_dates([2020], ["Q1", "Q2", "Q3", "FY"])
        assert dates == [
            "2020-03-31",
            "2020-06-30",
            "2020-09-30",
            "2020-12-31",
        ]

    def test_multiple_years_sorted(self) -> None:
        dates = build_dates([2022, 2020], ["FY", "Q1"])
        assert dates == [
            "2020-03-31",
            "2020-12-31",
            "2022-03-31",
            "2022-12-31",
        ]

    def test_unknown_period_ignored(self) -> None:
        dates = build_dates([2020], ["Q1", "HALF"])
        assert dates == ["2020-03-31"]

    def test_empty_years(self) -> None:
        assert build_dates([], ["Q1"]) == []


class TestFlattenRecord:
    def test_none_becomes_empty_string(self) -> None:
        assert flatten_record({"a": None}) == {"a": ""}

    def test_primitives_preserved(self) -> None:
        assert flatten_record({"i": 1, "f": 1.5, "s": "x"}) == {
            "i": 1,
            "f": 1.5,
            "s": "x",
        }

    def test_nested_converted_to_string(self) -> None:
        assert flatten_record({"nested": {"k": 1}}) == {"nested": "{'k': 1}"}


class TestWriteCsv:
    def test_writes_header_and_rows(self, tmp_path: Path) -> None:
        path = tmp_path / "out.csv"
        write_csv([{"a": 1, "b": "x"}, {"a": 2, "b": "y"}], path)
        with open(path, encoding="utf-8-sig") as f:
            reader = list(csv.DictReader(f))
        assert reader == [{"a": "1", "b": "x"}, {"a": "2", "b": "y"}]

    def test_append_adds_rows_no_duplicate_header(self, tmp_path: Path) -> None:
        path = tmp_path / "out.csv"
        write_csv([{"a": 1}], path)
        write_csv([{"a": 2}], path, append=True)
        with open(path, encoding="utf-8-sig") as f:
            lines = f.readlines()
        assert lines[0].strip() == "a"
        assert len(lines) == 3

    def test_empty_records_writes_nothing(self, tmp_path: Path) -> None:
        path = tmp_path / "out.csv"
        write_csv([], path)
        assert not path.exists()


class TestFetchPaginated:
    async def test_success_single_page(
        self, make_stub_session, monkeypatch
    ) -> None:
        """Happy path: one page of records is parsed via flatten_record."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "name": "平安银行"}],
                    "pages": 1,
                },
            }
        )
        t = Throttle(min_interval=0)
        records = await fetch_paginated(
            stub, t, "RPT_TEST", "REPORT_DATE", "2024-12-31"
        )
        assert len(records) == 1
        assert records[0]["code"] == "000001"

    async def test_429_retry_then_success(
        self, make_stub_session, monkeypatch
    ) -> None:
        """First request gets 429, second succeeds — throttle + 429 sleeps happen."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = 0

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                return StubResponse(status_code=429)
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [{"a": 1}], "pages": 1},
                }
            )

        session = make_stub_session()
        session.get = _get  # type: ignore[method-assign]

        t = Throttle(min_interval=0)
        records = await fetch_paginated(
            session, t, "RPT", "DATE", "2024-01-01"
        )
        assert len(records) == 1
        assert call_count >= 2
        assert mock_sleep.call_count >= 3

    async def test_success_false(
        self, make_stub_session, monkeypatch
    ) -> None:
        """When API returns success=False, records list is empty."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"success": False, "message": "invalid param"}
        )
        t = Throttle(min_interval=0)
        records = await fetch_paginated(
            stub, t, "RPT", "DATE", "2024-01-01"
        )
        assert records == []

    async def test_pages_cap_capped_to_500(
        self, make_stub_session, monkeypatch
    ) -> None:
        """When result.pages exceeds 500, total_pages is capped at 500.

        Page 2 returns result=None to break early (avoiding 500 real calls).
        """
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = 0

        async def _counter(*args, **kwargs):  # noqa: ANN002, ANN003
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                return StubResponse(
                    json_data={
                        "success": True,
                        "result": {"data": [{"x": 1}], "pages": 1000},
                    }
                )
            return StubResponse(
                json_data={"success": True, "result": None}
            )

        session = make_stub_session()
        session.get = _counter  # type: ignore[method-assign]

        t = Throttle(min_interval=0)
        records = await fetch_paginated(
            session, t, "RPT", "DATE", "2024-01-01"
        )
        assert len(records) == 1
        assert call_count == 2

    async def test_no_data_key_breaks(
        self, make_stub_session, monkeypatch
    ) -> None:
        """JSON without 'result.data' returns empty list (no crash)."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"success": True, "result": {}}
        )
        t = Throttle(min_interval=0)
        records = await fetch_paginated(
            stub, t, "RPT", "DATE", "2024-01-01"
        )
        assert records == []

    async def test_empty_items_breaks(
        self, make_stub_session, monkeypatch
    ) -> None:
        """Empty data list returns empty records."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [], "pages": 1},
            }
        )
        t = Throttle(min_interval=0)
        records = await fetch_paginated(
            stub, t, "RPT", "DATE", "2024-01-01"
        )
        assert records == []


class TestThrottle:
    async def test_acquire_calls_sleep(self, monkeypatch) -> None:
        """Throttle.acquire always calls asyncio.sleep (jitter)."""
        monkeypatch.setattr(time, "monotonic", lambda: 0.0)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        t = Throttle(min_interval=0.5)
        await t.acquire()
        mock_sleep.assert_called_once()

    async def test_acquire_interval_elapsed_still_sleeps(
        self, monkeypatch
    ) -> None:
        """Even when min_interval has elapsed, short jitter sleep fires."""
        monkeypatch.setattr(time, "monotonic", lambda: 100.0)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        t = Throttle(min_interval=0.5)
        await t.acquire()
        mock_sleep.assert_called_once()

    async def test_acquire_respects_min_interval(
        self, monkeypatch
    ) -> None:
        """When called quickly, the second acquire adds extra wait."""
        monkeypatch.setattr(time, "monotonic", lambda: 0.0)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        t = Throttle(min_interval=1.0)
        await t.acquire()
        await t.acquire()
        assert mock_sleep.call_count == 2


class TestDoltDir:
    def test_env_set(self, monkeypatch, tmp_path: Path) -> None:
        """COMPASS_DATA_DIR overrides the default path."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        assert dolt_dir() == tmp_path

    def test_env_unset_returns_default(self, monkeypatch) -> None:
        """When COMPASS_DATA_DIR is absent, the hard-coded default is used."""
        monkeypatch.delenv("COMPASS_DATA_DIR", raising=False)
        result = dolt_dir()
        assert result == Path("/data/compass-data/compass_data")


class TestLastReportDate:
    def test_no_dolt_dir_returns_empty(
        self, monkeypatch, tmp_path: Path
    ) -> None:
        """When .dolt sub-directory does not exist, returns empty string."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        assert last_report_date("any_table") == ""


# ── import_replace_table (shared atomic-replace import) ──────────


class TestImportReplaceTable:
    _DDL = """\
CREATE TABLE test_replace (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    value DOUBLE,
    PRIMARY KEY (symbol, trade_date)
)"""

    _INSERT = """
        INSERT INTO test_replace (symbol, trade_date, value)
        SELECT symbol, trade_date, value
        FROM _tmp_tst
        WHERE symbol IN (SELECT symbol FROM stock_basic)
    """

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
            "INSERT INTO stock_basic VALUES ('SH600519')"
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

    @staticmethod
    def _rows(stdout: str) -> list[dict[str, str]]:
        return list(csv.DictReader(io.StringIO(stdout)))

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(["symbol", "trade_date", "value"])
            writer.writerows(rows)

    def _import(self, csv_path: Path, ddl: str | None = None) -> int:
        from common import import_replace_table  # noqa: E402

        return import_replace_table(
            csv_path=csv_path,
            tmp_name="_tmp_tst",
            ddl=ddl or self._DDL,
            insert_sql=self._INSERT,
            dolt_table="test_replace",
            source_label="test source",
            last_report_expr="MAX(trade_date)",
        )

    def test_happy_path_creates_table_and_upserts_data_updates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First run: table created, rows imported, data_updates 5 columns filled."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])

        assert self._import(csv_path) == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"

        rows = self._rows(dolt_sql_csv(
            "SELECT table_name, last_updated, source, row_count, last_report_date "
            "FROM data_updates WHERE table_name='test_replace'"
        ))
        assert len(rows) == 1
        assert rows[0]["table_name"] == "test_replace"
        assert rows[0]["source"] == "test source"
        assert rows[0]["row_count"] == "1"
        assert rows[0]["last_report_date"] == "2026-07-31"
        assert rows[0]["last_updated"] != ""

    def test_csv_not_found_returns_zero(self, monkeypatch, tmp_path: Path) -> None:
        """Missing CSV → 0 without touching Dolt."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        assert self._import(tmp_path / "nonexistent.csv") == 0

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure (stock_basic dropped) drops the table cleanly."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])
        dolt_sql_csv("DROP TABLE stock_basic")

        assert self._import(csv_path) == 0
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='test_replace'"
        ))
        assert cnt == "0"
        # temp tables cleaned up
        for tbl in ("_tmp_tst", "_tmp_tst_old"):
            cnt = self._last(dolt_sql_csv(
                f"SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{tbl}'"
            ))
            assert cnt == "0"

    def test_rerun_insert_failure_rolls_back_previous_data(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores the previous table contents."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])
        assert self._import(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        assert self._import(csv_path) == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"
        val = self._last(dolt_sql_csv("SELECT value FROM test_replace"))
        assert val == "1.5"
        # watermark untouched after failed rerun
        rows = self._rows(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='test_replace'"
        ))
        assert rows[0]["row_count"] == "1"
        assert rows[0]["last_report_date"] == "2026-07-31"

    def test_ddl_failure_rolls_back_without_temp_residue(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Broken DDL on rerun: previous table restored, temp tables dropped."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])
        assert self._import(csv_path) == 1

        assert self._import(csv_path, ddl="CREATE TABLE test_replace (broken") == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"
        for tbl in ("_tmp_tst", "_tmp_tst_old"):
            cnt = self._last(dolt_sql_csv(
                f"SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{tbl}'"
            ))
            assert cnt == "0"

    def test_rerun_replaces_table_without_duplicates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Idempotency: rerunning the same CSV must not grow row count."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])

        assert self._import(csv_path) == 1
        assert self._import(csv_path) == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"

    def test_last_report_expr_controls_watermark_value(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """last_report_expr is queried against the fresh table for the watermark."""
        from common import import_replace_table  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])

        import_replace_table(
            csv_path=csv_path,
            tmp_name="_tmp_tst",
            ddl=self._DDL,
            insert_sql=self._INSERT,
            dolt_table="test_replace",
            source_label="test source",
            last_report_expr="CURDATE()",
        )
        rows = self._rows(dolt_sql_csv(
            "SELECT last_report_date FROM data_updates WHERE table_name='test_replace'"
        ))
        assert rows[0]["last_report_date"] == date.today().isoformat()


class TestImportReplaceTableMerge:
    """PIN tests for import_replace_table(merge=True) semantics (issue #160)."""

    _DDL_MERGE = """\
CREATE TABLE IF NOT EXISTS test_replace (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    value DOUBLE,
    PRIMARY KEY (symbol, trade_date)
)"""

    _INSERT_IGNORE = """
        INSERT IGNORE INTO test_replace (symbol, trade_date, value)
        SELECT symbol, trade_date, value
        FROM _tmp_tst
        WHERE symbol IN (SELECT symbol FROM stock_basic)
    """

    _DDL_PLAIN = """\
CREATE TABLE test_replace (
    symbol VARCHAR(20) NOT NULL,
    trade_date DATE NOT NULL,
    value DOUBLE,
    PRIMARY KEY (symbol, trade_date)
)"""

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
            "INSERT INTO stock_basic VALUES ('SH600519')"
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

    @staticmethod
    def _rows(stdout: str) -> list[dict[str, str]]:
        return list(csv.DictReader(io.StringIO(stdout)))

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(["symbol", "trade_date", "value"])
            writer.writerows(rows)

    def _import(self, csv_path: Path, ddl: str | None = None) -> int:
        from common import import_replace_table  # noqa: E402

        return import_replace_table(
            csv_path,
            "_tmp_tst",
            ddl or self._DDL_MERGE,
            self._INSERT_IGNORE,
            "test_replace",
            "test source",
            "MAX(trade_date)",
            merge=True,
        )

    def test_merge_first_run_creates_table_and_upserts(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First merge run: table created, row upserted, data_updates filled."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])

        assert self._import(csv_path) == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"

        rows = self._rows(dolt_sql_csv(
            "SELECT last_updated, source, row_count, last_report_date "
            "FROM data_updates WHERE table_name='test_replace'"
        ))
        assert len(rows) == 1
        assert rows[0]["source"] == "test source"
        assert rows[0]["row_count"] == "1"
        assert rows[0]["last_report_date"] == "2026-07-31"
        assert rows[0]["last_updated"] != ""

    def test_merge_incremental_csv_appends_without_loss(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Incremental window CSV appends: no history loss, original bytes intact."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_a = tmp_path / "a.csv"
        csv_b = tmp_path / "b.csv"
        self._write_csv(csv_a, [
            ["SH600519", "2026-06-30", "2.5"],
            ["SH600519", "2026-07-31", "1.5"],
        ])
        assert self._import(csv_a) == 2
        self._write_csv(csv_b, [
            ["SH600519", "2026-07-31", "1.5"],
            ["SH600519", "2026-08-31", "0.5"],
        ])

        assert self._import(csv_b) == 3
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "3"
        # original rows byte-identical after the merge
        assert self._last(dolt_sql_csv(
            "SELECT value FROM test_replace "
            "WHERE symbol='SH600519' AND trade_date='2026-06-30'"
        )) == "2.5"
        assert self._last(dolt_sql_csv(
            "SELECT value FROM test_replace "
            "WHERE symbol='SH600519' AND trade_date='2026-07-31'"
        )) == "1.5"
        assert self._last(dolt_sql_csv(
            "SELECT value FROM test_replace "
            "WHERE symbol='SH600519' AND trade_date='2026-08-31'"
        )) == "0.5"

    def test_merge_same_csv_twice_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Same CSV twice: PK dedupe keeps exactly one row."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "t.csv"
        self._write_csv(csv_path, [["SH600519", "2026-07-31", "1.5"]])

        assert self._import(csv_path) == 1
        assert self._import(csv_path) == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"

    def test_merge_watermark_full_table_count_and_max(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Watermark reflects FULL table count and MAX trade_date, not CSV size."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_a = tmp_path / "a.csv"
        csv_b = tmp_path / "b.csv"
        self._write_csv(csv_a, [["SH600519", "2026-06-30", "2.5"]])
        assert self._import(csv_a) == 1
        self._write_csv(csv_b, [["SH600519", "2026-07-31", "1.5"]])
        # merge returns the FULL table count, not this CSV's row count
        assert self._import(csv_b) == 2

        rows = self._rows(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='test_replace'"
        ))
        assert rows[0]["row_count"] == "2"
        assert rows[0]["last_report_date"] == "2026-07-31"

    def test_merge_insert_failure_preserves_rows_and_watermark(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Failed INSERT (stock_basic dropped) keeps prior rows and watermark."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_a = tmp_path / "a.csv"
        csv_b = tmp_path / "b.csv"
        self._write_csv(csv_a, [
            ["SH600519", "2026-06-30", "2.5"],
            ["SH600519", "2026-07-31", "1.5"],
        ])
        assert self._import(csv_a) == 2

        dolt_sql_csv("DROP TABLE stock_basic")
        self._write_csv(csv_b, [["SH600519", "2026-08-31", "0.5"]])
        assert self._import(csv_b) == 0

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "2"
        assert self._last(dolt_sql_csv(
            "SELECT value FROM test_replace "
            "WHERE symbol='SH600519' AND trade_date='2026-06-30'"
        )) == "2.5"
        # no temp table residue
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='_tmp_tst'"
        ))
        assert cnt == "0"
        # watermark untouched after failed merge
        rows = self._rows(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='test_replace'"
        ))
        assert rows[0]["row_count"] == "2"
        assert rows[0]["last_report_date"] == "2026-07-31"

    def test_merge_plain_ddl_silently_skips_import(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Hazard pin: plain CREATE TABLE on existing table returns 0, no change."""
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_a = tmp_path / "a.csv"
        csv_b = tmp_path / "b.csv"
        self._write_csv(csv_a, [["SH600519", "2026-07-31", "1.5"]])
        assert self._import(csv_a) == 1

        self._write_csv(csv_b, [["SH600519", "2026-08-31", "0.5"]])
        assert self._import(csv_b, ddl=self._DDL_PLAIN) == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM test_replace")) == "1"
        rows = self._rows(dolt_sql_csv(
            "SELECT row_count FROM data_updates WHERE table_name='test_replace'"
        ))
        assert rows[0]["row_count"] == "1"
