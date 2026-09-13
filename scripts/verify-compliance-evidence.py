#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.compliance import verify_compliance
from evidence.model import EvidenceError, canonical_json_bytes


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify all 39 F1 compliance bindings")
    parser.add_argument("--aggregate", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--plan-sha256", required=True)
    parser.add_argument("--matrix", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    checks = verify_compliance(
        args.aggregate, args.contract, args.plan, args.plan_sha256, args.matrix, args.out
    )
    print(canonical_json_bytes({"checks": checks, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
