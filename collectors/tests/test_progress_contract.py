"""Requirement acceptance tests for issue #267 — the ``progress`` CLI contract.

BDD scenarios derive from the issue's declared acceptance criteria:

1. ``main.py progress`` (no target) lists every ``*.progress.json`` under
   csv_dir() in human-readable form.
2. ``main.py progress <target>`` shows a single collector's status; ``--json``
   emits the raw JSON.
3. The 6 wired-in collector run-paths (main_flow / block_trade / index_daily /
   institution_survey / concept_member / dragon) each write ``*.progress.json``;
   success = ``completed``, exception = ``failed`` with error.
4. Progress files are atomically updated so queries see no torn read.
5. **``progress`` target choices must contain ONLY the 6 wired-in collectors**
   (must not accept un-wired names such as ``income`` / ``stock_basic`` /
   ``fin_indicators`` / ``balance_sheet`` / ``cash_flow``). This is the known
   gap in the current implementation — choices are hard-coded to 11 names
   including 5 un-wired append-type collectors. The tests below that require
   argparse rejection are RED until the choices list is trimmed.
6. ``.dsh/kb/user/cli.md`` documentation (handled by the main agent).

Run:  uv run pytest tests/test_progress_contract.py -q
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# The 6 collectors wired into progress tracking (write *.progress.json).
CONNECTED = [
    "main_flow",
    "block_trade",
    "index_daily",
    "institution_survey",
    "concept_member",
    "dragon",
]
# The 5 un-wired, append-type collectors currently in the hard-coded choices.
UNWIRED = [
    "stock_basic",
    "fin_indicators",
    "balance_sheet",
    "income",
    "cash_flow",
]


# ═══════════════════════════════════════════════════════════════════
# Acceptance 5 — argparse must reject un-wired `progress` targets
# (RED: current choices still hard-code the 11-name list incl. income)
# ═══════════════════════════════════════════════════════════════════


class TestProgressChoicesRejectUnwired:
    """AC-5: `progress` target choices contain only the 6 wired-in collectors.

    An un-wired collector name such as ``income`` must be rejected by argparse
    (SystemExit code 2, "invalid choice"), NOT reach ``dispatch_progress`` and
    fall through to its "No progress file" SystemExit(1). The current
    implementation fails this — ``income`` is still an accepted choice.
    """

    @pytest.mark.parametrize("name", UNWIRED)
    def test_unwired_target_rejected_by_argparse(self, monkeypatch, name: str) -> None:
        """AC-5: ``progress <unwired>`` exits 2 with an 'invalid choice' error."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "progress", name])
        # argparse rejects before any dispatch runs.
        monkeypatch.setattr(main_mod, "dispatch_progress", Mock())
        with pytest.raises(SystemExit) as exc:
            main_mod.main()
        assert exc.value.code == 2, (
            f"{name} should be rejected by argparse (code 2), got code {exc.value.code}"
        )

    @pytest.mark.parametrize("name", CONNECTED)
    def test_connected_target_accepted(self, monkeypatch, name: str) -> None:
        """AC-5 complement: every wired-in collector is still an accepted choice."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "progress", name])
        mock_progress = Mock()
        monkeypatch.setattr(main_mod, "dispatch_progress", mock_progress)
        # Should not exit 2; should dispatch to the given target.
        main_mod.main()
        mock_progress.assert_called_once_with(name, as_json=False)


# ═══════════════════════════════════════════════════════════════════
# Acceptance 1+2 — query contract via main() argparse wiring
# ═══════════════════════════════════════════════════════════════════


class TestProgressCli:
    """AC-1: `progress` with no target shows all; AC-2: `--json` raw output."""

    def test_no_target_dispatches_all(self, monkeypatch) -> None:
        """AC-1: ``progress`` (no target) dispatches ``dispatch_progress(None)``."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "progress"])
        mock_progress = Mock()
        monkeypatch.setattr(main_mod, "dispatch_progress", mock_progress)
        main_mod.main()
        mock_progress.assert_called_once_with(None, as_json=False)

    def test_target_json_flag(self, monkeypatch) -> None:
        """AC-2: ``progress <target> --json`` dispatches with as_json=True."""
        import main as main_mod

        monkeypatch.setattr(sys, "argv", ["main.py", "progress", "dragon", "--json"])
        mock_progress = Mock()
        monkeypatch.setattr(main_mod, "dispatch_progress", mock_progress)
        main_mod.main()
        mock_progress.assert_called_once_with("dragon", as_json=True)


# ═══════════════════════════════════════════════════════════════════
# Acceptance 1/2/4 — dispatch_progress behaviour (per-collector + all)
# ═══════════════════════════════════════════════════════════════════


class TestProgressDispatch:
    """AC-1/2/4: dispatch_progress query behaviour (mirrors TestDispatchProgress
    in test_main.py, re-stated here for issue #267 acceptance evidence)."""

    @staticmethod
    def _write(tmp_path: Path, name: str, status: str = "running") -> None:
        (tmp_path / f"{name}.progress.json").write_text(
            json.dumps({"name": name, "status": status, "percent": 50.0, "message": "x"}),
            encoding="utf-8",
        )

    def test_target_human_readable_has_status_and_percent(
        self, monkeypatch, tmp_path: Path, capsys
    ) -> None:
        """AC-1/2: per-target human output includes status and percent."""
        import main as main_mod

        self._write(tmp_path, "block_trade", "completed")
        main_mod.dispatch_progress("block_trade")
        out = capsys.readouterr().out
        assert "[block_trade] completed" in out
        assert "50.0%" in out

    def test_target_json_output(self, monkeypatch, tmp_path: Path, capsys) -> None:
        """AC-2: ``--json`` per-target emits parseable raw JSON."""
        import main as main_mod

        self._write(tmp_path, "dragon", "running")
        main_mod.dispatch_progress("dragon", as_json=True)
        data = json.loads(capsys.readouterr().out)
        assert data["name"] == "dragon"
        assert data["status"] == "running"

    def test_all_lists_every_progress_file(self, monkeypatch, tmp_path: Path, capsys) -> None:
        """AC-1: `progress` with no target lists every collector file."""
        import main as main_mod

        self._write(tmp_path, "block_trade", "completed")
        self._write(tmp_path, "main_flow", "running")
        main_mod.dispatch_progress()
        out = capsys.readouterr().out
        assert "block_trade" in out
        assert "main_flow" in out

    def test_missing_target_exits(self, monkeypatch, tmp_path: Path, capsys) -> None:
        """AC-2 error path: target with no progress file raises SystemExit."""
        import main as main_mod

        with pytest.raises(SystemExit):
            main_mod.dispatch_progress("missing")

    def test_no_files_prints_stderr(self, monkeypatch, tmp_path: Path, capsys) -> None:
        """AC-1 empty path: no progress files prints a stderr hint."""
        import main as main_mod

        main_mod.dispatch_progress()
        captured = capsys.readouterr()
        assert "No fetch progress files found." in captured.err

    def test_corrupt_progress_file_skipped(self, monkeypatch, tmp_path: Path, capsys) -> None:
        """AC-4 (query side): a tear/corrupt file is skipped, not a crash."""
        import main as main_mod

        (tmp_path / "bad.progress.json").write_text("{not json", encoding="utf-8")
        main_mod.dispatch_progress()
        assert "bad" not in capsys.readouterr().out


# ═══════════════════════════════════════════════════════════════════
# Acceptance 3+4 — each wired-in collector writes a progress file whose
# status is completed (success) or failed-with-error (exception), and the
# write is atomic (query never reads a torn/partial JSON).
# ═══════════════════════════════════════════════════════════════════


class TestProgressWiredCollectors:
    """AC-3/4: the 6 wired-in collectors write valid, atomically-consistent
    progress JSON through common.Progress (success=completed, exception=failed)."""

    @pytest.mark.parametrize("name", CONNECTED)
    def test_connected_collector_write_atomic_progress(
        self, monkeypatch, tmp_path: Path, name: str
    ) -> None:
        """AC-3/4: Progress(name) writes a parseable file; success -> completed."""
        from common import Progress

        p = Progress(name=name, total_items=3, output_csv=f"{name}.csv")
        p.finish()
        path = tmp_path / f"{name}.progress.json"
        assert path.exists()
        data = json.loads(path.read_text(encoding="utf-8"))
        assert data["status"] == "completed"
        assert data["name"] == name

    @pytest.mark.parametrize("name", CONNECTED)
    def test_connected_collector_failure_writes_error(
        self, monkeypatch, tmp_path: Path, name: str
    ) -> None:
        """AC-3: a collector exception records status=failed with an error."""
        from common import Progress

        p = Progress(name=name)
        p.fail("boom")
        data = json.loads((tmp_path / f"{name}.progress.json").read_text(encoding="utf-8"))
        assert data["status"] == "failed"
        assert data["error"] == "boom"


# ═══════════════════════════════════════════════════════════════════
# Acceptance 4 — atomic write / no torn read
# ═══════════════════════════════════════════════════════════════════


class TestProgressAtomicity:
    """AC-4: Progress file is written atomically — read_progress either sees a
    complete previous state or the complete new state, never a truncated one."""

    def test_read_progress_returns_none_on_partial_file(self, monkeypatch, tmp_path: Path) -> None:
        """AC-4: a partial/corrupt file reads as missing (no torn read)."""
        from common import read_progress

        (tmp_path / "x.progress.json").write_text('{"status": "runn', encoding="utf-8")
        assert read_progress("x") is None

    def test_progress_write_is_atomic(self, monkeypatch, tmp_path: Path) -> None:
        """AC-4: updating progress does not leave a truncated file in place."""
        from common import Progress

        p = Progress(name="dragon")
        # Simulate several intermediate updates; a final complete record
        # (success) must be fully readable afterwards without any torn read.
        p.update(fetched_rows=10, message="fetching")
        p.finish(fetched_rows=10)
        data = json.loads((tmp_path / "dragon.progress.json").read_text(encoding="utf-8"))
        assert data["status"] == "completed"
        assert data["fetched_rows"] == 10
