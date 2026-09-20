#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.manifest import verify_manifest
from evidence.model import (
    EvidenceError,
    decode_json_bytes,
    fail,
    require_hex,
    require_safe_id,
    sha256_file,
    write_new,
)
from evidence.receipt import (
    RELEASE_PATHS,
    AggregateIdentity,
    validate_aggregate_receipt,
)


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify release aggregate identity and all five publish inputs")
    parser.add_argument("--artifact", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    commit = require_hex(args.commit, 40, "commit")
    run_id = require_safe_id(args.run_id, "run_id")
    expected_artifact = f"aura-evidence-{commit}-{run_id}-{args.run_attempt}-aggregate"
    if args.artifact != expected_artifact:
        fail("aggregate artifact identity differs")
    aggregate = _find_aggregate()
    value = decode_json_bytes((aggregate / "receipt.json").read_bytes(), "aggregate receipt")
    if not isinstance(value, dict):
        fail("aggregate receipt must be an object")
    digest = verify_manifest(aggregate, frozenset({"SHA256SUMS", "receipt.json"}))
    identity = AggregateIdentity(commit, str(value.get("plan_sha256")), str(value.get("tag")), digest)
    validate_aggregate_receipt(value, identity)
    if value["run_id"] != run_id or value["run_attempt"] != args.run_attempt:
        fail("aggregate run identity differs")
    for relative in RELEASE_PATHS:
        path = aggregate / relative
        if not path.is_file() or path.is_symlink():
            fail(f"missing release input: {relative}")
    releases = {str(item["path"]): str(item["sha256"]) for item in value["releases"] if isinstance(item, dict)}
    for relative in RELEASE_PATHS:
        if releases.get(relative) != sha256_file(aggregate / relative):
            fail(f"release digest drift: {relative}")
    lines = ["# F3 Release Aggregate", "", f"- Artifact: `{args.artifact}`", f"- Commit: `{commit}`"]
    lines.extend(f"- `{relative}`: `{releases[relative]}`" for relative in RELEASE_PATHS)
    write_new(args.out, ("\n".join(lines) + "\n").encode("utf-8"))
    return 0


def _find_aggregate() -> Path:
    candidates = (
        Path("final/work/F3-input-linux/aggregate"),
        Path("final/work/F3-input-macos/aggregate"),
        Path("final/work/F3-input/aggregate"),
    )
    existing = [path for path in candidates if path.is_dir()]
    if len(existing) != 1:
        fail(f"expected exactly one F3 aggregate materialization, got {existing}")
    return existing[0]


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
