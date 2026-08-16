"""Adversarial tests for issue #278 + #286 — Tencent fallback for official indices.

Complement (not a duplicate) of test_tencent_fallback_requirement.py. That
file owns the declared happy paths (mapping, basic pagination, basic
fallback+amount, basic fast-fail). This file ATTACKS the edge cases the plan
declares but does not nail down:
  A1. _tencent_code(secid): behavior on INVALID/inapplicable secids (prefix not
      1. / 0., no code, empty) must be explicit — raise ValueError OR return
      None — and must NEVER fabricate an sh/sz symbol with a garbage prefix.
      A wrong symbol would silently route an EastMoney secid to the wrong
      Tencent security.
  A2. _fetch_tencent_kline pagination boundedness: when the source keeps
      answering FULL (==2000) pages WITHOUT forward progress (same earliest
      date / resetting window every request) a naive ``while len==2000`` loop
      never sees a short page and spins forever — resource exhaustion. The
      helper must terminate after a bounded number of requests (a page cap or
      a no-progress guard), and also stay bounded on a healthy always-full
      stream.
  A3. Malformed Tencent payload degrade-to-failure, never a crash: missing
      ``data`` / missing ``data[code].day`` / ``day`` not a list / a day row
      with fewer than 6 fields. At run() level these are double-failures
      (EastMoney already failed), so they must count once and terminate, not
      blow up on a TypeError / KeyError.
  A4. Fallback semantics on EastMoney code-MISMATCH: a non-empty kline whose
      echoed code does not match the whitelisted code is a #277 SKIP (neither
      a failure nor a success) and must NOT trigger the Tencent fallback. Only
      an EastMoney failure or EMPTY list may fall back.
  A5. Fast-fail through the fallback: 5 consecutive double-fails (EastMoney +
      Tencent both fail) terminate AND stop issuing further Tencent requests;
      a Tencent success mid-streak RESETS the counter so a fresh streak starts.

#286 extensions (newfqkline/get + real amount): the Tencent index fallback must
switch to newfqkline/get, whose 11-field day rows carry 成交额 in 万元 at index 8,
and write NON-ZERO amount (万元×10000 = yuan) for valid rows. These go RED
against the current (issue #278) implementation which writes amount 0.
  N1. Short/missing rows (fewer than 9 fields) degrade gracefully: amount
      0/empty, no crash, other fields still parsed.
  N2. Malformed amount cell (empty, "-", non-numeric, "NaN", whitespace)
      degrades consistently (amount 0/empty or skipped) with valid siblings
      still writing non-zero amount.
  N3. Pagination boundary: a page with exactly _TENCENT_PAGE_SIZE rows plus a
      short final page still paginates and PRESERVES amount across pages.
  N4. Tencent success resets the fast-fail counter AND writes a NON-ZERO amount.
  N6. Very large amount (999999999999.99 万元) parses without float overflow;
      "0" amount stays 0.

STATUS: RED for #286 — the current implementation uses fqkline/get and writes
amount 0, so every non-zero-amount assertion below fails (amount is "0").
"""

import asyncio
import math
import sys
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

TENCENT_URL = "https://web.ifzq.gtimg.cn/appstock/app/fqkline/get"
_TENCENT_PAGE_SIZE = 2000


# ── payload / stub builders re-used across test classes ──────────────────────


def _tencent_row(
    day: str, close: float = 3000.0, amount: str | None = None
) -> list[object]:
    """A Tencent day row for the fallback helper.

    ``amount`` (成交额, 万元) produces the newfqkline/get 11-field index shape
    with the amount at index 8 (万元 ~ 10000 yuan). ``amount=None`` yields the
    legacy 6-field fqkline/get shape (no amount) so the original #278 tests
    keep exercising the amount-less code path.
    """
    base = [
        day,
        f"{close - 1}",  # open
        f"{close}",      # close
        f"{close + 1}",  # high
        f"{close - 2}",  # low
        "120000000",     # volume
    ]
    if amount is None:
        return base
    # newfqkline index day row — index 8 is 成交额 in 万元:
    # date, open, close, high, low, volume, {}, 振幅, 万元成交额, 涨跌幅, 涨跌额
    return [
        *base,
        {},                  # index 6 (empty dict in the real shape)
        "1.03",              # index 7 振幅
        str(amount),         # index 8 成交额 (万元)
        "0.00",              # index 9
        "0.00",              # index 10
    ]


def _amount_yuan(wanyi: str) -> float:
    """Local oracle: newfqkline index-8 amount (万元) → yuan (×10000)."""
    return float(wanyi) * 10000.0


def _tencent_payload(code: str, rows: list[list[str]]) -> dict:
    return {"code": 0, "msg": "", "data": {code: {"day": rows}}}


def _kline_payload(code: str, klines: list[str]) -> dict:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict]) -> dict:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _board(code: str, name: str) -> dict:
    return {"f12": code, "f14": name}


def _pin_today(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("fetch_index_daily._today", lambda: date(2026, 8, 2))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


def _env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))


# A.1 ── _tencent_code on invalid / inapplicable secids ────────────────────────


class TestTencentCodeInvalidInputs:
    """A1: invalid secids must yield an explicit None/ValueError, never a
    fabricated sh/sz symbol with a garbage prefix."""

    @pytest.mark.parametrize(
        "bad_secid",
        [
            "5.000001",        # prefix not 1./0.
            "0.3990019",       # 9-digit code
            "1.0X0001",        # non-digit chars in code
            "sh000001",        # already fully-prefixed symbol, not a secid
            "1.",              # prefix with NO code
            "",                # empty
            ".000001",         # no market prefix at all
            "7.000001",        # 7. = another (unknown) category
        ],
    )
    def test_invalid_secid_is_explicit_not_garbage_symbol(self, bad_secid: str) -> None:
        """The final symbol for an inapplicable secid must be explicit None or
        a ValueError — it must never be a str that starts with sh/sz (which
        would silently map junk into the Tencent symbol space)."""
        from fetch_index_daily import _tencent_code  # noqa: E402

        try:
            result = _tencent_code(bad_secid)
        except (ValueError, TypeError):
            return  # explicit rejection is acceptable
        assert result is None or (
            isinstance(result, str) and not result.startswith(("sh", "sz"))
        ), (
            f"_tencent_code({bad_secid!r}) must reject inapplicable input, "
            f"got {result!r}"
        )

    def test_valid_prefix_still_maps_after_invalid_rejected(self) -> None:
        """Regression: after rejecting invalid input the valid mapping must
        keep working (1.000001 → sh000001). Guards against an over-eager
        reorder in _tencent_code."""
        from fetch_index_daily import _tencent_code  # noqa: E402

        assert _tencent_code("1.000001") == "sh000001"
        assert _tencent_code("0.399001") == "sz399001"


# A.2 ── _fetch_tencent_kline pagination must be bounded (resource) ───────────


class TestTencentPaginationBounded:
    """A2: full-width source must not make the helper loop forever."""

    @staticmethod
    def _days(n: int, start: str) -> list[str]:
        d = date.fromisoformat(start)
        from datetime import timedelta

        return [(d + timedelta(days=i)).isoformat() for i in range(n)]

    async def test_no_progress_full_pages_terminate_in_bounded_requests(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """RED (resource-exhaustion attack on pagination): the stub answers
        EXACTLY 2000 rows every time and the days NEVER move forward (the same
        earliest date is re-served on every request). A naive
        ``while len(page) == 2000: ...`` loop would never observe a short page
        and would request forever. The helper must terminate after a bounded
        number of requests (page cap or a no-forward-progress guard),
        raising or returning partial data — never spinning indefinitely.

        Bounded means: far fewer requests than a pure data race could exhaust
        (≤ 10 pages), because there is provably no more data to harvest.
        """
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        # A full page, but every request starts from the same window → no
        # forward progress on start_date.
        full = [_tencent_row(d) for d in self._days(_TENCENT_PAGE_SIZE, "2026-01-01")]

        n_requests = {"n": 0}
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            n_requests["n"] += 1
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                return StubResponse(json_data=_tencent_payload(code, full))
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        # Must either return or raise — must NOT hang.
        await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")

        assert n_requests["n"] <= 10, (
            "a source with no forward progress must be detected and stopped, "
            f"got {n_requests['n']} requests (unbounded loop)"
        )

    async def test_always_full_advancing_pages_still_bounded(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """RED (ordering attack, second boundedness check): even when start_date
        DOES advance every page but the page stays exactly 2000 rows wide, the
        helper must eventually give up after a page cap rather than enumerate
        months of synthetic data. Assert termination and a hard bound on the
        number of requests (≤ 200, an order below the 192-request budget the
        whole module already documents for its worst case)."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        days = self._days(_TENCENT_PAGE_SIZE, "2026-01-01")
        full = [_tencent_row(d) for d in days]

        n_requests = {"n": 0}
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            n_requests["n"] += 1
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                # Serve the SAME full window every time (page never goes short).
                return StubResponse(
                    json_data=_tencent_payload(param.split(",")[0], full)
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")

        assert n_requests["n"] <= 200, (
            "full-width endless pages must be capped by a page limit, "
            f"got {n_requests['n']} requests"
        )


# A.3 ── malformed Tencent payloads degrade to failure, never crash ───────────


class TestTencentMalformedPayloads:
    """A3: malformed Tencent JSON must degrade to a counted failure, not crash
    the run with a TypeError/KeyError."""

    @staticmethod
    def _days(n: int, start: str) -> list[str]:
        from datetime import timedelta

        d = date.fromisoformat(start)
        return [(d + timedelta(days=i)).isoformat() for i in range(n)]

    @pytest.mark.parametrize(
        "bad_payload",
        [
            {"code": 0, "msg": ""},                                # no "data"
            {"code": 0, "msg": "", "data": {}},                    # data w/o the code key
            {"code": 0, "msg": "", "data": {"sh000001": {}}},      # code present, no "day"
            {"code": 0, "msg": "", "data": {"sh000001": {"day": None}}},   # day is None
            {"code": 0, "msg": "", "data": {"sh000001": {"day": {}}}},     # day is a dict
            {"code": 0, "msg": "", "data": {"sh000001": {"day": "not-a-list"}}},
            {"code": 0, "msg": "", "data": []},                            # data is a list
            {"code": 0, "msg": "", "data": "bad"},                         # data is a string
        ],
    )
    async def test_malformed_payload_is_failure_not_crash(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
        bad_payload: dict,
    ) -> None:
        """RED: EastMoney fails for the official, Tencent returns a structurally
        broken body → the target is a double-fail (counts once, run completes,
        no crash). A TypeError on ``payload["data"][code]["day"]`` would escape
        run() and crash the pipeline — the helper must treat it as empty/failed."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.OFFICIAL_INDICES",
            ({"secid": "1.000001", "code": "000001", "name": "上证指数"},),
        )

        requests = {"tencent": 0}
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:  # Tencent structurally broken
                requests["tencent"] += 1
                return StubResponse(json_data=bad_payload)
            if "kline/get" in url:  # EastMoney fails
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        # A malformed Tencent body must degrade to a clean failure (RuntimeError
        # when no data exists), never a TypeError/KeyError crash.
        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError):
            await run()

        assert requests["tencent"] == 1, (
            "the malformed Tencent body must have been requested exactly once "
            "(EastMoney already failed → one Tencent fallback attempt)"
        )

    async def test_day_row_fewer_than_six_fields_skipped_safely(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (N1): a short day row (fewer than 9 fields — missing the index-8
        amount) inside an otherwise valid ``day`` list must not crash the row
        builder; short siblings degrade to amount 0/empty while a valid 11-field
        sibling writes its NON-ZERO amount (RED vs the current amount-0 impl)."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.OFFICIAL_INDICES",
            ({"secid": "1.000001", "code": "000001", "name": "上证指数"},),
        )

        valid = _tencent_row("2026-07-30", 3000.0, amount="499525613.00")
        bad = ["2026-07-31"]  # only the date — 1 field, must be skipped
        # 6-field row: has prices but NO index-8 amount → amount must degrade
        # to 0/empty, never crash.
        short6 = _tencent_row("2026-07-29", 2900.0)
        rows = [valid, bad, short6]

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                return StubResponse(
                    json_data=_tencent_payload(param.split(",")[0], rows)
                )
            if "kline/get" in url:
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            tmp_path_ok = await run()

        import csv

        with open(tmp_path_ok, newline="", encoding="utf-8-sig") as f:
            records = list(csv.DictReader(f))
        official = [r for r in records if r["symbol"] == "SH000001"]
        # The 1-field row was skipped; the valid 11-field and 6-field rows survived.
        assert len(official) == 2, (
            f"only parseable rows may be written, got {official!r}"
        )
        assert all(r["trade_date"] in {"2026-07-30", "2026-07-29"} for r in official)

        by_date = {r["trade_date"]: r for r in official}
        # Valid 11-field row carries its real (non-zero) amount — RED today.
        assert float(by_date["2026-07-30"]["amount"]) == pytest.approx(
            _amount_yuan("499525613.00")
        ), (
            "valid newfqkline row must write non-zero amount (万元×10000), "
            f"got {by_date['2026-07-30']['amount']!r}"
        )
        # Short row (missing index 8) degrades gracefully to 0/empty.
        assert by_date["2026-07-29"]["amount"] in {"0", ""}, (
            "a row with no index-8 amount must degrade to 0/empty, "
            f"got {by_date['2026-07-29']['amount']!r}"
        )


# A.4 ── EastMoney code-mismatch must NOT trigger the Tencent fallback ────────


class TestTencentNoFallbackOnCodeMismatch:
    """A4: an EastMoney respose whose echoed code doesn't match the whitelisted
    code is a #277 SKIP — non-empty, so it is neither a failure (must not count
    toward the streak) nor a success (must not reset). Critically it must NOT
    trigger the Tencent fallback: the API is telling us the secid maps to a
    different index, so falling back would silently substitute Tencent data for
    the wrong security."""

    async def test_code_mismatch_skips_fallback_and_preserves_counter(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED: 4 THS industries fail (counter=4). O1 mismatches on EastMoney
        (skip → counter stays 4, NO Tencent request). O2 then double-fails →
        counter=5 → abort on O2. If O1 wrongly reset the counter
        (mismatch-as-success) there would be no abort; if O1 wrongly triggered
        Tencent fallback that fallback's success/failure would corrupt the
        streak. Both are pinned."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        ths_codes = [f"8814{i:02d}" for i in range(1, 5)]
        o1 = {"secid": "1.000001", "code": "000001", "name": "上证指数"}
        o2 = {"secid": "1.000016", "code": "000016", "name": "上证50"}
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", (o1, o2))

        tencent_reqs: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "thshy" in url:
                # THS list yields the 4 industries whose klines all fail.
                anchors = "\n".join(
                    f'<a href="http://q.10jqka.com.cn/thshy/{code}/">{code}</a>'
                    for code in ths_codes
                )
                resp = StubResponse(status_code=200)
                resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")
                return resp
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                tencent_reqs.append(param)
                return StubResponse(status_code=500, json_data={})  # Tencent fail
            if "d.10jqka.com.cn" in url:
                return StubResponse(status_code=500, json_data={})  # THS klines fail
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                bare = secid.rsplit(".", 1)[-1]
                if secid == "1.000001":
                    # mismatch: non-empty klines echo a DIFFERENT code
                    return StubResponse(
                        json_data=_kline_payload(f"99{bare}", ["2026-07-31,1,2,3,4,5,6,7,8,9,0"])
                    )
                if secid == "1.000016":
                    return StubResponse(status_code=500, json_data={})
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(
            RuntimeError
        ) as e:
            await run()

        assert "连续" in str(e.value), (
            "4 THS industry fails + O1 mismatch (skip) + O2 double-fail = 5 "
            "consecutive failures → abort; the mismatch must NOT reset the "
            f"counter. got {str(e.value)!r}"
        )
        # The mismatch (O1) must never have been routed to Tencent; O2's
        # double-fail may still have one Tencent attempt.
        assert not any(p.startswith("sh000001") for p in tencent_reqs), (
            "a code-mismatch skip must NOT trigger the Tencent fallback; "
            f"got tencent params {tencent_reqs!r}"
        )


# A.5 ── fast-fail through the fallback + amount ───────────────────────────────


class TestTencentFastFailAdversarial:
    """A5: 5 consecutive double-fails terminate the run AND stop further Tencent
    requests; a Tencent success mid-streak resets the counter; amount stays 0."""

    async def test_five_double_fails_abort_and_stop_tencent_requests(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (exact boundary + resource): 6 officials, all EastMoney fail.
        Tencent ALSO fails for all 6 → the first 5 are consecutive double-fails
        → run aborts and the 6th is never requested BY EITHER source. This pins
        that the Tencent segment is inside the #277 boundary: nothing after the
        5th double-fail, including Tencent, may consume a request."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        targets = tuple(
            {"secid": f"{'1' if i % 2 else '0'}.{100000 + i:06d}",
             "code": f"{100000 + i:06d}", "name": f"官方{i}"}
            for i in range(1, 7)
        )
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", targets)

        em_secids: list[str] = []
        tencent_params: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                tencent_params.append((params or {}).get("param", ""))
                return StubResponse(status_code=500, json_data={})
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                em_secids.append(secid)
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(
            RuntimeError
        ) as e:
            await run()

        assert "连续" in str(e.value), f"5 double-fails must fast-fail, got {str(e.value)!r}"

        # The 6th target must be untouched by EastMoney...
        last = targets[-1]["secid"]
        assert last not in em_secids, (
            "after 5 consecutive double-fails the 6th must not be requested on "
            f"EastMoney; requested {em_secids}"
        )
        # ...and NOT touched by Tencent either (resource: fallback stops too).
        # Exactly the first 5 targets had one logical Tencent fallback each
        # (HTTP retries may repeat the same param, so count distinct params).
        distinct_tencent = set(tencent_params)
        assert len(distinct_tencent) == 5, (
            "exactly the 5 double-failed targets each get one Tencent fallback "
            f"attempt, got {len(distinct_tencent)}"
        )
        last_tencent = _tencent_symbol(targets[-1]["secid"], targets[-1]["code"])
        assert not any(p.startswith(last_tencent) for p in distinct_tencent), (
            "the 6th target must not receive a Tencent fallback request"
        )

    async def test_tencent_success_resets_counter_and_writes_nonzero_amount(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """RED (N4): officials O1,O2,E,F: E fails on EastMoney + Tencent, F fails
        on EastMoney but SUCCEEDS on Tencent. Then 5 more officials double-fail →
        the streak restarts AFTER F's success: the 5 double-fails FOLLOWING F
        must terminate only at their own 5th (O7). If F's Tencent success had NOT
        cleared the counter the run would have aborted earlier. F's written row
        must carry a NON-ZERO amount (万元×10000) — RED vs the amount-0 impl."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        # 7 officials: [0]=EM fail+tencen fail, [1]=EM fail+tencen success,
        # [2..6]=EM fail+tencen fail (5 consecutive double-fails after [1]).
        targets = tuple(
            {"secid": f"{'1' if i % 2 else '0'}.{100000 + i:06d}",
             "code": f"{100000 + i:06d}", "name": f"官方{i}"}
            for i in range(0, 7)
        )
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", targets)

        # [0] double-fail sets counter 1; [1] EM-fail + tencent SUCCESS resets 0;
        # [2..6] 5 double-fails → abort on [6]. An [7] would not be fetched, but
        # the boundary test already covers "no requests after abort".
        em_secids: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                # Tencent success only for target[1] (symbol from its secid).
                target_symbol = _tencent_symbol(targets[1]["secid"], targets[1]["code"])
                if code == target_symbol:
                    return StubResponse(
                        json_data=_tencent_payload(
                            code,
                            [_tencent_row("2026-07-31", 3100.0, amount="99037192.42")],
                        )
                    )
                return StubResponse(status_code=500, json_data={})
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                em_secids.append(secid)
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        # Proving the abort lands at [6], not earlier: if [1] did NOT reset the
        # counter, the streak from [0]+[2..] would abort at [5].
        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(
            RuntimeError
        ) as e:
            await run()

        assert "连续" in str(e.value), "the post-reset 5 double-fails must fast-fail"
        # [6] was requested (it is the 5th double-fail after the reset), so the
        # counter reached 5 AFTER [1]'s success — proving the reset happened.
        assert targets[6]["secid"] in em_secids, (
            "the 5 double-fails after the Tencent-success reset must reach their "
            "own 5th ([6]); a missing reset would have aborted earlier"
        )

        import csv

        daily = tmp_path / "index_daily.csv"
        with open(daily, newline="", encoding="utf-8-sig") as f:
            records = list(csv.DictReader(f))
        f_symbol = f"{'SH' if targets[1]['secid'].startswith('1.') else 'SZ'}{targets[1]['code']}"
        row = [r for r in records if r["symbol"] == f_symbol]
        assert row, f"the Tencent-success official must write a row ({f_symbol})"
        assert float(row[0]["amount"]) == pytest.approx(_amount_yuan("99037192.42")), (
            "Tencent success must write the real NON-ZERO amount (万元×10000), "
            f"got {row[0]['amount']!r}"
        )


def _tencent_symbol(secid: str, code: str) -> str:
    """Local oracle for the fallback symbol: sh for 1., sz for 0. (matches the
    #278 plan). NOT the implementation under test — this is the asserting side."""
    return ("sh" if secid.startswith("1.") else "sz") + code.lower()


# ── N: newfqkline/get amount handling (issue #286) ───────────────────────────


class TestNewFqKlineAmountAdversarial:
    """RED for #286: the Tencent index fallback must write NON-ZERO amount
    (index-8 成交额 in 万元 ×10000 = yuan) for valid rows, degrade gracefully on
    missing/malformed amounts, preserve amount across pagination, and handle
    very large/small values without float overflow."""

    @staticmethod
    def _splits(klines: list[str]) -> list[list[str]]:
        return [k.split(",") for k in klines]

    @staticmethod
    def _days(n: int, start: str) -> list[str]:
        from datetime import timedelta

        d = date.fromisoformat(start)
        return [(d + timedelta(days=i)).isoformat() for i in range(n)]

    # N_endpoint ── the fallback must hit newfqkline/get, not fqkline/get ─────

    async def test_hits_newfqkline_endpoint(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """RED (#286 N_endpoint): the Tencent fallback must target
        ``newfqkline/get`` (whose day rows carry 成交额 in 万元 at index 8). The
        current implementation still requests ``fqkline/get`` → this fails."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        urls: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            urls.append(url)
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                return StubResponse(
                    json_data=_tencent_payload(
                        param.split(",")[0],
                        [_tencent_row("2026-08-14", 3930.0, amount="99037192.42")],
                    )
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")
        assert klines is not None
        tencent = [u for u in urls if "ifzq.gtimg.cn" in u]
        assert tencent, "must issue a Tencent request"
        assert all("newfqkline/get" in u for u in tencent), (
            "the #286 fix must switch the fallback to newfqkline/get, "
            f"got urls {tencent!r}"
        )

    # N2 ── malformed amount cells degrade, valid sibling keeps amount ─────────

    @pytest.mark.parametrize(
        "bad_amount",
        ["", "-", "NaN", "not-a-number", "  ", "abc123"],
    )
    async def test_malformed_amount_cell_degrades_gracefully(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch,
        bad_amount: str,
    ) -> None:
        """RED (N2): an 11-field row whose index-8 amount is malformed must not
        crash the helper; it degrades to amount 0/empty, while a valid 11-field
        sibling row still writes its NON-ZERO amount (RED today: amount is 0)."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        valid = _tencent_row("2026-08-14", 3930.0, amount="499525613.00")
        # Build a full 11-field row then corrupt index 8.
        malformed = _tencent_row("2026-08-13", 3900.0, amount="1")
        malformed[8] = bad_amount
        rows = [valid, malformed]

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                return StubResponse(
                    json_data=_tencent_payload(param.split(",")[0], rows)
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")
        assert klines is not None, "malformed amount must not crash the helper"

        by_date = {s[0]: s for s in self._splits(klines)}
        # Valid row keeps its real non-zero amount — RED today.
        assert float(by_date["2026-08-14"][6]) == pytest.approx(
            _amount_yuan("499525613.00")
        ), (
            "valid sibling must keep non-zero amount, got "
            f"{by_date['2026-08-14'][6]!r}"
        )
        # Malformed row degrades to 0/empty (consistent, no exception).
        assert by_date["2026-08-13"][6] in {"0", ""}, (
            "malformed amount must degrade to 0/empty, got "
            f"{by_date['2026-08-13'][6]!r}"
        )

    # N3 ── pagination boundary: exactly _TENCENT_PAGE_SIZE rows + short page ──

    async def test_pagination_boundary_preserves_amount_across_pages(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """RED (N3): page 1 answers EXACTLY _TENCENT_PAGE_SIZE rows (full), page 2
        is a short final page. The helper must paginate (2 requests) and MERGE
        the full page + short page, preserving each row's NON-ZERO amount across
        the page boundary — RED today because every amount is written 0."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        page1 = [_tencent_row(d, 3000.0, amount="100.00")
                 for d in self._days(_TENCENT_PAGE_SIZE, "2026-01-01")]
        page2 = [_tencent_row(d, 2000.0, amount="200.00")
                 for d in self._days(50, "2025-01-01")]

        n_requests = {"n": 0}
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "ifzq.gtimg.cn" in url:
                n_requests["n"] += 1
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                parts = param.split(",")
                end = parts[3] if len(parts) > 3 else ""
                if end == "":
                    return StubResponse(json_data=_tencent_payload(code, page1))
                return StubResponse(json_data=_tencent_payload(code, page2))
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")
        assert klines is not None
        assert len(klines) == _TENCENT_PAGE_SIZE + 50, (
            "must merge the full page and the short final page"
        )
        # Two requests: first page empty end date, second advances it.
        assert n_requests["n"] == 2, "full page + short page must paginate once"
        # Amount preserved on BOTH pages (non-zero) — RED today (all zeros).
        splits = self._splits(klines)
        # page1 rows: first 2000 in chronological order (reversed merge).
        non_zero = {s[6] for s in splits}
        assert all(float(a) > 0 for a in non_zero), (
            "amount must be preserved and non-zero across the page boundary, "
            f"got {sorted(non_zero)!r}"
        )

    # N6 ── very large / very small amount values ──────────────────────────────

    async def test_very_large_amount_parses_without_overflow(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """RED (N6): a huge 成交额 (999999999999.99 万元 → 9.99e15 yuan) must parse
        into a Python float without overflow, and a "0" 万元 stays exactly 0."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        rows = [
            _tencent_row("2026-08-14", 3930.0, amount="999999999999.99"),
            _tencent_row("2026-08-13", 3900.0, amount="0"),
        ]
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                return StubResponse(
                    json_data=_tencent_payload(param.split(",")[0], rows)
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")
        assert klines is not None
        by_date = {s[0]: s for s in self._splits(klines)}

        # Huge amount: finite and exactly 万元×10000 — RED today (amount is "0").
        huge = float(by_date["2026-08-14"][6])
        assert math.isfinite(huge), f"huge amount overflowed to {huge}"
        assert huge == pytest.approx(_amount_yuan("999999999999.99")), (
            f"999999999999.99 万元 must be {_amount_yuan('999999999999.99')}, "
            f"got {by_date['2026-08-14'][6]!r}"
        )
        # Zero amount stays zero.
        assert by_date["2026-08-13"][6] == "0", (
            "a '0' 万元 amount must stay 0, got "
            f"{by_date['2026-08-13'][6]!r}"
        )
