from __future__ import annotations

import dataclasses
import gzip
import os
import shutil
import stat
import tarfile
from pathlib import Path, PurePosixPath

from .model import fail, sha256_fd, sha256_file
from .secure_file import open_regular_nofollow

MAX_ARCHIVE_COMPRESSED_BYTES = 2 * 1024**3
MAX_ARCHIVE_MEMBERS = 100_000
MAX_ARCHIVE_MEMBER_BYTES = 1024**3
MAX_ARCHIVE_EXPANDED_BYTES = 8 * 1024**3


@dataclasses.dataclass(frozen=True)
class ArchiveLimits:
    max_compressed_bytes: int = MAX_ARCHIVE_COMPRESSED_BYTES
    max_members: int = MAX_ARCHIVE_MEMBERS
    max_member_bytes: int = MAX_ARCHIVE_MEMBER_BYTES
    max_expanded_bytes: int = MAX_ARCHIVE_EXPANDED_BYTES


def create_deterministic_archive(source: Path, output: Path, root_name: str) -> str:
    source_metadata = source.lstat()
    if not stat.S_ISDIR(source_metadata.st_mode):
        fail(f"archive source is not a regular directory: {source}")
    source = source.resolve(strict=True)
    root = _archive_path(root_name)
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists() or output.is_symlink():
        fail(f"archive output already exists: {output}")
    entries = _source_entries(source, root)
    descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with (
            os.fdopen(descriptor, "wb", closefd=False) as raw,
            gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed,
            tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive,
        ):
            for path, name, metadata in entries:
                info = tarfile.TarInfo(name)
                info.uid = 0
                info.gid = 0
                info.uname = ""
                info.gname = ""
                info.mtime = 0
                if stat.S_ISDIR(metadata.st_mode):
                    info.type = tarfile.DIRTYPE
                    info.mode = 0o755
                    info.size = 0
                    archive.addfile(info)
                else:
                    info.type = tarfile.REGTYPE
                    info.mode = 0o644
                    info.size = metadata.st_size
                    file_descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
                    try:
                        with os.fdopen(file_descriptor, "rb", closefd=False) as file_object:
                            archive.addfile(info, file_object)
                        final_metadata = os.fstat(file_descriptor)
                        stable_fields = ("st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns")
                        if any(getattr(final_metadata, field) != getattr(metadata, field) for field in stable_fields):
                            fail(f"archive source changed while reading: {path}")
                    finally:
                        os.close(file_descriptor)
            raw.flush()
            os.fsync(raw.fileno())
    except BaseException:
        output.unlink(missing_ok=True)
        raise
    finally:
        os.close(descriptor)
    return sha256_file(output)


def safe_extract_archive(
    archive_path: Path,
    destination: Path,
    *,
    expected_root: str | None = None,
    expected_sha256: str | None = None,
    limits: ArchiveLimits | None = None,
) -> Path:
    limits = limits if limits is not None else ArchiveLimits()
    descriptor = open_regular_nofollow(archive_path)
    try:
        if os.fstat(descriptor).st_size > limits.max_compressed_bytes:
            fail(f"archive compressed size exceeds limit: {archive_path}")
        if expected_sha256 is not None and sha256_fd(descriptor) != expected_sha256:
            fail(f"archive digest mismatch: {archive_path}")
        raw = os.fdopen(os.dup(descriptor), "rb")
        try:
            with tarfile.open(fileobj=raw, mode="r:gz") as archive:
                members = _bounded_members(archive, limits)
                root = _validate_members(members, expected_root)
                if destination.exists() or destination.is_symlink():
                    fail(f"extraction destination must be fresh: {destination}")
                destination.mkdir(parents=True, mode=0o700)
                destination.chmod(0o700)
                root_descriptor = os.open(destination, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
                try:
                    for member in members:
                        _extract_member(archive, member, root_descriptor)
                except BaseException:
                    shutil.rmtree(destination, ignore_errors=True)
                    raise
                finally:
                    os.close(root_descriptor)
        finally:
            raw.close()
    finally:
        os.close(descriptor)
    return destination / root


def _bounded_members(archive: tarfile.TarFile, limits: ArchiveLimits) -> list[tarfile.TarInfo]:
    members: list[tarfile.TarInfo] = []
    total_bytes = 0
    for member in archive:
        if len(members) >= limits.max_members:
            fail(f"archive member count exceeds limit {limits.max_members}")
        if member.size > limits.max_member_bytes:
            fail(f"archive member exceeds size limit {limits.max_member_bytes}: {member.name}")
        total_bytes += member.size
        if total_bytes > limits.max_expanded_bytes:
            fail(f"archive expanded size exceeds limit {limits.max_expanded_bytes}")
        members.append(member)
    return members


def _source_entries(source: Path, root: str) -> list[tuple[Path, str, os.stat_result]]:
    entries: list[tuple[Path, str, os.stat_result]] = [(source, root, source.stat(follow_symlinks=False))]
    for path in source.rglob("*"):
        metadata = path.lstat()
        name = f"{root}/{path.relative_to(source).as_posix()}"
        if not stat.S_ISDIR(metadata.st_mode) and not stat.S_ISREG(metadata.st_mode):
            fail(f"archive source contains unsupported path: {name}")
        entries.append((path, name, metadata))
    entries.sort(key=lambda entry: os.fsencode(entry[1]))
    return entries


def _validate_members(members: list[tarfile.TarInfo], expected_root: str | None) -> str:
    if not members:
        fail("archive is empty")
    names: set[str] = set()
    roots: set[str] = set()
    canonical_names: list[str] = []
    by_name: dict[str, tarfile.TarInfo] = {}
    for member in members:
        name = _archive_path(member.name)
        if name in names:
            fail(f"duplicate archive path: {name}")
        names.add(name)
        canonical_names.append(name)
        by_name[name] = member
        roots.add(PurePosixPath(name).parts[0])
        if member.type not in {tarfile.DIRTYPE, tarfile.REGTYPE}:
            fail(f"unsupported archive member type: {name}")
        _validate_member_metadata(member, name)
    if canonical_names != sorted(canonical_names, key=os.fsencode):
        fail("archive paths are not sorted")
    if len(roots) != 1:
        fail(f"archive must contain exactly one root: {sorted(roots)}")
    root = next(iter(roots))
    if expected_root is not None:
        expected = _archive_path(expected_root)
        if expected not in names or any(name != expected and not name.startswith(f"{expected}/") for name in names):
            fail(f"archive root mismatch: expected {expected_root}, got {root}")
        root = expected
    if root not in by_name or not by_name[root].isdir():
        fail(f"archive root directory is missing: {root}")
    return root


def _validate_member_metadata(member: tarfile.TarInfo, name: str) -> None:
    expected_mode = 0o755 if member.isdir() else 0o644
    if member.mode != expected_mode:
        fail(f"non-canonical archive mode: {name}")
    if member.uid != 0 or member.gid != 0:
        fail(f"non-canonical archive ownership: {name}")
    if member.uname != "" or member.gname != "":
        fail(f"non-canonical archive owner names: {name}")
    if member.mtime != 0:
        fail(f"non-canonical archive timestamp: {name}")
    if member.pax_headers:
        fail(f"archive member has extended headers: {name}")
    if member.isdir() and member.size != 0:
        fail(f"archive directory has nonzero size: {name}")


def _extract_member(archive: tarfile.TarFile, member: tarfile.TarInfo, root_descriptor: int) -> None:
    parts = PurePosixPath(member.name).parts
    parent = _open_directory_path(root_descriptor, parts[:-1])
    try:
        leaf = parts[-1]
        if member.isdir():
            try:
                os.mkdir(leaf, 0o755, dir_fd=parent)
            except FileExistsError:
                directory = os.open(leaf, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
                os.close(directory)
            return
        source = archive.extractfile(member)
        if source is None:
            fail(f"unable to read archive member: {member.name}")
        descriptor = os.open(
            leaf,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o644,
            dir_fd=parent,
        )
        try:
            with os.fdopen(descriptor, "wb", closefd=False) as target:
                shutil.copyfileobj(source, target)
        finally:
            source.close()
            os.close(descriptor)
    finally:
        os.close(parent)


def _open_directory_path(root_descriptor: int, parts: tuple[str, ...]) -> int:
    current = os.dup(root_descriptor)
    for part in parts:
        try:
            try:
                os.mkdir(part, 0o755, dir_fd=current)
            except FileExistsError:
                pass
            next_descriptor = os.open(
                part,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                dir_fd=current,
            )
        finally:
            os.close(current)
        current = next_descriptor
    return current


def _archive_path(value: str) -> str:
    if not value or "\x00" in value or "\\" in value:
        fail(f"invalid archive path: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        fail(f"invalid archive path: {value!r}")
    normalized = path.as_posix()
    if normalized != value.rstrip("/"):
        fail(f"non-canonical archive path: {value!r}")
    return normalized
