"""Tests for fetch_main_flow.py — import_to_dolt, run() (push2 clist snapshot)."""

import asyncio
import csv
import subprocess
import sys
from collections.abc import Callable
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# CSV header: symbol is prefixed at fetch time (no SECUCODE/SECURITY_CODE columns)
_HEADER = [
    "symbol",
    "trade_date",
    "main_net_inflow",
    "main_net_inflow_rate",
    "super_large_net",
    "large_net",
    "medium_net",
    "small_net",
    "update_date",
]

# Unix epoch seconds (Beijing time) — 2026-07-31 15:34 CST, the last trading day
_F124 = 1785483240
_TRADE_DATE = "2026-07-31"


def _make_row(symbol: str = "SH600519", trade_date: str = _TRADE_DATE) -> list[str]:
    row = [""] * len(_HEADER)
    row[_HEADER.index("symbol")] = symbol
    row[_HEADER.index("trade_date")] = trade_date
    row[_HEADER.index("main_net_inflow")] = "123.45"
    row[_HEADER.index("main_net_inflow_rate")] = "1.23"
    row[_HEADER.index("super_large_net")] = "50.0"
    row[_HEADER.index("large_net")] = "73.45"
    row[_HEADER.index("medium_net")] = "-10.0"
    row[_HEADER.index("small_net")] = "-113.45"
    row[_HEADER.index("update_date")] = trade_date
    return row


def _stub_diff(code: str = "600519", f124: int = _F124) -> dict[str, object]:
    """One push2 clist diff item (empirically: f62 = f66 + f72, f72 = large, f78 = medium)."""
    return {
        "f12": code,
        "f14": "stub",
        "f2": 171.48,
        "f3": 5.98,
        "f62": 123.45,
        "f184": 1.23,
        "f66": 50.0,
        "f69": 0.5,
        "f72": 73.45,
        "f75": 0.73,
        "f78": -10.0,
        "f81": -0.1,
        "f84": -113.45,
        "f87": -1.13,
        "f124": f124,
    }


def _push2_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


# ── helper unit tests ──


class TestHelpers:
    def test_exchange_prefix_rules(self) -> None:
        from fetch_main_flow import _exchange_prefix  # noqa: E402

        assert _exchange_prefix("600519") == "SH"
        assert _exchange_prefix("688766") == "SH"
        assert _exchange_prefix("000001") == "SZ"
        assert _exchange_prefix("300058") == "SZ"
        assert _exchange_prefix("830799") == "BJ"

    def test_trade_date_from_quotes_uses_latest_f124(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """max(f124) → Beijing-time date (2026-07-31), not UTC-shifted."""
        from fetch_main_flow import _trade_date_from_quotes  # noqa: E402

        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 8, 2))
        diff = [
            {"f124": 1785483240},
            {"f124": 1785485497},
            {"f124": "-"},
        ]
        assert _trade_date_from_quotes(diff) == date(2026, 7, 31)

    def test_trade_date_from_quotes_falls_back_to_today(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """No usable f124 → today (documented limitation)."""
        from fetch_main_flow import _trade_date_from_quotes  # noqa: E402

        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 8, 2))
        assert _trade_date_from_quotes([]) == date(2026, 8, 2)
        assert _trade_date_from_quotes([{"f124": "-"}]) == date(2026, 8, 2)


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

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerows(rows)

    def test_first_run_creates_table_and_imports(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_main_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "mf.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM capital_main_flow")) == "1"

        row = self._last(
            dolt_sql_csv(
                "SELECT row_count, last_report_date, source FROM data_updates "
                "WHERE table_name='capital_main_flow'"
            )
        )
        assert row == f"1,{_TRADE_DATE},EastMoney push2 clist f62"

    def test_symbol_passes_through_and_stock_basic_filters(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """CSV symbol SH600519 → Dolt SH600519; unknown symbol SZ000001 filtered out."""
        from fetch_main_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "mf.csv"
        self._write_csv(csv_path, [_make_row(), _make_row(symbol="SZ000001")])

        assert import_to_dolt(csv_path) == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM capital_main_flow")) == "1"
        sym = self._last(dolt_sql_csv("SELECT symbol FROM capital_main_flow"))
        assert sym == "SH600519"

    def test_rerun_replaces_table_without_duplicates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Idempotency: rerunning the same CSV must not grow row count."""
        from fetch_main_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "mf.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM capital_main_flow")) == "1"

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure leaves the table present but empty
        (merge semantics: CREATE TABLE IF NOT EXISTS ran, a retry can succeed)."""
        from fetch_main_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "mf.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        cnt = self._last(
            dolt_sql_csv("SELECT COUNT(*) FROM capital_main_flow")

        )
        assert cnt == "0"

    def test_rerun_insert_failure_rolls_back(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores previous data."""
        from fetch_main_flow import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "mf.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM capital_main_flow")) == "1"

    def test_ddl_failure_rolls_back(
        self,
        dolt_env: tuple[Path, Callable[[str], str]],
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Broken DDL on rerun: _tmp_mf_old restored, no temp residue."""
        import fetch_main_flow  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "mf.csv"

        self._write_csv(csv_path, [_make_row()])
        assert fetch_main_flow.import_to_dolt(csv_path) == 1

        monkeypatch.setattr(fetch_main_flow, "DDL", "CREATE TABLE capital_main_flow (broken")
        rows = fetch_main_flow.import_to_dolt(csv_path)
        assert rows == 0
        # previous data restored
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM capital_main_flow")) == "1"
        # no temp residue
        for tbl in ("_tmp_mf", "_tmp_mf_old"):
            cnt = self._last(
                dolt_sql_csv(
                    f"SELECT COUNT(*) FROM information_schema.tables WHERE table_name='{tbl}'"
                )
            )
            assert cnt == "0"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_main_flow import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    @staticmethod
    def _stub(make_stub_session, *, payload=None, exc=None, counter=None):
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):  # noqa: ANN001, ANN002, ANN003
            if counter is not None:
                counter[0] += 1
            if exc is not None:
                raise exc
            return StubResponse(json_data=payload)

        stub.get = _get  # type: ignore[method-assign]
        return stub

    async def test_run_writes_csv_with_symbols_values_and_trade_date(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_main_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 8, 2))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        diff = [_stub_diff("600519"), _stub_diff("000001"), _stub_diff("300058")]
        stub = self._stub(make_stub_session, payload=_push2_payload(diff))

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_MAIN_MONEY_FLOW.csv"
        csv_path = tmp_path / "RPT_MAIN_MONEY_FLOW.csv"
        assert csv_path.exists()

        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert [r["symbol"] for r in rows] == ["SH600519", "SZ000001", "SZ300058"]
        for r in rows:
            assert r["trade_date"] == "2026-07-31"
            assert r["update_date"] == "2026-08-02"
        first = rows[0]
        assert first["main_net_inflow"] == "123.45"
        assert first["main_net_inflow_rate"] == "1.23"
        assert first["super_large_net"] == "50.0"
        assert first["large_net"] == "73.45"  # f72 (empirically the large-order flow)
        assert first["medium_net"] == "-10.0"  # f78
        assert first["small_net"] == "-113.45"

    async def test_run_short_circuits_when_today_imported(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """data_updates.last_report_date == today → skip fetch entirely."""
        from fetch_main_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 7, 31))
        monkeypatch.setattr("fetch_main_flow.last_report_date", lambda _tbl: "2026-07-31")
        counter = [0]
        stub = self._stub(
            make_stub_session, payload=_push2_payload([_stub_diff()]), counter=counter
        )

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_MAIN_MONEY_FLOW.csv"
        assert not (tmp_path / "RPT_MAIN_MONEY_FLOW.csv").exists()
        assert counter[0] == 0

    async def test_run_skips_import_when_trade_date_already_imported(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Weekend re-run: snapshot trade_date (Friday) already imported → fetch but no CSV."""
        from fetch_main_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 8, 2))
        monkeypatch.setattr("fetch_main_flow.last_report_date", lambda _tbl: "2026-07-31")
        monkeypatch.setattr(
            "fetch_main_flow._trade_date_from_quotes", lambda _diff: date(2026, 7, 31)
        )
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        counter = [0]
        stub = self._stub(
            make_stub_session, payload=_push2_payload([_stub_diff()]), counter=counter
        )

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_MAIN_MONEY_FLOW.csv"
        assert counter[0] >= 1  # snapshot was fetched
        assert not (tmp_path / "RPT_MAIN_MONEY_FLOW.csv").exists()

    async def test_run_fetch_exception_aborts_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """All retries exhausted on every domain → run() raises, no CSV written."""
        from fetch_main_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = self._stub(make_stub_session, exc=RuntimeError("simulated fetch error"))

        with patch("fetch_main_flow.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        assert not (tmp_path / "RPT_MAIN_MONEY_FLOW.csv").exists()

    async def test_run_fetch_exception_deletes_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """A failed run removes any stale CSV so import cannot publish old data."""
        from fetch_main_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stale = tmp_path / "RPT_MAIN_MONEY_FLOW.csv"
        stale.write_text("stale\n", encoding="utf-8")

        stub = self._stub(make_stub_session, exc=RuntimeError("simulated fetch error"))

        with patch("fetch_main_flow.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        assert not stale.exists()

    async def test_run_domain_fallback_on_empty_first_response(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """push2delay empty (rate-limited) → falls back to push2 main domain."""
        import fetch_main_flow  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 8, 2))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            canned_responses={
                fetch_main_flow.PUSH2_DELAY: {
                    "json_data": {"rc": 0, "data": {"total": 0, "diff": []}}
                },
                fetch_main_flow.PUSH2_MAIN: {"json_data": _push2_payload([_stub_diff()])},
            }
        )

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            result = await fetch_main_flow.run()

        assert result.name == "RPT_MAIN_MONEY_FLOW.csv"
        csv_path = tmp_path / "RPT_MAIN_MONEY_FLOW.csv"
        assert csv_path.exists()
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert [r["symbol"] for r in rows] == ["SH600519"]

    async def test_run_dash_values_become_empty_cells(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """push2 '-' cells (suspended/halted) normalize to empty CSV cells → NULL."""
        from fetch_main_flow import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        monkeypatch.setattr("fetch_main_flow._today", lambda: date(2026, 8, 2))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        diff = [
            {
                "f12": "600519",
                "f62": "-",
                "f184": "-",
                "f66": "-",
                "f72": "-",
                "f78": "-",
                "f84": "-",
                "f124": _F124,
            }
        ]
        stub = self._stub(make_stub_session, payload=_push2_payload(diff))

        with patch("fetch_main_flow.AsyncSession", return_value=stub):
            await run()

        with open(tmp_path / "RPT_MAIN_MONEY_FLOW.csv", newline="", encoding="utf-8-sig") as f:
            row = next(csv.DictReader(f))
        assert row["symbol"] == "SH600519"
        assert row["main_net_inflow"] == ""
        assert row["main_net_inflow_rate"] == ""
        assert row["small_net"] == ""
