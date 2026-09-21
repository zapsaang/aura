from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

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

ALLOWED_STATUS = frozenset({"approved", "no-op"})
ALLOWED_TARGETS = frozenset({"github-release", "homebrew"})
COMMON_KEYS = frozenset({
    "created_at",
    "formula_sha256",
    "manifest_sha256",
    "run_attempt",
    "run_id",
    "schema_version",
    "seal_actor",
    "status",
    "tag",
    "target",
    "verified_commit",
})
GITHUB_RELEASE_EXTRA = frozenset({
    "assets_manifest_sha256",
    "release_id",
    "release_url",
})
HOMEBREW_EXTRA = frozenset({
    "pr_branch",
    "pr_number",
    "pr_url",
    "tap_default_branch",
})


@dataclass(frozen=True, slots=True)
class PublishIdentity:
    target: Literal["github-release", "homebrew"]
    run_id: str
    run_attempt: int
    tag: str
    verified_commit: str
    formula_sha256: str


def validate_publish_receipt(payload: dict[str, object], identity: PublishIdentity) -> None:
    target = require_enum(identity.target, ALLOWED_TARGETS, "publish identity target")
    extras = GITHUB_RELEASE_EXTRA if target == "github-release" else HOMEBREW_EXTRA
    label = f"{target} publish receipt"
    require_exact_keys(payload, COMMON_KEYS | extras, label)
    _schema_status(payload, label, status=False)
    status = require_enum(payload["status"], ALLOWED_STATUS, "publish status")
    if require_enum(payload["target"], ALLOWED_TARGETS, "publish target") != target:
        fail("publish target drift")
    if require_safe_id(payload["run_id"], "publish run_id") != identity.run_id:
        fail("publish run_id drift")
    if require_uint(payload["run_attempt"], "publish run_attempt", positive=True) != identity.run_attempt:
        fail("publish run_attempt drift")
    if require_tag(payload["tag"]) != identity.tag:
        fail("publish tag drift")
    if require_hex(payload["verified_commit"], 40, "publish verified_commit") != identity.verified_commit:
        fail("publish verified_commit drift")
    if require_hex(payload["formula_sha256"], 64, "publish formula_sha256") != identity.formula_sha256:
        fail("publish formula_sha256 drift")
    require_hex(payload["manifest_sha256"], 64, "publish manifest_sha256")
    require_string(payload["created_at"], "publish created_at")
    if require_string(payload["seal_actor"], "publish seal_actor") != "aura-publisher":
        fail("publish seal_actor drift")
    if target == "github-release":
        require_string(payload["release_url"], "publish release_url")
        require_uint(payload["release_id"], "publish release_id", positive=True)
        require_hex(payload["assets_manifest_sha256"], 64, "publish assets_manifest_sha256")
        return
    pr_number = require_uint(payload["pr_number"], "publish pr_number")
    pr_branch = require_string(payload["pr_branch"], "publish pr_branch")
    pr_url = require_string(payload["pr_url"], "publish pr_url")
    require_string(payload["tap_default_branch"], "publish tap_default_branch")
    if status == "no-op" and pr_number == 0:
        if pr_branch or pr_url:
            fail("no-op without PR must have empty pr_branch/pr_url")
        return
    if pr_number < 1 or not pr_branch or not pr_url:
        fail("publish PR fields incomplete")
