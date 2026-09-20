from __future__ import annotations

from pathlib import Path

from .final_tree import validate_final_layout, verify_gate_tree, verify_native_handoffs
from .manifest import verify_manifest, write_manifest
from .model import canonical_json_bytes, decode_json_bytes, fail, sha256_file, write_new
from .receipt import (
    FinalIdentity,
    GateIdentity,
    HandoffIdentity,
    validate_final_receipt,
    validate_gate_receipt,
    validate_handoff_receipt,
)
from .registry import Registry
from .tuple import verify_tuple


def seal_gate(
    root: Path,
    registry: Registry,
    identity: GateIdentity,
    evidence_paths: list[Path],
    handoff_paths: list[tuple[str, Path]],
) -> dict[str, object]:
    if (root / "receipt.json").exists():
        fail(f"gate is already sealed: {identity.gate}")
    rows = [row for row in registry.rows if row.task_owner == "final" and row.id.startswith(f"{identity.gate}-")]
    actual = sorted(path.name for path in (root / "tuples").iterdir() if path.is_dir())
    expected = sorted(row.id for row in rows)
    if actual != expected:
        fail(f"{identity.gate} tuple closure differs: expected={expected}, actual={actual}")
    commands: list[dict[str, object]] = []
    for row in rows:
        path = root / "tuples" / row.id
        value = decode_json_bytes((path / "env.json").read_bytes(), f"{row.id} env.json")
        if not isinstance(value, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in value.items()):
            fail(f"invalid gate tuple environment: {row.id}")
        result = verify_tuple(path, row, {key: item for key, item in value.items() if isinstance(item, str)})
        commands.append({
            "exit_code": 0,
            "id": row.id,
            "tests_executed": result.tests_executed,
            "tuple_sha256": result.tuple_sha256,
        })
    evidence = _digest_paths(evidence_paths, root)
    handoffs: list[dict[str, object]] = []
    for platform, path in sorted(handoff_paths):
        handoffs.append({"platform": platform, "receipt_sha256": sha256_file(path)})
    receipt: dict[str, object] = {
        "aggregate_sha256": identity.aggregate_sha256,
        "commands": commands,
        "evidence": evidence,
        "gate": identity.gate,
        "handoffs": handoffs,
        "plan_sha256": identity.plan_sha256,
        "schema_version": 1,
        "status": "approved",
        "tag": identity.tag,
        "verified_commit": identity.verified_commit,
    }
    validate_gate_receipt(receipt, identity)
    write_new(root / "receipt.json", canonical_json_bytes(receipt))
    return receipt


def finalize(root: Path, identity: FinalIdentity) -> dict[str, object]:
    if (root / "receipt.json").exists() or (root / "SHA256SUMS").exists():
        fail("final evidence is already sealed")
    verdicts: dict[str, str] = {}
    for gate in ("F1", "F2", "F3", "F4"):
        gate_root = root / "inputs" / "gates" / gate
        path = gate_root / "receipt.json"
        value = decode_json_bytes(path.read_bytes(), f"{gate} receipt")
        if not isinstance(value, dict):
            fail(f"{gate} receipt must be an object")
        validate_gate_receipt(
            value,
            GateIdentity(gate, identity.tag, identity.verified_commit, identity.plan_sha256, identity.aggregate_sha256),
        )
        verify_gate_tree(gate_root, gate, value)
        if gate == "F3":
            verify_native_handoffs(gate_root, identity, value)
        verdicts[gate] = sha256_file(path)
    validate_final_layout(root / "inputs")
    manifest = write_manifest(
        root,
        frozenset({"SHA256SUMS", "receipt.json", "publication-receipt.json", "upload-receipt.json"}),
    )
    receipt: dict[str, object] = {
        "aggregate_sha256": identity.aggregate_sha256,
        "manifest_sha256": manifest,
        "plan_sha256": identity.plan_sha256,
        "schema_version": 1,
        "status": "approved",
        "tag": identity.tag,
        "verdict_receipts": verdicts,
        "verified_commit": identity.verified_commit,
    }
    validate_final_receipt(receipt, identity)
    write_new(root / "receipt.json", canonical_json_bytes(receipt))
    return receipt


def verify_final_tree(root: Path, identity: FinalIdentity) -> dict[str, object]:
    value = decode_json_bytes((root / "receipt.json").read_bytes(), "final receipt")
    if not isinstance(value, dict):
        fail("final receipt must be an object")
    validate_final_receipt(value, identity)
    manifest = verify_manifest(
        root,
        frozenset({"SHA256SUMS", "receipt.json", "publication-receipt.json", "upload-receipt.json"}),
    )
    if value["manifest_sha256"] != manifest:
        fail("final manifest digest drift")
    validate_final_layout(root / "inputs")
    for gate, digest in value["verdict_receipts"].items():
        if sha256_file(root / "inputs" / "gates" / gate / "receipt.json") != digest:
            fail(f"final verdict receipt digest drift: {gate}")
    return value


def seal_native_handoff(
    root: Path,
    registry: Registry,
    identity: HandoffIdentity,
    archive_path: str,
    archive_sha256: str,
    archive_file: Path,
) -> dict[str, object]:
    if not archive_file.is_file() or archive_file.is_symlink():
        fail(f"handoff archive is not a regular file: {archive_file}")
    if sha256_file(archive_file) != archive_sha256:
        fail("handoff archive digest differs from the archived bytes")
    row = registry.by_id()[f"F3-02-{identity.platform}"]
    tuple_path = root / "tuple"
    env = decode_json_bytes((tuple_path / "env.json").read_bytes(), "native tuple env.json")
    if not isinstance(env, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in env.items()):
        fail("native tuple environment is invalid")
    tuple_result = verify_tuple(tuple_path, row, {key: item for key, item in env.items() if isinstance(item, str)})
    artifacts = root / "artifacts"
    artifacts_manifest = write_manifest(artifacts)
    top_manifest = write_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
    receipt: dict[str, object] = {
        "aggregate_sha256": identity.aggregate_sha256,
        "archive_path": archive_path,
        "archive_sha256": archive_sha256,
        "artifacts_manifest_sha256": artifacts_manifest,
        "manifest_sha256": top_manifest,
        "platform": identity.platform,
        "run_attempt": identity.run_attempt,
        "run_id": identity.run_id,
        "schema_version": 1,
        "status": "approved",
        "tag": identity.tag,
        "tuple_sha256": tuple_result.tuple_sha256,
        "verified_commit": identity.verified_commit,
    }
    validate_handoff_receipt(receipt, identity)
    write_new(root / "receipt.json", canonical_json_bytes(receipt))
    return receipt


def _digest_paths(paths: list[Path], root: Path) -> list[dict[str, object]]:
    result: list[dict[str, object]] = []
    for path in sorted(paths, key=lambda value: value.as_posix()):
        if not path.is_file() or path.is_symlink():
            fail(f"gate evidence is not a regular file: {path}")
        try:
            relative = path.relative_to(root).as_posix()
        except ValueError:
            fail(f"gate evidence is outside its root: {path}")
        result.append({"path": relative, "sha256": sha256_file(path)})
    return result
