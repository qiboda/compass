"""Requirement acceptance tests for issue #278/#286 — Tencent fallback for official indices.

Contract under test (fetch_index_daily.py):

  C1. ``_tencent_code(secid)`` maps an EastMoney secid to the Tencent symbol:
      prefix ``1.`` → ``sh``, ``0.`` → ``sz`` + lowercase code
      (``1.000001`` → ``sh000001``, ``0.399001`` → ``sz399001``).
  C2. ``_fetch_tencent_kline(session, throttle, secid)`` fetches full history by
      paginating ``param=<code>,day,,<end_date>,<count>,qfq`` with count ≤ 2000;
      it keeps advancing the end_date backwards until a short (<2000) page is
      seen and returns the merged round bars.
  C3. ``run()`` fetches official indices from EastMoney first; when a target's
      EastMoney kline fails or is empty it automatically falls back to Tencent.
      The CSV output format is unchanged — ``date,open,close,high,low,volume,
      amount`` matching the EastMoney order.
  C4. ``run()``'s Tencent segment is protected by the #277 consecutive-failure
      fast fail: 5 consecutive official indices failing on BOTH sources raise a
      ``RuntimeError`` mentioning "连续" and never request the remaining target.

Issue #286 contract (implemented and GREEN):

  R1. The Tencent fallback switches from ``fqkline/get`` to
      ``newfqkline/get``, whose ``day`` rows are 11 fields with 成交额 in
      **万元** at 0-based index 8.
  R2. ``_fetch_tencent_kline`` converts that 万元 figure to **yuan**
      (万元 × 10000) and emits the same 7-field CSV row
      ``date,open,close,high,low,volume,amount`` consumed by
      ``_kline_records`` — so a valid Tencent-sourced official row has a
      NON-ZERO amount.
  R3. A ``newfqkline`` day row with fewer than 9 fields (no 成交额) degrades
      gracefully — amount 0/empty, never a crash (fallback safety net).

The #277/#278 machinery is GREEN, and issue #286 is implemented: the Tencent
fallback now uses ``newfqkline/get`` and writes real non-zero yuan amounts
(万元 × 10000). The tests below pin that contract and are GREEN on the current
production code.

STATUS: GREEN.
"""

import asyncio
import csv
import sys
from datetime import date, timedelta
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse  # noqa: E402

# Handoff-verified endpoints.
KLINE_URL = "https://push2his.eastmoney.com/api/qt/stock/kline/get"
CLIST_URL = "https://push2delay.eastmoney.com/api/qt/clist/get"
# Issue #286: the fallback switched to newfqkline/get, whose day rows are 11
# fields with 成交额 in 万元 at index 8.

_TENCENT_PAGE_SIZE = 2000

# Example newfqkline/get day row (0-based):
#  0        1        2        3        4        5             6   7     8             9    10
#  date    open    close    high     low     volume        pre 振幅  成交额(万元)   ...
_MARKER = {}  # the pre/post marker column (empty dict in the real payload)


def _tencent_row(day: str, close: float = 3000.0) -> list[str]:
    """Legacy 6-field Tencent day row (date,open,close,high,low,volume).

    Kept for tests that exercise pagination / degradation with NO 成交额 field;
    a len<9 row must still degrade gracefully to amount 0/empty (R3).
    """
    return [
        day,
        f"{close - 1}",
        f"{close}",
        f"{close + 1}",
        f"{close - 2}",
        "120000000",  # volume
    ]


def _tencent_row_new(
    day: str,
    open_: str = "3930.02",
    close: str = "3927.18",
    high: str = "3932.64",
    low: str = "3903.70",
    volume: str = "499525613.00",
    amount_wan: str = "99037192.42",
) -> list[object]:
    """11-field newfqkline/get day row with 成交额 (万元) at index 8.

    Mirrors the real payload shape from the issue:
    ``[date, open, close, high, low, volume, {}, 振幅, 成交额(万元), ...]``.
    """
    return [
        day,
        open_,
        close,
        high,
        low,
        volume,
        _MARKER,
        "1.03",           # 振幅 — index 7
        amount_wan,       # 成交额 in 万元 — index 8
        "0.00",           # index 9
        "0.00",           # index 10
    ]


def _tencent_payload(code: str, rows: list[list[str]]) -> dict[str, object]:
    """Shape of the Tencent fqkline/get response: data[code]["day"] = [[...]]. """
    return {"code": 0, "msg": "", "data": {code: {"day": rows}}}


def _kline_payload(code: str, klines: list[str]) -> dict[str, object]:
    return {"rc": 0, "data": {"code": code, "name": "stub", "klines": klines}}


def _clist_payload(diff: list[dict[str, object]]) -> dict[str, object]:
    return {"rc": 0, "data": {"total": len(diff), "diff": diff}}


def _pin_today(monkeypatch: pytest.MonkeyPatch, day: str = "2026-08-02") -> None:
    monkeypatch.setattr("fetch_index_daily._today", lambda: date.fromisoformat(day))
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())


def _env(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path / "no_dolt"))
    # COMPASS_CSV_DIR is already pointed at tmp_path by conftest._isolate_csv_dir.


def _read_rows(path: Path) -> list[dict[str, str]]:
    with open(path, newline="", encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


# ── C1: secid → Tencent symbol mapping ────────────────────────────────────────


class TestTencentCodeMapping:
    """C1: _tencent_code maps EastMoney secid to a Tencent symbol."""

    def test_sh_secid_maps_to_sh_prefix(self) -> None:
        """Test: 1.000001 (上证指数) → sh000001."""
        from fetch_index_daily import _tencent_code  # noqa: E402

        assert _tencent_code("1.000001") == "sh000001"

    def test_sz_secid_maps_to_sz_prefix(self) -> None:
        """Test: 0.399001 (深证成指) → sz399001."""
        from fetch_index_daily import _tencent_code  # noqa: E402

        assert _tencent_code("0.399001") == "sz399001"

    def test_all_official_indices_have_mappable_secid(self) -> None:
        """Test: every OFFICIAL_INDICES secid maps to a clean sh/sz+code symbol."""
        from fetch_index_daily import OFFICIAL_INDICES, _tencent_code  # noqa: E402

        for t in OFFICIAL_INDICES:
            mapped = _tencent_code(t["secid"])
            assert mapped.startswith(("sh", "sz")), f"{t['secid']} → {mapped}"
            assert mapped[2:] == t["code"].lower(), f"{t['secid']} → {mapped}"


# ── C2: full-history pagination via start_date advance ────────────────────────


class TestTencentPagination:
    """C2: _fetch_tencent_kline loops count=2000 + advances the end date."""

    def _days(self, n: int, start: str = "2026-01-01") -> list[str]:
        d = date.fromisoformat(start)
        return [(d + timedelta(days=i)).isoformat() for i in range(n)]

    async def test_merges_pages_and_advances_end_date(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Test: page 1 returns exactly 2000 rows (full), page 2 returns fewer —
        the helper must (a) request page 1 with empty end date, (b) advance the
        end date on the 2nd request, and (c) return ALL 2000+ rows merged."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        # page1 = latest 2000-bar window, page2 = strictly older window
        # (non-overlapping, matching the real Tencent end-date pagination).
        page1_days = self._days(_TENCENT_PAGE_SIZE, "2020-01-01")
        page2_days = self._days(50, "2018-01-01")
        page1 = [_tencent_row(d) for d in page1_days]
        page2 = [_tencent_row(d) for d in page2_days]

        calls: list[dict] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            calls.append(params or {})
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                parts = param.split(",")
                end = parts[3] if len(parts) > 3 else ""
                if end == "":
                    return StubResponse(json_data=_tencent_payload(code, page1))
                return StubResponse(json_data=_tencent_payload(code, page2))
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        secid = "1.000001"
        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), secid)

        # Full merge: 2000 + 50 rows returned.
        assert len(klines) == _TENCENT_PAGE_SIZE + 50, (
            f"must merge both pages, got {len(klines)} rows"
        )
        # Merged output must be chronological ascending and duplicate-free
        # (matches the live Tencent API verification).
        dates = [k.split(",")[0] for k in klines]
        assert dates == sorted(dates), "merged klines must be ascending by date"
        assert len(set(dates)) == len(dates), "merged klines must not duplicate dates"
        # First request: end date empty/absent (count ≤ 2000 caps the page).
        first = calls[0].get("param", "")
        assert first.startswith("sh000001,day,"), f"param {first!r}"
        assert ",,2000,qfq" in first or ",2000,qfq" in first, f"page1 count={first!r}"
        # Second request carries an advanced end date (a real date follows
        # the last fetched date, not an empty window).
        second = calls[1].get("param", "")
        assert second.startswith("sh000001,day,"), f"param2 {second!r}"
        parts2 = second.split(",")
        end2 = parts2[3] if len(parts2) > 3 else ""
        assert end2 != "", f"2nd page must advance end_date, got {second!r}"

    async def test_single_short_page_returns_without_second_request(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Test: when the first page is already short (<2000), no second request
        is made — full history fit in one page."""
        from fetch_index_daily import (
            Throttle,  # noqa: E402
            _fetch_tencent_kline,  # noqa: E402
        )

        rows = [_tencent_row(d) for d in self._days(100, "2026-01-01")]
        calls: list[dict] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            calls.append(params or {})
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                return StubResponse(
                    json_data=_tencent_payload(param.split(",")[0], rows)
                )
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(stub, Throttle(min_interval=0), "1.000001")
        assert len(klines) == 100
        dates = [k.split(",")[0] for k in klines]
        assert dates == sorted(dates), "single short page must stay ascending"
        assert len(set(dates)) == len(dates), "single short page must not duplicate dates"
        assert len(calls) == 1, "a short first page must not page again"


# ── R1/R2: newfqkline/get → 7-field CSV with NON-ZERO yuan amount ────────────


class TestTencentNewFqKlineAmount:
    """R1/R2: the fallback hits ``newfqkline/get`` and the 11-field day row's
    成交额 (万元, index 8) is converted to yuan (× 10000) in the emitted
    7-field CSV row, preserving the EastMoney field order.

    GREEN on current code: it uses ``newfqkline/get`` and writes real amount.
    """

    # Example from the issue:
    # ["2026-08-14","3930.02","3927.18","3932.64","3903.70","499525613.00",
    #  {},"1.03","99037192.42","0.00","0.00"] → amount = 99037192.42万 × 10000
    # = 990371924200.0 元.

    async def test_newfqkline_row_returns_nonzero_yuan_amount(
        self, make_stub_session
    ) -> None:
        """Test: an 11-field newfqkline day row (成交额 万元 at index 8) must come
        back as a 7-field kline whose amount (index 6) is yuan = 万元 × 10000.
        The request must target the NEW endpoint (newfqkline/get), not the old
        fqkline/get."""
        from fetch_index_daily import (  # noqa: E402
            Throttle,
            _fetch_tencent_kline,
        )

        row = _tencent_row_new("2026-08-14")  # amount_wan default 99037192.42
        stub = make_stub_session()
        urls: list[str] = []

        async def _get(url, params=None, headers=None):
            urls.append(url)
            assert "ifzq.gtimg.cn" in url, f"unexpected Tencent URL {url!r}"
            return StubResponse(
                json_data=_tencent_payload("sh000001", [row])
            )

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(
            stub, Throttle(min_interval=0), "1.000001"
        )
        assert len(klines) == 1, f"expected exactly one kline, got {klines!r}"
        # The fallback must hit the NEW endpoint.
        assert urls and urls[0].endswith("newfqkline/get"), (
            f"must call newfqkline/get, got {urls[0]!r}"
        )
        fields = klines[0].split(",")
        assert len(fields) == 7, (
            f"must emit 7-field CSV row date,open,close,high,low,volume,amount; "
            f"got {fields!r}"
        )
        # amount = 99037192.42 万元 × 10000 = 990371924200.0 yuan.
        assert float(fields[6]) == 990371924200.0, (
            f"amount must be 万元×10000 (yuan), got {fields[6]!r}"
        )

    async def test_newfqkline_row_preserves_eastmoney_field_order(
        self, make_stub_session
    ) -> None:
        """Test: the emitted 7-field row keeps the EastMoney column order
        date,open,close,high,low,volume,amount — the 11-field Tencent row must
        be mapped (not naively truncated), so close/high/low are not swapped."""
        from fetch_index_daily import (  # noqa: E402
            Throttle,
            _fetch_tencent_kline,
        )

        row = _tencent_row_new(
            "2026-08-14",
            open_="3930.02",
            close="3927.18",
            high="3932.64",
            low="3903.70",
            volume="499525613.00",
            amount_wan="99037192.42",
        )
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            return StubResponse(
                json_data=_tencent_payload("sh000001", [row])
            )

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(
            stub, Throttle(min_interval=0), "1.000001"
        )
        fields = klines[0].split(",")
        assert fields[0] == "2026-08-14", f"date field got {fields[0]!r}"
        assert fields[1] == "3930.02", f"open field got {fields[1]!r}"
        assert fields[2] == "3927.18", f"close field got {fields[2]!r}"
        assert fields[3] == "3932.64", f"high field got {fields[3]!r}"
        assert fields[4] == "3903.70", f"low field got {fields[4]!r}"
        assert fields[5] == "499525613.00", f"volume field got {fields[5]!r}"
        assert float(fields[6]) == 990371924200.0, f"amount field got {fields[6]!r}"

    async def test_missing_amount_field_degrades_gracefully(
        self, make_stub_session
    ) -> None:
        """R3: a day row with fewer than 9 fields (no 成交额) must still degrade
        gracefully — amount 0/empty, never a crash (fallback safety net)."""
        from fetch_index_daily import (  # noqa: E402
            Throttle,
            _fetch_tencent_kline,
        )

        row = _tencent_row("2026-08-14", 3000.0)  # legacy 6-field, no amount
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            return StubResponse(
                json_data=_tencent_payload("sh000001", [row])
            )

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(
            stub, Throttle(min_interval=0), "1.000001"
        )
        # Must not raise; if a row is produced its amount is 0/empty.
        assert isinstance(klines, list), f"expected a list, got {klines!r}"
        if klines:
            fields = klines[0].split(",")
            assert fields[6] in {"0", ""}, (
                f"a row without 成交额 must keep amount 0/empty, got {fields[6]!r}"
            )

    async def test_real_captured_newfqkline_row_maps_to_yuan(
        self, make_stub_session
    ) -> None:
        """Regression: a literal row captured from the live newfqkline/get API
        (2026-08-14 SZ399001) must map to the EastMoney 7-field order and
        non-zero yuan amount. Raw API values may differ slightly from the
        float-rounded Dolt/Parquet store; this pins the raw parse contract."""
        from fetch_index_daily import (  # noqa: E402
            Throttle,
            _fetch_tencent_kline,
        )

        real_row = [
            "2026-08-14",
            "14335.41",
            "14354.31",
            "14384.18",
            "14203.99",
            "642557319.00",
            {},
            "2.62",
            "115247130.12",
            "0.00",
            "0.00",
        ]
        stub = make_stub_session()
        urls: list[str] = []

        async def _get(url, params=None, headers=None):
            urls.append(url)
            return StubResponse(
                json_data=_tencent_payload("sz399001", [real_row])
            )

        stub.get = _get  # type: ignore[method-assign]

        klines = await _fetch_tencent_kline(
            stub, Throttle(min_interval=0), "0.399001"
        )
        assert klines is not None
        assert urls and urls[0].endswith("newfqkline/get"), (
            f"must call newfqkline/get, got {urls!r}"
        )
        fields = klines[0].split(",")
        assert len(fields) == 7, f"expected 7 fields, got {fields!r}"
        assert fields[0] == "2026-08-14"
        assert fields[1] == "14335.41"
        assert fields[2] == "14354.31"
        assert fields[3] == "14384.18"
        assert fields[4] == "14203.99"
        assert fields[5] == "642557319.00"
        assert float(fields[6]) == 115247130.12 * 10000.0


class TestTencentAmountYuanDirect:
    """Direct unit tests for the private ``_tencent_amount_yuan`` helper.

    These cover the degradation/formatting branches that are hard to reach
    through the full fetch path: overflow after ×10000, negative values,
    non-integral yuan output, and non-numeric/non-finite cells.
    """

    @staticmethod
    def _row(amount: object) -> list[object]:
        return [
            "2026-08-14",
            "3930.02",
            "3927.18",
            "3932.64",
            "3903.70",
            "499525613.00",
            {},
            "1.03",
            amount,
            "0.00",
            "0.00",
        ]

    def test_overflow_after_multiply_degrades_to_zero(self) -> None:
        from fetch_index_daily import _tencent_amount_yuan  # noqa: E402

        assert _tencent_amount_yuan(self._row("1e308")) == "0"

    def test_negative_amount_degrades_to_zero(self) -> None:
        from fetch_index_daily import _tencent_amount_yuan  # noqa: E402

        assert _tencent_amount_yuan(self._row("-500")) == "0"

    def test_non_integral_yuan_keeps_decimal(self) -> None:
        from fetch_index_daily import _tencent_amount_yuan  # noqa: E402

        assert _tencent_amount_yuan(self._row("0.00001")) == "0.1"

    def test_non_finite_literals_degrades_to_zero(self) -> None:
        from fetch_index_daily import _tencent_amount_yuan  # noqa: E402

        for bad in ("inf", "Infinity", "NaN"):
            assert _tencent_amount_yuan(self._row(bad)) == "0"

    def test_non_numeric_cells_degrades_to_zero(self) -> None:
        from fetch_index_daily import _tencent_amount_yuan  # noqa: E402

        for bad in (None, {}, [], "abc"):
            assert _tencent_amount_yuan(self._row(bad)) == "0"


# ── C3: EastMoney-fails → automatic Tencent fallback + amount default ────────


class TestTencentFallbackAndAmount:
    """C3: run() falls back to Tencent and writes NON-ZERO yuan amount for
    official rows (issue #286)."""

    async def test_official_falls_back_to_tencent_writes_nonzero_amount(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Test: ONLY 1.000001 is in the whitelist via monkeypatch, its EastMoney
        kline fails, so the run must fetch from Tencent (newfqkline/get) and
        write an official row whose amount (yuan = 成交额 万元 × 10000) is
        NON-ZERO and whose symbol is SH000001."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.OFFICIAL_INDICES",
            ({"secid": "1.000001", "code": "000001", "name": "上证指数"},),
        )

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                # One board makes boards non-empty so run() also writes
                # index_basic on this full run (official names ride along).
                return StubResponse(
                    json_data=_clist_payload([{"f12": "BK0475", "f14": "半导体"}])
                )
            if "ifzq.gtimg.cn" in url:  # Tencent → success (newfqkline row)
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                return StubResponse(
                    json_data=_tencent_payload(
                        code, [_tencent_row_new("2026-07-31")]
                    )
                )
            if "kline/get" in url:  # EastMoney push2his → fail (500)
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        # Official row landed in index_daily.csv with NON-ZERO yuan amount.
        rows = _read_rows(tmp_path / "index_daily.csv")
        official = [r for r in rows if r["symbol"] == "SH000001"]
        assert official, f"SH000001 row must be written from the Tencent fallback, got {rows!r}"
        assert float(official[0]["amount"]) == 990371924200.0, (
            f"amount must equal 成交额(万元)×10000; got {official[0]['amount']!r}"
        )
        assert official[0]["trade_date"] == "2026-07-31"
        # index_basic must have the official entry.
        basic = _read_rows(tmp_path / "index_basic.csv")
        assert any(r["symbol"] == "SH000001" for r in basic), (
            "index_basic must list the official index"
        )

    async def test_eastmoney_empty_falls_back_to_tencent(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Test: EastMoney returns EMPTY klines (non-error) for the official
        target → must still fall back to Tencent and produce rows."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)
        monkeypatch.setattr(
            "fetch_index_daily.OFFICIAL_INDICES",
            ({"secid": "0.399001", "code": "399001", "name": "深证成指"},),
        )

        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:  # Tencent success (newfqkline row)
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                return StubResponse(
                    json_data=_tencent_payload(
                        code, [_tencent_row_new("2026-07-31")]
                    )
                )
            if "kline/get" in url:  # EastMoney empty klines
                return StubResponse(json_data=_kline_payload("399001", []))
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            await run()

        rows = _read_rows(tmp_path / "index_daily.csv")
        official = [r for r in rows if r["symbol"] == "SZ399001"]
        assert official, "empty EastMoney must trigger Tencent fallback for SZ399001"
        assert float(official[0]["amount"]) == 990371924200.0, (
            f"amount must be 成交额(万元)×10000; got {official[0]['amount']!r}"
        )


# ── C4: Tencent segment protected by #277 fast-fail ───────────────────────────


class TestTencentFastFail:
    """C4: the Tencent segment is wired into #277 consecutive-failure fast-fail.

    The key #278-specific behavior: a target that FAILS on EastMoney but
    RECOVERS on Tencent must RESET the consecutive-failure counter (it is a
    success, not a failure). A target is only counted as a failure once BOTH
    sources are exhausted.
    """

    async def test_eastmoney_only_failures_recover_via_tencent(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Test: 6 official indices whitelisted; EastMoney fails for ALL of them
        but Tencent would succeed for the 5th. The #277 counter must only mark a
        target failed once Tencent was tried — so 4 EastMoney-only failures do
        NOT trigger, and the 5th succeeds on Tencent → no abort, all targets
        fetched. The current (no-tencent) implementation counts all 5 EastMoney
        failures consecutively and aborts → expected."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        # 5 official targets: first 4 fail on EastMoney, the 5th also fails on
        # EastMoney but SUCCEEDS on Tencent → it resets the streak.
        targets = tuple(
            {"secid": f"{'1' if i % 2 else '0'}.{100000 + i:06d}",
             "code": f"{100000 + i:06d}", "name": f"官方{i}"}
            for i in range(1, 7)
        )
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", targets)

        fetched_via_tencent: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                param = (params or {}).get("param", "")
                code = param.split(",")[0]
                # Tencent succeeds for all → every official target is recoverable,
                # so the consecutive-failure counter must never reach 5.
                fetched_via_tencent.append(code)
                return StubResponse(
                    json_data=_tencent_payload(code, [_tencent_row("2026-07-31")])
                )
            if "kline/get" in url:  # EastMoney fails for every official target
                return StubResponse(status_code=500, json_data={})
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        # No RuntimeError expected — every target recovers via Tencent.
        daily_path = None
        with patch("fetch_index_daily.AsyncSession", return_value=stub):
            daily_path = await run()

        assert daily_path.exists(), "recoverable targets must let the run complete"
        assert len(fetched_via_tencent) == len(targets), (
            "every official target must fall back to Tencent; "
            f"got {len(fetched_via_tencent)}/{len(targets)}"
        )
        # The 5th (recovered) target's row must be written → counter reset.
        rows = _read_rows(tmp_path / "index_daily.csv")
        fifth = targets[4]
        fifth_symbol = f"{'SH' if fifth['secid'].startswith('1.') else 'SZ'}{fifth['code']}"
        assert any(r["symbol"] == fifth_symbol for r in rows), (
            "a Tencent-recovered official target must persist its daily row"
        )

    async def test_five_doublefail_officials_terminate(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        """Test: when Tencent ALSO fails (double-fail) for 5 consecutive official
        targets, run() must abort (连续… RuntimeError) and never fetch the 6th —
        proving the Tencent segment is inside the #277 fast-fail boundary."""
        from fetch_index_daily import run  # noqa: E402

        _env(monkeypatch, tmp_path)
        _pin_today(monkeypatch)

        targets = tuple(
            {"secid": f"{'1' if i % 2 else '0'}.{100000 + i:06d}",
             "code": f"{100000 + i:06d}", "name": f"官方{i}"}
            for i in range(1, 7)
        )
        monkeypatch.setattr("fetch_index_daily.OFFICIAL_INDICES", targets)

        tracked: list[str] = []
        stub = make_stub_session()

        async def _get(url, params=None, headers=None):
            if "clist/get" in url:
                return StubResponse(json_data=_clist_payload([]))
            if "ifzq.gtimg.cn" in url:
                return StubResponse(status_code=500, json_data={})  # Tencent fail too
            if "kline/get" in url:
                secid = (params or {}).get("secid", "")
                tracked.append(secid)
                return StubResponse(status_code=500, json_data={})  # EastMoney fail
            return StubResponse(status_code=200, json_data={})

        stub.get = _get  # type: ignore[method-assign]

        with patch("fetch_index_daily.AsyncSession", return_value=stub), pytest.raises(RuntimeError) as e:
            await run()

        assert "连续" in str(e.value), f"must raise 连续…, got {str(e.value)!r}"
        # The 6th whitelisted target must never be requested.
        last = targets[-1]["secid"]
        assert last not in tracked, (
            "after 5 consecutive double-failures run() must not fetch the remaining target"
        )
        # The failing targets themselves must have been touched.
        assert targets[0]["secid"] in tracked, "the failing targets must be requested"
