"""Requirement acceptance tests for issue #283 — 同花顺行业板块采集（THS industry source).

Contract under test (issue #283 acceptance criteria 1, fetch_index_daily.py B2):

  C1. ``fetch_ths_industry_list(session, throttle)`` fetches the real-time THS
      industry list from ``q.10jqka.com.cn/thshy/`` (GBK-encoded HTML), extracts
      every ``881xxx`` code + its name from ``href``, and de-duplicates the raw
      ~140 rows into the 90 unique industries (申万一级, 881xxx). No EastMoney
      clist concept discovery is involved.

  C2. ``fetch_ths_kline(session, throttle, code, year)`` fetches one year of
      daily klines from ``d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js``
      (the 7-field CSV ``日期,开,高,低,收,量,额``), paginated by year over
      2007..current year, reusing the existing ``_kline_records`` mapping for
      the official ``index_daily`` shape.

  C3. ``run()`` no longer discovers concept (or EastMoney BK industry) boards:
      its output MUST NOT contain any ``index_type == "concept"`` row; the only
      BK symbols written are THS 881xxx industries tagged ``industry``. The
      fast-fail (#277 ``_MAX_CONSECUTIVE_FAILURES``) guard applies to the THS
      segment: 5 consecutive THS kline failures raise a ``RuntimeError``
      mentioning "连续".

STATUS: RED.
- C1/C2 helpers do not exist yet (B2 has no implementation commit) → the list /
  kline tests fail with AttributeError (new-interface RED).
- C3 ``run()`` still discovers concept boards today → the no-concept test fails
  with a *logic* failure (a concept row is present), and the THS fast-fail test
  fails because no THS segment exists to fast-fail (no RuntimeError raised).
"""

import asyncio
import datetime
import sys
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Handoff-verified THS endpoints (issue body + 2026-08-15 实测).
THS_LIST_URL = "https://q.10jqka.com.cn/thshy/"
THS_KLINE_TPL = "https://d.10jqka.com.cn/v4/line/bk_{code}/01/{year}.js"

# Legacy EastMoney clist / push2his endpoints (pre-per-removal constants used
# ONLY to manufacture a reliable CURRENT-behavior RED for C3).
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    """Shape of the EastMoney push2his kline response (used for the C3 RED)."""
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _kline_row(day: str) -> str:
    """One 11-field EastMoney kline row (7 fields used by ``_kline_records``)."""
    return (
        f"{day},2999,3000,3001,2998,"
        f"120000000,52000000000,1.5,0.5,1.0,0.5"
    )


def _pin_today(monkeypatch: pytest.MonkeyPatch, day: str = "2026-08-02") -> None:
    monkeypatch.setattr("fetch_index_daily._today", lambda: datetime.date.fromisoformat(day))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


# ── C1 ─────────────────────────────────────────────────────────────


class TestThsIndustryList:
    """C1: THS industry list fetch + 90-unique 881xxx extraction (GBK)."""

    async def test_list_parses_90_unique_881xxx(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """C1 RED: a 140-row page with 50 duplicate codes must yield 90 unique
        (code, name) pairs, all with 881xxx codes."""
        # 90 unique + 50 duplicates reproduces the live page's 140-rows→90.
        uniq = [(f"881{i:03d}", f"industry{i}") for i in range(90)]
        dupes = [(f"881{i:03d}", f"industry{i}") for i in range(50)]

        async def _get(url, params=None, headers=None):
            assert url == THS_LIST_URL, "list must come from q.10jqka.com.cn"
            # The THS page is GBK HTML; the helper must decode and pull hrefs.
            anchors = "\n".join(
                f'<a href="http://q.10jqka.com.cn/thshy/{code}/">{name}</a>'
                for code, name in (uniq + dupes)
            )
            resp = StubResponse(status_code=200)
            resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        import fetch_index_daily as fid  # noqa: E402

        # RED via AttributeError: fetch_ths_industry_list does not exist yet.
        out = await fid.fetch_ths_industry_list(stub, fid.Throttle())
        assert len(out) == 90, "140 rows must de-duplicate to 90"
        assert len({c for c, _ in out}) == 90, "codes must be unique"
        assert all(c.startswith("881") and c.isdigit() for c, _ in out), "881xxx only"
        assert ("881000", "industry0") in out


# ── C2 ─────────────────────────────────────────────────────────────


class TestThsAnnualKline:
    """C2: per-year THS kline pagination + 7-field CSV mapping."""

    async def test_kline_parses_7_field_rows(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """C2 RED: the year-JS kline body's 7-field CSV rows come back unchanged
        (ready for ``_kline_records``), preserving dates."""
        year = 2026
        url = THS_KLINE_TPL.format(code="881101", year=year)

        async def _get(u, params=None, headers=None):
            assert u == url, "kline must come from the per-year THS endpoint"
            rows = "\n".join(
                ["2026-07-31,2999,3000.5,3001,2998,120000000,52000000000",
                 "2026-07-30,2998,2999,3000,2997,110000000,51000000000"]
            )
            resp = StubResponse(status_code=200)
            resp._text = f"quotebridge_v4_line_bk_881101_01_{year}({rows})"  # type: ignore[attr-defined]
            return resp

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        import fetch_index_daily as fid  # noqa: E402

        # RED via AttributeError: fetch_ths_kline does not exist yet.
        klines = await fid.fetch_ths_kline(stub, fid.Throttle(), "881101", year)
        assert klines is not None
        assert len(klines) == 2
        assert klines[0].split(",")[0] == "2026-07-31"
        assert len(klines[0].split(",")) == 7, "7-field CSV preserved"


# ── C3 run() concept removal + THS segment fast-fail ───────────────


class TestRunNoConceptAndFastFail:
    """C3: run() drops concept/EastMoney-BK discovery and fast-fails on 5
    consecutive THS failures."""

    async def test_run_output_has_no_concept_type(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """C3 RED: a board universe that today produces a concept row must, after
        the D4 removal, write NO index_type == 'concept' row. Today run() still
        emits that concept row → the assertion fails (logic RED)."""
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )

        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            # Concept board discovered via clist + its kline; one official index.
            if "clist/get" in url:
                return StubResponse(
                    status_code=200,
                    json_data=_clist_payload([{"f12": "BK1169", "f14": "AI概念"}]),
                )
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                code = secid.rsplit(".", 1)[-1]
                return StubResponse(
                    status_code=200,
                    json_data=_kline_payload(code, [_kline_row("2026-07-31")]),
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = [r for batch in captured for r in batch]
        types = {r["index_type"] for r in rows}
        assert "concept" not in types, (
            "concept rows must never be written after the D4 removal"
        )

    async def test_ths_segment_fast_fails_on_5_consecutive(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """C3 RED: 5 consecutive THS kline failures abort the run with a
        RuntimeError mentioning '连续' — the #277 guard must cover the THS
        segment.

        Today there is no THS segment AND no concept removal: ``run()`` only
        discovers boards via clist (empty here) and its official indices all
        succeed, so a THS-only failure stream can never raise "连续" — the
        raises() block does NOT fire → logic RED. After B2, the THS list yields
        90 industries whose klines all fail → 5 consecutive → RuntimeError.
        """
        from fetch_index_daily import run  # noqa: E402

        monkeypatch.chdir(tmp_path)
        monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
        _pin_today(monkeypatch)

        captured: list[list[dict[str, object]]] = []
        monkeypatch.setattr(
            "fetch_index_daily.write_csv",
            lambda records, _path: captured.append(records),
        )

        # A pure THS universe whose klines ALL fail (500), plus official indices
        # that succeed so the ONLY possible "连续" source is the THS segment.
        stub = make_stub_session()
        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                # Pre-removal board discovery returns no boards (no clist data):
                # boards contribute nothing today.
                return StubResponse(status_code=200, json_data=_clist_payload([]))
            if "kline/get" in url:
                # Official indices succeed — no failure streak from official.
                secid = (params or {}).get("secid", "")
                code = secid.rsplit(".", 1)[-1]
                return StubResponse(
                    status_code=200,
                    json_data=_kline_payload(code, [_kline_row("2026-07-31")]),
                )
            if "thshy" in url:
                # THS list yields the 90 industries.
                anchors = "\n".join(
                    f'<a href="http://q.10jqka.com.cn/thshy/881{i:03d}/">{i}</a>'
                    for i in range(90)
                )
                resp = StubResponse(status_code=200)
                resp._content = f"<html><body>{anchors}</body></html>".encode("gbk")  # type: ignore[attr-defined]
                return resp
            # Every THS per-year kline call fails.
            return StubResponse(status_code=500, json_data={})

        stub.get = _get  # type: ignore[method-assign]
        with (
            patch("fetch_index_daily.AsyncSession", return_value=stub),
            pytest.raises(RuntimeError, match="连续"),
        ):
            await run()
