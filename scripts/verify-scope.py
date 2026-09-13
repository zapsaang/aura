#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path, PurePosixPath

from evidence.manifest import verify_manifest
from evidence.model import (
    EvidenceError,
    canonical_json_bytes,
    fail,
    parse_json_bytes,
    require_hex,
    write_new,
)
from evidence.secure_file import read_regular

SOURCE_KEYS = frozenset({
    "device", "head_tree", "imported_baseline_commit", "inode", "schema_version", "source_root",
})
ALLOWED_ROOTS = frozenset({
    ".github", ".gitignore", "Cargo.lock", "Cargo.toml", "README.md", "aura-cli", "aura-common",
    "aura-daemon", "deployment", "docs", "qa", "scripts",
})
FORBIDDEN = frozenset({"package.json", "bun.lock", "tatus"})


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify remediation commit ancestry and changed-path scope")
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--preflight", type=Path, required=True)
    parser.add_argument("--base-from-preflight", action="store_true")
    parser.add_argument("--head", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if not args.base_from_preflight:
        fail("scope base must be obtained from preflight")
    read_regular(args.plan)
    verify_manifest(args.preflight, frozenset({"SHA256SUMS", "receipt.json"}))
    source = parse_json_bytes(read_regular(args.preflight / "source-root.json"), SOURCE_KEYS, "source-root.json")
    base = require_hex(source["imported_baseline_commit"], 40, "imported_baseline_commit")
    head = require_hex(args.head, 40, "head")
    if _git(["rev-parse", "--verify", "HEAD^{commit}"]).decode("ascii").strip() != head:
        fail("scope head is not the caller checkout HEAD")
    _git(["merge-base", "--is-ancestor", base, head])
    raw_paths = _git(["diff", "--name-only", "-z", f"{base}..{head}", "--"])
    paths = [item.decode("utf-8") for item in raw_paths.split(b"\x00") if item]
    if not paths or paths != sorted(set(paths)):
        fail("scope path list must be nonempty, sorted, and duplicate-free")
    for value in paths:
        path = PurePosixPath(value)
        if path.is_absolute() or ".." in path.parts or path.parts[0] not in ALLOWED_ROOTS or value in FORBIDDEN:
            fail(f"out-of-scope changed path: {value}")
    receipt = {"base": base, "changed_paths": paths, "head": head, "schema_version": 1, "status": "approved"}
    write_new(args.out, canonical_json_bytes(receipt))
    return 0


def _git(arguments: list[str]) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        env={**os.environ, "GIT_MASTER": "1"},
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        fail(f"git {' '.join(arguments)} failed: {result.stderr.decode('utf-8', errors='replace')}")
    return result.stdout


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError, UnicodeDecodeError) as error:
        raise SystemExit(f"error: {error}") from error
