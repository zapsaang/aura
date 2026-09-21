from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Final


SCRIPT: Final = Path(__file__).resolve().parents[1] / "render-publish-pr-body.py"
TAG: Final = "v1.2.3"
COMMIT: Final = "a" * 40
FORMULA_SHA256: Final = "f" * 64
RELEASE_URL: Final = f"https://github.com/zapsaang/aura/releases/tag/{TAG}"
AUDIT_LOG_URL: Final = "https://github.com/zapsaang/aura/actions/runs/123456/job/789"
ASSETS: Final = (
    ("aura-x86_64-unknown-linux-gnu.tar.gz", "1" * 64),
    ("aura-aarch64-unknown-linux-gnu.tar.gz", "2" * 64),
    ("aura-x86_64-apple-darwin.tar.gz", "3" * 64),
    ("aura-aarch64-apple-darwin.tar.gz", "4" * 64),
)


class RenderPublishPrBodyTests(unittest.TestCase):
    def _run(
        self,
        rows: tuple[str, ...],
        replacements: dict[str, str] | None = None,
    ) -> tuple[subprocess.CompletedProcess[str], bytes | None]:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            asset_list = root / "assets.txt"
            asset_list.write_text("\n".join(rows) + "\n", encoding="ascii")
            output = root / "pr-body.md"
            values = {
                "--tag": TAG,
                "--verified-commit": COMMIT,
                "--formula-sha256": FORMULA_SHA256,
                "--asset-sha-list": str(asset_list),
                "--release-url": RELEASE_URL,
                "--audit-log-url": AUDIT_LOG_URL,
                "--tap-repository": "zapsaang/homebrew-tap",
                "--out": str(output),
            }
            values.update(replacements or {})
            arguments = [sys.executable, "-B", str(SCRIPT)]
            for name, value in values.items():
                arguments.extend((name, value))

            result = subprocess.run(
                arguments,
                check=False,
                capture_output=True,
                text=True,
            )
            rendered = output.read_bytes() if output.exists() else None
            return result, rendered

    def test_renders_four_target_rows_with_expected_urls_and_digests(self) -> None:
        # Given: all four release asset digests in noncanonical input order.
        rows = tuple(f"{name} {digest}" for name, digest in reversed(ASSETS))

        # When: the PR body is rendered.
        result, raw = self._run(rows)

        # Then: every target has one complete canonical table row.
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")
        self.assertIsNotNone(raw)
        markdown = raw.decode("utf-8")
        self.assertIn(f"| tag | `{TAG}` |", markdown)
        self.assertIn(f"| verified commit | `{COMMIT}` |", markdown)
        self.assertIn(f"| formula SHA-256 | `{FORMULA_SHA256}` |", markdown)
        for name, digest in ASSETS:
            target = name.removeprefix("aura-").removesuffix(".tar.gz")
            asset_url = f"https://github.com/zapsaang/aura/releases/download/{TAG}/{name}"
            self.assertIn(
                f"| `{target}` | [{name}]({asset_url}) | `{digest}` |",
                markdown,
            )

    def test_renders_ordered_publish_checklist(self) -> None:
        # Given: a valid four-asset inventory.
        rows = tuple(f"{name} {digest}" for name, digest in ASSETS)

        # When: the PR body is rendered.
        result, raw = self._run(rows)

        # Then: the four draft-aware operations appear once and in safe order.
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIsNotNone(raw)
        markdown = raw.decode("utf-8")
        self.assertEqual(markdown.count("- [ ]"), 4)
        positions = (
            markdown.index(f"gh release view {TAG} --repo zapsaang/aura --json assets"),
            markdown.index("Merge this tap PR"),
            markdown.index(f"gh release edit {TAG} --repo zapsaang/aura --draft=false"),
            markdown.index("brew tap zapsaang/tap && brew install aura"),
        )
        self.assertEqual(positions, tuple(sorted(positions)))

    def test_scopes_manual_commands_to_source_and_configured_tap_repositories(self) -> None:
        # Given a fork source release and a separately configured fork tap.
        rows = tuple(f"{name} {digest}" for name, digest in ASSETS)
        replacements = {
            "--release-url": f"https://github.com/fork-owner/aura-fork/releases/tag/{TAG}",
            "--audit-log-url": "https://github.com/fork-owner/aura-fork/actions/runs/123456/job/789",
            "--tap-repository": "tap-owner/homebrew-dry-run",
        }

        # When the PR body is rendered.
        result, raw = self._run(rows, replacements)

        # Then source commands cannot resolve against another checkout and brew uses the configured tap.
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIsNotNone(raw)
        markdown = raw.decode("utf-8")
        self.assertIn(
            f"gh release view {TAG} --repo fork-owner/aura-fork --json assets",
            markdown,
        )
        self.assertIn(
            f"gh release edit {TAG} --repo fork-owner/aura-fork --draft=false",
            markdown,
        )
        self.assertIn("brew tap tap-owner/dry-run && brew install aura", markdown)

    def test_renders_complete_release_audit_and_workflow_links(self) -> None:
        # Given: release and job-level audit URLs from one workflow run.
        rows = tuple(f"{name} {digest}" for name, digest in ASSETS)

        # When: the PR body is rendered.
        result, raw = self._run(rows)

        # Then: release, audit, and run-level workflow links are complete.
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIsNotNone(raw)
        markdown = raw.decode("utf-8")
        self.assertIn(f"[Draft release]({RELEASE_URL})", markdown)
        self.assertIn(f"[Homebrew audit log]({AUDIT_LOG_URL})", markdown)
        self.assertIn(
            "[release.yml workflow run](https://github.com/zapsaang/aura/actions/runs/123456)",
            markdown,
        )

    def test_output_is_deterministic_and_has_no_shell_variables(self) -> None:
        # Given: equivalent inventories in opposite orders.
        canonical = tuple(f"{name} {digest}" for name, digest in ASSETS)
        reversed_rows = tuple(reversed(canonical))

        # When: both inventories are rendered independently.
        first_result, first = self._run(canonical)
        second_result, second = self._run(reversed_rows)

        # Then: output bytes match and contain no unresolved shell variables.
        self.assertEqual(first_result.returncode, 0, first_result.stderr)
        self.assertEqual(second_result.returncode, 0, second_result.stderr)
        self.assertEqual(first, second)
        self.assertIsNotNone(first)
        markdown = first.decode("utf-8")
        self.assertNotRegex(markdown, r"\$\{?[A-Z][A-Z0-9_]*\}?")

    def test_rejects_invalid_asset_rows(self) -> None:
        # Given: inventories containing one malformed digest line.
        valid = tuple(f"{name} {digest}" for name, digest in ASSETS)
        cases = (
            (*valid[:3], f"{ASSETS[3][0]} short"),
            (*valid[:3], f"unexpected.tar.gz {ASSETS[3][1]}"),
            (*valid[:3], f"{ASSETS[3][0]} {ASSETS[3][1]} trailing"),
        )

        # When/Then: each malformed inventory is rejected without an output file.
        for rows in cases:
            with self.subTest(rows=rows):
                result, rendered = self._run(rows)
                self.assertEqual(result.returncode, 1)
                self.assertIn("render-publish-pr-body:", result.stderr)
                self.assertIsNone(rendered)

    def test_rejects_duplicate_asset_rows(self) -> None:
        # Given: four well-formed lines containing a duplicate asset name.
        rows = tuple(
            f"{name} {digest}"
            for name, digest in (*ASSETS[:3], ASSETS[0])
        )

        # When: the inventory is rendered.
        result, rendered = self._run(rows)

        # Then: duplicate identity is rejected before writing output.
        self.assertEqual(result.returncode, 1)
        self.assertIn("duplicate asset", result.stderr)
        self.assertIsNone(rendered)

    def test_rejects_missing_asset_rows(self) -> None:
        # Given: only three otherwise valid asset digest lines.
        rows = tuple(f"{name} {digest}" for name, digest in ASSETS[:3])

        # When: the inventory is rendered.
        result, rendered = self._run(rows)

        # Then: the incomplete inventory is rejected without output.
        self.assertEqual(result.returncode, 1)
        self.assertIn("exactly 4", result.stderr)
        self.assertIsNone(rendered)

    def test_rejects_invalid_scalar_inputs(self) -> None:
        # Given: each required scalar input malformed in isolation.
        rows = tuple(f"{name} {digest}" for name, digest in ASSETS)
        cases = (
            {"--tag": "1.2.3"},
            {"--verified-commit": "a" * 39},
            {"--formula-sha256": "F" * 64},
            {"--release-url": f"http://github.com/zapsaang/aura/releases/tag/{TAG}"},
            {"--audit-log-url": "https://github.com/zapsaang/aura/actions/runs/latest"},
            {"--tap-repository": "zapsaang/tap"},
        )

        # When/Then: each malformed boundary value is rejected.
        for replacements in cases:
            with self.subTest(replacements=replacements):
                result, rendered = self._run(rows, replacements)
                self.assertEqual(result.returncode, 1)
                self.assertIn("render-publish-pr-body:", result.stderr)
                self.assertIsNone(rendered)


if __name__ == "__main__":
    unittest.main()
