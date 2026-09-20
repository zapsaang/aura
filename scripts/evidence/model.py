from __future__ import annotations

import hashlib
import json
import os
import re
from pathlib import Path
from typing import NoReturn

HEX40 = re.compile(r"[0-9a-f]{40}\Z")
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
TAG = re.compile(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")
SAFE_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*\Z")


class EvidenceError(ValueError):
    pass


def fail(message: str) -> NoReturn:
    raise EvidenceError(message)


def canonical_json_bytes(value: object) -> bytes:
    try:
        encoded = json.dumps(
            value,
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
            sort_keys=True,
        )
    except (TypeError, ValueError) as error:
        raise EvidenceError(f"value is not canonical JSON: {error}") from error
    return encoded.encode("utf-8") + b"\n"


def _closed_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def decode_json_bytes(raw: bytes, label: str) -> object:
    try:
        value = json.loads(
            raw,
            object_pairs_hook=_closed_object,
            parse_constant=lambda value: fail(f"non-finite JSON value in {label}: {value}"),
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EvidenceError(f"invalid JSON in {label}: {error}") from error
    if canonical_json_bytes(value) != raw:
        fail(f"non-canonical JSON bytes in {label}")
    return value


def parse_json_bytes(raw: bytes, keys: frozenset[str], label: str) -> dict[str, object]:
    value = decode_json_bytes(raw, label)
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    require_exact_keys(value, keys, label)
    return value


def load_json(path: Path, keys: frozenset[str], label: str) -> dict[str, object]:
    return parse_json_bytes(path.read_bytes(), keys, label)


def require_exact_keys(value: dict[str, object], keys: frozenset[str], label: str) -> None:
    actual = frozenset(value)
    if actual != keys:
        missing = sorted(keys - actual)
        unknown = sorted(actual - keys)
        fail(f"{label} keys differ: missing={missing}, unknown={unknown}")


def require_list(value: object, label: str) -> list[object]:
    if not isinstance(value, list):
        fail(f"{label} must be an array")
    return value


def require_string(value: object, label: str) -> str:
    if not isinstance(value, str):
        fail(f"{label} must be a string")
    return value


def require_uint(value: object, label: str, *, positive: bool = False) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < int(positive):
        fail(f"{label} must be {'positive' if positive else 'nonnegative'} integer")
    return value


def require_enum(value: object, choices: frozenset[str], label: str) -> str:
    text = require_string(value, label)
    if text not in choices:
        fail(f"invalid {label}: {text}")
    return text


def require_hex(value: object, width: int, label: str) -> str:
    text = require_string(value, label)
    pattern = HEX40 if width == 40 else HEX64
    if pattern.fullmatch(text) is None:
        fail(f"{label} must be {width}-lowerhex")
    return text


def require_tag(value: object, label: str = "tag") -> str:
    text = require_string(value, label)
    if TAG.fullmatch(text) is None:
        fail(f"invalid canonical tag: {text}")
    return text


def require_safe_id(value: object, label: str) -> str:
    text = require_string(value, label)
    if SAFE_ID.fullmatch(text) is None:
        fail(f"invalid {label}: {text}")
    return text


def require_sorted_unique(values: list[str], label: str) -> None:
    if values != sorted(values) or len(values) != len(set(values)):
        fail(f"{label} must be sorted and duplicate-free")


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def sha256_fd(descriptor: int) -> str:
    os.lseek(descriptor, 0, os.SEEK_SET)
    digest = hashlib.sha256()
    with os.fdopen(os.dup(descriptor), "rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    os.lseek(descriptor, 0, os.SEEK_SET)
    return digest.hexdigest()


def sha256_file(path: Path) -> str:
    from .secure_file import open_regular_nofollow

    descriptor = open_regular_nofollow(path)
    try:
        return sha256_fd(descriptor)
    finally:
        os.close(descriptor)


def write_new(path: Path, raw: bytes, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
    try:
        with os.fdopen(descriptor, "wb", closefd=False) as target:
            target.write(raw)
            target.flush()
            os.fsync(target.fileno())
    finally:
        os.close(descriptor)
