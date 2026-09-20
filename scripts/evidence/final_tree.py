from __future__ import annotations

from pathlib import Path

from .manifest import verify_manifest
from .model import decode_json_bytes, fail, require_safe_id, require_uint, sha256_file
from .receipt import GATE_COMMANDS, FinalIdentity, HandoffIdentity, validate_handoff_receipt
from .tuple import TUPLE_FILES

HANDOFF_TOP = frozenset({"SHA256SUMS", "artifacts", "receipt.json", "tuple"})


def validate_final_layout(inputs: Path) -> None:
    required = (
        inputs / "aggregate" / "SHA256SUMS",
        inputs / "aggregate" / "receipt.json",
        inputs / "operations" / "evidence-operations.log",
    )
    for path in required:
        if not path.is_file() or path.is_symlink():
            fail(f"missing final input: {path}")
    for gate in GATE_COMMANDS:
        root = inputs / "gates" / gate
        if not (root / "report.md").is_file() or not (root / "receipt.json").is_file() or not (root / "tuples").is_dir():
            fail(f"incomplete final gate layout: {gate}")
    if not (inputs / "gates" / "F4" / "scope.json").is_file():
        fail("missing F4 scope input")
    for platform in ("linux", "macos"):
        if not (inputs / "gates" / "F3" / "native" / platform / "receipt.json").is_file():
            fail(f"missing F3 native handoff: {platform}")
    for path in inputs.rglob("*"):
        if path.is_symlink():
            fail(f"final input is a symlink: {path}")
        if path.is_dir():
            continue
        if not path.is_file():
            fail(f"final input is not a regular file: {path}")
        relative = path.relative_to(inputs).as_posix()
        if not _legal_final_input(relative):
            fail(f"unexpected final input: {relative}")


def verify_gate_tree(root: Path, gate: str, receipt: dict[str, object]) -> None:
    commands = receipt["commands"]
    if not isinstance(commands, list):
        fail(f"{gate} receipt commands must be an array")
    identifiers = [entry["id"] for entry in commands if isinstance(entry, dict)]
    _require_top_closure(root, gate)
    tuples = root / "tuples"
    actual = sorted(child.name for child in tuples.iterdir())
    if actual != identifiers or any(not (tuples / name).is_dir() or (tuples / name).is_symlink() for name in actual):
        fail(f"{gate} tuple directory closure differs")
    for entry in commands:
        if isinstance(entry, dict):
            _verify_tuple_dir(tuples / str(entry["id"]), entry["tuple_sha256"])


def verify_native_handoffs(root: Path, identity: FinalIdentity, receipt: dict[str, object]) -> None:
    handoffs = receipt["handoffs"]
    if not isinstance(handoffs, list):
        fail("F3 receipt handoffs must be an array")
    reference: tuple[str, int] | None = None
    for entry in handoffs:
        if not isinstance(entry, dict):
            fail("F3 handoff entry must be an object")
        platform = str(entry["platform"])
        handoff_root = root / "native" / platform
        payload = decode_json_bytes((handoff_root / "receipt.json").read_bytes(), f"F3 {platform} handoff receipt")
        if not isinstance(payload, dict):
            fail(f"F3 {platform} handoff receipt must be an object")
        run_id = require_safe_id(payload.get("run_id"), "run_id")
        run_attempt = require_uint(payload.get("run_attempt"), "run_attempt", positive=True)
        if reference is None:
            reference = (run_id, run_attempt)
        elif reference != (run_id, run_attempt):
            fail("native handoff run identity differs across platforms")
        validate_handoff_receipt(payload, HandoffIdentity(
            platform, run_id, run_attempt, identity.tag, identity.verified_commit, identity.aggregate_sha256,
        ))
        if sha256_file(handoff_root / "receipt.json") != entry["receipt_sha256"]:
            fail(f"F3 {platform} handoff receipt digest drift")
        if verify_manifest(handoff_root, frozenset({"SHA256SUMS", "receipt.json"})) != payload["manifest_sha256"]:
            fail(f"F3 {platform} handoff manifest digest drift")
        if verify_manifest(handoff_root / "artifacts") != payload["artifacts_manifest_sha256"]:
            fail(f"F3 {platform} artifacts manifest digest drift")
        _verify_tuple_dir(handoff_root / "tuple", payload["tuple_sha256"])
        _require_entries(handoff_root, HANDOFF_TOP, f"F3 {platform} handoff")


def _verify_tuple_dir(path: Path, digest: object) -> None:
    _require_entries(path, TUPLE_FILES, f"tuple {path.name}", files_only=True)
    if not isinstance(digest, str) or verify_manifest(path) != digest:
        fail(f"tuple manifest digest drift: {path.name}")


def _require_top_closure(root: Path, gate: str) -> None:
    expected = frozenset({"receipt.json", "report.md", "tuples"} | ({"scope.json"} if gate == "F4" else set())
                         | ({"native"} if gate == "F3" else set()))
    _require_entries(root, expected, f"{gate} gate input")


def _require_entries(root: Path, expected: frozenset[str], label: str, *, files_only: bool = False) -> None:
    actual = frozenset(child.name for child in root.iterdir())
    if actual != expected:
        fail(f"{label} closure differs: unexpected={sorted(actual - expected)}, missing={sorted(expected - actual)}")
    for child in root.iterdir():
        if child.is_symlink() or not (child.is_file() or (child.is_dir() and not files_only)):
            fail(f"{label} entry is not a regular file{' or directory' if not files_only else ''}: {child.name}")


def _legal_final_input(relative: str) -> bool:
    if relative in {"aggregate/SHA256SUMS", "aggregate/receipt.json", "operations/evidence-operations.log"}:
        return True
    parts = relative.split("/")
    if len(parts) < 3 or parts[0] != "gates" or parts[1] not in GATE_COMMANDS:
        return False
    gate, rest = parts[1], parts[2:]
    if rest in (["report.md"], ["receipt.json"]):
        return True
    if gate == "F4" and rest == ["scope.json"]:
        return True
    if len(rest) == 3 and rest[0] == "tuples" and rest[1] in GATE_COMMANDS[gate] and rest[2] in TUPLE_FILES:
        return True
    if gate == "F3" and len(rest) >= 2 and rest[0] == "native" and rest[1] in {"linux", "macos"}:
        tail = rest[2:]
        if tail in (["receipt.json"], ["SHA256SUMS"]):
            return True
        if len(tail) == 2 and tail[0] == "tuple" and tail[1] in TUPLE_FILES:
            return True
        return len(tail) >= 2 and tail[0] == "artifacts"
    return False
