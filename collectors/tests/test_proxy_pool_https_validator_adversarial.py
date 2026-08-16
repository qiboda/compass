"""Adversarial tests for the proxy_pool HTTPS-validator patch (issue #290).

Attacks the locked contract of the three planned artifacts:

* ``scripts/proxy_pool/validator.patch`` — a unified diff that, applied with
  ``patch -p1`` against upstream ``jhao104/proxy_pool:2.4.2``'s
  ``helper/validator.py``, changes **only** the ``httpsTimeOutValidator``
  proxies line so that its ``https`` key uses ``http://{proxy}`` (both keys
  become ``http://``).
* ``scripts/proxy_pool/Dockerfile`` — based on ``FROM jhao104/proxy_pool:2.4.2``,
  must actually invoke ``patch`` (not merely COPY) with ``-p1`` inside
  ``WORKDIR /app``.
* ``scripts/proxy_pool/docker-compose.yml`` — proxy_pool service must use
  ``build: .`` and no longer reference ``image: jhao104/proxy_pool``; the build
  context (the directory that contains the Dockerfile and the patch) is the
  only path that makes ``build: .`` resolve.

Adversarial angle: upstream has **identical** proxies lines in both
``httpTimeOutValidator`` and ``httpsTimeOutValidator``.  A naive patch that
replaces that line globally (or with insufficient hunk context) will hit the
wrong function first, leaving ``httpsTimeOutValidator`` untouched — exactly the
bug issue #290 exists to fix.  These tests pin the patch to the correct function
and to the correct ``-p1``/``/app`` path handling.

This suite is deliberately RED until the implementation artifacts land: it never
requires Docker, a network, or a running proxy_pool, and it never touches
production code.
"""

from __future__ import annotations

import re
import shutil
import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
PROXY_POOL_DIR = REPO_ROOT / "scripts" / "proxy_pool"
PATCH_PATH = PROXY_POOL_DIR / "validator.patch"
DOCKERFILE_PATH = PROXY_POOL_DIR / "Dockerfile"
COMPOSE_PATH = PROXY_POOL_DIR / "docker-compose.yml"

# ── upstream fixture ────────────────────────────────────────────────────────
# Byte-for-byte copy of jhao104/proxy_pool 2.4.2 `helper/validator.py`
# (fetched 2026-08-16 from the upstream tag).  The two `proxies` lines in
# httpTimeOutValidator (line ~62) and httpsTimeOutValidator (line ~75) are
# byte-identical — the crux of the adversarial scope attack.
UPSTREAM_VALIDATOR = r'''# -*- coding: utf-8 -*-
"""
-------------------------------------------------
   File Name：     _validators
   Description :   定义proxy验证方法
   Author :        JHao
   date：          2021/5/25
-------------------------------------------------
   Change Activity:
                   2023/03/10: 支持带用户认证的代理格式 username:password@ip:port
-------------------------------------------------
"""
__author__ = 'JHao'

import re
from requests import head
from util.six import withMetaclass
from util.singleton import Singleton
from handler.configHandler import ConfigHandler

conf = ConfigHandler()

HEADER = {'User-Agent': 'Mozilla/5.0 (Windows NT 6.1; WOW64; rv:34.0) Gecko/20100101 Firefox/34.0',
          'Accept': '*/*',
          'Connection': 'keep-alive',
          'Accept-Language': 'zh-CN,zh;q=0.8'}

IP_REGEX = re.compile(r"(.*:.*@)?\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d{1,5}")


class ProxyValidator(withMetaclass(Singleton)):
    pre_validator = []
    http_validator = []
    https_validator = []

    @classmethod
    def addPreValidator(cls, func):
        cls.pre_validator.append(func)
        return func

    @classmethod
    def addHttpValidator(cls, func):
        cls.http_validator.append(func)
        return func

    @classmethod
    def addHttpsValidator(cls, func):
        cls.https_validator.append(func)
        return func


@ProxyValidator.addPreValidator
def formatValidator(proxy):
    """检查代理格式"""
    return True if IP_REGEX.fullmatch(proxy) else False


@ProxyValidator.addHttpValidator
def httpTimeOutValidator(proxy):
    """ http检测超时 """

    proxies = {"http": "http://{proxy}".format(proxy=proxy), "https": "https://{proxy}".format(proxy=proxy)}

    try:
        r = head(conf.httpUrl, headers=HEADER, proxies=proxies, timeout=conf.verifyTimeout)
        return True if r.status_code == 200 else False
    except Exception as e:
        return False


@ProxyValidator.addHttpsValidator
def httpsTimeOutValidator(proxy):
    """https检测超时"""

    proxies = {"http": "http://{proxy}".format(proxy=proxy), "https": "https://{proxy}".format(proxy=proxy)}
    try:
        r = head(conf.httpsUrl, headers=HEADER, proxies=proxies, timeout=conf.verifyTimeout, verify=False)
        return True if r.status_code == 200 else False
    except Exception as e:
        return False


@ProxyValidator.addHttpValidator
def customValidatorExample(proxy):
    """自定义validator函数，校验代理是否可用, 返回True/False"""
    return True
'''

HTTP_PROXY_LINE = (
    '    proxies = {"http": "http://{proxy}".format(proxy=proxy), '
    '"https": "https://{proxy}".format(proxy=proxy)}'
)
HTTPS_PATCHED_LINE = (
    '    proxies = {"http": "http://{proxy}".format(proxy=proxy), '
    '"https": "http://{proxy}".format(proxy=proxy)}'
)
UPSTREAM_ARTICLE_PATH = "helper/validator.py"


# ── helpers ────────────────────────────────────────────────────────────────


def _require_patch() -> str:
    """Return the patch binary, failing (never skipping) if unavailable."""
    bin_ = shutil.which("patch")
    if not bin_:
        pytest.fail(
            "The `patch` binary is not available on PATH; the patch-validity "
            "contract cannot be verified. This suite requires the GNU/BSD "
            "patch utility to be installed."
        )
    return bin_


def _write_upstream(workdir: Path) -> Path:
    """Materialise upstream validator.py at ``<workdir>/helper/validator.py``."""
    helper_dir = workdir / "helper"
    helper_dir.mkdir(parents=True, exist_ok=True)
    target = helper_dir / "validator.py"
    target.write_text(UPSTREAM_VALIDATOR, encoding="utf-8")
    return target


def _run_patch(workdir: Path, *, dry_run: bool = False) -> subprocess.CompletedProcess[str]:
    """Apply ``validator.patch`` from PROXY_POOL_DIR with ``patch -p1`` in workdir."""
    patch_bin = _require_patch()
    cmd = [patch_bin, "-p1"]
    if dry_run:
        cmd.append("--dry-run")
    cmd += ["-i", str(PATCH_PATH)]
    return subprocess.run(
        cmd,
        cwd=workdir,
        capture_output=True,
        text=True,
        timeout=120,
    )


def _extract_proxy_lines(src: str) -> dict[str, str]:
    """Map each ``def <name>`` function to its (first) ``proxies = {...}`` line."""
    funcs: dict[str, str] = {}
    current: str | None = None
    for line in src.splitlines():
        if line.startswith("def "):
            current = line.split("def ", 1)[1].split("(", 1)[0].strip()
        elif "proxies = {" in line and current is not None:
            funcs[current] = line
            current = None  # only the first proxies line per function
    return funcs


def _proxy_pool_service_members(compose_text: str) -> str:
    """Return the indented member lines of the top-level ``proxy_pool:`` service."""
    match = re.search(r"^\s{2}proxy_pool:\s*$", compose_text, re.MULTILINE)
    assert match, "compose has no top-level `proxy_pool:` service"
    members: list[str] = []
    for line in compose_text[match.end():].splitlines():
        stripped = line.strip()
        if not stripped:  # tolerate blank separator lines inside the block
            continue
        if line.startswith("    "):  # members of a depth-2 service live at 4 spaces
            members.append(line)
        else:
            break  # reached a sibling key at depth < 2 — end of this service
    return "\n".join(members)


# ── dimension 1: patch validity ────────────────────────────────────────────


def test_patch_file_exists_and_is_nonempty() -> None:
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH}"
    assert PATCH_PATH.read_text(encoding="utf-8").strip(), f"{PATCH_PATH} is empty"


def test_patch_is_parseable_unified_diff_and_applies_cleanly(tmp_path: Path) -> None:
    _require_patch()
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH}"
    patch_text = PATCH_PATH.read_text(encoding="utf-8")
    assert re.search(r"^--- ", patch_text, re.MULTILINE), "patch has no unified-diff ---/+++ header"
    assert re.search(r"^\+\+\+ ", patch_text, re.MULTILINE), "patch has no +++ header"
    assert re.search(r"^@@ ", patch_text, re.MULTILINE), "patch has no hunk header"

    workdir = tmp_path / "apply"
    workdir.mkdir()
    _write_upstream(workdir)
    proc = _run_patch(workdir, dry_run=True)
    assert proc.returncode == 0, (
        f"`patch -p1 --dry-run` rejected the patch:\n"
        f"stdout={proc.stdout!r}\nstderr={proc.stderr!r}"
    )


def test_patch_headers_target_helper_with_p1_strippable_path() -> None:
    _require_patch()
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH}"
    text = PATCH_PATH.read_text(encoding="utf-8")
    for marker in ("---", "+++"):
        match = re.search(rf"^{re.escape(marker)} (\S+)", text, re.MULTILINE)
        assert match, f"patch has no {marker} path header"
        path = match.group(1)
        assert path.rsplit("/", 1)[-1] == "validator.py", f"patch targets wrong file: {path!r}"
        assert path.lstrip("ab/") == UPSTREAM_ARTICLE_PATH, (
            f"patch path {path!r} is not consistent with `patch -p1` on "
            f"{UPSTREAM_ARTICLE_PATH!r} under WORKDIR /app"
        )


# ── dimension 2+3: patch scope & effectiveness ─────────────────────────────


def test_patch_effectively_changes_https_key_to_http_scheme(tmp_path: Path) -> None:
    _require_patch()
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH}"
    workdir = tmp_path / "eff"
    workdir.mkdir()
    target = _write_upstream(workdir)
    before = target.read_text(encoding="utf-8")
    proc = _run_patch(workdir)
    assert proc.returncode == 0, (
        f"real `patch -p1` failed:\nstdout={proc.stdout!r}\nstderr={proc.stderr!r}"
    )
    after = target.read_text(encoding="utf-8")
    assert after != before, "patch is a no-op: applied file is byte-identical to upstream"

    proxy_lines = _extract_proxy_lines(after)
    https_line = proxy_lines.get("httpsTimeOutValidator", "")
    assert '"https": "http://{proxy}"' in https_line, (
        "httpsTimeOutValidator https key not switched to http://{proxy}: "
        f"{https_line!r}"
    )
    assert '"https": "https://{proxy}"' not in https_line, (
        "httpsTimeOutValidator still uses https:// proxy scheme: "
        f"{https_line!r}"
    )
    assert '"http": "http://{proxy}"' in https_line


def test_patch_does_not_modify_http_timeout_validator(tmp_path: Path) -> None:
    _require_patch()
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH}"
    workdir = tmp_path / "scope"
    workdir.mkdir()
    target = _write_upstream(workdir)
    proc = _run_patch(workdir)
    assert proc.returncode == 0, (
        f"real `patch -p1` failed:\nstdout={proc.stdout!r}\nstderr={proc.stderr!r}"
    )
    proxy_lines = _extract_proxy_lines(target.read_text(encoding="utf-8"))
    assert proxy_lines.get("httpTimeOutValidator") == HTTP_PROXY_LINE, (
        "patch leaked into httpTimeOutValidator — upstream proxies line must be "
        "unchanged (if it changed, the globally-identical line was replaced "
        "instead of the https function)"
    )


def test_patch_change_is_scoped_exactly_to_https_timeout_validator(tmp_path: Path) -> None:
    """Both dimensions at once: http side untouched, https side changed."""
    _require_patch()
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH}"
    workdir = tmp_path / "both"
    workdir.mkdir()
    target = _write_upstream(workdir)
    proc = _run_patch(workdir)
    assert proc.returncode == 0, (
        f"real `patch -p1` failed:\nstdout={proc.stdout!r}\nstderr={proc.stderr!r}"
    )
    proxy_lines = _extract_proxy_lines(target.read_text(encoding="utf-8"))
    assert proxy_lines.get("httpTimeOutValidator") == HTTP_PROXY_LINE
    assert proxy_lines.get("httpsTimeOutValidator") == HTTPS_PATCHED_LINE, (
        f"httpsTimeOutValidator not patched to {HTTPS_PATCHED_LINE!r}: got "
        f"{proxy_lines.get('httpsTimeOutValidator')!r}"
    )


# ── dimension 4: Dockerfile ────────────────────────────────────────────────


def test_dockerfile_exists_and_is_nonempty() -> None:
    assert DOCKERFILE_PATH.exists(), f"missing {DOCKERFILE_PATH}"
    assert DOCKERFILE_PATH.read_text(encoding="utf-8").strip(), f"{DOCKERFILE_PATH} is empty"


def test_dockerfile_based_on_upstream_base_image() -> None:
    _require_patch()
    assert DOCKERFILE_PATH.exists(), f"missing {DOCKERFILE_PATH}"
    text = DOCKERFILE_PATH.read_text(encoding="utf-8")
    assert re.search(r"^FROM\s+jhao104/proxy_pool:2\.4\.2\s*$", text, re.MULTILINE), (
        "Dockerfile must start from `FROM jhao104/proxy_pool:2.4.2`"
    )


def test_dockerfile_actually_invokes_patch_with_dash_p1() -> None:
    _require_patch()
    assert DOCKERFILE_PATH.exists(), f"missing {DOCKERFILE_PATH}"
    text = DOCKERFILE_PATH.read_text(encoding="utf-8")
    assert "validator.patch" in text, "Dockerfile does not reference validator.patch"
    assert re.search(r"\bpatch\b", text), (
        "Dockerfile never invokes `patch` — COPYing the file without applying it "
        "does not satisfy the contract"
    )
    assert re.search(r"-p1\b", text), "Dockerfile patch invocation must use -p1"


def test_dockerfile_apply_patch_inside_workdir_app() -> None:
    _require_patch()
    assert DOCKERFILE_PATH.exists(), f"missing {DOCKERFILE_PATH}"
    text = DOCKERFILE_PATH.read_text(encoding="utf-8")
    assert re.search(r"^WORKDIR\s+/app\s*$", text, re.MULTILINE), (
        "Dockerfile must set WORKDIR /app (patch paths are -p1-relative)"
    )
    wd_match = re.search(r"^WORKDIR\s+/app\s*$", text, re.MULTILINE)
    patch_matches = [m.start() for m in re.finditer(r"\bpatch\b.*", text)]
    assert wd_match and patch_matches, "missing WORKDIR /app or patch invocation"
    assert wd_match.start() < patch_matches[0], (
        "the `patch` invocation must happen after WORKDIR /app so the -p1 "
        "relative path resolves against /app"
    )


# ── dimension 5: compose ───────────────────────────────────────────────────


def test_compose_proxy_pool_service_uses_build_dot() -> None:
    assert COMPOSE_PATH.exists(), f"missing {COMPOSE_PATH}"
    members = _proxy_pool_service_members(COMPOSE_PATH.read_text(encoding="utf-8"))
    assert re.search(r"^\s*build:\s*\.\s*$", members, re.MULTILINE), (
        "proxy_pool service must contain `build: .`; found members:\n" + members
    )


def test_compose_proxy_pool_no_longer_references_upstream_image() -> None:
    assert COMPOSE_PATH.exists(), f"missing {COMPOSE_PATH}"
    members = _proxy_pool_service_members(COMPOSE_PATH.read_text(encoding="utf-8"))
    assert not re.search(r"image:\s*jhao104/proxy_pool\b", members, re.MULTILINE), (
        "proxy_pool service still references `image: jhao104/proxy_pool` — "
        "must build the patched image locally"
    )


# ── dimension 6: edge cases (missing files / build context) ────────────────


def test_build_context_dir_contains_both_dockerfile_and_patch() -> None:
    """`build: .` context must be the dir where Dockerfile and validator.patch live."""
    assert PATCH_PATH.exists(), f"missing {PATCH_PATH} (build context needs it)"
    assert DOCKERFILE_PATH.exists(), f"missing {DOCKERFILE_PATH} (build context needs it)"
    assert PROXY_POOL_DIR.is_dir(), f"build context {PROXY_POOL_DIR} is not a directory"
