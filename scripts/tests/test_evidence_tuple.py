from __future__ import annotations

import os
import sys
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.model import EvidenceError
from evidence.registry import CommandSpec, OutputRule, TestRule
from evidence.tuple import verify_tuple, write_tuple


class EvidenceTupleTests(unittest.TestCase):
    def test_tuple_is_canonical_and_counts_one_rust_summary(self) -> None:
        spec = self._rust_spec()
        stdout = b"test result: ok. 12 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 0.1s\n"
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "tuple"

            digest = write_tuple(path, spec, {}, 0, stdout, b"")
            result = verify_tuple(path, spec, {})

            self.assertEqual(result.tuple_sha256, digest)
            self.assertEqual(result.tests_executed, 12)
            self.assertEqual(sorted(child.name for child in path.iterdir()), [
                "SHA256SUMS", "command.bin", "cwd.bin", "env.json", "exit-code",
                "shell.bin", "stderr.bin", "stdout.bin", "tests-executed"
            ])

    def test_tuple_rejects_command_count_output_and_digest_drift(self) -> None:
        spec = self._json_spec()
        with tempfile.TemporaryDirectory() as raw:
            original = Path(raw) / "tuple"
            write_tuple(original, spec, {}, 0, b'{"checks":2,"status":"ok"}\n', b"")
            mutations = {
                "command": ("command.bin", b"false\n"),
                "count": ("tests-executed", b"3\n"),
                "output": ("stdout.bin", b'{"checks":3,"status":"ok"}\n'),
            }
            for label, (name, data) in mutations.items():
                with self.subTest(label=label):
                    path = Path(raw) / label
                    self._copy_tree(original, path)
                    (path / name).write_bytes(data)
                    with self.assertRaises(EvidenceError):
                        verify_tuple(path, spec, {})

            digest = Path(raw) / "digest"
            self._copy_tree(original, digest)
            manifest = (digest / "SHA256SUMS").read_text(encoding="ascii")
            (digest / "SHA256SUMS").write_text("0" + manifest[1:], encoding="ascii")
            with self.assertRaises(EvidenceError):
                verify_tuple(digest, spec, {})

    def test_rust_parser_rejects_missing_failed_and_multi_summaries(self) -> None:
        spec = self._rust_spec()
        cases = (
            b"",
            b"test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n",
            (
                b"test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                b"test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
            ),
        )
        for index, stdout in enumerate(cases):
            with self.subTest(index=index), tempfile.TemporaryDirectory() as raw:
                path = Path(raw) / "tuple"
                with self.assertRaises(EvidenceError):
                    write_tuple(path, spec, {}, 0, stdout, b"")

    def test_rust_parser_sums_identical_summaries_in_summed_mode(self) -> None:
        spec = self._rust_sum_spec()
        stdout = (
            b"test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
            b"test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        )
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "tuple"

            digest = write_tuple(path, spec, {}, 0, stdout, b"")
            result = verify_tuple(path, spec, {})

            self.assertEqual(result.tuple_sha256, digest)
            self.assertEqual(result.tests_executed, 10)

    def test_rust_parser_rejects_zero_passed_with_filtered_out(self) -> None:
        spec = self._rust_spec()
        stdout = b"test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out\n"
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "tuple"
            with self.assertRaises(EvidenceError):
                write_tuple(path, spec, {}, 0, stdout, b"")

    def test_row_contract_failures_include_bounded_captured_tails(self) -> None:
        stdout = b"stdout-outside-tail" + (b"s" * 500) + b"stdout-tail"
        stderr = b"stderr-outside-tail" + (b"e" * 500) + b"stderr-tail"
        base = self._json_spec()
        cases = (
            ("exit", replace(base, expected_exit=7), "tip-t17 exit mismatch:"),
            ("stderr", replace(base, stderr="empty"), "tip-t17 requires empty stderr"),
            ("stdout", base, "tip-t17 exact stdout mismatch"),
            (
                "count",
                replace(
                    base,
                    count_source="constant:0",
                    tests=TestRule("exact", 1),
                    stdout=OutputRule("unrestricted", None),
                ),
                "tip-t17 requires exactly 1 tests, got 0",
            ),
        )
        for label, spec, prefix in cases:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as raw:
                path = Path(raw) / "tuple"

                with self.assertRaises(EvidenceError) as caught:
                    write_tuple(path, spec, {}, 0, stdout, stderr)

                message = str(caught.exception)
                self.assertTrue(message.startswith(prefix))
                self.assertIn("captured stdout tail:", message)
                self.assertIn("stdout-tail", message)
                self.assertNotIn("stdout-outside-tail", message)
                self.assertIn("captured stderr tail:", message)
                self.assertIn("stderr-tail", message)
                self.assertNotIn("stderr-outside-tail", message)

    @staticmethod
    def _copy_tree(source: Path, destination: Path) -> None:
        destination.mkdir()
        for child in source.iterdir():
            (destination / child.name).write_bytes(child.read_bytes())

    @staticmethod
    def _rust_spec() -> CommandSpec:
        return CommandSpec(
            id="t17-03",
            command="cargo test",
            shell="/usr/bin/env bash --noprofile --norc",
            cwd=".",
            env={},
            execution_context="ubuntu-default",
            expected_exit=0,
            count_source="rust-harness",
            tests=TestRule("minimum", 12),
            stdout=OutputRule("unrestricted", None),
            stderr="tool",
            task_owner="17",
        )

    @staticmethod
    def _rust_sum_spec() -> CommandSpec:
        return CommandSpec(
            id="pre-01",
            command="cargo +stable test --workspace --locked",
            shell="/usr/bin/env bash --noprofile --norc",
            cwd=".",
            env={},
            execution_context="ubuntu-default",
            expected_exit=0,
            count_source="rust-harness-sum",
            tests=TestRule("minimum", 10),
            stdout=OutputRule("unrestricted", None),
            stderr="tool",
            task_owner="17",
        )

    @staticmethod
    def _json_spec() -> CommandSpec:
        return CommandSpec(
            id="tip-t17",
            command="python3 gate.py",
            shell="/usr/bin/env bash --noprofile --norc",
            cwd=".",
            env={},
            execution_context="ubuntu-msrv",
            expected_exit=0,
            count_source="stdout-json:checks",
            tests=TestRule("exact", 2),
            stdout=OutputRule("exact", '{"checks":2,"status":"ok"}\n'),
            stderr="tool",
            task_owner="17",
        )


if __name__ == "__main__":
    unittest.main()
