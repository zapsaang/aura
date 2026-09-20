#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path

from evidence.archive import create_deterministic_archive
from evidence.model import (
    EvidenceError,
    canonical_json_bytes,
    fail,
    require_hex,
    sha256_bytes,
    write_new,
)
from evidence.operations import append_operation
from evidence.producer import seal_producer
from evidence.registry import load_registry
from evidence.secure_file import copy_regular


def main() -> int:
    parser = argparse.ArgumentParser(description="Capture, seal, and archive the final source producer")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--matrix", type=Path, required=True)
    parser.add_argument("--plan-sha256", required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    args = parser.parse_args()
    commit = require_hex(args.verified_commit, 40, "verified_commit")
    plan_sha256 = require_hex(args.plan_sha256, 64, "plan_sha256")
    if args.root.exists() or args.root.is_symlink() or args.archive.exists() or args.archive.is_symlink():
        fail("source producer outputs must be fresh")
    if _git(["rev-parse", "--verify", "HEAD^{commit}"]).decode("ascii").strip() != commit:
        fail("source producer checkout differs from verified commit")
    if _git(["diff", "--quiet", "--"]) or _git(["diff", "--cached", "--quiet", "--"]):
        fail("source producer checkout has tracked changes")
    if _git(["status", "--porcelain=v1", "-z", "--untracked-files=all"]):
        fail("source producer checkout is not clean")
    matrix_destination = args.root / "compliance-matrix.json"
    matrix_digest = copy_regular(args.matrix, matrix_destination)
    head_tree = _git(["rev-parse", "--verify", "HEAD^{tree}"]).decode("ascii").strip()
    artifacts = {
        "compliance_matrix_sha256": matrix_digest,
        "head_tree": require_hex(head_tree, 40, "head_tree"),
        "plan_sha256": plan_sha256,
        "porcelain": "",
        "schema_version": 1,
        "verified_commit": commit,
    }
    artifacts_raw = canonical_json_bytes(artifacts)
    write_new(args.root / "source-artifacts.json", artifacts_raw)
    append_operation(
        args.root / "operations.log",
        "capture-source-artifacts",
        matrix_digest,
        sha256_bytes(artifacts_raw),
    )
    seal_producer(
        args.root,
        load_registry(args.registry),
        "source",
        run_id=args.run_id,
        run_attempt=args.run_attempt,
        commit=commit,
    )
    archive_digest = create_deterministic_archive(args.root, args.archive, "source")
    print(canonical_json_bytes({"producer": "source", "sha256": archive_digest, "status": "ok"}).decode("utf-8"), end="")
    return 0


def _git(arguments: list[str]) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        env={**os.environ, "GIT_MASTER": "1"},
        check=False,
        capture_output=True,
    )
    if result.returncode not in {0, 1}:
        fail(f"git command failed: {result.stderr.decode('utf-8', errors='replace')}")
    if arguments[0] not in {"diff"} and result.returncode != 0:
        fail(f"git command failed: {result.stderr.decode('utf-8', errors='replace')}")
    return result.stdout if result.returncode == 0 else b"dirty"


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
