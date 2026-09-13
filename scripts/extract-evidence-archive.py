#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.archive import safe_extract_archive
from evidence.model import EvidenceError, canonical_json_bytes


def main() -> int:
    parser = argparse.ArgumentParser(description="Safely extract a digest-bound evidence archive")
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--sha256", required=True)
    args = parser.parse_args()
    extracted = safe_extract_archive(
        args.archive,
        args.destination,
        expected_root=args.root,
        expected_sha256=args.sha256,
    )
    print(canonical_json_bytes({"root": extracted.as_posix(), "status": "ok"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
