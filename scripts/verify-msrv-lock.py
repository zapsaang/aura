#!/usr/bin/env python3
"""Offline MSRV lock validator: parses Cargo's sparse index cache, never invokes Cargo."""

import argparse
import glob
import json
import os
import stat
import sys
import tomllib


def fail(message):
    print(f"verify-msrv-lock: {message}", file=sys.stderr)
    sys.exit(1)


def read_lock(path):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    except OSError as exc:
        fail(f"cannot open {path}: {exc}")
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode):
            fail(f"{path} is not a regular file")
        with os.fdopen(os.dup(fd), "rb") as handle:
            return handle.read()
    finally:
        os.close(fd)


def cache_shard(name):
    lowered = name.lower()
    if len(lowered) == 1:
        return os.path.join("1", lowered)
    if len(lowered) == 2:
        return os.path.join("2", lowered)
    if len(lowered) == 3:
        return os.path.join("3", lowered[0], lowered)
    return os.path.join(lowered[0:2], lowered[2:4], lowered)


def parse_rust_version(text):
    parts = text.split(".")
    if not parts or not parts[0].isdigit():
        fail(f"unparseable rust_version {text!r}")
    major = int(parts[0])
    minor = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0
    return major, minor


def version_exceeds(text, max_version):
    return parse_rust_version(text) > parse_rust_version(max_version)


def find_cache_files(name):
    home = os.environ.get("HOME")
    if not home:
        fail("HOME is not set")
    pattern = os.path.join(home, ".cargo", "registry", "index", "*", ".cache")
    files = []
    for cache_dir in glob.glob(pattern):
        candidate = os.path.join(cache_dir, cache_shard(name))
        if os.path.isfile(candidate):
            files.append(candidate)
    return files


def rust_version_of(name, version, cache_path):
    try:
        fd = os.open(cache_path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    except OSError as exc:
        fail(f"cannot open index cache {cache_path}: {exc}")
    try:
        with os.fdopen(fd, "rb") as handle:
            data = handle.read()
    except OSError as exc:
        fail(f"cannot read index cache {cache_path}: {exc}")
    found = None
    for field in data.split(b"\0"):
        if not field.startswith(b"{"):
            continue
        try:
            entry = json.loads(field.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            continue
        if entry.get("name") == name and entry.get("vers") == version:
            found = entry.get("rust_version")
    return found


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--lock", required=True)
    parser.add_argument("--max-rust-version", required=True)
    parser.add_argument("--require-version", required=True, type=int)
    args = parser.parse_args()

    lock_bytes = read_lock(args.lock)
    try:
        lock = tomllib.loads(lock_bytes.decode("utf-8"))
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as exc:
        fail(f"{args.lock}: {exc}")

    if lock.get("version") != args.require_version:
        fail(
            f"lock format version is {lock.get('version')!r}, "
            f"expected {args.require_version}"
        )

    checked = 0
    for package in lock.get("package", []):
        source = package.get("source", "")
        if "registry+" not in source:
            continue
        name = package.get("name")
        version = package.get("version")
        if name is None or version is None:
            fail("lock contains a registry package without name/version")
        cache_files = find_cache_files(name)
        if not cache_files:
            fail(f"no sparse index cache entry for {name}")
        rust_version = None
        for cache_path in cache_files:
            candidate = rust_version_of(name, version, cache_path)
            if candidate is not None:
                rust_version = candidate
                break
        if rust_version and version_exceeds(rust_version, args.max_rust_version):
            fail(
                f"{name} {version} declares rust-version {rust_version} "
                f"> {args.max_rust_version}"
            )
        checked += 1

    sys.stdout.write(f'{{"packages":{checked},"status":"ok"}}\n')


if __name__ == "__main__":
    main()
