#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.aggregate import assemble_aggregate
from evidence.model import EvidenceError
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate producer and lane closures and assemble the sole aggregate")
    parser.add_argument("--lanes", type=Path, required=True)
    parser.add_argument("--producers", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--plan", type=Path, required=True)
    parser.add_argument("--plan-sha256", required=True)
    parser.add_argument("--operation-sources", required=True)
    parser.add_argument("--merged-operations-out", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    assemble_aggregate(
        args.lanes,
        args.producers,
        load_registry(args.registry),
        args.plan,
        args.plan_sha256,
        tuple(args.operation_sources.split(",")),
        args.merged_operations_out,
        args.out,
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
