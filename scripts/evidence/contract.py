from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from .model import (
    canonical_json_bytes,
    decode_json_bytes,
    fail,
    require_exact_keys,
    require_list,
    require_sorted_unique,
    require_string,
)
from .registry import CommandSpec, Registry


@dataclass(frozen=True)
class F1Check:
    id: str
    registry_ids: tuple[str, ...]
    aggregate_paths: tuple[str, ...]

    def as_json(self) -> dict[str, object]:
        return {
            "aggregate_paths": list(self.aggregate_paths),
            "id": self.id,
            "registry_ids": list(self.registry_ids),
        }


@dataclass(frozen=True)
class F1Contract:
    checks: tuple[F1Check, ...]

    def as_json(self) -> dict[str, object]:
        return {"checks": [check.as_json() for check in self.checks], "schema_version": 1}


def load_f1_contract(path: Path) -> F1Contract:
    root = decode_json_bytes(path.read_bytes(), "F1 contract")
    if not isinstance(root, dict):
        fail("F1 contract must be an object")
    require_exact_keys(root, frozenset({"checks", "schema_version"}), "F1 contract")
    if root["schema_version"] != 1:
        fail("unsupported F1 contract schema")
    checks: list[F1Check] = []
    for index, value in enumerate(require_list(root["checks"], "checks")):
        label = f"checks[{index}]"
        if not isinstance(value, dict):
            fail(f"{label} must be an object")
        require_exact_keys(value, frozenset({"aggregate_paths", "id", "registry_ids"}), label)
        checks.append(F1Check(
            require_string(value["id"], f"{label}.id"),
            tuple(_string_array(value["registry_ids"], f"{label}.registry_ids")),
            tuple(_string_array(value["aggregate_paths"], f"{label}.aggregate_paths")),
        ))
    return F1Contract(tuple(checks))


def validate_f1_contract(contract: F1Contract, registry: Registry) -> None:
    expected_ids = sorted(
        [f"T{number:02d}" for number in range(1, 18)]
        + [f"MN{number:02d}" for number in range(1, 8)]
        + [f"AUD-{number:03d}" for number in range(1, 16)]
    )
    identifiers = [check.id for check in contract.checks]
    if identifiers != expected_ids:
        fail("F1 contract must contain the exact sorted 39-check partition")
    rows = registry.by_id()
    for check in contract.checks:
        registry_ids = list(check.registry_ids)
        paths = list(check.aggregate_paths)
        require_sorted_unique(registry_ids, f"{check.id}.registry_ids")
        require_sorted_unique(paths, f"{check.id}.aggregate_paths")
        if not registry_ids or not paths:
            fail(f"{check.id} mappings must be nonempty")
        for identifier in registry_ids:
            if identifier not in rows:
                fail(f"{check.id} references unknown registry ID: {identifier}")
            if rows[identifier].task_owner == "final":
                fail(f"{check.id} references final-owned ID: {identifier}")
        for value in paths:
            path = PurePosixPath(value)
            if path.is_absolute() or not path.parts or ".." in path.parts:
                fail(f"{check.id} path escapes aggregate: {value}")


def build_f1_contract(registry: Registry, matrix_path: Path) -> F1Contract:
    checks: list[F1Check] = []
    rows = registry.by_id()
    for task in range(1, 18):
        owned = sorted(row.id for row in registry.rows if row.task_owner == str(task))
        paths = [f"tasks/task-{task}/SHA256SUMS", f"tasks/task-{task}/receipt.json"]
        paths.extend(_tuple_path(rows[identifier]) for identifier in owned)
        checks.append(_check(f"T{task:02d}", owned, paths))
    checks.extend(_must_not_have_checks(rows))
    checks.extend(_audit_checks(rows, matrix_path))
    checks.sort(key=lambda check: check.id)
    contract = F1Contract(tuple(checks))
    validate_f1_contract(contract, registry)
    return contract


def verify_f1_contract_matches_sources(contract: F1Contract, registry: Registry, matrix_path: Path) -> None:
    expected = build_f1_contract(registry, matrix_path)
    if canonical_json_bytes(contract.as_json()) != canonical_json_bytes(expected.as_json()):
        fail("F1 contract differs from registry or compliance matrix")


def _must_not_have_checks(rows: dict[str, CommandSpec]) -> list[F1Check]:
    mn01_ids = ["pre-01", "tip-prerequisite", "tip-m1516"] + [f"tip-t{task:02d}" for task in range(1, 18)]
    mn01_paths = [
        "merges/M1516/SHA256SUMS", "merges/M1516/receipt.json", "merges/M1516/tuples/**",
        "preflight/SHA256SUMS", "preflight/receipt.json", "preflight/tuples/pre-01/**",
        "prerequisite/SHA256SUMS", "prerequisite/receipt.json", "prerequisite/tuples/tip-prerequisite/**",
        "source/SHA256SUMS", "source/compliance-matrix.json", "source/receipt.json", "source/source-artifacts.json",
    ]
    for task in range(1, 18):
        mn01_paths.extend((
            f"tasks/task-{task}/SHA256SUMS",
            f"tasks/task-{task}/receipt.json",
            f"tasks/task-{task}/tuples/**",
        ))
    groups = (
        ("MN01", mn01_ids, mn01_paths),
        ("MN02", ["t01-06"], []),
        ("MN03", [f"t04-{number:02d}" for number in range(1, 5)], []),
        ("MN04", ["t06-01", "t06-02", "t08-01", "t12-02", "t12-04", "t12-05"], []),
        ("MN05", ["t05-02", "t17-02"], []),
        ("MN06", ["t07-01", "t13-01", "t13-02"], []),
        ("MN07", [f"t01-{number:02d}" for number in range(1, 12)] + ["t06-02", "t12-02"], []),
    )
    result: list[F1Check] = []
    for identifier, registry_ids, paths in groups:
        mapped = paths or [_tuple_path(rows[row_id]) for row_id in registry_ids]
        result.append(_check(identifier, registry_ids, mapped))
    return result


def _audit_checks(rows: dict[str, CommandSpec], matrix_path: Path) -> list[F1Check]:
    matrix = decode_json_bytes(matrix_path.read_bytes(), "compliance matrix")
    if not isinstance(matrix, dict):
        fail("compliance matrix must be an object")
    require_exact_keys(matrix, frozenset({"audits", "matrix_version"}), "compliance matrix")
    if matrix["matrix_version"] != 1:
        fail("unsupported compliance matrix schema")
    result: list[F1Check] = []
    for index, value in enumerate(require_list(matrix["audits"], "audits")):
        label = f"audits[{index}]"
        if not isinstance(value, dict):
            fail(f"{label} must be an object")
        require_exact_keys(value, frozenset({"assertions", "aud", "evidence", "todos", "verdict"}), label)
        audit_id = require_string(value["aud"], f"{label}.aud")
        assertions = _string_array(value["assertions"], f"{label}.assertions")
        evidence = _string_array(value["evidence"], f"{label}.evidence")
        if not assertions:
            fail(f"{label}.assertions must be nonempty")
        registry_ids = sorted({"t17-01", *evidence})
        paths = ["source/compliance-matrix.json", _tuple_path(rows["t17-01"])]
        for evidence_id in evidence:
            if evidence_id not in rows:
                fail(f"{label} references unknown evidence ID: {evidence_id}")
            paths.append(_tuple_path(rows[evidence_id]))
        result.append(_check(audit_id, registry_ids, paths))
    return result


def _tuple_path(row: CommandSpec) -> str:
    if row.task_owner.isdecimal():
        return f"tasks/task-{int(row.task_owner)}/tuples/{row.execution_context}/{row.id}/**"
    if row.task_owner == "merge":
        return f"merges/M1516/tuples/{row.execution_context}/{row.id}/**"
    if row.task_owner == "preflight":
        return f"preflight/tuples/{row.id}/**"
    if row.task_owner == "prerequisite":
        return f"prerequisite/tuples/{row.id}/**"
    fail(f"registry ID cannot map into aggregate: {row.id}")


def _check(identifier: str, registry_ids: list[str], paths: list[str]) -> F1Check:
    return F1Check(identifier, tuple(sorted(set(registry_ids))), tuple(sorted(set(paths))))


def _string_array(value: object, label: str) -> list[str]:
    raw = require_list(value, label)
    result = [require_string(item, f"{label}[{index}]") for index, item in enumerate(raw)]
    require_sorted_unique(result, label)
    return result
