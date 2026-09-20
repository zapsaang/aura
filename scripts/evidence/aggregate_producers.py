from __future__ import annotations

from pathlib import Path

from .aggregate_git import _git
from .archive import safe_extract_archive
from .model import decode_json_bytes, fail, require_hex, sha256_file
from .producer import source_provenance, verify_producer_tree
from .registry import Registry

PRODUCER_NAMES = ("preflight", "prerequisite", "source")


def _require_producer_closure(producer_input: Path, archives: Path) -> None:
    staged = sorted(path.name for path in producer_input.iterdir())
    if staged != sorted(PRODUCER_NAMES):
        fail(f"producer staging must be exactly {sorted(PRODUCER_NAMES)}: {staged}")
    expected_archives = [f"{name}.tar.gz" for name in sorted(PRODUCER_NAMES)]
    actual_archives = sorted(path.name for path in archives.iterdir())
    if actual_archives != expected_archives:
        fail(f"producer archive set must be exactly {expected_archives}: {actual_archives}")


def _open_producers(
    producer_input: Path,
    temporary: Path,
    registry: Registry,
    commit: str,
    run_id: str,
    run_attempt: int,
    plan: Path,
) -> tuple[dict[str, Path], list[dict[str, object]]]:
    roots: dict[str, Path] = {}
    entries: list[dict[str, object]] = []
    archives = producer_input.parent / "producers"
    _require_producer_closure(producer_input, archives)
    provenance = source_provenance(plan, _git)
    for producer in PRODUCER_NAMES:
        root = producer_input / producer
        if (root / producer).is_dir():
            root = root / producer
        roots[producer] = root
        archive = archives / f"{producer}.tar.gz"
        archive_digest = sha256_file(archive)
        extracted = safe_extract_archive(
            archive,
            temporary / f"producer-{producer}",
            expected_root=producer,
            expected_sha256=archive_digest,
        )
        if _tree_digests(extracted) != _tree_digests(root):
            fail(f"extracted {producer} tree differs from its staged archive")
        receipt_value = decode_json_bytes((root / "receipt.json").read_bytes(), f"{producer} receipt")
        if not isinstance(receipt_value, dict):
            fail(f"{producer} receipt must be an object")
        recorded_commit_key = {
            "preflight": "imported_baseline_commit",
            "prerequisite": "prerequisite_commit",
            "source": "verified_commit",
        }[producer]
        recorded_commit = require_hex(receipt_value.get(recorded_commit_key), 40, recorded_commit_key)
        verify_producer_tree(
            root,
            registry,
            producer,
            commit=recorded_commit,
            run_id=run_id if producer != "prerequisite" else None,
            run_attempt=run_attempt if producer != "prerequisite" else None,
            provenance=provenance if producer == "source" else None,
        )
        if producer == "source" and recorded_commit != commit:
            fail("source producer commit differs from lane commit")
        entries.append({
            "archive_sha256": archive_digest,
            "producer": producer,
            "receipt_sha256": sha256_file(root / "receipt.json"),
        })
    return roots, entries


def _tree_digests(root: Path) -> list[tuple[str, str]]:
    result: list[tuple[str, str]] = []
    for path in root.rglob("*"):
        if path.is_symlink() or (not path.is_file() and not path.is_dir()):
            fail(f"unsupported producer tree path: {path}")
        if path.is_file():
            result.append((path.relative_to(root).as_posix(), sha256_file(path)))
    return sorted(result)
