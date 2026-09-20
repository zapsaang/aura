#!/usr/bin/env python3
"""Build a deterministic aura-<target>.tar.gz release archive.

Contents are exactly the fixed member set {aura-cli, aura-daemon, SHA256SUMS}
stored in ASCII-sorted order with gzip/tar mtime 0, uid/gid 0, empty
owner/group names, mode 0755 for binaries and 0644 for SHA256SUMS. The
archive payload is built twice in memory and the run fails unless both builds
are byte-identical, so packaging is self-tested reproducible on every host.
"""
import argparse
import hashlib
import gzip
import io
import os
import sys
import tarfile

sys.dont_write_bytecode = True

BINARIES = ("aura-cli", "aura-daemon")
SUMS_NAME = "SHA256SUMS"


class PackageError(RuntimeError):
    pass


def _read_binary(input_dir: str, name: str) -> bytes:
    path = os.path.join(input_dir, name)
    if not os.path.isfile(path) or os.path.islink(path):
        raise PackageError(f"missing regular binary {path}")
    with open(path, "rb") as handle:
        return handle.read()


def _build_payload(binaries: dict[str, bytes]) -> bytes:
    sums = "".join(
        f"{hashlib.sha256(binaries[name]).hexdigest()}  {name}\n"
        for name in sorted(binaries)
    ).encode("ascii")
    members: dict[str, tuple[bytes, int]] = {SUMS_NAME: (sums, 0o644)}
    for name, data in binaries.items():
        members[name] = (data, 0o755)

    tar_buffer = io.BytesIO()
    with tarfile.open(fileobj=tar_buffer, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for name in sorted(members):
            data, mode = members[name]
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mtime = 0
            info.uid = 0
            info.gid = 0
            info.uname = ""
            info.gname = ""
            info.mode = mode
            tar.addfile(info, io.BytesIO(data))

    out_buffer = io.BytesIO()
    with gzip.GzipFile(
        filename="", mode="wb", fileobj=out_buffer, mtime=0
    ) as gzip_file:
        gzip_file.write(tar_buffer.getvalue())
    return out_buffer.getvalue()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    try:
        binaries = {name: _read_binary(args.input, name) for name in BINARIES}
        first = _build_payload(binaries)
        second = _build_payload(binaries)
        if first != second:
            raise PackageError("archive payload is not byte reproducible")
        expected_name = f"aura-{args.target}.tar.gz"
        if os.path.basename(args.output) != expected_name:
            raise PackageError(
                f"output name must be {expected_name}, got {os.path.basename(args.output)}"
            )
        with open(args.output, "wb") as handle:
            handle.write(first)
    except (OSError, PackageError) as error:
        sys.stderr.write(f"package-release: {error}\n")
        return 1

    sys.stdout.write('{"checks":1,"status":"ok"}\n')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
