from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

USES_LINE = re.compile(r"^\s*uses:\s+(?P<value>\S+)(?:\s+#\s*(?P<comment>\S.*))?$")
PINNED_EXTERNAL = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$")
WORKFLOW_FILES = (
    Path(".github/workflows/ci.yml"),
    Path(".github/workflows/final-f3-macos.yml"),
    Path(".github/workflows/release.yml"),
    Path(".github/actions/setup-rust/action.yml"),
)

PUBLISH_DOWNLOADS = {
    "aura-x86_64-unknown-linux-gnu": "artifacts/aura-x86_64-unknown-linux-gnu",
    "aura-aarch64-unknown-linux-gnu": "artifacts/aura-aarch64-unknown-linux-gnu",
    "aura-x86_64-apple-darwin": "artifacts/aura-x86_64-apple-darwin",
    "aura-aarch64-apple-darwin": "artifacts/aura-aarch64-apple-darwin",
    "homebrew-formula": "dist/homebrew",
    "homebrew-render-tuple": ".omo/evidence/design-compliance-remediation/lane/ubuntu-default/tuples/homebrew-render",
    "homebrew-audit-tuple": ".omo/evidence/design-compliance-remediation/lane/macos-default/tuples/homebrew-audit",
}
DOWNLOAD_STEP = re.compile(
    r"^      - name: .+\n"
    r"        uses: actions/download-artifact@[^\n]+\n"
    r"        with:\n"
    r"          name: (?P<name>[^\n]+)\n"
    r"          path: (?P<path>[^\n]+)$",
    re.MULTILINE,
)


def _workflow_job(workflow: str, job: str) -> str:
    marker = f"\n  {job}:\n"
    start = workflow.find(marker)
    if start < 0:
        return ""
    body = workflow[start + len(marker) :]
    next_job = re.search(r"^  [a-z0-9-]+:\n", body, re.MULTILINE)
    return body if next_job is None else body[: next_job.start()]


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

    def test_publish_jobs_exist_without_lanes(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")

        for job in ("publish-github-release", "publish-homebrew"):
            with self.subTest(job=job):
                body = _workflow_job(workflow, job)
                self.assertTrue(body, f"{job} job absent")
                self.assertNotIn("run-compliance-lane.py", body)
                self.assertNotIn("--job ", body)
                self.assertIn("runs-on: ubuntu-24.04", body)
                self.assertIn("fetch-depth: 0", body)
                self.assertEqual(
                    body.count("        run:"),
                    body.count("        shell: bash --noprofile --norc -eo pipefail {0}"),
                )

        github_release = _workflow_job(workflow, "publish-github-release")
        for prerequisite in (
            "release-linux-x86",
            "release-linux-arm64",
            "release-macos-arm64",
            "release-macos-x86",
            "homebrew-render",
            "homebrew-audit",
        ):
            with self.subTest(prerequisite=prerequisite):
                self.assertIn(f"      - {prerequisite}", github_release)
        self.assertIn("    permissions:\n      contents: write", github_release)
        self.assertIn("    needs:\n      - publish-github-release", _workflow_job(workflow, "publish-homebrew"))

    def test_publish_inputs_use_fixed_tuple_uploads_and_named_downloads(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        render = _workflow_job(workflow, "homebrew-render")
        audit = _workflow_job(workflow, "homebrew-audit")

        self.assertIn("name: homebrew-render-tuple", render)
        self.assertIn(PUBLISH_DOWNLOADS["homebrew-render-tuple"], render)
        self.assertIn("name: homebrew-audit-tuple", audit)
        self.assertIn(PUBLISH_DOWNLOADS["homebrew-audit-tuple"], audit)

        for job in ("publish-github-release", "publish-homebrew"):
            with self.subTest(job=job):
                body = _workflow_job(workflow, job)
                downloads = {
                    match.group("name"): match.group("path")
                    for match in DOWNLOAD_STEP.finditer(body)
                }
                self.assertEqual(downloads, PUBLISH_DOWNLOADS)
                self.assertEqual(body.count("actions/download-artifact@"), len(downloads))

    def test_github_release_publish_is_draft_guarded_and_content_addressed(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        body = _workflow_job(workflow, "publish-github-release")

        self.assertIn("concurrency:\n  group: release-${{ github.ref }}\n  cancel-in-progress: false", workflow)
        self.assertIn("--json isDraft", body)
        self.assertIn("Refusing to mutate published release", body)
        self.assertIn('gh release upload "$TAG" --clobber "${ASSETS[@]}"', body)
        self.assertIn('gh release create "$TAG" --draft', body)
        edit = body.index(
            'gh release edit "$TAG" --title "$TAG" --notes-file "$RELEASE_NOTES"'
        )
        upload = body.index('gh release upload "$TAG" --clobber "${ASSETS[@]}"')
        self.assertLess(edit, upload)
        self.assertNotIn("--draft=false", body)

        assets = re.search(r"ASSETS=\(\n(?P<values>(?:\s{12}[^\n]+\n){9})\s{10}\)", body)
        self.assertIsNotNone(assets, "release assets must be an explicit nine-item array")
        self.assertIn("python3 scripts/verify-release-asset.py", body)
        self.assertIn('--expected-manifest "$EXPECTED_ASSETS_MANIFEST"', body)
        self.assertIn('--assets-manifest-out "$SEAL_ROOT/assets.txt"', body)
        self.assertNotIn('cp "$ASSETS_MANIFEST" "$SEAL_ROOT/assets.txt"', body)
        self.assertIn("aura-publish-github-release-${{ steps.seal-digest.outputs.sha256 }}", body)

    def test_homebrew_publish_is_repository_parameterized_and_idempotent(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        body = _workflow_job(workflow, "publish-homebrew")

        self.assertIn("${{ vars.HOMEBREW_TAP_REPO || 'zapsaang/homebrew-tap' }}", body)
        self.assertNotIn("AURA_RELEASE_TOKEN", body)
        self.assertIn("HOMEBREW_TAP_TOKEN: ${{ secrets.HOMEBREW_TAP_TOKEN }}", body)
        self.assertNotIn("HOMEBREW_TAP_PUBLISH_TOKEN", body)
        self.assertNotIn("python3 scripts/verify-release-asset.py", body)
        self.assertIn("GIT_ASKPASS", body)
        self.assertIn("credential.helper=", body)
        self.assertIn("symbolic-ref refs/remotes/origin/HEAD", body)
        self.assertIn('--head "aura-${TAG}"', body)
        self.assertNotIn("--search", body)
        self.assertIn(
            "--json number,headRefName,headRepository,isCrossRepository",
            body,
        )
        self.assertIn(".headRepository.nameWithOwner == $repo", body)
        self.assertIn(".isCrossRepository == false", body)
        self.assertIn('git ls-remote origin "refs/heads/aura-${TAG}"', body)
        self.assertIn("--force-with-lease=refs/heads/aura-${TAG}:$REMOTE_SHA", body)
        self.assertIn('--tap-repository "$TAP_REPOSITORY"', body)
        self.assertIn('cp "$PR_BODY" "$SEAL_ROOT/pr-body.md"', body)
        self.assertIn('STATUS="no-op"', body)
        self.assertIn("aura-publish-homebrew-${{ steps.seal-digest.outputs.sha256 }}", body)

    def test_homebrew_pr_uniqueness_ignores_cross_repository_candidates(self) -> None:
        # Given the Homebrew publication job's open-PR query
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        body = _workflow_job(workflow, "publish-homebrew")

        # When the candidate-selection program reaches its uniqueness check
        selection = body.index('EXISTING_PR="$(jq')
        uniqueness = body.index("if length > 1 then", selection)
        filtering = body[selection:uniqueness]

        # Then only same-repository, non-cross-repository PRs remain
        self.assertIn("map(select(", filtering)
        self.assertIn(".headRepository.nameWithOwner == $repo", filtering)
        self.assertIn(".isCrossRepository == false", filtering)

    def test_github_release_notes_reference_configured_tap_repository(self) -> None:
        # Given the GitHub release publication job and its generated notes
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        body = _workflow_job(workflow, "publish-github-release")
        notes_start = body.index('RELEASE_NOTES="$RUNNER_TEMP/')
        notes_end = body.index("          EOF", notes_start)
        notes = body[notes_start:notes_end]

        # When a fork configures a different tap repository
        # Then the job propagates that setting and emits no upstream-only command
        self.assertIn("TAP_REPOSITORY: ${{ vars.HOMEBREW_TAP_REPO || 'zapsaang/homebrew-tap' }}", body)
        self.assertIn("https://github.com/${TAP_REPOSITORY}", notes)
        self.assertNotIn("brew install zapsaang/tap/aura", notes)

    def test_formula_source_repository_flows_through_compliance_lane(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
        render = _workflow_job(workflow, "homebrew-render")
        registry = json.loads(
            (ROOT / "qa" / "compliance-qa-registry.json").read_text(encoding="utf-8")
        )
        render_row = next(
            row for row in registry["commands"] if row["id"] == "homebrew-render"
        )

        self.assertIn("SOURCE_REPOSITORY: ${{ github.repository }}", render)
        self.assertIn('--binding "SOURCE_REPOSITORY=$SOURCE_REPOSITORY"', render)
        self.assertEqual(
            render_row["env"]["SOURCE_REPOSITORY"],
            "<github-repository>",
        )
        self.assertIn(
            '--source-repository "$SOURCE_REPOSITORY"',
            render_row["command"],
        )

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
