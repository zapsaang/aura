#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path

from evidence.model import (
    EvidenceError,
    decode_json_bytes,
    fail,
    require_hex,
    require_tag,
    sha256_file,
)
from evidence.registry import load_registry
from evidence.tuple import verify_tuple


def main() -> int:
    parser = argparse.ArgumentParser(description="Bind formula bytes to render, audit, tag, and commit evidence")
    parser.add_argument("--formula", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--audit-tuple", type=Path, required=True)
    parser.add_argument("--render-tuple", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--verified-commit", required=True)
    args = parser.parse_args()
    digest = require_hex(args.sha256, 64, "formula sha256")
    tag = require_tag(args.tag)
    commit = require_hex(args.verified_commit, 40, "verified_commit")
    if sha256_file(args.formula) != digest:
        fail("formula digest drift")
    _verify_registered_tuple(args.render_tuple, "homebrew-render")
    _verify_registered_tuple(args.audit_tuple, "homebrew-audit")
    formula = args.formula.read_text(encoding="utf-8")
    targets = (
        "aura-aarch64-apple-darwin.tar.gz",
        "aura-aarch64-unknown-linux-gnu.tar.gz",
        "aura-x86_64-apple-darwin.tar.gz",
        "aura-x86_64-unknown-linux-gnu.tar.gz",
    )
    if any(f"/download/{tag}/{target}" not in formula for target in targets):
        fail("formula release URLs do not bind the canonical tag")
    resolved = _git(["rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"])
    if resolved.decode("ascii").strip() != commit:
        fail("tag does not resolve to verified commit")
    return 0


def _verify_registered_tuple(path: Path, identifier: str) -> None:
    row = load_registry(Path("qa/compliance-qa-registry.json")).by_id()[identifier]
    value = decode_json_bytes((path / "env.json").read_bytes(), f"{identifier} env.json")
    if not isinstance(value, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in value.items()):
        fail(f"invalid tuple environment: {identifier}")
    verify_tuple(path, row, {key: item for key, item in value.items() if isinstance(item, str)})


def _git(arguments: list[str]) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        env={**os.environ, "GIT_MASTER": "1"},
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        fail(f"git command failed: {result.stderr.decode('utf-8', errors='replace')}")
    return result.stdout


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
