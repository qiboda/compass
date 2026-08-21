"""RED requirement tests for issue #299 — F10 三表按 UPDATE_DATE 增量抓取 + merge 导入.

Contracts under test (from .dsh/plans/f10-update-date-incremental.md + issue #299):

1. Three F10 modules (fetch_balance_sheet / fetch_income / fetch_cash_flow):
   - ``run(..., incremental=False)`` gains an ``incremental`` kwarg.
   - ``incremental=True`` uses the single UPDATE_DATE anchor path (no REPORT_DATE
     enumeration), with no-anchor falling back to the fixed ``"2020-01-01"``.
   - CSV written to ``csv_dir()/<REPORT_NAME>.csv``; when ``total_records > 0`` a
     ``<REPORT_NAME>.state.json`` is written with ``last_report_date``,
     ``last_update_date``, ``total_rows``, ``last_run``.
   - ``import_to_dolt()`` uses merge semantics (``import_replace_table(..., merge=True)``
     + ``INSERT ... ON DUPLICATE KEY UPDATE``): first-run creates the table,
     incremental CSVs keep history, same-PK revisions overwrite, data_updates updated.

2. main.py:
   - ``fetch balance_sheet --incremental`` (and income / cash_flow) takes the
     UPDATE_DATE incremental path.
   - ``do_sync()`` calls ``run(incremental=True)`` for the three tables.

Current implementation lacks these interfaces, so these tests are expected to fail
(TypeError / AttributeError / SystemExit / assertion failures are all valid RED).
"""

import asyncio
import csv
import json
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import AsyncMock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# Report_name / module / dolt_table / a DOUBLE value column present in each COLS.
F10 = [
    ("balance_sheet", "fetch_balance_sheet", "RPT_F10_FINANCE_GBALANCE", "fin_balance_sheet", "TOTAL_ASSETS"),
    ("income", "fetch_income", "RPT_F10_FINANCE_GINCOME", "fin_income", "PARENT_NETPROFIT"),
    ("cash_flow", "fetch_cash_flow", "RPT_F10_FINANCE_GCASHFLOW", "fin_cash_flow", "NETCASH_OPERATE"),
]


def _import_module(name: str):
    __import__(name)
    return sys.modules[name]


# ═══════════════════════════════════════════════════════════════════
# A. run(incremental=True) — UPDATE_DATE 单次增量 + state.json 落盘
# ═══════════════════════════════════════════════════════════════════

class TestRunIncremental:
    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    async def test_incremental_uses_update_date_single_fetch_and_writes_csv(
        self, target, mod, report, csv_name, val_col,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """incremental=True 构造 UPDATE_DATE>=anchor 单次增量拉取并写 CSV。

        RED: 当前 run() 无 incremental 参数 → TypeError。
        GREEN: 走 fetch_by_update_date，绕开 REPORT_DATE 枚举，写 CSV。
        """
        module = _import_module(mod)
        calls: list[dict] = []

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            calls.append({"report_name": report_name, "anchor": anchor, "page_size": page_size})
            return [
                {
                    "SECUCODE": "000001.SZ",
                    "SECURITY_CODE": "000001",
                    "REPORT_DATE": "2024-12-31",
                    "UPDATE_DATE": "2026-08-05 00:00:00",
                    val_col: "100",
                }
            ]

        # Attrs do not exist yet (RED) — use raising=False so GREEN can land names later.
        monkeypatch.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
        monkeypatch.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
        monkeypatch.setattr(module, "fetch_paginated", None, raising=False)
        monkeypatch.setattr(asyncio, "sleep", AsyncMock())

        fake_async_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_async_session, raising=False)
            output = await module.run(incremental=True, page_size=250)

        # single UPDATE_DATE fetch, no per-period enumeration
        assert len(calls) == 1
        assert calls[0]["report_name"] == report
        assert calls[0]["anchor"] == "2025-01-01"
        assert calls[0]["page_size"] == 250

        csv_path = tmp_path / f"{report}.csv"
        assert output == csv_path
        assert csv_path.exists()
        text = csv_path.read_text(encoding="utf-8-sig")
        assert "REPORT_DATE" in text and "UPDATE_DATE" in text

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    async def test_no_anchor_falls_back_to_2020_01_01(
        self, target, mod, report, csv_name, val_col,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """无 anchor（首次运行/无 state/无 data_updates）→ 固定 '2020-01-01' 走 UPDATE_DATE。

        RED: run() 无 incremental 参数 → TypeError。
        GREEN: update_date_anchor 返回 '' 时 fetch_by_update_date 收到 '2020-01-01'。
        """
        module = _import_module(mod)
        calls: list[dict] = []

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            calls.append({"anchor": anchor})
            return [
                {
                    "SECUCODE": "000001.SZ",
                    "SECURITY_CODE": "000001",
                    "REPORT_DATE": "2024-12-31",
                    "UPDATE_DATE": "2026-08-05 00:00:00",
                    val_col: "100",
                }
            ]

        fake_async_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_async_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        assert len(calls) == 1
        assert calls[0]["anchor"] == "2020-01-01"

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    async def test_writes_state_json_with_required_keys(
        self, target, mod, report, csv_name, val_col,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """total_records>0 时写 <REPORT_NAME>.state.json，含 last_report_date /
        last_update_date / total_rows / last_run。

        RED: run() 无 incremental → TypeError；state 文件也从未写入。
        """
        module = _import_module(mod)

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            return [
                {
                    "SECUCODE": "000001.SZ",
                    "SECURITY_CODE": "000001",
                    "REPORT_DATE": "2024-12-31",
                    "UPDATE_DATE": "2026-08-05 00:00:00",
                    val_col: "100",
                },
                {
                    "SECUCODE": "000002.SZ",
                    "SECURITY_CODE": "000002",
                    "REPORT_DATE": "2023-12-31",
                    "UPDATE_DATE": "2026-07-01 00:00:00",
                    val_col: "200",
                },
            ]

        fake_async_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_async_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        state_path = tmp_path / f"{report}.state.json"
        assert state_path.exists()
        state = json.loads(state_path.read_text())
        assert state["last_report_date"] == "2024-12-31"
        assert state["last_update_date"] == "2026-08-05"
        assert state["total_rows"] == 2
        assert isinstance(state["last_run"], str) and state["last_run"]

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    async def test_no_report_date_enumeration_in_incremental(
        self, target, mod, report, csv_name, val_col,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """incremental 路径绝不枚举 REPORT_DATE（只调用 fetch_by_update_date）。

        RED: run() 无 incremental → TypeError。
        GREEN: fetch_paginated（REPORT_DATE 枚举用）不被调用。
        """
        module = _import_module(mod)
        paginated_calls: list = []

        async def bad_paginated(*a, **k):
            paginated_calls.append(a)
            return []

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            return []

        fake_async_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", lambda r, s, **k: "2025-01-01", raising=False)
            mp.setattr(module, "fetch_paginated", bad_paginated, raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_async_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        assert paginated_calls == [], "incremental must not enumerate REPORT_DATE"

    @pytest.mark.asyncio
    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    async def test_run_forwards_dolt_table_to_anchor(
        self, target, mod, report, csv_name, val_col,
        make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """run(incremental=True) 必须把 DOLT_TABLE 传给 update_date_anchor。

        data_updates 按 Dolt 表名（fin_balance_sheet 等）登记，不是 report_name；
        若未来重构丢掉该 kwarg 会静默查错表。
        """
        module = _import_module(mod)
        captured: dict = {}

        def fake_anchor(report_name, state_path, **kwargs):
            captured["report_name"] = report_name
            captured["state_path"] = state_path
            captured.update(kwargs)
            return "2025-01-01"

        async def fake_fetch(session, throttle, report_name, anchor, page_size=100, *, pool=None):
            return []

        fake_session = make_stub_session()

        with pytest.MonkeyPatch.context() as mp:
            mp.setattr(module, "fetch_by_update_date", fake_fetch, raising=False)
            mp.setattr(module, "update_date_anchor", fake_anchor, raising=False)
            mp.setattr(module, "AsyncSession", lambda **k: fake_session, raising=False)
            monkeypatch.setattr(asyncio, "sleep", AsyncMock())
            await module.run(incremental=True)

        assert captured.get("dolt_table") == csv_name


# ═══════════════════════════════════════════════════════════════════
# B. import_to_dolt() merge 语义（temp Dolt + COMPASS_DATA_DIR）
# ═══════════════════════════════════════════════════════════════════

class TestImportMergeSemantics:
    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        """Init temp Dolt, point COMPASS_DATA_DIR at it. Returns (dir, dolt_sql_csv)."""
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
            "INSERT INTO stock_basic VALUES ('SZ000001'), ('SZ000002')"
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

    def _make_row(self, module, secucode: str, report_date: str, value: str, val_col: str) -> list[str]:
        header = [c.strip() for c in module.COLS.split(",")] + ["REPORT_DATE"]
        row = [""] * len(header)
        row[header.index("SECUCODE")] = secucode
        row[header.index("SECURITY_CODE")] = secucode.split(".")[0]
        row[header.index("REPORT_DATE")] = report_date
        row[header.index(val_col)] = value
        return row

    def _header(self, module) -> list[str]:
        return [c.strip() for c in module.COLS.split(",")] + ["REPORT_DATE"]

    def _write_csv(self, module, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(self._header(module))
            writer.writerows(rows)

    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    def test_first_run_creates_table_and_imports(
        self, target, mod, report, csv_name, val_col,
        dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path,
    ) -> None:
        """首建表成功（merge 模式也能在首建时导入）。"""
        module = _import_module(mod)
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(module, csv_path, [self._make_row(module, "000001.SZ", "2024-12-31", "100", val_col)])

        rows = module.import_to_dolt(csv_path)

        assert rows == 1
        assert self._last(dolt_sql_csv(f"SELECT COUNT(*) FROM {csv_name}")) == "1"
        row = dolt_sql_csv(
            f"SELECT row_count, last_report_date FROM data_updates "
            f"WHERE table_name='{csv_name}'"
        ).strip()
        assert "1" in row and "2024-12-31" in row

    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    def test_incremental_csv_keeps_historical_rows(
        self, target, mod, report, csv_name, val_col,
        dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path,
    ) -> None:
        """增量 CSV（仅含新/修订行的子集）导入后，历史行保留（merge 语义）。

        RED: 当前 replace 语义会用增量 CSV 清掉整表 → 历史行丢失，断言失败。
        """
        module = _import_module(mod)
        dolt_dir_, dolt_sql_csv = dolt_env
        full_csv = tmp_path / "full.csv"
        self._write_csv(
            module, full_csv,
            [
                self._make_row(module, "000001.SZ", "2023-12-31", "50", val_col),  # historical row
                self._make_row(module, "000001.SZ", "2024-12-31", "100", val_col),
            ],
        )
        assert module.import_to_dolt(full_csv) == 2

        # incremental CSV only carries the revised/new rows, NOT the untouched history
        inc_csv = tmp_path / "inc.csv"
        self._write_csv(
            module, inc_csv,
            [
                self._make_row(module, "000001.SZ", "2024-12-31", "200", val_col),  # revision
                self._make_row(module, "000002.SZ", "2025-06-30", "300", val_col),  # new row
            ],
        )
        module.import_to_dolt(inc_csv)

        # merge: all 3 distinct (symbol, report_date) rows survive
        assert self._last(dolt_sql_csv(f"SELECT COUNT(*) FROM {csv_name}")) == "3"
        # historical row untouched
        assert self._last(dolt_sql_csv(
            f"SELECT {val_col} FROM {csv_name} WHERE symbol='SZ000001' AND report_date='2023-12-31'"
        )) == "50"
        # revision overwritten to new value
        assert self._last(dolt_sql_csv(
            f"SELECT {val_col} FROM {csv_name} WHERE symbol='SZ000001' AND report_date='2024-12-31'"
        )) == "200"

    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    def test_import_replace_table_called_with_merge_true(
        self, target, mod, report, csv_name, val_col,
        dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """import_to_dolt() 以 merge=True + ODKU 语义调用 import_replace_table。

        RED: 当前调用 merge=False → 捕获的 merge 非 True，断言失败。
        """
        module = _import_module(mod)
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(module, csv_path, [self._make_row(module, "000001.SZ", "2024-12-31", "100", val_col)])

        captured: dict = {}
        def fake_import_replace_table(**kwargs) -> int:
            captured.update(kwargs)
            return 0

        monkeypatch.setattr(module, "import_replace_table", fake_import_replace_table, raising=False)
        module.import_to_dolt(csv_path)

        assert "merge" in captured, "import_replace_table must be called with merge=..."
        assert captured["merge"] is True, "F10 import must use merge (ODKU) semantics"
        # ODKU-style upsert, not plain INSERT IGNORE
        assert "ON DUPLICATE KEY UPDATE" in captured.get("insert_sql", "")

    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    def test_data_updates_updated_with_row_count_and_last_report_date(
        self, target, mod, report, csv_name, val_col,
        dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path,
    ) -> None:
        """增量导入后 data_updates 的 row_count / last_report_date 反映新状态。"""
        module = _import_module(mod)
        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "bs.csv"
        self._write_csv(
            module, csv_path,
            [
                self._make_row(module, "000001.SZ", "2023-12-31", "50", val_col),
                self._make_row(module, "000001.SZ", "2024-12-31", "100", val_col),
            ],
        )
        module.import_to_dolt(csv_path)

        row = dolt_sql_csv(
            f"SELECT row_count, last_report_date FROM data_updates WHERE table_name='{csv_name}'"
        )
        assert "2" in row and "2024-12-31" in row


# ═══════════════════════════════════════════════════════════════════
# C. main.py — dispatch_fetch / fetch CLI / do_sync
# ═══════════════════════════════════════════════════════════════════

class TestMainIncremental:
    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    def test_dispatch_fetch_passes_incremental_to_run(
        self, target, mod, report, csv_name, val_col, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """dispatch_fetch(target, incremental=True) 透传 incremental 到 run()。

        RED: current dispatch_fetch(target, years=None) 无 incremental → TypeError。
        """
        import main
        module = _import_module(mod)
        calls: list[dict] = []

        async def fake_run(**kwargs):
            calls.append(kwargs)
            return Path(f"{report}.csv")

        monkeypatch.setattr(module, "run", fake_run, raising=False)
        main.dispatch_fetch(target, incremental=True)

        assert calls, "dispatch_fetch must call run()"
        assert calls[0].get("incremental") is True

    @pytest.mark.parametrize("target,mod,report,csv_name,val_col", F10, ids=[f[0] for f in F10])
    def test_fetch_cli_supports_incremental_flag(
        self, target, mod, report, csv_name, val_col,
        monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """`fetch <target> --incremental` 是合法 CLI（不可 argparse 报错），并传给 run。

        RED: current fetch 子命令无 --incremental → argparse SystemExit。
        """
        import main
        module = _import_module(mod)
        calls: list[dict] = []

        async def fake_run(**kwargs):
            calls.append(kwargs)
            return Path(f"{report}.csv")

        monkeypatch.setattr(module, "run", fake_run, raising=False)
        monkeypatch.setattr(
            main.sys, "argv", ["main.py", "fetch", target, "--incremental"]
        )
        main.main()

        assert calls, "`fetch <target> --incremental` must reach run()"
        assert calls[0].get("incremental") is True

    def test_do_sync_calls_three_f10_tables_with_incremental_true(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """do_sync() 对 balance_sheet / income / cash_flow 均调用 run(incremental=True)。

        RED: current do_sync calls run() with no incremental → recorded kwarg missing.
        """
        import main

        collected: dict[str, list[dict]] = {}

        async def _noop_async() -> None:
            return None

        # Stub modules used by do_sync so no real network/import runs. Each
        # module gets its own fake so calls are attributable per call-site.
        def _stub(module_name: str) -> None:
            m = _import_module(module_name)

            async def fake_run(*_args, **_kwargs):
                collected.setdefault(module_name, []).append(_kwargs)
                return Path("out.csv")

            monkeypatch.setattr(m, "run", fake_run, raising=False)
            monkeypatch.setattr(m, "import_to_dolt", lambda *a, **k: 0, raising=False)

        for name in ("fetch_balance_sheet", "fetch_income", "fetch_cash_flow",
                     "fetch_dragon", "fetch_block_trade", "fetch_institution_survey",
                     "fetch_main_flow", "fetch_index_daily"):
            _stub(name)

        # stock_basic / fin_indicators special entrypoints + Dolt writes become no-ops
        sb = _import_module("fetch_stock_basic_official")
        monkeypatch.setattr(sb, "main", lambda: None, raising=False)
        monkeypatch.setattr(main, "_import_stock_basic", lambda: None, raising=False)
        fi = _import_module("fetch_fin_indicators")
        monkeypatch.setattr(fi, "main", _noop_async, raising=False)
        monkeypatch.setattr(main, "_import_fin_indicators", lambda: 0, raising=False)
        monkeypatch.setattr(main, "dolt_sql", lambda *a, **k: None, raising=False)

        main.do_sync()

        # do_sync must invoke run(incremental=True) for each of the three F10
        # tables specifically — not just "at least three incremental calls".
        assert collected.get("fetch_balance_sheet") == [{"incremental": True}]
        assert collected.get("fetch_income") == [{"incremental": True}]
        assert collected.get("fetch_cash_flow") == [{"incremental": True}]

    def test_do_sync_propagates_f10_fetch_failure(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """F10 抓取异常必须让 do_sync 失败，而不是静默继续导入旧数据。

        RED: 当前 fetch_incremental 吞掉异常，do_sync 会继续执行 import。
        """
        import main

        def _stub(module_name: str) -> None:
            m = _import_module(module_name)

            async def fake_run(*_args, **_kwargs):
                return Path("out.csv")

            monkeypatch.setattr(m, "run", fake_run, raising=False)
            monkeypatch.setattr(m, "import_to_dolt", lambda *a, **k: 0, raising=False)

        for name in ("fetch_balance_sheet", "fetch_income", "fetch_cash_flow",
                     "fetch_dragon", "fetch_block_trade", "fetch_institution_survey",
                     "fetch_main_flow", "fetch_index_daily"):
            _stub(name)

        fbs = _import_module("fetch_balance_sheet")

        async def boom(*_args, **_kwargs):
            raise RuntimeError("boom")

        monkeypatch.setattr(fbs, "run", boom, raising=False)

        sb = _import_module("fetch_stock_basic_official")
        monkeypatch.setattr(sb, "main", lambda: None, raising=False)
        monkeypatch.setattr(main, "_import_stock_basic", lambda: None, raising=False)
        fi = _import_module("fetch_fin_indicators")

        async def _noop_async() -> None:
            return None

        monkeypatch.setattr(fi, "main", _noop_async, raising=False)
        monkeypatch.setattr(main, "_import_fin_indicators", lambda: 0, raising=False)
        monkeypatch.setattr(main, "dolt_sql", lambda *a, **k: None, raising=False)

        with pytest.raises(RuntimeError, match="boom"):
            main.do_sync()

    def test_do_sync_propagates_fin_indicators_failure(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """fin_indicators 增量抓取异常必须让 do_sync 在 F10 之前失败。

        RED: 当前 fetch_fin_indicators 吞掉异常，do_sync 继续执行后续步骤。
        """
        import main

        sb = _import_module("fetch_stock_basic_official")
        monkeypatch.setattr(sb, "main", lambda: None, raising=False)
        monkeypatch.setattr(main, "_import_stock_basic", lambda: None, raising=False)

        fi = _import_module("fetch_fin_indicators")

        async def boom() -> None:
            raise RuntimeError("boom")

        monkeypatch.setattr(fi, "main", boom, raising=False)
        monkeypatch.setattr(main, "dolt_sql", lambda *a, **k: None, raising=False)

        with pytest.raises(RuntimeError, match="boom"):
            main.do_sync()
