#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.final import verify_final_tree
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.receipt import FinalIdentity


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify final evidence before publication")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--plan-sha256", required=True)
    parser.add_argument("--aggregate-sha256", required=True)
    args = parser.parse_args()
    verify_final_tree(
        args.root,
        FinalIdentity(args.tag, args.verified_commit, args.plan_sha256, args.aggregate_sha256),
    )
    print(canonical_json_bytes({"checks": 4, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
