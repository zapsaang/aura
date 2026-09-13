import os
import re
import shutil
import stat
import subprocess
import tempfile
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

COMMIT_PATTERN = re.compile(r"[0-9a-f]{40}")


class GateError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class GateConfig:
    commit: str
    toolchain: str
    cargo_action: str


def _run(
    command: Sequence[str],
    cwd: Path,
) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(
            command,
            cwd=cwd,
            check=False,
            stdin=subprocess.DEVNULL,
            capture_output=True,
        )
    except OSError as error:
        raise GateError(f"cannot execute {command[0]}: {error}") from error


def _require_success(
    result: subprocess.CompletedProcess[bytes],
    label: str,
) -> None:
    if result.returncode == 0:
        return
    stdout = result.stdout.decode("utf-8", errors="replace")
    stderr = result.stderr.decode("utf-8", errors="replace")
    raise GateError(
        f"{label} failed with exit {result.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    )


def _git(root: Path, arguments: Sequence[str], label: str) -> bytes:
    try:
        result = subprocess.run(
            ["git", *arguments],
            cwd=root,
            check=False,
            env={**os.environ, "GIT_MASTER": "1"},
            stdin=subprocess.DEVNULL,
            capture_output=True,
        )
    except OSError as error:
        raise GateError(f"cannot execute git: {error}") from error
    _require_success(result, label)
    return result.stdout


def _resolve_commit(root: Path, commit: str) -> str:
    if COMMIT_PATTERN.fullmatch(commit) is None:
        raise GateError("commit must be exactly 40 lowercase hexadecimal characters")
    resolved = _git(
        root,
        ["rev-parse", "--verify", "--end-of-options", f"{commit}^{{commit}}"],
        "resolve commit",
    ).strip()
    try:
        decoded = resolved.decode("ascii")
    except UnicodeDecodeError as error:
        raise GateError("resolved commit is not ASCII") from error
    if decoded != commit:
        raise GateError(f"commit resolved to {decoded}, expected {commit}")
    return decoded


def _porcelain(root: Path) -> bytes:
    return _git(
        root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        "inspect worktree state",
    )


def _lock_bytes(root: Path) -> bytes:
    try:
        return (root / "Cargo.lock").read_bytes()
    except OSError as error:
        raise GateError(f"cannot read Cargo.lock: {error}") from error


def _remove_scratch(root: Path, scratch: Path, sandbox: Path, added: bool) -> None:
    if added:
        _git(root, ["worktree", "remove", "--force", os.fspath(scratch)], "remove scratch worktree")
    try:
        shutil.rmtree(sandbox)
    except OSError as error:
        raise GateError(f"remove scratch directory: {error}") from error


def run_gate(config: GateConfig) -> None:
    try:
        root = Path.cwd().resolve(strict=True)
    except OSError as error:
        raise GateError(f"resolve caller checkout: {error}") from error
    commit = _resolve_commit(root, config.commit)
    caller_state = _porcelain(root)
    if caller_state:
        raise GateError("caller checkout must be clean")
    caller_lock = _lock_bytes(root)
    sandbox = Path(tempfile.mkdtemp(prefix="aura-gate-"))
    scratch = sandbox / "checkout"
    added = False
    failure = None
    try:
        _git(
            root,
            ["worktree", "add", "--detach", os.fspath(scratch), commit],
            "create scratch worktree",
        )
        added = True
        scratch.chmod(0o700)
        if stat.S_IMODE(scratch.stat().st_mode) != 0o700:
            raise GateError("scratch worktree mode is not 0700")
        if _porcelain(scratch):
            raise GateError("scratch worktree is dirty before verification")
        scratch_lock = _lock_bytes(scratch)
        commands = [
            ["python3", "scripts/check-rust-loc.py", "--root", "."],
            [
                "cargo",
                f"+{config.toolchain}",
                config.cargo_action,
                "--workspace",
                "--locked",
            ],
        ]
        for index, command in enumerate(commands, start=1):
            child = _run(command, scratch)
            _require_success(child, f"gate child {index}")
        if _lock_bytes(scratch) != scratch_lock:
            raise GateError("gate children changed Cargo.lock")
        if _porcelain(scratch):
            raise GateError("gate children changed the scratch worktree")
    except GateError as error:
        failure = error
    finally:
        try:
            _remove_scratch(root, scratch, sandbox, added)
        except GateError as error:
            failure = error if failure is None else GateError(f"{failure}; {error}")
    if _porcelain(root) != caller_state or _lock_bytes(root) != caller_lock:
        raise GateError("verification changed the caller checkout")
    if failure is not None:
        raise failure
