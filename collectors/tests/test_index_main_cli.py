"""Adversarial tests for the C1 index-data CLI surface (epic #255, plan T1).

Plan contract under attack (main.py):
- ``main.py fetch index_daily`` / ``main.py import index_daily`` must be valid
  CLI choices — plan: "CLI choices 含 index_daily" (main.py:496/503).
- ``dispatch_fetch`` / ``dispatch_import`` must route ``index_daily`` like the
  other 10 collectors — plan: "fetch/import CLI choices".
- ``do_sync()`` step 11 — plan: "main.py do_sync() 第 11 步" (after the
  existing 10 steps at main.py:400-484) + "data_updates 更新循环
  (main.py:474-483)".

These tests are RED against the current code: the CLI surface has no
``index_daily`` anywhere — ``fetch index_daily`` is rejected by argparse, the
dispatch functions silently no-op, and ``do_sync`` never reaches an 11th step.
Unlike ``test_index_daily.py`` (which targets the not-yet-existing
``fetch_index_daily.py`` module), every test here runs today and fails on
assertion, not on import.
"""

from __future__ import annotations

import inspect
import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


class TestMainCliIndexDaily:
    """argparse choices must include index_daily for fetch and import."""

    def test_fetch_cli_accepts_index_daily(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """RED: `main.py fetch index_daily` is currently rejected by argparse
        (SystemExit 2), so dispatch_fetch is never reached."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "fetch", "index_daily"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        # Current code: parser.error() → SystemExit before dispatch. GREEN:
        # the choice is accepted and dispatch_fetch called once.
        main_mod.main()
        mock_dispatch.assert_called_once_with("index_daily", years=None)

    def test_import_cli_accepts_index_daily(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """RED: `main.py import index_daily` is currently rejected by argparse."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "import", "index_daily"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_import", mock_dispatch)

        main_mod.main()
        mock_dispatch.assert_called_once_with("index_daily")

    def test_fetch_cli_rejects_unknown_choice_still(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """Guard: adding index_daily must not silently accept arbitrary typos."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "fetch", "index_daily_typo"])
        with pytest.raises(SystemExit):
            main_mod.main()


class TestDispatchFetchIndexDaily:
    """dispatch_fetch('index_daily') must run the async collector."""

    def test_dispatch_fetch_index_daily_routes_to_async_run(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: current dispatch_fetch has no index_daily branch — asyncio.run
        is never invoked (the call silently no-ops, which would mislead a user
        into believing data was fetched)."""
        import main as main_mod

        mock_run = Mock()
        monkeypatch.setattr(main_mod.asyncio, "run", mock_run)

        main_mod.dispatch_fetch("index_daily")

        mock_run.assert_called_once()

    def test_dispatch_fetch_source_mentions_index_daily(self) -> None:
        """Contract grep: the dispatch_fetch body must contain an index_daily
        branch (mirrors plan T8's grep acceptance style)."""
        import main as main_mod

        src = inspect.getsource(main_mod.dispatch_fetch)
        assert "index_daily" in src


class TestDispatchImportIndexDaily:
    """dispatch_import('index_daily') must import into Dolt."""

    def test_dispatch_import_source_mentions_index_daily(self) -> None:
        """Contract grep: dispatch_import must route index_daily to its import
        function (RED — currently absent)."""
        import main as main_mod

        src = inspect.getsource(main_mod.dispatch_import)
        assert "index_daily" in src


class TestDoSyncIndexDaily:
    """do_sync() must run index_daily as step 11 + update data_updates."""

    @staticmethod
    def _patch_all_fetches(monkeypatch: pytest.MonkeyPatch) -> Mock:
        """Neutralize the 10 existing do_sync steps (mirror test_main.py).

        Returns the patched ``asyncio.run`` mock so callers can count calls.
        """
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
        monkeypatch.setattr(main_mod, "_import_stock_basic", Mock())
        monkeypatch.setattr(main_mod, "_import_fin_indicators", Mock())
        for mod in (fbs, fi, fcf, fdr, fbt, fis, fmf):
            monkeypatch.setattr(mod, "import_to_dolt", Mock())

        return mock_run

    def test_do_sync_runs_index_daily_as_step_11(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: today do_sync runs exactly 9 asyncio fetches (stock_basic is
        sync). Plan T1 adds index_daily as step 11 → 10 asyncio fetches."""
        import main as main_mod

        mock_run = self._patch_all_fetches(monkeypatch)

        main_mod.do_sync()

        assert mock_run.call_count == 9, (
            "index_daily must be the 11th step (9 existing asyncio fetches + "
            f"index_daily); got {mock_run.call_count}"
        )

    def test_do_sync_updates_data_updates_for_index_daily(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: the data_updates maintenance loop (main.py:474-483) must also
        touch index_daily so freshness tracking covers the new table."""
        import common
        import main as main_mod

        self._patch_all_fetches(monkeypatch)
        mock_dolt = Mock()
        monkeypatch.setattr(common, "dolt_sql", mock_dolt)

        main_mod.do_sync()

        assert any(
            "index_daily" in str(call)
            for call in mock_dolt.call_args_list
        ), "data_updates loop must mention index_daily"
