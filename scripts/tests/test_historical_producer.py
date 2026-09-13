from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.model import EvidenceError, canonical_json_bytes, parse_json_bytes
from evidence.secure_file import require_directory_identity

SCRIPTS = Path(__file__).resolve().parents[1]


def _load_script(name: str):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), SCRIPTS / name)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


prepare = _load_script("prepare-historical-producer.py")
fidelity = _load_script("verify-source-fidelity.py")

SOURCE_ROOT_KEYS = frozenset({
    "device", "head_tree", "imported_baseline_commit", "inode", "schema_version", "source_root",
})
BASELINE = "b" * 40
HEAD_TREE = "a" * 40


def _git(root: Path, *arguments: str) -> str:
    environment = {
        **os.environ,
        "GIT_MASTER": "1",
        "GIT_AUTHOR_NAME": "test",
        "GIT_AUTHOR_EMAIL": "test@example.com",
        "GIT_COMMITTER_NAME": "test",
        "GIT_COMMITTER_EMAIL": "test@example.com",
    }
    result = subprocess.run(
        ["git", *arguments],
        check=True,
        capture_output=True,
        cwd=root,
        env=environment,
    )
    return result.stdout.decode("utf-8").strip()


def _retained_fixture(directory: Path, source_root: Path, device: int, inode: int) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    capture = {
        "device": device,
        "head_tree": HEAD_TREE,
        "imported_baseline_commit": "c" * 40,
        "inode": inode,
        "schema_version": 1,
        "source_root": os.fspath(source_root),
    }
    (directory / "source-root.json").write_bytes(canonical_json_bytes(capture))


class HistoricalProducerTests(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.base = Path(self._temporary.name)
        self.protected = self.base / "protected"
        self.protected.mkdir()
        metadata = self.protected.stat()
        self.device = metadata.st_dev
        self.inode = metadata.st_ino
        self.retained = self.base / "retained"
        _retained_fixture(self.retained, self.protected, self.device, self.inode)
        self.checkout = self.base / "checkout"
        self.checkout.mkdir()
        _git(self.checkout, "init", "-q", "-b", "main")
        (self.checkout / "file.txt").write_text("one\n", encoding="utf-8")
        _git(self.checkout, "add", "file.txt")
        _git(self.checkout, "commit", "-q", "-m", "first")

    def _capture(self) -> Path:
        out = self.base / "out"
        out.mkdir()
        prepare._capture_preflight(out, self.checkout, BASELINE, self.retained)
        return out

    def _written(self, out: Path) -> dict[str, object]:
        raw = (out / "source-root.json").read_bytes()
        return parse_json_bytes(raw, SOURCE_ROOT_KEYS, "source-root.json")

    def test_records_retained_source_root_values(self) -> None:
        out = self._capture()
        written = self._written(out)
        self.assertEqual(written["source_root"], os.fspath(self.protected))
        self.assertEqual(written["device"], self.device)
        self.assertEqual(written["inode"], self.inode)
        self.assertEqual(written["head_tree"], HEAD_TREE)
        self.assertEqual(written["imported_baseline_commit"], BASELINE)
        self.assertEqual(written["schema_version"], 1)

    def test_recorded_source_root_is_not_the_temp_checkout(self) -> None:
        out = self._capture()
        written = self._written(out)
        checkout_meta = self.checkout.stat()
        self.assertNotEqual(written["source_root"], os.fspath(self.checkout))
        self.assertFalse(
            written["device"] == checkout_meta.st_dev and written["inode"] == checkout_meta.st_ino
        )

    def test_capture_writes_worktree_evidence_and_operation(self) -> None:
        out = self._capture()
        self.assertEqual((out / "worktree" / "HEAD").read_text(encoding="ascii"), f"{BASELINE}\n")
        self.assertEqual((out / "worktree" / "status.bin").read_bytes(), b"")
        self.assertIn("capture-source-root", (out / "operations.log").read_text(encoding="utf-8"))

    def test_capture_rejects_missing_source_root(self) -> None:
        _retained_fixture(self.retained, self.base / "gone", self.device, self.inode)
        with self.assertRaises(EvidenceError) as caught:
            self._capture()
        self.assertIn("does not exist", str(caught.exception))

    def test_capture_rejects_device_change(self) -> None:
        _retained_fixture(self.retained, self.protected, self.device + 1, self.inode)
        with self.assertRaises(EvidenceError) as caught:
            self._capture()
        self.assertIn("device mismatch", str(caught.exception))

    def test_capture_rejects_inode_change(self) -> None:
        replacement = self.base / "replacement"
        replacement.mkdir()
        _retained_fixture(self.retained, self.protected, self.device, replacement.stat().st_ino)
        with self.assertRaises(EvidenceError) as caught:
            self._capture()
        self.assertIn("inode mismatch", str(caught.exception))

    def test_retained_loader_rejects_relative_source_root(self) -> None:
        _retained_fixture(self.retained, Path("relative/path"), self.device, self.inode)
        with self.assertRaises(EvidenceError):
            prepare._retained_source_root(self.retained / "source-root.json")


class SourceFidelityMessageTests(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.base = Path(self._temporary.name)
        self.protected = self.base / "protected"
        self.protected.mkdir()
        metadata = self.protected.stat()
        self.device = metadata.st_dev
        self.inode = metadata.st_ino

    def test_identity_accepts_matching_directory(self) -> None:
        require_directory_identity(self.protected, self.device, self.inode, "captured source root")

    def test_identity_distinguishes_missing_path(self) -> None:
        with self.assertRaises(EvidenceError) as caught:
            require_directory_identity(self.base / "gone", self.device, self.inode, "captured source root")
        self.assertIn("path does not exist", str(caught.exception))

    def test_identity_distinguishes_non_directory(self) -> None:
        file_path = self.base / "file.txt"
        file_path.write_text("x\n", encoding="utf-8")
        with self.assertRaises(EvidenceError) as caught:
            require_directory_identity(file_path, self.device, self.inode, "captured source root")
        self.assertIn("is not a directory", str(caught.exception))

    def test_identity_distinguishes_device_mismatch(self) -> None:
        with self.assertRaises(EvidenceError) as caught:
            require_directory_identity(self.protected, self.device + 1, self.inode, "captured source root")
        self.assertIn("device mismatch", str(caught.exception))

    def test_identity_distinguishes_inode_mismatch(self) -> None:
        with self.assertRaises(EvidenceError) as caught:
            require_directory_identity(self.protected, self.device, self.inode + 1, "captured source root")
        self.assertIn("inode mismatch", str(caught.exception))

    def test_head_tree_accepts_match(self) -> None:
        repo = self.base / "repo"
        repo.mkdir()
        _git(repo, "init", "-q", "-b", "main")
        (repo / "file.txt").write_text("one\n", encoding="utf-8")
        _git(repo, "add", "file.txt")
        _git(repo, "commit", "-q", "-m", "first")
        tree = _git(repo, "rev-parse", "--verify", "HEAD^{tree}")
        self.assertEqual(fidelity.require_head_tree(repo, tree), tree)

    def test_head_tree_distinguishes_drift(self) -> None:
        repo = self.base / "repo"
        repo.mkdir()
        _git(repo, "init", "-q", "-b", "main")
        (repo / "file.txt").write_text("one\n", encoding="utf-8")
        _git(repo, "add", "file.txt")
        _git(repo, "commit", "-q", "-m", "first")
        with self.assertRaises(EvidenceError) as caught:
            fidelity.require_head_tree(repo, "0" * 40)
        self.assertIn("HEAD tree mismatch", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
