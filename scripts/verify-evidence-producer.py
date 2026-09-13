#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.model import EvidenceError, canonical_json_bytes
from evidence.producer import verify_producer_tree
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify one sealed evidence producer")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--producer", choices=("preflight", "prerequisite", "source"), required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--run-id")
    parser.add_argument("--run-attempt", type=int)
    args = parser.parse_args()
    verify_producer_tree(
        args.root,
        load_registry(args.registry),
        args.producer,
        commit=args.commit,
        run_id=args.run_id,
        run_attempt=args.run_attempt,
    )
    print(canonical_json_bytes({"checks": 3, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
