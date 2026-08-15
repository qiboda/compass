"""Requirement-acceptance tests for the C1 index collector (epic #255, plan T1).

This file verifies the *functional contract* declared by the plan's acceptance
criteria — happy path + basic error path only. The adversarial coverage
(boundary codes, 429 exhaustion, CSV injection, pagination, PK dedup
idempotency) lives in the sibling files test_index_daily.py and
test_index_main_cli.py and is NOT repeated here.

Contract under test (fetch_index_daily.py):
- C1a. Three index classes — official (hardcoded whitelist), concept (clist
  ``fs=m:90 t:3``), industry (``fs=m:90 t:2``) — are pulled and every daily
  row carries the correct ``index_type`` tag; row counts equal the sum of the
  per-symbol kline rows (full history).
- C1b. ``index_basic`` carries name records for BOTH the official whitelist
  and the discovered boards — the GUI picker's merged ~6500 list and the
  market tab's board table both read names from index_basic, so official
  index names must be present, not only board names.
- C1c. ``import_to_dolt`` lands the plan's full DDL column set
  (symbol, trade_date, index_type, open/close/high/low/volume/amount,
  update_date) with correct numeric values, not just the PK columns.

STATUS: ready-and-waiting. ``fetch_index_daily.py`` does not exist yet, so
this module fails to collect until the first compilable interface commit
lands. Every assertion targets a plan-declared behavior; the GREEN
implementation must make them all pass.
"""

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

# Handoff-verified endpoints (fetch_index_daily must use these).
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"

# 东财 kline 11 字段: 日期,开盘,收盘,最高,最低,成交量,成交额,振幅,涨跌幅,涨跌额,换手率
def _kline_row(day: str, close: float = 3000.0) -> str:
    return (
        f"{day},{close - 1},{close},{close + 1},{close - 2},"
        f"120000000,50000000000,1.5,0.5,1.0,0.5"
    )


def _kline_payload(code: str, name: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": name, "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _patch_write_csv(monkeypatch: pytest.MonkeyPatch) -> list[list[dict[str, object]]]:
    """Capture every CSV batch the collector writes (both index_daily and
    index_basic), mirroring the adversarial test convention."""
    captured: list[list[dict[str, object]]] = []
    monkeypatch.setattr(
        "fetch_index_daily.write_csv",
        lambda records, _path: captured.append(records),
    )
    return captured


class TestFetchHappyPath:
    """C1a/C1b — three index classes, full-history row counts, index_basic names."""

    @staticmethod
    def _five_target_stub(make_stub_session) -> StubResponse:
        """Three official indices + one concept board + one industry board.

        secid → (symbol, index_type, kline rows). Two klines per target so
        the full-history row count is deterministic: 5 targets × 2 = 10 rows.
        """
        official = {
            "1.000001": ("SH000001", "official"),
            "0.399001": ("SZ399001", "official"),
            "0.399006": ("SZ399006", "official"),
        }
        boards = {
            "90.BK0475": ("BK0475", "concept"),
            "90.BK0476": ("BK0476", "industry"),
        }
        klines = ["2026-07-30", "2026-07-31"]
        kline_by_secid = {
            secid: _kline_payload(
                code, name, [_kline_row(day) for day in klines]
            )
            for secid, (code, name) in {**official, **boards}.items()
        }
        # clist: concept t:3 → BK0475; industry t:2 → BK0476.
        clist_by_fs = {
            "m:90 t:3": _clist_payload([{"f12": "BK0475", "f14": "半导体"}]),
            "m:90 t:2": _clist_payload([{"f12": "BK0476", "f14": "白酒"}]),
        }

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            params = params or {}
            if url == KLINE_URL:
                secid = params.get("secid", "")
                if secid in kline_by_secid:
                    return StubResponse(json_data=kline_by_secid[secid])
                # Officials outside the 3 under test return a code-mismatch
                # skip: neither a success nor a failure, so they add no rows
                # and cannot trigger the fast-fail streak (issue #277).
                return StubResponse(
                    json_data=_kline_payload(
                        "999999", "unknown", [_kline_row("2026-07-30")]
                    )
                )
            if url == CLIST_URL:
                fs = params.get("fs", "")
                # fs arrives as "m:90 t:3 f:!50" — match on the t: segment.
                for key, payload in clist_by_fs.items():
                    if key in fs:
                        return StubResponse(json_data=payload)
                return StubResponse(json_data=_clist_payload([]))
            return StubResponse(status_code=404, json_data={})

        stub.get = _get  # type: ignore[method-assign]
        return stub

    async def test_three_index_classes_full_history_row_count(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """C1a: 3 official + 2 boards × 2 klines → 10 daily rows; every row
        tagged with its index_type; index_basic carries 5 name records."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(
            "fetch_index_daily._today",
            lambda: __import__("datetime").date(2026, 8, 2),
        )
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        captured = _patch_write_csv(monkeypatch)

        stub = self._five_target_stub(make_stub_session)
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        daily = [r for r in rows if "name" not in r]
        basic = [r for r in rows if "name" in r]

        # Full-history row count: every kline of every target lands.
        assert len(daily) == 10, (
            f"5 targets × 2 klines must yield 10 daily rows; got {len(daily)}"
        )

        # Per-row index_type for all three classes.
        type_of = {r["symbol"]: r["index_type"] for r in daily}
        assert type_of["SH000001"] == "official"
        assert type_of["SZ399001"] == "official"
        assert type_of["SZ399006"] == "official"
        assert type_of["BK0475"] == "concept"
        assert type_of["BK0476"] == "industry"
        assert set(type_of.values()) == {"official", "concept", "industry"}, (
            "all three index classes must be present"
        )

        # index_basic: names for official indices AND boards (C1b — the picker
        # needs official index names too, not only board names).
        basic_symbols = {r["symbol"]: r for r in basic}
        assert len(basic_symbols) == 5, (
            f"index_basic must hold 5 name records; got {list(basic_symbols)}"
        )
        for sym in ("SH000001", "SZ399001", "SZ399006", "BK0475", "BK0476"):
            rec = basic_symbols.get(sym)
            assert rec is not None, f"index_basic must carry a record for {sym}"
            assert rec["name"], f"index_basic record for {sym} must carry a name"
            assert rec["index_type"] == type_of[sym], (
                f"index_basic index_type for {sym} must match the daily tag"
            )

    async def test_basic_error_path_empty_clist_no_crash(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """C1 basic error: empty clist (no boards discovered) must not crash
        run() — official indices still land, boards simply contribute none."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr(
            "fetch_index_daily._today",
            lambda: __import__("datetime").date(2026, 8, 2),
        )
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())
        captured = _patch_write_csv(monkeypatch)

        stub = make_stub_session(
            canned_responses={
                KLINE_URL: {
                    "json_data": _kline_payload(
                        "000001", "上证指数", [_kline_row("2026-07-31")]
                    )
                },
                CLIST_URL: {"json_data": _clist_payload([])},
            }
        )
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        daily = [r for r in rows if "name" not in r]
        assert any(r["symbol"] == "SH000001" for r in daily), (
            "official indices must still be fetched when the board list is empty"
        )


class TestImportSchemaContract:
    """C1c — import_to_dolt lands the full plan DDL column set."""

    _DAILY_HEADER = [
        "symbol", "trade_date", "index_type",
        "open", "close", "high", "low", "volume", "amount", "update_date",
    ]

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.email", "req@compass.local"],
            capture_output=True, text=True,
        )
        subprocess.run(
            ["dolt", "config", "--global", "--add", "user.name", "ReqTest"],
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

    @staticmethod
    def _last(stdout: str) -> str:
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def _write_csv(self, path: Path, header: list[str], rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(header)
            writer.writerows(rows)

    def test_full_ddl_column_set_lands_with_numeric_values(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """The plan DDL contract: every column survives import_to_dolt with
        its value intact — not just symbol/trade_date/index_type (which the
        adversarial PK-dedup test checks) but open/close/high/low/volume/
        amount/update_date too."""
        from fetch_index_daily import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "index_daily.csv"
        self._write_csv(
            csv_path,
            self._DAILY_HEADER,
            [
                [
                    "BK0475", "2026-07-31", "concept",
                    "1199.0", "1210.0", "1215.0", "1195.0", "80000000", "35000000000",
                    "2026-08-02",
                ]
            ],
        )

        import_to_dolt(csv_path)

        row = self._last(
            dolt_sql_csv(
                "SELECT open, close, high, low, volume, amount, update_date "
                "FROM index_daily WHERE symbol='BK0475' AND trade_date='2026-07-31'"
            )
        )
        fields = row.split(",")
        assert len(fields) == 7, f"all 7 columns must be selected; got {row!r}"
        open_, close, high, low, volume, amount, update_date = fields
        # Dolt formats DOUBLEs in scientific notation (8e+07); compare
        # numerically, not lexically.
        assert float(open_) == 1199.0
        assert float(close) == 1210.0
        assert float(high) == 1215.0
        assert float(low) == 1195.0
        assert float(volume) == 80000000.0
        assert float(amount) == 35000000000.0
        assert update_date == "2026-08-02"
