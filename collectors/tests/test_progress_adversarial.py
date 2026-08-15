"""Adversarial tests for the ``Progress`` tracker (issue #267).

Attacks the declared functional commitments of ``collectors.common``:

* **Atomic write** — every ``update()`` is a tmp-file + ``os.replace`` so a
  concurrent ``read_progress()`` never sees a torn/partial JSON document.
* **Percent correctness** — ``percent = round(min(completed/total*100, 100), 2)``,
  capped at 100, ``None`` when ``total_items`` is falsy.
* **Error handling** — ``fail()`` / context-manager exit persist ``status=failed``
  and ``error``; corrupt/non-finite files read back as ``None``.
* **Path locality** — the progress file "lives alongside the raw CSVs" (i.e.
  inside ``csv_dir()``), never outside it.

These are adversarial tests: they probe edge/value/race conditions the plan does
not spell out.  Every test is achievable — a correct fix to the implementation
(or confirmation the existing design already defends the invariant) must make
the whole module GREEN.  No happy-path acceptance coverage here (that is the
requirement suite's job, see tests/test_common.py).
"""

from __future__ import annotations

import threading
import time

import pytest

from common import Progress, progress_path, read_progress

# ── Dimension 1: boundary values ──────────────────────────────────


def test_total_zero_reports_none_percent(monkeypatch, tmp_path) -> None:
    """total_items=0 is falsy -> percent must be None (locked behavior).

    The implementation guards ``percent`` behind ``if self.total_items:``, so an
    empty batch reports ``percent=None`` rather than 0.0 or 100.0.
    """
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    Progress("boundary_zero", total_items=0).finish()
    data = read_progress("boundary_zero")
    assert data is not None
    assert data["percent"] is None
    assert data["completed_items"] == 0
    assert data["status"] == "completed"


def test_completed_above_total_caps_percent_at_100(monkeypatch, tmp_path) -> None:
    """completed > total_items must never yield percent > 100.0."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("boundary_over", total_items=10)
    p.update(completed=25, fetched_rows=99)
    data = read_progress("boundary_over")
    assert data is not None
    assert data["completed_items"] == 25
    assert data["percent"] == 100.0


def test_finish_without_total_keeps_completed_items(monkeypatch, tmp_path) -> None:
    """total_items=None: finish() must not fabricate completed_items."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("boundary_untotaled")
    p.update(completed=3)
    p.finish(fetched_rows=5)
    data = read_progress("boundary_untotaled")
    assert data is not None
    assert data["total_items"] is None
    assert data["completed_items"] == 3
    assert data["percent"] is None


def test_negative_completed_never_writes_negative_percent(monkeypatch, tmp_path) -> None:
    """A negative completed count must not leak a negative percent.

    ``round(min(-50, 100), 2)`` currently yields -50.0 and persists it into the
    live-query file.  percent is a progress ratio and must stay in [0, 100].
    """
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("boundary_neg", total_items=10)
    p.update(completed=-5)
    data = read_progress("boundary_neg")
    assert data is not None
    assert data["percent"] == 0.0


def test_percent_after_fractional_cap_stays_round_2(monkeypatch, tmp_path) -> None:
    """Percent is rounded to 2 decimals and capped at exactly 100.0 for 100%."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("boundary_frac", total_items=7)
    p.update(completed=1)
    data = read_progress("boundary_frac")
    assert data["percent"] == 14.29
    p.finish()
    assert read_progress("boundary_frac")["percent"] == 100.0


# ── Dimension 2: error paths ─────────────────────────────────────


def test_fail_persists_then_finish_overrides_status(monkeypatch, tmp_path) -> None:
    """fail() records status=error; a later finish() overwrites to completed."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("err_override", total_items=4)
    p.fail("disk full")
    mid = read_progress("err_override")
    assert mid is not None
    assert mid["status"] == "failed"
    assert mid["error"] == "disk full"
    assert mid["message"] == "failed"
    p.finish(fetched_rows=4, message="retried ok")
    after = read_progress("err_override")
    assert after is not None
    assert after["status"] == "completed"
    assert after["error"] is None
    assert after["message"] == "retried ok"
    assert after["completed_items"] == 4


def test_fail_with_exception_object_persists_str_error(monkeypatch, tmp_path) -> None:
    """fail() accepts arbitrary exception-y args; the stored error is its str()."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    Progress("err_exc").fail(ValueError("HTTP 500"))
    data = read_progress("err_exc")
    assert data is not None
    assert data["status"] == "failed"
    assert data["error"] == "HTTP 500"


def test_exception_inside_context_marks_failed_and_rethrows(monkeypatch, tmp_path) -> None:
    """Context exit on exception must persist failed state AND re-raise."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    # Fail before the write completes: a stale running state must not survive.
    with pytest.raises(ValueError, match="partial payload"), Progress("err_ctx", total_items=3):
        Progress("err_ctx").update(completed=2)  # simulate a concurrent probe
        raise ValueError("partial payload")
    data = read_progress("err_ctx")
    assert data is not None
    assert data["status"] == "failed"


def test_update_recomputes_percent_on_midrun_total_change(monkeypatch, tmp_path) -> None:
    """Changing total_items midway must recompute percent from current completed."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("err_total", total_items=100)
    p.update(completed=50)
    assert read_progress("err_total")["percent"] == 50.0
    p.update(total_items=200)  # same completed, doubled total
    data = read_progress("err_total")
    assert data is not None
    assert data["total_items"] == 200
    assert data["percent"] == 25.0


def test_update_with_no_args_still_recommits_state(monkeypatch, tmp_path) -> None:
    """update() with no kwargs must still persist (bump updated_at, keep fields)."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("err_noop", total_items=5)
    p.update(completed=2)
    p.update()  # bare update
    data = read_progress("err_noop")
    assert data is not None
    assert data["completed_items"] == 2
    assert data["percent"] == 40.0


# ── Dimension 3: invalid input & structure ───────────────────────


def test_corrupt_json_read_returns_none(monkeypatch, tmp_path) -> None:
    """A truncated/corrupt .progress.json must read back as None, never crash."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    # Simulate a torn write left behind by a crashed producer: valid start, cut off.
    payload = '{\n  "name": "corrupt",\n  "status": "run'
    progress_path("corrupt").write_text(payload, encoding="utf-8")
    assert read_progress("corrupt") is None
    # Invalid JSON entirely.
    progress_path("corrupt2").write_text("{not json", encoding="utf-8")
    assert read_progress("corrupt2") is None


def test_output_csv_path_and_str_serialize_identically(monkeypatch, tmp_path) -> None:
    """output_csv as Path and as str must persist identically (a plain str)."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    Progress("csv_path", output_csv=tmp_path / "out.csv")
    Progress("csv_str", output_csv=str(tmp_path / "out.csv"))
    a = read_progress("csv_path")
    b = read_progress("csv_str")
    assert a is not None and b is not None
    assert a["output_csv"] == str(tmp_path / "out.csv")
    assert b["output_csv"] == str(tmp_path / "out.csv")
    assert a["output_csv"] == b["output_csv"]


def test_no_progress_file_escape_via_name_traversal(monkeypatch, tmp_path) -> None:
    """A name containing a path separator must not write outside csv_dir.

    ``progress_path(name)`` interpolates the name directly into the filename.
    ``Progress("../escape")`` therefore resolves to ``csv_dir/../escape.progress.json``
    and writes into the parent directory — violating the "lives alongside the
    raw CSVs" locality commitment.  A progress file must never appear outside
    ``csv_dir()``.
    """
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    # A name that escapes exactly one level up.
    escaped = tmp_path.parent / "escape.progress.json"
    Progress("../escape")
    assert not escaped.exists(), f"progress file escaped csv_dir(): wrote {escaped}"


def test_empty_ok_flag_nonempty(monkeypatch, tmp_path) -> None:
    """A name that must be allowed (plain identifier) stays inside csv_dir."""
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    Progress("plain_name", total_items=2)  # must not raise
    assert (tmp_path / "plain_name.progress.json").exists()


# ── Dimension 4: concurrency — atomic write, no torn read ────────


def test_concurrent_update_and_read_never_observes_torn_state(monkeypatch, tmp_path) -> None:
    """Readers racing writers must always see an internally consistent document.

    The declared commitment is atomic writes (tmp + os.replace).  A reader may
    read an old or a new snapshot, but never a half-written one: every non-None
    read must have fields that are mutually consistent (percent matches
    completed/total, status in the allowed set, valid timestamps).
    """
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    name = "race"
    writers = 4
    per_writer = 60
    errors: list[str] = []
    stop = threading.Event()

    def writer(seed: int) -> None:
        p = Progress(f"{name}_w{seed}", total_items=100)
        try:
            for i in range(per_writer):
                p.update(completed=i % 101, fetched_rows=i * 2, message=f"w{seed}-{i}")
            p.finish(fetched_rows=per_writer, message="writer-done")
        except Exception as exc:  # pragma: no cover - diagnostics
            errors.append(f"writer {seed}: {exc!r}")

    def reader() -> None:
        while not stop.is_set():
            for seed in range(writers):
                snap = read_progress(f"{name}_w{seed}")
                if snap is None:
                    continue
                c, t = snap["completed_items"], snap["total_items"]
                if not isinstance(t, int) or t is None:
                    # total only ever set via constructor (int) — absent means bad state
                    errors.append(f"reader: non-int/None total {t!r}")
                    continue
                expected = round(min(c / t * 100, 100.0), 2) if t else None
                if snap["percent"] != expected:
                    errors.append(
                        f"reader: percent {snap['percent']!r} != expected {expected!r} "
                        f"(completed={c}, total={t})"
                    )
                if snap["status"] not in ("running", "completed"):
                    errors.append(f"reader: bad status {snap['status']!r}")
                if not (snap["status"] == "completed" and c == t) and snap["status"] == "completed":
                    errors.append(f"reader: completed {c} but total {t} with status completed")

    threads = [threading.Thread(target=writer, args=(seed,)) for seed in range(writers)]
    r = threading.Thread(target=reader)
    for t in threads:
        t.start()
    r.start()
    for t in threads:
        t.join()
    stop.set()
    r.join(timeout=5)

    assert not errors, "torn/inconsistent progress reads:\n" + "\n".join(errors)


# ── Dimension 5: performance — bounded small-write pacing ────────


def test_repeated_updates_bounded_under_linear_write_cost(monkeypatch, tmp_path) -> None:
    """500 sequential atomic updates must finish quickly (no quadratic blow-up).

    Loose upper bound (10s) only guards against a catastrophic regression
    (e.g. an accidental re-scan of the whole batch per update); it must not
    make CI flaky on slow machines.
    """
    monkeypatch.setenv("COMPASS_CSV_DIR", str(tmp_path))
    p = Progress("perf", total_items=1000)
    start = time.monotonic()
    for i in range(500):
        p.update(completed=i, current_item=str(i))
    elapsed = time.monotonic() - start
    assert elapsed < 10.0, f"500 updates took {elapsed:.2f}s — linear-write regression"
    final = read_progress("perf")
    assert final is not None and final["completed_items"] == 499
