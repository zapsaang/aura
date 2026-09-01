#!/usr/bin/env python3
"""Offline Cargo.lock v3 validator: descriptor-only, never invokes Cargo, never writes."""

import argparse
import hashlib
import json
import os
import stat
import sys
import tomllib


def fail(message):
    print(f"verify-cargo-lock-v3: {message}", file=sys.stderr)
    sys.exit(1)


def open_no_follow(path):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    except OSError as exc:
        fail(f"cannot open {path}: {exc}")
    return fd


def read_regular(path):
    fd = open_no_follow(path)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode):
            fail(f"{path} is not a regular file")
        with os.fdopen(os.dup(fd), "rb") as handle:
            data = handle.read()
        return data, info
    finally:
        os.close(fd)


def parse_version_requirement(name, declaration, source):
    if isinstance(declaration, str):
        return ("version", declaration)
    if isinstance(declaration, dict):
        if declaration.get("workspace") is True:
            return None
        version = declaration.get("version")
        if version is not None:
            return ("version", version)
        if declaration.get("path") is not None:
            return ("path", declaration["path"])
        fail(f"{source}: dependency {name} lacks a version requirement")
    fail(f"{source}: dependency {name} has unsupported declaration form")


def collect_requirements(workspace):
    requirements = []
    root_manifest = os.path.join(workspace, "Cargo.toml")
    root_bytes, _ = read_regular(root_manifest)
    try:
        root = tomllib.loads(root_bytes.decode("utf-8"))
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as exc:
        fail(f"{root_manifest}: {exc}")

    workspace_deps = root.get("workspace", {}).get("dependencies", {})
    for name, declaration in workspace_deps.items():
        parsed = parse_version_requirement(name, declaration, "workspace.dependencies")
        if parsed is None:
            fail(f"workspace.dependencies: {name} cannot itself be workspace-inherited")
        requirements.append((name, *parsed))

    members = root.get("workspace", {}).get("members", [])
    if not members:
        fail("workspace declares no members")
    for member in members:
        manifest_path = os.path.join(workspace, member, "Cargo.toml")
        manifest_bytes, _ = read_regular(manifest_path)
        try:
            manifest = tomllib.loads(manifest_bytes.decode("utf-8"))
        except (tomllib.TOMLDecodeError, UnicodeDecodeError) as exc:
            fail(f"{manifest_path}: {exc}")
        sections = ["dependencies", "dev-dependencies", "build-dependencies"]
        for section in sections:
            for name, declaration in manifest.get(section, {}).items():
                parsed = parse_version_requirement(name, declaration, f"{member}:{section}")
                if parsed is not None:
                    requirements.append((name, *parsed))
        for target in manifest.get("target", {}).values():
            for section in sections:
                for name, declaration in target.get(section, {}).items():
                    parsed = parse_version_requirement(
                        name, declaration, f"{member}:target:{section}"
                    )
                    if parsed is not None:
                        requirements.append((name, *parsed))
    return requirements


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--lock", required=True)
    parser.add_argument("--require-package", required=True)
    args = parser.parse_args()

    if "=" not in args.require_package:
        fail("--require-package must be name=version")
    required_name, required_version = args.require_package.split("=", 1)

    lock_path = args.lock
    first_bytes, first_stat = read_regular(lock_path)
    first_hash = hashlib.sha256(first_bytes).hexdigest()

    try:
        lock = tomllib.loads(first_bytes.decode("utf-8"))
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as exc:
        fail(f"{lock_path}: {exc}")

    # Check 2: lock format version is exactly 3.
    if lock.get("version") != 3:
        fail(f"lock format version is {lock.get('version')!r}, expected 3")

    packages = lock.get("package", [])
    by_name_version = {}
    for package in packages:
        name = package.get("name")
        version = package.get("version")
        if name is None or version is None:
            fail("lock contains a package entry without name/version")
        by_name_version.setdefault((name, version), []).append(package)

    # Check 3: every workspace direct requirement is an exact =version pin with a
    # matching selected package/version entry (checksum required for registry sources).
    # Workspace-internal path deps carry no version requirement; they must still
    # resolve to a selected non-registry package entry.
    requirements = collect_requirements(args.workspace)
    for name, kind, value in requirements:
        if kind == "path":
            entries = [p for p in packages if p.get("name") == name]
            if not entries:
                fail(f"no selected package matches path dependency {name}")
            for entry in entries:
                entry_source = entry.get("source")
                if entry_source and "registry+" in entry_source:
                    fail(f"path dependency {name} resolved to a registry package")
            continue
        if not value.startswith("="):
            fail(f"requirement for {name} is {value!r}, not a literal =version pin")
        pinned = value[1:]
        matches = by_name_version.get((name, pinned))
        if not matches:
            fail(f"no selected package matches {name}={pinned}")
        for match in matches:
            source = match.get("source")
            if source and "registry+" in source and not match.get("checksum"):
                fail(f"selected {name} {pinned} lacks a registry checksum")

    # Check 4: exactly one selected required package at the exact pinned version.
    selected = [p for p in packages if p.get("name") == required_name]
    if len(selected) != 1 or selected[0].get("version") != required_version:
        versions = [p.get("version") for p in selected]
        fail(
            f"expected exactly one {required_name} at {required_version}, found {versions}"
        )

    # Check 1: the lock is a regular stable-read file whose bytes and metadata do
    # not change across validation.
    second_bytes, second_stat = read_regular(lock_path)
    if hashlib.sha256(second_bytes).hexdigest() != first_hash:
        fail("lock bytes changed during validation")
    if (first_stat.st_ino, first_stat.st_size, first_stat.st_mtime_ns) != (
        second_stat.st_ino,
        second_stat.st_size,
        second_stat.st_mtime_ns,
    ):
        fail("lock metadata changed during validation")

    sys.stdout.write('{"checks":4,"status":"ok"}\n')


if __name__ == "__main__":
    main()
