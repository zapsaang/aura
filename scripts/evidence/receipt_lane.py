from __future__ import annotations

from .model import (
    fail,
    require_enum,
    require_exact_keys,
    require_hex,
    require_list,
    require_safe_id,
    require_sorted_unique,
    require_string,
    require_uint,
)
from .receipt_common import _validate_digest_entries

LANE_JOBS = (
    "home-manager-semantic",
    "macos-default",
    "release-linux-arm64",
    "release-linux-x86",
    "release-macos-arm64",
    "release-macos-x86",
    "ubuntu-default",
    "ubuntu-gpu",
    "ubuntu-msrv",
)
LANE_CONTRACTS = {
    "home-manager-semantic": ("x86_64-linux", (), "executed"),
    "macos-default": ("aarch64-apple-darwin", ("all",), "executed"),
    "release-linux-arm64": (
        "aarch64-unknown-linux-gnu",
        ("aura-daemon/gpu-nvml",),
        "build-only",
    ),
    "release-linux-x86": (
        "x86_64-unknown-linux-gnu",
        ("aura-daemon/gpu-nvml",),
        "executed",
    ),
    "release-macos-arm64": ("aarch64-apple-darwin", (), "executed"),
    "release-macos-x86": ("x86_64-apple-darwin", (), "build-only"),
    "ubuntu-default": ("x86_64-linux", (), "executed"),
    "ubuntu-gpu": ("x86_64-linux", ("aura-daemon/gpu-nvml",), "executed"),
    "ubuntu-msrv": ("x86_64-linux", (), "executed"),
}


def validate_lane_receipt(payload: dict[str, object], job: str, verified_commit: str) -> None:
    keys = frozenset({
        "commands", "environment_sha256", "features", "job", "manifest_sha256",
        "release_archives", "run_attempt", "run_id", "runner", "schema_version", "status",
        "target", "tools", "verified_commit",
    })
    require_exact_keys(payload, keys, "lane receipt")
    if payload["schema_version"] != 1:
        fail("unsupported lane receipt schema")
    expected_target, expected_features, expected_status = LANE_CONTRACTS[job]
    if payload["status"] != expected_status:
        fail("lane status differs")
    if require_enum(payload["job"], frozenset(LANE_JOBS), "job") != job:
        fail("lane job drift")
    if require_hex(payload["verified_commit"], 40, "verified_commit") != verified_commit:
        fail("lane commit drift")
    require_safe_id(payload["run_id"], "run_id")
    require_uint(payload["run_attempt"], "run_attempt", positive=True)
    require_string(payload["runner"], "runner")
    if require_string(payload["target"], "target") != expected_target:
        fail("lane target differs")
    for field in ("environment_sha256", "manifest_sha256"):
        require_hex(payload[field], 64, field)
    features = [require_string(item, "feature") for item in require_list(payload["features"], "features")]
    require_sorted_unique(features, "features")
    if features != list(expected_features):
        fail("lane features differ")
    _validate_tools(payload["tools"], job)
    _validate_lane_commands(payload["commands"])
    _validate_digest_entries(payload["release_archives"], "path", "release_archives")


def _validate_lane_commands(value: object) -> None:
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
        identifiers.append(require_safe_id(entry["id"], f"commands[{index}].id"))
        if require_uint(entry["exit_code"], "exit_code") != 0:
            fail("lane command failed")
        require_uint(entry["tests_executed"], "tests_executed")
        require_hex(entry["tuple_sha256"], 64, "tuple_sha256")
    require_sorted_unique(identifiers, "lane commands")


def _validate_tools(value: object, job: str) -> None:
    if not isinstance(value, dict):
        fail("tools must be an object")
    require_exact_keys(value, frozenset({"cargo", "cross", "nix", "rustc"}), "tools")
    for key, item in value.items():
        if item is not None and not isinstance(item, str):
            fail(f"tools.{key} must be string or null")
    if job == "home-manager-semantic":
        if any(value[key] is not None for key in ("cargo", "cross", "rustc")) or value["nix"] is None:
            fail("home-manager tools differ")
    elif value["cargo"] is None or value["rustc"] is None or value["nix"] is not None:
        fail("Rust lane tools differ")
