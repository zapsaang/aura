from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

from .manifest import verify_manifest, write_manifest
from .model import (
    EvidenceError,
    canonical_json_bytes,
    fail,
    require_exact_keys,
    write_new,
)
from .registry import CommandSpec

RUST_SUMMARY = re.compile(
    rb"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; "
    rb"(\d+) measured; (\d+) filtered out(?:; finished in [^\r\n]+)?$",
    re.MULTILINE,
)
CAPTURE_TAIL_BYTES = 500
TUPLE_FILES = frozenset({
    "SHA256SUMS", "command.bin", "cwd.bin", "env.json", "exit-code", "shell.bin",
    "stderr.bin", "stdout.bin", "tests-executed",
})


@dataclass(frozen=True)
class TupleResult:
    tuple_sha256: str
    tests_executed: int


def write_tuple(
    path: Path,
    spec: CommandSpec,
    environment: dict[str, str],
    exit_code: int,
    stdout: bytes,
    stderr: bytes,
) -> str:
    if path.exists() or path.is_symlink():
        fail(f"tuple path must be fresh: {path}")
    tests_executed = _validate_result(spec, environment, exit_code, stdout, stderr)
    path.mkdir(parents=True, mode=0o700)
    values = {
        "command.bin": _one_line(spec.command, "command"),
        "cwd.bin": _one_line(spec.cwd, "cwd"),
        "env.json": canonical_json_bytes(environment),
        "exit-code": f"{exit_code}\n".encode("ascii"),
        "shell.bin": _one_line(spec.shell, "shell"),
        "stderr.bin": stderr,
        "stdout.bin": stdout,
        "tests-executed": f"{tests_executed}\n".encode("ascii"),
    }
    for name, raw in values.items():
        write_new(path / name, raw)
    return write_manifest(path)


def verify_tuple(path: Path, spec: CommandSpec, environment: dict[str, str]) -> TupleResult:
    actual_files = frozenset(child.name for child in path.iterdir())
    if actual_files != TUPLE_FILES or any(not child.is_file() or child.is_symlink() for child in path.iterdir()):
        fail(f"tuple has missing, unknown, or non-regular files: {path}")
    digest = verify_manifest(path)
    if (path / "command.bin").read_bytes() != _one_line(spec.command, "command"):
        fail(f"tuple command drift: {path}")
    if (path / "shell.bin").read_bytes() != _one_line(spec.shell, "shell"):
        fail(f"tuple shell drift: {path}")
    if (path / "cwd.bin").read_bytes() != _one_line(spec.cwd, "cwd"):
        fail(f"tuple cwd drift: {path}")
    env_raw = (path / "env.json").read_bytes()
    if env_raw != canonical_json_bytes(environment):
        fail(f"tuple environment drift: {path}")
    exit_code = _canonical_decimal((path / "exit-code").read_bytes(), "exit-code")
    recorded_count = _canonical_decimal((path / "tests-executed").read_bytes(), "tests-executed")
    count = _validate_result(
        spec,
        environment,
        exit_code,
        (path / "stdout.bin").read_bytes(),
        (path / "stderr.bin").read_bytes(),
    )
    if recorded_count != count:
        fail(f"tuple test count drift: {path}")
    return TupleResult(digest, count)


def _validate_result(
    spec: CommandSpec,
    environment: dict[str, str],
    exit_code: int,
    stdout: bytes,
    stderr: bytes,
) -> int:
    try:
        _validate_environment(spec, environment)
        if exit_code != spec.expected_exit:
            fail(f"{spec.id} exit mismatch: expected {spec.expected_exit}, got {exit_code}")
        if spec.stderr == "empty" and stderr:
            fail(f"{spec.id} requires empty stderr")
        if spec.stderr not in {"empty", "tool"}:
            fail(f"{spec.id} has invalid stderr rule")
        _validate_stdout(spec, stdout)
        count = _count_tests(spec, stdout)
        if spec.tests.mode == "exact" and count != spec.tests.value:
            fail(f"{spec.id} requires exactly {spec.tests.value} tests, got {count}")
        if spec.tests.mode == "minimum" and count < spec.tests.value:
            fail(f"{spec.id} requires at least {spec.tests.value} tests, got {count}")
        return count
    except EvidenceError as error:
        stdout_tail = stdout[-CAPTURE_TAIL_BYTES:].decode("utf-8", errors="replace")
        stderr_tail = stderr[-CAPTURE_TAIL_BYTES:].decode("utf-8", errors="replace")
        raise EvidenceError(
            f"{error}; captured stdout tail: {stdout_tail!r}; captured stderr tail: {stderr_tail!r}"
        ) from error


def _validate_environment(spec: CommandSpec, environment: dict[str, str]) -> None:
    if sorted(environment) != sorted(spec.env):
        fail(f"{spec.id} environment keys differ from registry")
    for key, declared in spec.env.items():
        actual = environment[key]
        if not isinstance(actual, str) or "\x00" in actual or "\n" in actual:
            fail(f"{spec.id} environment value is invalid: {key}")
        if not declared.startswith("<") and actual != declared:
            fail(f"{spec.id} environment value drift: {key}")


def _validate_stdout(spec: CommandSpec, stdout: bytes) -> None:
    if spec.stdout.mode == "exact":
        if spec.stdout.value is None or stdout != spec.stdout.value.encode("utf-8"):
            fail(f"{spec.id} exact stdout mismatch")
    elif spec.stdout.mode == "nonempty":
        if not stdout:
            fail(f"{spec.id} requires nonempty stdout")
    elif spec.stdout.mode == "version-line":
        if not stdout.endswith(b"\n") or stdout.count(b"\n") != 1 or not stdout[:-1]:
            fail(f"{spec.id} requires exactly one version line")
        if any(byte < 32 or byte == 127 for byte in stdout[:-1]):
            fail(f"{spec.id} version line contains control bytes")
    elif spec.stdout.mode != "unrestricted":
        fail(f"{spec.id} has invalid stdout rule")


def _count_tests(spec: CommandSpec, stdout: bytes) -> int:
    if spec.count_source == "constant:0":
        return 0
    if spec.count_source == "stdout-json:checks":
        try:
            value = json.loads(stdout)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise EvidenceError(f"{spec.id} stdout is not JSON: {error}") from error
        if not isinstance(value, dict):
            fail(f"{spec.id} stdout JSON must be an object")
        require_exact_keys(value, frozenset({"checks", "status"}), f"{spec.id} stdout")
        checks = value["checks"]
        if not isinstance(checks, int) or isinstance(checks, bool) or checks < 0 or value["status"] != "ok":
            fail(f"{spec.id} stdout JSON values are invalid")
        if canonical_json_bytes(value) != stdout:
            fail(f"{spec.id} stdout JSON is not canonical")
        return checks
    if spec.count_source not in {"rust-harness", "rust-harness-sum"}:
        fail(f"{spec.id} has invalid count_source")
    return _rust_count(spec.id, stdout, spec.count_source == "rust-harness-sum")


def _rust_count(identifier: str, stdout: bytes, summed: bool) -> int:
    candidate_lines = [line for line in stdout.splitlines() if line.startswith(b"test result:")]
    matches = list(RUST_SUMMARY.finditer(stdout))
    if not matches or len(matches) != len(candidate_lines):
        fail(f"{identifier} has missing or malformed Rust test summary")
    if not summed and len(matches) != 1:
        fail(f"{identifier} requires exactly one Rust test summary")
    total = 0
    for match in matches:
        status = match.group(1)
        counts = tuple(int(match.group(index)) for index in range(2, 7))
        if status != b"ok" or counts[1] != 0:
            fail(f"{identifier} Rust tests did not all pass")
        total += counts[0]
        if total > 2**64 - 1:
            fail(f"{identifier} Rust test count overflow")
    return total


def _one_line(value: str, label: str) -> bytes:
    if not value or "\n" in value or "\x00" in value:
        fail(f"{label} must be one nonempty UTF-8 line")
    return value.encode("utf-8") + b"\n"


def _canonical_decimal(raw: bytes, label: str) -> int:
    if re.fullmatch(rb"(?:0|[1-9][0-9]*)\n", raw) is None:
        fail(f"{label} is not canonical unsigned decimal")
    return int(raw[:-1])
