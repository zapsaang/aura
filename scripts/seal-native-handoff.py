#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from evidence.final import seal_native_handoff
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.receipt import HandoffIdentity
from evidence.registry import load_registry


def main() -> int:
    parser = argparse.ArgumentParser(description="Seal one F3 native smoke handoff")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--platform", choices=("linux", "macos"), required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--aggregate-sha256", required=True)
    parser.add_argument("--archive-path", required=True)
    parser.add_argument("--archive-sha256", required=True)
    parser.add_argument("--archive-file", type=Path, required=True)
    args = parser.parse_args()
    identity = HandoffIdentity(
        args.platform, args.run_id, args.run_attempt, args.tag, args.verified_commit, args.aggregate_sha256
    )
    seal_native_handoff(
        args.root,
        load_registry(args.registry),
        identity,
        args.archive_path,
        args.archive_sha256,
        args.archive_file,
    )
    print(canonical_json_bytes({"platform": args.platform, "status": "approved"}).decode("utf-8"), end="")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
