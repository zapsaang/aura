from __future__ import annotations

import copy
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.contract import load_f1_contract, validate_f1_contract
from evidence.model import EvidenceError, canonical_json_bytes, parse_json_bytes
from evidence.operations import (
    append_operation,
    merge_operations,
    validate_merged_operations,
)
from evidence.receipt import (
    AggregateIdentity,
    FinalIdentity,
    GateIdentity,
    HandoffIdentity,
    validate_aggregate_receipt,
    validate_final_receipt,
    validate_gate_receipt,
    validate_handoff_receipt,
    validate_lane_receipt,
)
from evidence.registry import load_registry, verify_registry_matches_plan

ROOT = Path(__file__).resolve().parents[2]
PLAN = ROOT / ".omo" / "plans" / "design-compliance-remediation.md"


class EvidenceContractTests(unittest.TestCase):
    def test_registry_exactly_matches_current_plan_and_partitions_every_id(self) -> None:
        registry = load_registry(ROOT / "qa" / "compliance-qa-registry.json")

        if PLAN.is_file():
            verify_registry_matches_plan(registry, PLAN)

        ids = [row.id for row in registry.rows]
        self.assertEqual(len(ids), 114)
        self.assertEqual(ids, sorted(ids))
        self.assertEqual(len(ids), len(set(ids)))
        self.assertNotIn("*", {row.task_owner for row in registry.rows})
        self.assertEqual({row.count_source for row in registry.rows}, {
            "constant:0", "rust-harness", "rust-harness-sum", "stdout-json:checks"
        })

    def test_f1_contract_has_exact_sorted_39_check_partition(self) -> None:
        registry = load_registry(ROOT / "qa" / "compliance-qa-registry.json")
        contract = load_f1_contract(ROOT / "qa" / "f1-compliance-contract.json")

        validate_f1_contract(contract, registry)

        expected = sorted(
            [f"T{number:02d}" for number in range(1, 18)]
            + [f"MN{number:02d}" for number in range(1, 8)]
            + [f"AUD-{number:03d}" for number in range(1, 16)]
        )
        self.assertEqual([check.id for check in contract.checks], expected)

    def test_json_rejects_duplicate_unknown_and_missing_keys(self) -> None:
        with self.assertRaises(EvidenceError):
            parse_json_bytes(b'{"a":1,"a":2}\n', frozenset({"a"}), "duplicate")
        with self.assertRaises(EvidenceError):
            parse_json_bytes(b'{"a":1,"b":2}\n', frozenset({"a"}), "unknown")
        with self.assertRaises(EvidenceError):
            parse_json_bytes(b"{}\n", frozenset({"a"}), "missing")
        self.assertEqual(canonical_json_bytes({"z": 1, "a": 2}), b'{"a":2,"z":1}\n')

    def test_aggregate_rejects_digest_commit_tag_plan_and_schema_drift(self) -> None:
        identity = AggregateIdentity(
            verified_commit="1" * 40,
            plan_sha256="2" * 64,
            tag="v1.2.3",
            aggregate_sha256="3" * 64,
        )
        payload = self._aggregate(identity)
        validate_aggregate_receipt(payload, identity)
        for field, replacement in (
            ("verified_commit", "4" * 40),
            ("plan_sha256", "5" * 64),
            ("tag", "v9.9.9"),
            ("aggregate_sha256", "6" * 64),
        ):
            drifted = copy.deepcopy(payload)
            drifted[field] = replacement
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                validate_aggregate_receipt(drifted, identity)
        unknown = copy.deepcopy(payload)
        unknown["extra"] = True
        with self.assertRaises(EvidenceError):
            validate_aggregate_receipt(unknown, identity)

    def test_handoff_and_final_receipts_reject_identity_or_command_drift(self) -> None:
        handoff_identity = HandoffIdentity("linux", "r", 1, "v1.2.3", "1" * 40, "2" * 64)
        handoff: dict[str, object] = {
            "aggregate_sha256": "2" * 64,
            "archive_path": "releases/aura-x86_64-unknown-linux-gnu.tar.gz",
            "archive_sha256": "3" * 64,
            "artifacts_manifest_sha256": "4" * 64,
            "manifest_sha256": "5" * 64,
            "platform": "linux",
            "run_attempt": 1,
            "run_id": "r",
            "schema_version": 1,
            "status": "approved",
            "tag": "v1.2.3",
            "tuple_sha256": "6" * 64,
            "verified_commit": "1" * 40,
        }
        validate_handoff_receipt(handoff, handoff_identity)
        bad_handoff = copy.deepcopy(handoff)
        bad_handoff["platform"] = "macos"
        with self.assertRaises(EvidenceError):
            validate_handoff_receipt(bad_handoff, handoff_identity)

        final_identity = FinalIdentity("v1.2.3", "1" * 40, "7" * 64, "2" * 64)
        final: dict[str, object] = {
            "aggregate_sha256": "2" * 64,
            "manifest_sha256": "8" * 64,
            "plan_sha256": "7" * 64,
            "schema_version": 1,
            "status": "approved",
            "tag": "v1.2.3",
            "verdict_receipts": {"F1": "9" * 64, "F2": "a" * 64, "F3": "b" * 64, "F4": "c" * 64},
            "verified_commit": "1" * 40,
        }
        validate_final_receipt(final, final_identity)
        bad_final = copy.deepcopy(final)
        verdict_receipts = bad_final["verdict_receipts"]
        self.assertIsInstance(verdict_receipts, dict)
        if isinstance(verdict_receipts, dict):
            del verdict_receipts["F4"]
        with self.assertRaises(EvidenceError):
            validate_final_receipt(bad_final, final_identity)

    def test_lane_and_gate_receipts_reject_contract_drift(self) -> None:
        lane: dict[str, object] = {
            "commands": [],
            "environment_sha256": "1" * 64,
            "features": [],
            "job": "home-manager-semantic",
            "manifest_sha256": "2" * 64,
            "release_archives": [],
            "run_attempt": 1,
            "run_id": "run",
            "runner": "ubuntu-24.04",
            "schema_version": 1,
            "status": "executed",
            "target": "x86_64-linux",
            "tools": {"cargo": None, "cross": None, "nix": "nix 2.0", "rustc": None},
            "verified_commit": "3" * 40,
        }
        validate_lane_receipt(lane, "home-manager-semantic", "3" * 40)
        for field, value in (("target", "x86_64-darwin"), ("status", "approved"), ("features", ["all"])):
            drifted = copy.deepcopy(lane)
            drifted[field] = value
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                validate_lane_receipt(drifted, "home-manager-semantic", "3" * 40)

        identity = GateIdentity("F2", "v1.2.3", "3" * 40, "4" * 64, "5" * 64)
        gate = self._gate(identity, [f"F2-0{number}" for number in range(1, 8)])
        validate_gate_receipt(gate, identity)
        failed = copy.deepcopy(gate)
        commands = failed["commands"]
        self.assertIsInstance(commands, list)
        if isinstance(commands, list) and commands and isinstance(commands[0], dict):
            commands[0]["exit_code"] = 1
        with self.assertRaises(EvidenceError):
            validate_gate_receipt(failed, identity)

    def test_gate_receipt_requires_exact_registry_command_closure(self) -> None:
        identity = GateIdentity("F2", "v1.2.3", "3" * 40, "4" * 64, "5" * 64)
        validate_gate_receipt(self._gate(identity, [f"F2-0{number}" for number in range(1, 8)]), identity)
        for label, ids in (
            ("empty", []),
            ("missing F2-07", [f"F2-0{number}" for number in range(1, 7)]),
            ("extra F2-99", [f"F2-0{number}" for number in range(1, 8)] + ["F2-99"]),
            ("unknown id", [f"F2-0{number}" for number in range(1, 7)] + ["t17-01"]),
            ("unsorted", ["F2-02", "F2-01", "F2-03", "F2-04", "F2-05", "F2-06", "F2-07"]),
        ):
            with self.subTest(label=label), self.assertRaises(EvidenceError):
                validate_gate_receipt(self._gate(identity, ids), identity)

    def test_gate_handoff_reference_uses_receipt_sha256_field(self) -> None:
        identity = GateIdentity("F3", "v1.2.3", "3" * 40, "4" * 64, "5" * 64)
        receipt = self._gate(identity, ["F3-01", "F3-02-linux", "F3-02-macos"])
        receipt["handoffs"] = [
            {"platform": "linux", "receipt_sha256": "8" * 64},
            {"platform": "macos", "receipt_sha256": "9" * 64},
        ]
        validate_gate_receipt(receipt, identity)
        wrong = copy.deepcopy(receipt)
        handoffs = wrong["handoffs"]
        self.assertIsInstance(handoffs, list)
        if isinstance(handoffs, list):
            wrong["handoffs"] = [
                {"platform": entry["platform"], "sha256": entry["receipt_sha256"]}
                for entry in handoffs
                if isinstance(entry, dict)
            ]
        with self.assertRaises(EvidenceError):
            validate_gate_receipt(wrong, identity)

    @staticmethod
    def _gate(identity: GateIdentity, command_ids: list[str]) -> dict[str, object]:
        return {
            "aggregate_sha256": identity.aggregate_sha256,
            "commands": [
                {"exit_code": 0, "id": identifier, "tests_executed": 0, "tuple_sha256": "6" * 64}
                for identifier in command_ids
            ],
            "evidence": [{"path": "report.md", "sha256": "7" * 64}],
            "gate": identity.gate,
            "handoffs": [],
            "plan_sha256": identity.plan_sha256,
            "schema_version": 1,
            "status": "approved",
            "tag": identity.tag,
            "verified_commit": identity.verified_commit,
        }

    @staticmethod
    def _aggregate(identity: AggregateIdentity) -> dict[str, object]:
        lane_jobs = (
            "home-manager-semantic", "macos-default", "release-linux-arm64",
            "release-linux-x86", "release-macos-arm64", "release-macos-x86",
            "ubuntu-default", "ubuntu-gpu", "ubuntu-msrv",
        )
        releases = (
            "releases/aura-aarch64-apple-darwin.tar.gz",
            "releases/aura-aarch64-unknown-linux-gnu.tar.gz",
            "releases/aura-x86_64-apple-darwin.tar.gz",
            "releases/aura-x86_64-unknown-linux-gnu.tar.gz",
            "releases/homebrew/aura.rb",
        )
        return {
            "aggregate_sha256": identity.aggregate_sha256,
            "lanes": [{"archive_sha256": "4" * 64, "job": job} for job in lane_jobs],
            "merges": [{"merge": "M1516", "receipt_sha256": "5" * 64}],
            "plan_sha256": identity.plan_sha256,
            "preflight_receipt_sha256": "6" * 64,
            "prerequisite_receipt_sha256": "7" * 64,
            "producer_archives": [
                {"archive_sha256": "8" * 64, "producer": name, "receipt_sha256": "9" * 64}
                for name in ("preflight", "prerequisite", "source")
            ],
            "releases": [{"path": path, "sha256": "a" * 64} for path in releases],
            "run_attempt": 1,
            "run_id": "run",
            "schema_version": 1,
            "source_receipt_sha256": "b" * 64,
            "tag": identity.tag,
            "tasks": [{"receipt_sha256": "c" * 64, "task": str(number)} for number in range(1, 18)],
            "verified_commit": identity.verified_commit,
        }


class MergedOperationsTests(unittest.TestCase):
    def test_merge_assigns_global_sequence_with_unprefixed_operations(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            first = base / "first.log"
            second = base / "second.log"
            append_operation(first, "preflight-check", None, "1" * 64)
            append_operation(first, "preflight-verify", "2" * 64, None)
            append_operation(second, "source-pack", None, None)
            output = base / "merged" / "evidence-operations.log"

            merge_operations([("preflight", first), ("source", second)], output)

            entries = validate_merged_operations(output)
            self.assertEqual(entries, [
                {"input_sha256": None, "operation": "preflight-check",
                 "output_sha256": "1" * 64, "sequence": 0},
                {"input_sha256": "2" * 64, "operation": "preflight-verify",
                 "output_sha256": None, "sequence": 1},
                {"input_sha256": None, "operation": "source-pack",
                 "output_sha256": None, "sequence": 2},
            ])
            for entry in entries:
                self.assertNotIn("local_sequence", entry)
                self.assertNotIn("source", entry)

    def test_validate_merged_operations_rejects_noncanonical_entries(self) -> None:
        base_entry = {
            "input_sha256": None,
            "operation": "op",
            "output_sha256": None,
            "sequence": 0,
        }
        cases = {
            "local_sequence retained": {**base_entry, "local_sequence": 0},
            "source field": {**base_entry, "source": "preflight"},
            "unknown key": {**base_entry, "extra": True},
            "noncontiguous sequence": {**base_entry, "sequence": 1},
            "empty operation": {**base_entry, "operation": ""},
            "bad digest": {**base_entry, "output_sha256": "zz"},
        }
        for label, entry in cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "merged.log"
                path.write_bytes(canonical_json_bytes(entry))
                with self.assertRaises(EvidenceError):
                    validate_merged_operations(path)


if __name__ == "__main__":
    unittest.main()
