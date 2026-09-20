#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.contract import build_f1_contract
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate or check the F1 compliance mapping")
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--matrix", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = canonical_json_bytes(build_f1_contract(load_registry(args.registry), args.matrix).as_json())
    if args.check:
        if not args.out.is_file() or args.out.read_bytes() != expected:
            raise EvidenceError(f"F1 contract is stale: {args.out}")
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_bytes(expected)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvidenceError as error:
        raise SystemExit(f"error: {error}") from error
