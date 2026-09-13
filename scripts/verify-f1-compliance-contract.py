#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.contract import (
    load_f1_contract,
    validate_f1_contract,
    verify_f1_contract_matches_sources,
)
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Verify the F1 compliance mapping")
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--matrix", type=Path, required=True)
    args = parser.parse_args()
    registry = load_registry(args.registry)
    contract = load_f1_contract(args.contract)
    validate_f1_contract(contract, registry)
    verify_f1_contract_matches_sources(contract, registry, args.matrix)
    print(canonical_json_bytes({"checks": 3, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvidenceError as error:
        raise SystemExit(f"error: {error}") from error
