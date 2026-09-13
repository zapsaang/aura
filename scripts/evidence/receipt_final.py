from __future__ import annotations

from dataclasses import dataclass

from .model import fail, require_exact_keys, require_hex
from .receipt_common import _identity, _schema_status


@dataclass(frozen=True)
class FinalIdentity:
    tag: str
    verified_commit: str
    plan_sha256: str
    aggregate_sha256: str


def validate_final_receipt(payload: dict[str, object], identity: FinalIdentity) -> None:
    keys = frozenset({
        "aggregate_sha256", "manifest_sha256", "plan_sha256", "schema_version", "status",
        "tag", "verdict_receipts", "verified_commit",
    })
    require_exact_keys(payload, keys, "final receipt")
    _schema_status(payload, "final receipt", status=True)
    _identity(payload, identity.verified_commit, identity.plan_sha256, identity.tag, identity.aggregate_sha256)
    require_hex(payload["manifest_sha256"], 64, "manifest_sha256")
    verdicts = payload["verdict_receipts"]
    if not isinstance(verdicts, dict):
        fail("verdict_receipts must be an object")
    require_exact_keys(verdicts, frozenset({"F1", "F2", "F3", "F4"}), "verdict_receipts")
    for gate in sorted(verdicts):
        require_hex(verdicts[gate], 64, f"verdict_receipts.{gate}")
