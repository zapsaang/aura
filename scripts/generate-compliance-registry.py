#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.model import EvidenceError, canonical_json_bytes
from evidence.registry import build_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check the normative QA registry")
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = canonical_json_bytes(build_registry(args.plan).as_json())
    if args.check:
        if not args.out.is_file() or args.out.read_bytes() != expected:
            raise EvidenceError(f"registry is stale: {args.out}")
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_bytes(expected)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvidenceError as error:
        raise SystemExit(f"error: {error}") from error
