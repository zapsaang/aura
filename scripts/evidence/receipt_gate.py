from __future__ import annotations

from dataclasses import dataclass

from .model import (
    fail,
    require_enum,
    require_exact_keys,
    require_hex,
    require_list,
    require_safe_id,
    require_sorted_unique,
    require_uint,
)
from .receipt_common import _identity, _schema_status, _validate_digest_entries

GATE_COMMANDS = {
    "F1": ("F1-01", "F1-02", "F1-03", "F1-04", "F1-05"),
    "F2": ("F2-01", "F2-02", "F2-03", "F2-04", "F2-05", "F2-06", "F2-07"),
    "F3": ("F3-01", "F3-02-linux", "F3-02-macos"),
    "F4": ("F4-01", "F4-02"),
}


@dataclass(frozen=True)
class GateIdentity:
    gate: str
    tag: str
    verified_commit: str
    plan_sha256: str
    aggregate_sha256: str


def validate_gate_receipt(payload: dict[str, object], identity: GateIdentity) -> None:
    keys = frozenset({
        "aggregate_sha256", "commands", "evidence", "gate", "handoffs", "plan_sha256",
        "schema_version", "status", "tag", "verified_commit",
    })
    require_exact_keys(payload, keys, "gate receipt")
    _schema_status(payload, "gate receipt", status=True)
    gate = require_enum(payload["gate"], frozenset({"F1", "F2", "F3", "F4"}), "gate")
    if gate != identity.gate:
        fail("gate identity drift")
    _identity(payload, identity.verified_commit, identity.plan_sha256, identity.tag, identity.aggregate_sha256)
    _validate_gate_commands(payload["commands"], gate)
    _validate_digest_entries(payload["evidence"], "path", "gate evidence")
    handoffs = _validate_digest_entries(payload["handoffs"], "platform", "gate handoffs", digest_key="receipt_sha256")
    expected = ["linux", "macos"] if gate == "F3" else []
    if [entry["platform"] for entry in handoffs] != expected:
        fail(f"{gate} handoff set differs")


def _validate_gate_commands(value: object, gate: str) -> None:
    entries = require_list(value, "commands")
    identifiers: list[str] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            fail(f"commands[{index}] must be an object")
        require_exact_keys(
            entry,
            frozenset({"exit_code", "id", "tests_executed", "tuple_sha256"}),
            f"commands[{index}]",
        )
        identifier = require_safe_id(entry["id"], f"commands[{index}].id")
        if not identifier.startswith(f"{gate}-"):
            fail(f"command does not belong to {gate}: {identifier}")
        if require_uint(entry["exit_code"], "exit_code") != 0:
            fail(f"gate command failed: {identifier}")
        require_uint(entry["tests_executed"], "tests_executed")
        require_hex(entry["tuple_sha256"], 64, "tuple_sha256")
        identifiers.append(identifier)
    require_sorted_unique(identifiers, "gate commands")
    if identifiers != list(GATE_COMMANDS[gate]):
        fail(f"{gate} command set differs from the normative registry closure")
