#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.final import seal_gate
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.receipt import GateIdentity
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Seal one identity-bound final gate receipt")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--gate", choices=("F1", "F2", "F3", "F4"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--plan-sha256", required=True)
    parser.add_argument("--aggregate-sha256", required=True)
    parser.add_argument("--evidence", type=Path, action="append", default=[])
    parser.add_argument("--handoff", action="append", default=[])
    args = parser.parse_args()
    handoffs = [_handoff(value) for value in args.handoff]
    identity = GateIdentity(
        args.gate, args.tag, args.verified_commit, args.plan_sha256, args.aggregate_sha256
    )
    seal_gate(args.root, load_registry(args.registry), identity, args.evidence, handoffs)
    print(canonical_json_bytes({"gate": args.gate, "status": "approved"}).decode("utf-8"), end="")
    return 0


def _handoff(value: str) -> tuple[str, Path]:
    platform, separator, path = value.partition("=")
    if not separator or platform not in {"linux", "macos"}:
        raise EvidenceError(f"invalid handoff: {value}")
    return platform, Path(path)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
