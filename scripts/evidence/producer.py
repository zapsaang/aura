from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from .dag import GitRunner, git_runner
from .manifest import verify_manifest, write_manifest
from .model import (
    canonical_json_bytes,
    decode_json_bytes,
    fail,
    parse_json_bytes,
    require_exact_keys,
    require_hex,
    require_list,
    require_safe_id,
    require_uint,
    sha256_bytes,
    sha256_file,
    write_new,
)
from .operations import load_operations
from .registry import Registry
from .tuple import verify_tuple

SourceProvenance = Callable[[str], tuple[str, str]]


def source_provenance(plan: Path, git: GitRunner | None = None) -> SourceProvenance:
    runner = git_runner() if git is None else git
    plan_digest = sha256_file(plan)

    def provenance(commit: str) -> tuple[str, str]:
        tree = runner(["rev-parse", f"{commit}^{{tree}}"]).decode("utf-8").strip()
        return require_hex(tree, 40, "head_tree"), plan_digest

    return provenance


def seal_producer(
    root: Path,
    registry: Registry,
    producer: str,
    *,
    run_id: str | None,
    run_attempt: int | None,
    commit: str,
) -> dict[str, object]:
    if (root / "receipt.json").exists() or (root / "SHA256SUMS").exists():
        fail("producer is already sealed")
    if producer == "preflight":
        receipt = _preflight_receipt(root, registry, run_id, run_attempt, commit)
    elif producer == "prerequisite":
        receipt = _prerequisite_receipt(root, registry, commit)
    elif producer == "source":
        receipt = _source_receipt(root, run_id, run_attempt, commit)
    else:
        fail(f"unknown producer: {producer}")
    receipt["manifest_sha256"] = write_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
    validate_producer_receipt(receipt, producer, commit, run_id, run_attempt)
    write_new(root / "receipt.json", canonical_json_bytes(receipt))
    return receipt


def verify_producer_tree(
    root: Path,
    registry: Registry,
    producer: str,
    *,
    commit: str,
    run_id: str | None,
    run_attempt: int | None,
    provenance: SourceProvenance | None = None,
) -> dict[str, object]:
    value = decode_json_bytes((root / "receipt.json").read_bytes(), f"{producer} receipt")
    if not isinstance(value, dict):
        fail(f"{producer} receipt must be an object")
    validate_producer_receipt(value, producer, commit, run_id, run_attempt)
    digest = verify_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
    if value["manifest_sha256"] != digest:
        fail(f"{producer} manifest digest drift")
    if producer == "preflight":
        expected = _preflight_commands(root, registry)
        if value["commands"] != expected:
            fail("preflight command closure drift")
        _validate_source_root(root / "source-root.json", commit)
        load_operations(root / "operations.log")
    elif producer == "prerequisite":
        expected = _prerequisite_commands(root, registry)
        if value["commands"] != expected:
            fail("prerequisite command closure drift")
    else:
        digest = _validate_source_artifacts(
            root / "source-artifacts.json", root / "compliance-matrix.json", commit, provenance
        )
        if value["source_artifacts_sha256"] != digest:
            fail("source artifacts digest drift")
        load_operations(root / "operations.log")
    return value


def validate_producer_receipt(
    receipt: dict[str, object],
    producer: str,
    commit: str,
    run_id: str | None,
    run_attempt: int | None,
) -> None:
    if producer == "preflight":
        keys = frozenset({
            "commands", "imported_baseline_commit", "manifest_sha256", "producer", "run_attempt",
            "run_id", "schema_version", "status",
        })
        require_exact_keys(receipt, keys, "preflight receipt")
        if receipt["producer"] != "preflight":
            fail("preflight producer drift")
        _run_identity(receipt, run_id, run_attempt, "preflight")
        if require_hex(receipt["imported_baseline_commit"], 40, "imported_baseline_commit") != commit:
            fail("preflight commit drift")
        _validate_producer_commands(receipt["commands"], ["pre-01"], "preflight-ubuntu")
    elif producer == "prerequisite":
        keys = frozenset({
            "commands", "manifest_sha256", "prerequisite", "prerequisite_commit", "schema_version", "status",
        })
        require_exact_keys(receipt, keys, "prerequisite receipt")
        if receipt["prerequisite"] != "rust170-ploc":
            fail("prerequisite identity drift")
        if require_hex(receipt["prerequisite_commit"], 40, "prerequisite_commit") != commit:
            fail("prerequisite commit drift")
        _validate_producer_commands(receipt["commands"], ["tip-prerequisite"], "prerequisite-ubuntu")
    elif producer == "source":
        keys = frozenset({
            "manifest_sha256", "producer", "run_attempt", "run_id", "schema_version", "source_artifacts_sha256",
            "status", "verified_commit",
        })
        require_exact_keys(receipt, keys, "source receipt")
        if receipt["producer"] != "source":
            fail("source producer drift")
        _run_identity(receipt, run_id, run_attempt, "source")
        if require_hex(receipt["verified_commit"], 40, "verified_commit") != commit:
            fail("source commit drift")
        require_hex(receipt["source_artifacts_sha256"], 64, "source_artifacts_sha256")
    else:
        fail(f"unknown producer: {producer}")
    if receipt["schema_version"] != 1 or receipt["status"] != "approved":
        fail(f"{producer} receipt is not schema-1 approved")
    require_hex(receipt["manifest_sha256"], 64, "manifest_sha256")


def _preflight_receipt(
    root: Path, registry: Registry, run_id: str | None, run_attempt: int | None, commit: str
) -> dict[str, object]:
    _validate_source_root(root / "source-root.json", commit)
    load_operations(root / "operations.log")
    return {
        "commands": _preflight_commands(root, registry),
        "imported_baseline_commit": commit,
        "manifest_sha256": "0" * 64,
        "producer": "preflight",
        "run_attempt": _require_attempt(run_attempt),
        "run_id": _require_run_id(run_id),
        "schema_version": 1,
        "status": "approved",
    }


def _prerequisite_receipt(root: Path, registry: Registry, commit: str) -> dict[str, object]:
    return {
        "commands": _prerequisite_commands(root, registry),
        "manifest_sha256": "0" * 64,
        "prerequisite": "rust170-ploc",
        "prerequisite_commit": commit,
        "schema_version": 1,
        "status": "approved",
    }


def _source_receipt(root: Path, run_id: str | None, run_attempt: int | None, commit: str) -> dict[str, object]:
    artifacts_digest = _validate_source_artifacts(root / "source-artifacts.json", root / "compliance-matrix.json", commit)
    load_operations(root / "operations.log")
    return {
        "manifest_sha256": "0" * 64,
        "producer": "source",
        "run_attempt": _require_attempt(run_attempt),
        "run_id": _require_run_id(run_id),
        "schema_version": 1,
        "source_artifacts_sha256": artifacts_digest,
        "status": "approved",
        "verified_commit": commit,
    }


def _preflight_commands(root: Path, registry: Registry) -> list[dict[str, object]]:
    return [_producer_tuple(root, registry, "pre-01", "preflight-ubuntu")]


def _prerequisite_commands(root: Path, registry: Registry) -> list[dict[str, object]]:
    return [_producer_tuple(root, registry, "tip-prerequisite", "prerequisite-ubuntu")]


def _producer_tuple(root: Path, registry: Registry, identifier: str, context: str) -> dict[str, object]:
    row = registry.by_id()[identifier]
    path = root / "tuples" / identifier
    env_value = decode_json_bytes((path / "env.json").read_bytes(), f"{path}/env.json")
    if not isinstance(env_value, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in env_value.items()):
        fail(f"invalid tuple environment: {path}")
    result = verify_tuple(path, row, {key: item for key, item in env_value.items() if isinstance(item, str)})
    return {"execution_context": context, "id": identifier, "tuple_sha256": result.tuple_sha256}


def _validate_producer_commands(value: object, identifiers: list[str], context: str) -> None:
    commands = require_list(value, "producer commands")
    if len(commands) != len(identifiers):
        fail("producer command closure differs")
    for index, command in enumerate(commands):
        if not isinstance(command, dict):
            fail("producer command must be an object")
        require_exact_keys(command, frozenset({"execution_context", "id", "tuple_sha256"}), "producer command")
        if command["id"] != identifiers[index] or command["execution_context"] != context:
            fail("producer command identity differs")
        require_hex(command["tuple_sha256"], 64, "tuple_sha256")


def _validate_source_root(path: Path, commit: str) -> None:
    keys = frozenset({
        "device", "head_tree", "imported_baseline_commit", "inode", "schema_version", "source_root",
    })
    value = parse_json_bytes(path.read_bytes(), keys, "source-root.json")
    if value["schema_version"] != 1:
        fail("unsupported source-root schema")
    if require_hex(value["imported_baseline_commit"], 40, "imported_baseline_commit") != commit:
        fail("source-root baseline commit drift")
    require_hex(value["head_tree"], 40, "head_tree")
    require_uint(value["device"], "device")
    require_uint(value["inode"], "inode")
    source_root = value["source_root"]
    if not isinstance(source_root, str) or not Path(source_root).is_absolute():
        fail("source_root must be absolute")


def _validate_source_artifacts(
    path: Path, matrix: Path, commit: str, provenance: SourceProvenance | None = None
) -> str:
    keys = frozenset({
        "compliance_matrix_sha256", "head_tree", "plan_sha256", "porcelain", "schema_version",
        "verified_commit",
    })
    raw = path.read_bytes()
    value = parse_json_bytes(raw, keys, "source-artifacts.json")
    if value["schema_version"] != 1 or value["porcelain"] != "":
        fail("source artifacts schema or porcelain differs")
    if require_hex(value["verified_commit"], 40, "verified_commit") != commit:
        fail("source artifacts commit drift")
    head_tree = require_hex(value["head_tree"], 40, "head_tree")
    plan_digest = require_hex(value["plan_sha256"], 64, "plan_sha256")
    if provenance is not None:
        actual_tree, actual_plan = provenance(commit)
        if head_tree != actual_tree:
            fail("source head_tree differs from the verified commit tree")
        if plan_digest != actual_plan:
            fail("source plan_sha256 differs from the operational plan digest")
    if require_hex(value["compliance_matrix_sha256"], 64, "compliance_matrix_sha256") != sha256_file(matrix):
        fail("source compliance matrix digest drift")
    return sha256_bytes(raw)


def _run_identity(receipt: dict[str, object], run_id: str | None, run_attempt: int | None, label: str) -> None:
    actual_run_id = require_safe_id(receipt["run_id"], f"{label} run_id")
    actual_attempt = require_uint(receipt["run_attempt"], f"{label} run_attempt", positive=True)
    if run_id is not None and actual_run_id != run_id:
        fail(f"{label} run_id drift")
    if run_attempt is not None and actual_attempt != run_attempt:
        fail(f"{label} run_attempt drift")


def _require_run_id(value: str | None) -> str:
    if value is None:
        fail("producer requires run_id")
    return require_safe_id(value, "run_id")


def _require_attempt(value: int | None) -> int:
    if value is None:
        fail("producer requires run_attempt")
    return require_uint(value, "run_attempt", positive=True)
