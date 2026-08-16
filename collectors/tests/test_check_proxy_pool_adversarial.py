"""Adversarial tests for ``collectors/check_proxy_pool.py`` (issue #287).

Attacks the locked interface contract of the proxy_pool trial script (the
remote API / THS probe + PASS/FAIL judgement).  The module does not exist yet,
so this suite is RED today via ``ModuleNotFoundError``; a correct implementation
that follows the locked contract below must turn it fully GREEN.

Locked semantics this suite pins down (adversarial reading of the contract):

* Constants / dataclass shape are exact (typo / signature drift is caught).
* ``count=0`` and an empty proxy list are *valid* boundaries: they yield a
  ``TrialResult`` with ``total == 0``, ``success == 0``, ``failures == []``,
  ``success_rate == 0.0`` and ``avg_elapsed == 0.0`` — no ``ZeroDivisionError``,
  and **zero** ``fetch_with_proxy`` calls.
* ``total`` counts proxies *actually used*: fewer than ``count`` → everything
  available; more than ``count`` → only the first ``count``; malformed proxy
  strings are passed to ``fetch_with_proxy`` verbatim (no filtering/crash).
* Failure paths keep elapsed stats: a failed attempt's elapsed time still
  contributes to ``avg_elapsed``, and an exception *raised* by
  ``fetch_with_proxy`` is captured as a failure (never propagated, never
  silently dropped).
* ``judge``: pass iff ``success_rate >= success_threshold`` **and**
  ``avg_elapsed < max_avg_elapsed`` (the plan's through-standard is strictly
  ``< 5s``, so exactly ``5.0`` is a *reject* while exactly ``0.5`` success is a
  *pass*).  Zero/negative thresholds are invalid → ``ValueError``; an empty
  trial is not a division-by-zero hazard and simply fails.
* ``main``: returns ``0`` whenever the run *completes* (including a failed
  trial — a low success rate is NOT a setup error) and prints a JSON summary;
  returns ``1`` only for fatal setup errors (proxy_pool API unreachable /
  ``get_proxies`` returning a non-list).

No real network is ever touched: ``get_proxies`` / ``fetch_with_proxy`` are
monkeypatched per test.
"""

from __future__ import annotations

import dataclasses
import inspect
import json
import math
import time

import check_proxy_pool as mod
import pytest
from check_proxy_pool import (
    DEFAULT_API_URL,
    DEFAULT_TIMEOUT,
    THS_LIST_URL,
    TrialResult,
    judge,
    main,
    run_trial,
)

# ── shared helpers ──────────────────────────────────────────────────────────


def _tr(**kw: object) -> TrialResult:
    """Build a TrialResult with sane defaults, overriding per-test fields."""
    defaults: dict[str, object] = {
        "target": THS_LIST_URL,
        "total": 0,
        "success": 0,
        "failures": [],
        "success_rate": 0.0,
        "avg_elapsed": 0.0,
    }
    defaults.update(kw)
    return TrialResult(**defaults)  # type: ignore[arg-type]


def _make_fetch(canned: dict[str, tuple[bool, float, str | None]]):
    """Return (fetch_stub, calls) — records every (url, proxy, timeout) call.

    Unknown proxies fall back to a cheap success tuple.
    """

    calls: list[tuple[str, str, float]] = []

    def fetch(url: str, proxy: str, timeout: float) -> tuple[bool, float, str | None]:
        calls.append((url, proxy, timeout))
        if proxy in canned:
            return canned[proxy]
        return (True, 0.5, None)

    return fetch, calls


def _install_runtime(
    monkeypatch: pytest.MonkeyPatch,
    proxies: list[str],
    fetch,
) -> None:
    monkeypatch.setattr(mod, "get_proxies", lambda api_url, count: list(proxies))
    monkeypatch.setattr(mod, "fetch_with_proxy", fetch)


def _extract_json(out: str) -> dict:
    """Pull the JSON-object summary out of stdout, tolerant of surrounding text."""
    start = out.find("{")
    end = out.rfind("}")
    assert start != -1 and end > start, f"no JSON object in output: {out!r}"
    return json.loads(out[start : end + 1])


# ── dimension 0: contract shape — constants / dataclass / signatures ──────


def test_locked_constants_exist_with_exact_values() -> None:
    assert mod.DEFAULT_API_URL == "http://127.0.0.1:5010"
    assert mod.DEFAULT_COUNT == 15
    assert mod.DEFAULT_TIMEOUT == 10.0
    assert mod.THS_LIST_URL == "https://q.10jqka.com.cn/thshy/"
    assert mod.THS_KLINE_URL_TEMPLATE == "https://d.10jqka.com.cn/v4/line/bk_881101/01/{year}.js"


def test_kline_template_formats_the_year() -> None:
    assert (
        mod.THS_KLINE_URL_TEMPLATE.format(year=2026)
        == "https://d.10jqka.com.cn/v4/line/bk_881101/01/2026.js"
    )


def test_trial_result_is_dataclass_with_locked_fields() -> None:
    assert dataclasses.is_dataclass(mod.TrialResult)
    names = {f.name for f in dataclasses.fields(mod.TrialResult)}
    assert names == {
        "target",
        "total",
        "success",
        "failures",
        "success_rate",
        "avg_elapsed",
    }


def test_run_trial_signature_defaults_match_contract() -> None:
    params = inspect.signature(mod.run_trial).parameters
    assert params["url"].default is inspect.Parameter.empty
    assert params["count"].default is inspect.Parameter.empty
    assert params["api_url"].default == DEFAULT_API_URL
    assert params["timeout"].default == DEFAULT_TIMEOUT


def test_judge_signature_defaults_match_contract() -> None:
    params = inspect.signature(mod.judge).parameters
    assert params["success_threshold"].default == 0.5
    assert params["max_avg_elapsed"].default == 5.0


def test_main_signature_argv_defaults_to_none() -> None:
    assert inspect.signature(mod.main).parameters["argv"].default is None


# ── dimension 1: boundary values (run_trial) ──────────────────────────────


def test_run_trial_count_zero_returns_empty_result(monkeypatch: pytest.MonkeyPatch) -> None:
    fetch, calls = _make_fetch({})
    _install_runtime(monkeypatch, ["p1", "p2"], fetch)
    r = run_trial("u", 0)
    assert r.total == 0
    assert r.success == 0
    assert r.failures == []
    assert r.success_rate == 0.0
    assert r.avg_elapsed == 0.0
    assert calls == []  # count=0 -> zero fetch attempts


def test_run_trial_count_one(monkeypatch: pytest.MonkeyPatch) -> None:
    fetch, calls = _make_fetch({"p1": (True, 0.7, None)})
    _install_runtime(monkeypatch, ["p1"], fetch)
    r = run_trial("u", 1)
    assert r.total == 1
    assert r.success == 1
    assert r.success_rate == 1.0
    assert r.failures == []
    assert len(calls) == 1


def test_run_trial_success_rate_exactly_half(monkeypatch: pytest.MonkeyPatch) -> None:
    fetch, _ = _make_fetch({"p1": (True, 0.3, None), "p2": (False, 0.2, "boom")})
    _install_runtime(monkeypatch, ["p1", "p2"], fetch)
    r = run_trial("u", 2)
    assert r.total == 2
    assert r.success == 1
    assert r.success_rate == pytest.approx(0.5)


def test_run_trial_all_success(monkeypatch: pytest.MonkeyPatch) -> None:
    fetch, _ = _make_fetch({"p1": (True, 0.4, None), "p2": (True, 0.8, None)})
    _install_runtime(monkeypatch, ["p1", "p2"], fetch)
    r = run_trial("u", 2)
    assert r.total == 2
    assert r.success == 2
    assert r.success_rate == 1.0
    assert r.failures == []
    assert r.avg_elapsed == pytest.approx(0.6)


def test_run_trial_empty_proxy_list_is_graceful(monkeypatch: pytest.MonkeyPatch) -> None:
    fetch, calls = _make_fetch({})
    _install_runtime(monkeypatch, [], fetch)
    r = run_trial("u", 5)  # asked for 5, got 0
    assert r.total == 0
    assert r.success == 0
    assert r.failures == []
    assert calls == []
    assert r.success_rate == 0.0 and r.avg_elapsed == 0.0  # no ZeroDivisionError


def test_run_trial_fewer_proxies_than_count_uses_available(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fetch, calls = _make_fetch({"p1": (True, 0.1, None), "p2": (True, 0.2, None)})
    _install_runtime(monkeypatch, ["p1", "p2"], fetch)
    r = run_trial("u", 5)
    assert r.total == 2
    assert len(calls) == 2


def test_run_trial_more_proxies_than_count_uses_only_first_count(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fetch, calls = _make_fetch(
        {"p1": (True, 0.1, None), "p2": (True, 0.1, None), "p3": (True, 0.1, None)}
    )
    _install_runtime(monkeypatch, ["p1", "p2", "p3", "p4"], fetch)
    r = run_trial("u", 2)
    assert r.total == 2
    assert len(calls) == 2
    assert {c[1] for c in calls} == {"p1", "p2"}  # extra proxies must NOT be probed


# ── dimension 2: error paths ──────────────────────────────────────────────


def test_run_trial_all_failures_aggregate_and_keep_elapsed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fetch, _ = _make_fetch({"p1": (False, 1.0, "conn refused"), "p2": (False, 2.0, "HTTP 503")})
    _install_runtime(monkeypatch, ["p1", "p2"], fetch)
    r = run_trial("u", 2)
    assert r.total == 2
    assert r.success == 0
    assert r.success_rate == 0.0
    assert set(r.failures) == {"conn refused", "HTTP 503"}
    # Failed attempts still contribute elapsed time -> not dropped from avg.
    assert r.avg_elapsed == pytest.approx(1.5)


def test_run_trial_captures_fetch_with_proxy_exception_as_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def fetch(url: str, proxy: str, timeout: float):
        if proxy == "p1":
            raise ConnectionError("conn refused")
        if proxy == "p3":
            raise TimeoutError("timed out")
        return (True, 1.0, None)

    monkeypatch.setattr(mod, "get_proxies", lambda api_url, count: ["p1", "p2", "p3"])
    monkeypatch.setattr(mod, "fetch_with_proxy", fetch)
    r = run_trial("u", 3)
    assert r.total == 3
    assert r.success == 1
    assert r.success_rate == pytest.approx(1 / 3)
    msgs = " ".join(r.failures).lower()
    assert "conn refused" in msgs and "timed out" in msgs
    assert math.isfinite(r.avg_elapsed) and r.avg_elapsed >= 0.0


def test_run_trial_failure_reason_aggregation_reasonable(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # 3 failures share one reason + 2 successes: the failure reason must be
    # captured (deduped to 1 entry or kept raw as 3) — never phantom, never 0.
    def fetch(url: str, proxy: str, timeout: float):
        if proxy in ("p1", "p3", "p4"):
            return (False, 0.1, "HTTP 503")
        return (True, 0.1, None)

    _install_runtime(monkeypatch, ["p1", "p2", "p3", "p4", "p5"], fetch)
    r = run_trial("u", 5)
    assert r.total == 5 and r.success == 2
    assert 1 <= len(r.failures) <= 3
    assert set(r.failures) == {"HTTP 503"}


def test_run_trial_forwards_url_api_url_count_timeout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    gp_calls: list[tuple[str, int]] = []
    fetched: list[tuple[str, str, float]] = []

    def gp(api_url: str, count: int) -> list[str]:
        gp_calls.append((api_url, count))
        return ["pA"]

    def fetch(url: str, proxy: str, timeout: float):
        fetched.append((url, proxy, timeout))
        return (True, 0.2, None)

    monkeypatch.setattr(mod, "get_proxies", gp)
    monkeypatch.setattr(mod, "fetch_with_proxy", fetch)
    r = run_trial(
        "https://target.example/x",
        1,
        api_url="http://api.example:5010",
        timeout=3.5,
    )
    assert r.target == "https://target.example/x"
    assert gp_calls == [("http://api.example:5010", 1)]
    assert fetched == [("https://target.example/x", "pA", 3.5)]


def test_run_trial_passes_malformed_proxy_strings_verbatim(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # Blank / garbage proxy strings must not crash the trial and must be handed
    # to fetch_with_proxy verbatim (the fetch layer owns failure semantics).
    proxies = ["http://1.2.3.4:8080", "  ", "", "not-a-url"]
    seen: list[str] = []

    def fetch(url: str, proxy: str, timeout: float):
        seen.append(proxy)
        return (False, 0.1, f"err:{proxy!r}")

    _install_runtime(monkeypatch, proxies, fetch)
    r = run_trial("u", 4)
    assert r.total == 4
    assert r.success == 0
    assert seen == proxies
    assert len(r.failures) == 4  # every malformed attempt counted, none dropped
    assert set(r.failures) == {f"err:{p!r}" for p in proxies}


# ── dimension 3: invalid input ────────────────────────────────────────────


@pytest.mark.parametrize("bad", ["nope", {"p": 1}, 42, None])
def test_run_trial_rejects_non_list_from_get_proxies(
    monkeypatch: pytest.MonkeyPatch,
    bad: object,
) -> None:
    monkeypatch.setattr(mod, "get_proxies", lambda api_url, count: bad)
    monkeypatch.setattr(mod, "fetch_with_proxy", lambda *a: (True, 0.1, None))
    with pytest.raises(ValueError):
        run_trial("u", 2)


def test_run_trial_negative_count_rejected(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(mod, "get_proxies", lambda api_url, count: ["p"])
    with pytest.raises(ValueError):
        run_trial("u", -1)


def test_run_trial_negative_timeout_rejected(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(mod, "get_proxies", lambda api_url, count: ["p"])
    with pytest.raises(ValueError):
        run_trial("u", 2, timeout=-1.0)


# ── dimension 4: judge boundary / invalid thresholds ──────────────────────


def test_judge_passes_when_success_rate_exactly_threshold() -> None:
    r = _tr(success_rate=0.5, avg_elapsed=1.0)
    ok, msg = judge(r, success_threshold=0.5, max_avg_elapsed=5.0)
    assert ok is True
    assert msg  # non-empty explanation


def test_judge_rejects_when_avg_elapsed_exactly_at_max() -> None:
    # Plan through-standard is strictly "< 5s": exactly 5.0 must be rejected.
    r = _tr(success_rate=0.9, avg_elapsed=5.0)
    ok, msg = judge(r, success_threshold=0.5, max_avg_elapsed=5.0)
    assert ok is False
    assert msg


def test_judge_rejects_low_success_rate() -> None:
    r = _tr(success_rate=0.4, avg_elapsed=1.0)
    assert judge(r, 0.5, 5.0)[0] is False


def test_judge_rejects_elapsed_just_over_max_with_high_success() -> None:
    r = _tr(success_rate=1.0, avg_elapsed=5.01)
    assert judge(r, 0.5, 5.0)[0] is False


def test_judge_passes_just_under_elapsed_max() -> None:
    r = _tr(success_rate=1.0, avg_elapsed=4.99)
    assert judge(r, 0.5, 5.0)[0] is True


def test_judge_passes_all_success() -> None:
    r = _tr(success_rate=1.0, avg_elapsed=0.1)
    assert judge(r, 0.5, 5.0)[0] is True


def test_judge_rejects_all_failure() -> None:
    r = _tr(success_rate=0.0, avg_elapsed=0.2)
    assert judge(r, 0.5, 5.0)[0] is False


def test_judge_handles_empty_trial_without_division_by_zero() -> None:
    r = _tr(total=0, success=0, success_rate=0.0, avg_elapsed=0.0)
    ok, msg = judge(r, 0.5, 5.0)
    assert ok is False  # nothing succeeded -> fail, must not crash
    assert msg


@pytest.mark.parametrize(
    "kw",
    [
        {"success_threshold": 0.0},
        {"success_threshold": -0.1},
        {"max_avg_elapsed": 0.0},
        {"max_avg_elapsed": -1.0},
    ],
)
def test_judge_rejects_zero_or_negative_thresholds(kw: dict[str, float]) -> None:
    r = _tr(success_rate=0.5, avg_elapsed=1.0)
    with pytest.raises(ValueError):
        judge(r, **kw)


# ── dimension 5: performance / resource (no accidental O(n²)) ─────────────


def test_run_trial_large_all_failure_run_is_linear(monkeypatch: pytest.MonkeyPatch) -> None:
    """Many distinct failures: exactly one fetch + one failure entry each.

    Guards against quadratic blow-up in failure aggregation / success-rate
    recomputation and against accidentally probing extra proxies.
    """
    n = 400
    proxies = [f"p{i}" for i in range(n)]

    def fetch(url: str, proxy: str, timeout: float):
        return (False, 0.01, f"err-{proxy}")

    _install_runtime(monkeypatch, proxies, fetch)
    start = time.monotonic()
    r = run_trial("u", n)
    elapsed = time.monotonic() - start
    assert r.total == n and r.success == 0
    assert len(r.failures) == n  # exactly one entry per attempt — no O(n²) duplication
    assert len(set(r.failures)) == n  # every distinct reason retained
    assert elapsed < 5.0, f"400-call all-failure trial took {elapsed:.2f}s — O(n^2)"


# ── dimension 6: main() exit-code discipline ──────────────────────────────


def test_main_success_returns_zero_and_prints_json(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    result = _tr(total=15, success=15, success_rate=0.8, avg_elapsed=0.4)
    monkeypatch.setattr(mod, "run_trial", lambda *a, **k: result)
    monkeypatch.setattr(mod, "judge", lambda *a, **k: (True, "PASS"))
    rc = main([])
    out = capsys.readouterr().out
    assert rc == 0
    payload = _extract_json(out)  # valid JSON summary printed
    assert isinstance(payload, dict)
    dumped = json.dumps(payload)
    assert "0.8" in dumped  # success_rate represented
    assert "0.4" in dumped  # avg_elapsed represented


def test_main_completed_failed_trial_still_returns_zero(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    # A trial that fails the judge is a *completed run* — NOT a setup error.
    # rc must be 0 and the JSON must carry the (bad) success_rate.
    result = _tr(
        total=10,
        success=2,
        failures=["HTTP 403", "conn refused"],
        success_rate=0.2,
        avg_elapsed=2.0,
    )
    monkeypatch.setattr(mod, "run_trial", lambda *a, **k: result)
    monkeypatch.setattr(mod, "judge", lambda *a, **k: (False, "FAIL"))
    rc = main([])
    out = capsys.readouterr().out
    assert rc == 0
    payload = _extract_json(out)
    assert "0.2" in json.dumps(payload)


def test_main_fatal_setup_error_returns_one(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def boom(*a, **k):
        raise ConnectionError("cannot connect to proxy_pool API")

    monkeypatch.setattr(mod, "run_trial", boom)
    rc = main([])
    assert rc == 1


def test_main_get_proxies_non_list_is_fatal(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    # Real run_trial path: get_proxies returns a non-list -> run_trial raises
    # ValueError -> main treats it as a fatal setup error -> 1.
    monkeypatch.setattr(mod, "get_proxies", lambda api_url, count: "not-a-list")
    monkeypatch.setattr(mod, "fetch_with_proxy", lambda *a: (True, 0.1, None))
    rc = main([])
    assert rc == 1
