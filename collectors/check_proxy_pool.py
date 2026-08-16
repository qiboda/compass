"""Standalone proxy_pool trial harness for the THS (10jqka) board endpoints.

This module is intentionally independent from the existing collectors: it
verifies whether a locally deployed ``jhao104/proxy_pool`` instance can supply
working proxies for ``q.10jqka.com.cn`` (industry list) and
``d.10jqka.com.cn`` (board kline).  It never modifies collector production
code; a positive result is a prerequisite for a later integration trial.

The HTTP client is ``curl_cffi`` with the same TLS impersonation used by the
collectors (``chrome142``), so the probe reflects production request
fingerprinting as closely as possible.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Literal

from curl_cffi import requests as _curl_requests

DEFAULT_API_URL = "http://127.0.0.1:5010"
DEFAULT_COUNT = 15
DEFAULT_TIMEOUT = 10.0
THS_LIST_URL = "https://q.10jqka.com.cn/thshy/"
THS_KLINE_URL_TEMPLATE = "https://d.10jqka.com.cn/v4/line/bk_881101/01/{year}.js"

_IMPERSONATE: Literal["chrome142"] = "chrome142"


@dataclass
class TrialResult:
    """Aggregated outcome of one probe target."""

    target: str
    total: int
    success: int
    failures: list[str] = field(default_factory=list)
    success_rate: float = 0.0
    avg_elapsed: float = 0.0


def _proxy_pool_all_url(api_url: str) -> str:
    """Return the proxy_pool ``/all/`` endpoint URL for an API base URL.

    The trailing slash avoids the Flask redirect that the proxy_pool API
    returns for ``/all``.
    """
    return api_url.rstrip("/") + "/all/"


def get_proxies(api_url: str, count: int) -> list[str]:
    """Fetch up to ``count`` proxy strings from the proxy_pool API.

    A network/API failure is deliberately non-fatal at this layer: the caller
    can still run a trial with zero proxies and report a clear "no proxies"
    outcome.

    The proxy_pool API returns a JSON array of objects (``{"proxy": ...}``).
    The locked test contract also accepts the ``{"proxies": [...]}`` shape, so
    both are handled here.
    """
    try:
        resp = _curl_requests.get(
            _proxy_pool_all_url(api_url),
            timeout=DEFAULT_TIMEOUT,
        )
        resp.raise_for_status()
        data = resp.json()
        proxies: list[str]
        if isinstance(data, list):
            proxies = [
                item["proxy"]
                for item in data
                if isinstance(item, dict) and isinstance(item.get("proxy"), str)
            ]
        elif isinstance(data, dict):
            pool = data.get("proxies")
            if isinstance(pool, list):
                proxies = [p for p in pool if isinstance(p, str)]
            else:
                single = data.get("proxy")
                proxies = [single] if isinstance(single, str) else []
        else:
            proxies = []
        return proxies[:count]
    except Exception:
        return []


def fetch_with_proxy(url: str, proxy: str, timeout: float) -> tuple[bool, float, str | None]:
    """Fetch ``url`` through ``proxy`` using curl_cffi TLS impersonation.

    Returns ``(success, elapsed_seconds, error_message_or_None)``.  Any HTTP
    status other than 200, and any transport/parsing exception, is treated as a
    failed attempt.
    """
    start = time.monotonic()
    try:
        resp = _curl_requests.get(
            url,
            timeout=timeout,
            proxies={"http": proxy, "https": proxy},
            impersonate=_IMPERSONATE,
        )
        elapsed = time.monotonic() - start
        if resp.status_code == 200:
            return True, elapsed, None
        return False, elapsed, f"HTTP {resp.status_code}"
    except Exception as exc:  # noqa: BLE001 - report any proxy/network failure
        elapsed = time.monotonic() - start
        return False, elapsed, str(exc)


def run_trial(
    url: str,
    count: int,
    api_url: str = DEFAULT_API_URL,
    timeout: float = DEFAULT_TIMEOUT,
) -> TrialResult:
    """Run ``count`` proxied requests against ``url`` and aggregate the result.

    ``get_proxies`` supplies the candidate pool; at most ``count`` candidates
    are actually used.  Exceptions raised by ``fetch_with_proxy`` are captured
    as failures instead of aborting the trial.
    """
    if count < 0:
        raise ValueError("count must be >= 0")
    if timeout < 0:
        raise ValueError("timeout must be >= 0")

    proxies = get_proxies(api_url, count)
    if not isinstance(proxies, list):
        raise ValueError("get_proxies() must return a list")

    selected = proxies[:count]
    total = len(selected)
    if total == 0:
        return TrialResult(
            target=url,
            total=0,
            success=0,
            failures=[],
            success_rate=0.0,
            avg_elapsed=0.0,
        )

    success = 0
    total_elapsed = 0.0
    failures: list[str] = []
    for proxy in selected:
        try:
            ok, elapsed, err = fetch_with_proxy(url, proxy, timeout)
        except Exception as exc:  # noqa: BLE001 - keep the trial running
            ok, elapsed, err = False, 0.0, str(exc)
        total_elapsed += elapsed
        if ok:
            success += 1
        else:
            failures.append(err or "unknown error")

    return TrialResult(
        target=url,
        total=total,
        success=success,
        failures=failures,
        success_rate=success / total,
        avg_elapsed=total_elapsed / total,
    )


def judge(
    result: TrialResult,
    success_threshold: float = 0.5,
    max_avg_elapsed: float = 5.0,
) -> tuple[bool, str]:
    """Apply the locked trial pass criteria.

    Pass iff ``success_rate >= success_threshold`` AND
    ``avg_elapsed < max_avg_elapsed`` (the average-time limit is strict).
    """
    if success_threshold <= 0:
        raise ValueError("success_threshold must be > 0")
    if max_avg_elapsed <= 0:
        raise ValueError("max_avg_elapsed must be > 0")

    rate_ok = result.success_rate >= success_threshold
    time_ok = result.avg_elapsed < max_avg_elapsed
    passed = rate_ok and time_ok
    reason = (
        f"success_rate={result.success_rate:.3f} "
        f"(>={success_threshold:.3f}: {rate_ok}), "
        f"avg_elapsed={result.avg_elapsed:.3f}s "
        f"(<{max_avg_elapsed:.3f}s: {time_ok})"
    )
    return passed, f"{'PASS' if passed else 'FAIL'}: {reason}"


def _current_kline_url() -> str:
    """Return the board kline URL for the current calendar year."""
    return THS_KLINE_URL_TEMPLATE.format(year=datetime.now().year)


def main(argv: list[str] | None = None) -> int:
    """Run the full THS proxy trial and print a JSON summary.

    Returns 0 when the trial completes (even if it fails the pass criteria).
    An unreachable or empty proxy_pool is a completed run (rc=0, FAIL verdict);
    returns 1 only for fatal setup/validation errors (for example
    ``get_proxies`` returning a non-list or ``run_trial`` raising).
    """
    parser = argparse.ArgumentParser(description="Verify proxy_pool against THS board endpoints")
    parser.add_argument("--api-url", default=DEFAULT_API_URL, help="proxy_pool API base URL")
    parser.add_argument("--count", type=int, default=DEFAULT_COUNT, help="requests per target")
    parser.add_argument(
        "--timeout", type=float, default=DEFAULT_TIMEOUT, help="per-request timeout"
    )
    args = parser.parse_args(argv)

    try:
        list_result = run_trial(THS_LIST_URL, args.count, args.api_url, args.timeout)
        kline_result = run_trial(_current_kline_url(), args.count, args.api_url, args.timeout)
    except Exception as exc:  # noqa: BLE001 - fatal setup error
        print(f"fatal: {exc}", file=sys.stderr)
        return 1

    combined_total = list_result.total + kline_result.total
    combined_success = list_result.success + kline_result.success
    if combined_total > 0:
        combined_rate = combined_success / combined_total
        combined_avg = (
            list_result.total * list_result.avg_elapsed
            + kline_result.total * kline_result.avg_elapsed
        ) / combined_total
    else:
        combined_rate = 0.0
        combined_avg = 0.0

    combined = TrialResult(
        target="ALL",
        total=combined_total,
        success=combined_success,
        failures=list_result.failures + kline_result.failures,
        success_rate=combined_rate,
        avg_elapsed=combined_avg,
    )
    passed, reason = judge(combined)
    payload: dict[str, Any] = {
        "success_rate": combined.success_rate,
        "avg_elapsed": combined.avg_elapsed,
        "verdict": "PASS" if passed else "FAIL",
        "judge_reason": reason,
        "failures": combined.failures,
        "targets": [
            {
                "target": list_result.target,
                "total": list_result.total,
                "success": list_result.success,
                "success_rate": list_result.success_rate,
                "avg_elapsed": list_result.avg_elapsed,
            },
            {
                "target": kline_result.target,
                "total": kline_result.total,
                "success": kline_result.success,
                "success_rate": kline_result.success_rate,
                "avg_elapsed": kline_result.avg_elapsed,
            },
        ],
    }
    print(json.dumps(payload, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
