from __future__ import annotations

from dataclasses import dataclass

from .model import (
    fail,
    require_enum,
    require_exact_keys,
    require_hex,
    require_list,
    require_safe_id,
    require_string,
    require_uint,
)
from .receipt_common import _identity, _schema_status, _validate_digest_entries
from .receipt_lane import LANE_JOBS

RELEASE_PATHS = (
    "releases/aura-aarch64-apple-darwin.tar.gz",
    "releases/aura-aarch64-unknown-linux-gnu.tar.gz",
    "releases/aura-x86_64-apple-darwin.tar.gz",
    "releases/aura-x86_64-unknown-linux-gnu.tar.gz",
    "releases/homebrew/aura.rb",
)


@dataclass(frozen=True)
class AggregateIdentity:
    verified_commit: str
    plan_sha256: str
    tag: str
    aggregate_sha256: str


def validate_aggregate_receipt(payload: dict[str, object], identity: AggregateIdentity) -> None:
    keys = frozenset({
        "aggregate_sha256", "lanes", "merges", "plan_sha256", "preflight_receipt_sha256",
        "prerequisite_receipt_sha256", "producer_archives", "releases", "run_attempt", "run_id",
        "schema_version", "source_receipt_sha256", "tag", "tasks", "verified_commit",
    })
    require_exact_keys(payload, keys, "aggregate receipt")
    _schema_status(payload, "aggregate receipt", status=False)
    require_safe_id(payload["run_id"], "aggregate run_id")
    require_uint(payload["run_attempt"], "aggregate run_attempt", positive=True)
    _identity(payload, identity.verified_commit, identity.plan_sha256, identity.tag, identity.aggregate_sha256)
    for field in ("preflight_receipt_sha256", "prerequisite_receipt_sha256", "source_receipt_sha256"):
        require_hex(payload[field], 64, field)
    _validate_lanes(payload["lanes"])
    _validate_tasks(payload["tasks"])
    _validate_merges(payload["merges"])
    _validate_releases(payload["releases"])
    _validate_producer_archives(payload["producer_archives"])


def _validate_lanes(value: object) -> None:
    entries = _validate_digest_entries(value, "job", "lanes", digest_key="archive_sha256")
    if [entry["job"] for entry in entries] != list(LANE_JOBS):
        fail("aggregate lane set differs")


def _validate_tasks(value: object) -> None:
    entries = require_list(value, "tasks")
    tasks: list[str] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            fail(f"tasks[{index}] must be an object")
        require_exact_keys(entry, frozenset({"receipt_sha256", "task"}), f"tasks[{index}]")
        task = require_string(entry["task"], f"tasks[{index}].task")
        require_hex(entry["receipt_sha256"], 64, f"tasks[{index}].receipt_sha256")
        tasks.append(task)
    if tasks != [str(number) for number in range(1, 18)]:
        fail("aggregate task set differs")


def _validate_merges(value: object) -> None:
    entries = require_list(value, "merges")
    if len(entries) != 1 or not isinstance(entries[0], dict):
        fail("aggregate requires exactly one merge")
    require_exact_keys(entries[0], frozenset({"merge", "receipt_sha256"}), "merges[0]")
    if entries[0]["merge"] != "M1516":
        fail("aggregate merge identity differs")
    require_hex(entries[0]["receipt_sha256"], 64, "merges[0].receipt_sha256")


def _validate_releases(value: object) -> None:
    entries = _validate_digest_entries(value, "path", "releases")
    if [entry["path"] for entry in entries] != list(RELEASE_PATHS):
        fail("aggregate release set differs")


def _validate_producer_archives(value: object) -> None:
    entries = require_list(value, "producer_archives")
    producers: list[str] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            fail(f"producer_archives[{index}] must be an object")
        require_exact_keys(
            entry,
            frozenset({"archive_sha256", "producer", "receipt_sha256"}),
            f"producer_archives[{index}]",
        )
        producers.append(require_enum(
            entry["producer"], frozenset({"preflight", "prerequisite", "source"}), "producer"
        ))
        require_hex(entry["archive_sha256"], 64, "archive_sha256")
        require_hex(entry["receipt_sha256"], 64, "receipt_sha256")
    if producers != ["preflight", "prerequisite", "source"]:
        fail("aggregate producer archive set differs")
