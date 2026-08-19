"""Regression tests for the proxy_pool compose deployment (issue #296).

proxy_pool's keepalive/freeproxy defaults write to ``redis://@127.0.0.1:6379/0``.
The compose file must expose Redis on the host loopback so those defaults work
without passing a container-IP redis URL (which changes across container
recreates).
"""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
COMPOSE = REPO_ROOT / "scripts" / "proxy_pool" / "docker-compose.yml"


def _service_block(text: str, service: str) -> list[str]:
    """Return the YAML lines of one top-level compose service block."""
    lines = text.splitlines()
    marker = f"  {service}:"
    start = next(i for i, line in enumerate(lines) if line == marker)
    end = len(lines)
    for i in range(start + 1, len(lines)):
        # A new top-level key (two spaces + name + colon) ends the service.
        if i > start + 1 and lines[i] and lines[i][0].isalpha() and not lines[i].startswith("    "):
            end = i
            break
    return lines[start:end]


def test_compose_exists() -> None:
    assert COMPOSE.is_file(), f"compose file missing: {COMPOSE}"


def test_proxy_redis_exposes_host_loopback_6379() -> None:
    """proxy_redis must publish 127.0.0.1:6379 so keepalive's default works."""
    text = COMPOSE.read_text(encoding="utf-8")
    block = "\n".join(_service_block(text, "proxy_redis"))
    assert "127.0.0.1:6379:6379" in block, (
        "proxy_redis has no host loopback port mapping for 6379; "
        "keepalive default redis://@127.0.0.1:6379/0 cannot connect (issue #296)"
    )


def test_proxy_redis_port_binds_loopback_only() -> None:
    """The mapping must be loopback-only; never 0.0.0.0/exposed to LAN."""
    text = COMPOSE.read_text(encoding="utf-8")
    block = "\n".join(_service_block(text, "proxy_redis"))
    assert "127.0.0.1:6379:6379" in block
    assert "0.0.0.0:6379" not in block


def test_proxy_pool_db_conn_still_uses_service_name() -> None:
    """proxy_pool should keep talking to Redis via the compose service name."""
    text = COMPOSE.read_text(encoding="utf-8")
    block = "\n".join(_service_block(text, "proxy_pool"))
    assert 'DB_CONN: "redis://@proxy_redis:6379/0"' in block


def test_keepalive_default_redis_url_matches_host_port() -> None:
    """Keepalive's default redis URL must stay 127.0.0.1:6379 (host loopback)."""
    keepalive = REPO_ROOT / "collectors" / "proxy_keepalive.py"
    fetch_freeproxy = REPO_ROOT / "collectors" / "fetch_freeproxy.py"
    assert 'DEFAULT_REDIS_URL = "redis://@127.0.0.1:6379/0"' in fetch_freeproxy.read_text(encoding="utf-8")
    # Keepalive must actually wire its --redis-url default to the same constant;
    # otherwise a drift between the two would still pass the literal checks above.
    assert (
        "default=fetch_freeproxy.DEFAULT_REDIS_URL"
        in keepalive.read_text(encoding="utf-8")
    )
