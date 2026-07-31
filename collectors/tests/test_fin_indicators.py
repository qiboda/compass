"""Tests for fetch_fin_indicators.py — flatten_record, write_csv, fetch_period,
_last_report_date, main().
"""

import asyncio
import csv
import json
import sys
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
