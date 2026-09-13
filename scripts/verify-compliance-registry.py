#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.model import EvidenceError, canonical_json_bytes
from evidence.registry import load_registry, verify_registry_matches_plan


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify the closed QA registry against the plan")
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    args = parser.parse_args()
    registry = load_registry(args.registry)
    verify_registry_matches_plan(registry, args.plan)
    print(canonical_json_bytes({"checks": 2, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvidenceError as error:
        raise SystemExit(f"error: {error}") from error
