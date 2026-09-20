from __future__ import annotations

import io
import os
import stat
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.fspath(Path(__file__).resolve().parents[1]))

from evidence.archive import ArchiveLimits, create_deterministic_archive, safe_extract_archive
from evidence.model import EvidenceError


class EvidenceArchiveTests(unittest.TestCase):
    def test_archive_is_byte_reproducible_with_canonical_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "job"
            (source / "tuples" / "z").mkdir(parents=True)
            (source / "receipt.json").write_text("{}\n", encoding="utf-8")
            (source / "tuples" / "z" / "stdout.bin").write_bytes(b"ok\n")
            first = root / "first.tar.gz"
            second = root / "second.tar.gz"

            first_sha = create_deterministic_archive(source, first, "lane/job")
            second_sha = create_deterministic_archive(source, second, "lane/job")

            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(first_sha, second_sha)
            with tarfile.open(first, "r:gz") as archive:
                members = archive.getmembers()
            self.assertEqual([member.name for member in members], sorted(member.name for member in members))
            for member in members:
                self.assertEqual(member.mtime, 0)
                self.assertEqual(member.uid, 0)
                self.assertEqual(member.gid, 0)
                self.assertEqual(member.uname, "")
                self.assertEqual(member.gname, "")
                self.assertEqual(member.mode, 0o755 if member.isdir() else 0o644)

    def test_extraction_root_is_mode_0700(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            (source / "file").write_bytes(b"content")
            archive = root / "producer.tar.gz"
            create_deterministic_archive(source, archive, "source")
            destination = root / "input"

            extracted = safe_extract_archive(archive, destination, expected_root="source")

            self.assertEqual(extracted, destination / "source")
            self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o700)
            self.assertEqual((extracted / "file").read_bytes(), b"content")

    def test_nested_lane_root_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "job"
            source.mkdir()
            (source / "receipt.json").write_bytes(b"{}\n")
            archive = root / "lane.tar.gz"
            create_deterministic_archive(source, archive, "lane/job")

            extracted = safe_extract_archive(archive, root / "out", expected_root="lane/job")

            self.assertEqual(extracted, root / "out" / "lane" / "job")

    def test_hostile_paths_and_duplicate_members_are_rejected(self) -> None:
        cases = (
            ("absolute", ("/escape",)),
            ("traversal", ("root/../escape",)),
            ("duplicate", ("root/file", "root/file")),
        )
        for label, names in cases:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                archive = root / "hostile.tar.gz"
                self._write_archive(archive, tuple((name, tarfile.REGTYPE) for name in names))

                with self.assertRaises(EvidenceError):
                    safe_extract_archive(archive, root / "out")

    def test_missing_root_and_noncanonical_member_order_are_rejected(self) -> None:
        cases = (
            ("missing-root", (("root/file", tarfile.REGTYPE),)),
            (
                "unsorted",
                (
                    ("root", tarfile.DIRTYPE),
                    ("root/z", tarfile.REGTYPE),
                    ("root/a", tarfile.REGTYPE),
                ),
            ),
        )
        for label, members in cases:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                archive = root / "hostile.tar.gz"
                self._write_archive(archive, members)

                with self.assertRaises(EvidenceError):
                    safe_extract_archive(archive, root / "out", expected_root="root")

    def test_noncanonical_archive_metadata_is_rejected(self) -> None:
        mutations = {
            "file-mode": {"mode": 0o600},
            "directory-mode": {"mode": 0o700},
            "uid": {"uid": 1000},
            "gid": {"gid": 1000},
            "owner": {"uname": "runner"},
            "group": {"gname": "runner"},
            "mtime": {"mtime": 1},
        }
        for label, mutation in mutations.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                archive = root / "hostile.tar.gz"
                target = "root" if label == "directory-mode" else "root/file"
                self._write_archive(
                    archive,
                    (("root", tarfile.DIRTYPE), ("root/file", tarfile.REGTYPE)),
                    mutation=(target, mutation),
                )

                with self.assertRaises(EvidenceError):
                    safe_extract_archive(archive, root / "out", expected_root="root")

    def test_archive_creation_rejects_symlink_source_root(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            (source / "file").write_bytes(b"content")
            alias = root / "alias"
            alias.symlink_to(source, target_is_directory=True)

            with self.assertRaises(EvidenceError):
                create_deterministic_archive(alias, root / "archive.tar.gz", "source")

    def test_links_devices_fifos_and_sockets_are_rejected(self) -> None:
        rejected_types = (
            tarfile.SYMTYPE,
            tarfile.LNKTYPE,
            tarfile.CHRTYPE,
            tarfile.BLKTYPE,
            tarfile.FIFOTYPE,
            b"s",
        )
        for member_type in rejected_types:
            with self.subTest(member_type=member_type), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                archive = root / "hostile.tar.gz"
                self._write_archive(archive, (("root/hostile", member_type),))

                with self.assertRaises(EvidenceError):
                    safe_extract_archive(archive, root / "out")

    def test_symlink_archive_path_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            (source / "file").write_bytes(b"content")
            archive = root / "real.tar.gz"
            create_deterministic_archive(source, archive, "source")
            alias = root / "alias.tar.gz"
            alias.symlink_to(archive)

            with self.assertRaises(EvidenceError):
                safe_extract_archive(alias, root / "out")

    def test_archive_path_is_opened_exactly_once(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            source = root / "source"
            source.mkdir()
            (source / "file").write_bytes(b"content")
            archive = root / "source.tar.gz"
            digest = create_deterministic_archive(source, archive, "source")
            target = os.fspath(archive)
            opens: list[str] = []
            original_open = os.open

            def counting_open(path: object, flags: int, *args: object, **kwargs: object) -> int:
                if os.fspath(path) == target:
                    opens.append(target)
                return original_open(path, flags, *args, **kwargs)

            with mock.patch.object(os, "open", counting_open):
                extracted = safe_extract_archive(archive, root / "out", expected_sha256=digest)

            self.assertEqual(len(opens), 1)
            self.assertEqual((extracted / "file").read_bytes(), b"content")

    def test_resource_caps_are_enforced(self) -> None:
        members = (
            ("root", tarfile.DIRTYPE),
            ("root/a", tarfile.REGTYPE),
            ("root/b", tarfile.REGTYPE),
        )
        cases = (
            ("compressed", ArchiveLimits(max_compressed_bytes=1)),
            ("member-count", ArchiveLimits(max_members=2)),
            ("member-size", ArchiveLimits(max_member_bytes=0)),
            ("expanded-total", ArchiveLimits(max_expanded_bytes=1)),
        )
        for label, limits in cases:
            with self.subTest(label=label), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                archive = root / "hostile.tar.gz"
                self._write_archive(archive, members)

                with self.assertRaises(EvidenceError):
                    safe_extract_archive(archive, root / "out", limits=limits)

    @staticmethod
    def _write_archive(
        path: Path,
        members: tuple[tuple[str, bytes], ...],
        *,
        mutation: tuple[str, dict[str, object]] | None = None,
    ) -> None:
        with tarfile.open(path, "w:gz") as archive:
            for name, member_type in members:
                info = tarfile.TarInfo(name)
                info.type = member_type
                info.mode = 0o755 if member_type == tarfile.DIRTYPE else 0o644
                info.uid = 0
                info.gid = 0
                info.uname = ""
                info.gname = ""
                info.mtime = 0
                if mutation is not None and name == mutation[0]:
                    for field, value in mutation[1].items():
                        setattr(info, field, value)
                if member_type == tarfile.REGTYPE:
                    info.size = 1
                    archive.addfile(info, io.BytesIO(b"x"))
                else:
                    info.linkname = "target"
                    archive.addfile(info)


if __name__ == "__main__":
    unittest.main()
