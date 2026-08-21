"""Adversarial RED tests for issue #299 — F10 UPDATE_DATE incremental edge cases.

These attack the plan's declared contracts beyond the requirement happy-path
suite: anchor resolution edge cases, no-anchor behavior, state non-advancement,
UPDATE_DATE missing/future values, fetch pagination cap, and the new
``dedupe_csv(date_col=...)`` parameter.

Current implementation lacks the new interfaces, so these tests fail with
AttributeError / TypeError / assertion failures — all valid RED.
"""

import asyncio
import csv
import importlib
import json
import sys
from pathlib import Path
from unittest.mock import AsyncMock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

F10 = [
    ("balance_sheet", "fetch_balance_sheet", "RPT_F10_FINANCE_GBALANCE", "fin_balance_sheet"),
    ("income", "fetch_income", "RPT_F10_FINANCE_GINCOME", "fin_income"),
    ("cash_flow", "fetch_cash_flow", "RPT_F10_FINANCE_GCASHFLOW", "fin_cash_flow"),
]


def _import_module(name: str):
    return importlib.import_module(name)


def _common():
    return importlib.import_module("common")


def _make_dolt_dir(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Point COMPASS_DATA_DIR at a temp dir containing a minimal .dolt marker."""
    (tmp_path / ".dolt").mkdir()
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))


# ═══════════════════════════════════════════════════════════════════
# update_date_anchor / normalize_update_date (common)
# ═══════════════════════════════════════════════════════════════════

class TestUpdateDateAnchorAdversarial:
    def _call(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
        *,
        data_updated: str | None,
        state: dict | None,
        report_name: str = "RPT_F10_FINANCE_GBALANCE",
        dolt_table: str = "fin_balance_sheet",
        state_name: str = "RPT_F10_FINANCE_GBALANCE.state.json",
    ) -> str:
        common = _common()
        _make_dolt_dir(tmp_path, monkeypatch)

        def fake_dolt_sql_csv(sql: str) -> str:
            if "SELECT last_updated" in sql:
                value = data_updated if data_updated is not None else "NULL"
                return f"last_updated\n{value}\n"
            return ""

        monkeypatch.setattr(common, "dolt_sql_csv", fake_dolt_sql_csv)

        state_path = tmp_path / state_name
        if state is not None:
            state_path.write_text(json.dumps(state))

        return common.update_date_anchor(report_name, state_path, dolt_table=dolt_table)

    def test_anchor_is_min_of_data_updates_and_state(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """双源取 min：data_updates=2026-08-03, state=2026-07-01 → 2026-07-01。

        RED: common.update_date_anchor 不存在 → AttributeError。
        """
        result = self._call(
            tmp_path, monkeypatch,
            data_updated="2026-08-03",
            state={"last_update_date": "2026-07-01"},
        )
        assert result == "2026-07-01"

    def test_anchor_uses_dolt_table_for_f10_data_updates(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """F10 必须查 DOLT_TABLE（fin_balance_sheet），不是 report_name。"""
        common = _common()
        _make_dolt_dir(tmp_path, monkeypatch)
        seen: list[str] = []

        def fake_dolt_sql_csv(sql: str) -> str:
            seen.append(sql)
            return "last_updated\n2026-08-03\n"

        monkeypatch.setattr(common, "dolt_sql_csv", fake_dolt_sql_csv)
        common.update_date_anchor(
            "RPT_F10_FINANCE_GBALANCE", tmp_path / "missing.state.json",
            dolt_table="fin_balance_sheet",
        )
        assert any("'fin_balance_sheet'" in sql for sql in seen), seen

    def test_single_source_missing_uses_other_source(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """data_updates 为 NULL 时仅 state 源生效。"""
        result = self._call(
            tmp_path, monkeypatch,
            data_updated=None,
            state={"last_update_date": "2026-07-01"},
        )
        assert result == "2026-07-01"

    def test_future_anchor_clamped_to_today(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """未来 anchor clamp 到今天（防止预标未来日期造成空窗口）。"""
        from datetime import date

        result = self._call(
            tmp_path, monkeypatch,
            data_updated=None,
            state={"last_update_date": "2099-01-01"},
        )
        assert result == date.today().isoformat()


# ═══════════════════════════════════════════════════════════════════
# common.normalize_update_date — numeric/compact date forms
# ═══════════════════════════════════════════════════════════════════

class TestNormalizeUpdateDateAdversarial:
    def test_numeric_yyyymmdd_and_float_are_normalized(self) -> None:
        """紧凑数字日期（20260805 / 20260805.0）也应归一为 YYYY-MM-DD。

        RED: 当前 regex 只认 -/ 分隔形式，数字日期返回 None。
        """
        common = _common()
        assert common.normalize_update_date("20260805") == "2026-08-05"
        assert common.normalize_update_date(20260805) == "2026-08-05"
        assert common.normalize_update_date(20260805.0) == "2026-08-05"
        assert common.normalize_update_date("20260805.0") == "2026-08-05"
        assert common.normalize_update_date("20260805.00") == "2026-08-05"
        assert common.normalize_update_date("202608") is None
        assert common.normalize_update_date(2026) is None


# ═══════════════════════════════════════════════════════════════════
# run(incremental=True) — state edge cases
# ═══════════════════════════════════════════════════════════════════

class TestRunIncrementalAdversarial:
    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,dolt_table", F10, ids=[f[0] for f in F10])
    async def test_empty_result_does_not_write_state(
        self, target, mod, report, dolt_table,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """0 行结果不写 state.json（锚点绝不空推进）。"""
        module = _import_module(mod)

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            return []

        fake_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        assert not (tmp_path / f"{report}.state.json").exists()

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,dolt_table", F10, ids=[f[0] for f in F10])
    async def test_all_missing_update_date_preserves_previous_anchor(
        self, target, mod, report, dolt_table,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """本批 UPDATE_DATE 全缺失时，state.last_update_date 保留旧值而非清空。"""
        module = _import_module(mod)
        state_path = tmp_path / f"{report}.state.json"
        state_path.write_text(json.dumps({
            "last_report_date": "2024-12-31",
            "last_update_date": "2025-01-01",
        }))

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            return [
                {
                    "SECUCODE": "000001.SZ",
                    "SECURITY_CODE": "000001",
                    "REPORT_DATE": "2024-12-31",
                    "UPDATE_DATE": "",
                    "TOTAL_ASSETS": "100",
                }
            ]

        fake_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        assert state_path.exists()
        state = json.loads(state_path.read_text())
        assert state["last_update_date"] == "2025-01-01"

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,dolt_table", F10, ids=[f[0] for f in F10])
    async def test_fetch_failure_propagates_and_does_not_write_state(
        self, target, mod, report, dolt_table,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """抓取异常必须向上传播，不能伪装成空窗口成功返回。

        RED: 当前 fetch_incremental 吞掉异常并返回 0，run() 打印 Done: 0。
        """
        module = _import_module(mod)

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            raise RuntimeError("boom")

        fake_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            with pytest.raises(RuntimeError, match="boom"):
                await module.run(incremental=True)

        assert not (tmp_path / f"{report}.state.json").exists()

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,dolt_table", F10, ids=[f[0] for f in F10])
    async def test_state_last_report_date_normalizes_time_suffix(
        self, target, mod, report, dolt_table,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """state.last_report_date 应存 YYYY-MM-DD，不残留 API 时间后缀。

        RED: 当前 max_report_date 直接取原始字符串 "2024-12-31 00:00:00"。
        """
        module = _import_module(mod)

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            return [
                {
                    "SECUCODE": "000001.SZ",
                    "SECURITY_CODE": "000001",
                    "REPORT_DATE": "2024-12-31 00:00:00",
                    "UPDATE_DATE": "2026-08-05 00:00:00",
                    "TOTAL_ASSETS": "100",
                }
            ]

        fake_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        state_path = tmp_path / f"{report}.state.json"
        assert state_path.exists()
        state = json.loads(state_path.read_text())
        assert state["last_report_date"] == "2024-12-31"
        assert state["last_update_date"] == "2026-08-05"


# ═══════════════════════════════════════════════════════════════════
# common.fetch_by_update_date — request shape + page cap
# ═══════════════════════════════════════════════════════════════════

class TestFetchByUpdateDateAdversarial:
    async def test_uses_update_date_filter_and_sort(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """构造 filter=(UPDATE_DATE>='anchor') + sortColumns=UPDATE_DATE。"""
        common = _common()
        captured: list[dict] = []

        async def fake_proxy_get(session, pool, url, params=None, headers=None):
            captured.append(params)
            return StubResponse(json_data={
                "success": True,
                "result": {"data": [], "pages": 1},
            })

        monkeypatch.setattr(common, "proxy_get", fake_proxy_get, raising=False)

        class _Throttle:
            async def acquire(self) -> None:
                return None

        await common.fetch_by_update_date(
            object(), _Throttle(), "RPT_F10_FINANCE_GBALANCE", "2026-08-01", 100
        )

        assert captured, "fetch_by_update_date must issue a request"
        params = captured[0]
        assert "(UPDATE_DATE>='2026-08-01')" in params["filter"]
        assert params["sortColumns"] == "UPDATE_DATE"
        assert params["reportName"] == "RPT_F10_FINANCE_GBALANCE"

    async def test_pages_capped_at_500(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """API pages=501 时最多只请求 500 页（资源耗尽防护）。"""
        common = _common()
        seen_pages: list[int] = []

        async def fake_proxy_get(session, pool, url, params=None, headers=None):
            page = int(params["pageNumber"])
            seen_pages.append(page)
            return StubResponse(json_data={
                "success": True,
                "result": {
                    "data": [{
                        "SECUCODE": "000001.SZ",
                        "SECURITY_CODE": "000001",
                        "REPORT_DATE": "2024-12-31",
                        "UPDATE_DATE": "2026-08-05 00:00:00",
                    }],
                    "pages": 501,
                },
            })

        monkeypatch.setattr(common, "proxy_get", fake_proxy_get, raising=False)

        class _Throttle:
            async def acquire(self) -> None:
                return None

        rows = await common.fetch_by_update_date(
            object(), _Throttle(), "RPT_F10_FINANCE_GBALANCE", "2026-08-01", 100
        )
        assert len(seen_pages) == 500
        assert seen_pages[-1] == 500
        assert len(rows) == 500


# ═══════════════════════════════════════════════════════════════════
# common.dedupe_csv — date_col parameter
# ═══════════════════════════════════════════════════════════════════

class TestDedupeCsvAdversarial:
    def test_dedupe_csv_default_still_uses_reportdate(
        self, tmp_path: Path,
    ) -> None:
        """回归守卫：默认 date_col=REPORTDATE 行为保持 keep-last。"""
        common = _common()
        path = tmp_path / "fin.csv"
        path.write_text(
            "SECURITY_CODE,REPORTDATE,VALUE\n"
            "000001,2024-12-31,100\n"
            "000001,2024-12-31,200\n",
            encoding="utf-8-sig",
        )
        common.dedupe_csv(path)
        with open(path, encoding="utf-8-sig", newline="") as f:
            rows = list(csv.DictReader(f))
        assert len(rows) == 1
        assert rows[0]["VALUE"] == "200"

    def test_dedupe_csv_supports_f10_report_date(
        self, tmp_path: Path,
    ) -> None:
        """F10 的日期列是 REPORT_DATE；date_col 参数必须生效。

        RED: 当前 dedupe_csv 无 date_col 参数 → TypeError。
        """
        common = _common()
        path = tmp_path / "f10.csv"
        path.write_text(
            "SECURITY_CODE,REPORT_DATE,VALUE\n"
            "000001,2024-12-31,100\n"
            "000001,2024-12-31,200\n",
            encoding="utf-8-sig",
        )
        common.dedupe_csv(path, date_col="REPORT_DATE")
        with open(path, encoding="utf-8-sig", newline="") as f:
            rows = list(csv.DictReader(f))
        assert len(rows) == 1
        assert rows[0]["VALUE"] == "200"
