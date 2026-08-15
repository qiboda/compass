"""Adversarial tests for issue #283 — industry source switch to THS 90 + concept phase-out.

What the plan commits (plan B2/B4, D1/D4, acceptance #1/#4 — interfaces either
existing or their behavior-changed contracts):
- ``fetch_board_list`` (EastMoney push2 clist board discovery) is REMOVED: no
  EastMoney concept/industry board may be discovered anymore. The only industry
  source left is the THS 881xxx list.
- ``fetch_index_daily.py`` must hold NO residual concept logic (module text must
  be free of ``concept`` / ``clist`` / ``fetch_board_list`` references).
- The THS yearly kline parser REUSES the existing ``_kline_records`` 7-field
  mapping — its dirty-data tolerances and future-date drop are shared behavior
  that must survive the industry refactor unchanged.

Attack dimensions exercised here:
  D1 boundary+residual — concept/`clist`/`fetch_board_list` gone from the module
                        (contract grep; RED now, the references exist today).
  D2 boundary+residual — even if a board-list function remains (rename-proof),
                        no returned tuple may carry ``index_type == 'concept'``
                        or an EastMoney BK-symbol shape (RED now: today's
                        implementation discovers both).
  D3 error-path         — ``_kline_records`` tolerates a dirty 7-field row set
                        (short rows / non-numeric cells / '-' / empty) without
                        crashing and never fabricates a numeric cell.
  D4 boundary           — ``_kline_records`` drops future-dated rows shared with
                        the THS path (reused behavior locked).
  D5 resource           — THS fast-fail: the ``_bump_failure`` counter mechanism
                        (issue #277) is DRY-reused by the THS segment, not
                        shadowed by a second ad-hoc counter. (Runs against the
                        existing helper today; the THS fetch loop itself is the
                        DEFERRED interface — see DEFERRED section at file end.)

RED/GREEN STATE:
- D1/D2 are RED against the current code (concept/clist/fetch_board_list are
  still present). GREEN once B2 removes the EastMoney discovery + concept refs.
- D3/D4 lock existing ``_kline_records`` behavior shared with the THS path and
  must stay GREEN (the parser is not being changed by #283 — only re-used).
- D5 locks the existing ``_bump_failure`` helper; the THS *loop* that consumes
  it is DEFERRED (no interface yet).

NOT duplicated: happy-path THS parse / 90-unique list / tencent fallback are
owned by test_ths_industry_requirement.py and test_tencent_fallback_*.py.
"""

import asyncio
import inspect
import sys
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import fetch_index_daily as mod  # noqa: E402
from fetch_index_daily import _bump_failure, _kline_records  # noqa: E402

CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _pin_today(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("fetch_index_daily._today", lambda: date(2026, 8, 2))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


# ── D1: module must be free of residual concept logic (contract grep) ─────────


class TestModuleConceptRemovalGrep:
    """plan D4 / acceptance 2: the collector module must hold NO residual
    concept logic. This is the coarsest, rename-proof residual check — any
    surviving EastMoney concept/industry discovery keeps the module's text
    dirty and must fail here (RED today)."""

    _FORBIDDEN = ("concept", "clist", "fetch_board_list", "concept_member")

    def test_module_has_no_concept_reference(self) -> None:
        src = inspect.getsource(mod)
        hits = [tok for tok in self._FORBIDDEN if tok in src]
        assert not hits, (
            f"fetch_index_daily.py must be free of residual concept logic after "
            f"B2 (issue #283 D4): found {sorted(set(hits))} in module source"
        )

    def test_source_label_is_not_eastmoney_clist(self) -> None:
        # SOURCE documents where bars come from; after #283 the daily bars must
        # be THS-industry + official, never EastMoney product/industry boards.
        assert "clist" not in (getattr(mod, "SOURCE", "") or "")


# ── D2: no function may yield a concept or EastMoney-BK board row ─────────────


class TestBoardDiscoveryReturnsNoConceptRows:
    """plan B2: fetch_board_list (EastMoney push2 clist discovery) is REMOVED.
    Functionally: after the change, no discoverable board row may carry
    index_type=='concept' or an EastMoney ``BK``+4-digit symbol shape. Today
    the (un-removed) implementation returns exactly those → RED.

    This test is written to survive the removal: if ``fetch_board_list`` is
    deleted, ``hasattr`` short-circuits and the test scans for *any* function
    in the module whose ``__name__`` hints a board list, failing if one still
    hands back concept/EastMoney symbols."""

    def test_no_function_yields_concept_or_eastmoney_board(self, make_stub_session) -> None:
        import asyncio as _aio

        from fetch_index_daily import Throttle  # noqa: E402

        candidates: list[str] = []
        for name in dir(mod):
            if "board" in name or "list" in name:
                fn = getattr(mod, name)
                if callable(fn) and getattr(fn, "__module__", None) == mod.__name__:
                    candidates.append(name)

        fail_reasons: list[str] = []
        for cand in candidates:
            fn = getattr(mod, cand)
            # Only attempt sync-callable discovery functions taking
            # (session, throttle) — ADAPTIVE: if the function is async we still
            # need a session stub; we use a minimal one below.
            stub = make_stub_session(
                canned_responses={
                    CLIST_URL: {"json_data": _clist_payload([
                        {"f12": "BK1169", "f14": "AI概念"},   # concept
                        {"f12": "BK0475", "f14": "半导体"},   # industry w/ BK code
                    ])}
                }
            )
            try:
                result = _aio.run(fn(stub, Throttle(min_interval=0)))
            except Exception as exc:  # pragma: no cover - adaptive guard
                fail_reasons.append(f"{cand}: call exploded ({exc!r})")
                continue
            for symbol, name, index_type in result or []:
                if index_type == "concept":
                    fail_reasons.append(
                        f"{cand} still returns a concept row ({symbol} {name})"
                    )
                if symbol.startswith("BK") and len(symbol) == 6:
                    fail_reasons.append(
                        f"{cand} still emits EastMoney-BK board symbol {symbol}"
                    )

        # No surviving board-discovery function may return concept / EastMoney
        #-BK rows. If B2 deleted fetch_board_list, candidates is empty → pass.
        assert not fail_reasons, "; ".join(fail_reasons)


# ── D3: _kline_records tolerates dirty 7-field klines (shared THS parser) ─────


class TestKlineRecordsDirty7Field:
    """Reused ``_kline_records`` behavior (plan D2: THS 7-field CSV reuses it).
    Attack: dirty rows must degrade, never crash and never fabricate numerics —
    a broken THS year page must not poison the daily rows or blow up run()."""

    def test_short_row_less_than_7_fields_skipped(self) -> None:
        # _KLINE_FIELDS contract: date,open,close,high,low,volume,amount (7).
        # Row 1 has only 5 values → <7 parts → skipped; row 2 has exactly 7
        # parts → kept; row 3 has trailing junk → kept (first 7 only).
        recs = _kline_records(
            "BK881101", "industry",
            [
                "2026-07-31,1,2,3,4",                  # 5 parts → skip
                "2026-08-01,1,2,3,4,5,6",              # 7 parts → keep
                "2026-07-30,1,2,3,4,5,6,7,8",          # 8 parts → keep
            ],
            date(2026, 8, 2),
        )
        dates = [r["trade_date"] for r in recs]
        assert "2026-07-31" not in dates, "a row with <7 comma-parts must be skipped"
        assert "2026-08-01" in dates, "a row with exactly 7 comma-parts survives"
        assert "2026-07-30" in dates, "a wider row still rounds to the first 7"

    def test_non_numeric_cells_do_not_crash_and_stay_empty(self) -> None:
        # 7-part rows in _KLINE_FIELDS order: date,open,close,high,low,volume,amount.
        # Row 2 carries 'abc' in open, 'xyz' in high, '' in low, '-' in amount —
        # every dirty cell must degrade to '' and never fabricate a value.
        rows = [
            "2026-07-31,100,105,106,99,1200000,5e9",     # clean
            "2026-07-30,abc,105,xyz,,1200000,-",          # dirty (open=abc, high=xyz, low='', amount='-')
        ]
        recs = _kline_records("BK881102", "industry", rows, date(2026, 8, 2))
        assert len(recs) == 2, "dirty numeric cells must not drop the row"
        dirty = [r for r in recs if r["trade_date"] == "2026-07-30"][0]
        # open='abc' → ''; close='105' → 105; high='xyz' → ''; low='' → '';
        # volume='1200000' → 1200000; amount='-' → ''.
        assert dirty["open"] == "", f"non-numeric open must degrade to '', got {dirty['open']!r}"
        assert dirty["close"] == 105.0, "the parsable close survives"
        assert dirty["high"] == "", f"non-numeric high must degrade to '', got {dirty['high']!r}"
        assert dirty["low"] == "", f"empty low must degrade to '', got {dirty['low']!r}"
        assert dirty["amount"] == "", f"'-' amount must degrade to '', got {dirty['amount']!r}"
        # The clean sibling keeps its numeric values.
        clean = [r for r in recs if r["trade_date"] == "2026-07-31"][0]
        assert clean["close"] == 105.0
        assert clean["volume"] == 1200000

    def test_row_with_extra_fields_keeps_first_7_only(self) -> None:
        # A THS page may carry extra trailing columns after amount; only the
        # first 7 (date + 6 numeric) are consumed as OHLCV/amount.
        rows = ["2026-07-31,100,105,106,99,1200000,5e9,1.5,0.5"]
        recs = _kline_records("BK881103", "industry", rows, date(2026, 8, 2))
        assert len(recs) == 1
        assert recs[0]["amount"] == 5e9, "the 7th field is amount, trailing cols ignored"


# ── D4: future-dated THS klines are dropped (reused _kline_records) ───────────


class TestKlineRecordsFutureDateDrop:
    """plan D2: rows dated after ``today`` are dropped before publish — shared
    with the THS yearly path. A THS year page glitched with a future date must
    not leak into the daily rows."""

    def test_future_dated_row_dropped_normal_history_kept(self) -> None:
        rows = ["2026-07-31,100,105,106,99,1,1", "2099-01-01,100,105,106,99,1,1"]
        recs = _kline_records("BK881104", "industry", rows, date(2026, 8, 2))
        dates = {r["trade_date"] for r in recs}
        assert "2099-01-01" not in dates, "future-dated row must be dropped"
        assert "2026-07-31" in dates

    def test_large_future_filename_style_year_row_also_dropped(self) -> None:
        # A THS page labelled for a far-future year (e.g. 2200.js) still parses
        # here; the future DATE guard is the line of defense, not the filename.
        rows = ["2200-12-31,100,105,106,99,1,1"]
        recs = _kline_records("BK881105", "industry", rows, date(2026, 8, 2))
        assert not recs, "any future-dated row, regardless of source year, is dropped"


# ── D5: THS segment reuses the #277 fast-fail counter (existing helper) ────────


class TestFastFailCounterReuse:
    """plan D9: the THS segment reuses the issue #277 consecutive-failure
    mechanism, not a second ad-hoc counter. We lock the EXISTING ``_bump_failure``
    helper so a THS loop can depend on it; the consuming THS loop is DEFERRED
    (interface TBD). These run against today's code and must stay GREEN — they
    prove the reusable unit B2 must wire into the THS loop."""

    def test_reaches_abort_only_at_threshold(self) -> None:
        count = 0
        for i in range(1, 5):  # 1..4
            count, reason = _bump_failure(count)
            assert reason is None, f"streak {i} must not abort yet"
        count, reason = _bump_failure(count)  # 5th
        assert reason is not None, "the 5th consecutive failure must abort"
        assert "连续" in reason

    def test_success_resets_the_counter(self) -> None:
        # A THS success mid-streak must reset the SAME counter the helper owns:
        # after reset the next 5 failures re-abort at their own 5th.
        count = 0
        count, _ = _bump_failure(count)   # 1
        count, _ = _bump_failure(count)   # 2
        count = 0                          # THS success resets (contract)
        for _ in range(4):
            count, reason = _bump_failure(count)
            assert reason is None
        count, reason = _bump_failure(count)
        assert reason is not None, "post-reset 5th failure must abort independently"


# ── DEFERRED interfaces (issue #283, not yet implemented — see report) ────────

# The following THS-specific contracts are NOT yet present and are therefore
# NOT asserted here (per the RED/DEFERRED two-stage protocol). They will be
# implemented on the first compilable THS interface commit, per the interface
# list in the delegation report:
#   * THS list fetch (q.10jqka.com.cn/thshy/ GBK → 90 unique 881xxx boards;
#     href missing/malformed / GBK decode failure / non-881xxx rejection).
#   * THS yearly kline pagination (d.10jqka.com.cn/v4/line/bk_881xxx/01/{year}.js,
#     year loop 2007→current; empty-year termination; year-boundary handling).
#   * run() as official + ths_industry two-segment pipeline (with run() writing
#     only THS-industry + official rows, never EastMoney product/concept).
#   * main.py concept_member entry removed.
# These will be added as ``TestThsListParsing`` / ``TestThsYearPagination`` /
# ``TestRunTwoSegment`` once the implementing commit lands.
