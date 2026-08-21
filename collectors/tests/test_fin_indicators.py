"""Tests for fetch_fin_indicators.py — flatten_record, write_csv, fetch_period,
_last_report_date, main().
"""

import asyncio
import csv
import json
import sys
from datetime import datetime
from pathlib import Path
from unittest.mock import AsyncMock, Mock, patch

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

    async def test_success_false_breaks(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(json_data={"success": False, "message": "boom"})
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert records == []

    async def test_result_none_breaks(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(json_data={"success": True, "result": None})
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert records == []

    async def test_empty_items_breaks(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_period(
            stub, t, "RPT", "2024-01-01"
        )
        assert records == []


class TestThrottle:
    async def test_acquire_waits_when_below_min_interval(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Second acquire within min_interval takes the asyncio.sleep wait branch."""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        t = fetch_fin_indicators.Throttle(min_interval=10)
        await t.acquire()
        await t.acquire()
        assert mock_sleep.call_count >= 2


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

    @staticmethod
    def _init_dolt(tmp_path: Path) -> None:
        """Init a temp Dolt repo at tmp_path (mirrors test_import_to_dolt.py)."""
        import subprocess

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

    def test_dolt_subprocess_returns_max_report_date(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Real temp Dolt: SELECT MAX(report_date) returned via subprocess."""
        import subprocess

        self._init_dolt(tmp_path)
        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(tmp_path),
                "sql",
                "-r",
                "csv",
                "-q",
                "CREATE TABLE fin_indicators (symbol VARCHAR(20) PRIMARY KEY, "
                "report_date DATE NOT NULL); "
                "INSERT INTO fin_indicators VALUES ('SZ000001', '2024-12-31')",
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, result.stderr

        real_truediv = Path.__truediv__

        def _redirect(self, other):  # noqa: ANN001, ANN002
            return tmp_path if other == "compass_data" else real_truediv(self, other)

        monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)

        state_path = tmp_path / "nonexistent.state.json"
        result = fetch_fin_indicators._last_report_date("RPT_LICO_FN_CPD", state_path)

        assert result == "2024-12-31"

    def test_dolt_subprocess_null_falls_back_to_state(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Empty table → MAX is NULL → falls back to state.json."""
        import subprocess

        self._init_dolt(tmp_path)
        result = subprocess.run(
            [
                "dolt",
                "--data-dir",
                str(tmp_path),
                "sql",
                "-r",
                "csv",
                "-q",
                "CREATE TABLE fin_indicators (symbol VARCHAR(20) PRIMARY KEY, "
                "report_date DATE NOT NULL)",
            ],
            capture_output=True,
            text=True,
        )
        assert result.returncode == 0, result.stderr

        real_truediv = Path.__truediv__

        def _redirect(self, other):  # noqa: ANN001, ANN002
            return tmp_path if other == "compass_data" else real_truediv(self, other)

        monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)

        state_path = tmp_path / "RPT.state.json"
        state_path.write_text(json.dumps({"last_report_date": "2024-12-31"}))

        result = fetch_fin_indicators._last_report_date("RPT_LICO_FN_CPD", state_path)

        assert result == "2024-12-31"

    def test_dolt_subprocess_missing_table_returns_empty(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Query error (missing table) with no state file → returns ''."""
        self._init_dolt(tmp_path)

        real_truediv = Path.__truediv__

        def _redirect(self, other):  # noqa: ANN001, ANN002
            return tmp_path if other == "compass_data" else real_truediv(self, other)

        monkeypatch.setattr(fetch_fin_indicators.Path, "__truediv__", _redirect)

        state_path = tmp_path / "nonexistent.state.json"
        result = fetch_fin_indicators._last_report_date("RPT_LICO_FN_CPD", state_path)

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

    async def test_main_writes_state_json_under_csv_dir_not_cwd(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """State file must live beside the CSV in csv_dir(), not in the CWD.

        RED: state_path is currently ``Path(f"{report_name}.state.json")``,
        so running from a different directory silently loses the anchor.
        """
        cwd = tmp_path / "cwd"
        cwd.mkdir()
        monkeypatch.chdir(cwd)
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

        # COMPASS_CSV_DIR points at tmp_path via the autouse fixture; the
        # state file must be written there, not into the process CWD.
        assert (tmp_path / "RPT_CUSTOM.state.json").exists()
        assert not (cwd / "RPT_CUSTOM.state.json").exists()

    async def test_default_years_covers_2020_to_now(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        class FakeDatetime(datetime):
            @classmethod
            def now(cls, tz=None):
                return cls(2020, 6, 1, 12, 0, 0)

        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(fetch_fin_indicators, "datetime", FakeDatetime)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {
                    "data": [{"code": "000001", "REPORTDATE": "2020-12-31"}],
                    "pages": 1,
                },
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert state["last_report_date"] == "2020-12-31"

    async def test_incremental_filters_dates(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_update_anchor", lambda *a, **k: "2023-01-01"
        )
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
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists()

    async def test_incremental_zero_rows_writes_nothing(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """0-row incremental run (nothing newer than the anchor) writes no CSV
        and leaves state.json untouched — the anchor never advances."""
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_update_anchor", lambda *a, **k: "2025-01-01"
        )
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [], "pages": 1},
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()

    async def test_incremental_no_prior_data_full_fetch(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_update_anchor", lambda *a, **k: ""
        )
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
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists()

    async def test_period_fetch_failure_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path, capsys
    ) -> None:
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(exc=RuntimeError("boom"))
        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()
        assert "FAILED: boom" in capsys.readouterr().err

    async def test_incremental_fetch_failure_propagates(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """增量路径抓取异常必须向上传播（与 F10 三表一致），不能伪装成空窗口。

        RED: 当前 anchor 分支吞掉异常并继续，main() 正常返回。
        """
        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(
            fetch_fin_indicators, "_update_anchor", lambda *a, **k: "2025-01-01"
        )
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(exc=RuntimeError("boom"))
        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2024", "--periods", "FY"],
            ),
            pytest.raises(RuntimeError, match="boom"),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()

    async def test_empty_records_prints_empty(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--years", "2024", "--periods", "FY"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()


# ═══════════════════════════════════════════════════════════════════
# #135 collectors-revision-detect (RED wave) — 增量修订检测验收测试
# 契约（GREEN 实现必须提供，否则本批测试不转绿）：
#   - fetch_fin_indicators._update_anchor(report_name, state_path) -> str
#     = min(data_updates.last_updated, state.json.last_update_date)，双源
#     缺失（含 NULL/键缺失）视为该源缺失，两源皆无 → ""
#   - fetch_fin_indicators.dedupe_csv(path)：CSV 整文件 keep-LAST 去重，
#     键 (SECURITY_CODE, REPORTDATE)，utf-8-sig BOM 安全，空/缺列不崩溃
# ═══════════════════════════════════════════════════════════════════

_CSV_HEADER = [
    "SECUCODE", "SECURITY_CODE", "REPORTDATE", "UPDATE_DATE", "NOTICE_DATE",
    "DATATYPE", "QDATE", "EITIME", "DATAYEAR", "DATEMMDD",
    "SECURITY_NAME_ABBR", "TRADE_MARKET", "TRADE_MARKET_CODE", "TRADE_MARKET_ZJG",
    "SECURITY_TYPE", "SECURITY_TYPE_CODE", "PUBLISHNAME", "BOARD_CODE",
    "BOARD_NAME", "ORI_BOARD_CODE", "ORG_CODE", "ISNEW", "BASIC_EPS",
    "DEDUCT_BASIC_EPS", "TOTAL_OPERATE_INCOME", "PARENT_NETPROFIT",
    "WEIGHTAVG_ROE", "BPS", "MGJYXJJE", "XSMLL", "YSTZ", "SJLTZ", "YSHZ",
    "SJLHZ", "ZXGXL", "ASSIGNDSCRPT", "PAYYEAR",
]


def _full_row(
    secucode: str = "000858.SZ",
    report_date: str = "2025-03-31",
    update_date: str = "2026-04-30",
    revenue: str = "170.86",
    name: str = "五粮液",
    data_type: str = "2025年 一季报",
) -> dict[str, str]:
    """Build a full 37-col API row for 五粮液 2025Q1 (修订后: 170.86 / 2026-04-30).

    Key order follows _CSV_HEADER so write_csv infers the same header the
    production INSERT SELECT expects.
    """
    row: dict[str, str] = dict.fromkeys(_CSV_HEADER, "")
    row["SECUCODE"] = secucode
    row["SECURITY_CODE"] = secucode.split(".")[0]
    row["REPORTDATE"] = report_date
    row["UPDATE_DATE"] = update_date
    row["NOTICE_DATE"] = "2025-04-26"
    row["DATATYPE"] = data_type
    row["QDATE"] = "2025Q1"
    row["EITIME"] = "2025-04-26 00:00:00"
    row["DATAYEAR"] = "2025"
    row["DATEMMDD"] = "一季报"
    row["SECURITY_NAME_ABBR"] = name
    row["TRADE_MARKET"] = "深圳"
    row["TRADE_MARKET_CODE"] = "XSHE"
    row["TRADE_MARKET_ZJG"] = "SZSE"
    row["SECURITY_TYPE"] = "股票"
    row["SECURITY_TYPE_CODE"] = "1"
    row["PUBLISHNAME"] = "宜宾五粮液股份有限公司"
    row["BOARD_CODE"] = "ZSG"
    row["BOARD_NAME"] = "主板"
    row["ORI_BOARD_CODE"] = "012001"
    row["ORG_CODE"] = "9900005916"
    row["ISNEW"] = "0"
    row["BASIC_EPS"] = "0.85"
    row["DEDUCT_BASIC_EPS"] = "0.85"
    row["TOTAL_OPERATE_INCOME"] = revenue
    row["PARENT_NETPROFIT"] = "60.10"
    row["WEIGHTAVG_ROE"] = "4.08"
    row["BPS"] = "33.44"
    row["MGJYXJJE"] = "1.29"
    row["XSMLL"] = "75.87"
    row["YSTZ"] = "-23.10"
    row["SJLTZ"] = "16.76"
    row["YSHZ"] = "-2.35"
    row["SJLHZ"] = "-1.16"
    row["ZXGXL"] = "4.08"
    row["ASSIGNDSCRPT"] = "10派25.76元"
    row["PAYYEAR"] = "2025"
    return row


def _init_dolt(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    """Init a temp Dolt repo at tmp_path and point COMPASS_DATA_DIR at it.

    Returns a dolt_sql_csv callable. Mirrors the dolt_env pattern used by
    test_main.py::TestImportFinIndicatorsMerge / test_common.py.
    """
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
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))

    def dolt_sql_csv(sql: str) -> str:
        return subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
            capture_output=True, text=True,
        ).stdout

    return dolt_sql_csv


class TestRevisionDetect:
    """#135 T1: 同一 report_date 的 UPDATE_DATE 变化（修订）必须被抓取并覆盖。

    RED against current code:
    - fetch_fin_indicators.py:299-308 增量按 REPORTDATE 枚举 —— 锚点后无新报告期
      时直接 return，修订行永远不会被抓取（filter 断言失败、CSV 不生成）；
    - main.py:147 INSERT IGNORE 丢弃同 PK 修订（Dolt 覆盖断言失败）。
    GREEN 后增量 filter 为 UPDATE_DATE 锚点、import 为 UPSERT 覆盖。
    """

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ):
        """temp Dolt：stock_basic + data_updates + fin_indicators（真实 DDL）。

        锚点源预置：data_updates.last_updated=2026-04-30。state.json 双键
        （last_report_date / last_update_date）由各测试写入 —— 当前实现的
        _last_report_date 读 last_report_date 分支（→ 无新报告期 return，RED），
        GREEN 实现的 _update_anchor 读 min(data_updates, state)。
        """
        import main as main_mod

        dolt_sql_csv = _init_dolt(tmp_path, monkeypatch)
        dolt_sql_csv(
            "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY); "
            "INSERT INTO stock_basic VALUES ('SZ000858')"
        )
        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )
        dolt_sql_csv(
            "INSERT INTO data_updates (table_name, last_updated) "
            "VALUES ('fin_indicators', '2026-04-30')"
        )
        dolt_sql_csv(main_mod.FIN_INDICATORS_DDL)
        return dolt_sql_csv

    @staticmethod
    def _write_state(tmp_path: Path) -> None:
        state = {
            "last_report_date": "2026-04-30",  # 当前实现 _last_report_date 读此键
            "last_update_date": "2026-04-30",  # GREEN 实现 _update_anchor 读此键
        }
        (tmp_path / "RPT_LICO_FN_CPD.state.json").write_text(json.dumps(state))

    async def test_incremental_filter_uses_update_date_anchor(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env,
    ) -> None:
        """增量模式 API filter 必须是 (UPDATE_DATE>='<锚点>') 而非 REPORTDATE 枚举。"""
        import asyncio

        _ = dolt_env  # 锚点源（data_updates.last_updated=2026-04-30）
        self._write_state(tmp_path)
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [_full_row()], "pages": 1},
            }
        )
        seen: list[str] = []
        orig_get = stub.get

        async def _get(url, params=None, headers=None):  # noqa: ANN001, ANN003
            seen.append(params.get("filter", "") if params else "")
            return await orig_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2025", "--periods", "Q1"],
            ),
        ):
            await fetch_fin_indicators.main()

        # RED: 当前实现按 REPORTDATE 过滤，锚点 2026-04-30 下 all_dates 为空直接 return，
        # 从不发起请求 —— seen 为空即失败。
        assert seen, "incremental mode must issue an API request for revised rows"
        assert all(
            "(UPDATE_DATE>='2026-04-30')" in f for f in seen
        ), f"expected UPDATE_DATE anchor filter, got {seen}"

    async def test_revision_captured_csv_pk_unique_new_value(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env,
    ) -> None:
        """修订行被抓取，CSV 中该 PK 唯一且为新值（170.86 / 2026-04-30）。"""
        import asyncio

        _ = dolt_env
        self._write_state(tmp_path)
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [_full_row()], "pages": 1},
            }
        )

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2025", "--periods", "Q1"],
            ),
        ):
            await fetch_fin_indicators.main()

        # RED: 当前实现无新报告期直接 return，CSV 不生成。
        csv_path = fetch_fin_indicators.csv_dir() / "RPT_LICO_FN_CPD.csv"
        assert csv_path.exists(), "revision row must be fetched and CSV written"
        with open(csv_path, encoding="utf-8-sig") as f:
            rows = [
                r for r in csv.DictReader(f)
                if r["SECURITY_CODE"] == "000858" and r["REPORTDATE"] == "2025-03-31"
            ]
        assert len(rows) == 1, (
            f"PK (SECURITY_CODE, REPORTDATE) must be unique in CSV, got {len(rows)} rows"
        )
        assert rows[0]["TOTAL_OPERATE_INCOME"] == "170.86"
        assert rows[0]["UPDATE_DATE"] == "2026-04-30"

    async def test_dolt_import_overwrites_revised_value(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env,
    ) -> None:
        """端到端：预置旧值（369.40/2025-04-26）→ 增量抓取修订 → import 后值已覆盖。

        RED: INSERT IGNORE（main.py:147）丢弃同 PK 修订，Dolt 仍为旧值 369.40。
        """
        import asyncio
        import io

        import main as main_mod

        dolt_sql_csv = dolt_env
        # 预置旧值：上次抓取时五粮液 2025Q1 的旧数据
        dolt_sql_csv(
            "INSERT INTO fin_indicators (symbol, report_date, update_date, revenue, name, data_type) "
            "VALUES ('SZ000858', '2025-03-31', '2025-04-26', 369.40, '五粮液', '2025年 一季报')"
        )
        self._write_state(tmp_path)
        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [_full_row()], "pages": 1},
            }
        )
        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--incremental", "--years", "2025", "--periods", "Q1"],
            ),
        ):
            await fetch_fin_indicators.main()

        main_mod._import_fin_indicators()

        out = dolt_sql_csv(
            "SELECT revenue, update_date, name FROM fin_indicators "
            "WHERE symbol='SZ000858' AND report_date='2025-03-31'"
        )
        rows = list(csv.DictReader(io.StringIO(out)))
        assert len(rows) == 1
        # RED: INSERT IGNORE 丢弃修订 → 仍为 369.40；GREEN UPSERT → 170.86
        assert rows[0]["revenue"] == "170.86", (
            f"revised value must overwrite old one in Dolt, got revenue={rows[0]['revenue']!r}"
        )
        assert rows[0]["update_date"] == "2026-04-30"
        assert rows[0]["name"] == "五粮液"


class TestUpdateAnchor:
    """#135 T2: 增量锚点 = min(data_updates.last_updated, state.json.last_update_date)。

    两源取较早者（防跨日 fetch/import 或单独 import 导致锚点超前漏抓修订）；
    单源缺失（无行 / last_updated NULL / last_update_date 键缺失）时取另一源；
    两源皆无 → ""（触发全量 REPORTDATE 枚举）。
    RED: fetch_fin_indicators._update_anchor 尚不存在（AttributeError）。
    """

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ):
        dolt_sql_csv = _init_dolt(tmp_path, monkeypatch)
        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )
        return dolt_sql_csv

    def _call(self, state_path: Path) -> str:
        return fetch_fin_indicators._update_anchor("RPT_LICO_FN_CPD", state_path)

    def test_data_updates_row_wins_when_state_absent(
        self, dolt_env, tmp_path: Path
    ) -> None:
        """① data_updates 有 fin_indicators 行 last_updated=2026-08-03 → 锚点该值。"""
        dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "INSERT INTO data_updates (table_name, last_updated) "
            "VALUES ('fin_indicators', '2026-08-03')"
        )
        assert self._call(tmp_path / "no.state.json") == "2026-08-03"

    def test_state_wins_when_data_updates_missing(
        self, dolt_env, tmp_path: Path
    ) -> None:
        """② data_updates 无行 + state.json last_update_date=2026-07-01 → 该值。"""
        _ = dolt_env  # 空 data_updates 表
        state = tmp_path / "RPT_LICO_FN_CPD.state.json"
        state.write_text(json.dumps({"last_update_date": "2026-07-01"}))
        assert self._call(state) == "2026-07-01"

    def test_min_of_both_sources_takes_state(
        self, dolt_env, tmp_path: Path
    ) -> None:
        """③ 两源都有且 state 较早 → min 即 state 值（防锚点超前）。"""
        dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "INSERT INTO data_updates (table_name, last_updated) "
            "VALUES ('fin_indicators', '2026-08-03')"
        )
        state = tmp_path / "RPT_LICO_FN_CPD.state.json"
        state.write_text(json.dumps({"last_update_date": "2026-07-01"}))
        assert self._call(state) == "2026-07-01"  # min(08-03, 07-01)

    def test_both_absent_returns_empty(self, dolt_env, tmp_path: Path) -> None:
        """④ 两源皆无 → ""（全量 fallback）。"""
        _ = dolt_env  # 空 data_updates，无 state 文件
        assert self._call(tmp_path / "no.state.json") == ""

    def test_no_dolt_dir_state_single_source(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """无 .dolt 目录 → data_updates 源缺失，state 单源生效。"""
        state = tmp_path / "RPT.state.json"
        state.write_text(json.dumps({"last_update_date": "2026-07-01"}))
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))  # 无 .dolt
        assert fetch_fin_indicators._update_anchor("RPT", state) == "2026-07-01"

    def test_null_last_updated_treated_as_missing_source(
        self, dolt_env, tmp_path: Path
    ) -> None:
        """⑤ data_updates 行 last_updated NULL → 视为源缺失，state 单源生效。"""
        dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "INSERT INTO data_updates (table_name, last_updated) "
            "VALUES ('fin_indicators', NULL)"
        )
        state = tmp_path / "RPT_LICO_FN_CPD.state.json"
        state.write_text(json.dumps({"last_update_date": "2026-07-01"}))
        assert self._call(state) == "2026-07-01"

    def test_null_last_updated_no_state_returns_empty(
        self, dolt_env, tmp_path: Path
    ) -> None:
        """⑤ NULL + 无 state → 两源皆缺失 → ""。"""
        dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "INSERT INTO data_updates (table_name, last_updated) "
            "VALUES ('fin_indicators', NULL)"
        )
        assert self._call(tmp_path / "no.state.json") == ""

    def test_state_missing_last_update_date_key_ignored(
        self, dolt_env, tmp_path: Path
    ) -> None:
        """旧格式 state.json 只有 last_report_date → last_update_date 键缺失视为源缺失。"""
        dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "INSERT INTO data_updates (table_name, last_updated) "
            "VALUES ('fin_indicators', '2026-08-03')"
        )
        state = tmp_path / "RPT_LICO_FN_CPD.state.json"
        state.write_text(json.dumps({"last_report_date": "2025-12-31"}))
        assert self._call(state) == "2026-08-03"


class TestCsvDedup:
    """#135 T4: CSV 整文件 keep-LAST 去重，键 (SECURITY_CODE, REPORTDATE)。

    RED: fetch_fin_indicators.dedupe_csv 尚不存在（AttributeError）。
    GREEN 契约：utf-8-sig BOM 安全；增量（append）与全量两写入路径都去重；
    空文件 / 缺 PK 列不崩溃。
    """

    def _write(self, path: Path, rows: list[dict[str, str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_CSV_HEADER)
            for r in rows:
                writer.writerow([r[c] for c in _CSV_HEADER])

    def _read(self, path: Path) -> list[dict[str, str]]:
        with open(path, encoding="utf-8-sig") as f:
            return list(csv.DictReader(f))

    def test_keep_last_wins_same_pk(self, tmp_path: Path) -> None:
        """① 同 PK 旧值在前、新值在后 → 保留新值。"""
        p = tmp_path / "dup.csv"
        old = _full_row(update_date="2025-04-26", revenue="369.40")
        new = _full_row()  # 170.86 / 2026-04-30
        self._write(p, [old, new])
        fetch_fin_indicators.dedupe_csv(p)
        rows = self._read(p)
        assert len(rows) == 1
        assert rows[0]["TOTAL_OPERATE_INCOME"] == "170.86"
        assert rows[0]["UPDATE_DATE"] == "2026-04-30"

    def test_bom_does_not_pollute_first_line(self, tmp_path: Path) -> None:
        """② utf-8-sig BOM 首行不污染（列名保持干净）。"""
        p = tmp_path / "bom.csv"
        self._write(p, [_full_row()])
        fetch_fin_indicators.dedupe_csv(p)
        rows = self._read(p)
        assert len(rows) == 1
        assert rows[0]["SECURITY_CODE"] == "000858"
        assert rows[0]["REPORTDATE"] == "2025-03-31"

    def test_incremental_append_path_deduped(self, tmp_path: Path) -> None:
        """③ 增量路径（write_csv 全量旧值 + append 新值）去重后保留新值。"""
        p = tmp_path / "inc.csv"
        fetch_fin_indicators.write_csv(
            [_full_row(update_date="2025-04-26", revenue="369.40")], p
        )
        fetch_fin_indicators.write_csv([_full_row()], p, append=True)
        fetch_fin_indicators.dedupe_csv(p)
        rows = self._read(p)
        assert len(rows) == 1
        assert rows[0]["TOTAL_OPERATE_INCOME"] == "170.86"

    def test_full_write_path_deduped(self, tmp_path: Path) -> None:
        """③ 全量路径（单次 write_csv 含重复 PK）去重。"""
        p = tmp_path / "full.csv"
        fetch_fin_indicators.write_csv(
            [
                _full_row(update_date="2025-04-26", revenue="369.40"),
                _full_row(),
            ],
            p,
        )
        fetch_fin_indicators.dedupe_csv(p)
        assert len(self._read(p)) == 1

    def test_dedup_key_is_security_code_reportdate(self, tmp_path: Path) -> None:
        """④ 去重键 (SECURITY_CODE, REPORTDATE)：任一不同则不去重。"""
        p = tmp_path / "key.csv"
        r1 = _full_row()  # 000858 / 2025-03-31
        r2 = _full_row(report_date="2024-12-31")  # 同 SECURITY_CODE 不同 REPORTDATE
        r3 = _full_row(secucode="000001.SZ")  # 不同 SECURITY_CODE 同 REPORTDATE
        self._write(p, [r1, r2, r3])
        fetch_fin_indicators.dedupe_csv(p)
        assert len(self._read(p)) == 3

    def test_empty_file_no_crash(self, tmp_path: Path) -> None:
        """空文件不崩溃。"""
        p = tmp_path / "empty.csv"
        p.write_text("", encoding="utf-8-sig")
        fetch_fin_indicators.dedupe_csv(p)
        assert p.exists()

    def test_missing_pk_columns_no_crash(self, tmp_path: Path) -> None:
        """无 PK 列的文件不崩溃（保持原内容不删行）。"""
        p = tmp_path / "nopk.csv"
        p.write_text("a,b\n1,2\n", encoding="utf-8-sig")
        fetch_fin_indicators.dedupe_csv(p)
        assert self._read(p) == [{"a": "1", "b": "2"}]


class TestNonIncremental:
    """#135 T5: 非增量模式抓取语义不变 + 其他表 import 仍 INSERT IGNORE。"""

    async def test_no_incremental_keeps_reportdate_filter(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """① 无 --incremental 时 filter 仍为 (REPORTDATE='...')（PIN）。"""
        import asyncio

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(
            json_data={
                "success": True,
                "result": {"data": [_full_row()], "pages": 1},
            }
        )
        seen: list[str] = []
        orig_get = stub.get

        async def _get(url, params=None, headers=None):  # noqa: ANN001, ANN003
            seen.append(params.get("filter", "") if params else "")
            return await orig_get(url, params=params, headers=headers)

        stub.get = _get  # type: ignore[method-assign]

        with (
            patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
            patch.object(
                fetch_fin_indicators.sys,
                "argv",
                ["fetch_fin_indicators.py", "--years", "2025", "--periods", "Q1"],
            ),
        ):
            await fetch_fin_indicators.main()

        assert seen, "non-incremental mode must fetch"
        assert all(
            "(REPORTDATE='2025-03-31')" in f for f in seen
        ), f"non-incremental filter must stay REPORTDATE, got {seen}"

    @pytest.mark.parametrize(
        "module_name",
        [
            "fetch_main_flow",
            "fetch_dragon",
            "fetch_block_trade",
            "fetch_institution_survey",
        ],
    )
    def test_other_tables_stay_insert_ignore(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path, module_name: str
    ) -> None:
        """② main_flow/dragon/block_trade/institution_survey import 仍 INSERT IGNORE。"""
        import importlib

        mod = importlib.import_module(module_name)
        mock_import = Mock()
        monkeypatch.setattr(mod, "import_replace_table", mock_import)
        mod.import_to_dolt()
        assert mock_import.call_count == 1
        kwargs = mock_import.call_args.kwargs
        assert kwargs.get("merge") is True
        assert "INSERT IGNORE" in kwargs["insert_sql"], (
            f"{module_name} import must keep INSERT IGNORE semantics"
        )
