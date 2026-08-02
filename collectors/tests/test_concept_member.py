"""Tests for fetch_concept_member.py — import_to_dolt, run().

Concept board membership collector (概念板块成分).
Version-tracking semantics: each run fully replaces the previous version
(DELETE + full INSERT), no per-trading-day snapshots.
"""

import asyncio
import csv
import io
import subprocess
import sys
from collections.abc import Callable
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

from fetch_concept_member import BOARD_LIST_URL  # noqa: E402

# CSV columns written by run() — raw EastMoney fields, symbol is derived
# at import time (same pattern as fetch_income.py).
_HEADER = ["SECUCODE", "SECURITY_CODE", "NEW_BOARD_CODE", "BOARD_NAME"]


def _make_row(
    secucode: str = "600880.SH",
    board_code: str = "BK1169",
    board_name: str = "Kimi概念",
) -> list[str]:
    return [secucode, secucode.split(".")[0], board_code, board_name]


def _last(stdout: str) -> str:
    lines = stdout.strip().split("\n")
    return lines[-1] if lines else ""


def _rows(stdout: str) -> list[dict[str, str]]:
    return list(csv.DictReader(io.StringIO(stdout)))


def _board_list_json(boards: list[tuple[str, str]]) -> dict:
    """Board-list response (push2 clist format: f12 code / f14 name)."""
    return {
        "rc": 0,
        "data": {
            "total": len(boards),
            "diff": [{"f12": code, "f14": name} for code, name in boards],
        },
    }


def _member_json(members: list[dict]) -> dict:
    """Member response (datacenter format)."""
    return {
        "success": True,
        "result": {"count": len(members), "pages": 1, "data": members},
    }


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
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        return tmp_path, dolt_sql_csv

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerows(rows)

    def test_first_run_creates_table_and_imports(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_concept_member import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"
        self._write_csv(
            csv_path,
            [
                _make_row("600880.SH", "BK1169", "Kimi概念"),
                _make_row("300624.SZ", "BK1169", "Kimi概念"),
                _make_row("603999.SH", "BK1170", "光刻胶"),
            ],
        )

        rows = import_to_dolt(csv_path)
        assert rows == 3
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "3"

        # symbols are prefixed SH600880-style (same convention as other collectors)
        symbols = [
            r["symbol"]
            for r in _rows(dolt_sql_csv("SELECT symbol FROM concept_member ORDER BY symbol"))
        ]
        assert symbols == ["SH600880", "SH603999", "SZ300624"]

        # concept_code / concept_name land in the table
        board = _last(dolt_sql_csv(
            "SELECT concept_name FROM concept_member WHERE concept_code='BK1169' LIMIT 1"
        ))
        assert board == "Kimi概念"

        # update_date is the current date (version date)
        upd = _last(dolt_sql_csv("SELECT MAX(update_date) FROM concept_member"))
        assert upd == date.today().isoformat()

        # data_updates 5-column upsert
        du = _last(dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='concept_member'"
        ))
        assert du == f"3,{date.today().isoformat()}"

    def test_rerun_replaces_version_without_stale_members(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Version update idempotency: 50 members → 45 member rerun removes the
        dropped 5 (deletion propagates into the current version)."""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"

        # Version 1: 50 members (BK1169 Kimi概念, SZ codes 600001..600050)
        rows_50 = [_make_row(f"6000{i:02d}.SZ", "BK1169", "Kimi概念") for i in range(1, 51)]
        self._write_csv(csv_path, rows_50)
        assert import_to_dolt(csv_path) == 50
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "50"

        # Version 2: 45 members (last 5 removed: 600046..600050)
        rows_45 = rows_50[:45]
        self._write_csv(csv_path, rows_45)
        assert import_to_dolt(csv_path) == 45
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "45"

        # Removed members no longer exist (deletion propagates to current version)
        removed = ",".join(f"'SZ6000{i:02d}'" for i in range(46, 51))
        gone = _last(dolt_sql_csv(
            f"SELECT COUNT(*) FROM concept_member WHERE symbol IN ({removed})"
        ))
        assert gone == "0"

        # Remaining members still exist
        kept = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM concept_member WHERE symbol IN ('SZ600001','SZ600045')"
        ))
        assert kept == "2"

        # data_updates row_count tracks the new version
        du = _last(dolt_sql_csv(
            "SELECT row_count FROM data_updates WHERE table_name='concept_member'"
        ))
        assert du == "45"

    def test_rerun_same_version_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerunning the same version: row count unchanged, no duplicates (PK unique)."""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"
        rows = [_make_row(f"6000{i:02d}.SZ", "BK1169", "Kimi概念") for i in range(1, 11)]
        self._write_csv(csv_path, rows)

        assert import_to_dolt(csv_path) == 10
        assert import_to_dolt(csv_path) == 10
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "10"

    def test_insert_failure_rolls_back_previous_version(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores the previous version's data."""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"
        self._write_csv(csv_path, [_make_row("600880.SH", "BK1169", "Kimi概念")])
        assert import_to_dolt(csv_path) == 1

        # Destructive CSV: empty NEW_BOARD_CODE → NULL concept_code violates NOT NULL
        with open(csv_path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerow(["600880.SH", "600880", "", "Kimi概念"])
        assert import_to_dolt(csv_path) == 0
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "1"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """import_to_dolt returns 0 when the CSV does not exist."""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0

    async def test_failed_run_does_not_publish_new_version(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Board fetch failure → run() aborts → import refuses (0) → old version kept."""
        from fetch_concept_member import import_to_dolt, run  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"
        self._write_csv(csv_path, [_make_row("600880.SH", "BK1169", "Kimi概念")])
        assert import_to_dolt(csv_path) == 1
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "1"

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        # Stale CSV from a previous run must be removed by the failed run
        stale = tmp_path / "RPT_F10_CORETHEME_BOARDTYPE.csv"
        stale.write_text("stale\n", encoding="utf-8")

        boards = [("BK1169", "Kimi概念"), ("BK1170", "光刻胶")]

        async def _get(url: str, params: dict | None = None, headers: dict | None = None):  # noqa: ANN001, ANN002, ANN003
            if BOARD_LIST_URL in url:
                return StubResponse(json_data=_board_list_json(boards))
            flt = (params or {}).get("filter", "")
            code = flt.split('"')[1] if '"' in flt else ""
            if code == "BK1170":
                raise RuntimeError("simulated fetch error")
            return StubResponse(json_data=_member_json([]))

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        # No new CSV to import → old version stays in Dolt, watermark untouched
        assert not stale.exists()
        assert import_to_dolt() == 0
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "1"


# ── run() tests ──


class TestRun:
    def _make_get(
        self,
        boards: list[tuple[str, str]],
        members: dict[str, list[dict]],
    ) -> Callable:
        """Custom stub.get: board-list URL → board list; otherwise per-filter members."""

        async def _get(url: str, params: dict | None = None, headers: dict | None = None):  # noqa: ANN001, ANN002, ANN003
            if BOARD_LIST_URL in url:
                return StubResponse(json_data=_board_list_json(boards))
            flt = (params or {}).get("filter", "")
            code = flt.split('"')[1] if '"' in flt else ""
            return StubResponse(json_data=_member_json(members.get(code, [])))

        return _get

    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_concept_member import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        boards = [("BK1169", "Kimi概念"), ("BK1170", "光刻胶")]
        members = {
            "BK1169": [
                {"SECUCODE": "600880.SH", "SECURITY_CODE": "600880",
                 "NEW_BOARD_CODE": "BK1169", "BOARD_NAME": "Kimi概念"},
                {"SECUCODE": "300624.SZ", "SECURITY_CODE": "300624",
                 "NEW_BOARD_CODE": "BK1169", "BOARD_NAME": "Kimi概念"},
            ],
            "BK1170": [
                {"SECUCODE": "603999.SH", "SECURITY_CODE": "603999",
                 "NEW_BOARD_CODE": "BK1170", "BOARD_NAME": "光刻胶"},
            ],
        }

        stub = make_stub_session()
        stub.get = self._make_get(boards, members)  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_F10_CORETHEME_BOARDTYPE.csv"
        csv_path = tmp_path / result.name
        assert csv_path.exists()
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert len(rows) == 3
        assert {r["SECUCODE"] for r in rows} == {"600880.SH", "300624.SZ", "603999.SH"}
        assert {r["NEW_BOARD_CODE"] for r in rows} == {"BK1169", "BK1170"}

    async def test_run_member_fetch_exception_aborts_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """One board failing to fetch aborts the whole run: raises, no CSV."""
        from fetch_concept_member import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        boards = [("BK1169", "Kimi概念"), ("BK1170", "光刻胶")]
        members = {
            "BK1169": [
                {"SECUCODE": "600880.SH", "SECURITY_CODE": "600880",
                 "NEW_BOARD_CODE": "BK1169", "BOARD_NAME": "Kimi概念"},
            ],
        }

        async def _get(url: str, params: dict | None = None, headers: dict | None = None):  # noqa: ANN001, ANN002, ANN003
            if BOARD_LIST_URL in url:
                return StubResponse(json_data=_board_list_json(boards))
            flt = (params or {}).get("filter", "")
            code = flt.split('"')[1] if '"' in flt else ""
            if code == "BK1170":
                raise RuntimeError("simulated fetch error")
            return StubResponse(json_data=_member_json(members.get(code, [])))

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        assert not (tmp_path / "RPT_F10_CORETHEME_BOARDTYPE.csv").exists()

    async def test_run_board_list_fetch_exception_aborts(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Board-list fetch failure aborts: raises, no CSV."""
        from fetch_concept_member import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        async def _get(url: str, params: dict | None = None, headers: dict | None = None):  # noqa: ANN001, ANN002, ANN003
            raise RuntimeError("simulated fetch error")

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        assert not (tmp_path / "RPT_F10_CORETHEME_BOARDTYPE.csv").exists()

    async def test_run_empty_board_list_aborts(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Empty board list means no version can be produced: run() raises."""
        from fetch_concept_member import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session()
        stub.get = self._make_get([], {})  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        assert not (tmp_path / "RPT_F10_CORETHEME_BOARDTYPE.csv").exists()
