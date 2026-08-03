"""Tests for fetch_fin_indicators.py — flatten_record, write_csv, fetch_period,
_last_report_date, main().
"""

import asyncio
import csv
import json
import sys
from datetime import datetime
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Import the module-under-test's own copies of these functions
import fetch_fin_indicators  # noqa: E402

# ── flatten_record (duplicate of common.py's version) ──


class TestFlattenRecord:
    def test_none_becomes_empty_string(self) -> None:
        assert fetch_fin_indicators.flatten_record({"a": None}) == {"a": ""}

    def test_primitives_preserved(self) -> None:
        assert fetch_fin_indicators.flatten_record({"i": 1, "f": 1.5, "s": "x"}) == {
            "i": 1,
            "f": 1.5,
            "s": "x",
        }

    def test_nested_converted_to_string(self) -> None:
        assert fetch_fin_indicators.flatten_record({"nested": {"k": 1}}) == {"nested": "{'k': 1}"}


# ── write_csv (duplicate of common.py's version) ──


class TestWriteCsv:
    def test_writes_header_and_rows(self, tmp_path: Path) -> None:
        path = tmp_path / "out.csv"
        fetch_fin_indicators.write_csv(
            [{"a": 1, "b": "x"}, {"a": 2, "b": "y"}], path
        )
        with open(path, encoding="utf-8-sig") as f:
            reader = list(csv.DictReader(f))
        assert reader == [{"a": "1", "b": "x"}, {"a": "2", "b": "y"}]

    def test_append_adds_rows_no_duplicate_header(self, tmp_path: Path) -> None:
        path = tmp_path / "out.csv"
        fetch_fin_indicators.write_csv([{"a": 1}], path)
        fetch_fin_indicators.write_csv([{"a": 2}], path, append=True)
        with open(path, encoding="utf-8-sig") as f:
            lines = f.readlines()
        assert lines[0].strip() == "a"
        assert len(lines) == 3

    def test_empty_records_writes_nothing(self, tmp_path: Path) -> None:
        path = tmp_path / "out.csv"
        fetch_fin_indicators.write_csv([], path)
        assert not path.exists()

    def test_append_new_file_writes_header(self, tmp_path: Path) -> None:
        """append=True to non-existent file still writes header."""
        path = tmp_path / "new.csv"
        fetch_fin_indicators.write_csv([{"x": 1}], path, append=True)
        with open(path, encoding="utf-8-sig") as f:
            lines = f.readlines()
        assert lines[0].strip() == "x"
        assert len(lines) == 2


# ── fetch_period (stub session) ──


class TestFetchPeriod:
    async def test_success_single_page(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
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
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT_LICO_FN_CPD", "2024-12-31"
        )
        assert len(records) == 1
        assert records[0]["code"] == "000001"

    async def test_429_retry_then_success(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            if call_count[0] == 1:
                return StubResponse(status_code=429)
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [{"a": 1}], "pages": 1},
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert len(records) == 1
        assert call_count[0] >= 2
        assert mock_sleep.call_count >= 3  # throttle + 429 wait + more throttle

    async def test_failure_raises_after_max_retries(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """After EM_MAX_RETRIES failures, the exception propagates."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        exc = RuntimeError("simulated failure")

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            raise exc

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        t = fetch_fin_indicators.Throttle(min_interval=0)
        with pytest.raises(RuntimeError, match="simulated failure"):
            await fetch_fin_indicators.fetch_period(
                stub, t, "RPT", "2024-01-01"
            )

    async def test_success_false_breaks(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(json_data={"success": False, "message": "boom"})
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert records == []

    async def test_result_none_breaks(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(json_data={"success": True, "result": None})
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert records == []

    async def test_empty_items_breaks(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert records == []


class TestThrottle:
    async def test_acquire_waits_when_below_min_interval(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Second acquire within min_interval takes the asyncio.sleep wait branch."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        t = fetch_fin_indicators.Throttle(min_interval=10)
        await t.acquire()
        await t.acquire()
        assert mock_sleep.call_count >= 2


# ── _last_report_date ──


class TestLastReportDate:
    def test_dolt_absent_falls_back_to_state_json(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """When .dolt dir does NOT exist, _last_report_date reads state.json."""
        state_path = tmp_path / "RPT_TEST.state.json"
        state_path.write_text(json.dumps({"last_report_date": "2024-12-31"}))

        # Patch the dolt_dir path in the module to point to a non-existent dir
        # so the Dolt check always fails, testing the fallback path.
        with patch.object(
            fetch_fin_indicators.Path, "__truediv__", return_value=tmp_path / "no_dolt"
        ):
            result = fetch_fin_indicators._last_report_date("RPT_TEST", state_path)

        assert result == "2024-12-31"

    def test_dolt_absent_and_no_state_json_returns_empty(
        self, tmp_path: Path
    ) -> None:
        """No .dolt dir, no state.json → returns ''."""
        state_path = tmp_path / "nonexistent.state.json"

        with patch.object(
            fetch_fin_indicators.Path, "__truediv__", return_value=tmp_path / "no_dolt"
        ):
            result = fetch_fin_indicators._last_report_date("RPT_TEST", state_path)

        assert result == ""

    def test_dolt_absent_state_json_missing_last_report_date(
        self, tmp_path: Path
    ) -> None:
        """State.json exists but lacks last_report_date key → returns ''."""
        state_path = tmp_path / "RPT.state.json"
        state_path.write_text(json.dumps({"other": "data"}))

        with patch.object(
            fetch_fin_indicators.Path, "__truediv__", return_value=tmp_path / "no_dolt"
        ):
            result = fetch_fin_indicators._last_report_date("RPT", state_path)

        assert result == ""

    @staticmethod
    def _init_dolt(tmp_path: Path) -> None:
        """Init a temp Dolt repo at tmp_path (mirrors test_import_to_dolt.py)."""
        import subprocess

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

    def test_dolt_subprocess_returns_max_report_date(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Real temp Dolt: SELECT MAX(report_date) returned via subprocess."""
        import subprocess

        self._init_dolt(tmp_path)
        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(tmp_path),
                "sql",
                "-r",
                "csv",
                "-q",
                "CREATE TABLE fin_indicators (symbol VARCHAR(20) PRIMARY KEY, "
                "report_date DATE NOT NULL); "
                "INSERT INTO fin_indicators VALUES ('SZ000001', '2024-12-31')",
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, result.stderr

        real_truediv = Path.__truediv__

        def _redirect(self, other):  # noqa: ANN001, ANN002
            return tmp_path if other == "compass_data" else real_truediv(self, other)

        monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)

        state_path = tmp_path / "nonexistent.state.json"
        result = fetch_fin_indicators._last_report_date("RPT_LICO_FN_CPD", state_path)

        assert result == "2024-12-31"

    def test_dolt_subprocess_null_falls_back_to_state(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Empty table → MAX is NULL → falls back to state.json."""
        import subprocess

        self._init_dolt(tmp_path)
        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(tmp_path),
                "sql",
                "-r",
                "csv",
                "-q",
                "CREATE TABLE fin_indicators (symbol VARCHAR(20) PRIMARY KEY, "
                "report_date DATE NOT NULL)",
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, result.stderr

        real_truediv = Path.__truediv__

        def _redirect(self, other):  # noqa: ANN001, ANN002
            return tmp_path if other == "compass_data" else real_truediv(self, other)

        monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)

        state_path = tmp_path / "RPT.state.json"
        state_path.write_text(json.dumps({"last_report_date": "2024-12-31"}))

        result = fetch_fin_indicators._last_report_date("RPT_LICO_FN_CPD", state_path)

        assert result == "2024-12-31"

    def test_dolt_subprocess_missing_table_returns_empty(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Query error (missing table) with no state file → returns ''."""
        self._init_dolt(tmp_path)

        real_truediv = Path.__truediv__

        def _redirect(self, other):  # noqa: ANN001, ANN002
            return tmp_path if other == "compass_data" else real_truediv(self, other)

        monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)

        state_path = tmp_path / "nonexistent.state.json"
        result = fetch_fin_indicators._last_report_date("RPT_LICO_FN_CPD", state_path)

        assert result == ""


# ── main() ──


class TestMain:
    async def test_main_with_report_name_writes_csv_and_state_json(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--report-name", "RPT_CUSTOM", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        csv_path = tmp_path / "RPT_CUSTOM.csv"
        assert csv_path.exists()

        state_path = tmp_path / "RPT_CUSTOM.state.json"
        assert state_path.exists()
        state = json.loads(state_path.read_text())
        assert state["last_report_date"] == "2024-12-31"
        assert state["total_rows"] == 1

    async def test_default_years_covers_2020_to_now(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        class FakeDatetime(datetime):
            @classmethod
            def now(cls, tz=None):
                return cls(2020, 6, 1, 12, 0, 0)

        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(fetch_fin_indicators, "datetime", FakeDatetime)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2020-12-31"}],
                    "pages": 1,
                },
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert state["last_report_date"] == "2020-12-31"

    async def test_incremental_filters_dates(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_last_report_date", lambda *a, **k: "2023-01-01"
        )
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists()

    async def test_incremental_no_new_periods_returns(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_last_report_date", lambda *a, **k: "2025-01-01"
        )
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [{"code": "000001"}], "pages": 1},
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()

    async def test_incremental_no_prior_data_full_fetch(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_last_report_date", lambda *a, **k: ""
        )
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2024-12-31"}],
                    "pages": 1,
                },
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists()

    async def test_period_fetch_failure_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys
    ) -> None:
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(exc=RuntimeError("boom"))
        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()
        assert "FAILED: boom" in capsys.readouterr().err

    async def test_empty_records_prints_empty(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()
