from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

SCRIPTS = Path(__file__).resolve().parents[1]
TOKEN = "release-token-that-must-stay-secret"
TAG = "v1.2.3"
REPO = "example/aura"
TARGETS = (
    "aura-aarch64-apple-darwin.tar.gz",
    "aura-x86_64-apple-darwin.tar.gz",
    "aura-aarch64-unknown-linux-gnu.tar.gz",
    "aura-x86_64-unknown-linux-gnu.tar.gz",
)
ASSET_NAMES = tuple(sorted(("aura.rb", *TARGETS, *(f"{target}.sha256" for target in TARGETS))))


def _load_module(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


verify_release_asset = _load_module(
    "verify_release_asset", "verify-release-asset.py"
)


class VerifyReleaseAssetTests(unittest.TestCase):
    def _formula(self, *, tag: str = TAG, targets: tuple[str, ...] = TARGETS) -> str:
        return "\n".join(
            f'url "https://github.com/{REPO}/releases/download/{tag}/{target}"\n'
            f'  sha256 "{index:064x}"'
            for index, target in enumerate(targets, start=1)
        )

    def _assets(self) -> list[dict[str, str]]:
        matched = [
            {"name": target, "digest": f"sha256:{index:064x}"}
            for index, target in enumerate(TARGETS, start=1)
        ]
        extras = [
            {"name": "aura.rb", "digest": "sha256:" + "a" * 64},
            *(
                {
                    "name": f"{target}.sha256",
                    "digest": "sha256:" + f"{index + 10:064x}",
                }
                for index, target in enumerate(TARGETS)
            ),
        ]
        return matched + extras

    def _expected_manifest(self) -> str:
        by_name = {
            asset["name"]: asset["digest"].removeprefix("sha256:")
            for asset in self._assets()
        }
        return "".join(f"{name} {by_name[name]}\n" for name in ASSET_NAMES)

    def _run(
        self,
        formula: str,
        *,
        assets: list[dict[str, str]] | None = None,
        error: OSError | None = None,
        token: str = TOKEN,
    ) -> tuple[int, str, str, mock.Mock, bytes | None]:
        with tempfile.TemporaryDirectory() as raw:
            formula_path = Path(raw) / "aura.rb"
            formula_path.write_text(formula, encoding="utf-8")
            expected_manifest = Path(raw) / "expected-assets.txt"
            expected_manifest.write_text(self._expected_manifest(), encoding="ascii")
            assets_manifest = Path(raw) / "remote-assets.txt"
            argv = [
                "verify-release-asset.py",
                "--formula",
                os.fspath(formula_path),
                "--tag",
                TAG,
                "--repo",
                REPO,
                "--expected-manifest",
                os.fspath(expected_manifest),
                "--assets-manifest-out",
                os.fspath(assets_manifest),
                "--timeout",
                "2.5",
            ]
            response = io.BytesIO(
                json.dumps({"assets": self._assets() if assets is None else assets}).encode("utf-8")
            )
            stdout = io.StringIO()
            stderr = io.StringIO()
            with (
                mock.patch.object(sys, "argv", argv),
                mock.patch.dict(os.environ, {"AURA_RELEASE_TOKEN": token}),
                mock.patch.object(
                    verify_release_asset.urllib.request,
                    "urlopen",
                    side_effect=error,
                    return_value=response,
                ) as urlopen,
                contextlib.redirect_stdout(stdout),
                contextlib.redirect_stderr(stderr),
            ):
                status = verify_release_asset.main()
            manifest = assets_manifest.read_bytes() if assets_manifest.exists() else None
        return status, stdout.getvalue(), stderr.getvalue(), urlopen, manifest

    def assert_safe_failure(
        self,
        result: tuple[int, str, str, mock.Mock, bytes | None],
    ) -> None:
        status, stdout, stderr, _urlopen, manifest = result
        self.assertEqual(status, 1)
        self.assertEqual(stdout, "")
        self.assertTrue(stderr.startswith("verify-release-asset: "))
        self.assertNotIn(TOKEN, stderr)
        self.assertIsNone(manifest)

    def test_succeeds_when_formula_and_nine_assets_match(self) -> None:
        # Given a canonical four-pair formula and its nine release assets
        formula = self._formula()

        # When the authenticated API verification runs
        status, stdout, stderr, urlopen, manifest = self._run(
            formula,
            assets=list(reversed(self._assets())),
        )

        # Then it emits the exact success record and uses the configured request
        self.assertEqual(status, 0)
        self.assertEqual(stdout, '{"checks":10,"status":"ok"}\n')
        self.assertEqual(stderr, "")
        self.assertEqual(manifest, self._expected_manifest().encode("ascii"))
        request = urlopen.call_args.args[0]
        self.assertEqual(
            request.full_url,
            f"https://api.github.com/repos/{REPO}/releases/tags/{TAG}",
        )
        self.assertEqual(request.get_header("Authorization"), f"Bearer {TOKEN}")
        self.assertEqual(urlopen.call_args.kwargs, {"timeout": 2.5})

    def test_fails_without_token_before_network_request(self) -> None:
        # Given no release API token
        with mock.patch.dict(os.environ, {}, clear=True):
            with tempfile.TemporaryDirectory() as raw:
                path = Path(raw) / "aura.rb"
                path.write_text(self._formula(), encoding="utf-8")
                expected_manifest = Path(raw) / "expected-assets.txt"
                expected_manifest.write_text(self._expected_manifest(), encoding="ascii")
                argv = [
                    "verify-release-asset.py",
                    "--formula",
                    os.fspath(path),
                    "--tag",
                    TAG,
                    "--expected-manifest",
                    os.fspath(expected_manifest),
                    "--assets-manifest-out",
                    os.fspath(Path(raw) / "remote-assets.txt"),
                ]
                stdout = io.StringIO()
                stderr = io.StringIO()

                # When verification starts
                with (
                    mock.patch.object(sys, "argv", argv),
                    mock.patch.object(
                        verify_release_asset.urllib.request, "urlopen"
                    ) as urlopen,
                    contextlib.redirect_stdout(stdout),
                    contextlib.redirect_stderr(stderr),
                ):
                    status = verify_release_asset.main()

        # Then it fails locally without making a request
        self.assertEqual(status, 1)
        self.assertEqual(stdout.getvalue(), "")
        self.assertIn("AURA_RELEASE_TOKEN", stderr.getvalue())
        urlopen.assert_not_called()

    def test_fails_safely_when_api_returns_http_error(self) -> None:
        # Given an authenticated API request that returns 404
        error = urllib.error.HTTPError(
            "https://api.github.com/release", 404, "Not Found", {}, None
        )

        # When verification runs, then it reports a secret-safe failure
        self.assert_safe_failure(self._run(self._formula(), error=error))

    def test_fails_safely_when_api_times_out(self) -> None:
        # Given an authenticated API request that times out
        error = TimeoutError("request timed out")

        # When verification runs, then it reports a secret-safe failure
        self.assert_safe_failure(self._run(self._formula(), error=error))

    def test_fails_without_network_when_formula_tag_drifts(self) -> None:
        # Given formula URLs bound to a different tag
        result = self._run(self._formula(tag="v1.2.4"))

        # When verification runs, then offline validation rejects the drift
        self.assert_safe_failure(result)
        result[3].assert_not_called()

    def test_fails_without_network_when_formula_has_wrong_cardinality(self) -> None:
        # Given only three formula URL/digest pairs
        result = self._run(self._formula(targets=TARGETS[:3]))

        # When verification runs, then offline validation rejects the formula
        self.assert_safe_failure(result)
        result[3].assert_not_called()

    def test_fails_when_release_asset_count_is_not_nine(self) -> None:
        # Given a release with only eight assets
        result = self._run(self._formula(), assets=self._assets()[:-1])

        # When verification runs, then the exact asset-count contract fails
        self.assert_safe_failure(result)

    def test_fails_when_ninth_release_asset_name_is_unexpected(self) -> None:
        # Given nine assets with one non-authoritative filename
        assets = self._assets()
        assets[-1] = {
            "name": "unexpected.sha256",
            "digest": assets[-1]["digest"],
        }

        # When verification runs, then exact remote closure fails
        self.assert_safe_failure(self._run(self._formula(), assets=assets))

    def test_fails_when_non_formula_asset_digest_mismatches_expected_manifest(self) -> None:
        # Given an exact filename set whose checksum-file digest drifted remotely
        assets = self._assets()
        assets[-1] = {
            "name": assets[-1]["name"],
            "digest": "sha256:" + "f" * 64,
        }

        # When verification runs, then all-nine digest comparison fails
        self.assert_safe_failure(self._run(self._formula(), assets=assets))

    def test_fails_when_formula_asset_is_missing(self) -> None:
        # Given nine assets where one formula target is absent
        assets = self._assets()
        assets[0] = {"name": "unexpected.tar.gz", "digest": "sha256:" + "f" * 64}

        # When verification runs, then the missing formula asset fails
        self.assert_safe_failure(self._run(self._formula(), assets=assets))

    def test_fails_when_formula_digest_mismatches_api(self) -> None:
        # Given nine assets where one API digest differs from the formula
        assets = self._assets()
        assets[0] = {"name": TARGETS[0], "digest": "sha256:" + "f" * 64}

        # When verification runs, then the digest comparison fails
        self.assert_safe_failure(self._run(self._formula(), assets=assets))


if __name__ == "__main__":
    unittest.main()
