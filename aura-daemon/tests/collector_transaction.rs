mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
#[cfg(target_os = "linux")]
use std::io;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use aura_common::{AuraError, ARCHIVE_VERSION, CAP_META_WALLCLOCK};
#[cfg(target_os = "linux")]
use aura_common::{TelemetryArchive, CAP_CPU_CONTEXT_SWITCHES};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::cpu::linux::{collect_from_bytes, parse_cpu_stat};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::memory::linux::{
    classify_vmstat_error, parse_meminfo_checked, parse_meminfo_with_availability,
    parse_vmstat_page_faults_checked,
};
#[cfg(target_os = "linux")]
use aura_daemon::collectors::network::linux::parse_net_dev;
use aura_daemon::collectors::{FixedCollectorState, ProviderOutcome};
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::{Finalizer, Heartbeat, Notification};

use support::system_sources::SOURCE_CAPABILITIES;
#[cfg(target_os = "linux")]
use support::transaction::mid_scan_failure_lifecycle;
use support::transaction::{
    allocation_lifecycle, assert_archive_eq, assert_baselines_advanced_once, assert_fixed_state_eq,
    lifecycle, Failure,
};

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: [Category 13 — library contract] every allocation and deallocation
// forwards the original pointer and `Layout` unchanged to `System`; the two
// atomics only observe allocation calls and do not alter allocator ownership.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: [Category 13 — library contract] `layout` is the exact valid
        // layout supplied by the caller and ownership is delegated to `System`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: [Categories 12 and 13 — invalid free and library contract]
        // `pointer` and `layout` are forwarded unchanged to their owning allocator.
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

#[test]
fn collection_module_has_no_commit_without_publication_escape_hatch() {
    let source = include_str!("../src/collectors/mod.rs");
    assert!(
        !source.contains("pub fn collect_all"),
        "collection-only callers must not be able to commit staging"
    );
}

#[test]
fn transaction_fault_support_uses_real_shared_memory() {
    let source = include_str!("support/transaction.rs");
    assert!(
        !source.contains("RecordingPublisher"),
        "transaction rollback tests must observe mmap-backed SHM bytes"
    );
    assert!(source.contains("ShmHandle"));
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_optional_ctxt_does_not_fail_the_global_cpu_sample() {
    let mut cpu = TelemetryArchive::zeroed().cpu;
    let availability = collect_from_bytes(b"cpu 1 2 3 4 5 6 7 8\nctxt nope\n", &mut cpu)
        .expect("optional ctxt must degrade locally");
    assert!(!availability.context_switches);
    assert_eq!(availability.capability_mask() & CAP_CPU_CONTEXT_SWITCHES, 0);
    assert_eq!(cpu.context_switches, 0);
    assert_eq!(cpu.context_switches_per_sec, 0.0);
}

#[test]
#[cfg(target_os = "linux")]
fn absent_optional_ctxt_clears_its_capability_and_owned_bytes() {
    let mut cpu = TelemetryArchive::zeroed().cpu;
    cpu.context_switches = 91;
    cpu.context_switches_per_sec = 7.0;
    let availability = collect_from_bytes(b"cpu 1 2 3 4 5 6 7 8\n", &mut cpu)
        .expect("global CPU sample remains available");
    assert!(!availability.context_switches);
    assert_eq!(availability.capability_mask() & CAP_CPU_CONTEXT_SWITCHES, 0);
    assert_eq!(cpu.context_switches, 0);
    assert_eq!(cpu.context_switches_per_sec, 0.0);
}

#[test]
#[cfg(target_os = "linux")]
fn valid_optional_ctxt_sets_its_capability_and_owned_value() {
    let mut cpu = TelemetryArchive::zeroed().cpu;
    let availability =
        collect_from_bytes(b"cpu 1 2 3 4 5 6 7 8\nctxt 91\n", &mut cpu).expect("global CPU sample");
    assert!(availability.context_switches);
    assert_ne!(availability.capability_mask() & CAP_CPU_CONTEXT_SWITCHES, 0);
    assert_eq!(cpu.context_switches, 91);
}

#[test]
#[cfg(target_os = "linux")]
fn cpu_core_cap_path_has_no_post_warmup_warning_state() {
    let source = include_str!("../src/collectors/cpu/linux.rs");
    assert!(!source.contains("CORE_LIMIT_WARNED"));
    assert!(!source.contains("warn!("));
}

#[test]
#[cfg(target_os = "linux")]
fn collection_sample_finalizes_staging_without_advancing_committed_state() {
    let mut state = aura_daemon::collectors::CollectorState::new();
    aura_daemon::collectors::init(&mut state).expect("initialize collectors");
    let committed_before = state.committed().clone();
    let sample = aura_daemon::collectors::collect_sample(&mut state).expect("collect sample");
    assert_eq!(sample.archive.version, ARCHIVE_VERSION);
    assert_eq!(sample.archive.checksum, sample.archive.calculate_checksum());
    assert_fixed_state_eq(state.committed(), &committed_before);
}

struct RecordingClock<'a> {
    calls: &'a Cell<usize>,
    sample: ClockSample,
}

impl Clock for RecordingClock<'_> {
    fn sample(&mut self) -> aura_common::AuraResult<ClockSample> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.sample)
    }
}

#[test]
fn finalization_samples_clocks_and_seals_the_archive_once() {
    // Given: a staging archive containing unsupported-domain residue.
    let calls = Cell::new(0);
    let mut state = FixedCollectorState::default();
    state.archive.process.total = 9;
    state.archive.storage.disk_count = 1;
    state.archive.derived.ram_used_percent = 42.0;
    let mut finalizer = SystemFinalizer::new(RecordingClock {
        calls: &calls,
        sample: ClockSample {
            monotonic_ns: 11,
            wallclock_ns: 17,
        },
    });

    // When: the cycle is finalized.
    finalizer.finalize(&mut state).expect("finalize archive");

    // Then: clocks, ownership zeroing, version, and checksum are applied once.
    assert_eq!(calls.get(), 1);
    assert_eq!(state.archive.meta.timestamp_ns, 11);
    assert_eq!(state.archive.meta.wallclock_ns, 17);
    assert_ne!(state.archive.capabilities & CAP_META_WALLCLOCK, 0);
    assert_eq!(state.archive.process.total, 0);
    assert_eq!(state.archive.storage.disk_count, 0);
    assert_eq!(state.archive.derived.ram_used_percent, 0.0);
    assert_eq!(state.archive.version, ARCHIVE_VERSION);
    assert_eq!(state.archive.checksum, state.archive.calculate_checksum());
}

#[test]
fn invalid_clock_sample_leaves_staging_unchanged() {
    // Given: staging state and a clock returning an invalid zero monotonic sample.
    let calls = Cell::new(0);
    let mut state = FixedCollectorState::default();
    state.archive.meta.uptime_secs = 23;
    let before_checksum = state.archive.calculate_checksum();
    let mut finalizer = SystemFinalizer::new(RecordingClock {
        calls: &calls,
        sample: ClockSample {
            monotonic_ns: 0,
            wallclock_ns: 17,
        },
    });

    // When: finalization rejects the invalid sample.
    let result = finalizer.finalize(&mut state);

    // Then: no archive bytes are changed.
    assert!(result.is_err());
    assert_eq!(calls.get(), 1);
    assert_eq!(state.archive.meta.uptime_secs, 23);
    assert_eq!(state.archive.version, 0);
    assert_eq!(state.archive.capabilities, 0);
    assert_eq!(state.archive.calculate_checksum(), before_checksum);
    assert_eq!(state.baselines.prev_timestamp_ns, 0);
}

#[test]
fn collector_failure_preserves_committed_state_and_publication() {
    let mut lifecycle = lifecycle(Failure::Collector, None);
    let committed_before = lifecycle.state().committed().clone();
    let published_before = lifecycle.publisher().published();
    let shm_before = lifecycle.publisher().raw_bytes();
    let error = lifecycle.cycle().expect_err("collector failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_fixed_state_eq(lifecycle.state().committed(), &committed_before);
    assert_eq!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_archive_eq(&lifecycle.publisher().published(), &published_before);
    assert_eq!(lifecycle.publisher().calls, 0);
}

#[test]
#[cfg(target_os = "linux")]
fn mid_scan_directory_failure_commits_no_process_generation_or_rehash() {
    let mut lifecycle = mid_scan_failure_lifecycle();
    let committed_before = lifecycle.state().committed().clone();
    let generation_before = committed_before.baselines.process.generation();

    let error = lifecycle.cycle().expect_err("mid-scan directory failure");

    assert!(matches!(error, AuraError::Fatal(_)));
    assert_fixed_state_eq(lifecycle.state().committed(), &committed_before);
    assert_eq!(
        lifecycle.state().committed().baselines.process.generation(),
        generation_before
    );
    assert!(
        lifecycle
            .state()
            .committed()
            .baselines
            .process
            .get(&(7, 7))
            .is_none(),
        "partially observed identity was not committed"
    );
}

#[test]
fn finalization_failure_preserves_committed_state_and_publication() {
    let mut lifecycle = lifecycle(Failure::Finalizer, None);
    let committed_before = lifecycle.state().committed().clone();
    let published_before = lifecycle.publisher().published();
    let shm_before = lifecycle.publisher().raw_bytes();
    let error = lifecycle.cycle().expect_err("finalizer failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_fixed_state_eq(lifecycle.state().committed(), &committed_before);
    assert_eq!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_archive_eq(&lifecycle.publisher().published(), &published_before);
    assert_eq!(lifecycle.publisher().calls, 0);
}

#[test]
fn publication_failure_preserves_committed_state_and_old_bytes() {
    let mut lifecycle = lifecycle(Failure::Publisher, None);
    let committed_before = lifecycle.state().committed().clone();
    let published_before = lifecycle.publisher().published();
    let shm_before = lifecycle.publisher().raw_bytes();
    let error = lifecycle.cycle().expect_err("publisher failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_fixed_state_eq(lifecycle.state().committed(), &committed_before);
    assert_eq!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_archive_eq(&lifecycle.publisher().published(), &published_before);
    assert_eq!(lifecycle.publisher().calls, 1);
}

#[test]
fn ready_failure_retains_the_advanced_commit_and_published_bytes() {
    let mut lifecycle = lifecycle(Failure::None, Some(1));
    let committed_before = lifecycle.state().committed().clone();
    let shm_before = lifecycle.publisher().raw_bytes();
    let error = lifecycle.cycle().expect_err("READY failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_baselines_advanced_once(lifecycle.state().committed(), &committed_before);
    assert_ne!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_archive_eq(
        &lifecycle.publisher().published(),
        &lifecycle.state().committed().archive,
    );
    assert_eq!(
        lifecycle.notifier().notifications[0],
        Some(Notification::Ready)
    );
}

#[test]
fn watchdog_failure_retains_the_second_successful_publication() {
    let mut lifecycle = lifecycle(Failure::None, Some(2));
    lifecycle.cycle().expect("READY cycle");
    let committed_before = lifecycle.state().committed().clone();
    let shm_before = lifecycle.publisher().raw_bytes();
    let error = lifecycle.cycle().expect_err("WATCHDOG failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_baselines_advanced_once(lifecycle.state().committed(), &committed_before);
    assert_ne!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_archive_eq(
        &lifecycle.publisher().published(),
        &lifecycle.state().committed().archive,
    );
    assert_eq!(
        lifecycle.notifier().notifications[1],
        Some(Notification::Watchdog)
    );
}

#[test]
fn notification_failure_exits_without_another_collection() {
    let mut lifecycle = lifecycle(Failure::None, Some(1));
    let heartbeat = Heartbeat::from_millis(5).expect("positive heartbeat");
    let shutdown = AtomicBool::new(false);
    let error = lifecycle
        .run(heartbeat, &shutdown)
        .expect_err("notification failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_eq!(lifecycle.collector().calls, 2);
}

#[test]
fn warmup_commits_without_publication_or_notification() {
    let mut lifecycle = lifecycle(Failure::None, None);
    let committed_before = lifecycle.state().committed().clone();
    let shm_before = lifecycle.publisher().raw_bytes();
    let heartbeat = Heartbeat::from_millis(9).expect("positive heartbeat");
    lifecycle.warm_up(heartbeat).expect("warm-up");
    assert_baselines_advanced_once(lifecycle.state().committed(), &committed_before);
    assert_eq!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_eq!(lifecycle.publisher().calls, 0);
    assert_eq!(lifecycle.notifier().calls, 0);
    assert_eq!(lifecycle.sleeper().durations[0], Duration::from_millis(9));
}

#[test]
fn fatal_warmup_preserves_committed_state_and_does_not_sleep() {
    let mut lifecycle = lifecycle(Failure::Collector, None);
    let committed_before = lifecycle.state().committed().clone();
    let shm_before = lifecycle.publisher().raw_bytes();
    let heartbeat = Heartbeat::from_millis(9).expect("positive heartbeat");
    let error = lifecycle.warm_up(heartbeat).expect_err("warm-up failure");
    assert!(matches!(error, AuraError::Fatal(_)));
    assert_fixed_state_eq(lifecycle.state().committed(), &committed_before);
    assert_eq!(lifecycle.publisher().raw_bytes(), shm_before);
    assert_eq!(lifecycle.sleeper().calls, 0);
}

#[test]
fn first_published_cycle_starts_from_the_warmed_commit() {
    let mut lifecycle = lifecycle(Failure::None, None);
    let heartbeat = Heartbeat::from_millis(1).expect("positive heartbeat");
    lifecycle.warm_up(heartbeat).expect("warm-up");
    lifecycle.cycle().expect("first published cycle");
    assert_eq!(lifecycle.collector().seen_baselines[..2], [10, 11]);
}

#[test]
fn unavailable_cycle_still_finalizes_and_publishes() {
    let mut lifecycle = lifecycle(Failure::None, None);
    lifecycle.collector_mut().unavailable = true;
    lifecycle.cycle().expect("unavailable is local");
    assert_eq!(lifecycle.publisher().calls, 1);
    assert_eq!(lifecycle.finalizer().calls, 1);
}

#[test]
fn scratch_buffers_keep_their_initial_capacities_across_cycles() {
    let mut lifecycle = lifecycle(Failure::None, None);
    let capacities = lifecycle.state().scratch_capacities();
    lifecycle.cycle().expect("first cycle");
    lifecycle.cycle().expect("second cycle");
    assert_eq!(lifecycle.state().scratch_capacities(), capacities);
}

#[test]
fn counting_allocator_detects_a_transient_heap_allocation() {
    let probe = AllocationProbe::start();
    let allocation = std::hint::black_box(Vec::<u8>::with_capacity(1));
    let calls = probe.finish();
    drop(allocation);
    assert!(calls > 0);
}

#[test]
fn warmed_production_lifecycle_has_zero_allocator_delta() {
    const CHILD_MARKER: &str = "AURA_ALLOCATION_PROBE_CHILD";
    if std::env::var_os(CHILD_MARKER).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("warmed_production_lifecycle_has_zero_allocator_delta")
            .arg("--test-threads=1")
            .env(CHILD_MARKER, "1")
            .output()
            .expect("run isolated allocation probe");
        assert!(
            output.status.success(),
            "isolated allocation probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let mut lifecycle = allocation_lifecycle();
    let heartbeat = Heartbeat::from_millis(1).expect("positive heartbeat");
    lifecycle.warm_up(heartbeat).expect("warm-up");
    let capacities = lifecycle.state().scratch_capacities();
    let probe = AllocationProbe::start();
    lifecycle.cycle().expect("first measured cycle");
    lifecycle.cycle().expect("second measured cycle");
    let calls = probe.finish();
    assert_eq!(calls, 0);
    assert_eq!(lifecycle.state().scratch_capacities(), capacities);
    assert_eq!(lifecycle.collector().sources().calls, [3; 6]);
    assert_eq!(
        lifecycle.state().committed().archive.capabilities & SOURCE_CAPABILITIES,
        SOURCE_CAPABILITIES
    );
}

#[test]
#[cfg(target_os = "linux")]
fn vmstat_enoent_is_page_fault_local_unavailable() {
    let outcome = classify_vmstat_error(io::Error::from_raw_os_error(libc::ENOENT));
    assert!(matches!(outcome, ProviderOutcome::Unavailable));
}

#[test]
#[cfg(target_os = "linux")]
fn vmstat_eacces_is_page_fault_local_unavailable() {
    let outcome = classify_vmstat_error(io::Error::from_raw_os_error(libc::EACCES));
    assert!(matches!(outcome, ProviderOutcome::Unavailable));
}

#[test]
#[cfg(target_os = "linux")]
fn vmstat_eio_is_fatal() {
    let outcome = classify_vmstat_error(io::Error::from_raw_os_error(libc::EIO));
    assert!(matches!(outcome, ProviderOutcome::Fatal(_)));
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_vmstat_counter_is_fatal() {
    let outcome = parse_vmstat_page_faults_checked(b"pgfault nope\n");
    assert!(matches!(outcome, ProviderOutcome::Fatal(_)));
}

#[test]
#[cfg(target_os = "linux")]
fn missing_global_cpu_row_is_fatal() {
    assert!(parse_cpu_stat(b"ctxt 7\n").is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_network_headers_are_fatal() {
    let mut network = TelemetryArchive::zeroed().network;
    assert!(parse_net_dev(b"bad\nheaders\neth0: 1 0 0 0 0 0 0 0 2\n", &mut network).is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn malformed_interface_is_skipped_and_marks_truncation() {
    let mut network = TelemetryArchive::zeroed().network;
    let sample = b"Inter-| Receive | Transmit\n face |bytes |bytes\n eth0: nope 0 0 0 0 0 0 0 2\n";
    parse_net_dev(sample, &mut network).expect("valid global payload");
    assert_eq!(network.if_count, 0);
    assert_eq!(network.truncated, 1);
}

#[test]
#[cfg(target_os = "linux")]
fn interface_row_without_a_separator_marks_truncation() {
    // Given: valid network headers followed by a malformed interface record.
    let sample = b"Inter-| Receive | Transmit\n face |bytes |bytes\n malformed row\n";
    let mut network = TelemetryArchive::zeroed().network;

    // When: the global network payload is parsed.
    parse_net_dev(sample, &mut network).expect("valid global payload");

    // Then: the individual record is skipped and truncation records the loss.
    assert_eq!(network.if_count, 0);
    assert_eq!(network.truncated, 1);
}

#[test]
#[cfg(target_os = "linux")]
fn missing_mandatory_meminfo_field_is_fatal() {
    assert!(parse_meminfo_checked(b"MemFree: 3 kB\n").is_err());
}

#[test]
#[cfg(target_os = "linux")]
fn absent_optional_meminfo_keys_are_locally_unavailable() {
    // Given: valid mandatory memory counters without optional counters.
    let sample = b"MemTotal: 10 kB\nMemFree: 3 kB\n";

    // When: the global memory sample is parsed.
    let (stats, availability) = parse_meminfo_with_availability(sample).expect("core memory");

    // Then: mandatory memory remains available and optional values stay unowned.
    assert_eq!(stats.ram_total, 10 * 1024);
    assert_eq!(stats.ram_free, 3 * 1024);
    assert!(!availability.buffers);
    assert!(!availability.cached);
    assert!(!availability.swap);
}
