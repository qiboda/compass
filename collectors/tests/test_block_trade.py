"""Tests for fetch_block_trade.py — import_to_dolt, run()."""

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

# API field header for RPT_DATA_BLOCKTRADE (subset used by block_trade)
_HEADER = [
    "SECUCODE", "SECURITY_CODE", "TRADE_DATE",
    "DEAL_PRICE", "DEAL_VOLUME", "DEAL_AMT",
    "BUYER_NAME", "SELLER_NAME", "PREMIUM_RATIO",
]


def _make_row(secucode: str = "000001.SZ", price: str = "12.5") -> list[str]:
    row = [""] * len(_HEADER)
    row[_HEADER.index("SECUCODE")] = secucode
    row[_HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
    row[_HEADER.index("TRADE_DATE")] = "2024-12-31 00:00:00"
    row[_HEADER.index("DEAL_PRICE")] = price
    row[_HEADER.index("DEAL_VOLUME")] = "240000"
    row[_HEADER.index("DEAL_AMT")] = "3000000"
    row[_HEADER.index("BUYER_NAME")] = "华泰证券南京止马营营业部"
    row[_HEADER.index("SELLER_NAME")] = "华泰证券南京止马营营业部"
    row[_HEADER.index("PREMIUM_RATIO")] = "0.068376"
    return row


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
            "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY); "
            "INSERT INTO stock_basic VALUES ('SZ000001')"
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
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(csv_path, [_make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade")) == "1"

        # symbol gets exchange prefix (SZ000001), price/volume/buyer mapped
        row = self._last(
            dolt_sql_csv("SELECT symbol, trade_date, price, volume, buyer FROM block_trade")
        )
        assert row == "SZ000001,2024-12-31,12.5,240000,华泰证券南京止马营营业部"
        # DOUBLEs print in scientific/full precision in Dolt CSV output; check via CAST
        amount = self._last(
            dolt_sql_csv("SELECT CAST(amount AS DECIMAL(20,2)) FROM block_trade")
        )
        assert amount == "3000000.00"
        premium = self._last(
            dolt_sql_csv("SELECT CAST(premium_rate AS DECIMAL(10,6)) FROM block_trade")
        )
        assert premium == "0.068376"

        # data_updates 5-column upsert: table_name/last_updated/source/row_count/last_report_date
        up = self._last(dolt_sql_csv(
            "SELECT table_name, last_updated, source, row_count, last_report_date "
            "FROM data_updates WHERE table_name='block_trade'"
        ))
        assert up == (
            f"block_trade,{date.today().isoformat()},"
            "EastMoney datacenter RPT_DATA_BLOCKTRADE,1,2024-12-31"
        )

    def test_empty_deal_price_row_filtered_out(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """CSV rows with empty DEAL_PRICE (NULL in tmp table) are skipped by the
        WHERE guard — without it the NOT NULL price PK would fail the INSERT."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(csv_path, [_make_row(price=""), _make_row()])

        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade")) == "1"
        price = self._last(dolt_sql_csv("SELECT price FROM block_trade"))
        assert price == "12.5"

    def test_rerun_replaces_table_without_duplicates(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(csv_path, [_make_row()])

        assert import_to_dolt(csv_path) == 1
        rows = import_to_dolt(csv_path)
        assert rows == 1
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade")) == "1"

    def test_same_price_multi_vol_buyer_all_preserved(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Same (symbol, date, price) with different volume/buyer are distinct
        trades: the PK spans (symbol, date, price, volume, amount, buyer,
        seller) so none are dropped (F3 real-data regression: EastMoney ranks
        the same block trade multiple times)."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        rows = [
            _make_row(),  # 000001.SZ 12.5 / 240000 / 华泰
            _make_row(price="12.5"),  # same price, same default volume/buyer → exact dup
            ["000001.SZ", "000001", "2024-12-31 00:00:00", "12.5", "50000", "625000",
             "机构专用", "华泰证券南京止马营营业部", "0.068376"],
        ]
        self._write_csv(csv_path, rows)

        n = import_to_dolt(csv_path)
        # 2 distinct: the exact duplicate collapses, the 机构专用 row survives
        assert n == 2
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade")) == "2"
        assert "机构专用" in dolt_sql_csv("SELECT buyer FROM block_trade")

    def test_first_run_insert_failure_leaves_no_table(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """First-run INSERT failure drops the table cleanly."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"
        self._write_csv(csv_path, [_make_row()])
        dolt_sql_csv("DROP TABLE stock_basic")

        rows = import_to_dolt(csv_path)
        assert rows == 0
        cnt = self._last(dolt_sql_csv(
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_name='block_trade'"
        ))
        assert cnt == "0"

    def test_rerun_insert_failure_rolls_back(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """Rerun with failing INSERT restores previous data."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bt.csv"

        self._write_csv(csv_path, [_make_row()])
        assert import_to_dolt(csv_path) == 1

        dolt_sql_csv("DROP TABLE stock_basic")
        rows = import_to_dolt(csv_path)
        assert rows == 0
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM block_trade")) == "1"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When CSV does not exist, import_to_dolt returns 0."""
        from fetch_block_trade import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    async def test_run_writes_csv_with_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_block_trade import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{
                        "SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                        "TRADE_DATE": "2024-12-31 00:00:00",
                        "DEAL_PRICE": 12.5, "DEAL_VOLUME": 240000, "DEAL_AMT": 3000000,
                        "BUYER_NAME": "华泰证券南京止马营营业部",
                        "SELLER_NAME": "华泰证券南京止马营营业部",
                        "PREMIUM_RATIO": 0.068376,
                    }],
                    "pages": 1,
                },
            }
        )

        with patch("fetch_block_trade.AsyncSession", return_value=stub):
            result = await run(years=[2024])

        assert result.name == "RPT_DATA_BLOCKTRADE.csv"
        csv_path = tmp_path / "RPT_DATA_BLOCKTRADE.csv"
        assert csv_path.exists()

    async def test_run_incremental_since_short_circuits(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When last_report_date returns a future date, run() returns early."""
        from fetch_block_trade import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_block_trade.last_report_date", lambda _tbl: "2099-12-31")

        calls = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            calls[0] += 1
            return StubResponse(json_data={"success": True, "result": {"data": [], "pages": 1}})

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_block_trade.AsyncSession", return_value=stub):
            result = await run(years=[2024])

        assert result.name == "RPT_DATA_BLOCKTRADE.csv"
        assert calls[0] == 0

    async def test_run_incremental_since_excludes_watermark_day(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Exclusive boundary: dates <= last_report_date are not re-fetched."""
        from fetch_block_trade import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)
        monkeypatch.setattr("fetch_block_trade.last_report_date", lambda _tbl: "2024-12-31")

        calls = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            calls[0] += 1
            return StubResponse(json_data={"success": True, "result": {"data": [], "pages": 1}})

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_block_trade.AsyncSession", return_value=stub):
            result = await run(years=[2024])

        assert result.name == "RPT_DATA_BLOCKTRADE.csv"
        assert calls[0] == 0

    async def test_run_fetch_exception_aborts_without_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """When fetch_paginated raises, run() aborts: raises and writes no CSV."""
        from fetch_block_trade import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            if call_count[0] <= 4:
                raise RuntimeError("simulated fetch error")
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [], "pages": 1},
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_block_trade.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run(years=[2024], page_size=100)

        assert not (tmp_path / "RPT_DATA_BLOCKTRADE.csv").exists()

    async def test_run_fetch_exception_deletes_stale_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """A failed run removes any stale CSV so import cannot publish old data."""
        from fetch_block_trade import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stale = tmp_path / "RPT_DATA_BLOCKTRADE.csv"
        stale.write_text("stale\n", encoding="utf-8")

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            raise RuntimeError("simulated fetch error")

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_block_trade.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run(years=[2024], page_size=100)

        assert not stale.exists()
