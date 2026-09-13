#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True

from evidence.aggregate import _commit_subjects, _subject_commit
from evidence.archive import create_deterministic_archive
from evidence.environment import create_environment
from evidence.model import (
    EvidenceError,
    canonical_json_bytes,
    fail,
    parse_json_bytes,
    require_hex,
    require_uint,
    sha256_bytes,
    write_new,
)
from evidence.operations import append_operation
from evidence.producer import seal_producer
from evidence.registry import load_registry
from evidence.secure_file import read_regular, require_directory_identity

# The original one-time baseline import retained the REAL, long-lived protected
# source checkout at this in-repo location. That stable path is the source root
# recorded in source-root.json; the temporary worktree created below is only the
# execution site of pre-01 and is deleted afterwards, so recording ITS
# path/device/inode would make F4's cross-job descriptor-reopen check
# unpassable. The recorded source root must therefore outlive this script and
# remain valid on any later runner.
RETAINED_PREFLIGHT = Path(".omo") / "evidence" / "design-compliance-remediation" / "preflight"

SOURCE_ROOT_KEYS = frozenset({
    "device", "head_tree", "imported_baseline_commit", "inode", "schema_version", "source_root",
})


def main() -> int:
    parser = argparse.ArgumentParser(description="Reproduce and seal a historical compliance producer")
    parser.add_argument("--producer", choices=("preflight", "prerequisite"), required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--retained-source-root", type=Path)
    parser.add_argument("--run-id")
    parser.add_argument("--run-attempt", type=int)
    args = parser.parse_args()
    if args.root.exists() or args.root.is_symlink() or args.archive.exists() or args.archive.is_symlink():
        fail("historical producer outputs must be fresh")
    head = _git(Path.cwd(), ["rev-parse", "--verify", "HEAD^{commit}"]).decode("ascii").strip()
    subjects = _commit_subjects(head)
    subject = {
        "preflight": "chore: materialize audited remediation baseline",
        "prerequisite": "refactor: establish rust 1.70 and module prerequisites",
    }[args.producer]
    commit = _subject_commit(subject, subjects)
    registry = load_registry(args.registry)
    with tempfile.TemporaryDirectory(prefix=f"aura-{args.producer}-") as raw:
        temporary = Path(raw)
        temporary.chmod(0o700)
        checkout = temporary / "checkout"
        _add_worktree(checkout, commit)
        try:
            environment = temporary / "environment.json"
            home = temporary / "home"
            create_environment(environment, home, os.environ.get("PATH", ""), temporary / "tmp")
            _link_tool_home(home, Path.home())
            identifier = "pre-01" if args.producer == "preflight" else "tip-prerequisite"
            command = [
                sys.executable,
                "-B",
                os.fspath(Path(__file__).with_name("run-evidence-command.py")),
                "--registry",
                os.fspath(args.registry.resolve(strict=True)),
                "--id",
                identifier,
                "--environment",
                os.fspath(environment),
                "--workspace",
                os.fspath(checkout),
                "--tuple",
                os.fspath(args.root / "tuples" / identifier),
            ]
            if args.producer == "preflight":
                command.extend(("--operations", os.fspath(args.root / "operations.log")))
            else:
                command.extend(("--env", f"PREREQUISITE_COMMIT={commit}"))
            result = subprocess.run(command, check=False)
            if result.returncode != 0:
                fail(f"historical producer command failed: {identifier}")
            if args.producer == "preflight":
                retained = args.retained_source_root or Path.cwd() / RETAINED_PREFLIGHT
                _capture_preflight(
                    args.root,
                    checkout,
                    commit,
                    retained,
                    transport=args.retained_source_root is not None,
                )
            seal_producer(
                args.root,
                registry,
                args.producer,
                run_id=args.run_id,
                run_attempt=args.run_attempt,
                commit=commit,
            )
            digest = create_deterministic_archive(args.root, args.archive, args.producer)
        finally:
            _remove_worktree(checkout)
    print(canonical_json_bytes({"producer": args.producer, "sha256": digest, "status": "ok"}).decode("utf-8"), end="")
    return 0


def _capture_preflight(
    root: Path,
    checkout: Path,
    baseline: str,
    retained: Path,
    *,
    transport: bool = False,
) -> None:
    capture = _retained_source_root(retained if transport else retained / "source-root.json")
    device = require_uint(capture["device"], "device")
    inode = require_uint(capture["inode"], "inode")
    source_root = Path(str(capture["source_root"]))
    transporting = transport and not source_root.exists()
    if not transporting:
        require_directory_identity(source_root, device, inode, "recorded source root")
    record = {
        "device": device,
        "head_tree": require_hex(capture["head_tree"], 40, "head_tree"),
        "imported_baseline_commit": baseline,
        "inode": inode,
        "schema_version": 1,
        "source_root": capture["source_root"],
    }
    record_raw = canonical_json_bytes(record)
    write_new(root / "source-root.json", record_raw)
    write_new(root / "worktree" / "HEAD", f"{baseline}\n".encode("ascii"))
    status = _git(checkout, ["status", "--porcelain=v1", "-z", "--untracked-files=all"])
    if status:
        fail("historical preflight checkout is dirty")
    write_new(root / "worktree" / "status.bin", status)
    if transporting:
        append_operation(
            root / "operations.log",
            "transport-source-root",
            None,
            sha256_bytes(record_raw),
        )
    else:
        append_operation(root / "operations.log", "capture-source-root", None, None)


def _retained_source_root(path: Path) -> dict[str, object]:
    value = parse_json_bytes(read_regular(path), SOURCE_ROOT_KEYS, "retained source-root.json")
    if value["schema_version"] != 1:
        fail("unsupported retained source-root schema")
    source_root = value["source_root"]
    if not isinstance(source_root, str) or not Path(source_root).is_absolute():
        fail("retained source_root must be an absolute path")
    return value


def _add_worktree(path: Path, commit: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        ["git", "worktree", "add", "--detach", os.fspath(path), commit],
        env={**os.environ, "GIT_MASTER": "1"},
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        fail(f"cannot create historical worktree: {result.stderr.decode('utf-8', errors='replace')}")
    path.chmod(0o700)


def _remove_worktree(path: Path) -> None:
    if not path.exists():
        return
    result = subprocess.run(
        ["git", "worktree", "remove", "--force", os.fspath(path)],
        env={**os.environ, "GIT_MASTER": "1"},
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        fail(f"cannot remove historical worktree: {result.stderr.decode('utf-8', errors='replace')}")


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


def _link_tool_home(home: Path, original: Path) -> None:
    for name in (".cargo", ".rustup"):
        source = original / name
        if source.exists():
            (home / name).symlink_to(source, target_is_directory=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"error: {error}") from error
