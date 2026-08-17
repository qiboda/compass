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

STATUS: GREEN — the patch, Dockerfile, and compose update are implemented.
The original RED evidence (8 failing tests before implementation) is preserved
in the commit history.

Isolation: self-contained, no network, no external services, no PyYAML — all
assertions are text/path based under the repo root.
"""

from __future__ import annotations

import re
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


def _proxy_pool_service_members(compose_text: str) -> str:
    """Return the indented member lines of the top-level ``proxy_pool:`` service."""
    match = re.search(r"^\s{2}proxy_pool:\s*$", compose_text, re.MULTILINE)
    assert match, "compose has no top-level `proxy_pool:` service"
    members: list[str] = []
    for line in compose_text[match.end():].splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        if line.startswith("    "):
            members.append(line)
        else:
            break
    return "\n".join(members)


class TestValidatorPatchExists:
    """Contract item 1 — the patch file must exist with the expected content."""

    def test_patch_file_exists(self) -> None:
        assert PATCH_PATH.is_file(), (
            f"{PATCH_PATH.relative_to(REPO_ROOT)} must exist (patch file is missing)"
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
            f"{DOCKERFILE_PATH.relative_to(REPO_ROOT)} must exist (Dockerfile is missing)"
        )

    def test_dockerfile_starts_with_upstream_base(self) -> None:
        text = DOCKERFILE_PATH.read_text(encoding="utf-8")
        first_line = text.splitlines()[0].strip()
        parts = first_line.split()
        assert parts[:2] == ["FROM", "jhao104/proxy_pool:2.4.2"], (
            "Dockerfile must start with 'FROM jhao104/proxy_pool:2.4.2' "
            f"(multi-stage aliases allowed), got {first_line!r}"
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
        # The patched file lands in /app/helper/validator.py; the adversarial
        # suite verifies the WORKDIR /app -> patch ordering and the resulting
        # file content.  Keep this requirement test focused on the Dockerfile
        # instructions rather than a comment that could tautologically satisfy
        # the assertion.


class TestComposeUsesBuild:
    """Contract item 3 — compose must build locally, not pull the upstream tag."""

    def test_compose_proxy_pool_service_uses_build(self) -> None:
        members = _proxy_pool_service_members(COMPOSE_PATH.read_text(encoding="utf-8"))
        assert re.search(r"^\s*build:\s*\.\s*$", members, re.MULTILINE), (
            "docker-compose.yml proxy_pool service must use 'build: .'; "
            f"found members:\n{members}"
        )

    def test_compose_proxy_pool_service_has_no_upstream_image(self) -> None:
        members = _proxy_pool_service_members(COMPOSE_PATH.read_text(encoding="utf-8"))
        assert not re.search(r"image:\s*jhao104/proxy_pool\b", members, re.MULTILINE), (
            "the proxy_pool service must not reference image: jhao104/proxy_pool"
        )
