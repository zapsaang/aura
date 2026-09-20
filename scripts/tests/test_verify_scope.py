from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.manifest import manifest_bytes
from evidence.model import EvidenceError

SCRIPTS = Path(__file__).resolve().parents[1]


def _load_module(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


verify_scope = _load_module("verify_scope", "verify-scope.py")

HEAD = "1" * 40
BASE = "0" * 40


class ScopeAllowlistTests(unittest.TestCase):
    def _run_scope(self, changed: bytes, out: Path) -> int:
        """Run verify-scope main() with _git mocked to report `changed` paths."""
        root = out.parent
        preflight = root / "preflight"
        preflight.mkdir(exist_ok=True)
        source = {
            "device": "d",
            "head_tree": "t",
            "imported_baseline_commit": BASE,
            "inode": "i",
            "schema_version": 1,
            "source_root": "s",
        }
        (preflight / "source-root.json").write_bytes(
            json.dumps(source, sort_keys=True, separators=(",", ":")).encode() + b"\n"
        )
        (preflight / "receipt.json").write_bytes(b"{}\n")
        (preflight / "SHA256SUMS").write_bytes(
            manifest_bytes(preflight, frozenset({"SHA256SUMS", "receipt.json"}))
        )
        plan = root / "plan.md"
        plan.write_bytes(b"plan\n")

        def fake_git(arguments: list[str]) -> bytes:
            if arguments[0] == "rev-parse":
                return (HEAD + "\n").encode()
            if arguments[0] == "merge-base":
                return b""
            if arguments[0] == "diff":
                return changed
            raise AssertionError(arguments)

        argv = [
            "verify-scope.py",
            "--plan",
            str(plan),
            "--preflight",
            str(preflight),
            "--base-from-preflight",
            "--head",
            HEAD,
            "--out",
            str(out),
        ]
        with mock.patch.object(verify_scope, "_git", side_effect=fake_git):
            with mock.patch.object(sys, "argv", argv):
                return verify_scope.main()

    def test_cargo_config_toml_is_planned_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "scope.json"
            self.assertEqual(self._run_scope(b".cargo/config.toml\x00", out), 0)
            receipt = json.loads(out.read_bytes())
            self.assertEqual(receipt["status"], "approved")
            self.assertIn(".cargo/config.toml", receipt["changed_paths"])

    def test_gitattributes_and_cross_toml_are_planned_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "scope.json"
            self.assertEqual(
                self._run_scope(b".gitattributes\x00Cross.toml\x00", out), 0
            )
            receipt = json.loads(out.read_bytes())
            self.assertEqual(
                receipt["changed_paths"], [".gitattributes", "Cross.toml"]
            )

    def test_unrelated_hidden_root_still_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "scope.json"
            with self.assertRaises(EvidenceError) as raised:
                self._run_scope(b".vscode/settings.json\x00", out)
            self.assertIn("out-of-scope changed path", str(raised.exception))
            self.assertFalse(out.exists())


if __name__ == "__main__":
    unittest.main()
