from __future__ import annotations

import importlib.util
import json
import os
import stat
import subprocess
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.model import EvidenceError

SCRIPTS = Path(__file__).resolve().parents[1]


def _load_module(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


verify_release_binary = _load_module("verify_release_binary", "verify-release-binary.py")
run_compliance_lane = _load_module("run_compliance_lane", "run-compliance-lane.py")


class MacosFileCheckTests(unittest.TestCase):
    def _check(self, report: str, arch: str) -> None:
        with mock.patch.object(verify_release_binary.shutil, "which", return_value="/usr/bin/tool"):
            with mock.patch.object(verify_release_binary, "_run", return_value=report):
                verify_release_binary._check_macos(["/tmp/aura-cli"], arch, None, None)

    def test_accepts_darwin_order_arm64(self) -> None:
        self._check("Mach-O 64-bit executable arm64\n", "aarch64")

    def test_accepts_darwin_order_x86_64(self) -> None:
        self._check("Mach-O 64-bit executable x86_64\n", "x86_64")

    def test_accepts_gnu_order_arm64(self) -> None:
        self._check("Mach-O 64-bit arm64 executable, flags:<NOUNDEFS>\n", "aarch64")

    def test_accepts_gnu_order_x86_64(self) -> None:
        self._check("Mach-O 64-bit x86_64 executable, flags:<NOUNDEFS>\n", "x86_64")

    def test_rejects_wrong_arch_darwin_order(self) -> None:
        with self.assertRaises(verify_release_binary.VerifyError):
            self._check("Mach-O 64-bit executable arm64\n", "x86_64")

    def test_rejects_wrong_arch_gnu_order(self) -> None:
        with self.assertRaises(verify_release_binary.VerifyError):
            self._check("Mach-O 64-bit x86_64 executable, flags:<NOUNDEFS>\n", "aarch64")


class CrossValueTests(unittest.TestCase):
    def _run_result(self, returncode: int, stdout: bytes, stderr: bytes = b""):
        return subprocess.CompletedProcess(
            ["cross", "--version"], returncode, stdout, stderr
        )

    def test_passthrough_when_configured(self) -> None:
        self.assertEqual(run_compliance_lane._cross_value("cross 1.2.3"), "cross 1.2.3")
        self.assertIsNone(run_compliance_lane._cross_value(None))

    def test_accepts_multiline_stdout_with_stderr(self) -> None:
        result = self._run_result(
            0,
            b"cross 0.2.5\n[cross] banner\nhost cargo 1.85.0\n",
            b"syncing rustup toolchain...\n",
        )
        with mock.patch.object(run_compliance_lane.subprocess, "run", return_value=result):
            self.assertEqual(run_compliance_lane._cross_value("auto"), "cross 0.2.5")

    def test_rejects_nonzero_exit(self) -> None:
        result = self._run_result(1, b"cross 0.2.5\n", b"boom\n")
        with mock.patch.object(run_compliance_lane.subprocess, "run", return_value=result):
            with self.assertRaises(EvidenceError):
                run_compliance_lane._cross_value("auto")

    def test_rejects_malformed_first_line(self) -> None:
        result = self._run_result(0, b"cross version 0.2.5\n")
        with mock.patch.object(run_compliance_lane.subprocess, "run", return_value=result):
            with self.assertRaises(EvidenceError):
                run_compliance_lane._cross_value("auto")

    def test_rejects_empty_stdout(self) -> None:
        result = self._run_result(0, b"")
        with mock.patch.object(run_compliance_lane.subprocess, "run", return_value=result):
            with self.assertRaises(EvidenceError):
                run_compliance_lane._cross_value("auto")


class LaneTemporaryTests(unittest.TestCase):
    def _argv(self, root: Path) -> list[str]:
        return [
            "run-compliance-lane.py",
            "--job", "ubuntu-default",
            "--lane", os.fspath(root / "lane"),
            "--archive", os.fspath(root / "lane.tar.gz"),
            "--registry", os.fspath(root / "registry.json"),
            "--run-id", "1",
            "--run-attempt", "1",
            "--verified-commit", "0" * 40,
            "--runner", "test",
            "--target", "x86_64-linux",
        ]

    def _run_main(self, root: Path, rows: tuple = (), returncode: int = 0) -> dict:
        captured: dict = {}
        real_create_environment = run_compliance_lane.create_environment

        def spy(path, home, search_path, temporary):
            result = real_create_environment(path, home, search_path, temporary)
            captured["temporary"] = temporary
            captured["mode"] = stat.S_IMODE(temporary.stat(follow_symlinks=False).st_mode)
            captured["tmpdir_value"] = json.loads(path.read_bytes())["TMPDIR"]
            return result

        completed = subprocess.CompletedProcess(["row"], returncode, b"", b"")
        with mock.patch.object(sys, "argv", self._argv(root)), \
            mock.patch.object(run_compliance_lane, "require_checkout_identity"), \
            mock.patch.object(run_compliance_lane, "load_registry", return_value=object()), \
            mock.patch.object(run_compliance_lane, "create_environment", side_effect=spy), \
            mock.patch.object(run_compliance_lane, "_link_tool_home"), \
            mock.patch.object(run_compliance_lane, "_commit_subjects", return_value={}), \
            mock.patch.object(run_compliance_lane, "expected_lane_rows", return_value=rows), \
            mock.patch.object(run_compliance_lane.subprocess, "run", return_value=completed), \
            mock.patch.object(run_compliance_lane, "seal_lane"), \
            mock.patch.object(run_compliance_lane, "create_deterministic_archive", return_value="0" * 64):
            try:
                captured["exit"] = run_compliance_lane.main()
            except EvidenceError as error:
                captured["error"] = error
        captured["control"] = root / ".ubuntu-default-control"
        captured["environment"] = root / "lane" / "environment.json"
        return captured

    def test_tmp_is_short_0700_and_recorded(self) -> None:
        with tempfile.TemporaryDirectory() as anchor:
            captured = self._run_main(Path(anchor))
            self.assertEqual(captured["exit"], 0)
            temporary = captured["temporary"]
            self.assertLess(len(os.fspath(temporary)), 60)
            self.assertEqual(captured["mode"], 0o700)
            self.assertEqual(captured["tmpdir_value"], os.fspath(temporary.resolve()))
            self.assertTrue(captured["environment"].exists())

    def test_cleanup_on_success(self) -> None:
        with tempfile.TemporaryDirectory() as anchor:
            captured = self._run_main(Path(anchor))
            self.assertFalse(captured["temporary"].exists())
            self.assertFalse(captured["temporary"].parent.exists())
            self.assertFalse(captured["control"].exists())

    def test_cleanup_on_row_failure(self) -> None:
        row = types.SimpleNamespace(id="t99-99", env={})
        with tempfile.TemporaryDirectory() as anchor:
            captured = self._run_main(Path(anchor), rows=(row,), returncode=1)
            self.assertIsInstance(captured["error"], EvidenceError)
            self.assertIn("t99-99", str(captured["error"]))
            self.assertFalse(captured["temporary"].exists())
            self.assertFalse(captured["temporary"].parent.exists())
            self.assertFalse(captured["control"].exists())


if __name__ == "__main__":
    unittest.main()
