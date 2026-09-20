from __future__ import annotations

import shutil
import tempfile
from pathlib import Path

from .aggregate_git import (
    _commit_subjects,
    _require_commit_graph,
    _subject_commit,
    _tag_for_commit,
    _task_commit,
)
from .aggregate_producers import PRODUCER_NAMES, _open_producers, _require_producer_closure
from .aggregate_staging import (
    _assemble_owner_evidence,
    _copy_inputs,
    _copy_releases,
    _merge_logs,
)
from .archive import safe_extract_archive
from .lane import verify_lane_tree
from .manifest import verify_manifest, write_manifest
from .model import (
    canonical_json_bytes,
    decode_json_bytes,
    fail,
    require_hex,
    require_tag,
    sha256_file,
    write_new,
)
from .receipt import LANE_JOBS, AggregateIdentity, validate_aggregate_receipt
from .registry import Registry, verify_registry_matches_plan

__all__ = (
    "OPERATION_SOURCES",
    "PRODUCER_NAMES",
    "assemble_aggregate",
    "verify_aggregate_tree",
)

OPERATION_SOURCES = (
    "preflight", "source", "ubuntu-msrv", "ubuntu-default", "ubuntu-gpu", "macos-default",
    "home-manager-semantic", "release-linux-x86", "release-linux-arm64", "release-macos-arm64",
    "release-macos-x86",
)


def assemble_aggregate(
    lanes_directory: Path,
    producers_directory: Path,
    registry: Registry,
    plan: Path,
    plan_sha256: str,
    operation_sources: tuple[str, ...],
    merged_operations_out: Path,
    output: Path,
) -> dict[str, object]:
    if operation_sources != OPERATION_SOURCES:
        fail("operation source order differs from the normative merge order")
    if sha256_file(plan) != require_hex(plan_sha256, 64, "plan_sha256"):
        fail("operational plan digest drift")
    verify_registry_matches_plan(registry, plan)
    if output.exists() or output.is_symlink():
        fail(f"aggregate output must be fresh: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir(mode=0o700)
    try:
        with tempfile.TemporaryDirectory(prefix="aura-lanes-", dir=output.parent) as raw_temporary:
            temporary = Path(raw_temporary)
            temporary.chmod(0o700)
            lane_roots, lane_digests, commit, run_id, run_attempt = _open_lanes(
                lanes_directory, temporary, registry
            )
            producers, producer_entries = _open_producers(
                producers_directory, temporary, registry, commit, run_id, run_attempt, plan
            )
            _require_commit_graph(producers, commit)
            _copy_inputs(output, lanes_directory, lane_roots, producers)
            task_entries, merge_entries = _assemble_owner_evidence(output, lane_roots, registry, commit)
            release_entries = _copy_releases(output, lane_roots)
            _merge_logs(output, producers, lane_roots, operation_sources, merged_operations_out)
            tag = _tag_for_commit(commit)
            aggregate_digest = write_manifest(output, frozenset({"SHA256SUMS", "receipt.json"}))
            receipt: dict[str, object] = {
                "aggregate_sha256": aggregate_digest,
                "lanes": lane_digests,
                "merges": merge_entries,
                "plan_sha256": plan_sha256,
                "preflight_receipt_sha256": sha256_file(producers["preflight"] / "receipt.json"),
                "prerequisite_receipt_sha256": sha256_file(producers["prerequisite"] / "receipt.json"),
                "producer_archives": producer_entries,
                "releases": release_entries,
                "run_attempt": run_attempt,
                "run_id": run_id,
                "schema_version": 1,
                "source_receipt_sha256": sha256_file(producers["source"] / "receipt.json"),
                "tag": tag,
                "tasks": task_entries,
                "verified_commit": commit,
            }
            validate_aggregate_receipt(receipt, AggregateIdentity(commit, plan_sha256, tag, aggregate_digest))
            write_new(output / "receipt.json", canonical_json_bytes(receipt))
            return receipt
    except BaseException:
        shutil.rmtree(output, ignore_errors=True)
        raise


def verify_aggregate_tree(root: Path, plan: Path, plan_sha256: str) -> dict[str, object]:
    if sha256_file(plan) != plan_sha256:
        fail("operational plan digest drift")
    value = decode_json_bytes((root / "receipt.json").read_bytes(), "aggregate receipt")
    if not isinstance(value, dict):
        fail("aggregate receipt must be an object")
    aggregate_digest = verify_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
    identity = AggregateIdentity(
        require_hex(value.get("verified_commit"), 40, "verified_commit"),
        plan_sha256,
        require_tag(value.get("tag")),
        aggregate_digest,
    )
    validate_aggregate_receipt(value, identity)
    return value


def _open_lanes(
    lanes_directory: Path, temporary: Path, registry: Registry
) -> tuple[dict[str, Path], list[dict[str, object]], str, str, int]:
    roots: dict[str, Path] = {}
    entries: list[dict[str, object]] = []
    headers: list[dict[str, object]] = []
    actual = sorted(path.name for path in lanes_directory.iterdir())
    expected = [f"{job}.tar.gz" for job in LANE_JOBS]
    if actual != expected:
        fail(f"staged lane archive set differs: expected={expected}, actual={actual}")
    for job in LANE_JOBS:
        archive = lanes_directory / f"{job}.tar.gz"
        digest = sha256_file(archive)
        destination = temporary / job
        root = safe_extract_archive(archive, destination, expected_root=f"lane/{job}", expected_sha256=digest)
        header = decode_json_bytes((root / "receipt.json").read_bytes(), f"{job} receipt")
        if not isinstance(header, dict):
            fail(f"{job} receipt must be an object")
        headers.append(header)
        roots[job] = root
        entries.append({"archive_sha256": digest, "job": job})
    commits = {str(header.get("verified_commit")) for header in headers}
    run_ids = {str(header.get("run_id")) for header in headers}
    attempts = {header.get("run_attempt") for header in headers}
    if len(commits) != 1 or len(run_ids) != 1 or len(attempts) != 1:
        fail("lane receipt headers do not have unanimous identity")
    commit = require_hex(next(iter(commits)), 40, "verified_commit")
    run_id = next(iter(run_ids))
    run_attempt = next(iter(attempts))
    if not isinstance(run_attempt, int) or isinstance(run_attempt, bool) or run_attempt < 1:
        fail("invalid unanimous run_attempt")
    for job, root in roots.items():
        verify_lane_tree(root, registry, job, commit)
    return roots, entries, commit, run_id, run_attempt
