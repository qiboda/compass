"""Adversarial tests for issue #273 — the ``index_daily.csv`` column contract.

Bug under attack (fetch_index_daily.py + common.py):
- ``common.write_csv`` (common.py:407-419) infers the CSV header from the
  FIRST record's keys (``csv.DictWriter(fieldnames=list(records[0].keys()))``).
- ``fetch_index_daily._kline_records`` (fetch_index_daily.py:177-201) builds
  records WITHOUT an ``update_date`` key (:195-200), so a real ``run()``
  produces ``index_daily.csv`` lacking the ``update_date`` column.
- ``DAILY_INSERT_COLS`` (fetch_index_daily.py:147-149) and the INSERT SQL
  (:462-466) still reference ``update_date`` → Dolt merge-import fails with
  ``column "update_date" could not be found``.

Why these tests matter: existing ``TestImportToDolt`` writes the CSV with a
hand-crafted header via a private ``_write_csv`` helper — it never exercises
the real ``write_csv`` header-inference path, so the missing column slips
through. Every test below drives the REAL ``run()`` write path (no
``write_csv`` patched away), or checks the record/column contract directly.

Attack dimensions (plan/issue-declared commitment — CSV column contract must
equal ``DAILY_INSERT_COLS`` and stay consistent with DDL/INSERT):
1. Column-contract completeness — real run() CSV header == DAILY_INSERT_COLS
2. Empty-klines boundary        — empty/mixed targets keep CSV structure intact
3. Numeric round-trip           — '-', empty, huge values keep the column count
4. Import idempotency on real   — real-run CSV imports and does not double rows
5. Future-date filter           — future-dated klines dropped, contract intact
"""

import asyncio
import csv
import sys
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


import fetch_index_daily as mod  # noqa: E402
from fetch_index_daily import DAILY_INSERT_COLS, _kline_records  # noqa: E402

KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"

# The column set the module contract promises (module docstring, the DDL at
# :125-138 and DAILY_INSERT_COLS at :147-149).
EXPECTED_COLS = {
    "symbol", "trade_date", "index_type",
    "open", "close", "high", "low", "volume", "amount", "update_date",
}
# Order from the DAILY_INSERT_COLS string the INSERT SQL references.
EXPECTED_ORDER = [c.strip() for c in DAILY_INSERT_COLS.split(",")]


def _kline_row(day: str, close: float = 3000.0) -> str:
    return (
        f"{day},{close - 1},{close},{close + 1},{close - 2},"
        f"120000000,50000000000,1.5,0.5,1.0,0.5"
    )


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _read_header(path: Path) -> list[str]:
    with open(path, newline="", encoding="utf-8-sig") as f:
        return next(csv.reader(f))


def _pin_today(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(mod, "_today", lambda: __import__("datetime").date(2026, 8, 2))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


# ── dimension 1: column-contract completeness (the core RED) ──────


class TestColumnContract:
    """Real run() index_daily.csv must expose exactly DAILY_INSERT_COLS."""

    def test_kline_record_contains_update_date_key(self) -> None:
        """RED: every _kline_records record must carry an ``update_date`` key —
        write_csv forwards exactly those keys to the header (common.py:416)."""
        recs = _kline_records(
            "SH000001", "official", [_kline_row("2026-07-31")],
            __import__("datetime").date(2026, 8, 2),
        )
        assert recs, "a single kline row must map to >=1 record"
        missing = EXPECTED_COLS - set(recs[0].keys())
        assert not missing, (
            f"record missing columns {sorted(missing)} — write_csv would drop "
            f"them from the CSV header, but DAILY_INSERT_COLS references them"
        )

    async def test_real_run_csv_header_matches_insert_cols(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: the CSV the REAL run() writes (unpatched write_csv, real header
        inference) must expose every DAILY_INSERT_COLS column, in order."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31"), _kline_row("2026-07-30")]
                    )
                },
                CLIST_URL: {
                    "json_data": _clist_payload([{"f12": "BK0475", "f14": "半导体"}])
                },
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await mod.run()

        assert daily_path.exists(), "real run() must materialize index_daily.csv"
        header = _read_header(daily_path)
        assert set(header) == EXPECTED_COLS, (
            f"real CSV header {sorted(header)} != contract {sorted(EXPECTED_COLS)} "
            f"— update_date is REQUIRED by the INSERT SQL (issue #273)"
        )
        assert header == EXPECTED_ORDER, (
            f"header order {header} != DAILY_INSERT_COLS order {EXPECTED_ORDER}"
        )


# ── dimension 2: empty-klines boundary ────────────────────────────


class TestEmptyKlineBoundary:
    """Some targets empty / some full → CSV structure must stay intact."""

    async def test_all_empty_klines_no_broken_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Empty klines for every target → run() must not write a header-only
        or half-shaped CSV; whatever exists must carry the full contract."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {"json_data": _kline_payload("000001", [])},
                CLIST_URL: {"json_data": _clist_payload([{"f12": "BK0475", "f14": "x"}])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await mod.run()

        if daily_path.exists():
            with open(daily_path, newline="", encoding="utf-8-sig") as f:
                data = f.read()
            assert set(_read_header(daily_path)) == EXPECTED_COLS, (
                "any existing daily CSV must carry the full contract header"
            )
            # header-only allowed, but never data rows for empty klines.
            assert data.count("\r\n") + data.count("\n") == 1, (
                "header-only allowed; no data-body rows may exist for empty klines"
            )

    async def test_mixed_full_keeps_full_columns(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Official klines full + no boards → CSV still exposes all columns and
        every data row is the full width (no ragged rows)."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31"), _kline_row("2026-07-30")]
                    )
                },
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await mod.run()

        header = _read_header(daily_path)
        assert set(header) == EXPECTED_COLS
        with open(daily_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert rows, "official klines should produce data rows"
        for row in rows:
            assert set(row.keys()) == set(header), "ragged row in CSV"


# ── dimension 3: numeric round-trip boundary ──────────────────────


class TestNumericBoundary:
    """'-'/empty/huge numeric cells must round-trip without breaking columns."""

    async def test_numeric_edge_cells_keep_column_count(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """A kline row whose numeric cells are '-', empty, or 1e308 must not
        change the CSV column count (write_csv infers from the first record
        only, so width must be contract-stable)."""
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        # date,o,c,h,l,v,a — '-' volume, empty amount, 1e308 close.
        kline = "2026-07-31,3000,1e308,3002,2998,-,,1.5,0.5,1.0,0.5"
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {"json_data": _kline_payload("000001", [kline])},
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await mod.run()

        header = _read_header(daily_path)
        assert set(header) == EXPECTED_COLS, "numeric edges must not change columns"
        with open(daily_path, newline="", encoding="utf-8-sig") as f:
            row = list(csv.DictReader(f))[0]
        assert len(row) == len(EXPECTED_COLS), (
            f"row width {len(row)} != header width {len(EXPECTED_COLS)}"
        )


# ── dimension 4: import idempotency on the REAL run() output ──────


class TestRealCsvImportIdempotency:
    """The REAL run()-produced CSV must land in Dolt and re-importing the same
    file must not double rows (INSERT IGNORE on PK (symbol, trade_date))."""

    @staticmethod
    def _last(stdout: str) -> str:
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    async def test_real_csv_imports_and_stays_idempotent(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: the CSV produced by real run() must land >0 rows in Dolt (today
        it lacks update_date, so the INSERT references a missing column and the
        import returns 0) and a second identical import must not grow rows."""
        import subprocess

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
        dolt_dir = Path(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(dolt_dir))

        def dolt_sql_csv(sql: str) -> str:
            return subprocess.run(
                ["dolt", "--data-dir", str(dolt_dir), "sql", "-r", "csv", "-q", sql],
                capture_output=True, text=True,
            ).stdout

        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )

        # Produce the real CSV via run().
        monkeypatch.setenv("COMPASS_DATA_DIR", str(dolt_dir))
        _pin_today(monkeypatch)
        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", [_kline_row("2026-07-31"), _kline_row("2026-07-30")]
                    )
                },
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await mod.run()

        header = _read_header(daily_path)
        assert "update_date" in header, (
            "precondition: the real CSV must carry update_date for the INSERT "
            "to reference it — this is the very regression (issue #273)"
        )

        # First import on the real CSV.
        rows = mod.import_to_dolt(daily_path)
        assert rows > 0, (
            "real run() CSV must import non-zero rows; it currently returns 0 "
            'because the CSV lacks update_date ("column could not be found")'
        )
        count1 = self._last(dolt_sql_csv("SELECT COUNT(*) FROM index_daily"))
        assert count1 == "2", f"expected 2 rows after first import, got {count1}"

        # Second identical import → INSERT IGNORE dedupes on the PK.
        rows2 = mod.import_to_dolt(daily_path)
        count2 = self._last(dolt_sql_csv("SELECT COUNT(*) FROM index_daily"))
        assert rows2 == 2, "second import must report the stable total (2)"
        assert count1 == count2 == "2", (
            f"re-import must not double rows: {count1} → {count2}"
        )


# ── dimension 5: future-date filter ───────────────────────────────


class TestFutureDateFilter:
    """Future-dated klines are dropped; the column contract must survive."""

    async def test_future_dates_dropped_contract_intact(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001",
                        [_kline_row("2026-07-31"), _kline_row("2099-01-01")],
                    )
                },
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await mod.run()

        header = _read_header(daily_path)
        assert set(header) == EXPECTED_COLS, "column contract must survive the filter"
        with open(daily_path, newline="", encoding="utf-8-sig") as f:
            dates = [r["trade_date"] for r in csv.DictReader(f)]
        assert "2099-01-01" not in dates, "future-dated row must be dropped"
        assert "2026-07-31" in dates, "normal history must survive"
