from __future__ import annotations

import os
import stat
from pathlib import Path, PurePosixPath

from .model import fail, sha256_bytes, sha256_file, write_new


def manifest_bytes(root: Path, excluded: frozenset[str]) -> bytes:
    entries: list[tuple[bytes, str, str]] = []
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        name = relative.as_posix()
        if name in excluded:
            continue
        metadata = path.lstat()
        if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISREG(metadata.st_mode):
            if stat.S_ISDIR(metadata.st_mode):
                continue
            fail(f"manifest path is not a regular file: {name}")
        _validate_manifest_name(name)
        entries.append((os.fsencode(name), sha256_file(path), name))
    entries.sort(key=lambda entry: entry[0])
    return b"".join(f"{digest}  {name}\n".encode() for _, digest, name in entries)


def write_manifest(root: Path, excluded: frozenset[str] = frozenset({"SHA256SUMS"})) -> str:
    raw = manifest_bytes(root, excluded)
    write_new(root / "SHA256SUMS", raw)
    return sha256_bytes(raw)


def verify_manifest(root: Path, excluded: frozenset[str] = frozenset({"SHA256SUMS"})) -> str:
    manifest = root / "SHA256SUMS"
    if not manifest.is_file() or manifest.is_symlink():
        fail(f"missing regular manifest: {manifest}")
    actual = manifest.read_bytes()
    expected = manifest_bytes(root, excluded)
    if actual != expected:
        fail(f"manifest mismatch: {manifest}")
    return sha256_bytes(actual)


def _validate_manifest_name(name: str) -> None:
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or any(part in {"", ".", ".."} for part in path.parts):
        fail(f"invalid manifest path: {name!r}")
    if any(ord(character) < 32 or ord(character) == 127 for character in name):
        fail(f"control character in manifest path: {name!r}")
