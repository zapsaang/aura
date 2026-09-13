from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.lane import checkout_head, require_checkout_identity, worktree_state
from evidence.model import EvidenceError


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


class LaneCheckoutTests(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.root = Path(self._temporary.name)
        _git(self.root, "init", "-q", "-b", "main")
        (self.root / "file.txt").write_text("one\n", encoding="utf-8")
        _git(self.root, "add", "file.txt")
        _git(self.root, "commit", "-q", "-m", "first")
        self.first = _git(self.root, "rev-parse", "HEAD")
        (self.root / "file.txt").write_text("two\n", encoding="utf-8")
        _git(self.root, "commit", "-q", "-a", "-m", "second")
        self.second = _git(self.root, "rev-parse", "HEAD")
        self.assertNotEqual(self.first, self.second)
        self.assertEqual(checkout_head(self.root), self.second)

    def test_identity_accepts_matching_clean_checkout(self) -> None:
        require_checkout_identity(self.root, self.second)

    def test_identity_rejects_head_mismatch(self) -> None:
        with self.assertRaises(EvidenceError):
            require_checkout_identity(self.root, self.first)

    def test_identity_rejects_tracked_modification(self) -> None:
        (self.root / "file.txt").write_text("dirty\n", encoding="utf-8")
        with self.assertRaises(EvidenceError):
            require_checkout_identity(self.root, self.second)

    def test_identity_rejects_untracked_file(self) -> None:
        (self.root / "untracked.txt").write_text("x\n", encoding="utf-8")
        with self.assertRaises(EvidenceError):
            require_checkout_identity(self.root, self.second)

    def test_worktree_state_is_stable_when_clean(self) -> None:
        self.assertEqual(worktree_state(self.root), worktree_state(self.root))

    def test_worktree_state_detects_new_untracked_file(self) -> None:
        before = worktree_state(self.root)
        (self.root / "appeared.txt").write_text("x\n", encoding="utf-8")
        after = worktree_state(self.root)
        self.assertNotEqual(before, after)

    def test_worktree_state_detects_removed_untracked_file(self) -> None:
        stray = self.root / "stray.txt"
        stray.write_text("x\n", encoding="utf-8")
        before = worktree_state(self.root)
        stray.unlink()
        after = worktree_state(self.root)
        self.assertNotEqual(before, after)

    def test_worktree_state_reports_dirty_initial_state(self) -> None:
        (self.root / "dirty.txt").write_text("x\n", encoding="utf-8")
        self.assertTrue(worktree_state(self.root)[2])


if __name__ == "__main__":
    unittest.main()
