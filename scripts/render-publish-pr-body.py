#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Final
from urllib.parse import urlsplit

from evidence.model import EvidenceError, fail, require_hex, require_tag

sys.dont_write_bytecode = True

TARGETS: Final = (
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
)
ASSET_NAMES: Final = tuple(f"aura-{target}.tar.gz" for target in TARGETS)
RELEASE_PATH: Final = re.compile(
    r"/(?P<owner>[A-Za-z0-9_.-]+)/(?P<repo>[A-Za-z0-9_.-]+)/releases/tag/(?P<tag>[^/]+)"
)
AUDIT_PATH: Final = re.compile(
    r"/(?P<owner>[A-Za-z0-9_.-]+)/(?P<repo>[A-Za-z0-9_.-]+)/actions/runs/"
    r"(?P<run>[1-9][0-9]*)(?:/job/[1-9][0-9]*)?"
)
REPOSITORY: Final = re.compile(
    r"(?P<owner>[A-Za-z0-9_.-]+)/(?P<repo>[A-Za-z0-9_.-]+)"
)


@dataclass(frozen=True, slots=True)
class AssetDigest:
    target: str
    name: str
    sha256: str


@dataclass(frozen=True, slots=True)
class PublishPrBody:
    tag: str
    verified_commit: str
    formula_sha256: str
    assets: tuple[AssetDigest, ...]
    release_url: str
    audit_log_url: str
    source_repository: str
    tap_name: str
    repository_url: str
    workflow_run_url: str


def _github_match(value: str, label: str, pattern: re.Pattern[str]) -> re.Match[str]:
    parsed = urlsplit(value)
    if (
        parsed.scheme != "https"
        or parsed.netloc != "github.com"
        or parsed.query
        or parsed.fragment
    ):
        fail(f"{label} must be a complete https://github.com URL")
    matched = pattern.fullmatch(parsed.path)
    if matched is None:
        fail(f"invalid {label} path: {parsed.path}")
    return matched


def _read_assets(path: Path) -> tuple[AssetDigest, ...]:
    try:
        lines = path.read_bytes().decode("ascii").splitlines()
    except UnicodeDecodeError as error:
        raise EvidenceError("asset digest list must be ASCII") from error
    if len(lines) != len(ASSET_NAMES):
        fail(f"asset digest list must contain exactly {len(ASSET_NAMES)} lines")

    by_name: dict[str, AssetDigest] = {}
    for number, line in enumerate(lines, start=1):
        fields = line.split()
        if len(fields) != 2:
            fail(f"asset digest line {number} must be '<asset-name> <sha256>'")
        name, digest = fields
        if name not in ASSET_NAMES:
            fail(f"unexpected asset name on line {number}: {name}")
        if name in by_name:
            fail(f"duplicate asset name on line {number}: {name}")
        target = name.removeprefix("aura-").removesuffix(".tar.gz")
        by_name[name] = AssetDigest(
            target=target,
            name=name,
            sha256=require_hex(digest, 64, f"asset digest on line {number}"),
        )

    missing = tuple(name for name in ASSET_NAMES if name not in by_name)
    if missing:
        fail(f"asset digest list is missing: {', '.join(missing)}")
    return tuple(by_name[name] for name in ASSET_NAMES)


def _validated_body(args: argparse.Namespace) -> PublishPrBody:
    tag = require_tag(args.tag)
    release = _github_match(args.release_url, "release URL", RELEASE_PATH)
    audit = _github_match(args.audit_log_url, "audit log URL", AUDIT_PATH)
    release_repo = (release["owner"], release["repo"])
    audit_repo = (audit["owner"], audit["repo"])
    if release["tag"] != tag:
        fail("release URL tag does not match --tag")
    if release_repo != audit_repo:
        fail("release and audit URLs must reference the same repository")
    tap = REPOSITORY.fullmatch(args.tap_repository)
    if tap is None or not tap["repo"].startswith("homebrew-"):
        fail("tap repository must be OWNER/homebrew-NAME")
    tap_short_name = tap["repo"].removeprefix("homebrew-")
    if not tap_short_name:
        fail("tap repository name must follow homebrew-NAME")

    source_repository = f"{release['owner']}/{release['repo']}"
    repository_url = f"https://github.com/{source_repository}"
    return PublishPrBody(
        tag=tag,
        verified_commit=require_hex(args.verified_commit, 40, "verified commit"),
        formula_sha256=require_hex(args.formula_sha256, 64, "formula sha256"),
        assets=_read_assets(Path(args.asset_sha_list)),
        release_url=args.release_url,
        audit_log_url=args.audit_log_url,
        source_repository=source_repository,
        tap_name=f"{tap['owner']}/{tap_short_name}",
        repository_url=repository_url,
        workflow_run_url=f"{repository_url}/actions/runs/{audit['run']}",
    )


def render(body: PublishPrBody) -> str:
    lines = [
        f"# Homebrew tap update: `{body.tag}`",
        "",
        "| field | value |",
        "|---|---|",
        f"| tag | `{body.tag}` |",
        f"| verified commit | `{body.verified_commit}` |",
        f"| formula SHA-256 | `{body.formula_sha256}` |",
        "",
        "## Release assets",
        "",
        "| target | URL | expected SHA-256 |",
        "|---|---|---|",
    ]
    for asset in body.assets:
        asset_url = f"{body.repository_url}/releases/download/{body.tag}/{asset.name}"
        lines.append(
            f"| `{asset.target}` | [{asset.name}]({asset_url}) | `{asset.sha256}` |"
        )
    lines.extend(
        (
            "",
            "## Review links",
            "",
            f"- [Draft release]({body.release_url})",
            f"- [Homebrew audit log]({body.audit_log_url})",
            f"- [release.yml workflow run]({body.workflow_run_url})",
            "",
            "## Publish checklist",
            "",
            f"- [ ] Inspect the draft assets with `gh release view {body.tag} --repo {body.source_repository} --json assets` and confirm the asset count is 9.",
            "- [ ] Merge this tap PR.",
            f"- [ ] Publish the release with `gh release edit {body.tag} --repo {body.source_repository} --draft=false`.",
            f"- [ ] After publishing, run `brew tap {body.tap_name} && brew install aura`, then verify `aura-cli -m cpu` does not return `[AURA: OFFLINE]`.",
            "",
        )
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--formula-sha256", required=True)
    parser.add_argument("--asset-sha-list", required=True)
    parser.add_argument("--release-url", required=True)
    parser.add_argument("--audit-log-url", required=True)
    parser.add_argument("--tap-repository", default="zapsaang/homebrew-tap")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    try:
        rendered = render(_validated_body(args))
        output = Path(args.out)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered, encoding="utf-8", newline="")
    except (EvidenceError, OSError) as error:
        sys.stderr.write(f"render-publish-pr-body: {error}\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
