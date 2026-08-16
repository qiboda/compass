"""Adversarial tests for issue #283 — main.py concept member phase-out (plan B2).

Plan commitment (B2 + D4): ``main.py`` drops the ``concept_member`` CLI entry
(fetch/import/progress choices) and ``fetch_concept_member.py`` +
``test_concept_member.py`` are deleted. No residual concept-member path may
survive the refactor.

RED/GREEN: these assert the *removal* — RED today because the current main.py
still wires ``concept_member`` in dispatch_fetch / dispatch_import / do_sync /
argparse choices. GREEN once B2 removes the branch.

NOT duplicated: test_index_main_cli.py owns the index_daily CLI-surface wiring;
here we own the concept_member *presence* (the thing that must disappear). The
happy-path collector behavior is in test_concept_member.py (deleted by B2).
"""

import inspect
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


class TestMainConceptMemberRemoval:
    """The concept_member entry must be gone from every CLI surface."""

    def test_fetch_choice_rejects_concept_member(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """RED: `main.py fetch concept_member` must no longer be a valid choice
        — the CLI must never reach dispatch_fetch for it. Today the choice is
        accepted and dispatch_fetch('concept_member') IS called → this fails."""
        from unittest.mock import Mock

        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "fetch", "concept_member"])
        # Patch dispatch_fetch so a (buggy) acceptance can't reach the real
        # fetch_concept_member network call.
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_fetch", mock_dispatch)

        with pytest.raises(SystemExit):
            main_mod.main()  # argparse rejects the choice
        mock_dispatch.assert_not_called()

    def test_import_choice_rejects_concept_member(self, monkeypatch: pytest.MonkeyPatch) -> None:
        from unittest.mock import Mock

        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "import", "concept_member"])
        mock_dispatch = Mock()
        monkeypatch.setattr(main_mod, "dispatch_import", mock_dispatch)

        with pytest.raises(SystemExit):
            main_mod.main()  # argparse rejects the choice
        mock_dispatch.assert_not_called()

    def test_choice_lists_contain_no_concept_member(self) -> None:
        """Source-level: neither the fetch nor import choices list may mention
        concept_member (rename-robust — survives argparse restructuring)."""
        import main as main_mod

        src = inspect.getsource(main_mod.main)
        lines = [ln for ln in src.splitlines() if "choices=[" in ln]
        assert lines, "argparse choices list must be present"
        for ln in lines:
            assert "concept_member" not in ln, (
                f"a CLI choices list still offers concept_member: {ln.strip()!r}"
            )

    def test_dispatch_fetch_has_no_concept_member_branch(self) -> None:
        import main as main_mod

        src = inspect.getsource(main_mod.dispatch_fetch)
        assert "concept_member" not in src, (
            "dispatch_fetch must not import/run fetch_concept_member"
        )

    def test_dispatch_import_has_no_concept_member_branch(self) -> None:
        import main as main_mod

        src = inspect.getsource(main_mod.dispatch_import)
        assert "concept_member" not in src, (
            "dispatch_import must not import fetch_concept_member"
        )

    def test_do_sync_has_no_concept_member_step(self) -> None:
        import main as main_mod

        src = inspect.getsource(main_mod.do_sync)
        assert "concept_member" not in src, "do_sync must not fetch/import concept_member"

    def test_module_never_imports_fetch_concept_member(self) -> None:
        import main as main_mod

        src = inspect.getsource(main_mod)
        assert "fetch_concept_member" not in src, (
            "main.py must hold no import of the to-be-deleted fetch_concept_member"
        )
