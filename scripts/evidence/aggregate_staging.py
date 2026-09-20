from __future__ import annotations

import shutil
import stat
from pathlib import Path

from .aggregate_git import _commit_subjects, _subject_commit, _task_commit
from .manifest import write_manifest
from .model import canonical_json_bytes, fail, sha256_bytes, sha256_file, write_new
from .operations import merge_operations
from .receipt import LANE_JOBS
from .registry import CommandSpec, Registry


def _copy_inputs(output: Path, lanes: Path, lane_roots: dict[str, Path], producers: dict[str, Path]) -> None:
    for producer, root in producers.items():
        _copy_tree(root, output / producer)
    lane_output = output / "lanes"
    lane_output.mkdir(mode=0o700)
    for job in LANE_JOBS:
        _copy_file(lanes / f"{job}.tar.gz", lane_output / f"{job}.tar.gz")


def _assemble_owner_evidence(
    output: Path, lanes: dict[str, Path], registry: Registry, verified_commit: str
) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    commit_subjects = _commit_subjects(verified_commit)
    task_entries: list[dict[str, object]] = []
    for task in range(1, 18):
        rows = [row for row in registry.rows if row.task_owner == str(task)]
        root = output / "tasks" / f"task-{task}"
        commands = _copy_owner_tuples(root, rows, lanes)
        task_commit = verified_commit if task == 17 else _task_commit(task, commit_subjects)
        manifest = write_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
        receipt = {
            "commands": commands,
            "manifest_sha256": manifest,
            "schema_version": 1,
            "status": "approved",
            "task": str(task),
            "task_commit": task_commit,
            "verified_commit": verified_commit,
        }
        write_new(root / "receipt.json", canonical_json_bytes(receipt))
        task_entries.append({"receipt_sha256": sha256_file(root / "receipt.json"), "task": str(task)})
    merge_rows = [row for row in registry.rows if row.task_owner == "merge"]
    merge_root = output / "merges" / "M1516"
    commands = _copy_owner_tuples(merge_root, merge_rows, lanes)
    merge_commit = _subject_commit("merge: combine distribution and documentation", commit_subjects)
    manifest = write_manifest(merge_root, frozenset({"SHA256SUMS", "receipt.json"}))
    merge_receipt = {
        "commands": commands,
        "manifest_sha256": manifest,
        "merge": "M1516",
        "merge_commit": merge_commit,
        "schema_version": 1,
        "status": "approved",
        "verified_commit": verified_commit,
    }
    write_new(merge_root / "receipt.json", canonical_json_bytes(merge_receipt))
    return task_entries, [{"merge": "M1516", "receipt_sha256": sha256_file(merge_root / "receipt.json")}]


def _copy_owner_tuples(
    root: Path, rows: list[CommandSpec], lanes: dict[str, Path]
) -> list[dict[str, object]]:
    commands: list[dict[str, object]] = []
    for row in rows:
        source = lanes[row.execution_context] / "tuples" / row.id
        destination = root / "tuples" / row.execution_context / row.id
        _copy_tree(source, destination)
        commands.append({
            "id": row.id,
            "lane": row.execution_context,
            "tuple_sha256": sha256_bytes((source / "SHA256SUMS").read_bytes()),
        })
    return sorted(commands, key=lambda value: str(value["id"]))


def _copy_releases(output: Path, lanes: dict[str, Path]) -> list[dict[str, object]]:
    seen: dict[str, str] = {}
    for root in lanes.values():
        release_root = root / "releases"
        if not release_root.is_dir():
            continue
        for source in release_root.rglob("*"):
            if not source.is_file() or source.is_symlink():
                continue
            relative = source.relative_to(root).as_posix()
            digest = sha256_file(source)
            if relative in seen and seen[relative] != digest:
                fail(f"conflicting release input: {relative}")
            if relative not in seen:
                _copy_file(source, output / relative)
                seen[relative] = digest
    expected = {
        "releases/aura-aarch64-apple-darwin.tar.gz",
        "releases/aura-aarch64-unknown-linux-gnu.tar.gz",
        "releases/aura-x86_64-apple-darwin.tar.gz",
        "releases/aura-x86_64-unknown-linux-gnu.tar.gz",
        "releases/homebrew/aura.rb",
    }
    if set(seen) != expected:
        fail(f"aggregate release set differs: {sorted(seen)}")
    return [{"path": path, "sha256": seen[path]} for path in sorted(seen)]


def _merge_logs(
    output: Path,
    producers: dict[str, Path],
    lanes: dict[str, Path],
    sources: tuple[str, ...],
    requested_output: Path,
) -> None:
    expected_output = output / "operations" / "evidence-operations.log"
    if requested_output.resolve(strict=False) != expected_output.resolve(strict=False):
        fail("merged operations output path differs from aggregate layout")
    locations = {
        "preflight": producers["preflight"] / "operations.log",
        "source": producers["source"] / "operations.log",
        **{job: root / "operations.log" for job, root in lanes.items()},
    }
    merge_operations([(name, locations[name]) for name in sources], expected_output)


def _copy_tree(source: Path, destination: Path) -> None:
    if destination.exists() or destination.is_symlink():
        fail(f"copy destination already exists: {destination}")
    for path in [source, *source.rglob("*")]:
        metadata = path.lstat()
        if stat.S_ISLNK(metadata.st_mode) or (not stat.S_ISDIR(metadata.st_mode) and not stat.S_ISREG(metadata.st_mode)):
            fail(f"unsupported evidence input: {path}")
    shutil.copytree(source, destination, copy_function=shutil.copyfile)


def _copy_file(source: Path, destination: Path) -> None:
    if source.is_symlink() or not source.is_file():
        fail(f"copy source is not regular: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    raw = source.read_bytes()
    write_new(destination, raw)
    if sha256_file(destination) != sha256_file(source):
        fail(f"copy digest mismatch: {source}")
