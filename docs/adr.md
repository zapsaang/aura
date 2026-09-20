# AURA Architecture Decision Records

These records lock the architectural decisions produced by the design
compliance remediation (audit `docs/design-compliance-audit-2026-08-30.md`,
remediation plan executed 2026-09). Each entry is normative: code that
contradicts a locked decision is a defect, and changing a decision requires
a new ADR that supersedes the old one.

---

## ADR-001: Telemetry ABI v2

**Status:** locked

**Context.** The audited tree carried an evolving archive layout with no
explicit version negotiation. Process and Storage dimensions existed in the
original design (`docs/project.md`), were removed by `4c419fc` before the
audit, and the audit confirmed them missing at runtime (AUD-003).

**Decision.** The shared-memory payload is `TelemetryArchive` at
`ARCHIVE_VERSION = 2`. ABI v2 fixes: a per-dimension capability-bit table,
daemon-computed `DerivedStats`, restored Process and Storage dimensions as
first-class fields, and a compile-time bound of 65536 bytes per buffer
(`BUFFER_SIZE`). Integrity is enforced by `validate_archive` plus a CRC32
checksum stored beside each buffer.

**Consequences.** Readers and writers compiled against different versions
cannot interoperate; see ADR-009. Every new dimension must allocate a
capability bit and stay inside the 65536-byte bound.

---

## ADR-002: Per-buffer SeqLock double buffer

**Status:** locked (supersedes the single global counter in the original
`docs/tech_blueprint.md`)

**Context.** The original blueprint specified one global sequence counter.
`87e5e74` replaced it because a single counter produces false contention:
a reader of the inactive buffer was forced to retry on writes that could
not affect it.

**Decision.** The shared-memory header is 24 bytes: an 8-byte
`active_index` plus an independent `seq[2]` array of `AtomicU64`, one
timeline per 65536-byte buffer (`SHM_SIZE = 131096`). Odd sequence means a
write is in flight on that buffer; even means the buffer is consistent.
Writers alternate buffers and never touch the buffer a reader may be
validating; readers snapshot the active buffer's sequence, copy, and
re-validate, cross-checked by the archive CRC32.

**Consequences.** Readers of a stable buffer never spin on unrelated
writes. A rollback to a single counter is explicitly rejected (audit
section 8: "不推荐的回退").

---

## ADR-003: Read deadlines — 10 ms admission, 100 ms spin ceiling

**Status:** locked

**Context.** AUD-009 required bounded, wall-clock-based reader retries
instead of unbounded spinning or fixed retry counts.

**Decision.** A new SeqLock read attempt may only begin while total elapsed
time is below `SEQLOCK_RETRY_ADMISSION_MS = 10` ms; the absolute ceiling of
any read operation is `MAX_SPIN_WAIT_MS = 100` ms, after which the CLI
classifies the daemon as unreachable. Separately, data older than
`OFFLINE_THRESHOLD_SECS = 2.0` s (or a future/invalid timestamp) is the
stale-data offline class. Both classes render as `[AURA: OFFLINE]` with
exit code 1 — never a panic.

**Consequences.** Worst-case CLI latency is bounded by wall clock and is
independent of scheduler load. The CLI never blocks the writer.

---

## ADR-004: Shared-memory permissions `0600` in `0700` directories

**Status:** locked

**Context.** The audited tree created the state file with mode `0o666`
(AUD-013), exposing telemetry to every local user.

**Decision.** The state leaf is created with `SHM_FILE_MODE = 0o600`
(owner-only). It lives in a per-user runtime directory that is itself
`0700` and euid-owned: `/run/user/<euid>/aura/` on Linux (falling back to
`/tmp/aura-<euid>/`), `/private/tmp/aura-<euid>/` on macOS. All path
resolution is fd-relative (`openat`-style component walk, never
validate-then-reopen); the only sticky-rooted exceptions are `/tmp` and
`/private/tmp` (mode 01777). Default leaves are `state.dat` and
`state.lock`; the daemon holds the lock leaf for its lifetime.

**Consequences.** Cross-user telemetry is unsupported by design. Operator
overrides (`--shm-path`) must name a file inside an existing euid-owned
`0700` directory and inherit the same validation chain.

---

## ADR-005: CRC32 scope — integrity only, never authentication

**Status:** locked

**Context.** `TelemetryArchive` carries a CRC32 (`crc32fast`) checksum.
AUD-013 noted this detects only accidental corruption and could be mistaken
for a security control.

**Decision.** The checksum's scope is exactly accidental local corruption
(torn writes, bit flips, mismatched layouts). It is not, and must never be
represented as, authentication or tamper resistance; the threat model is
non-malicious local corruption. Confidentiality and access control are
provided exclusively by the filesystem permissions of ADR-004.

**Consequences.** A checksum failure is classified as corruption evidence
(`[AURA: ERROR - ...]` with expected/actual hex), never silently
auto-corrected and never downgraded to the offline class.

---

## ADR-006: macOS collectors use public APIs only

**Status:** locked

**Context.** Complete macOS telemetry can be tempting to source from
private frameworks, which breaks on OS updates and violates App Store /
notarization policy.

**Decision.** The macOS capability table is built exclusively from
documented public interfaces: `host_statistics`/`host_processor_info`
(Mach), `sysctl`/`sysctlbyname`, `proc_pidinfo` and related libsystem
calls, `getifaddrs`/`PF_ROUTE`, and IOKit public entry points. Capabilities
that have no public source are reported as unsupported via capability bits
rather than faked. GPU telemetry on macOS is unsupported (ADR-007).

**Consequences.** The supported-on-macOS set is explicit in the capability
table; consumers must test bits, not platform names.

---

## ADR-007: Linux NVML via runtime loading; no NVML on Apple

**Status:** locked

**Context.** The audited tree linked NVML at build time behind the
`gpu-nvml` feature, which made release binaries fail to start on machines
without NVIDIA drivers.

**Decision.** On Linux, the daemon loads `libnvidia-ml.so.1` at runtime
(dynamic loading). Absence of the library, initialization failure, or
per-cycle errors degrade gracefully: GPU capabilities clear and the rest
of the archive keeps publishing. Release and Home Manager Linux builds
enable the `gpu-nvml` feature; Apple builds never link or load NVML and
report GPU as unsupported.

**Consequences.** Linux release artifacts run identically on GPU and
non-GPU hosts. The release verifier asserts no `NEEDED` entry matching
`nvidia-ml|nvml` in shipped ELF binaries.

---

## ADR-008: Daemon-side derivation; CLI is presentation-only

**Status:** locked

**Context.** The audit (AUD-011) found the CLI recomputing percentages,
aggregates, and color thresholds, violating the zero-computation
philosophy of `docs/project.md`.

**Decision.** All arithmetic — utilization percentages, per-dimension
aggregates, threshold tones (`TONE_*`) — is computed once per heartbeat by
the daemon into `DerivedStats`. The CLI maps capability bits and derived
fields to strings (human, value, JSON) and performs no floating-point
derivation of its own. Missing prerequisites render as `N/A` per field
(compound tokens keep their labels).

**Consequences.** Every renderer (human, value, JSON, terminal) presents
identical numbers by construction. New presentation logic must not add
derivation to the CLI.

---

## ADR-009: No ABI v1 compatibility

**Status:** locked

**Context.** With ABI v2 locked (ADR-001), a compatibility shim for the
pre-remediation layout was considered and rejected.

**Decision.** There is no v1 reader, writer, or migration path. A reader
that encounters any version other than 2 fails closed with
`ABI version mismatch: expected 2, found <n>` and the ERROR exit contract.
Version skew is resolved by upgrading both binaries together (they ship in
the same archive and formula).

**Consequences.** The version field is a hard gate, not a negotiation.
Mixed-version deployments are a supported failure mode with a precise,
tested error message.

---

## History note: Process and Storage dimensions

Present in the original design, removed by `4c419fc` before the audit
(confirmed missing by AUD-003), and restored during remediation as
first-class ABI v2 dimensions with Linux collectors and capability-gated
macOS semantics. Their removal and restoration are recorded here so the
audit trail (`4c419fc` → remediation) remains interpretable without
archaeology.
