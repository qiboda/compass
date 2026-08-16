"""Requirement acceptance tests for issue #283 — main.py concept_member 入口移除.

Contract under test (issue #283 D4 + plan B2, main.py):

  C1. ``fetch concept_member`` / ``import concept_member`` are no longer routed
      targets in main.py. ``dispatch_fetch("concept_member")`` must NOT invoke
      ``fetch_concept_member.run()``, and ``dispatch_import("concept_member")``
      must NOT invoke ``fetch_concept_member.import_to_dolt()``. The collector
      module ``fetch_concept_member`` is deleted (D4).

STATUS: RED — today main.py still routes ``concept_member`` (lines 410-413 /
529-532) and ``fetch_concept_member.py`` still exists, so:
- the ``import fetch_concept_member`` tests fail with ImportError only AFTER the
  removal; right now they pass the import and fail the "not dispatched"
  assertion (logic RED against the current routing).
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


class TestMainConceptMemberRemoval:
    """C1: concept_member must be dropped from the main.py dispatch router."""



    def test_concept_member_module_deleted(self) -> None:
        """C1 RED: after D4, fetch_concept_member.py must be gone — importing it
        fails with ModuleNotFoundError. Today the file still exists, so the
        import succeeds and this assertion fails (logic RED)."""
        with pytest.raises(ModuleNotFoundError):
            __import__("fetch_concept_member")
