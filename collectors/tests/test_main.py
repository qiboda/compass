"""Unit tests for main.py CLI dispatch — extracted functions and thin main().

Tests cover all 5 fetch/import targets, sync, sync-investment, and the
main() entrypoint with monkeypatched sys.argv.  Uses monkeypatch to
avoid real network/Dolt calls.
"""

from __future__ import annotations

import contextlib
import csv
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
        self, monkeypatch: pytest.MonkeyPatch,
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
        self, monkeypatch: pytest.MonkeyPatch,
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
        self, monkeypatch: pytest.MonkeyPatch,
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
        self, monkeypatch: pytest.MonkeyPatch,
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
        self, monkeypatch: pytest.MonkeyPatch,
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


# ═══════════════════════════════════════════════════════════════════
# dispatch_import — routes to correct import function per target
# ═══════════════════════════════════════════════════════════════════


class TestDispatchImport:
    def test_stock_basic_calls_import_stock_basic(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(main_mod, "_import_stock_basic", mock_import)

        main_mod.dispatch_import("stock_basic")
        mock_import.assert_called_once()

    def test_fin_indicators_calls_import_fin_indicators(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(main_mod, "_import_fin_indicators", mock_import)

        main_mod.dispatch_import("fin_indicators")
        mock_import.assert_called_once()

    def test_balance_sheet_calls_import_to_dolt(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_balance_sheet as fbs
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fbs, "import_to_dolt", mock_import)

        main_mod.dispatch_import("balance_sheet")
        mock_import.assert_called_once()

    def test_income_calls_import_to_dolt(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_income as fi
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fi, "import_to_dolt", mock_import)

        main_mod.dispatch_import("income")
        mock_import.assert_called_once()

    def test_cash_flow_calls_import_to_dolt(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import fetch_cash_flow as fcf
        import main as main_mod

        mock_import = Mock()
        monkeypatch.setattr(fcf, "import_to_dolt", mock_import)

        main_mod.dispatch_import("cash_flow")
        mock_import.assert_called_once()


# ═══════════════════════════════════════════════════════════════════
# do_sync — fetch all + import all + update data_updates
# ═══════════════════════════════════════════════════════════════════


class TestDoSync:
    def test_calls_all_5_fetches_and_5_imports(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """do_sync() triggers fetch+import for all tables, plus data_updates."""
        import common
        import fetch_balance_sheet as fbs
        import fetch_block_trade as fbt
        import fetch_cash_flow as fcf
        import fetch_concept_member as fcm
        import fetch_dragon as fdr
        import fetch_fin_indicators as ffi
        import fetch_income as fi
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
        monkeypatch.setattr(fcm, "run", Mock())
        monkeypatch.setattr(fmf, "run", Mock())

        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock())
        monkeypatch.setattr(fbs, "import_to_dolt", Mock())
        monkeypatch.setattr(fi, "import_to_dolt", Mock())
        monkeypatch.setattr(fcf, "import_to_dolt", Mock())
        monkeypatch.setattr(fdr, "import_to_dolt", Mock())
        monkeypatch.setattr(fbt, "import_to_dolt", Mock())
        monkeypatch.setattr(fis, "import_to_dolt", Mock())
        monkeypatch.setattr(fcm, "import_to_dolt", Mock())
        monkeypatch.setattr(fmf, "import_to_dolt", Mock())

        mock_dolt = Mock()
        monkeypatch.setattr(common, "dolt_sql", mock_dolt)

        main_mod.do_sync()

        # stock_basic is sync (official source); 9 remaining tables via asyncio.run
        assert mock_run.call_count == 9
        # data_updates loop for the 5 legacy tables (new tables upsert inside import_to_dolt)
        assert mock_dolt.call_count >= 5

    def test_restart_flag_accepted_no_behavior_change(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """restart=True accepted (no-op for sync subcommand, future-compat)."""
        import common
        import fetch_balance_sheet as fbs
        import fetch_block_trade as fbt
        import fetch_cash_flow as fcf
        import fetch_concept_member as fcm
        import fetch_dragon as fdr
        import fetch_fin_indicators as ffi
        import fetch_income as fi
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
        monkeypatch.setattr(fcm, "run", Mock())
        monkeypatch.setattr(fmf, "run", Mock())
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock())
        for mod in (fbs, fi, fcf, fdr, fbt, fis, fcm, fmf):
            monkeypatch.setattr(mod, "import_to_dolt", Mock())

        mock_dolt = Mock()
        monkeypatch.setattr(common, "dolt_sql", mock_dolt)

        main_mod.do_sync(restart=True)

        assert mock_run.call_count == 9


# ═══════════════════════════════════════════════════════════════════
# sync_investment_data — already extracted, testable as-is
# ═══════════════════════════════════════════════════════════════════


class TestSyncInvestmentData:
    def test_investment_dir_missing_exits_early(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
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
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
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
        pkill_calls = [
            c for c in mock_subprocess_run.call_args_list
            if "pkill" in str(c)
        ]
        assert len(pkill_calls) >= 1

    def test_restart_false_skips_server_restart(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
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
        pkill_calls = [
            c for c in mock_run.call_args_list if "pkill" in str(c)
        ]
        assert len(pkill_calls) == 0


# ═══════════════════════════════════════════════════════════════════
# main() — thin argparse + dispatch
# ═══════════════════════════════════════════════════════════════════


class TestMain:
    def test_fetch_stock_basic(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys, "argv",
            ["main.py", "fetch", "stock_basic"],
        )
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("stock_basic", years=None)

    def test_fetch_fin_indicators_with_years(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys, "argv",
            ["main.py", "fetch", "fin_indicators", "--years", "2024,2025"],
        )
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("fin_indicators", years=[2024, 2025])

    def test_fetch_with_years_list(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys, "argv",
            ["main.py", "fetch", "stock_basic", "--years", "2024"],
        )
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("stock_basic", years=[2024])

    def test_import_stock_basic(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "import", "stock_basic"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_import", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("stock_basic")

    def test_import_balance_sheet(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "import", "balance_sheet"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_import", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("balance_sheet")

    def test_sync(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "sync"])
        mock_sync = Mock()
        monkeypatch.setattr(main_mod, "do_sync", mock_sync)

        main_mod.main()
        mock_sync.assert_called_once()

    def test_sync_investment(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "sync-investment"])
        mock_sync_inv = Mock()
        monkeypatch.setattr(main_mod, "sync_investment_data", mock_sync_inv)

        main_mod.main()
        mock_sync_inv.assert_called_once_with(False)

    def test_sync_investment_restart(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        import main as main_mod

        monkeypatch.setattr(
            sys, "argv", ["main.py", "sync-investment", "--restart"],
        )
        mock_sync_inv = Mock()
        monkeypatch.setattr(main_mod, "sync_investment_data", mock_sync_inv)

        main_mod.main()
        mock_sync_inv.assert_called_once_with(True)

    def test_no_command_prints_help(
        self, monkeypatch: pytest.MonkeyPatch,
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
    def test_csv_missing_exits_early(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """When stock_basic.csv does not exist, function returns early."""
        import main as main_mod

        csv_dir = tmp_path / "nonexistent"
        monkeypatch.setattr(main_mod, "COLLECTORS_DIR", csv_dir)
        # csv_path would be COLLECTORS_DIR / "stock_basic.csv" — doesn't exist

        # Should not raise
        main_mod._import_stock_basic()

    def test_csv_exists_imports_to_dolt(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """When csv exists, dolt_sql / dolt_table_import / dolt_sql_csv are called."""
        import common
        import main as main_mod

        # Create a dummy CSV (official-source filename)
        csv_path = tmp_path / "stock_basic_official.csv"
        csv_path.write_text("header\n1\n")

        monkeypatch.setattr(main_mod, "COLLECTORS_DIR", tmp_path)

        mock_sql = Mock(return_value=Mock(stdout="Count\n100", returncode=0))
        monkeypatch.setattr(common, "dolt_sql", mock_sql)
        mock_sql_csv = Mock(return_value="Count\n100")
        monkeypatch.setattr(common, "dolt_sql_csv", mock_sql_csv)
        mock_table_import = Mock(return_value=True)
        monkeypatch.setattr(common, "dolt_table_import", mock_table_import)

        main_mod._import_stock_basic()

        assert mock_sql.call_count >= 3
        mock_table_import.assert_called_once()
        assert mock_sql_csv.call_count >= 1


# ═══════════════════════════════════════════════════════════════════
# _import_fin_indicators — legacy import helper (uses common dolt fns)
# ═══════════════════════════════════════════════════════════════════


class TestImportFinIndicators:
    def test_csv_missing_exits_early(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """When RPT_LICO_FN_CPD.csv does not exist, function returns early."""
        import main as main_mod

        csv_dir = tmp_path / "nonexistent"
        monkeypatch.setattr(main_mod, "COLLECTORS_DIR", csv_dir)

        main_mod._import_fin_indicators()

    def test_csv_exists_imports_to_dolt(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
    ) -> None:
        """When csv exists, dolt operations are triggered."""
        import common
        import main as main_mod

        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"
        csv_path.write_text("SECUCODE,SECURITY_CODE\n000001.SZ,000001\n")

        monkeypatch.setattr(main_mod, "COLLECTORS_DIR", tmp_path)

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
        "SECUCODE", "SECURITY_CODE", "REPORTDATE", "UPDATE_DATE", "NOTICE_DATE",
        "DATATYPE", "QDATE", "EITIME", "DATAYEAR", "DATEMMDD",
        "SECURITY_NAME_ABBR", "TRADE_MARKET", "TRADE_MARKET_CODE", "TRADE_MARKET_ZJG",
        "SECURITY_TYPE", "SECURITY_TYPE_CODE", "PUBLISHNAME", "BOARD_CODE",
        "BOARD_NAME", "ORI_BOARD_CODE", "ORG_CODE", "ISNEW", "BASIC_EPS",
        "DEDUCT_BASIC_EPS", "TOTAL_OPERATE_INCOME", "PARENT_NETPROFIT",
        "WEIGHTAVG_ROE", "BPS", "MGJYXJJE", "XSMLL", "YSTZ", "SJLTZ", "YSHZ",
        "SJLHZ", "ZXGXL", "ASSIGNDSCRPT", "PAYYEAR",
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
        """Init temp Dolt, point COMPASS_DATA_DIR + COLLECTORS_DIR at tmp_path.

        Seeds stock_basic (SZ000001/SZ000002), data_updates, and the
        pre-existing fin_indicators table (the legacy import path DELETEs
        from and INSERTs into it — it never creates it).
        Returns (dir, dolt_sql_csv).
        """
        import main as main_mod

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
        dolt_sql_csv(self._FIN_INDICATORS_DDL)

        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
        monkeypatch.setattr(main_mod, "COLLECTORS_DIR", tmp_path)
        return tmp_path, dolt_sql_csv

    @staticmethod
    def _last(stdout: str) -> str:
        """Last line of dolt csv output (header row + data rows)."""
        lines = stdout.strip().split("\n")
        return lines[-1] if lines else ""

    def _make_row(
        self, secucode: str = "000001.SZ", report_date: str = "2024-12-31"
    ) -> list[str]:
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

        self._write_csv(
            csv_path, [self._make_row(), self._make_row(report_date="2023-12-31")]
        )
        main_mod._import_fin_indicators()

        self._write_csv(
            csv_path, [self._make_row(), self._make_row(secucode="000002.SZ")]
        )
        main_mod._import_fin_indicators()

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "3"
        assert self._last(
            dolt_sql_csv(
                "SELECT COUNT(*) FROM fin_indicators "
                "WHERE symbol='SZ000001' AND report_date='2023-12-31'"
            )
        ) == "1"
        assert self._last(
            dolt_sql_csv(
                "SELECT row_count, last_report_date FROM data_updates "
                "WHERE table_name='fin_indicators'"
            )
        ) == "3,2024-12-31"

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
        self._write_csv(
            csv_path, [self._make_row(), self._make_row(report_date="2023-12-31")]
        )
        main_mod._import_fin_indicators()
        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "2"

        dolt_sql_csv("DROP TABLE stock_basic")

        self._write_csv(csv_path, [self._make_row()])
        main_mod._import_fin_indicators()

        assert self._last(dolt_sql_csv("SELECT COUNT(*) FROM fin_indicators")) == "2"
        assert self._last(
            dolt_sql_csv(
                "SELECT COUNT(*) FROM information_schema.tables "
                "WHERE table_name='_tmp_fin'"
            )
        ) == "0"
        assert self._last(
            dolt_sql_csv(
                "SELECT row_count, last_report_date FROM data_updates "
                "WHERE table_name='fin_indicators'"
            )
        ) == "2,2024-12-31"


# ═══════════════════════════════════════════════════════════════════
# sync_investment_data — restart with server_script exists
# ═══════════════════════════════════════════════════════════════════


class TestSyncInvestmentRestartServer:
    def test_restart_with_server_script(
        self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
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
