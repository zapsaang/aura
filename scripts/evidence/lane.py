from __future__ import annotations

import os
import subprocess
from pathlib import Path

from .environment import environment_sha256
from .manifest import verify_manifest, write_manifest
from .model import (
    canonical_json_bytes,
    decode_json_bytes,
    fail,
    sha256_file,
    write_new,
)
from .operations import load_operations
from .receipt import validate_lane_receipt
from .registry import CommandSpec, Registry
from .tuple import TupleResult, verify_tuple


def expected_lane_rows(registry: Registry, job: str) -> tuple[CommandSpec, ...]:
    return tuple(
        row for row in registry.rows
        if row.execution_context == job and row.task_owner not in {"preflight", "prerequisite", "final"}
    )


def seal_lane(
    root: Path,
    registry: Registry,
    *,
    job: str,
    run_id: str,
    run_attempt: int,
    verified_commit: str,
    runner: str,
    target: str,
    features: list[str],
    tools: dict[str, str | None],
) -> dict[str, object]:
    if (root / "receipt.json").exists() or (root / "SHA256SUMS").exists():
        fail("lane is already sealed")
    commands = _verify_command_closure(root, registry, job)
    environment_digest = environment_sha256(root / "environment.json")
    load_operations(root / "operations.log")
    releases = _release_entries(root)
    manifest_digest = write_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
    status = "build-only" if job in {"release-linux-arm64", "release-macos-x86"} else "executed"
    receipt: dict[str, object] = {
        "commands": commands,
        "environment_sha256": environment_digest,
        "features": sorted(features),
        "job": job,
        "manifest_sha256": manifest_digest,
        "release_archives": releases,
        "run_attempt": run_attempt,
        "run_id": run_id,
        "runner": runner,
        "schema_version": 1,
        "status": status,
        "target": target,
        "tools": tools,
        "verified_commit": verified_commit,
    }
    validate_lane_receipt(receipt, job, verified_commit)
    write_new(root / "receipt.json", canonical_json_bytes(receipt))
    return receipt


def verify_lane_tree(root: Path, registry: Registry, job: str, verified_commit: str) -> dict[str, object]:
    raw = (root / "receipt.json").read_bytes()
    receipt = decode_json_bytes(raw, f"{job} receipt")
    if not isinstance(receipt, dict):
        fail(f"{job} receipt must be an object")
    validate_lane_receipt(receipt, job, verified_commit)
    manifest_digest = verify_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
    if receipt["manifest_sha256"] != manifest_digest:
        fail(f"{job} manifest digest drift")
    if receipt["environment_sha256"] != environment_sha256(
        root / "environment.json", require_directories=False
    ):
        fail(f"{job} environment digest drift")
    commands = _verify_command_closure(root, registry, job)
    if receipt["commands"] != commands:
        fail(f"{job} command closure drift")
    if receipt["release_archives"] != _release_entries(root):
        fail(f"{job} release archive drift")
    load_operations(root / "operations.log")
    return receipt


def _verify_command_closure(root: Path, registry: Registry, job: str) -> list[dict[str, object]]:
    rows = expected_lane_rows(registry, job)
    tuple_root = root / "tuples"
    actual = sorted(path.name for path in tuple_root.iterdir() if path.is_dir()) if tuple_root.is_dir() else []
    expected = sorted(row.id for row in rows)
    if actual != expected:
        fail(f"{job} tuple closure differs: expected={expected}, actual={actual}")
    commands: list[dict[str, object]] = []
    for row in rows:
        path = tuple_root / row.id
        environment = _tuple_environment(path)
        result: TupleResult = verify_tuple(path, row, environment)
        commands.append({
            "exit_code": 0,
            "id": row.id,
            "tests_executed": result.tests_executed,
            "tuple_sha256": result.tuple_sha256,
        })
    return sorted(commands, key=lambda value: str(value["id"]))


def _tuple_environment(path: Path) -> dict[str, str]:
    value = decode_json_bytes((path / "env.json").read_bytes(), f"{path}/env.json")
    if not isinstance(value, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in value.items()):
        fail(f"tuple environment must be a string object: {path}")
    return {key: item for key, item in value.items() if isinstance(item, str)}


def _release_entries(root: Path) -> list[dict[str, object]]:
    releases = root / "releases"
    if not releases.exists():
        return []
    result: list[dict[str, object]] = []
    for path in releases.rglob("*"):
        if path.is_symlink() or (not path.is_file() and not path.is_dir()):
            fail(f"unsupported release path in lane: {path}")
        if path.is_file():
            result.append({"path": path.relative_to(root).as_posix(), "sha256": sha256_file(path)})
    return sorted(result, key=lambda value: str(value["path"]))


def _git_output(root: Path, *arguments: str) -> bytes:
    environment = {**os.environ, "GIT_MASTER": "1"}
    return subprocess.run(
        ["git", *arguments],
        check=True,
        capture_output=True,
        env=environment,
        cwd=root,
    ).stdout


def checkout_head(root: Path) -> str:
    return _git_output(root, "rev-parse", "HEAD").decode("utf-8").strip()


def checkout_porcelain(root: Path) -> bytes:
    return _git_output(root, "status", "--porcelain=v1", "-z", "--untracked-files=all")


def require_checkout_identity(root: Path, verified_commit: str) -> None:
    actual = checkout_head(root)
    if actual != verified_commit:
        fail(f"lane checkout HEAD {actual} does not match verified commit {verified_commit}")
    if checkout_porcelain(root):
        fail(f"lane checkout is not clean: {root}")


def worktree_state(root: Path) -> tuple[bytes, bytes, bytes]:
    unstaged = _git_output(root, "diff", "--binary", "--full-index", "--")
    staged = _git_output(root, "diff", "--cached", "--binary", "--full-index", "--")
    return unstaged, staged, checkout_porcelain(root)
