from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

USES_LINE = re.compile(r"^\s*uses:\s+(?P<value>\S+)(?:\s+#\s*(?P<comment>\S.*))?$")
PINNED_EXTERNAL = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$")
WORKFLOW_FILES = (
    Path(".github/workflows/ci.yml"),
    Path(".github/workflows/release.yml"),
    Path(".github/actions/setup-rust/action.yml"),
)


class WorkflowContractTests(unittest.TestCase):
    def test_release_workflow_produces_every_lane_without_assembling(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        jobs = (
            "ubuntu-msrv",
            "ubuntu-default",
            "ubuntu-gpu",
            "macos-default",
            "home-manager-semantic",
            "release-linux-x86",
            "release-linux-arm64",
            "release-macos-arm64",
            "release-macos-x86",
        )
        for job in jobs:
            with self.subTest(job=job):
                self.assertEqual(workflow.count(f"--job {job}"), 1)
                self.assertIn(f"aura-lane-{job}-${{{{ steps.lane-digest.outputs.sha256 }}}}", workflow)
        self.assertNotIn("assemble-compliance-evidence.py", workflow)
        for producer in ("preflight", "prerequisite", "source"):
            with self.subTest(producer=producer):
                self.assertIn(f"aura-producer-{producer}-${{{{ steps.producer-digest.outputs.sha256 }}}}", workflow)

    def test_ci_checks_contracts_and_archives_home_manager(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("python3 -B -m unittest discover -s scripts/tests -v", workflow)
        self.assertIn("generate-f1-compliance-contract.py", workflow)
        self.assertIn("--job home-manager-semantic", workflow)
        self.assertNotIn("assemble-compliance-evidence.py", workflow)

    def test_external_action_refs_are_pinned_to_full_commit_sha(self) -> None:
        for rel_path in WORKFLOW_FILES:
            text = (ROOT / rel_path).read_text(encoding="utf-8")
            for lineno, line in enumerate(text.splitlines(), start=1):
                match = USES_LINE.match(line)
                if match is None:
                    continue
                value = match.group("value")
                with self.subTest(file=str(rel_path), line=lineno, uses=value):
                    if value.startswith("./"):
                        continue
                    self.assertRegex(
                        value,
                        PINNED_EXTERNAL,
                        f"external action ref must be pinned to a full 40-hex commit SHA: {value}",
                    )
                    comment = match.group("comment")
                    self.assertIsNotNone(
                        comment,
                        f"pinned action ref must record the original ref in a trailing comment: {value}",
                    )
                    self.assertTrue(
                        comment.strip(),
                        f"original-ref comment must be non-empty: {value}",
                    )


if __name__ == "__main__":
    unittest.main()
    unittest.main()
