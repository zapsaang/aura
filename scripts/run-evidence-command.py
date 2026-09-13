#!/usr/bin/env python3
from __future__ import annotations

import argparse
import shlex
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from evidence.environment import load_environment
from evidence.lane import worktree_state
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.operations import append_operation
from evidence.registry import load_registry
from evidence.tuple import write_tuple


def main() -> int:
    parser = argparse.ArgumentParser(description="Run one closed-registry command and seal its tuple")
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--id", required=True)
    parser.add_argument("--environment", type=Path, required=True)
    parser.add_argument("--tuple", dest="tuple_path", type=Path, required=True)
    parser.add_argument("--operations", type=Path)
    parser.add_argument("--workspace", type=Path, default=Path("."))
    parser.add_argument("--env", action="append", default=[])
    args = parser.parse_args()
    registry = load_registry(args.registry)
    try:
        spec = registry.by_id()[args.id]
    except KeyError as error:
        raise EvidenceError(f"unknown registry ID: {args.id}") from error
    declared = _declared_environment(args.env)
    base = load_environment(args.environment)
    child_environment = {**base, **declared}
    workspace = args.workspace.resolve(strict=True)
    before = worktree_state(workspace)
    if before[2]:
        raise EvidenceError(f"registry command requires a clean checkout: {spec.id}")
    result = subprocess.run(
        [*shlex.split(spec.shell), "-c", spec.command],
        cwd=workspace / spec.cwd,
        env=child_environment,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=False,
    )
    after = worktree_state(workspace)
    if after != before:
        raise EvidenceError(f"registry command mutated worktree state: {spec.id}")
    digest = write_tuple(args.tuple_path, spec, declared, result.returncode, result.stdout, result.stderr)
    if args.operations is not None:
        append_operation(args.operations, spec.id, None, digest)
    print(canonical_json_bytes({"id": spec.id, "tuple_sha256": digest}).decode("utf-8"), end="")
    return 0


def _declared_environment(values: list[str]) -> dict[str, str]:
    result: dict[str, str] = {}
    for value in values:
        key, separator, item = value.partition("=")
        if not separator or not key or key in result:
            raise EvidenceError(f"invalid or duplicate --env value: {value}")
        result[key] = item
    return {key: result[key] for key in sorted(result)}


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"error: {error}") from error
