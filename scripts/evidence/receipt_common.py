from __future__ import annotations

from .model import (
    fail,
    require_exact_keys,
    require_hex,
    require_list,
    require_sorted_unique,
    require_string,
    require_tag,
)


def _schema_status(payload: dict[str, object], label: str, *, status: bool) -> None:
    if payload["schema_version"] != 1:
        fail(f"unsupported {label} schema")
    if status and payload["status"] != "approved":
        fail(f"{label} is not approved")


def _identity(payload: dict[str, object], commit: str, plan: str, tag: str, aggregate: str) -> None:
    if require_hex(payload["verified_commit"], 40, "verified_commit") != commit:
        fail("verified_commit drift")
    if require_hex(payload["plan_sha256"], 64, "plan_sha256") != plan:
        fail("plan_sha256 drift")
    if require_tag(payload["tag"]) != tag:
        fail("tag drift")
    if require_hex(payload["aggregate_sha256"], 64, "aggregate_sha256") != aggregate:
        fail("aggregate_sha256 drift")


def _validate_digest_entries(
    value: object,
    identity_key: str,
    label: str,
    *,
    digest_key: str = "sha256",
) -> list[dict[str, object]]:
    raw = require_list(value, label)
    entries: list[dict[str, object]] = []
    identities: list[str] = []
    for index, entry in enumerate(raw):
        if not isinstance(entry, dict):
            fail(f"{label}[{index}] must be an object")
        require_exact_keys(entry, frozenset({identity_key, digest_key}), f"{label}[{index}]")
        identities.append(require_string(entry[identity_key], f"{label}[{index}].{identity_key}"))
        require_hex(entry[digest_key], 64, f"{label}[{index}].{digest_key}")
        entries.append(entry)
    require_sorted_unique(identities, label)
    return entries
