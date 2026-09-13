from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from .model import (
    canonical_json_bytes,
    decode_json_bytes,
    fail,
    require_exact_keys,
    require_list,
    require_safe_id,
    require_string,
    require_uint,
)

SHELL = "/usr/bin/env bash --noprofile --norc"
ROW = re.compile(r"^\| `([^`]+)` \| `(.+?)` \| `([^`]*)` \| ([^|]+?) \| (.+) \|$")
ROW_KEYS = frozenset({
    "command", "count_source", "cwd", "env", "execution_context", "expected_exit",
    "id", "shell", "stderr", "stdout", "task_owner", "tests",
})


@dataclass(frozen=True)
class TestRule:
    mode: str
    value: int


@dataclass(frozen=True)
class OutputRule:
    mode: str
    value: str | None


@dataclass(frozen=True)
class CommandSpec:
    id: str
    command: str
    shell: str
    cwd: str
    env: dict[str, str]
    execution_context: str
    expected_exit: int
    count_source: str
    tests: TestRule
    stdout: OutputRule
    stderr: str
    task_owner: str

    def as_json(self) -> dict[str, object]:
        return {
            "command": self.command,
            "count_source": self.count_source,
            "cwd": self.cwd,
            "env": self.env,
            "execution_context": self.execution_context,
            "expected_exit": self.expected_exit,
            "id": self.id,
            "shell": self.shell,
            "stderr": self.stderr,
            "stdout": {"mode": self.stdout.mode, "value": self.stdout.value},
            "task_owner": self.task_owner,
            "tests": {"mode": self.tests.mode, "value": self.tests.value},
        }


@dataclass(frozen=True)
class Registry:
    rows: tuple[CommandSpec, ...]

    def by_id(self) -> dict[str, CommandSpec]:
        return {row.id: row for row in self.rows}

    def as_json(self) -> dict[str, object]:
        return {"commands": [row.as_json() for row in self.rows], "schema_version": 1}


def build_registry(plan: Path) -> Registry:
    rows: list[CommandSpec] = []
    for line in plan.read_text(encoding="utf-8").splitlines():
        match = ROW.fullmatch(line)
        if match is None:
            continue
        identifier, command, env_text, context, rule = match.groups()
        if identifier == "ID":
            continue
        rows.append(_from_plan(identifier, command, env_text, context, rule))
    rows.sort(key=lambda row: row.id)
    identifiers = [row.id for row in rows]
    if not rows or len(identifiers) != len(set(identifiers)):
        fail("plan registry rows are empty or duplicated")
    return Registry(tuple(rows))


def load_registry(path: Path) -> Registry:
    root = decode_json_bytes(path.read_bytes(), "QA registry")
    if not isinstance(root, dict):
        fail("QA registry must be an object")
    require_exact_keys(root, frozenset({"commands", "schema_version"}), "QA registry")
    if root["schema_version"] != 1:
        fail("unsupported QA registry schema")
    rows = tuple(_parse_row(value, index) for index, value in enumerate(require_list(root["commands"], "commands")))
    identifiers = [row.id for row in rows]
    if identifiers != sorted(identifiers) or len(identifiers) != len(set(identifiers)):
        fail("registry IDs must be sorted and unique")
    return Registry(rows)


def verify_registry_matches_plan(registry: Registry, plan: Path) -> None:
    expected = build_registry(plan)
    if canonical_json_bytes(registry.as_json()) != canonical_json_bytes(expected.as_json()):
        fail("QA registry differs from the normative plan")


def _from_plan(identifier: str, command: str, env_text: str, context: str, rule: str) -> CommandSpec:
    env_value = json.loads(env_text)
    if not isinstance(env_value, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in env_value.items()):
        fail(f"invalid environment in plan row {identifier}")
    output = _output_rule(rule)
    tests = _test_rule(rule, output)
    count_source = _count_source(identifier, output, tests)
    return CommandSpec(
        id=identifier,
        command=command,
        shell=SHELL,
        cwd=".",
        env={key: env_value[key] for key in sorted(env_value)},
        execution_context=context,
        expected_exit=0,
        count_source=count_source,
        tests=tests,
        stdout=output,
        stderr="tool" if "stderr tool" in rule else "empty",
        task_owner=_task_owner(identifier),
    )


def _output_rule(rule: str) -> OutputRule:
    match = re.search(r"exact stdout `(.+)`", rule)
    if match is not None:
        return OutputRule("exact", match.group(1).replace(r"\n", "\n"))
    if "exact one version line+LF" in rule:
        return OutputRule("version-line", None)
    if "nonempty stdout" in rule:
        return OutputRule("nonempty", None)
    return OutputRule("unrestricted", None)


def _test_rule(rule: str, output: OutputRule) -> TestRule:
    match = re.search(r">=(\d+)", rule)
    if match is not None:
        return TestRule("minimum", int(match.group(1)))
    match = re.search(r"(?<!>)=(\d+)", rule)
    if match is not None:
        return TestRule("exact", int(match.group(1)))
    if output.mode == "exact" and output.value is not None and output.value.startswith('{"checks":'):
        parsed = json.loads(output.value)
        return TestRule("exact", int(parsed["checks"]))
    return TestRule("exact", 0)


def _count_source(identifier: str, output: OutputRule, tests: TestRule) -> str:
    if identifier in {"pre-01", "t12-03"}:
        return "rust-harness-sum"
    if output.mode == "exact" and output.value is not None and output.value.startswith('{"checks":'):
        return "stdout-json:checks"
    if tests.value > 0:
        return "rust-harness"
    return "constant:0"


def _task_owner(identifier: str) -> str:
    if identifier == "pre-01":
        return "preflight"
    if identifier == "tip-prerequisite":
        return "prerequisite"
    if identifier.startswith("F"):
        return "final"
    if identifier == "tip-m1516":
        return "merge"
    if identifier in {"pre-02", "pre-03"}:
        return "1"
    if identifier.startswith(("release-", "homebrew-")):
        return "15"
    if identifier.startswith("nix.home-manager."):
        return "17"
    match = re.match(r"(?:t|tip-t)(\d{2})(?:-|\Z)", identifier)
    if match is None:
        fail(f"registry ID has no task owner: {identifier}")
    return str(int(match.group(1)))


def _parse_row(value: object, index: int) -> CommandSpec:
    if not isinstance(value, dict):
        fail(f"commands[{index}] must be an object")
    require_exact_keys(value, ROW_KEYS, f"commands[{index}]")
    env_value = value["env"]
    if not isinstance(env_value, dict) or not all(isinstance(k, str) and isinstance(v, str) for k, v in env_value.items()):
        fail(f"commands[{index}].env must be a string map")
    test = _parse_rule(value["tests"], f"commands[{index}].tests")
    stdout = _parse_output(value["stdout"], f"commands[{index}].stdout")
    cwd = require_string(value["cwd"], f"commands[{index}].cwd")
    posix = PurePosixPath(cwd)
    if posix.is_absolute() or ".." in posix.parts or cwd != ".":
        fail(f"invalid registry cwd: {cwd}")
    expected_exit = require_uint(value["expected_exit"], f"commands[{index}].expected_exit")
    if expected_exit != 0:
        fail("registry expected_exit must be zero")
    return CommandSpec(
        id=require_safe_id(value["id"], f"commands[{index}].id"),
        command=require_string(value["command"], f"commands[{index}].command"),
        shell=require_string(value["shell"], f"commands[{index}].shell"),
        cwd=cwd,
        env={key: env_value[key] for key in sorted(env_value)},
        execution_context=require_safe_id(value["execution_context"], f"commands[{index}].execution_context"),
        expected_exit=expected_exit,
        count_source=require_string(value["count_source"], f"commands[{index}].count_source"),
        tests=test,
        stdout=stdout,
        stderr=require_string(value["stderr"], f"commands[{index}].stderr"),
        task_owner=require_string(value["task_owner"], f"commands[{index}].task_owner"),
    )


def _parse_rule(value: object, label: str) -> TestRule:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    require_exact_keys(value, frozenset({"mode", "value"}), label)
    mode = require_string(value["mode"], f"{label}.mode")
    if mode not in {"exact", "minimum"}:
        fail(f"invalid {label}.mode")
    return TestRule(mode, require_uint(value["value"], f"{label}.value"))


def _parse_output(value: object, label: str) -> OutputRule:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    require_exact_keys(value, frozenset({"mode", "value"}), label)
    mode = require_string(value["mode"], f"{label}.mode")
    output = value["value"]
    if output is not None and not isinstance(output, str):
        fail(f"{label}.value must be string or null")
    if mode not in {"exact", "nonempty", "unrestricted", "version-line"}:
        fail(f"invalid {label}.mode")
    return OutputRule(mode, output)
