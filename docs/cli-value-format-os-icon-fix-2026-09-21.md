# CLI `value` format OS icon fix plan

**Status:** approved and implemented with the catalog decision in section 2.
**Fact base:** current HEAD `f9b8e8e` and direct inspection of `6d67c28`,
`cfb3b18`, and `0abb161` on 2026-09-21.

## 1. Verified problem and scope

Current `aura-cli/src/output/value.rs::os_row` emits
`os=<wallclock_ns>` when `CAP_META_WALLCLOCK` is set. That is a category
error: wall-clock ownership says nothing about OS identity, and the value is
not an OS value.

The smallest correct change is limited to value output:

- Gate the OS value on `CAP_META_OS_IDENTITY`.
- Emit `os=<icon>` when identity is present, and `os=N/A` otherwise.
- Preserve `value::render(module, telemetry)` and its callers.
- Keep JSON, the ABI, daemon collection, color handling, and human output
  unchanged.

This scope is deliberate. Current `--color` routing applies only to human
rendering; neither the README nor the historical value renderer establishes a
promise that `key=value` data carries terminal escape sequences. Threading
the default ANSI color mode through value output would be a silent contract
change. A fixed green icon also conveys no telemetry state and cannot make a
font-missing glyph visible.

`aura-cli/src/output/AGENTS.md` is untracked in this checkout. It is stale,
but cannot be attributed to `0abb161` from repository history and is not part
of this product fix.

## 2. Selected product decision: `cfb3b18` Font Logos catalog

Do not claim that one historical implementation is being restored. The
history contains incompatible behaviors:

- `6d67c28` renders `<icon> <pretty-name>` for value output and uses a
  supplementary-private-use icon catalog.
- `cfb3b18`, the last icon implementation before `0abb161`, renders a pure
  icon but uses a different BMP catalog and a Tux fallback.
- `0abb161` introduced the current `key=value` value-format contract and the
  erroneous wall-clock OS row.

The selected source is exactly the Font Logos pure-icon catalog from commit
`cfb3b18`. No glyph from `6d67c28` is used. The preserved mappings are Ubuntu
`f31b`, Debian `f306`, Arch `f303`, Fedora `f30a`, RHEL `f316`, CentOS `f304`,
Rocky `f32b`, Alma `f31d`, openSUSE `f314`, Gentoo `f30d`, Alpine `f300`, NixOS
`f313`, Void `f322`, Linux Mint (`linuxmint` and `mint`) `f30e`, Manjaro `f312`,
EndeavourOS `f323`, Pop!_OS (`pop` and `pop_os`) `f32a`, Zorin `f32f`, Kali
`f327`, Raspbian `f315`, and Amazon Linux (`amzn`) `f270`.

The product deliberately extends that catalog with common distributions that
have distinct installed Nerd Font Font Logos definitions: openSUSE Leap
`f37e`, openSUSE Tumbleweed `f37d`, Solus `f32d`, AOSC `f301`, ArchLabs `f31e`,
Artix `f31f`, Devuan `f307`, elementary `f309`, Mageia `f310`, Mandriva `f311`,
Sabayon `f317`, Slackware `f318`, Deepin `f321`, Guix `f325`, Parrot `f329`,
Garuda `f335`, Kubuntu `f333`, KDE neon `f331`, Nobara `f380`, OpenWrt `f382`,
CoreOS `f305`, and CachyOS `f385`. This extension is intentional and is not
presented as a byte-for-byte historical restoration.

Apple uses `f302` for Darwin. The explicit fallback is the Font Logos Tux glyph
`f31a`; it applies to `ol`, `oracle`, `flatcar`, `container-linux`, `clearlinux`,
`photon`, and every unknown ID. The grouped production `match` keeps aliases
visible and avoids a second source of truth.

Regardless of catalog, macOS support is mandatory. The current producer emits
`os_type="Darwin"` and `os_id="macos"`; matching only lowercase `"darwin"`
is incorrect. The helper must recognize the producer's actual Darwin shape
(for example with an ASCII-case-insensitive Darwin check) before falling back
to the Linux/unknown mapping.

## 3. Design

Add a narrowly visible helper in `aura-cli/src/output/meta.rs`:

```rust
pub(super) fn os_icon(os_id: &str, os_type: &str) -> &'static str
```

It owns only the selected catalog and platform dispatch. It does not format
labels, pretty names, color, or capability checks. Keep the grouped `match`
in this module because the mapping is meta-domain data and `value.rs` is its
sole consumer after this fix; do not create an `icons.rs` module for one
helper.

Change `value.rs::os_row` to:

1. test `CAP_META_OS_IDENTITY`;
2. return `os=N/A` when clear;
3. otherwise return `format!("os={}", os_icon(...))`.

The archive validator guarantees that an archive with this capability has
non-empty `os_type`, `os_id`, and `os_pretty_name`. Do not add a recovery path
or test for identity-set plus empty `os_id`: it is an invalid archive and the
reader rejects it before rendering.

## 4. TDD sequence

All red tests are placed in `aura-cli/tests/output_contract.rs`, so they
compile against the current public renderer before production code changes.

1. **Write the red contracts.**
   - Replace `value_os_exact` with the selected Ubuntu icon byte sequence.
   - Rename `value_os_na_when_wallclock_clear` to
     `value_os_na_when_identity_clear`; clear `CAP_META_OS_IDENTITY` and
     expect `os=N/A`.
   - Add `value_os_macos_identity_exact` using the real producer shape
     (`os_type="Darwin"`, `os_id="macos"`) and the selected Apple icon.
   - Update `value_all_exact_eight_rows` to populate valid Linux identity
     fields and expect the selected Ubuntu icon.
   - Add one binary test for `-m os --format value --color none`. Its fixture
     must set `meta.timestamp_ns = monotonic_ns()` before `write_shm`, and it
     must assert exactly `os=<selected-ubuntu-icon>\n` with success and empty
     stderr.

2. **Run the focused test target and observe assertion failures.**

   ```bash
   cargo test -p aura-cli --test output_contract -- value_os
   ```

   Existing source compiles; current OS assertions fail because it emits the
   wall-clock value. The macOS assertion also prevents a lowercase-only Darwin
   implementation from passing.

3. **Implement the smallest change.** Add `os_icon` with the selected grouped
   catalog, then change only `os_row` to use identity gating and the helper.
   Do not change the value renderer signature, `lib.rs`, `color.rs`, or
   `meta::render`.

4. **Add focused helper tests in `meta.rs`.** Test at least Ubuntu, the exact
   real Darwin/macos pair, and the selected unknown-ID fallback. These tests
   use independent expected literals, not the implementation's catalog data.
   Run the focused helper and output-contract tests green.

5. **Run the full validation set.**

   ```bash
   cargo test -p aura-cli --locked
   cargo test --workspace --locked
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   python3 scripts/check-rust-loc.py --root .
   ```

The test-local `value(module, telemetry)` helper has 26 call sites and remains
unchanged; the production `value::render` signature also remains unchanged.
No mechanical signature ripple is needed.

## 5. Acceptance criteria

1. With valid Ubuntu identity, value output is exactly
   `os=<selected-ubuntu-icon>` without a trailing newline from the renderer.
2. With `CAP_META_OS_IDENTITY` clear, it is exactly `os=N/A`.
3. A valid macOS archive (`Darwin` / `macos`) uses the selected Apple icon.
4. The CLI binary adds exactly one final newline and no terminal escapes when
   invoked with `--format value --color none`.
5. CPU and other value rows remain byte-for-byte unchanged.
6. JSON, human output, shared-memory ABI, and daemon code are unchanged.
7. The selected catalog and fallback are documented as a deliberate product
   decision, with no claims of byte-identical restoration unless demonstrated
   against one named commit.
8. All commands in section 4 pass, and each changed Rust source remains under
   the repository's 250-PLOC limit.

## 6. Review record

### Round 1 corrections

- Removed the unsupported claim that color applies to every output format.
- Removed human-renderer, `AGENTS.md`, and color-threading scope creep.
- Replaced the table-driven hybrid catalog with a single-source, grouped-match
  decision.
- Removed impossible empty-identity fixtures and duplicate tests.
- Replaced active-tree `git stash` / `git checkout` verification with direct
  historical inspection; it cannot test new files and risks a dirty checkout.

### Round 2 corrections

- Corrected the historical claim: `6d67c28` did not emit a pure value icon.
- Corrected catalog compatibility claims: its distro mappings cannot be mixed
  with `cfb3b18` while calling the result byte-identical.
- Added the real current macOS producer shape (`Darwin`, `macos`).
- Corrected TDD ordering so the first red tests compile before any signature
  change, and removed the invalid BMP-only private-use-area test.
