from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence import dag
from evidence.aggregate import _require_producer_closure
from evidence.model import EvidenceError, canonical_json_bytes, sha256_bytes, sha256_file
from evidence.producer import _validate_source_artifacts

GIT_ENV = {
    **os.environ,
    "GIT_AUTHOR_NAME": "fixture",
    "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
    "GIT_COMMITTER_NAME": "fixture",
    "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
}


def _git(repo: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", "-C", os.fspath(repo), *arguments],
        env=GIT_ENV,
        check=True,
        capture_output=True,
    )
    return result.stdout.decode("utf-8").strip()


def _commit(repo: Path, message: str) -> str:
    _git(repo, "commit", "-q", "--allow-empty", "-m", message)
    return _git(repo, "rev-parse", "HEAD")


class DagFixture:
    def __init__(self, repo: Path) -> None:
        _git(repo, "init", "-q", "-b", "main")
        self.baseline = _commit(repo, "baseline")
        self.prerequisite = _commit(repo, "prerequisite")
        self.chain = [_commit(repo, f"task {index}") for index in range(1, 15)]
        self.t15 = _commit(repo, "task 15")
        _git(repo, "checkout", "-q", "-b", "side", self.chain[-1])
        self.t16 = _commit(repo, "task 16")
        _git(repo, "checkout", "-q", "main")
        _git(repo, "merge", "-q", "--no-ff", "-m", "merge", "side")
        self.merge = _git(repo, "rev-parse", "HEAD")
        self.final = _commit(repo, "final")

    def require(self, repo: Path) -> None:
        dag.require_exact_dag(
            dag.git_runner(repo),
            baseline=self.baseline,
            prerequisite=self.prerequisite,
            task_chain=self.chain,
            branch_tips=(self.t15, self.t16),
            final=self.final,
        )


class ExactDagTests(unittest.TestCase):
    def test_exact_dag_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            repo = Path(raw)
            DagFixture(repo).require(repo)

    def test_merge_with_single_parent_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            repo = Path(raw)
            _git(repo, "init", "-q", "-b", "main")
            baseline = _commit(repo, "baseline")
            prerequisite = _commit(repo, "prerequisite")
            chain = [_commit(repo, f"task {index}") for index in range(1, 15)]
            t15 = _commit(repo, "task 15")
            _git(repo, "checkout", "-q", "-b", "side", chain[-1])
            t16 = _commit(repo, "task 16")
            _git(repo, "checkout", "-q", "main")
            _commit(repo, "merge")
            final = _commit(repo, "final")
            with self.assertRaises(EvidenceError):
                dag.require_exact_dag(
                    dag.git_runner(repo),
                    baseline=baseline,
                    prerequisite=prerequisite,
                    task_chain=chain,
                    branch_tips=(t15, t16),
                    final=final,
                )

    def test_final_parent_not_merge_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            repo = Path(raw)
            fixture = DagFixture(repo)
            _git(repo, "checkout", "-q", "-b", "rogue", fixture.t16)
            rogue_final = _commit(repo, "final")
            with self.assertRaises(EvidenceError):
                dag.require_exact_dag(
                    dag.git_runner(repo),
                    baseline=fixture.baseline,
                    prerequisite=fixture.prerequisite,
                    task_chain=fixture.chain,
                    branch_tips=(fixture.t15, fixture.t16),
                    final=rogue_final,
                )

    def test_prerequisite_parent_not_baseline_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            repo = Path(raw)
            fixture = DagFixture(repo)
            with self.assertRaises(EvidenceError):
                dag.require_exact_dag(
                    dag.git_runner(repo),
                    baseline=fixture.chain[0],
                    prerequisite=fixture.prerequisite,
                    task_chain=fixture.chain,
                    branch_tips=(fixture.t15, fixture.t16),
                    final=fixture.final,
                )


class SourceProvenanceTests(unittest.TestCase):
    def _write_artifacts(self, root: Path, head_tree: str, plan_sha256: str) -> tuple[Path, Path]:
        matrix = root / "compliance-matrix.json"
        matrix.write_bytes(b"{}\n")
        artifacts = {
            "compliance_matrix_sha256": sha256_file(matrix),
            "head_tree": head_tree,
            "plan_sha256": plan_sha256,
            "porcelain": "",
            "schema_version": 1,
            "verified_commit": "a" * 40,
        }
        path = root / "source-artifacts.json"
        path.write_bytes(canonical_json_bytes(artifacts))
        return path, matrix

    def test_matching_provenance_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path, matrix = self._write_artifacts(Path(raw), "b" * 40, "c" * 64)
            digest = _validate_source_artifacts(
                path, matrix, "a" * 40, provenance=lambda commit: ("b" * 40, "c" * 64)
            )
            self.assertEqual(digest, sha256_bytes(path.read_bytes()))

    def test_head_tree_mismatch_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path, matrix = self._write_artifacts(Path(raw), "b" * 40, "c" * 64)
            with self.assertRaises(EvidenceError):
                _validate_source_artifacts(
                    path, matrix, "a" * 40, provenance=lambda commit: ("d" * 40, "c" * 64)
                )

    def test_plan_sha256_mismatch_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path, matrix = self._write_artifacts(Path(raw), "b" * 40, "c" * 64)
            with self.assertRaises(EvidenceError):
                _validate_source_artifacts(
                    path, matrix, "a" * 40, provenance=lambda commit: ("b" * 40, "e" * 64)
                )


class ProducerClosureTests(unittest.TestCase):
    def _stage(self, root: Path) -> tuple[Path, Path]:
        producer_input = root / "producer-input"
        archives = root / "producers"
        archives.mkdir()
        for name in ("preflight", "prerequisite", "source"):
            (producer_input / name).mkdir(parents=True)
            (archives / f"{name}.tar.gz").write_bytes(b"archive")
        return producer_input, archives

    def test_exact_closure_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            _require_producer_closure(*self._stage(Path(raw)))

    def test_extra_producer_directory_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            producer_input, archives = self._stage(Path(raw))
            (producer_input / "rogue").mkdir()
            with self.assertRaises(EvidenceError):
                _require_producer_closure(producer_input, archives)

    def test_extra_producer_archive_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            producer_input, archives = self._stage(Path(raw))
            (archives / "rogue.tar.gz").write_bytes(b"archive")
            with self.assertRaises(EvidenceError):
                _require_producer_closure(producer_input, archives)


if __name__ == "__main__":
    unittest.main()
