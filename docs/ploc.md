# PLOC Checker Rules (`scripts/check-rust-loc.py`)

The repository enforces a hard ceiling of **250 production lines of code
(PLOC) per Rust source file**. The checker is deterministic (no toolchain,
no network), runs as `python3 scripts/check-rust-loc.py --root .`, and is
wired into the lint stage of CI and every tip gate. This document is the
normative description of its lexical, counting, and module rules.

## Scope rules

- The walk is repository-wide: every `*.rs` file under `--root`.
- A file is **excluded** when any path component is one of: `.git`,
  `.omo`, `target`, `tests`, `benches`, `fixtures`. Integration tests,
  benchmarks, and fixture crates therefore never count.
- A production source file that is a **symlink is rejected** (error, exit
  1); sources must be regular files.
- Files must be valid UTF-8.

## Lexical rules (what counts as code)

The checker lexes Rust; it does not count text lines.

- `//` line comments are discarded to end of line.
- `/* … */` block comments are discarded, with **nesting**; an
  unterminated block comment is an error, not a miscount.
- String literals `"…"` (with `\` escapes) are blanked; newlines inside
  them are preserved so multi-line strings do not manufacture code lines.
- Raw strings `r"…"`, `r#"…"#`, `br"…"`, `br#"…"#` with any number of
  `#` are blanked the same way (the closing delimiter honors the hash
  count).
- Byte strings `b"…"` follow the normal string path.
- Character literals `'x'`, `'\n'`, `'\''` are blanked; **lifetimes and
  labels (`'a`, `'static`) are not** — a single quote only opens a literal
  when a matching closing quote follows on the same line.
- Everything else is code, including attribute text and macro bodies.

## Counting rules (line granularity)

- A physical line counts as **1 PLOC** when at least one character
  position on it is code after lexing and it is not inside an excluded
  module (below).
- Blank lines, comment-only lines, and lines containing only literal
  content count as 0.
- A final unterminated line (no trailing newline) is still counted.

## Module rules (test-module exclusion)

- A module introduced by `#[cfg(test)]` — optionally followed by further
  attributes and `pub`/`pub(...)` — and its entire brace-matched body are
  excluded from the production count.
- The brace matcher operates on the lexed structure, so braces inside
  strings/comments do not corrupt the span.
- An unterminated `#[cfg(test)]` module is an error.

## Failure and self-test contract

- Any file above 250 PLOC fails the run; every offending file is listed
  with its count, and the exit code is 1.
- `python3 scripts/check-rust-loc.py --self-test --root .` replays the
  12 lexer fixtures under `scripts/fixtures/rust-loc/` against
  `expected.json` and prints exactly `{"checks":12,"status":"ok"}` on
  success. The fixture set is the negative-test corpus for the lexical
  rules; adding a lexer case means adding a fixture and updating the
  expected count (which changes the printed `checks` value by design).

## Splitting guidance

When a file exceeds the ceiling, split by responsibility along module
boundaries (new `mod` file, extractor module, or table-driven data moved
into a dedicated module). Do not game the counter: reformatting code to
evade the lexical rules without a real structural split is a review
defect.
