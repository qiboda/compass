"""Requirement-acceptance tests for the C1 CLI routing surface (epic #255,
plan T1) — the *precise routing* half of the functional contract.

The adversarial suite (test_index_main_cli.py) proves argparse accepts
``index_daily``, that ``asyncio.run`` fires, and that ``do_sync`` gains an
11th asyncio step. This file proves the *routing target*: the exact
``fetch_index_daily`` module functions must be invoked — ``do_sync()`` must
call ``fetch_index_daily.run()`` + ``import_to_dolt()`` (plan: "main.py
do_sync() 第 11 步"), ``dispatch_fetch("index_daily")`` must call
``fetch_index_daily.run()``, and ``dispatch_import("index_daily")`` must call
``fetch_index_daily.import_to_dolt()``.

Unlike the C1 collector tests, these run TODAY and fail on assertion (RED):
the current ``do_sync`` has no 11th step and neither dispatch branch mentions
``index_daily``, so the fake module's mocks are never called. The fake module
is injected via ``sys.modules`` so the tests do NOT require
``fetch_index_daily.py`` to exist — once the collector lands, ``main.py``'s
own ``import fetch_index_daily`` picks the fake up in tests and the routing
assertions pass.
"""

from __future__ import annotations

import sys
import types
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


def _install_fake_index_module(monkeypatch: pytest.MonkeyPatch) -> tuple[Mock, Mock]:
    """Inject a fake ``fetch_index_daily`` module with recorded run/import mocks.

    Returns (run_mock, import_to_dolt_mock). The module only exists inside
    ``sys.modules``; the real collector file is not required.
    """
    mod = types.ModuleType("fetch_index_daily")
    run_mock = Mock()
    import_mock = Mock()
    mod.run = run_mock
    mod.import_to_dolt = import_mock
    monkeypatch.setitem(sys.modules, "fetch_index_daily", mod)
    return run_mock, import_mock


class TestDoSyncStep11Routing:
    """do_sync() step 11 must run the index collector fetch + import."""

    @staticmethod
    def _patch_existing_steps(monkeypatch: pytest.MonkeyPatch) -> Mock:
        """Neutralize the 10 existing do_sync steps (mirror test_index_main_cli.py)."""
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

    def test_do_sync_invokes_fetch_index_daily_run_and_import(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: today do_sync never touches fetch_index_daily — neither the
        async fetch nor the Dolt import is invoked. GREEN: step 11 calls
        both."""
        import main as main_mod

        self._patch_existing_steps(monkeypatch)
        run_mock, import_mock = _install_fake_index_module(monkeypatch)

        main_mod.do_sync()

        run_mock.assert_called_once()
        import_mock.assert_called_once()


class TestDispatchRouting:
    """dispatch_fetch / dispatch_import must route index_daily precisely."""

    def test_dispatch_fetch_routes_to_fetch_index_daily_run(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: dispatch_fetch('index_daily') currently has no branch — the
        collector's run() is never called (the call silently no-ops)."""
        import main as main_mod

        run_mock, _ = _install_fake_index_module(monkeypatch)
        monkeypatch.setattr(main_mod.asyncio, "run", Mock())

        main_mod.dispatch_fetch("index_daily")

        run_mock.assert_called_once()

    def test_dispatch_import_routes_to_fetch_index_daily_import(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """RED: dispatch_import('index_daily') currently has no branch — the
        collector's import_to_dolt() is never called."""
        import main as main_mod

        _, import_mock = _install_fake_index_module(monkeypatch)

        main_mod.dispatch_import("index_daily")

        import_mock.assert_called_once()

    def test_dispatch_fetch_other_targets_unaffected(
        self, monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Guard: adding the index_daily branch must not disturb existing
        routing (main_flow still reaches fetch_main_flow.run)."""
        import fetch_main_flow as fmf
        import main as main_mod

        run_mock, _ = _install_fake_index_module(monkeypatch)
        mock_main_flow_run = Mock()
        monkeypatch.setattr(fmf, "run", mock_main_flow_run)
        monkeypatch.setattr(main_mod.asyncio, "run", Mock())

        main_mod.dispatch_fetch("main_flow")

        mock_main_flow_run.assert_called_once()
        run_mock.assert_not_called()
