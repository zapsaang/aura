#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.model import EvidenceError, canonical_json_bytes
from evidence.producer import seal_producer
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Seal one preflight, prerequisite, or source producer")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--producer", choices=("preflight", "prerequisite", "source"), required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--run-id")
    parser.add_argument("--run-attempt", type=int)
    args = parser.parse_args()
    receipt = seal_producer(
        args.root,
        load_registry(args.registry),
        args.producer,
        run_id=args.run_id,
        run_attempt=args.run_attempt,
        commit=args.commit,
    )
    print(canonical_json_bytes({"producer": args.producer, "status": receipt["status"]}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
