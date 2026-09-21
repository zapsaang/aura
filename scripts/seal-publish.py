#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from evidence.final import seal_publish
from evidence.model import EvidenceError, canonical_json_bytes
from evidence.receipt import PublishIdentity


def main() -> int:
    parser = argparse.ArgumentParser(description="Seal one evidence-bound publication receipt")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--target", choices=("github-release", "homebrew"), required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", type=int, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--verified-commit", required=True)
    parser.add_argument("--formula-sha256", required=True)
    parser.add_argument("--status", choices=("approved", "no-op"), default="approved")
    parser.add_argument("--release-url")
    parser.add_argument("--release-id", type=int)
    parser.add_argument("--assets-manifest-sha256")
    parser.add_argument("--pr-number", type=int)
    parser.add_argument("--pr-branch")
    parser.add_argument("--pr-url")
    parser.add_argument("--tap-default-branch")
    args = parser.parse_args()
    identity = PublishIdentity(
        target=args.target,
        run_id=args.run_id,
        run_attempt=args.run_attempt,
        tag=args.tag,
        verified_commit=args.verified_commit,
        formula_sha256=args.formula_sha256,
    )
    if args.target == "github-release":
        extras = {
            "assets_manifest_sha256": args.assets_manifest_sha256,
            "release_id": args.release_id,
            "release_url": args.release_url,
            "status": args.status,
        }
    else:
        extras = {
            "pr_branch": args.pr_branch,
            "pr_number": args.pr_number,
            "pr_url": args.pr_url,
            "status": args.status,
            "tap_default_branch": args.tap_default_branch,
        }
    receipt = seal_publish(args.root, identity, extras)
    print(
        canonical_json_bytes({"status": receipt["status"], "target": receipt["target"]}).decode("utf-8"),
        end="",
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (EvidenceError, OSError) as error:
        raise SystemExit(f"error: {error}") from error
