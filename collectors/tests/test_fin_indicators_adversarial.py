"""Adversarial RED tests for issue #135 — fin_indicators incremental revision detection.

Attack surfaces (all against the plan's declared commitments, none against
imagined interfaces; every test is exercised BLACK-BOX through
``fetch_fin_indicators.main()`` / ``main._import_fin_indicators()`` so the
tests stay GREEN-able no matter what internal helper names T6/T7 choose):

1. UPDATE_DATE missing / empty on API rows  → filter construction + state max
   must not crash; empty/missing rows are SKIPPED in the max computation.
2. UPDATE_DATE abnormal formats ("2026/08/13", "2026-8-3", time-suffixed) →
   normalization ("date prefix") must not crash and must yield YYYY-MM-DD.
3. Multiple rows on the same (SECURITY_CODE, REPORTDATE) / same UPDATE_DATE
   boundary → CSV keep-LAST dedup (last value wins) and boundary-inclusive
   UPDATE_DATE >= anchor rows are all retained (none dropped).
4. Future / pre-stamped anchor (data_updates.last_updated > today) → clamped
   to today (no-update semantics), NEVER an infinite REPORTDATE enumeration
   loop, never an empty ``(UPDATE_DATE>='')`` filter.
5. 0-row run (nothing newer than anchor) → no crash, no CSV, anchor NOT
   advanced (keeps old value so late revisions are not skipped).
6. Anchor min-of-two-sources boundaries: data_updates.last_updated NULL/empty
   → that source is MISSING (fall back to state.json); state.json lacking
   ``last_update_date`` (old format) → that source is MISSING (fall back to
   data_updates). Both present → EARLIER of the two wins (never max).
7. Both anchor sources absent → anchor "" triggers FULL REPORTDATE
   enumeration (non-incremental semantics), never a degenerate UPDATE_DATE
   filter, and state.json still gets the double-write.
8. --report-name != RPT_LICO_FN_CPD → old REPORTDATE behavior preserved
   (no UPDATE_DATE anchor even when data_updates has a row) — regression guard.
9. Incremental mode IGNORES --years/--periods: a revision whose REPORTDATE
   falls OUTSIDE the years/periods window must still be fetched by the
   UPDATE_DATE anchor (revisions cross report periods).
10. Pagination boundary: UPDATE_DATE filter spans multiple pages (page > 1),
    all pages aggregated; total_pages capped at 500 (early-break verified),
    sortColumns must be UPDATE_DATE.

Plus the UPSERT contract (plan Must-have): revision overwrite must replace
ALL 35 non-PK value columns (DDL 37 - 2 PK) on the same PK — a column left
out silently mixes old+new values. Writing constraint PIN: SELECT-side
unique aliases + ODKU unqualified alias refs (SUCCESS); qualified source
refs on TRIM-wrapped columns and VALUES() (FAIL) — both forbidden forms.

RED/GREEN status per class (documented at each class):
- Strict RED: current production code (REPORTDATE enumeration + INSERT IGNORE
  + single-key state.json) fails the UPDATE_DATE-filter / double-write /
  keep-LAST / 35-col-overwrite assertions.
- Defensive (passes today, guards against T6/T7 regression): the parts of
  #5 (anchor-advance), #7 (fallback enumeration itself) and #8 (old behavior)
  that the current code already satisfies — their strict RED trigger is the
  UPDATE_DATE filter / state double-write assertion in the same test.

Required behavior contract (from plan, exercised black-box):
- incremental main() with a resolved anchor issues ONE fetch whose filter is
  ``(UPDATE_DATE>='<anchor>')`` with sortColumns=UPDATE_DATE.
- anchor = min(data_updates.last_updated (table_name='fin_indicators'),
  state.json last_update_date), read via COMPASS_DATA_DIR (env-aware, NOT the
  repo-relative _last_report_date path); NULL/empty/missing-key = source
  missing; both missing → "" → full REPORTDATE enumeration.
- state.json double-writes last_update_date (normalized date prefix; max over
  fetched rows, skipping empty/missing) + last_report_date; 0-row runs do not
  advance last_update_date.
- CSV write dedups per (SECURITY_CODE, REPORTDATE) keep-LAST.
- _import_fin_indicators UPSERTs (SELECT aliases + ODKU unqualified refs)
  over all 35 value columns.

These tests were authored in Wave 1 (RED) and must go GREEN after T6/T7/T8.
"""

from __future__ import annotations

import asyncio
import csv
import io
import json
import re
import subprocess
import sys
from collections.abc import Callable
from datetime import date, timedelta
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from conftest import StubResponse, StubSession  # noqa: E402

import fetch_fin_indicators  # noqa: E402

# ── Shared helpers ──────────────────────────────────────────────────────


class _RecordingStub(StubSession):
    """StubSession that records every (url, params) request for assertions."""

    def __init__(self, **kwargs: object) -> None:
        super().__init__(**kwargs)  # type: ignore[arg-type]
        self.calls: list[tuple[str, dict | None]] = []

    async def get(
        self, url: str, params: dict | None = None, headers: dict | None = None
    ) -> StubResponse:
        self.calls.append((url, params))
        return await super().get(url, params, headers)


async def _run_main(
    stub: _RecordingStub,
    argv: list[str],
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """Run fetch_fin_indicators.main() with a stub session from tmp_path.

    chdir to tmp_path so state.json/CSV land in the temp dir; stub
    asyncio.sleep so throttling never sleeps for real.
    """
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(asyncio, "sleep", AsyncMock())
    with (
        patch.object(fetch_fin_indicators, "AsyncSession", return_value=stub),
        patch.object(fetch_fin_indicators.sys, "argv", argv),
    ):
        await fetch_fin_indicators.main()


def _write_state(tmp_path: Path, report_name: str, state: dict) -> Path:
    path = tmp_path / f"{report_name}.state.json"
    path.write_text(json.dumps(state))
    return path


def _read_csv_rows(path: Path) -> list[dict[str, str]]:
    with open(path, encoding="utf-8-sig") as f:
        return list(csv.DictReader(f))


def _first_filter(stub: _RecordingStub) -> str:
    assert stub.calls, "expected at least one API request"
    params = stub.calls[0][1] or {}
    return str(params.get("filter", ""))


def _assert_anchor_filter(stub: _RecordingStub, expected_anchor: str) -> None:
    """Assert the FIRST request filters by UPDATE_DATE>=anchor (plan T6)."""
    f = _first_filter(stub)
    assert f"(UPDATE_DATE>='{expected_anchor}')" in f, (
        f"incremental mode must fetch by UPDATE_DATE>='{expected_anchor}', got filter {f!r}"
    )


# ── Dolt fixture ────────────────────────────────────────────────────────


@pytest.fixture
def dolt_env(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Callable[[str], str]]:
    """Init a temp Dolt repo and point COMPASS_DATA_DIR at it.

    Seeds stock_basic (symbol filter target) + data_updates (anchor source);
    fin_indicators is created lazily by _import_fin_indicators.
    """
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.email", "ci@compass.local"],
        capture_output=True, text=True,
    )
    subprocess.run(
        ["dolt", "config", "--global", "--add", "user.name", "CI"],
        capture_output=True, text=True,
    )
    init = subprocess.run(
        ["dolt", "--data-dir", str(tmp_path), "init"],
        capture_output=True, text=True,
    )
    assert init.returncode == 0, init.stderr

    def dolt_sql_csv(sql: str) -> str:
        return subprocess.run(
            ["dolt", "--data-dir", str(tmp_path), "sql", "-r", "csv", "-q", sql],
            capture_output=True, text=True,
        ).stdout

    dolt_sql_csv(
        "CREATE TABLE stock_basic (symbol VARCHAR(20) PRIMARY KEY); "
        "INSERT INTO stock_basic VALUES ('SZ000001')"
    )
    dolt_sql_csv(
        "CREATE TABLE data_updates (table_name VARCHAR(50) PRIMARY KEY, "
        "last_updated DATE, source VARCHAR(200), row_count INT, last_report_date DATE)"
    )
    monkeypatch.setenv("COMPASS_DATA_DIR", str(tmp_path))
    return tmp_path, dolt_sql_csv


def _seed_data_updates(
    dolt_sql_csv: Callable[[str], str], *, last_updated: str | None
) -> None:
    """Insert the fin_indicators data_updates row (NULL last_updated = missing)."""
    val = "NULL" if last_updated is None else f"'{last_updated}'"
    dolt_sql_csv(
        f"INSERT INTO data_updates (table_name, last_updated, source, row_count, "
        f"last_report_date) VALUES ('fin_indicators', {val}, 'test', 0, NULL)"
    )


# ═══════════════════════════════════════════════════════════════════════
# Attack 1 — UPDATE_DATE missing / empty on API rows
# ═══════════════════════════════════════════════════════════════════════


class TestUpdateDateMissingOrEmpty:
    """API rows lacking UPDATE_DATE (or empty) must not crash the incremental
    run; the state-file max computation must SKIP them (plan T6:
    "空 UPDATE_DATE 行跳过 max 计算")."""

    async def test_missing_and_empty_update_date_skipped_in_state_and_filter(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        # valid row FIRST so the CSV header includes UPDATE_DATE
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                         "REPORTDATE": "2024-12-31", "UPDATE_DATE": "2026-08-05",
                         "TOTAL_OPERATE_INCOME": "3"},
                        {"SECUCODE": "000002.SZ", "SECURITY_CODE": "000002",
                         "REPORTDATE": "2024-12-31", "TOTAL_OPERATE_INCOME": "1"},
                        {"SECUCODE": "000003.SZ", "SECURITY_CODE": "000003",
                         "REPORTDATE": "2024-12-31", "UPDATE_DATE": "",
                         "TOTAL_OPERATE_INCOME": "2"},
                    ],
                    "pages": 1,
                },
            }
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        _assert_anchor_filter(stub, "2026-08-03")
        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert state["last_update_date"] == "2026-08-05", (
            "state last_update_date must be the max over non-empty UPDATE_DATE "
            f"values (missing/empty rows skipped), got {state['last_update_date']!r}"
        )


# ═══════════════════════════════════════════════════════════════════════
# Attack 2 — UPDATE_DATE abnormal formats
# ═══════════════════════════════════════════════════════════════════════


class TestUpdateDateFormatAbnormal:
    """Malformed UPDATE_DATE ("2026/08/13", "2026-8-3") and time-suffixed
    values must not crash filter construction, comparison, or the state max;
    the stored last_update_date is NORMALIZED to a YYYY-MM-DD date prefix
    (plan T6: "UPDATE_DATE 规范化取日期前缀")."""

    async def test_malformed_update_date_normalized_no_crash(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                         "REPORTDATE": "2024-12-31", "UPDATE_DATE": "2026/08/13",
                         "TOTAL_OPERATE_INCOME": "1"},
                        {"SECUCODE": "000002.SZ", "SECURITY_CODE": "000002",
                         "REPORTDATE": "2024-12-31", "UPDATE_DATE": "2026-8-3",
                         "TOTAL_OPERATE_INCOME": "2"},
                        {"SECUCODE": "000003.SZ", "SECURITY_CODE": "000003",
                         "REPORTDATE": "2024-12-31",
                         "UPDATE_DATE": "2026-08-05 00:00:00",
                         "TOTAL_OPERATE_INCOME": "3"},
                    ],
                    "pages": 1,
                },
            }
        )
        # must NOT raise (a correct implementation normalizes or skips)
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        _assert_anchor_filter(stub, "2026-08-03")
        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert re.fullmatch(r"\d{4}-\d{2}-\d{2}", state["last_update_date"]), (
            "last_update_date must be a normalized YYYY-MM-DD date prefix, got "
            f"{state['last_update_date']!r}"
        )


# ═══════════════════════════════════════════════════════════════════════
# Attack 3 — same (SECURITY_CODE, REPORTDATE) duplicates + UPDATE_DATE boundary
# ═══════════════════════════════════════════════════════════════════════


class TestSameKeyBoundaryRows:
    """UPDATE_DATE>=anchor is INCLUSIVE: rows exactly at the boundary must all
    be retained. Duplicate (SECURITY_CODE, REPORTDATE) rows in one run must be
    deduped keep-LAST (last row wins) — plan Must-have "CSV 每 PK 唯一
    （整文件 keep-LAST 去重）"."""

    async def test_boundary_rows_included_and_dup_pk_keep_last(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        # all four rows sit exactly ON the anchor boundary (UPDATE_DATE >= anchor
        # must include them); the last two share a PK — keep-LAST keeps 999.
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                         "REPORTDATE": "2025-03-31", "UPDATE_DATE": "2026-08-03",
                         "TOTAL_OPERATE_INCOME": "100"},
                        {"SECUCODE": "000002.SZ", "SECURITY_CODE": "000002",
                         "REPORTDATE": "2025-06-30", "UPDATE_DATE": "2026-08-03",
                         "TOTAL_OPERATE_INCOME": "200"},
                        {"SECUCODE": "000003.SZ", "SECURITY_CODE": "000003",
                         "REPORTDATE": "2025-09-30", "UPDATE_DATE": "2026-08-03",
                         "TOTAL_OPERATE_INCOME": "300"},
                        {"SECUCODE": "000003.SZ", "SECURITY_CODE": "000003",
                         "REPORTDATE": "2025-09-30", "UPDATE_DATE": "2026-08-03",
                         "TOTAL_OPERATE_INCOME": "999"},
                    ],
                    "pages": 1,
                },
            }
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        rows = _read_csv_rows(tmp_path / "RPT_LICO_FN_CPD.csv")
        assert len(rows) == 3, (
            "boundary rows must all be retained and dup PK deduped to one "
            f"row each — expected 3 rows, got {len(rows)}"
        )
        dup = [r for r in rows if r["SECURITY_CODE"] == "000003"]
        assert len(dup) == 1, f"duplicate PK must dedup to one row, got {dup!r}"
        assert dup[0]["TOTAL_OPERATE_INCOME"] == "999", (
            "keep-LAST dedup must keep the LAST value, got "
            f"{dup[0]['TOTAL_OPERATE_INCOME']!r}"
        )


# ═══════════════════════════════════════════════════════════════════════
# Attack 4 — future / pre-stamped anchor
# ═══════════════════════════════════════════════════════════════════════


class TestFutureAnchorClamp:
    """data_updates.last_updated in the future (or a pre-stamped MAX update_date)
    must be treated as no-update (anchor clamped to today, plan T6: "锚点>今天
    或 NULL 时按无更新处理（锚点取今天）") — never an infinite REPORTDATE
    enumeration loop and never a degenerate ``(UPDATE_DATE>='')`` filter."""

    async def test_future_anchor_clamped_no_infinite_loop(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        dolt_dir_, dolt_sql_csv = dolt_env
        future = (date.today() + timedelta(days=1)).isoformat()
        _seed_data_updates(dolt_sql_csv, last_updated=future)
        # no state.json — only the future data_updates source exists
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        # no CSV / no state (0 rows) and never a full enumeration loop
        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()
        if stub.calls:
            today = date.today().isoformat()
            for _, raw_params in stub.calls:
                params = raw_params or {}
                f = str(params.get("filter", ""))
                assert "UPDATE_DATE" in f, (
                    "a future anchor must NOT fall back to REPORTDATE "
                    f"enumeration — got filter {f!r}"
                )
                assert f != "(UPDATE_DATE>='')", (
                    "a future anchor must never produce an empty-anchor "
                    "UPDATE_DATE filter"
                )
                assert f"(UPDATE_DATE>='{today}')" in f, (
                    f"future anchor must clamp to today ({today}), got {f!r}"
                )


# ═══════════════════════════════════════════════════════════════════════
# Attack 5 — 0-row run
# ═══════════════════════════════════════════════════════════════════════


class TestZeroRowRun:
    """Anchor with no newer data → fetch returns 0 rows: no crash, no CSV, and
    last_update_date is NOT advanced (plan T6: "0 行运行时不推进锚点——保留原
    锚点值，避免跳过晚到修订"). The anchor-advance assertions are defensive
    (current code also leaves state untouched on 0 rows); the strict RED
    trigger is the UPDATE_DATE filter assertion."""

    async def test_zero_rows_no_crash_no_anchor_advance(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-08-03")
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        # must NOT raise
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        _assert_anchor_filter(stub, "2026-08-03")
        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists(), (
            "0-row run must not create a CSV"
        )
        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert state["last_update_date"] == "2026-08-03", (
            "0-row run must NOT advance last_update_date (keeps the old anchor "
            "so late revisions are not skipped), got "
            f"{state['last_update_date']!r}"
        )


# ═══════════════════════════════════════════════════════════════════════
# Attack 6 — anchor min-of-two-sources boundaries
# ═══════════════════════════════════════════════════════════════════════


class TestAnchorMinDualSource:
    """anchor = min(data_updates.last_updated, state.json last_update_date);
    NULL/empty/missing-key source treated as MISSING (falls back to the other);
    when both present the EARLIER wins — never max (max overshoots and misses
    late revisions; plan TL;DR decision 1)."""

    async def test_min_state_earlier_than_data_updates(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-08-03")
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-07-01", "last_report_date": "2026-07-01"},
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [{"SECURITY_CODE": "000001"}], "pages": 1}}
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )
        # min(2026-08-03, 2026-07-01) = 2026-07-01 (state earlier)
        _assert_anchor_filter(stub, "2026-07-01")

    async def test_min_data_updates_earlier_than_state(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-07-01")
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [{"SECURITY_CODE": "000001"}], "pages": 1}}
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )
        # min(2026-07-01, 2026-08-03) = 2026-07-01 (data_updates earlier)
        _assert_anchor_filter(stub, "2026-07-01")

    async def test_null_last_updated_falls_back_to_state(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        """data_updates row EXISTS but last_updated is NULL → that source is
        MISSING → anchor comes from state.json alone (plan T2 branch ⑤)."""
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated=None)
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-07-01"},
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [{"SECURITY_CODE": "000001"}], "pages": 1}}
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )
        _assert_anchor_filter(stub, "2026-07-01")

    async def test_state_missing_key_falls_back_to_data_updates(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        """state.json in OLD format (only last_report_date, no last_update_date)
        → that source is MISSING → anchor comes from data_updates alone."""
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-08-03")
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_report_date": "2026-07-01"},
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [{"SECURITY_CODE": "000001"}], "pages": 1}}
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )
        _assert_anchor_filter(stub, "2026-08-03")


# ═══════════════════════════════════════════════════════════════════════
# Attack 7 — both anchor sources absent
# ═══════════════════════════════════════════════════════════════════════


class TestBothSourcesAbsent:
    """No data_updates row AND no state.json → anchor "" triggers FULL
    REPORTDATE enumeration (plan: "两源皆无 → 返回 \"\" 触发全量 REPORTDATE 枚举"),
    never a degenerate ``(UPDATE_DATE>='')`` filter. state.json still gets the
    double-write (last_update_date + last_report_date) after the run.

    The fallback-enumeration behavior itself is defensive (current code also
    enumerates); the strict RED trigger is the missing last_update_date key."""

    async def test_fallback_full_reportdate_enumeration_with_double_write(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        dolt_dir_, dolt_sql_csv = dolt_env
        # no data_updates row for fin_indicators, no state.json
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [{"SECURITY_CODE": "000001", "UPDATE_DATE": "2026-08-05"}],
                    "pages": 1,
                },
            }
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        assert stub.calls, "fallback must perform a full fetch (not skip)"
        for _, raw_params in stub.calls:
            params = raw_params or {}
            f = str(params.get("filter", ""))
            assert f.startswith("(REPORTDATE='"), (
                "no-anchor fallback must enumerate REPORTDATE periods, got "
                f"filter {f!r}"
            )
            assert "UPDATE_DATE" not in f, (
                "no-anchor fallback must NOT use a degenerate UPDATE_DATE "
                f"filter, got {f!r}"
            )
        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert state["last_update_date"] == "2026-08-05", (
            "state.json must double-write last_update_date after a full "
            f"fallback run, got {state.get('last_update_date')!r}"
        )


# ═══════════════════════════════════════════════════════════════════════
# Attack 8 — non-RPT_LICO_FN_CPD report name keeps OLD behavior
# ═══════════════════════════════════════════════════════════════════════


class TestNonCpdReportOldBehavior:
    """--report-name != RPT_LICO_FN_CPD must keep the OLD REPORTDATE-filter
    semantics even when data_updates has an anchor row (plan T6: "非
    RPT_LICO_FN_CPD 时保持旧行为（不启用 UPDATE_DATE 锚点）").

    DEFENSIVE/regression guard: current code already satisfies this; the test
    pins it so T6 cannot accidentally apply the UPDATE_DATE anchor to every
    report name."""

    async def test_non_cpd_report_keeps_reportdate_filter(
        self,
        make_stub_session,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-08-03")
        # old-format state.json (only last_report_date)
        _write_state(
            tmp_path,
            "RPT_DMSK_FN_BALANCE",
            {"last_report_date": "2026-01-01"},
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [{"SECURITY_CODE": "000001"}], "pages": 1}}
        )
        await _run_main(
            stub,
            [
                "fetch_fin_indicators.py",
                "--report-name", "RPT_DMSK_FN_BALANCE",
                "--incremental", "--years", "2026", "--periods", "FY",
            ],
            monkeypatch,
            tmp_path,
        )

        assert stub.calls, "non-CPD incremental run must still fetch"
        for _, raw_params in stub.calls:
            params = raw_params or {}
            f = str(params.get("filter", ""))
            assert f.startswith("(REPORTDATE='"), (
                "non-CPD report must keep REPORTDATE filters, got " f"{f!r}"
            )
            assert "UPDATE_DATE" not in f, (
                "non-CPD report must NOT use the UPDATE_DATE anchor, got " f"{f!r}"
            )
        state_path = tmp_path / "RPT_DMSK_FN_BALANCE.state.json"
        state = json.loads(state_path.read_text())
        assert "last_update_date" not in state, (
            "non-CPD report keeps the old single-key state format"
        )


# ═══════════════════════════════════════════════════════════════════════
# Attack 9 — incremental mode ignores --years/--periods
# ═══════════════════════════════════════════════════════════════════════


class TestIncrementalIgnoresYearsPeriods:
    """Incremental mode must IGNORE --years/--periods (plan T6: "增量模式下
    忽略 --years/--periods（锚点过滤跨报告期）"): a revision whose REPORTDATE is
    OUTSIDE the years/periods window must still be fetched by the UPDATE_DATE
    anchor — revisions cross report periods (五粮液 2025Q1 case)."""

    async def test_revision_outside_years_window_is_fetched(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        # REPORTDATE 2024-06-30 is OUTSIDE --years 2020 --periods FY
        # (window = [2020-12-31] only) but UPDATE_DATE >= anchor → must be fetched.
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                         "REPORTDATE": "2024-06-30", "UPDATE_DATE": "2026-08-05",
                         "TOTAL_OPERATE_INCOME": "170.86"},
                    ],
                    "pages": 1,
                },
            }
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2020", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"
        assert csv_path.exists(), (
            "incremental run must fetch the revision even though its REPORTDATE "
            "is outside the --years/--periods window (anchor crosses periods)"
        )
        rows = _read_csv_rows(csv_path)
        assert any(r["REPORTDATE"] == "2024-06-30" for r in rows), (
            "the out-of-window revision row must be present in the CSV, got "
            f"{rows!r}"
        )
        _assert_anchor_filter(stub, "2026-08-03")


# ═══════════════════════════════════════════════════════════════════════
# Attack 10 — pagination boundary (multi-page + 500 cap)
# ═══════════════════════════════════════════════════════════════════════


class TestPaginationBoundary:
    """UPDATE_DATE fetch must page like fetch_period: aggregate all pages
    (page > 1) and cap total_pages at 500 (plan T6: "500 页上限，分页日志").
    sortColumns must be UPDATE_DATE (plan T6)."""

    async def test_multiple_pages_aggregated(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        stub = _RecordingStub()
        call_count = [0]

        async def _get(url, params=None, headers=None):  # noqa: ANN001, ANN002, ANN003
            stub.calls.append((url, params))
            call_count[0] += 1
            if call_count[0] == 1:
                return StubResponse(json_data={
                    "success": True,
                    "result": {
                        "data": [
                            {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                             "REPORTDATE": "2025-03-31", "UPDATE_DATE": "2026-08-05"},
                            {"SECUCODE": "000002.SZ", "SECURITY_CODE": "000002",
                             "REPORTDATE": "2025-06-30", "UPDATE_DATE": "2026-08-05"},
                        ],
                        "pages": 2,
                    },
                })
            if call_count[0] == 2:
                return StubResponse(json_data={
                    "success": True,
                    "result": {
                        "data": [
                            {"SECUCODE": "000003.SZ", "SECURITY_CODE": "000003",
                             "REPORTDATE": "2025-09-30", "UPDATE_DATE": "2026-08-05"},
                        ],
                        "pages": 2,
                    },
                })
            return StubResponse(json_data={
                "success": True, "result": {"data": [], "pages": 2}
            })

        stub.get = _get  # type: ignore[method-assign]
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        # fetch_period-style pagination: page1 (pages=2) → page2 (1 row,
        # pages=2) → loop exits (page=3 > total_pages=2). Exactly 2 requests
        # aggregate 3 records; the empty third page is never requested.
        assert call_count[0] == 2, (
            "UPDATE_DATE filter spanning 2 pages must walk both pages — "
            f"expected 2 requests, got {call_count[0]}"
        )
        first_params = stub.calls[0][1] or {}
        assert first_params.get("sortColumns") == "UPDATE_DATE", (
            "incremental fetch must sort by UPDATE_DATE, got "
            f"{first_params.get('sortColumns')!r}"
        )
        _assert_anchor_filter(stub, "2026-08-03")
        rows = _read_csv_rows(tmp_path / "RPT_LICO_FN_CPD.csv")
        assert [r["SECURITY_CODE"] for r in rows] == ["000001", "000002", "000003"], (
            "records from both pages must be aggregated in order, got "
            f"{[r['SECURITY_CODE'] for r in rows]!r}"
        )

    async def test_total_pages_capped_at_500(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
    ) -> None:
        _write_state(
            tmp_path,
            "RPT_LICO_FN_CPD",
            {"last_update_date": "2026-08-03", "last_report_date": "2026-08-03"},
        )
        stub = _RecordingStub()
        call_count = [0]

        async def _get(url, params=None, headers=None):  # noqa: ANN001, ANN002, ANN003
            stub.calls.append((url, params))
            call_count[0] += 1
            if call_count[0] == 1:
                return StubResponse(json_data={
                    "success": True,
                    "result": {"data": [{"SECURITY_CODE": "000001"}], "pages": 1000},
                })
            return StubResponse(json_data={"success": True, "result": None})

        stub.get = _get  # type: ignore[method-assign]
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        # pages=1000 must be capped at 500 (and the empty second result breaks
        # the walk early — 2 calls, never a 1000-request loop)
        assert call_count[0] == 2, (
            "total_pages must be capped at 500 and the walk must break on an "
            f"empty result — expected 2 requests, got {call_count[0]}"
        )
        _assert_anchor_filter(stub, "2026-08-03")


# ═══════════════════════════════════════════════════════════════════════
# UPSERT contract — revision overwrite (35 value columns) + writing PIN
# ═══════════════════════════════════════════════════════════════════════

# 37-col API header (mirrors test_trim_imports._FIN_HEADER — the CSV header
# written by the fetch side; Dolt INSERT SELECT maps these into fin_indicators).
_FIN_HEADER = [
    "SECUCODE", "SECURITY_CODE", "REPORTDATE", "UPDATE_DATE", "NOTICE_DATE",
    "DATATYPE", "QDATE", "EITIME", "DATAYEAR", "DATEMMDD",
    "SECURITY_NAME_ABBR", "TRADE_MARKET", "TRADE_MARKET_CODE", "TRADE_MARKET_ZJG",
    "SECURITY_TYPE", "SECURITY_TYPE_CODE", "PUBLISHNAME", "BOARD_CODE",
    "BOARD_NAME", "ORI_BOARD_CODE", "ORG_CODE", "ISNEW", "BASIC_EPS",
    "DEDUCT_BASIC_EPS", "TOTAL_OPERATE_INCOME", "PARENT_NETPROFIT",
    "WEIGHTAVG_ROE", "BPS", "MGJYXJJE", "XSMLL", "YSTZ", "SJLTZ", "YSHZ",
    "SJLHZ", "ZXGXL", "ASSIGNDSCRPT", "PAYYEAR",
]

# API column → Dolt column mapping used by _import_fin_indicators' INSERT SELECT.
_API_TO_DOLT = {
    "UPDATE_DATE": "update_date",
    "NOTICE_DATE": "notice_date",
    "DATATYPE": "data_type",
    "QDATE": "qdate",
    "EITIME": "eitime",
    "DATAYEAR": "data_year",
    "DATEMMDD": "date_label",
    "SECUCODE": "secucode",
    "SECURITY_NAME_ABBR": "name",
    "TRADE_MARKET": "trade_market",
    "TRADE_MARKET_CODE": "trade_market_code",
    "TRADE_MARKET_ZJG": "trade_market_zjg",
    "SECURITY_TYPE": "security_type",
    "SECURITY_TYPE_CODE": "security_type_code",
    "PUBLISHNAME": "industry",
    "BOARD_CODE": "board_code",
    "BOARD_NAME": "board_name",
    "ORI_BOARD_CODE": "ori_board_code",
    "ORG_CODE": "org_code",
    "ISNEW": "is_new",
    "BASIC_EPS": "basic_eps",
    "DEDUCT_BASIC_EPS": "deduct_basic_eps",
    "TOTAL_OPERATE_INCOME": "revenue",
    "PARENT_NETPROFIT": "net_profit",
    "WEIGHTAVG_ROE": "roe",
    "BPS": "bps",
    "MGJYXJJE": "cash_flow_per_share",
    "XSMLL": "gross_margin",
    "YSTZ": "revenue_yoy",
    "SJLTZ": "net_profit_yoy",
    "YSHZ": "operating_profit_yoy",
    "SJLHZ": "net_profit_qoq",
    "ZXGXL": "shares_growth",
    "ASSIGNDSCRPT": "dividend_plan",
    "PAYYEAR": "dividend_year",
}


def _dolt_schema() -> tuple[list[str], dict[str, str]]:
    """Extract (value-column list, col→type map) from main.FIN_INDICATORS_DDL.

    Per plan T7 the 35-col list comes from main.py FIN_INDICATORS_DDL (the
    schema authority) — NOT from the implemented UPSERT SQL (no circular
    verification). PK columns (symbol, report_date) are excluded.
    """
    import main as main_mod  # noqa: PLC0415

    cols: list[str] = []
    types: dict[str, str] = {}
    for line in main_mod.FIN_INDICATORS_DDL.splitlines():
        line = line.strip()
        if not line or line.startswith(("CREATE", "PRIMARY", ")")):
            continue
        parts = line.split()
        cols.append(parts[0])
        types[parts[0]] = parts[1].lower()
    value_cols = [c for c in cols if c not in ("symbol", "report_date")]
    return value_cols, types


def _dolt_value_cols() -> list[str]:
    return _dolt_schema()[0]


def _fin_csv_row(markers: dict[str, str]) -> list[str]:
    """Build one 37-col API row from a dolt_col → value markers dict."""
    row = [""] * len(_FIN_HEADER)
    row[_FIN_HEADER.index("SECUCODE")] = "000001.SZ"
    row[_FIN_HEADER.index("SECURITY_CODE")] = "000001"
    row[_FIN_HEADER.index("REPORTDATE")] = "2025-03-31"
    for api, dolt in _API_TO_DOLT.items():
        row[_FIN_HEADER.index(api)] = markers[dolt]
    return row


class TestUpsertRevisionOverwrite:
    """The core #135 promise: importing a REVISED row with the same PK must
    OVERWRITE the old row (UPDATE_DATE moved forward), covering ALL 35 non-PK
    value columns — a column left out of the UPDATE silently mixes old+new
    values. Strict RED: current INSERT IGNORE keeps the old row."""

    def test_revision_overwrites_all_35_value_columns(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        import main as main_mod  # noqa: PLC0415

        dolt_dir_, dolt_sql_csv = dolt_env
        value_cols, col_types = _dolt_schema()
        assert len(value_cols) == 35, (
            f"DDL must declare exactly 35 non-PK value columns, got {len(value_cols)}"
        )

        old = {
            "update_date": "2025-04-26", "notice_date": "2025-04-26",
            "data_type": "OLD_DATA_TYPE", "qdate": "OLD_QDAT",
            "eitime": "2025-04-26 00:00:00", "data_year": "2024",
            "date_label": "OLD_LABEL", "secucode": "000001.SZ",
            "name": "OLD_NAME", "trade_market": "OLD_MARKET",
            "trade_market_code": "OLD_TMC", "trade_market_zjg": "OLD_ZJG",
            "security_type": "OLD_STYPE", "security_type_code": "OLD_STC",
            "industry": "OLD_INDUSTRY", "board_code": "OLD_BC",
            "board_name": "OLD_BNAME", "ori_board_code": "OLD_OBC",
            "org_code": "OLD_ORG", "is_new": "0",
            "basic_eps": "1.11", "deduct_basic_eps": "2.22",
            "revenue": "369.4", "net_profit": "3.33", "roe": "4.44",
            "bps": "5.55", "cash_flow_per_share": "6.66",
            "gross_margin": "7.77", "revenue_yoy": "8.88",
            "net_profit_yoy": "9.99", "operating_profit_yoy": "10.1",
            "net_profit_qoq": "11.11", "shares_growth": "12.12",
            "dividend_plan": "OLD_PLAN", "dividend_year": "OLD_YEAR",
        }
        new = {
            "update_date": "2026-04-30", "notice_date": "2026-04-30",
            "data_type": "NEW_DATA_TYPE", "qdate": "NEW_QDAT",
            "eitime": "2026-04-30 00:00:00", "data_year": "2026",
            "date_label": "NEW_LABEL", "secucode": "000001.SZ",
            "name": "五粮液", "trade_market": "NEW_MARKET",
            "trade_market_code": "NEW_TMC", "trade_market_zjg": "NEW_ZJG",
            "security_type": "NEW_STYPE", "security_type_code": "NEW_STC",
            "industry": "NEW_INDUSTRY", "board_code": "NEW_BC",
            "board_name": "NEW_BNAME", "ori_board_code": "NEW_OBC",
            "org_code": "NEW_ORG", "is_new": "1",
            "basic_eps": "21.11", "deduct_basic_eps": "22.22",
            "revenue": "170.86", "net_profit": "23.33", "roe": "24.44",
            "bps": "25.55", "cash_flow_per_share": "26.66",
            "gross_margin": "27.77", "revenue_yoy": "28.88",
            "net_profit_yoy": "29.99", "operating_profit_yoy": "30.3",
            "net_profit_qoq": "31.11", "shares_growth": "32.12",
            "dividend_plan": "NEW_PLAN", "dividend_year": "NEW_YEAR",
        }

        csv_path = tmp_path / "RPT_LICO_FN_CPD.csv"
        self._write_fin_csv(csv_path, old)
        main_mod._import_fin_indicators()
        # same PK, revised values (五粮液 2025Q1: revenue 369.4 → 170.86)
        self._write_fin_csv(csv_path, new)
        main_mod._import_fin_indicators()

        out = dolt_sql_csv(
            "SELECT " + ", ".join(value_cols) + " FROM fin_indicators "
            "WHERE symbol='SZ000001' AND report_date='2025-03-31'"
        )
        rows = list(csv.DictReader(io.StringIO(out)))
        assert len(rows) == 1, (
            "revision overwrite must NOT create a duplicate row — same PK "
            f"stays one row, got {len(rows)}"
        )
        row = rows[0]
        for col in value_cols:
            if "double" in col_types[col]:
                assert abs(float(row[col]) - float(new[col])) < 1e-9, (
                    f"revision must overwrite {col} (double): expected "
                    f"{new[col]}, got {row[col]!r}"
                )
            else:
                assert row[col] == new[col], (
                    f"revision must overwrite {col}: expected {new[col]!r}, "
                    f"got {row[col]!r} — a column left out of the UPSERT "
                    f"silently mixes old+new values"
                )

    @staticmethod
    def _write_fin_csv(path: Path, markers: dict[str, str]) -> None:
        with open(path, "w", newline="", encoding="utf-8-sig") as f:
            writer = csv.writer(f)
            writer.writerow(_FIN_HEADER)
            writer.writerow(_fin_csv_row(markers))

    def test_alias_ref_form_succeeds_forbidden_forms_fail(
        self, dolt_env: tuple[Path, Callable[[str], str]]
    ) -> None:
        """PIN the mandated UPSERT writing on real Dolt (plan T7, Round-2 实测):

        - SELECT-side unique aliases + ODKU unqualified alias refs → SUCCESS
          (full overwrite of numeric + TRIM-wrapped text columns)
        - qualified source-column ref ``_tmp_fin.<col>`` on a TRIM column → FAILS
        - ``VALUES()`` ODKU form → FAILS

        Wave-1-GREEN by design (pure SQL facts, no production code) — pins the
        writing constraint so the implementer cannot silently use a forbidden
        form; the strict RED revision-overwrite lives in
        test_revision_overwrites_all_35_value_columns.
        """
        dolt_dir_, dolt_sql_csv = dolt_env
        dolt_sql_csv(
            "CREATE TABLE fin_indicators (symbol VARCHAR(20) NOT NULL, "
            "report_date DATE NOT NULL, update_date DATE, name VARCHAR(100), "
            "revenue DOUBLE, PRIMARY KEY (symbol, report_date)); "
            "CREATE TABLE _tmp_fin (SECUCODE VARCHAR(20), SECURITY_CODE VARCHAR(20), "
            "REPORTDATE DATE, SECURITY_NAME_ABBR VARCHAR(100), "
            "TOTAL_OPERATE_INCOME DOUBLE); "
            "INSERT INTO _tmp_fin VALUES "
            "('000001.SZ', '000001', '2025-03-31', ' 五粮液 ', 170.86); "
            "INSERT INTO fin_indicators VALUES "
            "('SZ000001', '2025-03-31', '2025-04-26', '五粮液旧', 369.4)"
        )

        def run(sql: str) -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                ["dolt", "--data-dir", str(dolt_dir_), "sql", "-q", sql],
                capture_output=True, text=True,
            )

        alias_sql = (
            "INSERT INTO fin_indicators (symbol, report_date, update_date, name, revenue) "
            "SELECT CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE,'.',-1)), SECURITY_CODE) AS _sym, "
            "REPORTDATE AS _rpt, CURDATE() AS _upd, TRIM(SECURITY_NAME_ABBR) AS _nm, "
            "TOTAL_OPERATE_INCOME AS _rev FROM _tmp_fin "
            "ON DUPLICATE KEY UPDATE update_date=_upd, name=_nm, revenue=_rev"
        )
        r = run(alias_sql)
        assert r.returncode == 0, f"alias-ref UPSERT must succeed: {r.stderr}"
        row = list(csv.DictReader(io.StringIO(
            dolt_sql_csv("SELECT update_date, name, revenue FROM fin_indicators "
                         "WHERE symbol='SZ000001'")
        )))[0]
        assert row["name"] == "五粮液", (
            f"TRIM-wrapped text column must be overwritten via alias ref, got {row['name']!r}"
        )
        assert abs(float(row["revenue"]) - 170.86) < 1e-9, (
            f"numeric column must be overwritten via alias ref, got {row['revenue']!r}"
        )

        qualified_sql = (
            "INSERT INTO fin_indicators (symbol, report_date, update_date, name, revenue) "
            "SELECT CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE,'.',-1)), SECURITY_CODE) AS _sym, "
            "REPORTDATE AS _rpt, CURDATE() AS _upd, TRIM(SECURITY_NAME_ABBR) AS _nm, "
            "TOTAL_OPERATE_INCOME AS _rev FROM _tmp_fin "
            "ON DUPLICATE KEY UPDATE update_date=_upd, "
            "name=_tmp_fin.SECURITY_NAME_ABBR, revenue=_rev"
        )
        r = run(qualified_sql)
        assert r.returncode != 0, (
            "qualified source-column ref on a TRIM column must FAIL on Dolt "
            "(plan: `table _tmp_fin does not have column`)"
        )

        values_sql = (
            "INSERT INTO fin_indicators (symbol, report_date, update_date, name, revenue) "
            "SELECT CONCAT(UPPER(SUBSTRING_INDEX(SECUCODE,'.',-1)), SECURITY_CODE) AS _sym, "
            "REPORTDATE AS _rpt, CURDATE() AS _upd, TRIM(SECURITY_NAME_ABBR) AS _nm, "
            "TOTAL_OPERATE_INCOME AS _rev FROM _tmp_fin "
            "ON DUPLICATE KEY UPDATE update_date=_upd, name=VALUES(name), revenue=_rev"
        )
        r = run(values_sql)
        assert r.returncode != 0, "VALUES() ODKU form must FAIL on Dolt (plan: `__new_ins`)"


# ═══════════════════════════════════════════════════════════════════════
# T9 — coverage 补测: fetch_fin_indicators 防御性分支（429/异常重试、
# 无数据/API 错误 break、main 级 fallback、损坏 state.json）
# ═══════════════════════════════════════════════════════════════════════
# 均为 GREEN 补覆盖（非 RED）：断言"不崩溃 + 正确 fallback 行为"。
# 直接调用 fetch_by_update_date / _update_anchor / main()，不触碰真实网络。


class TestT9FetchByUpdateDateRetry:
    """fetch_by_update_date 的 429 / 异常重试分支（镜像 TestFetchPeriod 的
    fetch_period 重试用例，覆盖 326-329 / 335-344 行）。"""

    async def test_429_retry_then_success(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """① 首次 429 → 等待 15-20s（mock sleep）→ 重试成功。"""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            if call_count[0] == 1:
                return StubResponse(status_code=429)
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [{"code": "000001"}], "pages": 1},
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_by_update_date(
            stub, t, "RPT_LICO_FN_CPD", "2026-08-03"
        )
        assert len(records) == 1
        assert records[0]["code"] == "000001"
        assert call_count[0] >= 2
        assert mock_sleep.call_count >= 3  # throttle + 429 wait + more throttle

    async def test_exception_retry_then_success(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """② 首次抛异常 → 指数退避重试 → 第二次成功。"""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            if call_count[0] == 1:
                raise RuntimeError("transient failure")
            return StubResponse(
                json_data={
                    "success": True,
                    "result": {"data": [{"code": "000002"}], "pages": 1},
                }
            )

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_by_update_date(
            stub, t, "RPT_LICO_FN_CPD", "2026-08-03"
        )
        assert len(records) == 1
        assert call_count[0] == 2

    async def test_retries_exhausted_raises(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """③ EM_MAX_RETRIES 次全部失败 → 最后一次 retry 后异常向外传播。"""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            raise RuntimeError("persistent failure")

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        t = fetch_fin_indicators.Throttle(min_interval=0)
        with pytest.raises(RuntimeError, match="persistent failure"):
            await fetch_fin_indicators.fetch_by_update_date(
                stub, t, "RPT_LICO_FN_CPD", "2026-08-03"
            )
        assert call_count[0] == fetch_fin_indicators.EM_MAX_RETRIES


class TestT9FetchByUpdateDateBreak:
    """fetch_by_update_date 的 break 防御分支（覆盖 347-348 / 350-351 行）。"""

    async def test_all_429_no_data_break(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """④ 持续 429：重试耗尽后 data 仍为 None → "No data returned" break，不崩溃。"""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        call_count = [0]

        async def _get(*args, **kwargs):  # noqa: ANN002, ANN003
            call_count[0] += 1
            return StubResponse(status_code=429)

        stub = make_stub_session()
        stub.get = _get  # type: ignore[method-assign]

        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_by_update_date(
            stub, t, "RPT_LICO_FN_CPD", "2026-08-03"
        )
        assert records == []
        assert call_count[0] == fetch_fin_indicators.EM_MAX_RETRIES

    async def test_api_error_break(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """⑤ success=False → "API error" break，不崩溃。"""
        mock_sleep = AsyncMock()
        monkeypatch.setattr(asyncio, "sleep", mock_sleep)

        stub = make_stub_session(json_data={"success": False, "message": "boom"})
        t = fetch_fin_indicators.Throttle(min_interval=0)
        records = await fetch_fin_indicators.fetch_by_update_date(
            stub, t, "RPT_LICO_FN_CPD", "2026-08-03"
        )
        assert records == []


class TestT9MainFallbacks:
    """main() 级 fallback 分支（覆盖 470 / 472-473 / 508-510 / 574-575 行）。"""

    async def test_incremental_fetch_exception_propagates(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
        capsys,
    ) -> None:
        """⑥ 增量 fetch 抛异常 → FAILED 打印后向上传播，不伪装成空窗口。"""
        monkeypatch.setattr(
            fetch_fin_indicators, "_update_anchor", lambda *a, **k: "2026-01-01"
        )
        stub = _RecordingStub(exc=RuntimeError("boom"))
        with pytest.raises(RuntimeError, match="boom"):
            await _run_main(
                stub,
                ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
                monkeypatch,
                tmp_path,
            )

        assert "FAILED: boom" in capsys.readouterr().err
        assert not (tmp_path / "RPT_LICO_FN_CPD.csv").exists()
        assert not (tmp_path / "RPT_LICO_FN_CPD.state.json").exists()

    async def test_non_cpd_incremental_no_new_periods_returns(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
        capsys,
    ) -> None:
        """⑦ 非 CPD 增量：无 prior data 且周期窗口为空 → 打印后 return，零请求。

        覆盖 "No prior data found"（L470）与 "No new report periods to
        fetch."（L472-473）两个防御分支。
        """
        monkeypatch.setattr(
            fetch_fin_indicators, "_last_report_date", lambda *a, **k: ""
        )
        stub = _RecordingStub(
            json_data={"success": True, "result": {"data": [], "pages": 1}}
        )
        await _run_main(
            stub,
            [
                "fetch_fin_indicators.py",
                "--report-name", "RPT_T9_COV",
                "--incremental", "--years", "2020", "--periods", "BOGUS",
            ],
            monkeypatch,
            tmp_path,
        )

        err = capsys.readouterr().err
        assert "No prior data found, fetching full history." in err
        assert "No new report periods to fetch." in err
        assert stub.calls == [], "empty window must return before any API request"
        assert not (tmp_path / "RPT_T9_COV.csv").exists()

    async def test_incremental_corrupt_state_json_preserves_anchor(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        """⑧ 增量 CPD：全部行缺 UPDATE_DATE 且 state.json 损坏 → 读 prev 时
        JSONDecodeError → prev="" → 不崩溃，state.json 以合法 JSON 重写。"""
        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-08-03")
        state_path = tmp_path / "RPT_LICO_FN_CPD.state.json"
        state_path.write_text("{corrupt json!!")  # 非法 JSON

        # 行有 REPORTDATE 但无 UPDATE_DATE → max_update_date 为空 → 走 prev fallback
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                         "REPORTDATE": "2026-06-30", "TOTAL_OPERATE_INCOME": "1"},
                    ],
                    "pages": 1,
                },
            }
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        assert (tmp_path / "RPT_LICO_FN_CPD.csv").exists(), (
            "rows without UPDATE_DATE must still be written to CSV"
        )
        state = json.loads(state_path.read_text())  # 重写后必须是合法 JSON
        assert state["last_update_date"] == "", (
            "corrupt state must fall back to empty previous anchor (no crash), "
            f"got {state['last_update_date']!r}"
        )
        assert state["last_report_date"] == "2026-06-30"

    async def test_future_update_date_clamped_to_today(
        self, make_stub_session, monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
        dolt_env: tuple[Path, Callable[[str], str]],
    ) -> None:
        """⑨ 抓取行的 UPDATE_DATE 在未来 → state 写入前钳制到今天（不写未来值）。"""
        from datetime import date

        dolt_dir_, dolt_sql_csv = dolt_env
        _seed_data_updates(dolt_sql_csv, last_updated="2026-08-03")
        stub = _RecordingStub(
            json_data={
                "success": True,
                "result": {
                    "data": [
                        {"SECUCODE": "000001.SZ", "SECURITY_CODE": "000001",
                         "REPORTDATE": "2026-06-30", "UPDATE_DATE": "2099-01-01",
                         "TOTAL_OPERATE_INCOME": "1"},
                    ],
                    "pages": 1,
                },
            }
        )
        await _run_main(
            stub,
            ["fetch_fin_indicators.py", "--incremental", "--years", "2026", "--periods", "FY"],
            monkeypatch,
            tmp_path,
        )

        state = json.loads((tmp_path / "RPT_LICO_FN_CPD.state.json").read_text())
        assert state["last_update_date"] == date.today().isoformat(), (
            "future UPDATE_DATE must be clamped to today in state, got "
            f"{state['last_update_date']!r}"
        )


class TestT9UnitBranches:
    """单函数防御分支（覆盖 128 / 165-166 行）。"""

    def test_normalize_update_date_unparseable_returns_none(self) -> None:
        """非空但无法解析的 UPDATE_DATE → None（不崩溃、不抛异常）。"""
        assert fetch_fin_indicators._normalize_update_date("not-a-date") is None
        assert fetch_fin_indicators._normalize_update_date("2026") is None
        assert fetch_fin_indicators._normalize_update_date("") is None
        assert fetch_fin_indicators._normalize_update_date(None) is None

    def test_update_anchor_corrupt_state_json_falls_back(
        self, dolt_env: tuple[Path, Callable[[str], str]], tmp_path: Path
    ) -> None:
        """_update_anchor 读损坏 state.json → JSONDecodeError → 视为无 state 源。"""
        _ = dolt_env  # data_updates 表存在但无 fin_indicators 行 → 单源缺失
        state = tmp_path / "RPT_LICO_FN_CPD.state.json"
        state.write_text("{corrupt json!!")
        assert fetch_fin_indicators._update_anchor("RPT_LICO_FN_CPD", state) == ""
