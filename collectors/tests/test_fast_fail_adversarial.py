"""Adversarial tests for issue #277 — consecutive-failure fast fail.

Contract under attack (issue #277 acceptance, fetch_index_daily.run() +
common.py):
  F1. run() keeps a consecutive-failure counter; **5 consecutive failed
      targets** (request-failure OR empty klines) immediately terminates,
      without requesting the remaining targets.
  F2. Failure = _get_json returns None (all host×attempt exhausted) OR empty
      ``klines``. Both count.
  F3. On termination keep previously fetched data (write CSV, resumable) then
      raise RuntimeError whose message hints 连续 / 疑似反爬/接口故障.
  F4. A success resets the counter; exactly 5 consecutive failures terminate
      (4 do NOT). A code-mismatch official (skip, neither success nor failure)
      — the declared contract says only a *success* resets, so a skip must NOT
      reset the counter (否则漏杀) and must NOT count toward it either
      (否则误杀).
  F5. common.EM_MIN_INTERVAL == 2.0, and default-constructed Throttle() binds
      to it.

Attack dimensions (all realizable via public run() + the conftest stub):
  1. boundary:   success-reset then a streak crossing the board→official loop
                 boundary terminates exactly at the 5th failure (never earlier,
                 and never late).
  2. error path: a streak whose length is the sum of *mixed* failure kinds
                 (request-fail AND empty-klines) across the boundary terminates
                 on the 5th.
  3. resource:   request log proves zero requests after the 5th failure is
                 satisfied (a later would-be-success target is untouched).
  4. resource:   Progress json status after the abort is "failed", never a
                 leftover "running".
  5. error path: partial success + 5 streak → the fetched daily CSV rows are
                 actually on disk (real write_csv, unpatched).
  6. boundary:   zero success + 5 streak → no half-written / header-only daily
                 CSV is left behind.
  7. boundary:   code-mismatch skip neither resets the counter (漏杀) nor
                 counts it (误杀).

STATUS: RED — the current implementation prints FAILED/empty and continues
(hardcoded -- COMPASS never aborts early), and EM_MIN_INTERVAL == 0.5.
"""

import asyncio
import csv
import inspect
import json
import sys
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

from fetch_index_daily import OFFICIAL_INDICES  # noqa: E402

_CLIST = "clist/get"

_OK = "ok"
_FAIL = "fail"       # HTTP 500 → _get_json None → a failure
_EMPTY = "empty"     # 200 but empty klines → counted as a failure
_MISMATCH = "mismatch"  # non-empty klines but echo wrong code → skip (neither fail nor success)


def _kline_row(day: str = "2026-07-31", close: float = 3000.0) -> str:
    return (
        f"{day},{close - 1},{close},{close + 1},{close - 2},"
        f"120000000,50000000000,1.5,0.5,1.0,0.5"
    )


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _board(code: str, name: str) -> dict[str, object]:
    return {"f12": code, "f14": name}


def _pin_today(monkeypatch: pytest.MonkeyPatch) -> None:
    """Pin _today and no-op asyncio.sleep (no real waits / retry backoffs)."""
    monkeypatch.setattr("fetch_index_daily._today", lambda: date(2026, 8, 2))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


def _env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    """No Dolt dir → last_report_date empty → full fetch; CSV dir isolated by conftest."""
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))


def _make_stub(make_stub_session, boards: list[dict], kline: dict[str, str],
               default: str = _OK):
    """Stub whose get() dispatches per secid; ``official_reqs`` captures each
    official secid actually asked for (in order), boards via ``tracked``.

    ``kline`` maps ``secid → {_OK,_FAIL,_EMPTY,_MISMATCH}``.

    Returns (stub, tracked_secids, official_reqs).
    """
    tracked: list[str] = []
    official_reqs: list[str] = []
    stub = make_stub_session()

    async def _get(url, params=None, headers=None):
        if "kline/get" in url:
            secid = (params or {}).get("secid", "")
            if secid:
                tracked.append(secid)
                if not secid.startswith("90."):
                    official_reqs.append(secid)
            kind = kline.get(secid, default)
            bare = secid.rsplit(".", 1)[-1]
            if kind == _FAIL:
                return StubResponse(status_code=500, json_data={})
            if kind == _EMPTY:
                return StubResponse(json_data=_kline_payload(bare, []))
            if kind == _MISMATCH:
                # Echo a *different* official bare code → run() must skip it as
                # neither failure nor success (no kline rows, no reset).
                return StubResponse(json_data=_kline_payload(f"99{bare}", [_kline_row()]))
            return StubResponse(json_data=_kline_payload(bare, [_kline_row()]))
        if _CLIST in url:
            return StubResponse(json_data=_clist_payload(boards))
        return StubResponse(status_code=200, json_data={})

    stub.get = _get  # type: ignore[method-assign]
    return stub, tracked, official_reqs


def _read_rows(path: Path) -> list[dict[str, str]]:
    with open(path, newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


# ── 1 + 4: boundary — success resets, streak crosses loops, terminates at 5 ──


class TestBoundaryLoops:
    async def test_streak_crosses_board_to_official_boundary_terminates_at_5(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: board B1 fails, B2 succeeds (counter resets to 0), B3,B4 fail
        (counter=2), boards end. Officials O1,O2 fail (counter=3,4), O3 fails
        (counter=5) → terminate ON O3. Assert a later official is never
        requested and the message hints the failure streak."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [
            _board("BK2101", "B1"), _board("BK2102", "B2"),
            _board("BK2103", "B3"), _board("BK2104", "B4"),
        ]
        kline = {
            "90.BK2101": _FAIL,
            "90.BK2102": _OK,
            "90.BK2103": _FAIL,
            "90.BK2104": _FAIL,
        }
        # Officials 1-3 fail consecutively.
        o1, o2, o3 = OFFICIAL_INDICES[0]["secid"], OFFICIAL_INDICES[1]["secid"], OFFICIAL_INDICES[2]["secid"]
        kline.update({o1: _FAIL, o2: _FAIL, o3: _FAIL})
        # Official #4 would succeed if ever asked — must NOT be.
        o4 = OFFICIAL_INDICES[3]["secid"]

        stub, tracked, official_reqs = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), f"message must mention 连续, got {str(e.value)!r}"
        assert "疑似反爬" in str(e.value) or "接口" in str(e.value), (
            f"message must hint anti-bot/interface fault, got {str(e.value)!r}"
        )
        # Terminated on O3 (the 5th consecutive failure) — O4 never reached.
        assert o3 in official_reqs, f"the 5th failure ({o3}) must have been requested"
        assert o4 not in official_reqs, (
            f"counter must terminate at the 5th failure ({o3}); {o4} must be untouched, "
            f"got requested officials {official_reqs}"
        )

    async def test_skips_do_not_reset_and_do_not_false_kill(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (漏杀/误杀 boundary): B1-B4 fail (counter=4), official O1 is a
        code-mismatch SKIP (non-empty klines, wrong echo code). A skip is not a
        success (must NOT reset → else the final failure is lost = 漏杀) and not
        a failure either (so counter stays 4, not 5 → no premature abort at the
        skip). O2 then fails → counter=5 → terminate on O2. This pins BOTH
        directions: the mismatch alone must never trigger, and must never mask
        the streak."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [_board(f"BK22{i:02d}", f"B{i}") for i in range(1, 5)]  # BK2201..BK2204
        kline = {f"90.BK22{i:02d}": _FAIL for i in range(1, 5)}
        o1, o2 = OFFICIAL_INDICES[0]["secid"], OFFICIAL_INDICES[1]["secid"]
        kline[o1] = _MISMATCH   # skip — neither fail nor success
        kline[o2] = _FAIL       # 5th true failure → terminates here
        o3 = OFFICIAL_INDICES[2]["secid"]

        stub, tracked, official_reqs = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "termination must happen at the true 5th failure (O2)"
        # The mismatch skip precedes O2 — if the code falsely counted it as a
        # failure it would have aborted at O1 (before asserting 连续 with O2 in
        # the streak). O2 must be reached because it IS the 5th.
        assert o2 in official_reqs, (
            f"the 4 fails + mismatch skip must leave the counter at 4, so {o2} "
            f"(the 5th true failure) is still requested; got {official_reqs}"
        )
        assert o3 not in official_reqs, (
            "after the 5th true failure (O2) no later official may be fetched"
        )
        # The mismatch skip must not have left daily rows behind.
        daily = tmp_path / "index_daily.csv"
        assert not daily.exists() or daily.stat().st_size == 0, (
            "a code-mismatch skip produces no data rows"
        )


# ── 2 + 3: mixed failure streak + resource (no requests after abort) ─────────


class TestMixedStreakAndResource:
    async def test_mixed_request_and_empty_streak_across_boundary_stops_after_5(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED+resource: a consecutive streak mixing 500-request-failures and
        empty-klines failures (B1 E, B2 F, boards end; O1 E, O2 F, O3 E → #5)
        terminates on O3. Total kline requests must stop exactly at the 5th
        failure: the count of kline requests equals the targets actually
        touched, and a later success target is never hit (resource exhaustion:
        fast fail exists to stop wasting requests on a melting API)."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [_board("BK2301", "B1"), _board("BK2302", "B2")]
        kline = {"90.BK2301": _EMPTY, "90.BK2302": _FAIL}
        o1, o2, o3 = (OFFICIAL_INDICES[i]["secid"] for i in (0, 1, 2))
        kline.update({o1: _EMPTY, o2: _FAIL, o3: _EMPTY})
        o4 = OFFICIAL_INDICES[3]["secid"]

        stub, tracked, official_reqs = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "mixed fail+empty streak must fast-fail"
        # Resource-exhaustion guard: no request past the 5th failure.
        assert o4 not in official_reqs, "no requests may be issued after the 5th failure"
        # The 5 failures themselves were each requested exactly once (no phantom
        # requests to already-aborted targets beyond the streak).
        expected_requested = {"90.BK2301", "90.BK2302", o1, o2, o3}
        assert expected_requested <= set(tracked), (
            f"each streaked target must be requested once; got {tracked}"
        )

    async def test_progress_status_is_failed_after_abort(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (state leak): when run() aborts on 5 consecutive failures, the
        Progress file must report status "failed", never a stale "running".
        Current code never raises the fast-fail RuntimeError, so the abort
        path (and its progress teardown) is simply missing."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [_board("BK2401", "B1"), _board("BK2402", "B2")]
        kline = {f"90.BK24{i:02d}": _FAIL for i in range(1, 3)}
        o1, o2, o3 = (OFFICIAL_INDICES[i]["secid"] for i in (0, 1, 2))
        kline.update({o1: _FAIL, o2: _FAIL, o3: _FAIL})

        stub, _, _ = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        prog = tmp_path / "index_daily.progress.json"
        assert prog.exists(), "Progress must write its json on abort"
        state = json.loads(prog.read_text(encoding="utf-8"))
        assert state.get("status") == "failed", (
            f"after abort Progress must be 'failed', got {state.get('status')!r}"
        )
        assert state.get("error"), "abort must record the failure reason in progress error"

    async def test_progress_is_not_left_running_on_zero_success_abort(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (state leak on all-fail): even with no prior success the abort
        must tear the Progress down to 'failed'."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [_board("BK2501", "B1")]
        kline = {"90.BK2501": _FAIL}
        o1, o2, o3, o4, o5 = (OFFICIAL_INDICES[i]["secid"] for i in range(5))
        kline.update({o1: _FAIL, o2: _FAIL, o3: _FAIL, o4: _FAIL, o5: _FAIL})

        stub, _, _ = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        prog = tmp_path / "index_daily.progress.json"
        assert prog.exists(), "Progress json must exist to prove non-running state"
        state = json.loads(prog.read_text(encoding="utf-8"))
        assert state.get("status") == "failed", (
            "an aborting run must leave Progress 'failed', never 'running'"
        )


# ── 5: partial success + streak → CSV actually preserves the fetched rows ────


class TestPartialSuccessCsv:
    async def test_partial_success_rows_are_on_disk_before_abort(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: one board succeeds, then 5 consecutive 500-failures. Terminated
        daily CSV (real write_csv, NOT patched) must contain the successful
        board's rows — keep-resumable contract. Current code never aborts, so
        this data is only reachable via the (missing) abort path."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        ok = _board("BK2601", "赢家板")
        boards = [ok, *[_board(f"BK26{i:02d}", f"F{i}") for i in range(2, 7)]]  # BK2602..BK2606
        kline = {"90.BK2601": _OK, **{f"90.BK26{i:02d}": _FAIL for i in range(2, 7)}}

        stub, _, _ = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), "abort must still be a 连续... RuntimeError"
        daily = tmp_path / "index_daily.csv"
        assert daily.exists(), "daily CSV must exist despite the abort (keep-resumable)"
        symbols = {r["symbol"] for r in _read_rows(daily)}
        assert "BK2601" in symbols, (
            "rows fetched before the streak must be persisted on disk; got symbols {symbols}"
        )
        # The 5 failed boards contributed no rows.
        for i in range(2, 7):
            assert f"BK26{i:02d}" not in symbols, "failed targets must not leak rows"


# ── 6: zero success + streak → no half-written daily CSV ─────────────────────


class TestZeroSuccessNoHalfCsv:
    async def test_zero_success_abort_leaves_no_daily_csv(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (boundary): zero success anywhere + 5 consecutive failures aborts
        without leaving a half-written/header-only index_daily.csv. Empty daily
        CSV would poison a later resumable merge (INSERT IGNORE no-ops then a
        Dolt import writes an empty file). index_basic.md is allowed to exist at
        the implementer's discretion, but the DAILY file must not be a stub."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        boards = [_board("BK2701", "B1")]
        kline = {"90.BK2701": _FAIL}
        o1, o2, o3, o4, o5 = (OFFICIAL_INDICES[i]["secid"] for i in range(5))
        kline.update({o1: _FAIL, o2: _FAIL, o3: _FAIL, o4: _FAIL, o5: _FAIL})

        stub, _, _ = _make_stub(make_stub_session, boards, kline)

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        # The abort must be the 连续 fast-fail, not the unrelated "No index data"
        # end-of-run error of the unmodified code (which only fires after
        # exhausting all 30 officials).
        assert "连续" in str(e.value), (
            f"zero-success abort must be the fast-fail error, got {str(e.value)!r}"
        )
        daily = tmp_path / "index_daily.csv"
        assert not daily.exists() or daily.stat().st_size == 0, (
            "no half-written/header-only daily CSV may be left on a zero-success abort"
        )


# ── F5: rate-limit constant widened and bound to Throttle default ────────────


class TestRateLimitConstant:
    def test_em_min_interval_is_two_seconds(self) -> None:
        import common  # noqa: E402

        assert common.EM_MIN_INTERVAL == 2.0, (
            f"common.EM_MIN_INTERVAL must be 2.0 per issue #277, got {common.EM_MIN_INTERVAL!r}"
        )

    def test_throttle_default_binds_to_the_constant(self) -> None:
        """The default Throttle() must be constructed with EM_MIN_INTERVAL —
        i.e. when the constant becomes 2.0, a default-constructed throttle
        rate-limits at 2.0 (widening the global EastMoney pace)."""
        from common import EM_MIN_INTERVAL, Throttle  # noqa: E402

        sig = inspect.signature(Throttle.__init__)
        default = sig.parameters["min_interval"].default
        assert default is EM_MIN_INTERVAL, (
            f"Throttle() default must reference EM_MIN_INTERVAL, got {default!r}"
        )
        # Behavioral binding: the effective interval equals the widened constant.
        assert default == 2.0, f"default throttle interval must be 2.0, got {default!r}"
