#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.lane import seal_lane
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate command closure and seal one compliance lane")
    parser.add_argument("--lane", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--job", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--runner", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--feature", action="append", default=[])
    parser.add_argument("--rustc")
    parser.add_argument("--cargo")
    parser.add_argument("--cross")
    parser.add_argument("--nix")
    args = parser.parse_args()
    receipt = seal_lane(
        args.lane,
        load_registry(args.registry),
        job=args.job,
        run_id=args.run_id,
        run_attempt=args.run_attempt,
        verified_commit=args.verified_commit,
        runner=args.runner,
        target=args.target,
        features=args.feature,
        tools={"cargo": args.cargo, "cross": args.cross, "nix": args.nix, "rustc": args.rustc},
    )
    print(canonical_json_bytes({"job": receipt["job"], "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
