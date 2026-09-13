#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path

from evidence.manifest import verify_manifest
from evidence.model import (
    EvidenceError,
    fail,
    parse_json_bytes,
    require_hex,
    require_uint,
    sha256_bytes,
    write_new,
)
from evidence.secure_file import read_regular, require_directory_identity

SOURCE_KEYS = frozenset({
    "device", "head_tree", "imported_baseline_commit", "inode", "schema_version", "source_root",
})


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify the immutable audited source and operational plan")
    parser.add_argument("--preflight", type=Path, required=True)
    parser.add_argument("--source-root-from-preflight", action="store_true")
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--plan-sha256", required=True)
    parser.add_argument("--allowlist", nargs="+", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if not args.source_root_from_preflight:
        fail("source root must be obtained from preflight")
    verify_manifest(args.preflight, frozenset({"SHA256SUMS", "receipt.json"}))
    source_value = parse_json_bytes(read_regular(args.preflight / "source-root.json"), SOURCE_KEYS, "source-root.json")
    if source_value["schema_version"] != 1:
        fail("unsupported source-root schema")
    source_root = Path(str(source_value["source_root"]))
    device = require_uint(source_value["device"], "device")
    inode = require_uint(source_value["inode"], "inode")
    require_directory_identity(source_root, device, inode, "captured source root")
    plan_raw = read_regular(args.plan)
    digest = sha256_bytes(plan_raw)
    if digest != require_hex(args.plan_sha256, 64, "plan_sha256"):
        fail("operational plan digest drift")
    source_plan = source_root / ".omo" / "plans" / args.plan.name
    if read_regular(source_plan) != plan_raw:
        fail("source and operational plan copies differ")
    head_tree = require_head_tree(source_root, require_hex(source_value["head_tree"], 40, "head_tree"))
    expected_status = read_regular(args.preflight / "worktree" / "status.bin")
    actual_status = _git(source_root, ["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    if actual_status != expected_status:
        fail("source worktree status drift")
    for relative in args.allowlist:
        path = source_root / relative
        if not path.is_file() or path.is_symlink():
            fail(f"allowlisted product input is missing or unsafe: {relative}")
        read_regular(path)
    lines = [
        "# F4 Source Fidelity",
        "",
        f"- Source root: `{source_root}`",
        f"- HEAD tree: `{head_tree}`",
        f"- Plan SHA-256: `{digest}`",
        f"- Allowlisted paths: {len(args.allowlist)}",
        "- Status: approved",
    ]
    write_new(args.out, ("\n".join(lines) + "\n").encode("utf-8"))
    return 0


def require_head_tree(source_root: Path, expected: str) -> str:
    actual = _git(source_root, ["rev-parse", "--verify", "HEAD^{tree}"]).decode("ascii").strip()
    if actual != expected:
        fail(f"source HEAD tree mismatch: recorded {expected}, actual {actual}")
    return actual


def _git(root: Path, arguments: list[str]) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        cwd=root,
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
