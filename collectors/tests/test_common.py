"""Unit tests for common.py — shared collector infrastructure."""

import asyncio
import csv
import sys
import time
from pathlib import Path
from unittest.mock import AsyncMock

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
