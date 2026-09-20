#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.lane import verify_lane_tree
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify one sealed compliance lane")
    parser.add_argument("--lane", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--job", required=True)
    parser.add_argument("--verified-commit", required=True)
    args = parser.parse_args()
    receipt = verify_lane_tree(args.lane, load_registry(args.registry), args.job, args.verified_commit)
    print(canonical_json_bytes({"commands": len(receipt["commands"]), "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
