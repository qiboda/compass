"""Requirement-acceptance tests for issue #290 — proxy-pool HTTPS validator patch.

Contract under test (issue #290 / approved plan):

  1. ``scripts/proxy_pool/validator.patch`` exists and is a unified diff that,
     applied from the upstream repo root (WORKDIR ``/app``) with ``patch -p1``,
     changes ONLY the ``httpsTimeOutValidator`` function's ``proxies`` line in
     ``helper/validator.py`` from::

         proxies = {"http": "http://{proxy}".format(proxy=proxy), "https": "https://{proxy}".format(proxy=proxy)}

     to::

         proxies = {"http": "http://{proxy}".format(proxy=proxy), "https": "http://{proxy}".format(proxy=proxy)}

  2. ``scripts/proxy_pool/Dockerfile`` exists, starts with
     ``FROM jhao104/proxy_pool:2.4.2``, and applies ``validator.patch`` during
     build (``COPY validator.patch`` + ``RUN patch -p1 < validator.patch``) so
     the patched file lands in ``/app/helper/validator.py``.

  3. ``scripts/proxy_pool/docker-compose.yml`` has the ``proxy_pool`` service
     use ``build: .`` and must NOT reference ``image: jhao104/proxy_pool:2.4.2``
     for that service.

These tests are RED now: the patch and Dockerfile do not exist yet and
docker-compose.yml still uses ``image:`` instead of ``build: .``.

Isolation: self-contained, no network, no external services, no PyYAML — all
assertions are text/path based under the repo root.
"""

from __future__ import annotations

from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PROXY_POOL_DIR = REPO_ROOT / "scripts" / "proxy_pool"
PATCH_PATH = PROXY_POOL_DIR / "validator.patch"
DOCKERFILE_PATH = PROXY_POOL_DIR / "Dockerfile"
COMPOSE_PATH = PROXY_POOL_DIR / "docker-compose.yml"

# The exact expected lines from the contract.
OLD_PROXIES_LINE = (
    '    proxies = {"http": "http://{proxy}".format(proxy=proxy), '
    '"https": "https://{proxy}".format(proxy=proxy)}'
)
NEW_PROXIES_LINE = (
    '    proxies = {"http": "http://{proxy}".format(proxy=proxy), '
    '"https": "http://{proxy}".format(proxy=proxy)}'
)
PATCHED_TARGET = "helper/validator.py"


class TestValidatorPatchExists:
    """Contract item 1 — the patch file must exist with the expected content."""

    def test_patch_file_exists(self) -> None:
        assert PATCH_PATH.is_file(), (
            f"{PATCH_PATH.relative_to(REPO_ROOT)} must exist (RED: patch not "
            f"implemented yet)"
        )

    def test_patch_is_unified_diff_with_p1_target(self) -> None:
        text = PATCH_PATH.read_text(encoding="utf-8")
        # A proper unified diff carries at least one hunk header and a file header.
        assert "--- " in text, "patch must be a unified diff with --- file header"
        assert "+++ " in text, "patch must be a unified diff with +++ file header"
        assert "@@" in text, "patch must contain at least one unified hunk (@@)"
        # Applied from the upstream repo root (WORKDIR /app) with patch -p1,
        # the target file is helper/validator.py.
        assert PATCHED_TARGET in text, (
            f"patch must target {PATCHED_TARGET} (relative to /app at -p1)"
        )

    def test_patch_changes_only_the_proxies_line(self) -> None:
        text = PATCH_PATH.read_text(encoding="utf-8")

        # The removed (-) line must be the old https scheme.
        assert f"-{OLD_PROXIES_LINE}" in text, (
            "patch must remove the old https:// proxies line"
        )
        # The added (+) line must be the new http:// proxies line.
        assert f"+{NEW_PROXIES_LINE}" in text, (
            "patch must add the new http:// proxies line"
        )
        # Same indentation on both sides — a whitespace-only scheme change.
        removed = [
            ln[1:]
            for ln in text.splitlines()
            if ln.startswith("-") and not ln.startswith("---")
        ]
        added = [
            ln[1:]
            for ln in text.splitlines()
            if ln.startswith("+") and not ln.startswith("+++")
        ]
        assert removed == [OLD_PROXIES_LINE], (
            "patch must change ONLY the httpsTimeOutValidator proxies line "
            f"(removed lines: {removed})"
        )
        assert added == [NEW_PROXIES_LINE], (
            "patch must change ONLY the httpsTimeOutValidator proxies line "
            f"(added lines: {added})"
        )


class TestDockerfileAppliesPatch:
    """Contract item 2 — Dockerfile must base on the upstream image and patch it."""

    def test_dockerfile_exists(self) -> None:
        assert DOCKERFILE_PATH.is_file(), (
            f"{DOCKERFILE_PATH.relative_to(REPO_ROOT)} must exist (RED: "
            f"Dockerfile not implemented yet)"
        )

    def test_dockerfile_starts_with_upstream_base(self) -> None:
        text = DOCKERFILE_PATH.read_text(encoding="utf-8")
        first_line = text.splitlines()[0].strip()
        assert first_line == "FROM jhao104/proxy_pool:2.4.2", (
            "Dockerfile must start with 'FROM jhao104/proxy_pool:2.4.2', "
            f"got {first_line!r}"
        )

    def test_dockerfile_applies_validator_patch_during_build(self) -> None:
        text = DOCKERFILE_PATH.read_text(encoding="utf-8")
        assert "COPY validator.patch" in text, (
            "Dockerfile must COPY validator.patch into the build context"
        )
        assert "patch" in text, "Dockerfile must invoke patch during build"
        # The canonical application: from WORKDIR /app, patch -p1 < patch.
        assert "RUN patch -p1 < validator.patch" in text, (
            "Dockerfile must apply the patch with 'RUN patch -p1 < validator.patch'"
        )
        # After COPY + RUN patch, the patched file lands in /app/helper/validator.py.
        assert "/app/helper/validator.py" in text, (
            "Dockerfile must reference the patched file path /app/helper/validator.py"
        )


class TestComposeUsesBuild:
    """Contract item 3 — compose must build locally, not pull the upstream tag."""

    def test_compose_proxy_pool_service_uses_build(self) -> None:
        text = COMPOSE_PATH.read_text(encoding="utf-8")
        assert "build: ." in text, (
            "docker-compose.yml must use 'build: .' for the proxy_pool service "
            "(RED: still uses image:)"
        )

    def test_compose_proxy_pool_service_has_no_upstream_image(self) -> None:
        text = COMPOSE_PATH.read_text(encoding="utf-8")
        # The upstream image tag must not be referenced for the proxy_pool
        # service. (The redis service legitimately has its own image.)
        lines = text.splitlines()
        service_block_started = False
        for ln in lines:
            if ln.strip() == "proxy_pool:":
                service_block_started = True
                continue
            if service_block_started and ln.strip() and not ln.startswith((" ", "\t")):
                break  # next top-level key -> proxy_pool block ended
            if service_block_started:
                assert "image: jhao104/proxy_pool:2.4.2" not in ln, (
                    "the proxy_pool service must not reference "
                    "image: jhao104/proxy_pool:2.4.2"
                )
