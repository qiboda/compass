"""End-to-end requirement tests for the C1 index collector (issue #273).

RED target: ``_kline_records()`` builds daily records WITHOUT the
``update_date`` key, and ``common.write_csv()`` infers the CSV header from the
first record's keys — so a real ``run() → write_csv()`` pipeline produces an
``index_daily.csv`` missing the ``update_date`` column. ``DAILY_INSERT_COLS``
and the ``index_daily`` DDL both reference ``update_date``, so the import into
Dolt fails with ``column "update_date" could not be found in any table in
scope`` (2026-08-15 first real production run, 1000 板块).

The existing ``TestImportToDolt`` in ``test_index_daily.py`` bypasses this path
by hand-writing the CSV with a *manually constructed* header that already
includes ``update_date`` (``_DAILY_HEADER`` + ``_write_csv``). These tests go
through the REAL ``run()`` pipeline so the defect is not masked.

Contract under test (issue #273 acceptance):
1. ``_kline_records()`` records carry ``update_date`` (= ``_today()``) and a
   real ``run()`` emits an ``index_daily.csv`` header containing ``update_date``
   with non-empty per-row values.
2. End-to-end RED→GREEN: mock kline responses → ``run()`` produces the CSV →
   ``import_to_dolt(csv_path)`` succeeds and ``update_date`` is non-NULL in the
   Dolt ``index_daily`` table (rows > 0).
"""

import asyncio
import csv
import datetime
import subprocess
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

# Reuse the existing kline-row / payload helpers from the primary test module
# so the fixtures are identical to the current convention.
from test_index_daily import (  # noqa: E402
    CLIST_URL,
    KLINE_URL,
    _clist_payload,
    _kline_payload,
    _kline_row,
)

PINNED_TODAY = datetime.date(2026, 8, 15)  # the 2026-08-15 real failure day


def _kline_header(csv_path: Path) -> list[str] | None:
    """Read the header row of a written CSV (BOM-safe)."""
    with open(csv_path, newline="", encoding="utf-8-sig") as f:
        return csv.DictReader(f).fieldnames


class TestKlineRecordsUpdateDate:
    """Acceptance #1 — _kline_records() must tag each record with update_date."""

    def test_record_contains_update_date(self) -> None:
        """RED: _kline_records() record must carry an update_date key (= today)."""
        from fetch_index_daily import _kline_records  # noqa: E402

        records = _kline_records(
            "BK0475", "concept", [_kline_row("2026-07-31")], PINNED_TODAY
        )
        assert records, "a populated kline must yield at least one record"
        assert "update_date" in records[0], (
            "record must carry the update_date key; got "
            f"{sorted(records[0].keys())}"
        )
        assert records[0]["update_date"] == PINNED_TODAY.isoformat(), (
            f"update_date must equal the collection day {PINNED_TODAY}, got "
            f"{records[0].get('update_date')!r}"
        )


class TestEndToEndRunImport:
    """End-to-end issue #273 regression: real run() → CSV → Dolt import."""

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        """A fresh Dolt repo in tmp_path with the data_updates bootstrap table.

        Fresh repo means ``data_updates`` exists but holds no index_daily row,
        so ``last_report_date`` returns "" and run() does NOT short-circuit.
        """
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
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )
        # csv_dir() already points at tmp_path via the autouse _isolate_csv_dir
        # fixture; run()/import() read the same tmp_path Dolt repo.
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        return tmp_path, dolt_sql_csv

    @staticmethod
    def _last(stdout: str) -> str:
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    async def test_real_run_csv_imports_into_dolt(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        """RED (first assertion): mock klines → run() produces index_daily.csv
        with an update_date column → import_to_dolt() lands non-NULL rows."""
        from fetch_index_daily import import_to_dolt, run  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        monkeypatch.setattr("fetch_index_daily._today", lambda: PINNED_TODAY)
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        # Official index SH000001 (code match "000001") + one concept board.
        klines = [_kline_row("2026-07-31"), _kline_row("2026-08-14")]
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload("000001", klines),
                },
                CLIST_URL: {
                    "json_data": _clist_payload([{"f12": "BK0475", "f14": "半导体"}]),
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await run()

        # ── First RED gate: the real write_csv() header must carry update_date.
        header = _kline_header(daily_path)
        assert header is not None, "run() must have written index_daily.csv"
        assert "update_date" in header, (
            "real run() index_daily.csv header must include update_date — "
            f"got {header}; _kline_records() omits the key, so write_csv() "
            "inferred the header from record keys"
        )

        rows = []
        with open(daily_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows, "index_daily.csv must contain data rows"
        assert all(r.get("update_date") for r in rows), (
            "every index_daily.csv row must carry a non-empty update_date"
        )
        shr = next(r for r in rows if r["symbol"] == "SH000001")
        assert shr["update_date"] == PINNED_TODAY.isoformat(), (
            f"SH000001 update_date must be {PINNED_TODAY.isoformat()}, got "
            f"{shr['update_date']!r}"
        )

        # ── Second RED gate: importing THAT generated CSV must land non-NULL rows.
        count = import_to_dolt(daily_path)
        assert count > 0, (
            "import_to_dolt() of the real run() CSV must return > 0 rows; "
            "absent update_date in the CSV, DAILY_INSERT_COLS SELECT fails"
        )
        assert self._last(
            dolt_sql_csv("SELECT COUNT(*) FROM index_daily")
        ) == str(count), "Dolt index_daily row count must match the import result"

        upd = self._last(
            dolt_sql_csv(
                "SELECT GROUP_CONCAT(IFNULL(update_date,'NULL')) FROM index_daily"
            )
        )
        assert "NULL" not in upd and upd, (
            "Dolt index_daily.update_date must be non-NULL for every row; got "
            f"{upd!r}"
        )
        assert PINNED_TODAY.isoformat() in upd, (
            f"imported update_date must reflect collection day {PINNED_TODAY.isoformat()}"
        )
