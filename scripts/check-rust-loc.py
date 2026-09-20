import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path


MAX_PLOC = 250
EXCLUDED_PARTS = frozenset({".git", ".omo", "target", "tests", "benches", "fixtures"})
RAW_PREFIX = re.compile(r'(?:br|r)(?P<hashes>#+)?"')
TEST_MODULE = re.compile(
    r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*"
    r"(?:#\s*\[[^\]]*\]\s*)*"
    r"(?:(?:pub(?:\s*\([^)]*\))?)\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{"
)


class LocError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class LexedSource:
    structural: str
    code_mask: tuple[bool, ...]


def _char_literal_end(source: str, start: int) -> int | None:
    pos = start + 1
    if pos >= len(source) or source[pos] == "\n":
        return None
    if source[pos] != "\\":
        end = pos + 1
        return end + 1 if end < len(source) and source[end] == "'" else None
    pos += 1
    while pos < len(source) and source[pos] != "\n":
        if source[pos] == "'" and source[pos - 1] != "\\":
            return pos + 1
        pos += 1
    return None


def _mark_literal(
    source: str,
    structural: list[str],
    code_mask: list[bool],
    start: int,
    end: int,
) -> None:
    for index in range(start, end):
        structural[index] = "\n" if source[index] == "\n" else " "
        code_mask[index] = True


def lex_rust(source: str) -> LexedSource:
    structural = [" "] * len(source)
    code_mask = [False] * len(source)
    pos = 0
    block_depth = 0
    while pos < len(source):
        if block_depth:
            if source.startswith("/*", pos):
                block_depth += 1
                pos += 2
            elif source.startswith("*/", pos):
                block_depth -= 1
                pos += 2
            else:
                structural[pos] = "\n" if source[pos] == "\n" else " "
                pos += 1
            continue
        if source.startswith("//", pos):
            end = source.find("\n", pos)
            pos = len(source) if end < 0 else end
            continue
        if source.startswith("/*", pos):
            block_depth = 1
            pos += 2
            continue
        raw = RAW_PREFIX.match(source, pos)
        if raw:
            hashes = raw.group("hashes") or ""
            close = '"' + hashes
            end = source.find(close, raw.end())
            end = len(source) if end < 0 else end + len(close)
            _mark_literal(source, structural, code_mask, pos, end)
            pos = end
            continue
        if source[pos] == '"':
            end = pos + 1
            while end < len(source):
                if source[end] == "\\":
                    end += 2
                    continue
                end += 1
                if source[end - 1] == '"':
                    break
            _mark_literal(source, structural, code_mask, pos, min(end, len(source)))
            pos = min(end, len(source))
            continue
        if source[pos] == "'":
            end = _char_literal_end(source, pos)
            if end is not None:
                _mark_literal(source, structural, code_mask, pos, end)
                pos = end
                continue
        char = source[pos]
        structural[pos] = char
        if not char.isspace():
            code_mask[pos] = True
        pos += 1
    if block_depth:
        raise LocError("unterminated block comment")
    return LexedSource("".join(structural), tuple(code_mask))


def _test_module_ranges(structural: str) -> list[tuple[int, int]]:
    ranges = []
    for match in TEST_MODULE.finditer(structural):
        opening = structural.rfind("{", match.start(), match.end())
        depth = 0
        closing = None
        for pos in range(opening, len(structural)):
            if structural[pos] == "{":
                depth += 1
            elif structural[pos] == "}":
                depth -= 1
                if depth == 0:
                    closing = pos + 1
                    break
        if closing is None:
            raise LocError("unterminated #[cfg(test)] module")
        ranges.append((match.start(), closing))
    return ranges


def count_production_ploc(source: str) -> int:
    lexed = lex_rust(source)
    excluded = [False] * len(source)
    for start, end in _test_module_ranges(lexed.structural):
        excluded[start:end] = [True] * (end - start)
    count = 0
    offset = 0
    for line in source.splitlines(keepends=True):
        end = offset + len(line)
        if any(
            lexed.code_mask[pos] and not excluded[pos]
            for pos in range(offset, end)
        ):
            count += 1
        offset = end
    if offset < len(source) and any(
        lexed.code_mask[pos] and not excluded[pos]
        for pos in range(offset, len(source))
    ):
        count += 1
    return count


def production_rust_files(root: Path) -> list[Path]:
    files = []
    for path in root.rglob("*.rs"):
        relative = path.relative_to(root)
        if any(part in EXCLUDED_PARTS for part in relative.parts):
            continue
        if path.is_symlink():
            raise LocError(f"production Rust source is a symlink: {relative}")
        if path.is_file():
            files.append(path)
    return sorted(files)


def check_workspace(root: Path) -> None:
    failures = []
    for path in production_rust_files(root):
        relative = path.relative_to(root)
        try:
            source = path.read_text(encoding="utf-8")
            ploc = count_production_ploc(source)
        except (OSError, UnicodeError, LocError) as error:
            raise LocError(f"{relative}: {error}") from error
        if ploc > MAX_PLOC:
            failures.append(f"{relative}: {ploc} production PLOC exceeds {MAX_PLOC}")
    if failures:
        raise LocError("\n".join(failures))


def run_self_tests() -> None:
    fixture_dir = Path(__file__).resolve().parent / "fixtures" / "rust-loc"
    expected_path = fixture_dir / "expected.json"
    expected = json.loads(expected_path.read_text(encoding="utf-8"))
    if not isinstance(expected, dict) or len(expected) != 12:
        raise LocError("self-test fixture registry must contain exactly 12 cases")
    for name in sorted(expected):
        wanted = expected[name]
        if not isinstance(name, str) or not isinstance(wanted, int):
            raise LocError("invalid self-test fixture registry entry")
        source = (fixture_dir / name).read_text(encoding="utf-8")
        actual = count_production_ploc(source)
        if actual != wanted:
            raise LocError(f"self-test {name}: expected {wanted}, got {actual}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    try:
        root = args.root.resolve(strict=True)
        if args.self_test:
            run_self_tests()
            sys.stdout.write('{"checks":12,"status":"ok"}\n')
        else:
            check_workspace(root)
    except (OSError, UnicodeError, json.JSONDecodeError, LocError) as error:
        sys.stderr.write(f"check-rust-loc: {error}\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
