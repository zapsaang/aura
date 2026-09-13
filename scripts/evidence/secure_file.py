from __future__ import annotations

import os
import stat
from pathlib import Path

from .model import fail, sha256_bytes, write_new


def open_regular_nofollow(path: Path) -> int:
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    except OSError as error:
        fail(f"unable to open regular file without following links: {path}: {error}")
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            fail(f"path is not a regular file: {path}")
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def read_regular(path: Path) -> bytes:
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        fail(f"path is not a regular file: {path}")
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino, opened.st_mode) != (before.st_dev, before.st_ino, before.st_mode):
            fail(f"path identity changed while opening: {path}")
        with os.fdopen(descriptor, "rb", closefd=False) as source:
            raw = source.read()
        after = os.fstat(descriptor)
        stable = ("st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns")
        if any(getattr(opened, field) != getattr(after, field) for field in stable):
            fail(f"path changed while reading: {path}")
        return raw
    finally:
        os.close(descriptor)


def require_directory_identity(path: Path, device: int, inode: int, label: str) -> None:
    try:
        metadata = path.stat(follow_symlinks=False)
    except FileNotFoundError:
        fail(f"{label} path does not exist: {path}")
    if not stat.S_ISDIR(metadata.st_mode):
        fail(f"{label} path is not a directory: {path}")
    if metadata.st_dev != device:
        fail(f"{label} device mismatch: recorded {device}, actual {metadata.st_dev}")
    if metadata.st_ino != inode:
        fail(f"{label} inode mismatch: recorded {inode}, actual {metadata.st_ino}")


def copy_regular(source: Path, destination: Path) -> str:
    raw = read_regular(source)
    write_new(destination, raw)
    return sha256_bytes(raw)
