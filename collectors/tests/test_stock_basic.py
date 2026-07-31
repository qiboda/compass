"""Unit tests for stock_basic utilities — symbol/ts_code/exchange logic."""

import asyncio
import csv
import sys
from unittest.mock import AsyncMock

import pytest

from fetch_stock_basic import (  # noqa: E402
    Throttle,
    fetch_page,
    infer_exchange,
    to_symbol,
    to_ts_code,
    ts_to_date,
)


class TestInferExchange:
    def test_shanghai_prefix_6(self):
        assert infer_exchange("600519") == "SH"
        assert infer_exchange("601318") == "SH"
        assert infer_exchange("688001") == "SH"

    def test_beijing_prefix_8(self):
        assert infer_exchange("830799") == "BJ"
        assert infer_exchange("836149") == "BJ"
        assert infer_exchange("873169") == "BJ"

    def test_shenzhen_default(self):
        assert infer_exchange("000001") == "SZ"
        assert infer_exchange("300750") == "SZ"
        assert infer_exchange("002415") == "SZ"


class TestToSymbol:
    def test_shanghai(self):
        assert to_symbol("600519") == "SH600519"

    def test_shenzhen(self):
        assert to_symbol("000001") == "SZ000001"

    def test_beijing(self):
        assert to_symbol("830799") == "BJ830799"


class TestToTsCode:
    def test_shanghai(self):
        assert to_ts_code("600519") == "600519.SH"

    def test_shenzhen(self):
        assert to_ts_code("000001") == "000001.SZ"

    def test_beijing(self):
        assert to_ts_code("830799") == "830799.BJ"


class TestTsToDate:
    def test_valid_timestamp(self):
        result = ts_to_date(997920000)
        assert result == "2001-08-15" or result == "2001-08-16"  # timezone-dependent

    def test_zero_timestamp(self):
        assert ts_to_date(0) == ""

    def test_none_timestamp(self):
        assert ts_to_date(None) == ""

    def test_negative_timestamp(self):
        assert ts_to_date(-1) == ""


class TestFetchPage:
    async def test_success_parses_rows(self, make_stub_session, monkeypatch):
        """Stub session returns items; expect symbol/ts_code added to each."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "data": {
                    "total": 2,
                    "diff": [
                        {"f12": "000001", "f14": "平安银行", "f100": "银行"},
                        {"f12": "600519", "f14": "贵州茅台", "f100": "白酒"},
                    ],
                }
            }
        )
        t = Throttle(min_interval=0)
        result = await fetch_page(stub, t, 1, 100)

        assert len(result) == 2
        assert result[0]["symbol"] == "SZ000001"
        assert result[0]["ts_code"] == "000001.SZ"
        assert result[0]["f12"] == "000001"
        assert result[1]["symbol"] == "SH600519"

    async def test_empty_diff_returns_empty(self, make_stub_session, monkeypatch):
        """diff=None returns empty list without crashing."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"data": {"total": 0, "diff": None}}
        )
        t = Throttle(min_interval=0)
        result = await fetch_page(stub, t, 1, 100)
        assert result == []

    async def test_no_code_skipped(self, make_stub_session, monkeypatch):
        """Items with empty f12 code are filtered out."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "data": {
                    "diff": [
                        {"f12": "", "f14": "no_code"},
                        {"f12": "000001", "f14": "has_code"},
                    ]
                }
            }
        )
        t = Throttle(min_interval=0)
        result = await fetch_page(stub, t, 1, 100)
        assert len(result) == 1
        assert result[0]["f12"] == "000001"

    async def test_retry_exhausted_raises(self, make_stub_session, monkeypatch):
        """All 4 attempts fail → last exception propagates."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        async def _raise_get(*args, **kwargs):
            raise Exception("connection error")

        session = make_stub_session()
        session.get = _raise_get  # type: ignore[method-assign]
        t = Throttle(min_interval=0)

        with pytest.raises(Exception, match="connection error"):
            await fetch_page(session, t, 1, 100)
        # 3 sleeps (attempts 0,1,2) before attempt 3 raises
        assert mock_sleep.call_count >= 3


class TestMain:
    async def test_basic_csv_output(self, make_stub_session, monkeypatch, tmp_path):
        """main() writes expected CSV with stock_basic data to tmp_path."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        async def _mock_fetch_page(session, throttle, page, page_size):
            if page == 1:
                return [
                    {
                        "symbol": "SZ000001",
                        "ts_code": "000001.SZ",
                        "f12": "000001",
                        "f14": "平安银行",
                    },
                ]
            return []

        import fetch_stock_basic as fsb

        monkeypatch.setattr(fsb, "fetch_page", _mock_fetch_page)

        stub = make_stub_session(json_data={"data": {"total": 1}})
        monkeypatch.setattr(
            fsb, "AsyncSession", lambda impersonate=None: stub
        )

        output = tmp_path / "test_out.csv"
        monkeypatch.setattr(
            sys,
            "argv",
            ["fetch_stock_basic.py", "-o", str(output), "--page-size", "100"],
        )

        await fsb.main()

        assert output.exists()
        with open(output, encoding="utf-8-sig") as f:
            reader = list(csv.DictReader(f))
        assert len(reader) == 1
        assert reader[0]["symbol"] == "SZ000001"

    async def test_resume_mode_appends(
        self, make_stub_session, monkeypatch, tmp_path
    ):
        """--resume reads existing CSV and appends new rows."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        output = tmp_path / "resume.csv"
        with open(output, "w", encoding="utf-8-sig") as f:
            f.write("symbol,ts_code,f12,f14\n")
            for i in range(10):
                f.write(f"SZ000001,000001.SZ,000001,existing{i}\n")

        page_results = iter(
            [
                [
                    {
                        "symbol": "SZ000002",
                        "ts_code": "000002.SZ",
                        "f12": "000002",
                        "f14": "new_row",
                    },
                ],
                [],
            ]
        )

        async def _mock_fetch_page(session, throttle, page, page_size):
            try:
                return next(page_results)
            except StopIteration:
                return []

        import fetch_stock_basic as fsb

        monkeypatch.setattr(fsb, "fetch_page", _mock_fetch_page)

        stub = make_stub_session(json_data={"data": {"total": 101}})
        monkeypatch.setattr(
            fsb, "AsyncSession", lambda impersonate=None: stub
        )

        monkeypatch.setattr(
            sys,
            "argv",
            [
                "fetch_stock_basic.py",
                "-o",
                str(output),
                "--resume",
                "--page-size",
                "100",
            ],
        )

        await fsb.main()

        with open(output, encoding="utf-8-sig") as f:
            lines = f.readlines()
        assert len(lines) == 12
