from __future__ import annotations

import os
import stat
import subprocess
from pathlib import Path
from typing import Final

from .model import fail
from .secure_file import read_regular

SourceState = tuple[tuple[str, bytes], ...]

SOURCE_STATE_COMMANDS: Final = (
    ("status.bin", ("status", "--porcelain=v1", "-z", "--untracked-files=all")),
    ("tracked.patch", ("diff", "--binary", "--full-index", "HEAD", "--")),
    ("name-status.bin", ("diff", "--name-status", "-z", "HEAD", "--")),
    ("name-status-no-renames.bin", ("diff", "--name-status", "-z", "--no-renames", "HEAD", "--")),
)
SOURCE_STATE_NAMES: Final = frozenset(name for name, _arguments in SOURCE_STATE_COMMANDS)


def capture_source_state(source_root: Path) -> SourceState:
    environment = {
        **os.environ,
        "GIT_MASTER": "1",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_OPTIONAL_LOCKS": "0",
    }
    prefix = [
        "git",
        "--no-optional-locks",
        "-c",
        "core.excludesFile=/dev/null",
        "-c",
        "core.attributesFile=/dev/null",
    ]
    captured: list[tuple[str, bytes]] = []
    for name, arguments in SOURCE_STATE_COMMANDS:
        result = subprocess.run(
            [*prefix, *arguments],
            cwd=source_root,
            env=environment,
            check=False,
            capture_output=True,
        )
        if result.returncode != 0:
            fail(f"git source-state capture failed for {name}: {result.stderr.decode('utf-8', errors='replace')}")
        captured.append((name, result.stdout))
    return tuple(captured)


def read_source_state(directory: Path) -> SourceState:
    try:
        metadata = directory.stat(follow_symlinks=False)
        entries = tuple(directory.iterdir())
    except OSError as error:
        fail(f"unable to read source-state directory: {directory}: {error}")
    if not stat.S_ISDIR(metadata.st_mode):
        fail(f"source-state path is not a directory: {directory}")
    actual = frozenset(entry.name for entry in entries)
    if actual != SOURCE_STATE_NAMES:
        fail(
            "source-state directory closure differs: "
            f"missing={sorted(SOURCE_STATE_NAMES - actual)}, unknown={sorted(actual - SOURCE_STATE_NAMES)}"
        )
    return tuple((name, read_regular(directory / name)) for name, _arguments in SOURCE_STATE_COMMANDS)
