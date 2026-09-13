#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from evidence.environment import create_environment
from evidence.model import EvidenceError, canonical_json_bytes


def main() -> int:
    parser = argparse.ArgumentParser(description="Create the closed lane environment")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--home", type=Path, required=True)
    parser.add_argument("--tmpdir", type=Path, required=True)
    parser.add_argument("--path", default=os.environ.get("PATH", ""))
    args = parser.parse_args()
    create_environment(args.out, args.home, args.path, args.tmpdir)
    print(canonical_json_bytes({"checks": 3, "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except EvidenceError as error:
        raise SystemExit(f"error: {error}") from error
