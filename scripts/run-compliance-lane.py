#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True

from evidence.aggregate import _commit_subjects, _subject_commit, _task_commit
from evidence.archive import create_deterministic_archive
from evidence.environment import create_environment
from evidence.lane import expected_lane_rows, require_checkout_identity, seal_lane
from evidence.model import EvidenceError, canonical_json_bytes, fail
from evidence.registry import CommandSpec, load_registry
from evidence.secure_file import copy_regular


def main() -> int:
    parser = argparse.ArgumentParser(description="Execute, seal, and archive one complete registry lane")
    parser.add_argument("--job", required=True)
    parser.add_argument("--lane", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--runner", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--feature", action="append", default=[])
    parser.add_argument("--binding", action="append", default=[])
    parser.add_argument("--release", action="append", default=[])
    parser.add_argument("--rustc")
    parser.add_argument("--cargo")
    parser.add_argument("--cross")
    parser.add_argument("--nix")
    args = parser.parse_args()
    require_checkout_identity(Path.cwd(), args.verified_commit)
    registry = load_registry(args.registry)
    if args.lane.exists() or args.lane.is_symlink():
        fail(f"lane must be fresh: {args.lane}")
    environment_path = args.lane / "environment.json"
    control = args.lane.parent / f".{args.job}-control"
    # Lane commands bind unix sockets under TMPDIR; keep the path far below SUN_LEN.
    temporary = Path(tempfile.mkdtemp(prefix="aura-lane-tmp-"))
    os.chmod(temporary, 0o700)
    original_home = Path.home()
    try:
        create_environment(
            environment_path,
            control / "home",
            os.environ.get("PATH", ""),
            temporary / "tmp",
        )
        _link_tool_home(control / "home", original_home)
        bindings = _bindings(args.binding)
        subjects = _commit_subjects(args.verified_commit)
        for row in _execution_order(expected_lane_rows(registry, args.job)):
            command = [
                sys.executable,
                "-B",
                "scripts/run-evidence-command.py",
                "--registry",
                os.fspath(args.registry),
                "--id",
                row.id,
                "--environment",
                os.fspath(environment_path),
                "--tuple",
                os.fspath(args.lane / "tuples" / row.id),
                "--operations",
                os.fspath(args.lane / "operations.log"),
            ]
            for key, value in _row_environment(row, bindings, subjects).items():
                command.extend(("--env", f"{key}={value}"))
            result = subprocess.run(command, check=False)
            if result.returncode != 0:
                fail(f"lane command failed: {row.id}")
        for value in args.release:
            source, separator, destination = value.partition("=")
            if not separator or not destination.startswith("releases/"):
                fail(f"invalid --release mapping: {value}")
            copy_regular(Path(source), args.lane / destination)
        seal_lane(
            args.lane,
            registry,
            job=args.job,
            run_id=args.run_id,
            run_attempt=args.run_attempt,
            verified_commit=args.verified_commit,
            runner=args.runner,
            target=args.target,
            features=args.feature,
            tools={
                "cargo": _tool_value("cargo", args.cargo),
                "cross": _cross_value(args.cross),
                "nix": _tool_value("nix", args.nix),
                "rustc": _tool_value("rustc", args.rustc),
            },
        )
    finally:
        shutil.rmtree(control, ignore_errors=True)
        shutil.rmtree(temporary, ignore_errors=True)
    digest = create_deterministic_archive(args.lane, args.archive, f"lane/{args.job}")
    print(canonical_json_bytes({"job": args.job, "sha256": digest, "status": "ok"}).decode("utf-8"), end="")
    return 0


def _bindings(values: list[str]) -> dict[str, str]:
    result: dict[str, str] = {}
    for value in values:
        key, separator, item = value.partition("=")
        if not separator or not key or key in result:
            fail(f"invalid or duplicate binding: {value}")
        result[key] = item
    return result


def _execution_order(rows: tuple[CommandSpec, ...]) -> list[CommandSpec]:
    def priority(row: CommandSpec) -> tuple[int, str]:
        if row.id == "t15-02":
            return 0, row.id
        if row.id.endswith("-build"):
            return 10, row.id
        if row.id.endswith("-package") or row.id == "homebrew-render":
            return 20, row.id
        if row.id == "homebrew-ruby-syntax":
            return 21, row.id
        if row.id.startswith("tip-"):
            return 90, row.id
        return 30, row.id

    return sorted(rows, key=priority)


def _row_environment(
    row: CommandSpec, bindings: dict[str, str], subjects: dict[str, list[str]]
) -> dict[str, str]:
    result: dict[str, str] = {}
    for key, declared in row.env.items():
        if not declared.startswith("<"):
            result[key] = declared
        elif key == "TASK_COMMIT":
            task = int(row.id.removeprefix("tip-t"))
            result[key] = _task_commit(task, subjects)
        elif key == "MERGE_COMMIT":
            result[key] = _subject_commit("merge: combine distribution and documentation", subjects)
        elif key == "PREREQUISITE_COMMIT":
            result[key] = _subject_commit("refactor: establish rust 1.70 and module prerequisites", subjects)
        elif key in bindings:
            result[key] = bindings[key]
        else:
            fail(f"missing runtime binding for {row.id}: {key}")
    return result


def _tool_value(command: str, configured: str | None) -> str | None:
    if configured != "auto":
        return configured
    result = subprocess.run(
        [command, "--version"],
        check=False,
        capture_output=True,
    )
    if result.returncode != 0 or result.stderr:
        fail(f"unable to capture {command} version")
    value = result.stdout.decode("utf-8").strip()
    if not value or "\n" in value:
        fail(f"invalid {command} version output")
    return value


def _cross_value(configured: str | None) -> str | None:
    if configured != "auto":
        return configured
    result = subprocess.run(
        ["cross", "--version"],
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        fail("unable to capture cross version")
    first_line = result.stdout.decode("utf-8").splitlines()[0] if result.stdout else ""
    if re.fullmatch(r"cross \d+\.\d+\.\d+", first_line) is None:
        fail("unable to capture cross version")
    return first_line


def _link_tool_home(home: Path, original: Path) -> None:
    if home.resolve() == original.resolve():
        return
    for name in (".cargo", ".rustup", ".nix-profile"):
        source = original / name
        target = home / name
        if source.exists() and not target.exists():
            target.symlink_to(source, target_is_directory=source.is_dir())


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"error: {error}") from error
