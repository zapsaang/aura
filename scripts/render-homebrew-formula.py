#!/usr/bin/env python3
"""Render dist/homebrew/aura.rb from deployment/homebrew/aura.rb.in.

Pure offline substitution: replaces {SOURCE_REPOSITORY}, {TAG}, and the four
{SHA256_*} placeholders with validated runtime inputs and writes the result
byte-deterministically (same inputs always produce the same bytes).
Fails on any missing or unresolved placeholder; performs no network access.
"""
import argparse
import os
import re
import sys

sys.dont_write_bytecode = True

TAG_PATTERN = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
SHA_PATTERN = re.compile(r"^[0-9a-f]{64}$")
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
PLACEHOLDER_PATTERN = re.compile(r"\{[A-Z0-9_]+\}")
TEMPLATE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "deployment",
    "homebrew",
    "aura.rb.in",
)


class RenderError(RuntimeError):
    pass


def _validated(
    tag: str,
    shas: dict[str, str],
    source_repository: str,
) -> dict[str, str]:
    if TAG_PATTERN.fullmatch(tag) is None:
        raise RenderError(f"tag {tag!r} is not a canonical vMAJOR.MINOR.PATCH tag")
    if REPOSITORY_PATTERN.fullmatch(source_repository) is None:
        raise RenderError(f"invalid source repository: {source_repository!r}")
    values = {"SOURCE_REPOSITORY": source_repository, "TAG": tag}
    for key, value in shas.items():
        if SHA_PATTERN.fullmatch(value) is None:
            raise RenderError(f"{key} is not 64 lowercase hex characters")
        values[f"SHA256_{key}"] = value
    return values


def render(template_text: str, values: dict[str, str]) -> str:
    def substitute(match: re.Match[str]) -> str:
        name = match.group(0)[1:-1]
        if name not in values:
            raise RenderError(f"missing value for placeholder {{{name}}}")
        return values[name]

    rendered = PLACEHOLDER_PATTERN.sub(substitute, template_text)
    leftover = PLACEHOLDER_PATTERN.findall(rendered)
    if leftover:
        raise RenderError(f"unresolved placeholders remain: {sorted(leftover)!r}")
    return rendered


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", required=True)
    parser.add_argument("--source-repository", default="zapsaang/aura")
    parser.add_argument("--linux-x86", required=True)
    parser.add_argument("--linux-arm", required=True)
    parser.add_argument("--macos-arm", required=True)
    parser.add_argument("--macos-x86", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    try:
        values = _validated(
            args.tag,
            {
                "LINUX_X86": args.linux_x86,
                "LINUX_ARM": args.linux_arm,
                "MACOS_ARM": args.macos_arm,
                "MACOS_X86": args.macos_x86,
            },
            args.source_repository,
        )
        with open(TEMPLATE, "r", encoding="utf-8", newline="") as handle:
            template_text = handle.read()
        rendered = render(template_text, values)
        out_dir = os.path.dirname(args.out)
        if out_dir:
            os.makedirs(out_dir, exist_ok=True)
        with open(args.out, "w", encoding="utf-8", newline="") as handle:
            handle.write(rendered)
    except (OSError, RenderError) as error:
        sys.stderr.write(f"render-homebrew-formula: {error}\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
