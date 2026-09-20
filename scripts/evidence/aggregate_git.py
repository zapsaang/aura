from __future__ import annotations

import re
from pathlib import Path

from .dag import git_runner, require_exact_dag
from .model import decode_json_bytes, fail, require_hex

_git = git_runner()


def _require_commit_graph(producers: dict[str, Path], final_commit: str) -> None:
    commits: dict[str, str] = {}
    for producer, key in (("preflight", "imported_baseline_commit"), ("prerequisite", "prerequisite_commit")):
        value = decode_json_bytes((producers[producer] / "receipt.json").read_bytes(), f"{producer} receipt")
        if not isinstance(value, dict):
            fail(f"invalid {producer} receipt")
        commits[producer] = require_hex(value.get(key), 40, key)
    subjects = _commit_subjects(final_commit)
    require_exact_dag(
        _git,
        baseline=commits["preflight"],
        prerequisite=commits["prerequisite"],
        task_chain=[_task_commit(task, subjects) for task in range(1, 15)],
        branch_tips=(_task_commit(15, subjects), _task_commit(16, subjects)),
        final=final_commit,
    )


def _tag_for_commit(commit: str) -> str:
    tags = [line for line in _git(["tag", "--points-at", commit]).decode("utf-8").splitlines() if re.fullmatch(
        r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", line
    )]
    if len(tags) != 1:
        fail(f"verified commit must have exactly one canonical tag: {tags}")
    return tags[0]


def _commit_subjects(commit: str) -> dict[str, list[str]]:
    raw = _git(["log", "--format=%H%x00%s%x00", commit])
    # git log --format terminates each entry with a newline; strip the
    # record-separator newline so hash fields stay 40-lowerhex.
    fields = raw.decode("utf-8").replace("\x00\n", "\x00").split("\x00")
    result: dict[str, list[str]] = {}
    for index in range(0, len(fields) - 1, 2):
        if fields[index] and fields[index + 1]:
            result.setdefault(fields[index + 1], []).append(fields[index])
    return result


def _task_commit(task: int, subjects: dict[str, list[str]]) -> str:
    prefixes = re.compile(r"^(?:feat|fix|refactor|build|docs|test): ")
    candidates = [commit for subject, commits in subjects.items() for commit in commits if prefixes.match(subject)]
    task_tips = {
        1: "feat(common): define telemetry ABI v2 contract",
        2: "fix(ipc): secure and unify per-user shared memory",
        3: "fix(network): enforce interface bound every cycle",
        4: "fix(ipc): harden seqlock ordering timeout and recovery",
        5: "fix(daemon): publish only complete telemetry cycles",
        6: "fix(daemon): use systemd software watchdog notifications",
        7: "fix(collectors): compute interval and derived metrics in daemon",
        8: "feat(collectors): restore process telemetry",
        9: "feat(collectors): restore storage telemetry",
        10: "feat(macos): expose public telemetry with capability flags",
        11: "feat(meta): complete identity and wall-clock telemetry",
        12: "feat(gpu): enable dynamically loaded NVML on Linux",
        13: "feat(cli): render complete capability-aware telemetry",
        14: "fix(cli): classify offline and IPC failures precisely",
        15: "build: align telemetry deployment and release contracts",
        16: "docs: record design compliance remediation decisions",
        17: "test: prove design compliance across all audit findings",
    }
    subject = task_tips.get(task)
    if subject is None:
        fail(f"no historical subject for task {task}; candidates={len(candidates)}")
    return _subject_commit(subject, subjects)


def _subject_commit(subject: str, subjects: dict[str, list[str]]) -> str:
    commits = subjects.get(subject, [])
    if len(commits) != 1:
        fail(f"commit subject must resolve exactly once: {subject}")
    return require_hex(commits[0], 40, "commit")
