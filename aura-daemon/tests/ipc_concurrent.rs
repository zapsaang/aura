use std::fs::{File, OpenOptions};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use aura_common::{
    read_double_buffer, read_double_buffer_with_elapsed, AuraError, DoubleBufferHeader,
    TelemetryArchive, SEQLOCK_RETRY_ADMISSION_MS, SHM_SIZE,
};
use aura_daemon::state::ShmHandle;
use memmap2::{Mmap, MmapOptions};

const MULTIPROCESS_HANDOFF_GENERATION: u64 = 512;
const MULTIPROCESS_FINAL_GENERATION: u64 = 1_024;

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
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod test temp dir");
    }
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
                stats.max_seen_version = stats.max_seen_version.max(snapshot.meta.timestamp_ns);
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
                stats.max_seen_version = stats.max_seen_version.max(snapshot.meta.timestamp_ns);
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
    // SAFETY: `ShmHandle` created the file at `SHM_SIZE`, and the stress test never truncates it while readers map it.
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
        // SAFETY: `mmap` covers the full SHM layout and the writer uses the same atomic double-buffer protocol.
        let mut snapshot = match unsafe { read_double_buffer(mmap.as_ptr()) } {
            Ok(snapshot) => snapshot,
            Err(_) => {
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

#[test]
fn zero_sequence_is_not_published_after_retry_deadline() {
    // Given
    let map = memmap2::MmapOptions::new()
        .len(SHM_SIZE)
        .map_anon()
        .expect("create anonymous map");

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the anonymous
    // map is page-aligned, live, and exactly `SHM_SIZE` bytes.
    let error = unsafe {
        read_double_buffer_with_elapsed(map.as_ptr(), || {
            Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS)
        })
    }
    .expect_err("zero sequence must not publish");

    // Then
    assert!(matches!(error, AuraError::NotPublished));
}

#[test]
fn odd_sequence_times_out_after_retry_deadline() {
    // Given
    let mut map = memmap2::MmapOptions::new()
        .len(SHM_SIZE)
        .map_anon()
        .expect("create anonymous map");
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] sequence zero is
    // a naturally aligned atomic at offset eight in the page-aligned map.
    unsafe { (*map.as_mut_ptr().add(8).cast::<AtomicU64>()).store(1, Ordering::Release) };

    // When
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the anonymous
    // map remains live and aligned for the protocol read.
    let error = unsafe {
        read_double_buffer_with_elapsed(map.as_ptr(), || {
            Duration::from_millis(SEQLOCK_RETRY_ADMISSION_MS)
        })
    }
    .expect_err("odd sequence must time out");

    // Then
    assert!(matches!(error, AuraError::SeqLockTimeout));
}

#[test]
fn writer_generations_increase_under_repeated_publications() {
    // Given
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod test temp dir");
    }
    let path = temp_dir.path().join("generation-state.dat");
    let mut writer = ShmHandle::new(&path).expect("create writer");
    let file = OpenOptions::new().read(true).open(&path).unwrap();
    // SAFETY: [Categories 6 and 10 — alignment and bounds] the state file is
    // exactly `SHM_SIZE` and maps read-only at page alignment.
    let map = unsafe { MmapOptions::new().len(SHM_SIZE).map(&file).unwrap() };
    // SAFETY: [Categories 5, 6, 10 — validity, alignment, bounds] the live map
    // starts with the initialized atomic header.
    let header = unsafe { &*map.as_ptr().cast::<DoubleBufferHeader>() };
    let mut previous = [0u64; 2];

    // When
    for marker in 1..=128 {
        writer
            .write(&mut make_archive(marker))
            .expect("publish generation");
        let current = [
            header.seq[0].load(Ordering::Acquire),
            header.seq[1].load(Ordering::Acquire),
        ];
        assert!(current[0] >= previous[0]);
        assert!(current[1] >= previous[1]);
        previous = current;
    }

    // Then
    assert_eq!(previous, [128, 128]);
}

#[test]
fn concurrent_abandoned_sequence_recovery_never_returns_mixed_archive() {
    // Given
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod test temp dir");
    }
    let path = temp_dir.path().join("recovery-state.dat");
    let mut writer = ShmHandle::new(&path).expect("create writer");
    writer
        .write(&mut make_archive(300))
        .expect("seed old snapshot");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    // SAFETY: [Categories 6 and 10 — alignment and bounds] the state file is
    // exactly `SHM_SIZE` and maps writable at page alignment.
    let mut injection = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] inactive
    // sequence zero is naturally aligned and no other thread accesses it yet.
    unsafe { (*injection.as_mut_ptr().add(8).cast::<AtomicU64>()).store(1, Ordering::Release) };
    drop(injection);
    let reader_map = open_read_map(&path);
    let barrier = Arc::new(Barrier::new(2));
    let reader_barrier = Arc::clone(&barrier);
    let reader = thread::spawn(move || {
        reader_barrier.wait();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the map
            // is live and the writer uses only the matching atomic protocol.
            if let Ok(snapshot) = unsafe { read_double_buffer(reader_map.as_ptr()) } {
                assert_eq!(snapshot.meta.timestamp_ns, snapshot.cpu.total_ticks);
                if snapshot.meta.timestamp_ns == 301 {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "reader did not observe recovery");
            thread::yield_now();
        }
    });

    // When
    barrier.wait();
    writer
        .write(&mut make_archive(301))
        .expect("recover abandoned sequence");

    // Then
    reader.join().expect("reader thread join");
}

#[test]
fn ipc_multiprocess_release_stress_uses_atomic_protocol() {
    // Given
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp_dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod test temp dir");
    }
    let state = temp_dir.path().join("multiprocess-state.dat");
    let writer_ready = temp_dir.path().join("writer.ready");
    let reader_ready = temp_dir.path().join("reader.ready");
    let start = temp_dir.path().join("start");
    let done = temp_dir.path().join("done");
    let observed = temp_dir.path().join("observed");
    let executable = std::env::current_exe().expect("locate test executable");
    let mut writer = Command::new(&executable)
        .args(["--exact", "multiprocess_writer_child", "--ignored"])
        .env("AURA_IPC_STATE", &state)
        .env("AURA_IPC_READY", &writer_ready)
        .env("AURA_IPC_START", &start)
        .env("AURA_IPC_DONE", &done)
        .env("AURA_IPC_OBSERVED", &observed)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn writer child");
    if let Err(error) = wait_for_marker(&writer_ready) {
        terminate(&mut writer);
        panic!("writer readiness failed: {error}");
    }
    let mut reader = Command::new(&executable)
        .args(["--exact", "multiprocess_reader_child", "--ignored"])
        .env("AURA_IPC_STATE", &state)
        .env("AURA_IPC_READY", &reader_ready)
        .env("AURA_IPC_START", &start)
        .env("AURA_IPC_DONE", &done)
        .env("AURA_IPC_OBSERVED", &observed)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn reader child");
    if let Err(error) = wait_for_marker(&reader_ready) {
        terminate(&mut writer);
        terminate(&mut reader);
        panic!("reader readiness failed: {error}");
    }

    // When
    File::create(&start).expect("release child processes");
    let writer_output = writer.wait_with_output().expect("wait for writer child");
    let reader_output = reader.wait_with_output().expect("wait for reader child");

    // Then
    assert!(
        writer_output.status.success(),
        "writer child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&writer_output.stdout),
        String::from_utf8_lossy(&writer_output.stderr)
    );
    assert!(
        reader_output.status.success(),
        "reader child failed: stdout={} stderr={}",
        String::from_utf8_lossy(&reader_output.stdout),
        String::from_utf8_lossy(&reader_output.stderr)
    );
    let observed_generation = std::fs::read_to_string(&observed)
        .expect("reader must report an observed intermediate generation")
        .parse::<u64>()
        .expect("observed generation must be a u64");
    assert!(
        (2..=MULTIPROCESS_HANDOFF_GENERATION).contains(&observed_generation),
        "reader reported non-intermediate generation {observed_generation}"
    );
}

#[test]
#[ignore = "spawned by ipc_multiprocess_release_stress_uses_atomic_protocol"]
fn multiprocess_writer_child() {
    // Given
    let state = required_path("AURA_IPC_STATE");
    let ready = required_path("AURA_IPC_READY");
    let start = required_path("AURA_IPC_START");
    let done = required_path("AURA_IPC_DONE");
    let observed = required_path("AURA_IPC_OBSERVED");
    let mut writer = ShmHandle::new(&state).expect("child creates state");
    writer
        .write(&mut make_archive(1))
        .expect("child seeds state");
    File::create(ready).expect("signal writer ready");
    wait_for_marker(&start).expect("wait for parent release");

    // When
    for marker in 2..=MULTIPROCESS_HANDOFF_GENERATION {
        writer
            .write(&mut make_archive(marker))
            .expect("child publishes snapshot");
    }
    wait_for_marker(&observed).expect("wait for reader intermediate observation");
    for marker in (MULTIPROCESS_HANDOFF_GENERATION + 1)..=MULTIPROCESS_FINAL_GENERATION {
        writer
            .write(&mut make_archive(marker))
            .expect("child publishes snapshot");
    }

    // Then
    File::create(done).expect("signal writer completion");
}

#[test]
#[ignore = "spawned by ipc_multiprocess_release_stress_uses_atomic_protocol"]
fn multiprocess_reader_child() {
    // Given
    let state = required_path("AURA_IPC_STATE");
    let ready = required_path("AURA_IPC_READY");
    let start = required_path("AURA_IPC_START");
    let done = required_path("AURA_IPC_DONE");
    let observed = required_path("AURA_IPC_OBSERVED");
    let map = open_read_map(&state);
    File::create(ready).expect("signal reader ready");
    wait_for_marker(&start).expect("wait for parent release");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut successful_reads = 0u64;
    let mut observed_generation = None;

    // When
    loop {
        // SAFETY: [Categories 2, 6, 10 — races, alignment, bounds] the mapping
        // is live while the writer process uses the matching atomic protocol.
        if let Ok(mut snapshot) = unsafe { read_double_buffer(map.as_ptr()) } {
            assert_eq!(snapshot.meta.timestamp_ns, snapshot.cpu.total_ticks);
            let expected = snapshot.checksum;
            snapshot.checksum = 0;
            assert_eq!(snapshot.calculate_checksum(), expected);
            successful_reads += 1;
            let generation = snapshot.meta.timestamp_ns;
            if observed_generation.is_none()
                && (2..=MULTIPROCESS_HANDOFF_GENERATION).contains(&generation)
            {
                std::fs::write(&observed, generation.to_string())
                    .expect("report intermediate generation");
                observed_generation = Some(generation);
            }
        }
        if done.exists() && observed_generation.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "multiprocess reader timed out");
        thread::yield_now();
    }

    // Then
    assert!(successful_reads > 0);
    assert!(observed_generation.is_some());
}

fn required_path(name: &str) -> std::path::PathBuf {
    std::env::var_os(name)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn wait_for_marker(path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("marker {} timed out", path.display()));
        }
        thread::yield_now();
    }
    Ok(())
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
