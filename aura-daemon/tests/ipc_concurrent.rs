use std::fs::OpenOptions;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use aura_common::{read_double_buffer, TelemetryArchive, SHM_SIZE};
use aura_daemon::state::ShmHandle;
use memmap2::{Mmap, MmapOptions};

#[derive(Debug, Default, Clone, Copy)]
struct ReaderStats {
    total_reads: u64,
    successful_reads: u64,
    checksum_failures: u64,
    version_spin_count: u64,
    version_mismatches: u64,
    max_seen_version: u64,
    observed_latest_snapshot: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct WriterStats {
    total_writes: u64,
    write_errors: u64,
    latest_committed_version: u64,
}

#[derive(Debug, Clone, Copy)]
enum ReadErrorKind {
    SeqLockTimeout,
    ChecksumMismatch,
}

#[test]
fn ipc_concurrent_reader_writer_stress() {
    const READER_COUNT: usize = 4;
    const TEST_DURATION: Duration = Duration::from_secs(3);
    const READ_TIMEOUT: Duration = Duration::from_millis(100);
    const CATCHUP_TIMEOUT: Duration = Duration::from_secs(1);

    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let shm_path = temp_dir.path().join("aura-ipc-concurrent.dat");

    let mut handle = ShmHandle::new(&shm_path).expect("create shm handle");
    let mut initial = make_archive(1);
    handle.write(&mut initial).expect("seed initial telemetry");

    let shutdown = Arc::new(AtomicBool::new(false));
    let latest_writer_version = Arc::new(AtomicU64::new(1));

    let writer_shutdown = Arc::clone(&shutdown);
    let writer_latest = Arc::clone(&latest_writer_version);
    let writer_thread = thread::spawn(move || {
        run_writer_loop(
            handle,
            writer_shutdown,
            writer_latest,
            Duration::from_micros(200),
        )
    });

    let mut reader_threads = Vec::with_capacity(READER_COUNT);
    for _ in 0..READER_COUNT {
        let reader_shutdown = Arc::clone(&shutdown);
        let reader_latest = Arc::clone(&latest_writer_version);
        let reader_path = shm_path.clone();
        reader_threads.push(thread::spawn(move || {
            run_reader_loop(
                &reader_path,
                reader_shutdown,
                reader_latest,
                READ_TIMEOUT,
                CATCHUP_TIMEOUT,
            )
        }));
    }

    thread::sleep(TEST_DURATION);
    shutdown.store(true, Ordering::SeqCst);

    let writer_stats = writer_thread.join().expect("writer thread join");

    let reader_stats: Vec<ReaderStats> = reader_threads
        .into_iter()
        .map(|handle| handle.join().expect("reader thread join"))
        .collect();

    let final_version = writer_stats.latest_committed_version;
    let total_reads: u64 = reader_stats.iter().map(|s| s.total_reads).sum();
    let total_successful_reads: u64 = reader_stats.iter().map(|s| s.successful_reads).sum();
    let total_checksum_failures: u64 = reader_stats.iter().map(|s| s.checksum_failures).sum();
    let total_version_spins: u64 = reader_stats.iter().map(|s| s.version_spin_count).sum();
    let total_version_mismatches: u64 = reader_stats.iter().map(|s| s.version_mismatches).sum();

    eprintln!(
        "writer(total_writes={}, write_errors={}, latest_version={}); readers(total_reads={}, successful_reads={}, checksum_failures={}, version_spins={}, version_mismatches={})",
        writer_stats.total_writes,
        writer_stats.write_errors,
        writer_stats.latest_committed_version,
        total_reads,
        total_successful_reads,
        total_checksum_failures,
        total_version_spins,
        total_version_mismatches,
    );

    assert!(
        writer_stats.total_writes > 0,
        "writer produced no snapshots"
    );
    assert_eq!(
        writer_stats.write_errors, 0,
        "writer observed write errors under contention"
    );

    assert!(
        final_version >= 1,
        "writer did not commit a valid final snapshot"
    );

    for (idx, stats) in reader_stats.iter().enumerate() {
        assert!(
            stats.total_reads > 0,
            "reader #{idx} performed no reads: {:?}",
            stats
        );
        assert!(
            stats.successful_reads > 0,
            "reader #{idx} had no successful reads: {:?}",
            stats
        );
        assert!(
            stats.observed_latest_snapshot,
            "reader #{idx} did not observe latest version {}: {:?}",
            final_version, stats
        );
    }

    assert_eq!(
        total_checksum_failures, 0,
        "readers observed torn data (checksum mismatches)"
    );

    let _ = std::fs::remove_file(&shm_path);
}

fn run_writer_loop(
    mut handle: ShmHandle,
    shutdown: Arc<AtomicBool>,
    latest_version: Arc<AtomicU64>,
    pause_between_writes: Duration,
) -> WriterStats {
    let mut stats = WriterStats::default();
    let mut version = latest_version.load(Ordering::SeqCst);

    while !shutdown.load(Ordering::SeqCst) {
        version = version.saturating_add(1);
        let mut telemetry = make_archive(version);

        stats.total_writes = stats.total_writes.saturating_add(1);
        match handle.write(&mut telemetry) {
            Ok(()) => {
                stats.latest_committed_version = version;
                latest_version.store(version, Ordering::SeqCst);
            }
            Err(_) => {
                stats.write_errors = stats.write_errors.saturating_add(1);
            }
        }

        thread::sleep(pause_between_writes);
    }

    stats
}

fn run_reader_loop(
    path: &std::path::Path,
    shutdown: Arc<AtomicBool>,
    latest_version: Arc<AtomicU64>,
    read_timeout: Duration,
    catchup_timeout: Duration,
) -> ReaderStats {
    let mmap = open_read_map(path);
    let mut stats = ReaderStats::default();

    while !shutdown.load(Ordering::SeqCst) {
        stats.total_reads = stats.total_reads.saturating_add(1);
        match read_snapshot_once(&mmap, read_timeout, &mut stats) {
            Ok(snapshot) => {
                stats.successful_reads = stats.successful_reads.saturating_add(1);
                stats.max_seen_version = stats.max_seen_version.max(snapshot.version);
            }
            Err(ReadErrorKind::ChecksumMismatch) => {
                stats.checksum_failures = stats.checksum_failures.saturating_add(1);
            }
            Err(ReadErrorKind::SeqLockTimeout) => {
                stats.version_spin_count = stats.version_spin_count.saturating_add(1);
            }
        }
    }

    let expected_latest = latest_version.load(Ordering::SeqCst);
    let deadline = Instant::now() + catchup_timeout;
    while stats.max_seen_version < expected_latest && Instant::now() < deadline {
        stats.total_reads = stats.total_reads.saturating_add(1);
        match read_snapshot_once(&mmap, read_timeout, &mut stats) {
            Ok(snapshot) => {
                stats.successful_reads = stats.successful_reads.saturating_add(1);
                stats.max_seen_version = stats.max_seen_version.max(snapshot.version);
            }
            Err(ReadErrorKind::ChecksumMismatch) => {
                stats.checksum_failures = stats.checksum_failures.saturating_add(1);
            }
            Err(ReadErrorKind::SeqLockTimeout) => {
                stats.version_spin_count = stats.version_spin_count.saturating_add(1);
            }
        }
    }

    stats.observed_latest_snapshot = stats.max_seen_version >= expected_latest;
    stats
}

fn open_read_map(path: &std::path::Path) -> Mmap {
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .expect("open shm for reader");
    unsafe {
        MmapOptions::new()
            .len(SHM_SIZE)
            .map(&file)
            .expect("mmap shared memory for reader")
    }
}

fn read_snapshot_once(
    mmap: &Mmap,
    timeout: Duration,
    stats: &mut ReaderStats,
) -> Result<TelemetryArchive, ReadErrorKind> {
    let start = Instant::now();

    loop {
        let mut snapshot = match unsafe { read_double_buffer(mmap.as_ptr()) } {
            Ok(snapshot) => snapshot,
            Err(()) => {
                stats.version_mismatches = stats.version_mismatches.saturating_add(1);
                stats.version_spin_count = stats.version_spin_count.saturating_add(1);
                if start.elapsed() >= timeout {
                    return Err(ReadErrorKind::SeqLockTimeout);
                }
                continue;
            }
        };

        let expected = snapshot.checksum;
        snapshot.checksum = 0;
        let actual = snapshot.calculate_checksum();
        snapshot.checksum = expected;

        if expected != actual {
            return Err(ReadErrorKind::ChecksumMismatch);
        }

        return Ok(snapshot);
    }
}

fn make_archive(version: u64) -> TelemetryArchive {
    let mut archive = TelemetryArchive::zeroed();
    archive.version = version;
    archive.meta.timestamp_ns = version;
    archive.cpu.total_ticks = version;
    archive.cpu.user_ticks = version / 2;
    archive.cpu.system_ticks = version / 4;
    archive.cpu.idle_ticks = version / 8;
    archive.cpu.usage_percent = (version % 100) as f32;
    archive.checksum = 0;
    archive.checksum = archive.calculate_checksum();
    archive
}
