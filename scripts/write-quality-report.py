#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.model import EvidenceError, decode_json_bytes, fail, write_new
from evidence.registry import load_registry
from evidence.tuple import verify_tuple

IDS = tuple(f"F2-{number:02d}" for number in range(1, 7))


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify the six quality tuples and write the deterministic F2 report")
    parser.add_argument("--tuples", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    registry = load_registry(Path("qa/compliance-qa-registry.json")).by_id()
    actual = sorted(path.name for path in args.tuples.iterdir() if path.is_dir())
    if actual != list(IDS):
        fail(f"F2 tuple set differs: {actual}")
    rows: list[tuple[str, int, str]] = []
    for identifier in IDS:
        path = args.tuples / identifier
        env = decode_json_bytes((path / "env.json").read_bytes(), f"{identifier} env.json")
        if not isinstance(env, dict) or not all(isinstance(key, str) and isinstance(item, str) for key, item in env.items()):
            fail(f"invalid F2 tuple environment: {identifier}")
        result = verify_tuple(path, registry[identifier], {key: item for key, item in env.items() if isinstance(item, str)})
        rows.append((identifier, result.tests_executed, result.tuple_sha256))
    lines = ["# F2 Quality Report", "", "| Command | Tests | Tuple SHA-256 |", "|---|---:|---|"]
    lines.extend(f"| {identifier} | {tests} | `{digest}` |" for identifier, tests, digest in rows)
    write_new(args.out, ("\n".join(lines) + "\n").encode("utf-8"))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
