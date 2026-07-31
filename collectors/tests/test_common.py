"""Unit tests for common.py — shared collector infrastructure."""

import csv
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from common import build_dates, flatten_record, write_csv  # noqa: E402


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
