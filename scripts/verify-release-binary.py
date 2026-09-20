#!/usr/bin/env python3
"""Offline verifier for packaged aura release binaries.

Checks both packaged binaries inside aura-<target>.tar.gz and emits exactly
{"checks":2,"status":"ok"}. Raw tool output is mirrored to stderr; stdout
carries only the contract JSON line.

Linux (readelf): ELF machine matches --arch; with --forbid-import no NEEDED
entry may case-insensitively match the pattern.
macOS (file/otool/nm): file reports Mach-O 64-bit of the resolved arch; with
--minos every LC_BUILD_VERSION in both binaries matches; with
--forbid-import neither otool -L libraries nor nm -u undefined symbols may
case-insensitively match the pattern.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.dont_write_bytecode = True

BINARIES = ("aura-cli", "aura-daemon")
ELF_MACHINES = {
    "x86_64": "Advanced Micro Devices X86-64",
    "aarch64": "AArch64",
}
MACHO_ARCHES = {
    "x86_64": "x86_64",
    "aarch64": "arm64",
}


class VerifyError(RuntimeError):
    pass


def _log(text: str) -> None:
    sys.stderr.write(text)
    if not text.endswith("\n"):
        sys.stderr.write("\n")


def _run(tool: list[str], label: str) -> str:
    try:
        result = subprocess.run(
            tool,
            check=False,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except OSError as error:
        raise VerifyError(f"cannot execute {tool[0]}: {error}") from error
    output = result.stdout.decode("utf-8", errors="replace")
    _log(f"$ {' '.join(tool)}\n{output}{result.stderr.decode('utf-8', errors='replace')}")
    if result.returncode != 0:
        raise VerifyError(f"{label}: {' '.join(tool)} exited {result.returncode}")
    return output


def _extract(archive: str, dest: str) -> list[str]:
    paths = []
    payloads: dict[str, bytes] = {}
    try:
        with tarfile.open(archive, "r:gz") as tar:
            members = {member.name: member for member in tar.getmembers()}
            for name in BINARIES:
                member = members.get(name)
                if member is None or not member.isreg():
                    raise VerifyError(f"archive lacks regular member {name}")
                if name.startswith("/") or ".." in name.split("/"):
                    raise VerifyError(f"unsafe member path {name}")
                extracted = tar.extractfile(member)
                if extracted is None:
                    raise VerifyError(f"cannot extract member {name}")
                payloads[name] = extracted.read()
    except (OSError, tarfile.TarError) as error:
        raise VerifyError(f"cannot read archive {archive}: {error}") from error
    for name in BINARIES:
        data = payloads[name]
        path = os.path.join(dest, name)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o700)
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        os.chmod(path, 0o700)
        paths.append(path)
    return paths


def _infer_arch(archive: str) -> str:
    base = os.path.basename(archive)
    for arch in ("x86_64", "aarch64"):
        if arch in base:
            return arch
    raise VerifyError(f"cannot infer architecture from archive name {base}")


def _check_linux(paths: list[str], arch: str, forbid: re.Pattern[str] | None) -> None:
    readelf = shutil.which("readelf")
    if readelf is None:
        raise VerifyError("readelf is required on PATH for linux verification")
    for path in paths:
        header = _run([readelf, "-h", path], "elf header")
        machine_lines = [
            line.split(":", 1)[1].strip()
            for line in header.splitlines()
            if line.strip().startswith("Machine:")
        ]
        if machine_lines != [ELF_MACHINES[arch]]:
            raise VerifyError(
                f"{os.path.basename(path)}: expected Machine {ELF_MACHINES[arch]!r},"
                f" got {machine_lines!r}"
            )
        if forbid is not None:
            dynamic = _run([readelf, "-d", path], "elf dynamic")
            needed = [
                line for line in dynamic.splitlines() if "(NEEDED)" in line
            ]
            offenders = [line for line in needed if forbid.search(line)]
            if offenders:
                raise VerifyError(
                    f"{os.path.basename(path)}: forbidden NEEDED entries {offenders!r}"
                )


def _check_macos(
    paths: list[str],
    arch: str,
    minos: str | None,
    forbid: re.Pattern[str] | None,
) -> None:
    for tool in ("file", "otool", "nm"):
        if shutil.which(tool) is None:
            raise VerifyError(f"{tool} is required on PATH for macos verification")
    file_tool = shutil.which("file") or "file"
    otool = shutil.which("otool") or "otool"
    nm = shutil.which("nm") or "nm"
    for path in paths:
        name = os.path.basename(path)
        report = _run([file_tool, "-b", path], "macho file")
        macho = MACHO_ARCHES[arch]
        accepted = (f"Mach-O 64-bit {macho}", f"Mach-O 64-bit executable {macho}")
        if not any(expected in report for expected in accepted):
            raise VerifyError(
                f"{name}: expected one of {accepted!r} in file report: {report!r}"
            )
        if minos is not None:
            listing = _run([otool, "-l", path], "macho load commands")
            blocks = listing.split("Load command")
            minos_values: list[str] = []
            for block in blocks:
                if "LC_BUILD_VERSION" in block:
                    match = re.search(r"\bminos\s+([0-9]+(?:\.[0-9]+)*)", block)
                    if match is None:
                        raise VerifyError(f"{name}: LC_BUILD_VERSION without minos")
                    minos_values.append(match.group(1))
            if not minos_values:
                raise VerifyError(f"{name}: no LC_BUILD_VERSION command found")
            bad = [value for value in minos_values if value != minos]
            if bad:
                raise VerifyError(
                    f"{name}: expected minos {minos}, found {minos_values!r}"
                )
        if forbid is not None:
            libraries = _run([otool, "-L", path], "macho libraries")
            symbols = _run([nm, "-u", path], "macho undefined symbols")
            offenders = [
                line
                for line in (libraries + symbols).splitlines()
                if forbid.search(line)
            ]
            if offenders:
                raise VerifyError(f"{name}: forbidden imports {offenders!r}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", choices=("linux", "macos"), required=True)
    parser.add_argument("--arch", choices=("x86_64", "aarch64"), default=None)
    parser.add_argument("--archive", required=True)
    parser.add_argument("--minos", default=None)
    parser.add_argument("--forbid-import", default=None)
    args = parser.parse_args()

    try:
        arch = args.arch or _infer_arch(args.archive)
        forbid = (
            re.compile(args.forbid_import, re.IGNORECASE)
            if args.forbid_import is not None
            else None
        )
        with tempfile.TemporaryDirectory(prefix="aura-verify-") as temp_dir:
            os.chmod(temp_dir, 0o700)
            paths = _extract(args.archive, temp_dir)
            if args.platform == "linux":
                _check_linux(paths, arch, forbid)
            else:
                _check_macos(paths, arch, args.minos, forbid)
    except VerifyError as error:
        sys.stderr.write(f"verify-release-binary: {error}\n")
        return 1

    sys.stdout.write('{"checks":2,"status":"ok"}\n')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
