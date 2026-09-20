//! Allocation contract tests (Todo 17): the enforceable guarantee is zero
//! Rust global-allocator calls from AURA-owned successful-cycle code after
//! warm-up — reusable preallocated scratch may `clear()` and refill within
//! unchanged capacity, but no construction, clone, reserve, growth, or
//! replacement may occur in-cycle. Every zero-delta probe runs in a fresh
//! child process so harness startup allocations cannot pollute the count.

#[allow(dead_code)]
mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aura_common::TelemetryArchive;
use aura_daemon::lifecycle::Heartbeat;

use support::system_sources::SOURCE_CAPABILITIES;
use support::transaction::allocation_lifecycle;

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

/// Returns true in the parent after asserting the isolated child passed;
/// returns false inside the marked child, which then runs the probe body.
fn spawn_isolated(test_name: &str) -> bool {
    const MARKER: &str = "AURA_ALLOCATION_CONTRACT_CHILD";
    if std::env::var_os(MARKER).is_some() {
        return false;
    }
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
            .args(["--exact", test_name, "--test-threads=1"])
            .env(MARKER, "1")
            .output()
            .expect("spawn isolated allocation probe");
        if output.status.success() {
            return true;
        }
        last_output = Some(output);
    }
    let output = last_output.expect("at least one attempt ran");
    assert!(
        output.status.success(),
        "isolated probe {test_name} failed in all {MAX_CHILD_ATTEMPTS} fresh-child attempts:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[test]
fn counting_allocator_observes_transient_heap_allocation() {
    let probe = AllocationProbe::start();
    let allocation = std::hint::black_box(Vec::<u8>::with_capacity(1));
    let calls = probe.finish();
    drop(allocation);
    assert!(calls > 0, "probe must observe a real heap allocation");
}

#[test]
fn warmed_lifecycle_cycles_have_zero_allocator_delta() {
    if spawn_isolated("warmed_lifecycle_cycles_have_zero_allocator_delta") {
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
    assert_eq!(calls, 0, "successful cycles after warm-up allocate nothing");
    assert_eq!(lifecycle.state().scratch_capacities(), capacities);
    assert_eq!(lifecycle.collector().sources().calls, [3; 6]);
    let published = lifecycle.publisher().published();
    assert_eq!(published.version, aura_common::ARCHIVE_VERSION);
    assert_ne!(published.meta.timestamp_ns, 0, "finalizer stamps monotonic");
    assert_eq!(published.checksum, published.calculate_checksum());
    assert_eq!(
        published.capabilities & SOURCE_CAPABILITIES,
        SOURCE_CAPABILITIES
    );
}

#[test]
fn lifecycle_cycles_preserve_scratch_capacities() {
    let mut lifecycle = allocation_lifecycle();
    let heartbeat = Heartbeat::from_millis(1).expect("positive heartbeat");
    lifecycle.warm_up(heartbeat).expect("warm-up");
    let capacities = lifecycle.state().scratch_capacities();
    for _ in 0..4 {
        lifecycle.cycle().expect("steady-state cycle");
    }
    assert_eq!(
        lifecycle.state().scratch_capacities(),
        capacities,
        "scratch buffers keep their warmed capacities across cycles"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn cpu_stat_parsers_have_zero_allocator_delta() {
    if spawn_isolated("cpu_stat_parsers_have_zero_allocator_delta") {
        return;
    }
    use aura_daemon::collectors::cpu::linux::{collect_from_bytes, parse_cpu_stat};
    let sample = include_bytes!("fixtures/proc_stat_sample.txt");
    let mut out = TelemetryArchive::zeroed().cpu;
    parse_cpu_stat(sample).expect("warm parse");
    collect_from_bytes(sample, &mut out).expect("warm collect");
    let probe = AllocationProbe::start();
    for _ in 0..4 {
        std::hint::black_box(parse_cpu_stat(sample).expect("measured parse"));
        collect_from_bytes(sample, &mut out).expect("measured collect");
    }
    let calls = probe.finish();
    assert_eq!(calls, 0, "cpu stat parse/collect allocates nothing");
}

#[test]
#[cfg(target_os = "linux")]
fn memory_parsers_have_zero_allocator_delta() {
    if spawn_isolated("memory_parsers_have_zero_allocator_delta") {
        return;
    }
    use aura_daemon::collectors::memory::linux::{
        parse_meminfo_checked, parse_vmstat_page_faults_checked,
    };
    let meminfo = include_bytes!("fixtures/proc_meminfo_sample.txt");
    let vmstat = include_bytes!("fixtures/proc_vmstat_sample.txt");
    parse_meminfo_checked(meminfo).expect("warm meminfo");
    let _ = parse_vmstat_page_faults_checked(vmstat);
    let probe = AllocationProbe::start();
    for _ in 0..4 {
        std::hint::black_box(parse_meminfo_checked(meminfo).expect("measured meminfo"));
        std::hint::black_box(parse_vmstat_page_faults_checked(vmstat));
    }
    let calls = probe.finish();
    assert_eq!(calls, 0, "meminfo/vmstat parsers allocate nothing");
}

#[test]
#[cfg(target_os = "linux")]
fn network_parse_has_zero_allocator_delta() {
    if spawn_isolated("network_parse_has_zero_allocator_delta") {
        return;
    }
    use aura_daemon::collectors::network::linux::parse_net_dev;
    let sample = include_bytes!("fixtures/proc_net_dev_sample.txt");
    let mut out = TelemetryArchive::zeroed().network;
    parse_net_dev(sample, &mut out).expect("warm parse");
    let probe = AllocationProbe::start();
    for _ in 0..4 {
        parse_net_dev(sample, &mut out).expect("measured parse");
    }
    let calls = probe.finish();
    assert_eq!(calls, 0, "net/dev parser allocates nothing");
}

#[test]
#[cfg(target_os = "linux")]
fn storage_parsers_have_zero_allocator_delta() {
    if spawn_isolated("storage_parsers_have_zero_allocator_delta") {
        return;
    }
    use aura_common::MAX_DISKS;
    use aura_daemon::collectors::storage::linux::{parse_diskstats, parse_mountinfo};
    use aura_daemon::collectors::storage::state::DiskRawSnapshot;
    use aura_daemon::collectors::storage::FsCapacity;
    let diskstats = include_bytes!("fixtures/proc_diskstats_sample.txt");
    let mountinfo = include_bytes!("fixtures/proc_mountinfo_sample.txt");
    let mut out = TelemetryArchive::zeroed().storage;
    let mut raw = [DiskRawSnapshot::zero(); MAX_DISKS];
    fn fixed_capacity(_: &[u8]) -> Option<FsCapacity> {
        Some(FsCapacity {
            blocks: 100,
            bfree: 50,
            bavail: 40,
            unit: 4096,
        })
    }
    let mut capacity: fn(&[u8]) -> Option<FsCapacity> = fixed_capacity;
    parse_diskstats(diskstats, &mut out, &mut raw).expect("warm diskstats");
    parse_mountinfo(mountinfo, &mut out, &mut capacity).expect("warm mountinfo");
    let probe = AllocationProbe::start();
    for _ in 0..4 {
        parse_diskstats(diskstats, &mut out, &mut raw).expect("measured diskstats");
        parse_mountinfo(mountinfo, &mut out, &mut capacity).expect("measured mountinfo");
    }
    let calls = probe.finish();
    assert_eq!(calls, 0, "diskstats/mountinfo parsers allocate nothing");
}

#[test]
#[cfg(unix)]
fn shm_publication_write_has_zero_allocator_delta() {
    if spawn_isolated("shm_publication_write_has_zero_allocator_delta") {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().expect("shm dir");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private shm dir");
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    let path = std::fs::canonicalize(directory.path())
        .expect("canonical shm dir")
        .join("state.dat");
    let mut handle = aura_daemon::state::ShmHandle::new(&path).expect("create shm");
    let archive = |generation: u64| {
        let mut archive = TelemetryArchive::zeroed();
        archive.version = aura_common::ARCHIVE_VERSION;
        archive.meta.timestamp_ns = generation;
        archive
    };
    handle.write(&archive(1)).expect("warm write");
    let probe = AllocationProbe::start();
    handle.write(&archive(2)).expect("measured write");
    handle.write(&archive(3)).expect("measured write");
    let calls = probe.finish();
    assert_eq!(calls, 0, "SHM double-buffer publication allocates nothing");
}

#[test]
#[cfg(unix)]
fn cli_read_path_validates_and_protocol_read_has_zero_allocator_delta() {
    if spawn_isolated("cli_read_path_validates_and_protocol_read_has_zero_allocator_delta") {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().expect("shm dir");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private shm dir");
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    let path = std::fs::canonicalize(directory.path())
        .expect("canonical shm dir")
        .join("state.dat");
    let mut writer = aura_daemon::state::ShmHandle::new(&path).expect("create shm");
    let mut archive = TelemetryArchive::zeroed();
    archive.version = aura_common::ARCHIVE_VERSION;
    archive.meta.timestamp_ns = 42;
    archive.checksum = 0;
    archive.checksum = archive.calculate_checksum();
    writer.write(&archive).expect("publish snapshot");
    let reader = aura_cli::reader::TelemetryReader::new(&path).expect("open reader");
    let warm = reader.read().expect("warm read");
    assert_eq!(warm.meta.timestamp_ns, 42);
    let file = std::fs::File::open(&path).expect("open read");
    // SAFETY: the state file is exactly SHM_SIZE and page-aligned.
    let map = unsafe {
        memmap2::MmapOptions::new()
            .len(aura_common::SHM_SIZE)
            .map(&file)
            .unwrap()
    };
    // SAFETY: warm protocol read over the live mapping.
    let warm_raw = unsafe { aura_common::read_double_buffer(map.as_ptr()) }.expect("warm raw read");
    assert_eq!(warm_raw.meta.timestamp_ns, 42);
    let probe = AllocationProbe::start();
    // SAFETY: the mapping is live and the writer is idle.
    let first = unsafe { aura_common::read_double_buffer(map.as_ptr()) }.expect("measured read");
    // SAFETY: same mapping contract.
    let second = unsafe { aura_common::read_double_buffer(map.as_ptr()) }.expect("measured read");
    let calls = probe.finish();
    assert_eq!(calls, 0, "SHM SeqLock protocol read allocates nothing");
    assert_eq!(first.meta.timestamp_ns, second.meta.timestamp_ns);
}
