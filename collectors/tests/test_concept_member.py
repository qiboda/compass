"""Tests for fetch_concept_member.py — import_to_dolt, run().

Concept board membership collector (概念板块成分).
Version-tracking semantics: each run fully replaces the previous version
(DELETE + full INSERT), no per-trading-day snapshots.
"""

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

        # symbol 拼接为 SH600880 前缀格式（同其他 collector）
        symbols = dolt_sql_csv("SELECT symbol FROM concept_member ORDER BY symbol")
        assert "SH600880" in symbols and "SZ300624" in symbols and "SH603999" in symbols

        # concept_code / concept_name 落库
        board = dolt_sql_csv(
            "SELECT concept_name FROM concept_member WHERE concept_code='BK1169' LIMIT 1"
        ).strip()
        assert "Kimi概念" in board

        # update_date 为当前日期（版本日期）
        upd = _last(dolt_sql_csv("SELECT MAX(update_date) FROM concept_member"))
        assert upd == date.today().isoformat()

        # data_updates 5 列 upsert
        du = dolt_sql_csv(
            "SELECT row_count, last_report_date FROM data_updates "
            "WHERE table_name='concept_member'"
        ).strip()
        assert "3" in du and date.today().isoformat() in du

    def test_rerun_replaces_version_without_stale_members(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """版本更新幂等：50 成分 → 45 成分重跑，被移除 5 只不复存在（删除传播）。"""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"

        # 第一版：50 只成分（BK1169 Kimi概念，SZ 代码 600001..600050）
        rows_50 = [_make_row(f"6000{i:02d}.SZ", "BK1169", "Kimi概念") for i in range(1, 51)]
        self._write_csv(csv_path, rows_50)
        assert import_to_dolt(csv_path) == 50
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "50"

        # 第二版：45 只成分（移除末尾 5 只 600046..600050）
        rows_45 = rows_50[:45]
        self._write_csv(csv_path, rows_45)
        assert import_to_dolt(csv_path) == 45
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "45"

        # 被移除的 5 只不复存在（删除传播到当前版本）
        removed = ",".join(f"'SZ6000{i:02d}'" for i in range(46, 51))
        gone = _last(dolt_sql_csv(
            f"SELECT COUNT(*) FROM concept_member WHERE symbol IN ({removed})"
        ))
        assert gone == "0"

        # 剩余成分仍存在
        kept = _last(dolt_sql_csv(
            "SELECT COUNT(*) FROM concept_member WHERE symbol IN ('SZ600001','SZ600045')"
        ))
        assert kept == "2"

        # data_updates row_count 随版本更新
        du = dolt_sql_csv(
            "SELECT row_count FROM data_updates WHERE table_name='concept_member'"
        ).strip()
        assert "45" in du

    def test_rerun_same_version_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """重跑相同版本：行数不变、无重复（PK 唯一）。"""
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
        """重跑时 INSERT 失败 → 恢复旧版本数据。"""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "cm.csv"
        self._write_csv(csv_path, [_make_row("600880.SH", "BK1169", "Kimi概念")])
        assert import_to_dolt(csv_path) == 1

        # 破坏性 CSV：NEW_BOARD_CODE 空 → concept_code NULL，违反 NOT NULL → INSERT 失败
        with open(csv_path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_HEADER)
            writer.writerow(["600880.SH", "600880", "", "Kimi概念"])
        assert import_to_dolt(csv_path) == 0
        assert _last(dolt_sql_csv("SELECT COUNT(*) FROM concept_member")) == "1"

    def test_csv_not_found_returns_zero(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """CSV 不存在时 import_to_dolt 返回 0。"""
        from fetch_concept_member import import_to_dolt  # noqa: E402

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        result = import_to_dolt(tmp_path / "nonexistent.csv")
        assert result == 0


# ── run() tests ──


class TestRun:
    def _board_list_json(
        self, boards: list[tuple[str, str]]
    ) -> dict:
        """板块列表响应（push2 clist 格式：f12 板块代码 / f14 板块名称）。"""
        return {
            "rc": 0,
            "data": {
                "total": len(boards),
                "diff": [{"f12": code, "f14": name} for code, name in boards],
            },
        }

    def _member_json(self, members: list[dict]) -> dict:
        """成分股响应（datacenter 格式）。"""
        return {
            "success": True,
            "result": {"count": len(members), "pages": 1, "data": members},
        }

    def _make_get(
        self,
        boards: list[tuple[str, str]],
        members: dict[str, list[dict]],
    ) -> Callable:
        """自定义 stub.get：板块列表 URL → 板块列表；其余 → 按 filter 取成分。"""

        async def _get(url: str, params: dict | None = None, headers: dict | None = None):  # noqa: ANN001, ANN002, ANN003
            if BOARD_LIST_URL in url:
                return StubResponse(json_data=self._board_list_json(boards))
            flt = (params or {}).get("filter", "")
            code = flt.split('"')[1] if '"' in flt else ""
            return StubResponse(json_data=self._member_json(members.get(code, [])))

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

    async def test_run_member_fetch_exception_continues(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """单个板块成分拉取失败 → 捕获并继续，不中断整体。"""
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
                return StubResponse(json_data=self._board_list_json(boards))
            flt = (params or {}).get("filter", "")
            code = flt.split('"')[1] if '"' in flt else ""
            if code == "BK1170":
                raise RuntimeError("simulated fetch error")
            return StubResponse(json_data=self._member_json(members.get(code, [])))

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_F10_CORETHEME_BOARDTYPE.csv"
        csv_path = tmp_path / result.name
        assert csv_path.exists()
        with open(csv_path, newline="", encoding="utf-8-sig") as f:
            rows = list(csv.DictReader(f))
        assert len(rows) == 1
        assert rows[0]["SECUCODE"] == "600880.SH"

    async def test_run_empty_board_list_writes_no_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """板块列表为空 → 直接返回（无 CSV 写入）。"""
        from fetch_concept_member import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session()
        stub.get = self._make_get([], {})  # type: ignore[method-assign]

        with patch("fetch_concept_member.AsyncSession", return_value=stub):
            result = await run()

        assert result.name == "RPT_F10_CORETHEME_BOARDTYPE.csv"
        assert not (tmp_path / result.name).exists()
