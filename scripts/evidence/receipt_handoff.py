from __future__ import annotations

from dataclasses import dataclass

from .model import (
    fail,
    require_enum,
    require_exact_keys,
    require_hex,
    require_safe_id,
    require_string,
    require_tag,
    require_uint,
)
from .receipt_common import _schema_status


@dataclass(frozen=True)
class HandoffIdentity:
    platform: str
    run_id: str
    run_attempt: int
    tag: str
    verified_commit: str
    aggregate_sha256: str


def validate_handoff_receipt(payload: dict[str, object], identity: HandoffIdentity) -> None:
    keys = frozenset({
        "aggregate_sha256", "archive_path", "archive_sha256", "artifacts_manifest_sha256",
        "manifest_sha256", "platform", "run_attempt", "run_id", "schema_version", "status",
        "tag", "tuple_sha256", "verified_commit",
    })
    require_exact_keys(payload, keys, "native handoff receipt")
    _schema_status(payload, "native handoff receipt", status=True)
    platform = require_enum(payload["platform"], frozenset({"linux", "macos"}), "platform")
    if platform != identity.platform:
        fail("handoff platform drift")
    if require_safe_id(payload["run_id"], "run_id") != identity.run_id:
        fail("handoff run_id drift")
    if require_uint(payload["run_attempt"], "run_attempt", positive=True) != identity.run_attempt:
        fail("handoff run_attempt drift")
    if require_tag(payload["tag"]) != identity.tag:
        fail("handoff tag drift")
    if require_hex(payload["verified_commit"], 40, "verified_commit") != identity.verified_commit:
        fail("handoff commit drift")
    if require_hex(payload["aggregate_sha256"], 64, "aggregate_sha256") != identity.aggregate_sha256:
        fail("handoff aggregate digest drift")
    expected_archive = {
        "linux": "releases/aura-x86_64-unknown-linux-gnu.tar.gz",
        "macos": "releases/aura-aarch64-apple-darwin.tar.gz",
    }[platform]
    if require_string(payload["archive_path"], "archive_path") != expected_archive:
        fail("handoff archive path drift")
    for field in ("archive_sha256", "artifacts_manifest_sha256", "manifest_sha256", "tuple_sha256"):
        require_hex(payload[field], 64, field)
