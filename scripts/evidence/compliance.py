from __future__ import annotations

from pathlib import Path

from .aggregate import verify_aggregate_tree
from .contract import (
    F1Check,
    F1Contract,
    load_f1_contract,
    validate_f1_contract,
    verify_f1_contract_matches_sources,
)
from .manifest import verify_manifest
from .model import decode_json_bytes, fail, require_exact_keys, require_hex, write_new
from .producer import source_provenance, verify_producer_tree
from .registry import CommandSpec, Registry, load_registry, verify_registry_matches_plan
from .tuple import verify_tuple


def verify_compliance(
    aggregate: Path,
    contract_path: Path,
    plan: Path,
    plan_sha256: str,
    matrix: Path,
    report: Path,
) -> int:
    registry = load_registry(Path("qa/compliance-qa-registry.json"))
    verify_registry_matches_plan(registry, plan)
    receipt = verify_aggregate_tree(aggregate, plan, plan_sha256)
    expected_matrix = aggregate / "source" / "compliance-matrix.json"
    if matrix.resolve(strict=True) != expected_matrix.resolve(strict=True):
        fail("matrix must be the manifest-covered aggregate source matrix")
    contract = load_f1_contract(contract_path)
    validate_f1_contract(contract, registry)
    verify_f1_contract_matches_sources(contract, registry, matrix)
    verified_rows = _verify_registry_evidence(aggregate, registry, receipt, plan)
    manifest_paths = _aggregate_manifest_paths(aggregate)
    for check in contract.checks:
        _verify_check(aggregate, check, verified_rows, manifest_paths)
    _write_report(report, contract, receipt)
    return len(contract.checks)


def _verify_registry_evidence(
    aggregate: Path, registry: Registry, aggregate_receipt: dict[str, object], plan: Path
) -> dict[str, str]:
    verified: dict[str, str] = {}
    verified_commit = require_hex(aggregate_receipt["verified_commit"], 40, "verified_commit")
    for row in registry.rows:
        if row.task_owner == "final":
            continue
        tuple_path = _tuple_path(aggregate, row)
        env_value = decode_json_bytes((tuple_path / "env.json").read_bytes(), f"{row.id} env.json")
        if not isinstance(env_value, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in env_value.items()):
            fail(f"{row.id} tuple environment is invalid")
        environment = {key: item for key, item in env_value.items() if isinstance(item, str)}
        result = verify_tuple(tuple_path, row, environment)
        verified[row.id] = result.tuple_sha256
    _verify_producers(aggregate, registry, aggregate_receipt, verified_commit, plan)
    for task in range(1, 18):
        root = aggregate / "tasks" / f"task-{task}"
        receipt = decode_json_bytes((root / "receipt.json").read_bytes(), f"task-{task} receipt")
        if not isinstance(receipt, dict):
            fail(f"task-{task} receipt must be an object")
        require_exact_keys(
            receipt,
            frozenset({
                "commands", "manifest_sha256", "schema_version", "status", "task", "task_commit",
                "verified_commit",
            }),
            f"task-{task} receipt",
        )
        if receipt["task"] != str(task) or receipt["status"] != "approved" or receipt["schema_version"] != 1:
            fail(f"task-{task} receipt identity differs")
        if require_hex(receipt["task_commit"], 40, "task_commit") == "0" * 40:
            fail(f"task-{task} commit is invalid")
        if receipt["verified_commit"] != verified_commit:
            fail(f"task-{task} verified commit differs")
        expected_commands = sorted(
            [
                {"id": row.id, "lane": row.execution_context, "tuple_sha256": verified[row.id]}
                for row in registry.rows
                if row.task_owner == str(task)
            ],
            key=lambda value: str(value["id"]),
        )
        if receipt["commands"] != expected_commands:
            fail(f"task-{task} command closure differs")
        if receipt.get("manifest_sha256") != verify_manifest(root, frozenset({"SHA256SUMS", "receipt.json"})):
            fail(f"task-{task} manifest digest differs")
    merge = aggregate / "merges" / "M1516"
    receipt = decode_json_bytes((merge / "receipt.json").read_bytes(), "M1516 receipt")
    if not isinstance(receipt, dict):
        fail("M1516 receipt must be an object")
    require_exact_keys(
        receipt,
        frozenset({
            "commands", "manifest_sha256", "merge", "merge_commit", "schema_version", "status",
            "verified_commit",
        }),
        "M1516 receipt",
    )
    if receipt["merge"] != "M1516" or receipt["status"] != "approved" or receipt["schema_version"] != 1:
        fail("M1516 receipt identity differs")
    require_hex(receipt["merge_commit"], 40, "merge_commit")
    if receipt["verified_commit"] != verified_commit:
        fail("M1516 verified commit differs")
    merge_rows = [row for row in registry.rows if row.task_owner == "merge"]
    expected_merge_commands = [
        {"id": row.id, "lane": row.execution_context, "tuple_sha256": verified[row.id]}
        for row in merge_rows
    ]
    if receipt["commands"] != expected_merge_commands:
        fail("M1516 command closure differs")
    if receipt.get("manifest_sha256") != verify_manifest(merge, frozenset({"SHA256SUMS", "receipt.json"})):
        fail("M1516 manifest digest differs")
    return verified


def _verify_producers(
    aggregate: Path,
    registry: Registry,
    receipt: dict[str, object],
    verified_commit: str,
    plan: Path,
) -> None:
    run_id = str(receipt["run_id"])
    run_attempt = receipt["run_attempt"]
    if not isinstance(run_attempt, int) or isinstance(run_attempt, bool):
        fail("aggregate run_attempt is invalid")
    for producer, key in (("preflight", "imported_baseline_commit"), ("prerequisite", "prerequisite_commit")):
        root = aggregate / producer
        value = decode_json_bytes((root / "receipt.json").read_bytes(), f"{producer} receipt")
        if not isinstance(value, dict):
            fail(f"{producer} receipt must be an object")
        commit = require_hex(value.get(key), 40, key)
        verify_producer_tree(
            root,
            registry,
            producer,
            commit=commit,
            run_id=run_id if producer == "preflight" else None,
            run_attempt=run_attempt if producer == "preflight" else None,
        )
    verify_producer_tree(
        aggregate / "source",
        registry,
        "source",
        commit=verified_commit,
        run_id=run_id,
        run_attempt=run_attempt,
        provenance=source_provenance(plan),
    )


def _verify_check(
    aggregate: Path,
    check: F1Check,
    verified_rows: dict[str, str],
    manifest_paths: set[str],
) -> None:
    for identifier in check.registry_ids:
        if identifier not in verified_rows:
            fail(f"{check.id} registry binding is not verified: {identifier}")
    for pattern in check.aggregate_paths:
        matches = _path_matches(aggregate, pattern)
        if not matches:
            fail(f"{check.id} aggregate path does not resolve: {pattern}")
        for path in matches:
            if path.is_file() and path.relative_to(aggregate).as_posix() not in manifest_paths:
                fail(f"{check.id} path is not aggregate-manifest-covered: {path}")


def _path_matches(root: Path, pattern: str) -> list[Path]:
    if pattern.endswith("/**"):
        base = root / pattern[:-3]
        if not base.is_dir() or base.is_symlink():
            return []
        return [base, *sorted(base.rglob("*"), key=lambda path: path.as_posix())]
    path = root / pattern
    if not path.exists() or path.is_symlink():
        return []
    return [path]


def _aggregate_manifest_paths(root: Path) -> set[str]:
    paths: set[str] = set()
    for line in (root / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        digest, separator, path = line.partition("  ")
        if not separator or len(digest) != 64 or path in paths:
            fail("aggregate manifest is malformed or duplicated")
        paths.add(path)
    return paths


def _tuple_path(root: Path, row: CommandSpec) -> Path:
    if row.task_owner == "preflight":
        return root / "preflight" / "tuples" / row.id
    if row.task_owner == "prerequisite":
        return root / "prerequisite" / "tuples" / row.id
    if row.task_owner == "merge":
        return root / "merges" / "M1516" / "tuples" / row.execution_context / row.id
    if row.task_owner.isdecimal():
        return root / "tasks" / f"task-{int(row.task_owner)}" / "tuples" / row.execution_context / row.id
    fail(f"registry ID has no aggregate tuple: {row.id}")


def _write_report(path: Path, contract: F1Contract, receipt: dict[str, object]) -> None:
    lines = [
        "# F1 Compliance Evidence",
        "",
        f"- Verified commit: `{receipt['verified_commit']}`",
        f"- Plan SHA-256: `{receipt['plan_sha256']}`",
        f"- Aggregate SHA-256: `{receipt['aggregate_sha256']}`",
        f"- Checks: {len(contract.checks)}",
        "",
        "| Check | Registry bindings | Aggregate bindings |",
        "|---|---:|---:|",
    ]
    lines.extend(
        f"| {check.id} | {len(check.registry_ids)} | {len(check.aggregate_paths)} |"
        for check in contract.checks
    )
    write_new(path, ("\n".join(lines) + "\n").encode("utf-8"))
