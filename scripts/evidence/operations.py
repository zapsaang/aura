from __future__ import annotations

import os
from pathlib import Path

from .model import (
    canonical_json_bytes,
    decode_json_bytes,
    fail,
    require_exact_keys,
    require_hex,
    require_string,
    require_uint,
)

KEYS = frozenset({"input_sha256", "local_sequence", "operation", "output_sha256"})
MERGED_KEYS = frozenset({"input_sha256", "operation", "output_sha256", "sequence"})


def append_operation(path: Path, operation: str, input_sha256: str | None, output_sha256: str | None) -> None:
    entries = load_operations(path) if path.exists() else []
    value: dict[str, object] = {
        "input_sha256": input_sha256,
        "local_sequence": len(entries),
        "operation": operation,
        "output_sha256": output_sha256,
    }
    _validate_entry(value, len(entries), f"operation {len(entries)}")
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("ab") as target:
        target.write(canonical_json_bytes(value))


def load_operations(path: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for index, raw in enumerate(path.read_bytes().splitlines(keepends=True)):
        value = decode_json_bytes(raw, f"{path}:{index + 1}")
        if not isinstance(value, dict):
            fail(f"{path}:{index + 1} must be an object")
        _validate_entry(value, index, f"{path}:{index + 1}")
        entries.append(value)
    return entries


def merge_operations(sources: list[tuple[str, Path]], output: Path) -> None:
    if output.exists() or output.is_symlink():
        fail(f"merged operations output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as target:
        sequence = 0
        for _source_name, path in sources:
            for entry in load_operations(path):
                merged = {
                    "input_sha256": entry["input_sha256"],
                    "operation": entry["operation"],
                    "output_sha256": entry["output_sha256"],
                    "sequence": sequence,
                }
                target.write(canonical_json_bytes(merged))
                sequence += 1
        target.flush()
        os.fsync(target.fileno())


def validate_merged_operations(path: Path) -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for index, raw in enumerate(path.read_bytes().splitlines(keepends=True)):
        label = f"{path}:{index + 1}"
        value = decode_json_bytes(raw, label)
        if not isinstance(value, dict):
            fail(f"{label} must be an object")
        require_exact_keys(value, MERGED_KEYS, label)
        if require_uint(value["sequence"], f"{label}.sequence") != index:
            fail(f"noncontiguous merged sequence in {label}")
        if not require_string(value["operation"], f"{label}.operation"):
            fail(f"{label}.operation must be non-empty")
        for field in ("input_sha256", "output_sha256"):
            digest = value[field]
            if digest is not None:
                require_hex(digest, 64, f"{label}.{field}")
        entries.append(value)
    return entries


def _validate_entry(value: dict[str, object], index: int, label: str) -> None:
    require_exact_keys(value, KEYS, label)
    if require_uint(value["local_sequence"], f"{label}.local_sequence") != index:
        fail(f"noncontiguous local sequence in {label}")
    require_string(value["operation"], f"{label}.operation")
    for field in ("input_sha256", "output_sha256"):
        digest = value[field]
        if digest is not None:
            require_hex(digest, 64, f"{label}.{field}")
