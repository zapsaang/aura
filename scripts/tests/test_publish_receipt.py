from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.final import seal_publish
from evidence.manifest import verify_manifest
from evidence.model import EvidenceError, sha256_file, utcnow_rfc3339
from evidence.receipt import PublishIdentity, validate_publish_receipt

SCRIPTS = Path(__file__).resolve().parents[1]
GITHUB_IDENTITY = PublishIdentity(
    target="github-release",
    run_id="run-123",
    run_attempt=2,
    tag="v1.2.3",
    verified_commit="a" * 40,
    formula_sha256="b" * 64,
)
HOMEBREW_IDENTITY = PublishIdentity(
    target="homebrew",
    run_id="run-123",
    run_attempt=2,
    tag="v1.2.3",
    verified_commit="a" * 40,
    formula_sha256="b" * 64,
)


def _common_payload(identity: PublishIdentity, status: str = "approved") -> dict[str, object]:
    return {
        "created_at": "2026-09-21T12:34:56Z",
        "formula_sha256": identity.formula_sha256,
        "manifest_sha256": "c" * 64,
        "run_attempt": identity.run_attempt,
        "run_id": identity.run_id,
        "schema_version": 1,
        "seal_actor": "aura-publisher",
        "status": status,
        "tag": identity.tag,
        "target": identity.target,
        "verified_commit": identity.verified_commit,
    }


def _github_payload(status: str = "approved") -> dict[str, object]:
    return {
        **_common_payload(GITHUB_IDENTITY, status),
        "assets_manifest_sha256": "d" * 64,
        "release_id": 42,
        "release_url": "https://github.com/zapsaang/aura/releases/tag/v1.2.3",
    }


def _homebrew_payload(
    status: str = "approved",
    pr_number: int = 17,
    pr_branch: str = "aura-v1.2.3",
    pr_url: str = "https://github.com/zapsaang/homebrew-tap/pull/17",
) -> dict[str, object]:
    return {
        **_common_payload(HOMEBREW_IDENTITY, status),
        "pr_branch": pr_branch,
        "pr_number": pr_number,
        "pr_url": pr_url,
        "tap_default_branch": "main",
    }


def _github_extras(assets_manifest_sha256: str = "d" * 64) -> dict[str, object]:
    return {
        "assets_manifest_sha256": assets_manifest_sha256,
        "release_id": 42,
        "release_url": "https://github.com/zapsaang/aura/releases/tag/v1.2.3",
        "status": "approved",
    }


class PublishReceiptValidationTests(unittest.TestCase):
    def test_github_release_receipt_is_accepted_when_approved(self) -> None:
        # Given
        payload = _github_payload()

        # When / Then
        validate_publish_receipt(payload, GITHUB_IDENTITY)

    def test_receipt_is_rejected_when_schema_version_is_missing(self) -> None:
        # Given
        payload = _github_payload()
        del payload["schema_version"]

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, GITHUB_IDENTITY)

    def test_receipt_is_rejected_when_status_is_unknown(self) -> None:
        # Given
        payload = _github_payload("executed")

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, GITHUB_IDENTITY)

    def test_receipt_is_rejected_when_target_differs_from_identity(self) -> None:
        # Given
        payload = _github_payload()
        payload["target"] = "homebrew"

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, GITHUB_IDENTITY)

    def test_receipt_is_rejected_when_formula_digest_differs(self) -> None:
        # Given
        payload = _github_payload()
        payload["formula_sha256"] = "e" * 64

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, GITHUB_IDENTITY)

    def test_homebrew_receipt_is_accepted_with_complete_pr_fields(self) -> None:
        # Given
        payload = _homebrew_payload()

        # When / Then
        validate_publish_receipt(payload, HOMEBREW_IDENTITY)

    def test_homebrew_receipt_rejects_cross_target_fields(self) -> None:
        # Given
        payload = _homebrew_payload()
        payload["release_id"] = 42

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, HOMEBREW_IDENTITY)

    def test_homebrew_no_op_is_accepted_without_a_pr(self) -> None:
        # Given
        payload = _homebrew_payload("no-op", 0, "", "")

        # When / Then
        validate_publish_receipt(payload, HOMEBREW_IDENTITY)

    def test_homebrew_approved_receipt_rejects_missing_pr(self) -> None:
        # Given
        payload = _homebrew_payload("approved", 0, "", "")

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, HOMEBREW_IDENTITY)

    def test_homebrew_no_op_without_pr_rejects_partial_pr_fields(self) -> None:
        # Given
        payload = _homebrew_payload("no-op", 0, "", "https://example.invalid/pr/1")

        # When / Then
        with self.assertRaises(EvidenceError):
            validate_publish_receipt(payload, HOMEBREW_IDENTITY)


class PublishSealTests(unittest.TestCase):
    def test_seal_publish_binds_receipt_to_deterministic_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            # Given
            root = Path(directory)
            (root / "assets.txt").write_bytes(b"aura.rb digest\n")
            assets_manifest_sha256 = sha256_file(root / "assets.txt")

            # When
            receipt = seal_publish(
                root,
                GITHUB_IDENTITY,
                _github_extras(assets_manifest_sha256),
            )

            # Then
            manifest = verify_manifest(root, frozenset({"SHA256SUMS", "receipt.json"}))
            self.assertEqual(receipt["manifest_sha256"], manifest)
            self.assertEqual(receipt["assets_manifest_sha256"], assets_manifest_sha256)
            self.assertEqual(receipt["target"], "github-release")

    def test_seal_publish_rejects_unrelated_asset_manifest_digest(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            # Given
            root = Path(directory)
            (root / "assets.txt").write_bytes(b"aura.rb actual-digest\n")

            # When / Then
            with self.assertRaises(EvidenceError):
                seal_publish(root, GITHUB_IDENTITY, _github_extras("d" * 64))
            self.assertFalse((root / "SHA256SUMS").exists())

    def test_seal_publish_rejects_reentry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            # Given
            root = Path(directory)
            (root / "assets.txt").write_bytes(b"aura.rb digest\n")
            extras = _github_extras(sha256_file(root / "assets.txt"))
            seal_publish(root, GITHUB_IDENTITY, extras)

            # When / Then
            with self.assertRaises(EvidenceError):
                seal_publish(root, GITHUB_IDENTITY, extras)

    def test_seal_publish_rejects_missing_extra(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            # Given
            root = Path(directory)
            extras = _github_extras()
            del extras["release_id"]

            # When / Then
            with self.assertRaises(EvidenceError):
                seal_publish(root, GITHUB_IDENTITY, extras)
            self.assertFalse((root / "SHA256SUMS").exists())

    def test_seal_publish_rejects_unknown_extra(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            # Given
            root = Path(directory)
            extras = {**_github_extras(), "pr_number": 17}

            # When / Then
            with self.assertRaises(EvidenceError):
                seal_publish(root, GITHUB_IDENTITY, extras)
            self.assertFalse((root / "SHA256SUMS").exists())


class PublishUtilityTests(unittest.TestCase):
    def test_utcnow_rfc3339_returns_utc_z_timestamp(self) -> None:
        # Given / When
        timestamp = utcnow_rfc3339()

        # Then
        self.assertEqual(datetime.strptime(timestamp, "%Y-%m-%dT%H:%M:%SZ").year, datetime.now().year)

    def test_seal_publish_cli_reports_evidence_errors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            # Given
            command = [
                sys.executable,
                "-B",
                os.fspath(SCRIPTS / "seal-publish.py"),
                "--root", directory,
                "--target", "github-release",
                "--run-id", "run-123",
                "--run-attempt", "2",
                "--tag", "v1.2.3",
                "--verified-commit", "a" * 40,
                "--formula-sha256", "b" * 64,
                "--release-url", "https://example.invalid/release",
                "--release-id", "0",
                "--assets-manifest-sha256", "d" * 64,
            ]

            # When
            result = subprocess.run(command, check=False, capture_output=True, text=True)

            # Then
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, "")
            self.assertTrue(result.stderr.startswith("error: "), result.stderr)


if __name__ == "__main__":
    unittest.main()
