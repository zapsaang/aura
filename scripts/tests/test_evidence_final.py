from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.final import seal_native_handoff
from evidence.final_tree import validate_final_layout
from evidence.model import EvidenceError, sha256_bytes, sha256_file
from evidence.receipt import HandoffIdentity
from evidence.registry import CommandSpec, OutputRule, Registry, TestRule
from evidence.tuple import write_tuple

IDENTITY = HandoffIdentity("linux", "run", 1, "v1.2.3", "1" * 40, "2" * 64)
ARCHIVE_PATH = "releases/aura-x86_64-unknown-linux-gnu.tar.gz"


def _registry() -> Registry:
    spec = CommandSpec(
        id="F3-02-linux",
        command="true",
        shell="/usr/bin/env bash --noprofile --norc",
        cwd=".",
        env={},
        execution_context="native",
        expected_exit=0,
        count_source="constant:0",
        tests=TestRule("exact", 0),
        stdout=OutputRule("exact", ""),
        stderr="empty",
        task_owner="final",
    )
    return Registry((spec,))


def _handoff_root(base: Path) -> Path:
    root = base / f"handoff-{len(list(base.iterdir()))}"
    root.mkdir()
    write_tuple(root / "tuple", _registry().rows[0], {}, 0, b"", b"")
    artifacts = root / "artifacts"
    artifacts.mkdir()
    (artifacts / "smoke.txt").write_bytes(b"smoke\n")
    return root


class FinalTreeTests(unittest.TestCase):
    def test_seal_native_handoff_hashes_the_actual_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            archive = base / "archive.tar.gz"
            archive.write_bytes(b"release bytes\n")
            digest = sha256_file(archive)
            receipt = seal_native_handoff(
                _handoff_root(base), _registry(), IDENTITY, ARCHIVE_PATH, digest, archive,
            )
            self.assertEqual(receipt["archive_sha256"], digest)

    def test_seal_native_handoff_rejects_archive_digest_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            archive = base / "archive.tar.gz"
            archive.write_bytes(b"release bytes\n")
            forged = sha256_bytes(b"forged bytes\n")
            with self.assertRaises(EvidenceError):
                seal_native_handoff(_handoff_root(base), _registry(), IDENTITY, ARCHIVE_PATH, forged, archive)
            missing = base / "absent.tar.gz"
            with self.assertRaises(EvidenceError):
                seal_native_handoff(
                    _handoff_root(base), _registry(), IDENTITY, ARCHIVE_PATH, sha256_bytes(b""), missing,
                )

    def test_final_layout_accepts_exact_legal_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = self._legal_inputs(Path(directory))
            validate_final_layout(inputs)

    def test_final_layout_rejects_unexpected_files(self) -> None:
        for relative in (
            "stray.bin",
            "gates/F2/notes.txt",
            "gates/F1/tuples/F1-01/evil.txt",
            "gates/F1/tuples/F1-99/SHA256SUMS",
            "gates/F3/native/linux/evil.txt",
            "gates/F4/native/linux/receipt.json",
            "aggregate/extra.json",
        ):
            with self.subTest(relative=relative), tempfile.TemporaryDirectory() as directory:
                inputs = self._legal_inputs(Path(directory))
                extra = inputs / relative
                extra.parent.mkdir(parents=True, exist_ok=True)
                extra.write_bytes(b"unexpected\n")
                with self.assertRaises(EvidenceError):
                    validate_final_layout(inputs)

    def test_final_layout_rejects_missing_required_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            inputs = self._legal_inputs(Path(directory))
            (inputs / "gates" / "F4" / "scope.json").unlink()
            with self.assertRaises(EvidenceError):
                validate_final_layout(inputs)

    @staticmethod
    def _legal_inputs(base: Path) -> Path:
        inputs = base / "inputs"
        for relative in (
            "aggregate/SHA256SUMS",
            "aggregate/receipt.json",
            "operations/evidence-operations.log",
            "gates/F4/scope.json",
            "gates/F3/native/linux/receipt.json",
            "gates/F3/native/macos/receipt.json",
        ):
            path = inputs / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"placeholder\n")
        for gate in ("F1", "F2", "F3", "F4"):
            root = inputs / "gates" / gate
            (root / "tuples").mkdir(parents=True, exist_ok=True)
            (root / "report.md").write_bytes(b"report\n")
            (root / "receipt.json").write_bytes(b"{}\n")
        return inputs


if __name__ == "__main__":
    unittest.main()
