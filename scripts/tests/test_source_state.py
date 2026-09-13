from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.model import EvidenceError
from evidence.source_state import SOURCE_STATE_COMMANDS, capture_source_state, read_source_state

SCRIPTS = Path(__file__).resolve().parents[1]


def _load_script(name: str):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), SCRIPTS / name)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


fidelity = _load_script("verify-source-fidelity.py")


def _git(root: Path, *arguments: str) -> None:
    subprocess.run(
        ["git", *arguments],
        cwd=root,
        env={
            **os.environ,
            "GIT_AUTHOR_NAME": "test",
            "GIT_AUTHOR_EMAIL": "test@example.com",
            "GIT_COMMITTER_NAME": "test",
            "GIT_COMMITTER_EMAIL": "test@example.com",
        },
        check=True,
        capture_output=True,
    )


class SourceStateTests(unittest.TestCase):
    def setUp(self) -> None:
        self._temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self._temporary.cleanup)
        self.base = Path(self._temporary.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        _git(self.repo, "init", "-q", "-b", "main")
        (self.repo / "tracked.txt").write_text("one\n", encoding="utf-8")
        _git(self.repo, "add", "tracked.txt")
        _git(self.repo, "commit", "-q", "-m", "first")

    def test_capture_ignores_conflicting_home_global_excludes(self) -> None:
        (self.repo / ".gitignore").write_text("repo-ignored.txt\n", encoding="utf-8")
        _git(self.repo, "add", ".gitignore")
        _git(self.repo, "commit", "-q", "-m", "ignore")
        (self.repo / ".git" / "info" / "exclude").write_text("info-ignored.txt\n", encoding="utf-8")
        (self.repo / "repo-ignored.txt").write_text("repo\n", encoding="utf-8")
        (self.repo / "info-ignored.txt").write_text("info\n", encoding="utf-8")
        (self.repo / "hidden-one.txt").write_text("one\n", encoding="utf-8")
        (self.repo / "hidden-two.txt").write_text("two\n", encoding="utf-8")
        captures = []
        for index, ignored in enumerate(("hidden-one.txt", "hidden-two.txt"), start=1):
            home = self.base / f"home-{index}"
            home.mkdir()
            excludes = home / "global-ignore"
            excludes.write_text(f"{ignored}\n", encoding="utf-8")
            (home / ".gitconfig").write_text(
                f"[core]\n\texcludesFile = {excludes}\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"HOME": os.fspath(home)}, clear=False):
                captures.append(capture_source_state(self.repo))

        self.assertEqual(captures[0], captures[1])
        status = dict(captures[0])["status.bin"]
        self.assertIn(b"hidden-one.txt\x00", status)
        self.assertIn(b"hidden-two.txt\x00", status)
        self.assertNotIn(b"repo-ignored.txt", status)
        self.assertNotIn(b"info-ignored.txt", status)

    def test_capture_sets_git_master_for_every_git_subprocess(self) -> None:
        completed = subprocess.CompletedProcess(args=[], returncode=0, stdout=b"", stderr=b"")
        with patch("evidence.source_state.subprocess.run", return_value=completed) as run:
            capture_source_state(self.repo)

        self.assertEqual(run.call_count, len(SOURCE_STATE_COMMANDS))
        for call in run.call_args_list:
            self.assertEqual(call.kwargs["env"]["GIT_MASTER"], "1")

    def test_transport_directory_requires_exact_regular_file_closure(self) -> None:
        state = self.base / "state"
        state.mkdir()
        for name, _arguments in SOURCE_STATE_COMMANDS:
            (state / name).write_bytes(name.encode("ascii"))
        (state / "unknown.bin").write_bytes(b"unknown")
        with self.assertRaises(EvidenceError) as unknown:
            read_source_state(state)
        self.assertIn("unknown=['unknown.bin']", str(unknown.exception))
        (state / "unknown.bin").unlink()
        (state / "status.bin").unlink()
        (state / "status.bin").symlink_to(state / "tracked.patch")
        with self.assertRaises(EvidenceError) as symlink:
            read_source_state(state)
        self.assertIn("not a regular file", str(symlink.exception))

    def test_verifier_rejects_one_byte_drift_in_each_source_state_artifact(self) -> None:
        (self.repo / "tracked.txt").write_text("two\n", encoding="utf-8")
        (self.repo / "untracked.txt").write_text("new\n", encoding="utf-8")
        captured = capture_source_state(self.repo)
        for drift_name, _raw in captured:
            with self.subTest(artifact=drift_name):
                expected = self.base / f"expected-{drift_name}"
                expected.mkdir()
                for name, raw in captured:
                    written = raw + b"x" if name == drift_name else raw
                    (expected / name).write_bytes(written)
                with self.assertRaises(EvidenceError) as caught:
                    fidelity.require_source_state(self.repo, expected)
                self.assertIn(drift_name, str(caught.exception))


if __name__ == "__main__":
    unittest.main()
