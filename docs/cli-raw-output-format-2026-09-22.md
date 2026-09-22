# CLI `raw` output format plan

**Status:** implemented 2026-09-22; all section 3 gates pass.
**Fact base:** current working tree at `d3c0099`, inspected on 2026-09-22.
The target document and several local `AGENTS.md` files are untracked; this
plan describes the current source tree, not an assertion about committed
history.

## 1. Verified problem and scope

`--format value` is an established, byte-exact `key=value` contract. Its
single-module rows and `all` output are covered by integration and binary
tests. It is useful for labelled status-bar tokens, but it does not provide a
bare value for callers that already know the selected module.

Add a fourth output format, `raw`, with the following contract:

- `raw` accepts exactly one telemetry module: `cpu`, `process`, `mem`, `swap`,
  `disk`, `net`, `os`, or `gpu`.
- `raw` with `all`, including the default module when `-m` is omitted, is a
  user-input error. The CLI exits 1, writes the normal `[AURA: ERROR - ...]`
  line to stdout, and writes nothing to stderr.
- For a supported single module, the output is the corresponding value-format
  row with only its outer module key and `=` removed. Thus scalar rows are
  bare values or `N/A`; disk remains `read=.../s,write=.../s`; network remains
  `rx=.../s,tx=.../s`.
- Capability ownership, required non-empty arrays, decimal SI formatting, the
  OS icon mapping, and the literal `N/A` sentinel are identical to value
  output. `--color` remains irrelevant to both formats.
- The daemon, archive ABI, collectors, JSON, human rendering, and existing
  value-format bytes remain unchanged.

The error must be classified as an argument error, not `Fatal`, and must be
validated by `aura_cli::run` before constructing `TelemetryReader`. This
makes the error independent of shared-memory availability and preserves the
offline/error split in `main.rs`.

## 2. Design

### 2.1 Public CLI surface

Add `OutputFormat::Raw` to the existing clap value enum. Clap then exposes the
lowercase, case-sensitive spelling in help consistently with the current
formats. Add `AuraError::InvalidArgument(String)` with display text
`invalid argument: {0}`.

At the first line of `aura_cli::run`, reject `Raw + All` with:

```text
--format raw requires a single module (got --module all)
```

Only after that guard may the function choose a shared-memory path or construct
a `TelemetryReader`.

### 2.2 One source of row semantics

Do not create a line-for-line `output/raw.rs` copy of `output/value.rs`. That
would duplicate all eight capability predicates, row ordering, SI calls, and
future fixes.

Instead, refactor the value renderer around one private row formatter with a
small internal key mode (`labelled` or `bare`). Keep the existing public
`output::value::render(Module, &TelemetryArchive) -> String` signature and its
byte-for-byte output. Add a `pub(crate)` raw entry point used only by `lib.rs`;
it uses the bare mode and returns `AuraResult<String>` so
`Module::All` is an ordinary `InvalidArgument` error even if a future caller
bypasses `run`'s guard. No `unreachable!()` or panic is permitted: the README
states that the CLI never panics, and defense at this library boundary costs no
extra I/O or allocation cycle.

This keeps labels as rendering policy, not duplicated metric logic. Each row
still allocates its final `String` exactly as the current value renderer does;
there is no collector hot path involved and no new dependency.

### 2.3 Deliberate non-changes

- Do not reinterpret `--format value` for a single module; existing consumers
  rely on its label.
- Do not use a clap `ArgGroup`: the relationship is conditional (`raw` only
  rejects `all`) and the application owns the stable error presentation.
- Do not add raw to `scripts/smoke-release.py`'s current `format x color x
  -m all` matrix. That matrix requires every selected format to accept `all`;
  raw intentionally does not. Instead, add a separate packaged-binary raw
  loop for `-m cpu` across the existing color modes. It must assert exit 0,
  empty stderr, and exactly one non-empty output line. This retains the
  all-module matrix while ensuring the release artifact exercises every format.
- Do not update untracked, stale local `AGENTS.md` content as an incidental
  feature change. If that file is to be maintained, repair it comprehensively
  in a separately owned documentation change rather than adding one misleading
  row to an already inaccurate table.

## 3. TDD sequence

The first red tests must compile before `Raw`, a raw renderer, or
`InvalidArgument` exists. Therefore begin with black-box integration tests that
invoke `CARGO_BIN_EXE_aura-cli` using literal command-line strings, rather than
tests that import proposed Rust symbols.

1. **Write executable red contracts first.** In
   `aura-cli/tests/output_contract.rs`, add binary tests backed by the existing
   shared-memory fixture helpers. Immediately before every `write_shm` call,
   set `t.meta.timestamp_ns = monotonic_ns()`; the archive builders use a stale
   fixed timestamp and `write_shm` does not refresh it.
   - A table-driven supported-data test covering all eight modules and asserting
     exact stdout, one trailing newline, empty stderr, and exit 0.
   - A table-driven unsupported/empty-prerequisite test covering every distinct
     value-row predicate, asserting exactly `N/A\n` where the bare contract
     requires it.
   In `aura-cli/tests/error_contract.rs`, add a two-case literal-argument
   table for both `--format raw` and `--format raw -m all`, with no shared-memory
   setup. Each case asserts exit 1, empty stderr, and the complete normal error
   line on stdout.

   These tests compile against the current binary. Before implementation, the
   lowercase raw requests fail because clap does not recognize `raw`; their
   required assertions therefore fail rather than becoming compile errors. Keep
   `--format Raw` as a separate, intentionally green baseline regression: it
   already fails under clap's current case-sensitive parser and is not evidence
   of a red-first raw implementation test.

2. **Observe the red result.** Run only the new binary tests. Record that they
   fail for the missing format, while the existing `value_*` and binary-value
   contracts still pass unchanged.

3. **Implement the minimum production surface.** Add the error variant, the
   enum variant, the pre-reader validation, and the shared labelled/bare row
   formatter. Do not alter existing value expectations or introduce a second
   renderer copy.

4. **Add white-box regression tests only after the public red contracts are
   green.** Unit-test the private formatter in its owning module for the eight
   exact bare rows, representative unavailable cases, and the defensive
   `Module::All` error. These tests prevent future changes to the shared helper
   from making value and raw drift, without exposing an implementation-only API
   to integration tests.

5. **Run the focused and full suites.**

```bash
cargo test -p aura-cli --test output_contract -- raw_
cargo test -p aura-cli --test error_contract -- raw_
cargo test -p aura-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
python3 scripts/check-rust-loc.py --root .
```

The PLOC check applies to production Rust files and excludes `tests` paths.

## 4. Acceptance criteria

1. `--format raw -m <single-module>` emits exactly the documented bare row,
   with the binary adding one final newline, exit 0, and empty stderr.
2. Every bare row uses exactly the same capability predicate and numeric/icon
   formatter as its labelled value counterpart; value output remains
   byte-for-byte unchanged.
3. `--format raw` and `--format raw -m all` fail before shared-memory access,
   exit 1, use the normal stdout error framing, and include the exact argument
   message in section 2.1.
4. `--format Raw` remains invalid according to clap's existing case-sensitive
   enum policy.
5. The implementation contains no `unreachable!()` or panic path for raw
   dispatch, no duplicated eight-row renderer, no added dependency, and no
   collector/ABI/JSON/human/color change.
6. The release smoke retains its documented all-module matrix and adds a
   separate packaged-binary raw smoke loop for `-m cpu` across every color mode.
   Rust binary tests cover the full raw module and error contracts.
7. All commands in section 3 exit zero after implementation. Existing tests
   are not deleted or weakened.
8. README CLI usage documents `aura-cli --format raw -m cpu`, states that raw
   requires a single module, and preserves the existing module aliases.

## 5. Audit record

### Round 1: fact correction

The prior draft was not reliable as a fact record. Its commit identifier did
not match current HEAD, its asserted test counts and function names did not
match the present integration test, and several absolute line references were
already stale. The current `value::render` has eight labelled rows plus an
`all` join; its unavailable coverage contains more cases than one-per-module,
because empty arrays and missing prerequisite capabilities are independently
meaningful. This plan therefore uses current symbol names and behavioral
contracts, not fragile line ranges or inflated counts.

The prior README example also omitted a module despite raw rejecting the
default `all`; any future README update must show `-m <module>`. The existing
output-directory AGENTS file is untracked and materially inconsistent with the
current module topology, so it is not treated as reliable implementation
evidence or patched piecemeal here.

### Round 2: TDD and debt correction

The prior draft called tests that import `OutputFormat::Raw` and
`output::raw::render` “red-first”; before production code those tests do not
compile, so they cannot demonstrate a behavioral red state. This revision
uses binary contracts first and confines private-helper tests to the later
regression layer.

The prior design also proposed a second full renderer and an `unreachable!()`
fallback. The former creates predictable value/raw drift; the latter conflicts
with the project’s explicit no-panic CLI contract and aborts in release builds.
The shared formatter plus an ordinary defensive error removes both sources of
technical debt while retaining a strict public rejection rule.

The second review also tightened release and boundary coverage. Raw must not be
inserted into the existing all-module smoke matrix, but omitting it from the
packaged release smoke entirely would contradict that script's every-format
claim; a separate single-module loop is therefore required. The raw renderer
entry point is explicitly `pub(crate)`, the two syntactic forms of `Raw + All`
are independently tested in the error-contract suite, and every binary fixture
refreshes its timestamp. Finally, README coverage is required rather than
optional, and uppercase `Raw` is recorded as a pre-existing green parser
regression, not a new red test.
