#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

sys.dont_write_bytecode = True

from evidence.model import (
    EvidenceError,
    canonical_json_bytes,
    fail,
    require_hex,
    require_list,
    require_string,
    require_tag,
    write_new,
)

FORMULA_PAIR = re.compile(
    r'url "([^"]+)"\s*\n\s*sha256 "([0-9a-f]{64})"'
)
REPOSITORY = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\Z")
FORMULA_ASSET_NAMES = (
    "aura-aarch64-apple-darwin.tar.gz",
    "aura-aarch64-unknown-linux-gnu.tar.gz",
    "aura-x86_64-apple-darwin.tar.gz",
    "aura-x86_64-unknown-linux-gnu.tar.gz",
)
ASSET_NAMES = tuple(sorted((
    "aura.rb",
    *FORMULA_ASSET_NAMES,
    *(f"{name}.sha256" for name in FORMULA_ASSET_NAMES),
)))


def _formula_assets(path: Path, tag: str, repo: str) -> tuple[tuple[str, str], ...]:
    matches = FORMULA_PAIR.findall(path.read_text(encoding="utf-8"))
    if len(matches) != 4:
        fail(f"formula must contain exactly 4 url/sha256 pairs, got {len(matches)}")
    prefix = f"https://github.com/{repo}/releases/download/{tag}/"
    assets: list[tuple[str, str]] = []
    names: set[str] = set()
    for url, sha256 in matches:
        if not url.startswith(prefix):
            fail("formula release URLs do not bind repository and canonical tag")
        name = url.removeprefix(prefix)
        if not name or "/" in name or name in names:
            fail("formula release asset URLs must have unique canonical filenames")
        names.add(name)
        assets.append((name, sha256))
    if names != set(FORMULA_ASSET_NAMES):
        fail("formula release asset filenames differ from the four supported targets")
    return tuple(assets)


def _manifest_assets(path: Path) -> tuple[tuple[str, str], ...]:
    try:
        lines = path.read_bytes().decode("ascii").splitlines()
    except UnicodeDecodeError:
        fail("expected assets manifest must be ASCII")
    if len(lines) != len(ASSET_NAMES):
        fail(f"expected assets manifest must contain exactly {len(ASSET_NAMES)} lines")
    assets: dict[str, str] = {}
    for index, line in enumerate(lines):
        fields = line.split()
        if len(fields) != 2:
            fail(f"expected assets manifest line {index + 1} is malformed")
        name, digest = fields
        if name not in ASSET_NAMES or name in assets:
            fail(f"expected assets manifest contains invalid asset name: {name}")
        assets[name] = require_hex(digest, 64, f"expected asset digest for {name}")
    if set(assets) != set(ASSET_NAMES):
        fail("expected assets manifest does not contain the exact release asset set")
    return tuple((name, assets[name]) for name in ASSET_NAMES)


def _release_assets(repo: str, tag: str, token: str, timeout: float) -> tuple[tuple[str, str], ...]:
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repo}/releases?per_page=100",
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
    except TimeoutError:
        fail("GitHub release API request timed out")
    except urllib.error.HTTPError as error:
        status = error.code
        error.close()
        fail(f"GitHub release API returned HTTP {status}")
    except (urllib.error.URLError, OSError):
        fail("GitHub release API request failed")
    try:
        payload = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("GitHub release API returned invalid JSON")
    releases = require_list(payload, "GitHub releases")
    matches: list[dict[str, object]] = []
    for index, release in enumerate(releases):
        if not isinstance(release, dict):
            fail(f"GitHub release {index} must be an object")
        if require_string(release.get("tag_name"), f"GitHub release {index} tag") == tag:
            matches.append(release)
    if len(matches) != 1:
        fail(f"expected exactly one GitHub release matching tag {tag}")
    values = require_list(matches[0].get("assets"), "release assets")
    assets: list[tuple[str, str]] = []
    for index, value in enumerate(values):
        if not isinstance(value, dict):
            fail(f"release asset {index} must be an object")
        name = require_string(value.get("name"), f"release asset {index} name")
        digest = require_string(value.get("digest"), f"release asset {index} digest")
        assets.append((name, digest))
    return tuple(assets)


def _verify_assets(
    formula: tuple[tuple[str, str], ...],
    expected: tuple[tuple[str, str], ...],
    release: tuple[tuple[str, str], ...],
) -> bytes:
    if len(release) != len(ASSET_NAMES):
        fail(f"release must contain exactly {len(ASSET_NAMES)} assets, got {len(release)}")
    digests: dict[str, str] = {}
    for name, digest in release:
        if name in digests:
            fail(f"duplicate release asset: {name}")
        digests[name] = digest
    if set(digests) != set(ASSET_NAMES):
        fail("remote release asset names differ from the exact expected set")
    expected_digests = dict(expected)
    for name in ASSET_NAMES:
        if digests[name] != f"sha256:{expected_digests[name]}":
            fail(f"remote release asset digest mismatch: {name}")
    for name, sha256 in formula:
        if expected_digests[name] != sha256:
            fail(f"formula digest differs from expected release asset: {name}")
    return "".join(
        f"{name} {digests[name].removeprefix('sha256:')}\n"
        for name in ASSET_NAMES
    ).encode("ascii")


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify GitHub release assets against a Homebrew formula")
    parser.add_argument("--formula", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repo", default="zapsaang/aura")
    parser.add_argument("--expected-manifest", type=Path, required=True)
    parser.add_argument("--assets-manifest-out", type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=10.0)
    args = parser.parse_args()
    try:
        tag = require_tag(args.tag)
        if REPOSITORY.fullmatch(args.repo) is None:
            fail(f"invalid repository: {args.repo}")
        if not math.isfinite(args.timeout) or args.timeout <= 0:
            fail("timeout must be a positive finite number")
        token = os.environ.get("AURA_RELEASE_TOKEN")
        if not token:
            fail("AURA_RELEASE_TOKEN is required")
        formula = _formula_assets(args.formula, tag, args.repo)
        expected = _manifest_assets(args.expected_manifest)
        release = _release_assets(args.repo, tag, token, args.timeout)
        manifest = _verify_assets(formula, expected, release)
        write_new(args.assets_manifest_out, manifest)
    except (EvidenceError, OSError) as error:
        sys.stderr.write(f"verify-release-asset: {error}\n")
        return 1
    sys.stdout.write(canonical_json_bytes({"checks": 10, "status": "ok"}).decode("utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
