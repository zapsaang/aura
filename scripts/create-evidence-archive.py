#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.archive import create_deterministic_archive
from evidence.model import EvidenceError, canonical_json_bytes


def main() -> int:
    parser = argparse.ArgumentParser(description="Create a deterministic evidence archive")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    digest = create_deterministic_archive(args.source, args.out, args.root)
    print(canonical_json_bytes({"sha256": digest, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
