"""Adversarial tests for issue #283 — the IMPLEMENTED THS collector interfaces.

Bases on commit f59df12 (feat: industry boards switch to THS source). The two
new public async interfaces ``fetch_ths_industry_list(session, throttle)`` and
``fetch_ths_kline(session, throttle, code, year)`` are locked here against the
plan's declared commitments (plan industry-ths.md B2 / D1 / D2 + the "已实现
接口" list handed to this agent):

  L1. ``fetch_ths_industry_list`` — GBK page, href regex ``(881\\d{3})``,
      ~140 rows → 90 unique, ``(code, name)`` in page order.
  L2. ``fetch_ths_kline`` — strips the JSONP wrapper, reads the ``data`` field
      (``;``-separated), reorders THS 7-field rows into the EastMoney
      (``_kline_records``) column order ``date,open,close,high,low,volume,amount``.
  L3. Two-segment ``run()`` THS year loop — only an EMPTY year terminates the
      loop (no older-data boundary); a request/parse FAILURE (None) is a
      transient glitch and is logged and walked past.

Attack dimensions (everything exercised against the *already-implemented*
interfaces, so the tests lock real behavior — GREEN now unless a defect is
found, which is reported for the main agent to fix):

  A1 column-integrity (L2, HIGH VALUE) — the plan's own warning (2026-08-16
     实测: THS 序是 日期,开,高,低,收,量,额, 东财 序是 日期,开,收,高,低,量,额)
     is that reusing the EM mapping as-is silently swaps high/close. The
     requirement test feeds symmetric (high==low==close) values and only
     checks date+len, so a wrong or lost reorder passes it. Here every THS
     cell is DISTINCT and the ``_kline_records`` output mapping is asserted
     cell-by-cell — a reorder regression (swap of high/low/close) must fail.
  A2 column-integrity under dirty cells (L2) — when the THS close cell is
     non-numeric but the THS high cell is numeric, the EM close must degrade to
     '' and MUST NOT borrow the (clean) high value: a misplaced reorder index
     would leak a price into the wrong column.
  A3 malformed hrefs / non-881xxx (L1) — 5-digit (88112), 8-digit (88112345),
     alphanumeric (881AB12), bare /thshy/, no href → all rejected; only the
     two real 881xxx anchors survive.
  A4 boundary+resource (L1) — duplicate codes keep the FIRST name in page
     order (dedup by code, not by name); return order == dedup'd page order.
  A5 error path (L1) — non-GBK / binary content must decode with errors
     replaced (never raise) and yield a stable (possibly empty) list; invalid
     UTF-8 NOT interpreted as a crash.
  A6 error path (L2) — malformed JSONP variants: no parens → None; non-string
     ``data`` → []; structurally empty data → []; empty year → [], all without
     raising.
  A7 boundary (L3, resource) — an empty year mid-sequence must stop the year
     loop so older (later) years are never requested (no phantom requests).

Coverage note: fast-fail on 5 consecutive THS kline failures, concept-removal
grep, and ``_kline_records`` dirty-7-field tolerances are owned by
test_ths_industry_adversarial.py / test_fast_fail_adversarial.py — not
duplicated here.
"""

import asyncio
import sys
from datetime import date
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

import fetch_index_daily as fid  # noqa: E402


def _pin_today(monkeypatch: pytest.MonkeyPatch, day: str = "2026-08-02") -> None:
    monkeypatch.setattr("fetch_index_daily._today", lambda: date.fromisoformat(day))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


def _anchor(code: str, name: str, *, detail: bool = False) -> str:
    path = f"/thshy/detail/code/{code}/" if detail else f"/thshy/{code}/"
    return f'<a href="http://q.10jqka.com.cn{path}">{name}</a>'


# ── A1: THS→EastMoney column reorder is cell-exact (uncovered by requirement) ─


class TestKlineReorderColumnIntegrity:
    """L2 core claim: THS row ``date,open,high,low,close,volume,amount`` must
    land in ``_kline_records`` as ``date,open,close,high,low,volume,amount``.
    Uses DISTINCT open>high>low>close so any high/close residue is visible."""

    async def test_distinct_values_reorder_to_eastmoney_order(
        self, make_stub_session
    ) -> None:
        url = fid.THS_KLINE_TPL.format(code="881101", year=2026)
        # THS order: date, open=10, high=9, low=7, close=8, volume=1e6, amount=5e9
        ths_row = "2026-07-31,10,9,7,8,1000000,5000000000"

        async def _get(u, params=None, headers=None):
            assert u == url
            resp = StubResponse(status_code=200)
            resp._text = f'quotebridge_v4_line_bk_881101_01_2026({{"data": "{ths_row}"}})'  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        klines = await fid.fetch_ths_kline(stub, fid.Throttle(min_interval=0), "881101", 2026)
        assert klines is not None
        recs = fid._kline_records("BK881101", "industry", klines, date(2026, 8, 2))
        assert len(recs) == 1
        r = recs[0]
        # EastMoney columns: open=10 → 10; close must be THS close(8), NOT THS
        # high(9); high must be THS high(9), NOT THS close(8); low = THS low(7).
        assert r["open"] == 10
        assert r["close"] == 8, (
            "EM close must equal THS close (idx4), got {!r} — high/close swap".format(r["close"])
        )
        assert r["high"] == 9, (
            "EM high must equal THS high (idx2), got {!r} — high/close swap".format(r["high"])
        )
        assert r["low"] == 7, "EM low must equal THS low (idx3)"
        assert r["volume"] == 1000000
        assert r["amount"] == 5000000000

    async def test_detail_code_path_on_list_and_clean_kline(self, make_stub_session) -> None:
        """A1 companion on the list side: the detail/code/ href form (the live
        page shape) parses identically, and per-value reorder is preserved end
        to end through the list → kline path the run() builds."""
        anchors = _anchor("881101", "半导体", detail=True)

        async def _get(url, params=None, headers=None):
            if "thshy" in url:
                resp = StubResponse(status_code=200)
                resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")  # type: ignore[attr-defined]
                return resp
            if "d.10jqka.com.cn" in url:
                resp = StubResponse(status_code=200)
                resp._text = 'cb({"data": "2026-07-31,10,9,7,8,1000000,5000000000"})'  # type: ignore[attr-defined]
                return resp
            return StubResponse(status_code=200)

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        boards = await fid.fetch_ths_industry_list(stub, fid.Throttle(min_interval=0))
        assert boards == [("881101", "半导体")]
        klines = await fid.fetch_ths_kline(stub, fid.Throttle(min_interval=0), "881101", 2026)
        assert klines is not None
        r = fid._kline_records("BK881101", "industry", klines, date(2026, 8, 2))[0]
        assert (r["open"], r["high"], r["low"], r["close"]) == (10, 9, 7, 8)


# ── A2: dirty THS cells never leak into a sibling clean column ────────────────


class TestKlineReorderDirtyNoLeak:
    """When a THS numeric cell is dirty, a wrong reorder index would pull a
    clean value from a sibling column. Assert each column degrades (or keeps)
    exactly the value THS assigned it — nothing borrowed."""

    async def test_dirty_close_does_not_borrow_a_clean_column(
        self, make_stub_session
    ) -> None:
        url = fid.THS_KLINE_TPL.format(code="881102", year=2026)
        # THS order: date, open=10, high=9, low=7, close=abc(dirty), volume, amount
        # If the reorder mistakenly put THS high(9) into EM close it would
        # fabricate a close — a silent value leak. Correct behavior: EM close ''.
        ths_row = "2026-07-30,10,9,7,abc,1000000,5000000000"

        async def _get(u, params=None, headers=None):
            assert u == url
            resp = StubResponse(status_code=200)
            resp._text = f'cb({{"data": "{ths_row}"}})'  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        klines = await fid.fetch_ths_kline(stub, fid.Throttle(min_interval=0), "881102", 2026)
        recs = fid._kline_records("BK881102", "industry", klines or [], date(2026, 8, 2))
        assert len(recs) == 1
        r = recs[0]
        assert r["close"] == "", (
            "dirty THS close must degrade to '', got {!r} — a sibling price leaked into EM close".format(r["close"])
        )
        # The clean THS high (index2) still lands in EM high.
        assert r["high"] == 9, "clean THS high must land in EM high, got {!r}".format(r["high"])


# ── A3: malformed / non-881xxx hrefs are rejected ──────────────────────────────


class TestListMalformedHrefRejection:
    """L1 regex ``(881\\d{3})`` — codes of the wrong length/scanc rejected and a
    malformed row never crashes the parser. Only well-formed 881xxx anchors are
    emitted."""

    async def test_wrong_length_and_alnum_codes_rejected(self, make_stub_session) -> None:
        good = [_anchor("881201", "半导体"), _anchor("881202", "软件")]
        bad = [
            _anchor("88112", "five"),      # 5-digit
            _anchor("88112345", "eight"),  # 8-digit
            _anchor("881AB12", "alnum"),   # non-numeric tail
            _anchor("88112", "ninesix"),   # 5-digit again
            '<a href="http://q.10jqka.com.cn/thshy/">no-code</a>',
            '<td>881999</td>',             # not an href
            '<a class="x">no href attr</a>',
        ]
        body = "<html><body>" + "<br>".join(good + bad) + "</body></html>"

        async def _get(url, params=None, headers=None):
            assert "thshy" in url
            resp = StubResponse(status_code=200)
            resp._content = body.encode("gbk")  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        out = await fid.fetch_ths_industry_list(stub, fid.Throttle(min_interval=0))
        assert out == [("881201", "半导体"), ("881202", "软件")], (
            f"only the two well-formed 881xxx anchors must survive, got {out!r}"
        )

    async def test_malformed_kline_jsonp_returns_none(self, make_stub_session) -> None:
        """A3-companion on the kline side: a body with no JSONP parentheses is
        not a JSONP wrapper → the caller must treat it as a failed fetch
        (None), never fabricate rows."""
        url = fid.THS_KLINE_TPL.format(code="881203", year=2026)

        async def _get(u, params=None, headers=None):
            assert u == url
            resp = StubResponse(status_code=200)
            resp._text = "not a jsonp wrapper at all, just text"  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        out = await fid.fetch_ths_kline(stub, fid.Throttle(min_interval=0), "881203", 2026)
        assert out is None, f"a paren-less body must be a failed fetch (None), got {out!r}"


# ── A4: dedup keeps the first name, page order preserved ──────────────────────


class TestListDedupOrder:
    """L1 dedup contract: ~140 rows → 90 unique, keyed by code ONLY. When the
    same code repeats with a DIFFERENT name the first occurrence wins (no
    last-wins or merge), and the returned order is the dedup'd page order."""

    async def test_duplicate_code_keeps_first_name(self, make_stub_session) -> None:
        anchors = [
            _anchor("881301", "半导体"),
            _anchor("881302", "软件"),
            _anchor("881301", "半导体改名"),   # dup code, different name
            _anchor("881303", "银行"),
            _anchor("881302", "软件改名"),
        ]
        body = "<html><body>" + "".join(anchors) + "</body></html>"

        async def _get(url, params=None, headers=None):
            resp = StubResponse(status_code=200)
            resp._content = body.encode("gbk")  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        out = await fid.fetch_ths_industry_list(stub, fid.Throttle(min_interval=0))
        assert [c for c, _ in out] == ["881301", "881302", "881303"], (
            "dedup must keep first-seen order, got %r" % [c for c, _ in out]
        )
        names = dict(out)
        assert names["881301"] == "半导体", (
            "first-seen name must win over a later duplicate, got {!r}".format(names["881301"])
        )
        assert names["881302"] == "软件", "first-seen name must win, got {!r}".format(names["881302"])


# ── A5: GBK decode failure / non-GBK bytes degrade, never crash ───────────────


class TestListGbkDecodeRobustness:
    """L1 error path: content that cannot be decoded as valid GBK must be
    handled with replace-errors (no UnicodeDecodeError), yielding a stable —
    possibly empty — list."""

    async def test_binary_not_gbk_does_not_raise(self, make_stub_session) -> None:
        async def _get(url, params=None, headers=None):
            resp = StubResponse(status_code=200)
            # 0xFF 0xFE is an invalid GBK leading byte pair, plus embedded NULs.
            resp._content = b"\xff\xfe\x00\x01\x80\x81\x02<html>\x00</html>"  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        # Guard: even random bytes must not crash the parser.
        out = await fid.fetch_ths_industry_list(stub, fid.Throttle(min_interval=0))
        assert isinstance(out, list), f"a non-decodable page must return a list, got {out!r}"

    async def test_utf8_page_degrades_to_empty_not_crash(self, make_stub_session) -> None:
        """A page served as UTF-8 (not GBK) decodes as mojibake — the regex on
        ASCII anchors still matches, so a names-bearing UTF-8 page keeps its
        codes. Never raises, never fabricates codes."""
        anchors = [
            '<a href="http://q.10jqka.com.cn/thshy/881401/">半导体</a>',  # UTF-8 bytes
        ]
        body = "<html><body>" + "".join(anchors) + "</body></html>"

        async def _get(url, params=None, headers=None):
            resp = StubResponse(status_code=200)
            resp._content = body.encode("utf-8")  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        out = await fid.fetch_ths_industry_list(stub, fid.Throttle(min_interval=0))
        assert [c for c, _ in out] == ["881401"], (
            f"the ASCII code is still extractable from a UTF-8 page, got {out!r}"
        )

    async def test_http_error_yields_empty_list(self, make_stub_session) -> None:
        async def _get(url, params=None, headers=None):
            return StubResponse(status_code=403)

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        out = await fid.fetch_ths_industry_list(stub, fid.Throttle(min_interval=0))
        assert out == [], f"an HTTP error must yield [] (no-op boards), got {out!r}"


# ── A6: JSONP parse anomalies on the kline interface ──────────────────────────


class TestKlineJsonpAnomalies:
    """L2 error path: structurally odd JSONP bodies must each resolve to a
    defined output ([], None) without raising or fabricating rows."""

    async def _get_kline(self, make_stub_session, body: str):
        url = fid.THS_KLINE_TPL.format(code="881501", year=2026)

        async def _get(u, params=None, headers=None):
            assert u == url
            resp = StubResponse(status_code=200)
            resp._text = body  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        return await fid.fetch_ths_kline(stub, fid.Throttle(min_interval=0), "881501", 2026)

    async def test_data_field_non_string_yields_empty(self, make_stub_session) -> None:
        # A valid JSON object whose 'data' is not a string (e.g. a number) is
        # not a CSV carrier → [] (bare-body fallback also yields no 7-field rows).
        out = await self._get_kline(make_stub_session, 'cb({"data": 123})')
        assert out == [], f"a non-string data must yield [], got {out!r}"

    async def test_empty_data_string_yields_empty(self, make_stub_session) -> None:
        out = await self._get_kline(make_stub_session, 'cb({"data": ""})')
        assert out == [], f"empty data string must yield [], got {out!r}"

    async def test_bare_csv_body_parses_as_rows(self, make_stub_session) -> None:
        # Test-fixture shape: rows directly inside the JSONP parens, no JSON
        # object wrapper (json.loads fails → the raw rows fall through). The
        # shape is a NEWLINE of comma-rows, not a ';' string.
        body = "(\n2026-07-31,10,9,7,8,1000000,5000000000\n2026-07-30,9,8,6,7,900000,4000000000\n)"
        out = await self._get_kline(make_stub_session, body)
        assert out is not None
        assert len(out) == 2, f"a bare-CSV JSONP body must parse, got {out!r}"
        # First row (THS order) reordered to EM order.
        assert out[0] == "2026-07-31,10,8,9,7,1000000,5000000000"

    async def test_body_with_unbalanced_parens_returns_none(self, make_stub_session) -> None:
        # rfind(")") < find("(") or missing ')' → end<=start → None.
        out = await self._get_kline(make_stub_session, 'cb({"data": "2026-07-31,1,2,3,4,5,6"')
        assert out is None, f"an unterminated wrapper must be a failed fetch (None), got {out!r}"

    async def test_semicolon_separated_rows_within_data(self, make_stub_session) -> None:
        # Live API shape: a single string field with ';'-separated rows.
        data = ("2026-07-31,10,9,7,8,1000000,5000000000"
                ";2026-07-30,9,8,6,7,900000,4000000000")
        out = await self._get_kline(make_stub_session, f'cb({{"data": "{data}"}})')
        assert out is not None and len(out) == 2, f"semicolon rows must parse, got {out!r}"


# ── A7: empty year terminates the loop (L3 run() resource guard) ──────────────


class TestRunEmptyYearTermination:
    """L3 boundary+resource: a THS board's year loop must stop at the first
    empty year so older (2007..year-2) years are never requested — a silently
    empty middle glitch must not spin ~18 more requests per broken board across
    90 boards (≈1620 wasted calls). The empty year ITSELF is fetched (it has to
    be, to discover there is no data); the guard is that nothing older is."""

    async def test_empty_year_stops_older_year_requests(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch, "2026-08-02")

        requested_years: list[int] = []

        async def _get(url, params=None, headers=None):
            if "thshy" in url:
                resp = StubResponse(status_code=200)
                resp._content = _anchor("881601", "半导体", detail=True).encode("gbk")  # type: ignore[attr-defined]
                return resp
            if "d.10jqka.com.cn" in url:
                year = int(url.rsplit(".", 1)[0].rsplit("/", 1)[-1])
                requested_years.append(year)
                resp = StubResponse(status_code=200)
                if year == 2026:
                    resp._text = "cb({'data':'2026-07-31,10,9,7,8,1000000,5e9'})"  # type: ignore[attr-defined]
                else:
                    # Year 2025 and older are empty → loop must halt after 2025.
                    resp._text = 'cb({"data": ""})'  # type: ignore[attr-defined]
                return resp
            if "kline/get" in url:  # official indices succeed
                secid = (params or {}).get("secid", "")
                code = secid.rsplit(".", 1)[-1]
                return StubResponse(
                    status_code=200,
                    json_data={
                        "rc": 0,
                        "data": {
                            "code": code,
                            "klines": ["2026-07-31,1,2,3,4,5,6,0,0,0,0,0"],
                        },
                    },
                )
            return StubResponse(status_code=200)

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            patch("fetch_index_daily.write_csv", lambda records, _p: None),
        ):
            await run()

        # 2026 has data, 2025 empty → 2025 must be fetched (to discover the gap)
        # then the loop halts: 2024/2007 are never requested.
        assert requested_years == [2026, 2025], (
            f"after the empty 2025 the loop must halt; got requested years {requested_years!r}"
        )

    async def test_transient_year_failure_keeps_going_and_preserves_data(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """L3 error-path (adversarial on the pending review P1-1 contract): a
        request/parse FAILURE (None) on one year is a transient glitch — the
        loop must NOT truncate the history, it must keep walking back. An
        EMPTY year remains the hard boundary (no older data). Scenario:
        2026 OK, 2025 → 500 (continue, logged), 2024 empty (halt); the 2026
        bars must be preserved despite the 2025 failure, and nothing older
        than the empty 2024 is requested."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch, "2026-08-02")

        requested_years: list[int] = []
        written: list[list[dict[str, object]]] = []

        async def _get(url, params=None, headers=None):
            if "thshy" in url:
                resp = StubResponse(status_code=200)
                resp._content = _anchor("881603", "医药", detail=True).encode("gbk")  # type: ignore[attr-defined]
                return resp
            if "d.10jqka.com.cn" in url:
                year = int(url.rsplit(".", 1)[0].rsplit("/", 1)[-1])
                requested_years.append(year)
                if year == 2026:
                    resp = StubResponse(status_code=200)
                    resp._text = 'cb({"data": "2026-07-31,10,9,7,8,1000000,5e9"})'  # type: ignore[attr-defined]
                    return resp
                if year == 2025:
                    return StubResponse(status_code=500)  # transient failure
                resp = StubResponse(status_code=200)
                resp._text = 'cb({"data": ""})'  # type: ignore[attr-defined]
                return resp
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                code = secid.rsplit(".", 1)[-1]
                return StubResponse(
                    status_code=200,
                    json_data={"rc": 0, "data": {
                        "code": code, "klines": ["2026-07-31,1,2,3,4,5,6,0,0,0,0,0"]}},
                )
            return StubResponse(status_code=200)

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            patch("fetch_index_daily.write_csv",
                  lambda records, _p: written.append(records)),
        ):
            await run()

        # Distinct years traversed: 2026 (data) → 2025 (500, kept going) → 2024
        # (empty halt). A failing year may be retried by the fetch helper, so
        # assert the distinct-order contract, not the raw retry count — but the
        # empty 2024 must halt before 2023 / older is ever probed.
        seen_years = []
        for y in requested_years:
            if not seen_years or seen_years[-1] != y:
                seen_years.append(y)
        assert seen_years == [2026, 2025, 2024], (
            f"distinct traversal must be 2026(ok)→2025(500)→2024(empty halt), "
            f"got {seen_years!r}"
        )
        assert 2023 not in requested_years, (
            "the empty 2024 must halt before 2023 is ever requested"
        )
        daily_rows = [r for batch in written for r in batch]
        assert any(
            r["symbol"] == "BK881603" and r["trade_date"] == "2026-07-31"
            for r in daily_rows
        ), "the 2026 bars survive the transient 2025 failure"

    async def test_board_with_all_years_failing_still_fast_fails(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """L3 resource: when EVERY year of several boards 500s (the 'kept going'
        branch), each board still walks back to 2007, collects nothing, and each
        counts toward the #277 fast-fail counter — a fully broken board can never
        masquerade as a success, and five of them abort the run."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch, "2026-08-02")

        requested_years: list[int] = []

        async def _get(url, params=None, headers=None):
            if "thshy" in url:
                # 5 boards, all broken (every year → 500).
                anchors = "".join(
                    _anchor(f"88170{i}", f"B{i}", detail=True) for i in range(1, 6)
                )
                resp = StubResponse(status_code=200)
                resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")  # type: ignore[attr-defined]
                return resp
            if "d.10jqka.com.cn" in url:
                year = int(url.rsplit(".", 1)[0].rsplit("/", 1)[-1])
                requested_years.append(year)
                return StubResponse(status_code=500)  # every year fails
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                code = secid.rsplit(".", 1)[-1]
                return StubResponse(
                    status_code=200,
                    json_data={"rc": 0, "data": {
                        "code": code, "klines": ["2026-07-31,1,2,3,4,5,6,0,0,0,0,0"]}},
                )
            return StubResponse(status_code=200)

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            patch("fetch_index_daily.write_csv", lambda records, _p: None),
            pytest.raises(RuntimeError, match="连续"),
        ):
            await run()

        # 5 broken boards each probe the full 20-year range (no silent
        # truncation), bump the counter, and abort at the 5th board. Failing
        # years may be retried, so assert the distinct full-coverage + that the
        # loop walked all the way back to 2007 — not the raw retry count.
        assert set(requested_years) == set(range(2007, 2027)), (
            f"the all-500 boards must probe every year 2026..2007; got {sorted(set(requested_years))!r}"
        )
        for y in range(2007, 2027):
            assert requested_years.count(y) >= 1, f"year {y} must be probed"
        assert min(requested_years) == 2007 and max(requested_years) == 2026, (
            "every board must probe the full year range"
        )

    async def test_compact_date_normalized_to_iso(self, make_stub_session) -> None:
        """Real THS klines carry YYYYMMDD dates (20260105); the reordered row
        must normalize to ISO so _kline_records keeps the row (lexical
        compare vs ISO today drops the compact form — review finding)."""
        import fetch_index_daily as fid  # noqa: E402

        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            resp = StubResponse(status_code=200)
            resp._text = (
                'cb({"data":"20260105,10,12,9,11,1000000,5000000000'
                ';2026-01-06,11,13,10,12,1100000,5100000000"})'
            )
            return resp
        stub.get = _get  # type: ignore[method-assign]

        out = await fid.fetch_ths_kline(stub, fid.Throttle(min_interval=0), "881101", 2026)
        assert out is not None and len(out) == 2
        # Compact date normalized; ISO date passes through.
        assert out[0].startswith("2026-01-05,"), out[0]
        assert out[1].startswith("2026-01-06,"), out[1]
        # End-to-end: _kline_records must keep the normalized row.
        from datetime import date

        from fetch_index_daily import _kline_records  # noqa: E402

        recs = _kline_records("BK881101", "industry", out, date(2026, 8, 2))
        assert len(recs) == 2, "normalized rows must survive _kline_records"
