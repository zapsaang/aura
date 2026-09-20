from __future__ import annotations

import os
import stat
from pathlib import Path

from .model import canonical_json_bytes, fail, parse_json_bytes, sha256_bytes, write_new

KEYS = frozenset({"HOME", "PATH", "TMPDIR"})


def create_environment(path: Path, home: Path, search_path: str, temporary: Path) -> dict[str, str]:
    home.mkdir(parents=True, mode=0o700)
    temporary.mkdir(parents=True, mode=0o700)
    home.chmod(0o700)
    temporary.chmod(0o700)
    value = {
        "HOME": os.fspath(home.resolve(strict=True)),
        "PATH": search_path,
        "TMPDIR": os.fspath(temporary.resolve(strict=True)),
    }
    validate_environment(value, require_directories=True)
    write_new(path, canonical_json_bytes(value))
    return value


def load_environment(path: Path, *, require_directories: bool = True) -> dict[str, str]:
    raw = path.read_bytes()
    value = parse_json_bytes(raw, KEYS, "lane environment")
    environment = {key: value[key] for key in sorted(value)}
    if not all(isinstance(item, str) for item in environment.values()):
        fail("lane environment values must be strings")
    typed = {key: str(item) for key, item in environment.items()}
    validate_environment(typed, require_directories=require_directories)
    return typed


def environment_sha256(path: Path, *, require_directories: bool = True) -> str:
    load_environment(path, require_directories=require_directories)
    return sha256_bytes(path.read_bytes())


def validate_environment(value: dict[str, str], *, require_directories: bool) -> None:
    if frozenset(value) != KEYS:
        fail("lane environment must contain exactly HOME, PATH, and TMPDIR")
    for key, item in value.items():
        if not item or any(ord(character) < 32 or ord(character) == 127 for character in item):
            fail(f"invalid lane environment value: {key}")
    for key in ("HOME", "TMPDIR"):
        path = Path(value[key])
        if not path.is_absolute():
            fail(f"{key} must be absolute")
        if require_directories:
            metadata = path.stat(follow_symlinks=False)
            if not stat.S_ISDIR(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o700:
                fail(f"{key} must be a mode-0700 directory")
    components = value["PATH"].split(":")
    if not components or any(not component or not Path(component).is_absolute() for component in components):
        fail("PATH must be a nonempty colon-list of absolute paths")
