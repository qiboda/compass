"""Requirement-acceptance tests for issue #287 — proxy_pool 试用 (proxy-pool-trial).

Contract under test (locked interface, ``collectors/check_proxy_pool.py``):

  Constants
    DEFAULT_API_URL         = "http://127.0.0.1:5010"
    DEFAULT_COUNT           = 15
    DEFAULT_TIMEOUT         = 10.0
    THS_LIST_URL            = "https://q.10jqka.com.cn/thshy/"
    THS_KLINE_URL_TEMPLATE  = "https://d.10jqka.com.cn/v4/line/bk_881101/01/{year}.js"

  ``@dataclass TrialResult``
    target: str, total: int, success: int, failures: list[str],
    success_rate: float, avg_elapsed: float

  ``get_proxies(api_url: str, count: int) -> list[str]``
    Fetch a proxy list from the proxy_pool API.

  ``fetch_with_proxy(url: str, proxy: str, timeout: float)
      -> tuple[bool, float, str | None]``
    Returns ``(是否成功, 耗时秒, 错误信息或 None)``.

  ``run_trial(url, count, api_url=DEFAULT_API_URL, timeout=DEFAULT_TIMEOUT)
      -> TrialResult``

  ``judge(result, success_threshold=0.5, max_avg_elapsed=5.0)
      -> tuple[bool, str]``
    Pass iff success_rate >= success_threshold AND avg_elapsed < max_avg_elapsed.

  ``main(argv: list[str] | None = None) -> int``
    Runs the full verification, prints a JSON summary (success_rate /
    avg_elapsed / verdict), returns 0 on completion, 1 on fatal setup errors
    (e.g. proxy_pool API unreachable).

STATUS: RED. The module does not exist yet, so every ``from
check_proxy_pool import ...`` fails with ModuleNotFoundError. Once the locked
contract is implemented, all tests here must pass.

Network isolation: no real HTTP is performed. Leaf functions are exercised by
monkeypatching both ``requests.get`` (project sync-requests convention) and
``curl_cffi.requests.get`` (the TLS-fingerprint client named in the plan) so
whichever sync client the implementation uses is intercepted; the
orchestration layers (``run_trial`` / ``main``) are exercised by
monkeypatching the contract-level functions themselves.
"""

import json
import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# The two sync HTTP clients the locked contract / plan allow. Leaf tests patch
# both so the implementation's actual client is always intercepted.
import curl_cffi.requests as _curl_requests  # noqa: E402
import requests as _requests  # noqa: E402

# ── Contract constants (mirror of the locked interface) ────────────────


class TestConstants:
    """The module must expose the exact locked constants."""

    def test_default_constants(self) -> None:
        import check_proxy_pool  # noqa: E402

        assert check_proxy_pool.DEFAULT_API_URL == "http://127.0.0.1:5010"
        assert check_proxy_pool.DEFAULT_COUNT == 15
        assert check_proxy_pool.DEFAULT_TIMEOUT == 10.0
        assert check_proxy_pool.THS_LIST_URL == "https://q.10jqka.com.cn/thshy/"
        assert (
            check_proxy_pool.THS_KLINE_URL_TEMPLATE
            == "https://d.10jqka.com.cn/v4/line/bk_881101/01/{year}.js"
        )


class TestTrialResult:
    """TrialResult must be a dataclass exposing exactly the locked fields."""

    def test_dataclass_fields(self) -> None:
        import check_proxy_pool  # noqa: E402

        result = check_proxy_pool.TrialResult(
            target="https://example.test",
            total=10,
            success=7,
            failures=["proxy A: timeout", "proxy B: 403"],
            success_rate=0.7,
            avg_elapsed=1.5,
        )
        assert result.target == "https://example.test"
        assert result.total == 10
        assert result.success == 7
        assert result.failures == ["proxy A: timeout", "proxy B: 403"]
        assert result.success_rate == 0.7
        assert result.avg_elapsed == 1.5


# ── get_proxies ────────────────────────────────────────────────────────


class TestGetProxies:
    """get_proxies must return the proxy list from the proxy_pool API."""

    @staticmethod
    def _stub_resp(*, status_code: int = 200, json_data: dict[str, object] | None = None) -> object:
        """A minimal fake requests.Response / curl_cffi Response."""

        data = json_data if json_data is not None else {}

        class _Resp:
            def __init__(self) -> None:
                self.status_code = status_code
                self._json = data

            def raise_for_status(self) -> None:
                if self.status_code >= 400:
                    raise RuntimeError(f"HTTP {self.status_code}")

            def json(self) -> dict[str, object]:
                return self._json

        return _Resp()

    @staticmethod
    def _patch_http_get(monkeypatch: pytest.MonkeyPatch, handler: object) -> None:
        """Intercept the sync GET on both allowed clients."""
        monkeypatch.setattr(_requests, "get", handler)
        monkeypatch.setattr(_curl_requests, "get", handler)

    def test_returns_proxy_list(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """Happy path: API returns a proxy list → returned unchanged."""
        import check_proxy_pool  # noqa: E402

        proxies = ["1.2.3.4:8080", "5.6.7.8:3128", "9.10.11.12:1080"]
        self._patch_http_get(
            monkeypatch,
            lambda url, timeout=None, **kw: self._stub_resp(json_data={"proxies": proxies}),
        )

        got = check_proxy_pool.get_proxies("http://127.0.0.1:5010", count=3)
        assert got == proxies

    def test_api_unreachable_returns_empty(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """API unreachable (network exception) → predictable empty list."""
        import check_proxy_pool  # noqa: E402

        def _boom(url: str, timeout: object = None, **kw: object) -> object:
            raise ConnectionError("proxy_pool API unreachable")

        self._patch_http_get(monkeypatch, _boom)

        got = check_proxy_pool.get_proxies("http://127.0.0.1:5010", count=5)
        assert got == []

    def test_empty_proxies_returns_empty(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """API reachable but returns no proxies → empty list."""
        import check_proxy_pool  # noqa: E402

        self._patch_http_get(
            monkeypatch,
            lambda url, timeout=None, **kw: self._stub_resp(json_data={"proxies": []}),
        )

        got = check_proxy_pool.get_proxies("http://127.0.0.1:5010", count=5)
        assert got == []


# ── fetch_with_proxy ───────────────────────────────────────────────────


class TestFetchWithProxy:
    """fetch_with_proxy must return (success, elapsed, error) per the contract."""

    @staticmethod
    def _stub_resp(*, status_code: int = 200) -> object:
        class _Resp:
            def __init__(self) -> None:
                self.status_code = status_code
                self.text = "ok"

            def raise_for_status(self) -> None:
                if self.status_code >= 400:
                    raise RuntimeError(f"HTTP {self.status_code}")

        return _Resp()

    @staticmethod
    def _patch_http_get(monkeypatch: pytest.MonkeyPatch, handler: object) -> None:
        """Intercept the sync GET on both allowed clients."""
        monkeypatch.setattr(_requests, "get", handler)
        monkeypatch.setattr(_curl_requests, "get", handler)

    def test_success_returns_true_zero_error(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """HTTP 200 → (True, elapsed>=0, None)."""
        import check_proxy_pool  # noqa: E402

        self._patch_http_get(
            monkeypatch,
            lambda url, timeout=None, proxies=None, **kw: self._stub_resp(status_code=200),
        )

        ok, elapsed, err = check_proxy_pool.fetch_with_proxy(
            "https://q.10jqka.com.cn/thshy/", "1.2.3.4:8080", timeout=10.0
        )
        assert ok is True
        assert elapsed >= 0
        assert err is None

    def test_http_non_200_returns_failure(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """HTTP 500 → (False, elapsed>=0, error message)."""
        import check_proxy_pool  # noqa: E402

        self._patch_http_get(
            monkeypatch,
            lambda url, timeout=None, proxies=None, **kw: self._stub_resp(status_code=500),
        )

        ok, elapsed, err = check_proxy_pool.fetch_with_proxy(
            "https://q.10jqka.com.cn/thshy/", "1.2.3.4:8080", timeout=10.0
        )
        assert ok is False
        assert elapsed >= 0
        assert isinstance(err, str) and err

    def test_network_exception_returns_failure(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """Network exception → (False, elapsed>=0, error message)."""
        import check_proxy_pool  # noqa: E402

        def _boom(url: str, timeout: object = None, proxies: object = None, **kw: object) -> object:
            raise TimeoutError("proxy timed out")

        self._patch_http_get(monkeypatch, _boom)

        ok, elapsed, err = check_proxy_pool.fetch_with_proxy(
            "https://q.10jqka.com.cn/thshy/", "1.2.3.4:8080", timeout=10.0
        )
        assert ok is False
        assert elapsed >= 0
        assert isinstance(err, str) and err


# ── run_trial ─────────────────────────────────────────────────────────


class TestRunTrial:
    """run_trial must orchestrate count requests and aggregate statistics."""

    URL = "https://q.10jqka.com.cn/thshy/"

    def test_counts_success_and_stats(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """With 4 proxies (2 success / 2 failure), run_trial must report
        total=4, success=2, success_rate=0.5, avg_elapsed=mean(elapsed),
        and failures containing the two error strings."""
        import check_proxy_pool  # noqa: E402

        proxies = ["p1", "p2", "p3", "p4"]
        # (success, elapsed, err) per proxy in order.
        outcomes = [
            (True, 0.5, None),
            (False, 1.0, "p2: timeout"),
            (True, 1.5, None),
            (False, 2.0, "p4: 403"),
        ]

        monkeypatch.setattr(check_proxy_pool, "get_proxies", lambda api_url, count: proxies[:count])
        monkeypatch.setattr(
            check_proxy_pool,
            "fetch_with_proxy",
            lambda url, proxy, timeout: outcomes[proxies.index(proxy)],
        )

        result = check_proxy_pool.run_trial(self.URL, count=4, api_url="http://x", timeout=1.0)

        assert isinstance(result, check_proxy_pool.TrialResult)
        assert result.target == self.URL
        assert result.total == 4
        assert result.success == 2
        assert result.failures == ["p2: timeout", "p4: 403"]
        assert result.success_rate == 0.5
        # avg_elapsed = (0.5 + 1.0 + 1.5 + 2.0) / 4 = 1.25
        assert result.avg_elapsed == pytest.approx(1.25)

    def test_all_success(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """All success → success_rate == 1.0, failures empty."""
        import check_proxy_pool  # noqa: E402

        monkeypatch.setattr(check_proxy_pool, "get_proxies", lambda api_url, count: ["p1", "p2"])
        monkeypatch.setattr(
            check_proxy_pool,
            "fetch_with_proxy",
            lambda url, proxy, timeout: (True, 0.25, None),
        )

        result = check_proxy_pool.run_trial(self.URL, count=2)
        assert result.success == 2
        assert result.total == 2
        assert result.success_rate == 1.0
        assert result.failures == []
        assert result.avg_elapsed == pytest.approx(0.25)

    def test_default_count_and_api_url(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ) -> None:
        """run_trial defaults must use DEFAULT_COUNT and DEFAULT_API_URL,
        and issue exactly DEFAULT_COUNT requests."""
        import check_proxy_pool  # noqa: E402

        calls: list[tuple[str, int]] = []

        def _get_proxies(api_url: str, count: int) -> list[str]:
            calls.append((api_url, count))
            return ["p"] * count

        monkeypatch.setattr(check_proxy_pool, "get_proxies", _get_proxies)
        monkeypatch.setattr(
            check_proxy_pool,
            "fetch_with_proxy",
            lambda url, proxy, timeout: (True, 0.1, None),
        )

        result = check_proxy_pool.run_trial(self.URL, count=check_proxy_pool.DEFAULT_COUNT)

        assert calls == [(check_proxy_pool.DEFAULT_API_URL, check_proxy_pool.DEFAULT_COUNT)]
        assert result.total == check_proxy_pool.DEFAULT_COUNT
        assert result.success == check_proxy_pool.DEFAULT_COUNT


# ── judge ─────────────────────────────────────────────────────────────


class TestJudge:
    """judge must implement the locked threshold semantics:
    pass iff success_rate >= 0.5 AND avg_elapsed < 5.0."""

    @staticmethod
    def _result(*, success_rate: float, avg_elapsed: float) -> object:
        import check_proxy_pool  # noqa: E402

        return check_proxy_pool.TrialResult(
            target="u",
            total=10,
            success=int(success_rate * 10),
            failures=[],
            success_rate=success_rate,
            avg_elapsed=avg_elapsed,
        )

    def test_pass_when_above_threshold(self) -> None:
        import check_proxy_pool  # noqa: E402

        ok, msg = check_proxy_pool.judge(self._result(success_rate=0.8, avg_elapsed=2.0))
        assert ok is True
        assert isinstance(msg, str) and msg

    def test_pass_on_success_rate_boundary_exactly_0_5(self) -> None:
        """Boundary: success_rate == 0.5 (>=) and avg_elapsed < 5 → pass."""
        import check_proxy_pool  # noqa: E402

        ok, _ = check_proxy_pool.judge(self._result(success_rate=0.5, avg_elapsed=4.99))
        assert ok is True

    def test_fail_when_avg_elapsed_boundary_exactly_5_0(self) -> None:
        """Boundary: avg_elapsed == 5.0 (must be strictly <) → fail even with
        success_rate >= 0.5."""
        import check_proxy_pool  # noqa: E402

        ok, msg = check_proxy_pool.judge(self._result(success_rate=0.8, avg_elapsed=5.0))
        assert ok is False
        assert isinstance(msg, str) and msg

    def test_fail_when_success_rate_below_0_5(self) -> None:
        import check_proxy_pool  # noqa: E402

        ok, msg = check_proxy_pool.judge(self._result(success_rate=0.49, avg_elapsed=1.0))
        assert ok is False
        assert isinstance(msg, str) and msg


# ── main ──────────────────────────────────────────────────────────────


class TestMain:
    """main must print a JSON summary and return 0 on completion, 1 on fatal
    setup errors (e.g. proxy_pool API unreachable)."""

    def test_happy_path_returns_zero_and_prints_json(
        self,
        monkeypatch: pytest.MonkeyPatch,
        capsys: pytest.CaptureFixture[str],
    ) -> None:
        """Completion → JSON on stdout (success_rate/avg_elapsed/verdict), rc 0."""
        import check_proxy_pool  # noqa: E402

        result = check_proxy_pool.TrialResult(
            target="https://q.10jqka.com.cn/thshy/",
            total=15,
            success=9,
            failures=["p: timeout"],
            success_rate=0.6,
            avg_elapsed=1.2,
        )
        monkeypatch.setattr(check_proxy_pool, "run_trial", Mock(return_value=result))
        monkeypatch.setattr(
            check_proxy_pool, "judge", Mock(return_value=(True, "PASS: 0.6 >= 0.5"))
        )

        rc = check_proxy_pool.main(argv=["--count", "15"])

        assert rc == 0
        out = capsys.readouterr().out
        payload = json.loads(out)
        assert "success_rate" in payload
        assert "avg_elapsed" in payload
        assert "verdict" in payload
        assert payload["success_rate"] == 0.6
        assert payload["avg_elapsed"] == 1.2

    def test_fatal_proxy_pool_unreachable_returns_one(
        self,
        monkeypatch: pytest.MonkeyPatch,
        capsys: pytest.CaptureFixture[str],
    ) -> None:
        """proxy_pool API unreachable (get_proxies raises) → rc 1, not 0."""
        import check_proxy_pool  # noqa: E402

        def _boom(api_url: str, count: int) -> list[str]:
            raise ConnectionError("cannot connect to proxy_pool API")

        monkeypatch.setattr(check_proxy_pool, "get_proxies", _boom)

        rc = check_proxy_pool.main([])
        assert rc == 1
        # A fatal setup error must NOT be reported as a successful PASS JSON.
        captured = capsys.readouterr()
        assert "verdict" not in captured.out or "PASS" not in captured.out.upper()
