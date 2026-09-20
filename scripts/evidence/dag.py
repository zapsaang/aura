from __future__ import annotations

import os
import subprocess
from collections.abc import Callable
from pathlib import Path

from .model import fail, require_hex

GitRunner = Callable[[list[str]], bytes]


def git_runner(cwd: Path | None = None) -> GitRunner:
    def run(arguments: list[str]) -> bytes:
        command = ["git"]
        if cwd is not None:
            command += ["-C", os.fspath(cwd)]
        result = subprocess.run(
            [*command, *arguments],
            env={**os.environ, "GIT_MASTER": "1"},
            check=False,
            capture_output=True,
        )
        if result.returncode != 0:
            fail(f"git {' '.join(arguments)} failed: {result.stderr.decode('utf-8', errors='replace')}")
        return result.stdout

    return run


def commit_parents(git: GitRunner, commit: str) -> list[str]:
    fields = git(["rev-list", "--parents", "-n", "1", commit]).decode("utf-8").split()
    if not fields or fields[0] != require_hex(commit, 40, "commit"):
        fail(f"cannot resolve commit parents: {commit}")
    return [require_hex(parent, 40, "parent") for parent in fields[1:]]


def require_ancestor(git: GitRunner, ancestor: str, descendant: str, label: str) -> None:
    base = git(["merge-base", ancestor, descendant]).decode("utf-8").strip()
    if base != require_hex(ancestor, 40, label):
        fail(f"{label} is not an ancestor of {descendant}")


def require_exact_dag(
    git: GitRunner,
    *,
    baseline: str,
    prerequisite: str,
    task_chain: list[str],
    branch_tips: tuple[str, str],
    final: str,
) -> None:
    if len(task_chain) != 14:
        fail("task chain must resolve T1..T14 in order")
    for value in (baseline, prerequisite, final, *task_chain, *branch_tips):
        require_hex(value, 40, "dag commit")
    require_ancestor(git, baseline, final, "baseline")
    require_ancestor(git, prerequisite, final, "prerequisite")
    if commit_parents(git, prerequisite) != [baseline]:
        fail("prerequisite parent must be the imported baseline commit")
    expected_parent = prerequisite
    for index, task_commit in enumerate(task_chain, start=1):
        if commit_parents(git, task_commit) != [expected_parent]:
            fail(f"T{index} is not a direct child of its required parent")
        expected_parent = task_commit
    t14 = task_chain[-1]
    for label, tip in (("T15", branch_tips[0]), ("T16", branch_tips[1])):
        if commit_parents(git, tip) != [t14]:
            fail(f"{label} must be a direct child of T14")
    final_parents = commit_parents(git, final)
    if len(final_parents) != 1:
        fail("final commit must have exactly one parent (the M1516 merge)")
    merge = final_parents[0]
    merge_parents = commit_parents(git, merge)
    if len(merge_parents) != 2:
        fail("M1516 merge commit must have exactly two parents")
    if set(merge_parents) != set(branch_tips):
        fail("M1516 parents must be exactly the T15 and T16 tips")
    require_ancestor(git, prerequisite, merge, "prerequisite")
