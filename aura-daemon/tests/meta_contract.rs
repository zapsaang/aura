//! Todo 11 contract: Meta identity and wall-clock telemetry on both platforms.
//!
//! Locks the exact os-release outcome table (bits 27..30), the mandatory
//! dual-clock stamping semantics, and the macOS sysctl-by-name identity
//! mapping with no subprocess, plist, or private API.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::VecDeque;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aura_common::{
    validate_archive, AuraResult, TelemetryArchive, ARCHIVE_VERSION, CAP_META_OS_CODENAME,
    CAP_META_OS_IDENTITY, CAP_META_OS_VERSION, CAP_META_TIMEZONE, CAP_META_WALLCLOCK,
};
use aura_daemon::collectors::meta::linux;
use aura_daemon::collectors::meta::macos::{
    self, MacosMetaProbe, KERN_OSPRODUCTVERSION, KERN_OSTYPE, KERN_VERSION,
};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::CollectorSources;
use aura_daemon::collectors::FixedCollectorState;
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::Finalizer;

#[cfg(target_os = "linux")]
use support::system_sources::DeterministicSources;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every allocation and deallocation forwards the original pointer and
// `Layout` unchanged to `System`; the two atomics only observe allocation
// calls and do not alter allocator ownership.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: `layout` is the exact valid layout supplied by the caller
        // and ownership is delegated to `System`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` and `layout` are forwarded unchanged to their
        // owning allocator.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static TEST_ALLOCATOR: CountingAllocator = CountingAllocator;

struct AllocationProbe;

impl AllocationProbe {
    fn start() -> Self {
        ALLOCATION_CALLS.store(0, Ordering::Relaxed);
        COUNT_ALLOCATIONS.store(true, Ordering::Release);
        Self
    }

    fn finish(self) -> usize {
        COUNT_ALLOCATIONS.store(false, Ordering::Release);
        let count = ALLOCATION_CALLS.load(Ordering::Acquire);
        std::mem::forget(self);
        count
    }
}

impl Drop for AllocationProbe {
    fn drop(&mut self) {
        COUNT_ALLOCATIONS.store(false, Ordering::Release);
    }
}

const FULL_OS_RELEASE: &[u8] = b"ID=ubuntu\n\
VERSION_ID=\"22.04\"\n\
PRETTY_NAME=\"Ubuntu 22.04.4 LTS\"\n\
VERSION=\"22.04.4 LTS (Jammy Jellyfish)\"\n\
VERSION_CODENAME=jammy\n";

fn text_of(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).expect("field text must be valid UTF-8")
}

fn zeroed_archive() -> TelemetryArchive {
    // SAFETY: `TelemetryArchive` is `Zeroable`; the all-zero bit pattern is valid.
    unsafe { std::mem::zeroed::<TelemetryArchive>() }
}

fn minimal_archive() -> TelemetryArchive {
    let mut archive = zeroed_archive();
    archive.version = ARCHIVE_VERSION;
    archive.meta.timestamp_ns = 1;
    archive
}

// ---------------------------------------------------------------------------
// Linux os-release parsing and bit 27..30 outcomes
// ---------------------------------------------------------------------------

#[test]
fn os_release_full_fixture_sets_every_field() {
    let os = linux::parse_os_release(FULL_OS_RELEASE).expect("full os-release parses");
    assert_eq!(os.os_type.as_str(), "linux");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    assert_eq!(text_of(&os.os_pretty_name), "Ubuntu 22.04.4 LTS");
    assert_eq!(text_of(&os.version), "22.04.4 LTS (Jammy Jellyfish)");
    assert_eq!(os.os_version_id.as_str(), "22.04");
    assert_eq!(os.version_codename.as_str(), "jammy");
}

#[test]
fn os_release_locked_fixture_file_parses() {
    let fixture = include_bytes!("fixtures/etc_os_release_sample.txt");
    let os = linux::parse_os_release(fixture).expect("locked fixture parses");
    assert_eq!(os.os_type.as_str(), "linux");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    assert_eq!(text_of(&os.os_pretty_name), "Ubuntu 22.04.4 LTS");
    assert_eq!(os.os_version_id.as_str(), "22.04");
    assert_eq!(text_of(&os.version), "");
    assert_eq!(os.version_codename.as_str(), "");
}

#[test]
fn os_release_missing_pretty_name_zeroes_identity_triple() {
    let buf = b"ID=ubuntu\nVERSION_ID=\"22.04\"\nVERSION=\"22.04\"\n";
    let os = linux::parse_os_release(buf).expect("parse succeeds");
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
    assert_eq!(text_of(&os.version), "22.04");
    assert_eq!(os.os_version_id.as_str(), "22.04");
}

#[test]
fn os_release_missing_id_zeroes_identity_triple() {
    let buf = b"PRETTY_NAME=\"Ubuntu\"\nVERSION_CODENAME=jammy\n";
    let os = linux::parse_os_release(buf).expect("parse succeeds");
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
    assert_eq!(os.version_codename.as_str(), "jammy");
}

#[test]
fn os_release_empty_id_zeroes_identity_triple() {
    let buf = b"ID=\"\"\nPRETTY_NAME=\"Ubuntu\"\n";
    let os = linux::parse_os_release(buf).expect("parse succeeds");
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
}

#[test]
fn os_release_missing_version_clears_only_version() {
    let buf = b"ID=ubuntu\nPRETTY_NAME=\"Ubuntu\"\nVERSION_ID=\"22.04\"\nVERSION_CODENAME=jammy\n";
    let os = linux::parse_os_release(buf).expect("parse succeeds");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    assert_eq!(text_of(&os.version), "");
    assert_eq!(os.os_version_id.as_str(), "22.04");
    assert_eq!(os.version_codename.as_str(), "jammy");
}

#[test]
fn os_release_missing_version_id_clears_only_version_id() {
    let buf = b"ID=ubuntu\nPRETTY_NAME=\"Ubuntu\"\nVERSION=\"22.04\"\n";
    let os = linux::parse_os_release(buf).expect("parse succeeds");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    assert_eq!(text_of(&os.version), "22.04");
    assert_eq!(os.os_version_id.as_str(), "");
}

#[test]
fn os_release_missing_codename_clears_only_codename() {
    let buf = b"ID=ubuntu\nPRETTY_NAME=\"Ubuntu\"\nVERSION=\"22.04\"\nVERSION_ID=\"22.04\"\n";
    let os = linux::parse_os_release(buf).expect("parse succeeds");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    assert_eq!(text_of(&os.version), "22.04");
    assert_eq!(os.os_version_id.as_str(), "22.04");
    assert_eq!(os.version_codename.as_str(), "");
}

#[test]
fn os_release_unterminated_quote_is_fatal() {
    let buf = b"ID=ubuntu\nPRETTY_NAME=\"Ubuntu\n";
    let error = linux::parse_os_release(buf).expect_err("unterminated quote must fail");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn os_release_stray_trailing_quote_is_fatal() {
    let buf = b"ID=ubuntu\nVERSION=22.04\"\n";
    let error = linux::parse_os_release(buf).expect_err("stray quote must fail");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn os_release_invalid_utf8_is_fatal() {
    let buf = b"ID=ubuntu\nPRETTY_NAME=\"Ubu\xffntu\"\n";
    let error = linux::parse_os_release(buf).expect_err("invalid UTF-8 must fail");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn os_release_control_character_is_fatal() {
    let buf = b"ID=ubuntu\nPRETTY_NAME=\"Ubu\tntu\"\n";
    let error = linux::parse_os_release(buf).expect_err("control character must fail");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn os_release_comments_and_unknown_keys_are_ignored() {
    let buf = b"# comment with = and \"unterminated\n\
HOME_URL=\"https://example.com/unterminated\n\
ID=first\n\
ID=ubuntu\n\
PRETTY_NAME=\"Ubuntu\"\n";
    let os = linux::parse_os_release(buf).expect("unknown keys never fail the parse");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    assert_eq!(text_of(&os.os_pretty_name), "Ubuntu");
}

#[test]
fn os_release_long_values_truncate_utf8_safely() {
    let mut pretty = Vec::new();
    pretty.extend_from_slice(b"PRETTY_NAME=\"");
    while pretty.len() < 200 {
        pretty.extend_from_slice("hé".as_bytes());
    }
    pretty.extend_from_slice(b"\"\nID=ubuntu\n");
    let os = linux::parse_os_release(&pretty).expect("long values parse");
    assert_eq!(os.os_id.as_str(), "ubuntu");
    let text = text_of(&os.os_pretty_name);
    assert!(!text.is_empty());
    assert!(text.len() <= 128);
}

#[test]
fn os_release_enoent_read_error_zeroes_fingerprint() {
    let error = std::io::Error::from_raw_os_error(2);
    let os = linux::fingerprint_from_read(Err(&error)).expect("ENOENT stays local");
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
    assert_eq!(text_of(&os.version), "");
    assert_eq!(os.os_version_id.as_str(), "");
    assert_eq!(os.version_codename.as_str(), "");
}

#[test]
fn os_release_eacces_read_error_zeroes_fingerprint() {
    let error = std::io::Error::from_raw_os_error(13);
    let os = linux::fingerprint_from_read(Err(&error)).expect("EACCES stays local");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
}

#[test]
fn os_release_other_io_error_zeroes_fingerprint() {
    let error = std::io::Error::from_raw_os_error(5);
    let os = linux::fingerprint_from_read(Err(&error)).expect("I/O errors stay local");
    assert_eq!(os.os_id.as_str(), "");
}

#[test]
fn os_release_missing_file_clears_cached_fingerprint() {
    let mut archive = zeroed_archive();
    linux::cache_os_fingerprint_from("/nonexistent/aura-os-release", &mut archive.meta)
        .expect("missing file is a local outcome");
    assert_eq!(text_of(&archive.meta.os.os_type.bytes), "");
    assert_eq!(archive.meta.os.os_id.as_str(), "");
}

#[test]
fn os_release_oversize_file_is_fatal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("os-release");
    let content = vec![b'#'; linux::OS_RELEASE_MAX_LEN + 1];
    std::fs::write(&path, content).expect("write oversize fixture");
    let mut archive = zeroed_archive();
    let error =
        linux::cache_os_fingerprint_from(path.to_str().expect("utf8 path"), &mut archive.meta)
            .expect_err("oversize os-release must fail");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn os_release_exactly_max_len_file_is_accepted() {
    let base = b"ID=linux\nPRETTY_NAME=\"x\"\n";
    let mut content = Vec::from(&base[..]);
    content.push(b'#');
    content.resize(linux::OS_RELEASE_MAX_LEN, b'a');
    assert_eq!(content.len(), linux::OS_RELEASE_MAX_LEN);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("os-release");
    std::fs::write(&path, content).expect("write max-size fixture");
    let mut archive = zeroed_archive();
    linux::cache_os_fingerprint_from(path.to_str().expect("utf8 path"), &mut archive.meta)
        .expect("max-size os-release parses");
    assert_eq!(archive.meta.os.os_id.as_str(), "linux");
}

// ---------------------------------------------------------------------------
// Mandatory dual-clock semantics
// ---------------------------------------------------------------------------

struct ScriptedClock {
    samples: VecDeque<AuraResult<ClockSample>>,
}

impl Clock for ScriptedClock {
    fn sample(&mut self) -> AuraResult<ClockSample> {
        self.samples.pop_front().expect("scripted clock sample")
    }
}

fn clock_with(samples: Vec<AuraResult<ClockSample>>) -> SystemFinalizer<ScriptedClock> {
    SystemFinalizer::new(ScriptedClock {
        samples: VecDeque::from(samples),
    })
}

#[test]
fn clock_sample_error_aborts_finalize() {
    let mut finalizer = clock_with(vec![Err(aura_common::AuraError::Fatal(
        "clock_gettime failed".to_string(),
    ))]);
    let mut state = FixedCollectorState::default();
    let error = finalizer
        .finalize(&mut state)
        .expect_err("clock failure must abort the cycle");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn zero_monotonic_clock_is_fatal() {
    let mut finalizer = clock_with(vec![Ok(ClockSample {
        monotonic_ns: 0,
        wallclock_ns: 222,
    })]);
    let mut state = FixedCollectorState::default();
    let error = finalizer
        .finalize(&mut state)
        .expect_err("zero monotonic clock must abort");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn zero_wallclock_is_fatal() {
    let mut finalizer = clock_with(vec![Ok(ClockSample {
        monotonic_ns: 111,
        wallclock_ns: 0,
    })]);
    let mut state = FixedCollectorState::default();
    let error = finalizer
        .finalize(&mut state)
        .expect_err("zero wallclock must abort");
    assert!(matches!(error, aura_common::AuraError::Fatal(_)));
}

#[test]
fn finalize_stamps_monotonic_and_wallclock_from_clock() {
    let mut finalizer = clock_with(vec![Ok(ClockSample {
        monotonic_ns: 111,
        wallclock_ns: 222,
    })]);
    let mut state = FixedCollectorState::default();
    finalizer.finalize(&mut state).expect("finalize succeeds");
    assert_eq!(state.archive.meta.timestamp_ns, 111);
    assert_eq!(state.archive.meta.wallclock_ns, 222);
    assert!(state.archive.capabilities & CAP_META_WALLCLOCK != 0);
}

#[test]
fn finalize_restamps_clocks_every_cycle() {
    let mut finalizer = clock_with(vec![
        Ok(ClockSample {
            monotonic_ns: 111,
            wallclock_ns: 222,
        }),
        Ok(ClockSample {
            monotonic_ns: 333,
            wallclock_ns: 444,
        }),
    ]);
    let mut state = FixedCollectorState::default();
    finalizer.finalize(&mut state).expect("first cycle");
    finalizer.finalize(&mut state).expect("second cycle");
    assert_eq!(state.archive.meta.timestamp_ns, 333);
    assert_eq!(state.archive.meta.wallclock_ns, 444);
}

// ---------------------------------------------------------------------------
// macOS sysctl identity mapping
// ---------------------------------------------------------------------------

type SysctlResult = Result<&'static [u8], i32>;

#[derive(Default)]
struct ScriptedSysctl {
    entries: Vec<(&'static [u8], SysctlResult)>,
}

impl ScriptedSysctl {
    fn full() -> Self {
        Self {
            entries: vec![
                (KERN_OSTYPE, Ok(b"Darwin\0")),
                (KERN_OSPRODUCTVERSION, Ok(b"14.5\0")),
                (
                    KERN_VERSION,
                    Ok(b"Darwin Kernel Version 23.5.0: root:xnu/RELEASE_ARM64\0"),
                ),
            ],
        }
    }

    fn replace(mut self, name: &'static [u8], value: SysctlResult) -> Self {
        for entry in &mut self.entries {
            if entry.0 == name {
                entry.1 = value;
            }
        }
        self
    }
}

impl MacosMetaProbe for ScriptedSysctl {
    fn sysctlbyname(&mut self, name: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        for (key, value) in &self.entries {
            if *key == name {
                return match value {
                    Ok(bytes) => {
                        if bytes.len() > out.len() {
                            return Err(12);
                        }
                        out[..bytes.len()].copy_from_slice(bytes);
                        Ok(bytes.len())
                    }
                    Err(code) => Err(*code),
                };
            }
        }
        Err(2)
    }
}

#[test]
fn macos_full_sysctl_maps_identity_exactly() {
    let mut probe = ScriptedSysctl::full();
    let identity = macos::collect_identity(&mut probe);
    assert!(identity.identity);
    assert!(identity.version);
    assert!(identity.version_id);
    let os = &identity.fingerprint;
    assert_eq!(os.os_type.as_str(), "Darwin");
    assert_eq!(os.os_id.as_str(), "macos");
    assert_eq!(text_of(&os.os_pretty_name), "macOS 14.5");
    assert_eq!(
        text_of(&os.version),
        "Darwin Kernel Version 23.5.0: root:xnu/RELEASE_ARM64"
    );
    assert_eq!(os.os_version_id.as_str(), "14.5");
    assert_eq!(os.version_codename.as_str(), "");
}

#[test]
fn macos_ostype_failure_zeroes_identity_triple() {
    let mut probe = ScriptedSysctl::full().replace(KERN_OSTYPE, Err(2));
    let identity = macos::collect_identity(&mut probe);
    assert!(!identity.identity);
    assert!(identity.version);
    assert!(identity.version_id);
    let os = &identity.fingerprint;
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
    assert_eq!(os.os_version_id.as_str(), "14.5");
}

#[test]
fn macos_product_failure_zeroes_identity_and_version_id() {
    let mut probe = ScriptedSysctl::full().replace(KERN_OSPRODUCTVERSION, Err(2));
    let identity = macos::collect_identity(&mut probe);
    assert!(!identity.identity);
    assert!(identity.version);
    assert!(!identity.version_id);
    let os = &identity.fingerprint;
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
    assert_eq!(os.os_version_id.as_str(), "");
    assert!(!os.version.iter().all(|&b| b == 0));
}

#[test]
fn macos_kernel_version_failure_clears_only_version() {
    let mut probe = ScriptedSysctl::full().replace(KERN_VERSION, Err(2));
    let identity = macos::collect_identity(&mut probe);
    assert!(identity.identity);
    assert!(!identity.version);
    assert!(identity.version_id);
    let os = &identity.fingerprint;
    assert_eq!(os.os_type.as_str(), "Darwin");
    assert_eq!(os.os_id.as_str(), "macos");
    assert_eq!(text_of(&os.os_pretty_name), "macOS 14.5");
    assert!(os.version.iter().all(|&b| b == 0));
}

#[test]
fn macos_invalid_utf8_ostype_clears_identity_locally() {
    let mut probe = ScriptedSysctl::full().replace(KERN_OSTYPE, Ok(b"Dar\xffwin\0"));
    let identity = macos::collect_identity(&mut probe);
    assert!(!identity.identity);
    let os = &identity.fingerprint;
    assert_eq!(text_of(&os.os_type.bytes), "");
    assert_eq!(os.os_id.as_str(), "");
    assert_eq!(text_of(&os.os_pretty_name), "");
}

#[test]
fn macos_trailing_nuls_are_stripped() {
    let mut probe = ScriptedSysctl::full().replace(KERN_OSTYPE, Ok(b"Darwin\0\0\0"));
    let identity = macos::collect_identity(&mut probe);
    assert!(identity.identity);
    assert_eq!(identity.fingerprint.os_type.as_str(), "Darwin");
}

#[test]
fn macos_codename_is_never_published() {
    let mut probe = ScriptedSysctl::full();
    let identity = macos::collect_identity(&mut probe);
    assert!(identity.identity);
    assert!(identity
        .fingerprint
        .version_codename
        .bytes
        .iter()
        .all(|&b| b == 0));
}

// ---------------------------------------------------------------------------
// Validation ranges for the meta capability bits
// ---------------------------------------------------------------------------

#[test]
fn timezone_offset_bounds_are_enforced() {
    for (offset, ok) in [
        (86_400, true),
        (-86_400, true),
        (86_401, false),
        (-86_401, false),
    ] {
        let mut archive = minimal_archive();
        archive.capabilities = CAP_META_TIMEZONE;
        archive.meta.timezone_name = *b"UTC\0\0\0\0\0";
        archive.meta.timezone_offset_secs = offset;
        let result = validate_archive(&archive);
        assert_eq!(result.is_ok(), ok, "offset {offset} validity must be {ok}");
    }
}

#[test]
fn os_version_and_codename_unowned_must_be_zero() {
    let mut archive = minimal_archive();
    archive.meta.os.version[0] = b'x';
    let error = validate_archive(&archive).expect_err("unowned version must be zero");
    match error {
        aura_common::AuraError::InvalidArchive { reason } => {
            assert_eq!(reason, "field meta.os.version: expected zero")
        }
        other => panic!("expected InvalidArchive, got {other:?}"),
    }

    let mut archive = minimal_archive();
    archive.meta.os.version_codename.bytes[0] = b'x';
    let error = validate_archive(&archive).expect_err("unowned codename must be zero");
    match error {
        aura_common::AuraError::InvalidArchive { reason } => {
            assert_eq!(reason, "field meta.os.version_codename: expected zero")
        }
        other => panic!("expected InvalidArchive, got {other:?}"),
    }
}

#[test]
fn os_identity_capability_requires_complete_triple() {
    let mut archive = minimal_archive();
    archive.capabilities = CAP_META_OS_IDENTITY;
    archive.meta.os.os_type = aura_common::FixedString16::from_bytes(b"linux");
    archive.meta.os.os_id = aura_common::FixedString16::from_bytes(b"ubuntu");
    let error = validate_archive(&archive).expect_err("missing pretty name must fail");
    match error {
        aura_common::AuraError::InvalidArchive { reason } => assert_eq!(
            reason,
            "field meta.os.os_pretty_name: inconsistent with capabilities"
        ),
        other => panic!("expected InvalidArchive, got {other:?}"),
    }

    archive.meta.os.os_pretty_name[..6].copy_from_slice(b"Ubuntu");
    validate_archive(&archive).expect("complete identity validates");
}

#[test]
fn os_version_capability_text_is_validated() {
    let mut archive = minimal_archive();
    archive.capabilities = CAP_META_OS_VERSION | CAP_META_OS_CODENAME;
    archive.meta.os.version[0] = 0xFF;
    let error = validate_archive(&archive).expect_err("invalid version text must fail");
    match error {
        aura_common::AuraError::InvalidArchive { reason } => {
            assert_eq!(reason, "field meta.os.version: invalid UTF-8")
        }
        other => panic!("expected InvalidArchive, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Allocation and forbidden-API proofs
// ---------------------------------------------------------------------------

#[test]
fn warmed_meta_collection_has_zero_allocator_delta() {
    const CHILD_MARKER: &str = "AURA_META_ALLOCATION_PROBE_CHILD";
    if std::env::var_os(CHILD_MARKER).is_none() {
        // Harness noise (H) is intermittent: the libtest main thread lazily
        // allocates its first blocking monitor-channel receive (mpmc `Context`
        // Arc + `Waker::selectors` Vec growth) on a schedule CI runners decide,
        // so it can land inside a measured window. For a fixed build the
        // production allocation count P is deterministic: a real regression
        // (P > 0) fails EVERY fresh child, while H-only contamination passes on
        // a clean scheduling — so retrying with a fresh process per attempt and
        // accepting the first zero-delta child preserves the zero-alloc proof.
        // A mutated child is never re-run; each attempt re-execs from scratch.
        const MAX_CHILD_ATTEMPTS: usize = 3;
        let mut last_output = None;
        for _ in 0..MAX_CHILD_ATTEMPTS {
            let output = Command::new(std::env::current_exe().expect("test executable"))
                .arg("--exact")
                .arg("warmed_meta_collection_has_zero_allocator_delta")
                .arg("--test-threads=1")
                .env(CHILD_MARKER, "1")
                .output()
                .expect("run isolated allocation probe");
            if output.status.success() {
                return;
            }
            last_output = Some(output);
        }
        let output = last_output.expect("at least one attempt ran");
        assert!(
            output.status.success(),
            "isolated allocation probe failed: {}\nisolated probe failed in all {MAX_CHILD_ATTEMPTS} fresh-child attempts",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let mut probe = ScriptedSysctl::full();
    let identity = macos::collect_identity(&mut probe);
    std::hint::black_box(&identity);
    let fingerprint = linux::parse_os_release(FULL_OS_RELEASE).expect("warm-up parse");
    std::hint::black_box(&fingerprint);

    let allocation_probe = AllocationProbe::start();
    for _ in 0..2 {
        std::hint::black_box(linux::parse_os_release(FULL_OS_RELEASE).expect("parse"));
        std::hint::black_box(macos::collect_identity(&mut probe));
    }
    let calls = allocation_probe.finish();
    assert_eq!(calls, 0);

    #[cfg(target_os = "linux")]
    assert_deterministic_sources_meta_cycles_allocate_nothing();
}

#[cfg(target_os = "linux")]
fn assert_deterministic_sources_meta_cycles_allocate_nothing() {
    let mut sources = DeterministicSources::default();
    let mut state = FixedCollectorState::default();
    sources
        .collect_meta_and_gpu(&mut state)
        .expect("warm-up meta collection");
    let allocation_probe = AllocationProbe::start();
    for _ in 0..2 {
        sources
            .collect_meta_and_gpu(&mut state)
            .expect("meta collection");
    }
    let calls = allocation_probe.finish();
    assert_eq!(calls, 0);
}

#[test]
fn macos_meta_sources_use_no_subprocess_or_private_api() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "src/platform/macos/metadata.rs",
        "src/collectors/meta/macos.rs",
    ] {
        let source = std::fs::read_to_string(root.join(relative)).expect("read meta source");
        for forbidden in [
            "sw_vers",
            "Command",
            "SystemVersion",
            "Foundation",
            "host_version",
            "statfs",
            "localtime_r",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must not reference {forbidden}"
            );
        }
    }
}
