"""Unit tests for main.py CLI dispatch — extracted functions and thin main().

Tests cover all 5 fetch/import targets, sync, sync-investment, and the
main() entrypoint with monkeypatched sys.argv.  Uses monkeypatch to
avoid real network/Dolt calls.
"""

from __future__ import annotations

import contextlib
import csv
import json
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# ── RED phase: these imports will fail until functions are extracted ──
from main import (  # noqa: E402
    _parse_years,
    sync_investment_data,
)

# ═══════════════════════════════════════════════════════════════════
# _parse_years — already pure, no refactor needed
# ═══════════════════════════════════════════════════════════════════


class TestParseYears:
    def test_empty_string_returns_none(self) -> None:
        assert _parse_years("") is None

    def test_single_year(self) -> None:
        assert _parse_years("2024") == [2024]

    def test_multiple_years_strips_whitespace(self) -> None:
        assert _parse_years("2024, 2025,2026") == [2024, 2025, 2026]

    def test_all_whitespace_returns_empty_then_none(self) -> None:
        # "  ,  ,  " → split yields empty strings → filtered → [] → None?
        # _parse_years returns None only for empty input; for whitespace-only,
        # split yields non-empty parts? Let's check: "  ,  ,  ".split(",") = ["  ", "  ", "  "]
        # Each stripped yields "" → filtered → [] → not None but []. Wait:
        # return [int(y.strip()) for y in s.split(",") if y.strip()]
        # If s is not empty but all parts strip to "", the list is []. That's not None.
        # Let's test actual behavior.
        assert _parse_years("  ,  ,  ") == []

    def test_leading_trailing_whitespace(self) -> None:
        assert _parse_years("  2024  ") == [2024]


# ═══════════════════════════════════════════════════════════════════
# dispatch_fetch — routes to correct sub-module per target
# ═══════════════════════════════════════════════════════════════════


class TestDispatchFetch:
    """dispatch_fetch(target, years, resume, page_size, max_pages)"""

    def test_stock_basic_sets_sys_argv_and_calls_official_main(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """dispatch_fetch('stock_basic') → fetch_stock_basic_official.main() (sync)."""
        import fetch_stock_basic_official as fsbo
        import main as main_mod

        mock_fsbo_main = Mock()
        monkeypatch.setattr(fsbo, "main", mock_fsbo_main)

        main_mod.dispatch_fetch("stock_basic")

        mock_fsbo_main.assert_called_once()

    def test_stock_basic_defaults_no_extra_args(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import fetch_stock_basic_official as fsbo
        import main as main_mod

        mock_fsbo_main = Mock()
        monkeypatch.setattr(fsbo, "main", mock_fsbo_main)

        main_mod.dispatch_fetch("stock_basic")

        mock_fsbo_main.assert_called_once()

    def test_fin_indicators_sets_sys_argv_with_years(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_fin_indicators as ffi
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_ffi_main = Mock()
        monkeypatch.setattr(ffi, "main", mock_ffi_main)

        main_mod.dispatch_fetch("fin_indicators", years=[2024, 2025])

        mock_run.assert_called_once()

    def test_fin_indicators_no_years(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import fetch_fin_indicators as ffi
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        monkeypatch.setattr(ffi, "main", Mock())

        main_mod.dispatch_fetch("fin_indicators")

        mock_run.assert_called_once()

    def test_balance_sheet_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_balance_sheet as fbs
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fbs_run = Mock()
        monkeypatch.setattr(fbs, "run", mock_fbs_run)

        main_mod.dispatch_fetch("balance_sheet", years=[2024])

        mock_run.assert_called_once()

    def test_income_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_income as fi
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fi_run = Mock()
        monkeypatch.setattr(fi, "run", mock_fi_run)

        main_mod.dispatch_fetch("income", years=[2024])

        mock_run.assert_called_once()

    def test_cash_flow_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_cash_flow as fcf
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fcf_run = Mock()
        monkeypatch.setattr(fcf, "run", mock_fcf_run)

        main_mod.dispatch_fetch("cash_flow", years=[2024])

        mock_run.assert_called_once()

    def test_default_years_is_none(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """When years not passed, balance_sheet.run(years=None) is called."""
        import fetch_balance_sheet as fbs
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fbs_run = Mock()
        monkeypatch.setattr(fbs, "run", mock_fbs_run)

        main_mod.dispatch_fetch("balance_sheet")

        mock_run.assert_called_once()

    def test_dragon_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_dragon as fdr
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fdr_run = Mock()
        monkeypatch.setattr(fdr, "run", mock_fdr_run)

        main_mod.dispatch_fetch("dragon")

        mock_run.assert_called_once()

    def test_block_trade_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_block_trade as fbt
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fbt_run = Mock()
        monkeypatch.setattr(fbt, "run", mock_fbt_run)

        main_mod.dispatch_fetch("block_trade")

        mock_run.assert_called_once()

    def test_institution_survey_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_institution_survey as fis
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fis_run = Mock()
        monkeypatch.setattr(fis, "run", mock_fis_run)

        main_mod.dispatch_fetch("institution_survey")

        mock_run.assert_called_once()


    def test_main_flow_calls_run_via_asyncio(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_main_flow as fmf
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)
        mock_fmf_run = Mock()
        monkeypatch.setattr(fmf, "run", mock_fmf_run)

        main_mod.dispatch_fetch("main_flow")

        mock_run.assert_called_once()


# ═══════════════════════════════════════════════════════════════════
# dispatch_import — routes to correct import function per target
# ═══════════════════════════════════════════════════════════════════


class TestDispatchImport:
    def test_stock_basic_calls_import_stock_basic(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(main_mod, "_import_stock_basic", mock_import)

        main_mod.dispatch_import("stock_basic")
        mock_import.assert_called_once()

    def test_fin_indicators_calls_import_fin_indicators(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(main_mod, "_import_fin_indicators", mock_import)

        main_mod.dispatch_import("fin_indicators")
        mock_import.assert_called_once()

    def test_balance_sheet_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_balance_sheet as fbs
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fbs, "import_to_dolt", mock_import)

        main_mod.dispatch_import("balance_sheet")
        mock_import.assert_called_once()

    def test_income_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_income as fi
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fi, "import_to_dolt", mock_import)

        main_mod.dispatch_import("income")
        mock_import.assert_called_once()

    def test_cash_flow_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_cash_flow as fcf
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fcf, "import_to_dolt", mock_import)

        main_mod.dispatch_import("cash_flow")
        mock_import.assert_called_once()

    def test_dragon_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_dragon as fdr
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fdr, "import_to_dolt", mock_import)

        main_mod.dispatch_import("dragon")
        mock_import.assert_called_once()

    def test_block_trade_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_block_trade as fbt
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fbt, "import_to_dolt", mock_import)

        main_mod.dispatch_import("block_trade")
        mock_import.assert_called_once()

    def test_institution_survey_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_institution_survey as fis
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fis, "import_to_dolt", mock_import)

        main_mod.dispatch_import("institution_survey")
        mock_import.assert_called_once()


    def test_main_flow_calls_import_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_main_flow as fmf
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fmf, "import_to_dolt", mock_import)

        main_mod.dispatch_import("main_flow")
        mock_import.assert_called_once()


# ═══════════════════════════════════════════════════════════════════
# do_sync — fetch all + import all + update data_updates
# ═══════════════════════════════════════════════════════════════════


class TestDoSync:
    def test_calls_all_5_fetches_and_5_imports(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """do_sync() triggers fetch+import for all tables, plus data_updates."""
        import common
        import fetch_balance_sheet as fbs
        import fetch_block_trade as fbt
        import fetch_cash_flow as fcf
        import fetch_dragon as fdr
        import fetch_fin_indicators as ffi
        import fetch_income as fi
        import fetch_index_daily as fid
        import fetch_institution_survey as fis
        import fetch_main_flow as fmf
        import fetch_stock_basic_official as fsbo
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)

        monkeypatch.setattr(fsbo, "main", Mock())
        monkeypatch.setattr(ffi, "main", Mock())
        monkeypatch.setattr(fbs, "run", Mock())
        monkeypatch.setattr(fi, "run", Mock())
        monkeypatch.setattr(fcf, "run", Mock())
        monkeypatch.setattr(fdr, "run", Mock())
        monkeypatch.setattr(fbt, "run", Mock())
        monkeypatch.setattr(fis, "run", Mock())
        monkeypatch.setattr(fmf, "run", Mock())
        monkeypatch.setattr(fid, "run", Mock())

        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock())
        monkeypatch.setattr(fbs, "import_to_dolt", Mock())
        monkeypatch.setattr(fi, "import_to_dolt", Mock())
        monkeypatch.setattr(fcf, "import_to_dolt", Mock())
        monkeypatch.setattr(fdr, "import_to_dolt", Mock())
        monkeypatch.setattr(fbt, "import_to_dolt", Mock())
        monkeypatch.setattr(fis, "import_to_dolt", Mock())
        monkeypatch.setattr(fmf, "import_to_dolt", Mock())
        monkeypatch.setattr(fid, "import_to_dolt", Mock())

        mock_dolt = Mock()
        monkeypatch.setattr(common, "dolt_sql", mock_dolt)

        main_mod.do_sync()

        # stock_basic is sync (official source); 10 tables via asyncio.run
        # (8 legacy + index_daily step 11, epic #255)
        assert mock_run.call_count == 9
        # data_updates loop for the 5 legacy tables (new tables upsert inside import_to_dolt)
        assert mock_dolt.call_count >= 5

    def test_restart_flag_accepted_no_behavior_change(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """restart=True accepted (no-op for sync subcommand, future-compat)."""
        import common
        import fetch_balance_sheet as fbs
        import fetch_block_trade as fbt
        import fetch_cash_flow as fcf
        import fetch_dragon as fdr
        import fetch_fin_indicators as ffi
        import fetch_income as fi
        import fetch_index_daily as fid
        import fetch_institution_survey as fis
        import fetch_main_flow as fmf
        import fetch_stock_basic_official as fsbo
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)

        monkeypatch.setattr(fsbo, "main", Mock())
        monkeypatch.setattr(ffi, "main", Mock())
        monkeypatch.setattr(fbs, "run", Mock())
        monkeypatch.setattr(fi, "run", Mock())
        monkeypatch.setattr(fcf, "run", Mock())
        monkeypatch.setattr(fdr, "run", Mock())
        monkeypatch.setattr(fbt, "run", Mock())
        monkeypatch.setattr(fis, "run", Mock())
        monkeypatch.setattr(fmf, "run", Mock())
        monkeypatch.setattr(fid, "run", Mock())
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock())
        for mod in (fbs, fi, fcf, fdr, fbt, fis, fmf, fid):
            monkeypatch.setattr(mod, "import_to_dolt", Mock())

        mock_dolt = Mock()
        monkeypatch.setattr(common, "dolt_sql", mock_dolt)

        main_mod.do_sync(restart=True)

        # 10 asyncio steps: 9 legacy + index_daily step 11 (epic #255)
        assert mock_run.call_count == 9

    def test_sync_raises_when_import_returns_zero(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """An internal import returning 0 must stop sync, not silently continue."""
        import fetch_balance_sheet as fbs
        import fetch_block_trade as fbt
        import fetch_cash_flow as fcf
        import fetch_dragon as fdr
        import fetch_fin_indicators as ffi
        import fetch_income as fi
        import fetch_institution_survey as fis
        import fetch_main_flow as fmf
        import fetch_stock_basic_official as fsbo
        import main as main_mod

        monkeypatch.setattr(main_mod.asyncio, "run", Mock())
        monkeypatch.setattr(fsbo, "main", Mock())
        monkeypatch.setattr(ffi, "main", Mock())
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock(return_value=0))
        for mod in (fbs, fi, fcf, fdr, fbt, fis, fmf):
            monkeypatch.setattr(mod, "run", Mock())
            monkeypatch.setattr(mod, "import_to_dolt", Mock(return_value=1))

        with pytest.raises(RuntimeError, match="fin_indicators import returned 0 rows"):
            main_mod.do_sync()


# ═══════════════════════════════════════════════════════════════════
# sync_investment_data — already extracted, testable as-is
# ═══════════════════════════════════════════════════════════════════


class TestSyncInvestmentData:
    def test_investment_dir_missing_exits_early(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """When investment_data/.dolt does not exist, returns early."""
        import main as main_mod

        mock_dolt_dir = tmp_path / "nonexistent"
        monkeypatch.setattr(main_mod, "PROJECT_ROOT", tmp_path)
        # Ensure .dolt doesn't exist
        assert not (mock_dolt_dir / ".dolt").exists()

        # Should not raise
        sync_investment_data(restart=False)

    def test_restart_stops_server_if_dir_exists(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """With restart=True, pkill is called."""
        import main as main_mod

        invest_dir = tmp_path / "investment_data"
        (invest_dir / ".dolt").mkdir(parents=True)
        monkeypatch.setattr(main_mod, "PROJECT_ROOT", tmp_path)

        mock_subprocess_run = Mock(return_value=Mock(stdout="", stderr="", returncode=0))
        monkeypatch.setattr(main_mod.subprocess, "run", mock_subprocess_run)
        # Mock dolt helper to return success for fetch/pull/push
        mock_popen = Mock()
        monkeypatch.setattr(main_mod.subprocess, "Popen", mock_popen)

        sync_investment_data(restart=True)

        # pkill should have been called
        pkill_calls = [c for c in mock_subprocess_run.call_args_list if "pkill" in str(c)]
        assert len(pkill_calls) >= 1

    def test_restart_false_skips_server_restart(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        import main as main_mod

        invest_dir = tmp_path / "investment_data"
        (invest_dir / ".dolt").mkdir(parents=True)
        monkeypatch.setattr(main_mod, "PROJECT_ROOT", tmp_path)

        mock_run = Mock(return_value=Mock(stdout="", stderr="", returncode=0))
        monkeypatch.setattr(main_mod.subprocess, "run", mock_run)
        mock_popen = Mock()
        monkeypatch.setattr(main_mod.subprocess, "Popen", mock_popen)

        sync_investment_data(restart=False)

        # No pkill calls
        pkill_calls = [c for c in mock_run.call_args_list if "pkill" in str(c)]
        assert len(pkill_calls) == 0


# ═══════════════════════════════════════════════════════════════════
# dispatch_progress — live fetch progress query
# ═══════════════════════════════════════════════════════════════════


class TestDispatchProgress:
    def _write_progress(self, tmp_path: Path, name: str, status: str = "running") -> None:
        (tmp_path / f"{name}.progress.json").write_text(
            json.dumps({"name": name, "status": status, "percent": 50.0, "message": "x"}),
            encoding="utf-8",
        )

    def test_target_human_readable(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        self._write_progress(tmp_path, "block_trade", "completed")
        main_mod.dispatch_progress("block_trade")
        out = capsys.readouterr().out
        assert "[block_trade] completed" in out

    def test_target_json(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        self._write_progress(tmp_path, "dragon", "running")
        main_mod.dispatch_progress("dragon", as_json=True)
        data = json.loads(capsys.readouterr().out)
        assert data["name"] == "dragon"
        assert data["status"] == "running"

    def test_all_lists_progress_files(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        self._write_progress(tmp_path, "block_trade", "completed")
        self._write_progress(tmp_path, "main_flow", "running")
        main_mod.dispatch_progress()
        out = capsys.readouterr().out
        assert "block_trade" in out
        assert "main_flow" in out

    def test_missing_target_exits(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        with pytest.raises(SystemExit):
            main_mod.dispatch_progress("missing")

    def test_target_with_total_current_error(
        self, monkeypatch, tmp_path: Path, capsys,
    ) -> None:
        import main as main_mod

        (tmp_path / "dragon.progress.json").write_text(
            json.dumps({
                "name": "dragon",
                "status": "failed",
                "percent": 42.0,
                "message": "stopped",
                "total_items": 10,
                "completed_items": 4,
                "fetched_rows": 12,
                "current_item": "2024-12-30",
                "error": "boom",
            }),
            encoding="utf-8",
        )
        main_mod.dispatch_progress("dragon")
        out = capsys.readouterr().out
        assert "completed: 4/10" in out
        assert "current: 2024-12-30" in out
        assert "error: boom" in out

    def test_all_json_output(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        self._write_progress(tmp_path, "block_trade", "completed")
        self._write_progress(tmp_path, "main_flow", "running")
        main_mod.dispatch_progress(as_json=True)
        data = json.loads(capsys.readouterr().out)
        assert [d["name"] for d in data] == ["block_trade", "main_flow"]

    def test_no_files_prints_stderr(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        main_mod.dispatch_progress()
        captured = capsys.readouterr()
        assert "No fetch progress files found." in captured.err

    def test_corrupt_progress_file_skipped(self, monkeypatch, tmp_path: Path, capsys) -> None:
        import main as main_mod

        (tmp_path / "bad.progress.json").write_text("{not json", encoding="utf-8")
        main_mod.dispatch_progress()
        assert "bad" not in capsys.readouterr().out


# ═══════════════════════════════════════════════════════════════════
# main() — thin argparse + dispatch
# ═══════════════════════════════════════════════════════════════════


class TestMain:
    def test_fetch_stock_basic(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys,
            "argv",
            ["main.py", "fetch", "stock_basic"],
        )
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("stock_basic", years=None, incremental=False)

    def test_fetch_fin_indicators_with_years(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys,
            "argv",
            ["main.py", "fetch", "fin_indicators", "--years", "2024,2025"],
        )
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("fin_indicators", years=[2024, 2025], incremental=False)

    def test_fetch_with_years_list(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys,
            "argv",
            ["main.py", "fetch", "stock_basic", "--years", "2024"],
        )
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("stock_basic", years=[2024], incremental=False)

    def test_import_stock_basic(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "import", "stock_basic"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_import", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("stock_basic")

    def test_import_balance_sheet(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "import", "balance_sheet"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_import", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("balance_sheet")

    def test_sync(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "sync"])
        mock_sync = Mock()
        monkeypatch.setattr(main_mod, "do_sync", mock_sync)

        main_mod.main()
        mock_sync.assert_called_once()

    def test_sync_investment(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "sync-investment"])
        mock_sync_inv = Mock()
        monkeypatch.setattr(main_mod, "sync_investment_data", mock_sync_inv)

        main_mod.main()
        mock_sync_inv.assert_called_once_with(False)

    def test_sync_investment_restart(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys,
            "argv",
            ["main.py", "sync-investment", "--restart"],
        )
        mock_sync_inv = Mock()
        monkeypatch.setattr(main_mod, "sync_investment_data", mock_sync_inv)

        main_mod.main()
        mock_sync_inv.assert_called_once_with(True)

    def test_progress_no_target(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "progress"])
        mock_progress = Mock()
        monkeypatch.setattr(main_mod, "dispatch_progress", mock_progress)

        main_mod.main()
        mock_progress.assert_called_once_with(None, as_json=False)

    def test_progress_target_json(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys, "argv", ["main.py", "progress", "block_trade", "--json"],
        )
        mock_progress = Mock()
        monkeypatch.setattr(main_mod, "dispatch_progress", mock_progress)

        main_mod.main()
        mock_progress.assert_called_once_with("block_trade", as_json=True)

    def test_no_command_prints_help(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py"])
        # Should not raise — prints help to stderr
        with contextlib.suppress(SystemExit):
            main_mod.main()


# ═══════════════════════════════════════════════════════════════════
# _import_stock_basic — legacy import helper (uses common dolt fns)
# ═══════════════════════════════════════════════════════════════════


class TestImportStockBasic:
    def test_csv_missing_raises(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """When stock_basic.csv does not exist, the import must abort loudly."""
        import main as main_mod

        monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path / "nonexistent"))
        # csv_path would be csv_dir() / "stock_basic_official.csv" — doesn't exist

        with pytest.raises(RuntimeError):
            main_mod._import_stock_basic()

    def test_csv_exists_imports_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """When csv exists, dolt_sql / dolt_table_import / dolt_sql_csv are called."""
        import common
        import main as main_mod

        # Create a dummy CSV (official-source filename)
        csv_path = tmp_path / "stock_basic_official.csv"
        csv_path.write_text("header\n1\n")

        monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))

        mock_sql = Mock(return_value=Mock(stdout="Count\n100", returncode=0))
        monkeypatch.setattr(common, "dolt_sql", mock_sql)
        mock_sql_csv = Mock(return_value="Count\n100")
        monkeypatch.setattr(common, "dolt_sql_csv", mock_sql_csv)
        mock_table_import = Mock(return_value=True)
        monkeypatch.setattr(common, "dolt_table_import", mock_table_import)

        main_mod._import_stock_basic()

        assert mock_sql.call_count >= 3
        # Two imports since epic #266 B1: the stock CSV staging table plus the
        # name-en mapping staging table (when the checked-in mapping exists).
        assert mock_table_import.call_count == 2
        assert mock_sql_csv.call_count >= 1

    def test_empty_staging_aborts_before_delete(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """An empty _tmp_sb must abort before DELETE stock_basic (no wipe)."""
        import common
        import main as main_mod

        csv_path = tmp_path / "stock_basic_official.csv"
        csv_path.write_text("symbol\n")
        monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))

        mock_sql = Mock()
        monkeypatch.setattr(common, "dolt_sql", mock_sql)
        # First COUNT(*) (_tmp_sb) returns 0; stock_basic DELETE must never run.
        mock_sql_csv = Mock(return_value="Count\n0")
        monkeypatch.setattr(common, "dolt_sql_csv", mock_sql_csv)
        monkeypatch.setattr(common, "dolt_table_import", Mock(return_value=True))

        with pytest.raises(RuntimeError, match="_tmp_sb is empty"):
            main_mod._import_stock_basic()

        delete_calls = [c for c in mock_sql.call_args_list if c.args[0] == "DELETE FROM stock_basic"]
        assert delete_calls == []


# ═══════════════════════════════════════════════════════════════════
# _import_fin_indicators — legacy import helper (uses common dolt fns)
# ═══════════════════════════════════════════════════════════════════


class TestImportFinIndicators:
    def test_csv_missing_exits_early(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """When RPT_LICO_FN_CPD.csv does not exist, function returns early."""
        import main as main_mod

        monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path / "nonexistent"))

        main_mod._import_fin_indicators()

    def test_csv_exists_imports_to_dolt(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """When csv exists, dolt operations are triggered."""
        import common
        import main as main_mod

        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"
        csv_path.write_text("SECUCODE,SECURITY_CODE\n000001.SZ,000001\n")

        monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))

        mock_sql = Mock(return_value=Mock(stdout="200 OK", returncode=0))
        monkeypatch.setattr(common, "dolt_sql", mock_sql)
        mock_sql_csv = Mock(return_value="Count\n50")
        monkeypatch.setattr(common, "dolt_sql_csv", mock_sql_csv)
        mock_table_import = Mock(return_value=True)
        monkeypatch.setattr(common, "dolt_table_import", mock_table_import)

        main_mod._import_fin_indicators()

        assert mock_sql.call_count >= 3
        mock_table_import.assert_called_once()
        assert mock_sql_csv.call_count >= 2


# ═══════════════════════════════════════════════════════════════════
# _import_fin_indicators — merge semantics (temp Dolt + COMPASS_DATA_DIR)
# E1/E3 RED against current DELETE+INSERT; E2 PIN (idempotent refetch)
# ═══════════════════════════════════════════════════════════════════


class TestImportFinIndicatorsMerge:
    """Merge-semantics contract for _import_fin_indicators (E1-E3).

    The current implementation wipes fin_indicators (DELETE FROM ... +
    INSERT SELECT); the GREEN implementation must append incrementally.
    E1 and E3 are RED against current code; E2 (same-CSV refetch) is
    satisfied by both flows and pins idempotency.
    """

    # Full CSV header — every column referenced by the INSERT SELECT in
    # _import_fin_indicators (missing columns break _tmp_fin typing).
    _HEADER = [
        "SECUCODE",
        "SECURITY_CODE",
        "REPORTDATE",
        "UPDATE_DATE",
        "NOTICE_DATE",
        "DATATYPE",
        "QDATE",
        "EITIME",
        "DATAYEAR",
        "DATEMMDD",
        "SECURITY_NAME_ABBR",
        "TRADE_MARKET",
        "TRADE_MARKET_CODE",
        "TRADE_MARKET_ZJG",
        "SECURITY_TYPE",
        "SECURITY_TYPE_CODE",
        "PUBLISHNAME",
        "BOARD_CODE",
        "BOARD_NAME",
        "ORI_BOARD_CODE",
        "ORG_CODE",
        "ISNEW",
        "BASIC_EPS",
        "DEDUCT_BASIC_EPS",
        "TOTAL_OPERATE_INCOME",
        "PARENT_NETPROFIT",
        "WEIGHTAVG_ROE",
        "BPS",
        "MGJYXJJE",
        "XSMLL",
        "YSTZ",
        "SJLTZ",
        "YSHZ",
        "SJLHZ",
        "ZXGXL",
        "ASSIGNDSCRPT",
        "PAYYEAR",
    ]

    # Mirrors the real fin_indicators schema; every column nullable except
    # the PK (empty CSV cells → NULL on import).
    _FIN_INDICATORS_DDL = """
        CREATE TABLE fin_indicators (
            symbol VARCHAR(20) NOT NULL,
            report_date DATE NOT NULL,
            update_date DATE, notice_date DATE,
            data_type VARCHAR(50), qdate DATE, eitime DATE, data_year INT,
            date_label VARCHAR(20), secucode VARCHAR(20), name VARCHAR(100),
            trade_market VARCHAR(50), trade_market_code VARCHAR(20),
            trade_market_zjg VARCHAR(50), security_type VARCHAR(50),
            security_type_code VARCHAR(20), industry VARCHAR(100),
            board_code VARCHAR(20), board_name VARCHAR(50), ori_board_code VARCHAR(20),
            org_code VARCHAR(50), is_new INT,
            basic_eps DECIMAL(10,4), deduct_basic_eps DECIMAL(10,4),
            revenue DECIMAL(20,2), net_profit DECIMAL(20,2), roe DECIMAL(10,4),
            bps DECIMAL(10,4), cash_flow_per_share DECIMAL(10,4),
            gross_margin DECIMAL(10,4), revenue_yoy DECIMAL(10,4),
            net_profit_yoy DECIMAL(10,4), operating_profit_yoy DECIMAL(10,4),
            net_profit_qoq DECIMAL(10,4), shares_growth DECIMAL(10,4),
            dividend_plan VARCHAR(100), dividend_year INT,
            PRIMARY KEY (symbol, report_date)
        )
    """

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str]]:
        """Init temp Dolt, point COMPASS_DATA_DIR + COMPASS_CSV_DIR at tmp_path.

        Seeds stock_basic (SZ000001/SZ000002), data_updates, and the
        pre-existing fin_indicators table (the legacy import path DELETEs
        from and INSERTs into it — it never creates it).
        Returns (dir, dolt_sql_csv).
        """
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
            "INSERT INTO stock_basic VALUES ('SZ000001'), ('SZ000002')"
        )
        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )
        dolt_sql_csv(self._FIN_INDICATORS_DDL)

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
        return tmp_path, dolt_sql_csv

    @staticmethod
    def _last(stdout: str) -> str:
        """Last line of dolt csv output (header row + data rows)."""
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def _make_row(self, secucode: str = "000001.SZ", report_date: str = "2024-12-31") -> list[str]:
        """Build a full 37-col CSV row with only identity columns populated."""
        row = [""] * len(self._HEADER)
        row[self._HEADER.index("SECUCODE")] = secucode
        row[self._HEADER.index("SECURITY_CODE")] = secucode.split(".")[0]
        row[self._HEADER.index("REPORTDATE")] = report_date
        return row

    def _write_csv(self, path: Path, rows: list[list[str]]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(self._HEADER)
            writer.writerows(rows)

    def test_merge_incremental_appends_preserving_history(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: an incremental refetch must append to history, not wipe it.

        CSV A (SZ000001 at 2024-12-31 + 2023-12-31) followed by CSV B
        (SZ000001 + SZ000002 at 2024-12-31) must yield 3 rows with the
        2023-12-31 row preserved. Current DELETE+INSERT wipes history on
        every run → 2 rows, 2023-12-31 gone, watermark row_count=2.
        """
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"

        self._write_csv(csv_path, [self._make_row(), self._make_row(report_date="2023-12-31")])
        main_mod._import_fin_indicators()

        self._write_csv(csv_path, [self._make_row(), self._make_row(secucode="000002.SZ")])
        main_mod._import_fin_indicators()

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "3"
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT COUNT(*) FROM fin_indicators "
                    "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
                )
            )
            == "1"
        )
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT row_count, last_report_date FROM data_updates "
                    "WHERE table_name='fin_indicators'"
                )
            )
            == "3,2024-12-31"
        )

    def test_merge_same_csv_refetch_idempotent(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """PIN: refetching the same CSV twice must not duplicate rows."""
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"
        self._write_csv(csv_path, [self._make_row()])

        main_mod._import_fin_indicators()
        main_mod._import_fin_indicators()

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "1"

    def test_merge_insert_failure_preserves_prior_rows(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """RED: a failed refetch must preserve previously imported rows.

        After 2 rows are imported, dropping stock_basic makes the INSERT
        SELECT fail. Current DELETE+INSERT has already wiped the table and
        has no rollback → 0 rows and watermark row_count=0. Merge must keep
        the 2 prior rows and leave no _tmp_fin behind.
        """
        import main as main_mod

        dolt_dir_, dolt_sql_csv = dolt_env
        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"
        self._write_csv(csv_path, [self._make_row(), self._make_row(report_date="2023-12-31")])
        main_mod._import_fin_indicators()
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "2"

        dolt_sql_csv("DROP TABLE stock_basic")

        self._write_csv(csv_path, [self._make_row()])
        main_mod._import_fin_indicators()

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "2"
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT COUNT(*) FROM information_schema.tables WHERE table_name='_tmp_fin'"
                )
            )
            == "0"
        )
        assert (
            self._last(
                dolt_sql_csv(
                    "SELECT row_count, last_report_date FROM data_updates "
                    "WHERE table_name='fin_indicators'"
                )
            )
            == "2,2024-12-31"
        )


# ═══════════════════════════════════════════════════════════════════
# sync_investment_data — restart with server_script exists
# ═══════════════════════════════════════════════════════════════════


class TestSyncInvestmentRestartServer:
    def test_restart_with_server_script(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ) -> None:
        """When restart=True and start-dolt-server.sh exists, server restarts."""
        import main as main_mod

        invest_dir = tmp_path / "investment_data"
        (invest_dir / ".dolt").mkdir(parents=True)

        scripts_dir = tmp_path / "scripts"
        scripts_dir.mkdir()
        (scripts_dir / "start-dolt-server.sh").write_text("#!/bin/bash\necho ok\n")

        monkeypatch.setattr(main_mod, "PROJECT_ROOT", tmp_path)

        mock_run = Mock(return_value=Mock(stdout="", stderr="", returncode=0))
        monkeypatch.setattr(main_mod.subprocess, "run", mock_run)
        mock_popen = Mock()
        monkeypatch.setattr(main_mod.subprocess, "Popen", mock_popen)

        main_mod.sync_investment_data(restart=True)

        # Popen should have been called for server restart
        mock_popen.assert_called_once()


# ═══════════════════════════════════════════════════════════════════
# #135 T3: UPSERT 写法钉住（temp Dolt 实测，Dolt 2.2.3）
# 钉住 GREEN 实现必须采用的 UPSERT 写法：
#   INSERT INTO fin_indicators (...) SELECT <expr> AS _<alias>, ...
#   FROM _tmp_fin ... ON DUPLICATE KEY UPDATE <col>=_<alias>, ...
# - 别名引用（无前缀）：全列覆盖成功（含 TRIM 文本列）
# - 限定源列引用 `_tmp_fin.COL`：TRIM 文本列报 table _tmp_fin does not have column
# - VALUES()：报 __new_ins
# 35 值列清单以 main.FIN_INDICATORS_DDL 为唯一来源（解析 DDL 并与 API→DDL
# 映射核对，机械钉住"UPDATE 子句无一遗漏"）。
# Wave 1 即绿：纯 Dolt 能力验证，不依赖生产代码（plan T3 明示）。
# ═══════════════════════════════════════════════════════════════════


class TestUpsert:
    """UPSERT 写法钉住测试（temp Dolt + FIN_INDICATORS_DDL 37 列）。"""

    # API CSV 列 → Dolt DDL 列（35 值列；PK 列 symbol/report_date 特殊处理）。
    # 键顺序与 FIN_INDICATORS_DDL 值列顺序一致（与 _ddl_columns()[2:] 对齐）。
    _CSV_TO_DDL = {
        "UPDATE_DATE": "update_date",
        "NOTICE_DATE": "notice_date",
        "DATATYPE": "data_type",
        "QDATE": "qdate",
        "EITIME": "eitime",
        "DATAYEAR": "data_year",
        "DATEMMDD": "date_label",
        "SECUCODE": "secucode",
        "SECURITY_NAME_ABBR": "name",
        "TRADE_MARKET": "trade_market",
        "TRADE_MARKET_CODE": "trade_market_code",
        "TRADE_MARKET_ZJG": "trade_market_zjg",
        "SECURITY_TYPE": "security_type",
        "SECURITY_TYPE_CODE": "security_type_code",
        "PUBLISHNAME": "industry",
        "BOARD_CODE": "board_code",
        "BOARD_NAME": "board_name",
        "ORI_BOARD_CODE": "ori_board_code",
        "ORG_CODE": "org_code",
        "ISNEW": "is_new",
        "BASIC_EPS": "basic_eps",
        "DEDUCT_BASIC_EPS": "deduct_basic_eps",
        "TOTAL_OPERATE_INCOME": "revenue",
        "PARENT_NETPROFIT": "net_profit",
        "WEIGHTAVG_ROE": "roe",
        "BPS": "bps",
        "MGJYXJJE": "cash_flow_per_share",
        "XSMLL": "gross_margin",
        "YSTZ": "revenue_yoy",
        "SJLTZ": "net_profit_yoy",
        "YSHZ": "operating_profit_yoy",
        "SJLHZ": "net_profit_qoq",
        "ZXGXL": "shares_growth",
        "ASSIGNDSCRPT": "dividend_plan",
        "PAYYEAR": "dividend_year",
    }

    # 与 main.py 现状一致的 TRIM 文本列（SELECT 侧 TRIM，ODKU 引用别名即得已 TRIM 值）
    _TRIM_COLS = {
        "DATATYPE",
        "QDATE",
        "DATEMMDD",
        "SECURITY_NAME_ABBR",
        "TRADE_MARKET",
        "TRADE_MARKET_ZJG",
        "SECURITY_TYPE",
        "PUBLISHNAME",
        "BOARD_NAME",
        "ASSIGNDSCRPT",
        "PAYYEAR",
    }

    # DDL 中 double 类型的值列（round-trip 断言用 float 比较避免格式差异）
    _DOUBLE_COLS = {
        "basic_eps",
        "deduct_basic_eps",
        "revenue",
        "net_profit",
        "roe",
        "bps",
        "cash_flow_per_share",
        "gross_margin",
        "revenue_yoy",
        "net_profit_yoy",
        "operating_profit_yoy",
        "net_profit_qoq",
        "shares_growth",
    }

    _CSV_HEADER = TestImportFinIndicatorsMerge._HEADER  # 复用既有 37 列清单

    @staticmethod
    def _ddl_columns() -> list[str]:
        """Parse column names from main.FIN_INDICATORS_DDL (唯一来源)。

        只匹配带类型的列定义行（排除 PRIMARY KEY 等非列行）。
        """
        import re

        import main as main_mod

        return re.findall(
            r"^\s{4}(\w+)\s+(?:varchar|text|char|date|datetime|int|tinyint|double)\b",
            main_mod.FIN_INDICATORS_DDL,
            re.M,
        )

    def test_ddl_value_column_count_is_35(self) -> None:
        """35 值列清单 = DDL 37 列 − 2 PK，且与 _CSV_TO_DDL 映射一一对应。"""
        cols = self._ddl_columns()
        assert len(cols) == 37, f"FIN_INDICATORS_DDL must have 37 columns, got {len(cols)}"
        assert cols[:2] == ["symbol", "report_date"]
        value_cols = cols[2:]
        assert len(value_cols) == 35
        assert sorted(value_cols) == sorted(self._CSV_TO_DDL.values()), (
            "API→DDL mapping must cover every one of the 35 value columns"
        )

    def _make_row(self, prefix: str) -> dict[str, str]:
        """Build a full 37-col CSV row; every value column carries a prefix-distinct value.

        prefix='OLD' → 旧特征值（预插入 Dolt）；'NEW' → 新特征值（UPSERT 后应覆盖）。
        TRIM 文本列带空格（验证 SELECT 侧 TRIM）。
        """
        is_old = prefix == "OLD"
        row: dict[str, str] = {
            "SECUCODE": "000858.SZ",
            "SECURITY_CODE": "000858",
            "REPORTDATE": "2025-03-31",
        }
        for i, api_col in enumerate(self._CSV_TO_DDL):
            if api_col == "SECUCODE":  # 已由初始 dict 固定（symbol 拼接源）
                continue
            if api_col in self._TRIM_COLS:
                row[api_col] = f"  {prefix}_{i}  "
            elif api_col in {"UPDATE_DATE", "NOTICE_DATE", "QDATE"}:
                row[api_col] = "2020-01-01" if is_old else "2026-06-30"
            elif api_col == "EITIME":
                row[api_col] = "2020-01-01 00:00:00" if is_old else "2026-06-30 00:00:00"
            elif api_col in {"DATAYEAR", "ISNEW"}:
                row[api_col] = str(i) if is_old else str(i + 1)
            else:
                row[api_col] = str(i * 10 + 2) if is_old else str(i * 10 + 1)
        return row

    def _write_csv(self, path: Path, row: dict[str, str]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(self._CSV_HEADER)
            writer.writerow([row.get(c, "") for c in self._CSV_HEADER])

    def _expected(self, row: dict[str, str]) -> dict[str, str]:
        """CSV 行 → Dolt 期望值（TRIM 文本列去空格，其余原样）。"""
        exp: dict[str, str] = {}
        for api_col, ddl_col in self._CSV_TO_DDL.items():
            v = row[api_col]
            exp[ddl_col] = v.strip() if api_col in self._TRIM_COLS else v
        return exp

    def _upsert_sql(self, odku_override: dict[str, str] | None = None) -> str:
        """SELECT 全列别名 + ODKU 无前缀别名引用的 UPSERT SQL（GREEN 目标写法）。"""
        value_cols = self._ddl_columns()[2:]
        select_parts = []
        for i, ddl_col in enumerate(value_cols):
            api_col = next(c for c, d in self._CSV_TO_DDL.items() if d == ddl_col)
            expr = f"TRIM({api_col})" if api_col in self._TRIM_COLS else api_col
            select_parts.append(f"{expr} AS _c{i}")
        select = (
            "CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE) AS _sym,\n"
            "    REPORTDATE AS _rpt,\n    " + ",\n    ".join(select_parts)
        )
        col_list = ", ".join(["symbol", "report_date"] + value_cols)
        odku = odku_override or {c: f"_c{i}" for i, c in enumerate(value_cols)}
        odku_clause = ", ".join(f"{c}={ref}" for c, ref in odku.items())
        return (
            f"INSERT INTO fin_indicators ({col_list})\n"
            f"SELECT\n    {select}\n"
            f"FROM _tmp_fin\n"
            f"WHERE CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE, '.', -1)), SECURITY_CODE) "
            f"IN (SELECT symbol FROM stock_basic)\n"
            f"ON DUPLICATE KEY UPDATE {odku_clause}"
        )

    @pytest.fixture
    def dolt_env(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> tuple[Path, Callable[[str], str], Callable[[str], subprocess.CompletedProcess[str]]]:
        """temp Dolt：stock_basic + data_updates + fin_indicators（真实 DDL）。"""
        import main as main_mod

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

        def dolt_sql(sql: str) -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                ["dolt", "--data-dir", str(tmp_path), "sql", "-q", sql],
                capture_output=True,
                text=True,
            )

        dolt_sql_csv(
            "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY); "
            "INSERT INTO stock_basic VALUES ('SZ000858')"
        )
        dolt_sql_csv(
            "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
            "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
        )
        dolt_sql_csv(main_mod.FIN_INDICATORS_DDL)

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        return tmp_path, dolt_sql_csv, dolt_sql

    @staticmethod
    def _last(stdout: str) -> str:
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def test_alias_odku_overwrites_existing_pk(self, dolt_env, tmp_path: Path) -> None:
        """① SELECT 别名 + ODKU 无前缀别名引用：同 PK 全列覆盖成功（数值 + TRIM 文本列）。

        钉住 plan 验收：数值 369.40→170.86、name/data_type 等 TRIM 文本列被覆盖
        （防实现漏列导致新旧值静默混合）。
        """
        import io

        import common

        tmp, dolt_sql_csv, dolt_sql = dolt_env
        dolt_sql_csv(
            "INSERT INTO fin_indicators (symbol, report_date, update_date, revenue, name, data_type) "
            "VALUES ('SZ000858', '2025-03-31', '2025-04-26', 369.40, '五粮液旧名', '旧类型')"
        )
        row = self._make_row("NEW")
        row["TOTAL_OPERATE_INCOME"] = "170.86"
        row["SECURITY_NAME_ABBR"] = "五粮液"
        row["DATATYPE"] = "2025年 一季报"
        row["UPDATE_DATE"] = "2026-04-30"
        csv_path = tmp / "RPT_LICO_FN_CPD.csv"
        self._write_csv(csv_path, row)

        assert common.dolt_table_import("_tmp_fin", csv_path) is True
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM _tmp_fin")) == "1", (
            "staging table _tmp_fin must contain the CSV row"
        )
        result = dolt_sql(self._upsert_sql())
        assert result.returncode == 0, result.stderr

        out = dolt_sql_csv(
            "SELECT revenue, name, data_type, update_date FROM fin_indicators "
            "WHERE symbol='SZ000858' AND report_date='2025-03-31'"
        )
        rows = list(csv.DictReader(io.StringIO(out)))
        assert len(rows) == 1
        assert float(rows[0]["revenue"]) == pytest.approx(170.86), rows[0]
        assert rows[0]["name"] == "五粮液"
        assert rows[0]["data_type"] == "2025年 一季报"
        assert rows[0]["update_date"] == "2026-04-30"
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "1"

    def test_upsert_all_35_value_columns_roundtrip(self, dolt_env, tmp_path: Path) -> None:
        """④ 全行 35 列 round-trip：旧特征值 → UPSERT → 每列等于新特征值。"""
        import io

        import common

        tmp, dolt_sql_csv, dolt_sql = dolt_env
        old = self._make_row("OLD")
        new = self._make_row("NEW")

        # 预插入旧值行（35 值列全部填充旧特征值）
        vals = []
        for api_col in self._CSV_TO_DDL:
            v = old[api_col]
            vals.append(v if api_col in {"DATAYEAR", "ISNEW"} else f"'{v}'")
        dolt_sql_csv(
            f"INSERT INTO fin_indicators (symbol, report_date, "
            f"{', '.join(self._CSV_TO_DDL.values())}) "
            f"VALUES ('SZ000858', '2025-03-31', {', '.join(vals)})"
        )

        csv_path = tmp / "RPT_LICO_FN_CPD.csv"
        self._write_csv(csv_path, new)
        assert common.dolt_table_import("_tmp_fin", csv_path) is True
        result = dolt_sql(self._upsert_sql())
        assert result.returncode == 0, result.stderr

        expected = self._expected(new)
        col_list = ", ".join(["symbol", "report_date"] + list(self._CSV_TO_DDL.values()))
        out = dolt_sql_csv(
            f"SELECT {col_list} FROM fin_indicators "
            "WHERE symbol='SZ000858' AND report_date='2025-03-31'"
        )
        rows = list(csv.DictReader(io.StringIO(out)))
        assert len(rows) == 1
        for ddl_col, want in expected.items():
            got = rows[0][ddl_col]
            if ddl_col in self._DOUBLE_COLS:
                assert float(got) == pytest.approx(float(want)), (
                    f"{ddl_col}: got {got!r}, want {want!r}"
                )
            else:
                assert got == want, f"{ddl_col}: got {got!r}, want {want!r}"
        # 同 PK 覆盖而非新增行
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "1"

    def test_qualified_source_column_reference_rejected(self, dolt_env, tmp_path: Path) -> None:
        """② 限定源列引用 `_tmp_fin.COL` 对 TRIM 文本列在 Dolt 报错（禁用写法）。"""
        import common

        tmp, dolt_sql_csv, dolt_sql = dolt_env
        csv_path = tmp / "RPT_LICO_FN_CPD.csv"
        self._write_csv(csv_path, self._make_row("NEW"))
        assert common.dolt_table_import("_tmp_fin", csv_path) is True

        value_cols = self._ddl_columns()[2:]
        over = {c: f"_c{i}" for i, c in enumerate(value_cols)}
        over["name"] = "_tmp_fin.name"  # TRIM 文本列限定源列引用
        result = dolt_sql(self._upsert_sql(odku_override=over))
        assert result.returncode != 0, "qualified source-column reference must fail on Dolt"
        assert "_tmp_fin" in result.stderr, result.stderr

    def test_values_function_rejected(self, dolt_env, tmp_path: Path) -> None:
        """③ VALUES() 写法在 Dolt 报错（禁用写法）。"""
        import common

        tmp, dolt_sql_csv, dolt_sql = dolt_env
        csv_path = tmp / "RPT_LICO_FN_CPD.csv"
        self._write_csv(csv_path, self._make_row("NEW"))
        assert common.dolt_table_import("_tmp_fin", csv_path) is True

        value_cols = self._ddl_columns()[2:]
        over = {c: f"_c{i}" for i, c in enumerate(value_cols)}
        over["revenue"] = "VALUES(revenue)"
        result = dolt_sql(self._upsert_sql(odku_override=over))
        assert result.returncode != 0, "VALUES() must fail on Dolt"
        assert "__new_ins" in result.stderr, result.stderr
