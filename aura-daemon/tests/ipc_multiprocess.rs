//! Multiprocess IPC evidence (Todo 17): the SeqLock double-buffer protocol,
//! lock mutual exclusion, crash recovery, corruption detection, and CLI
//! offline/freshness classification are exercised across real process
//! boundaries by re-executing this test binary with `#[ignore]`d child
//! entry points synchronized through marker files.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use aura_common::{
    read_double_buffer, AuraError, TelemetryArchive, ARCHIVE_VERSION, BUFFER_0_OFFSET,
    BUFFER_1_OFFSET, SHM_SIZE,
};
use aura_daemon::state::ShmHandle;
use memmap2::{Mmap, MmapOptions};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn make_rich_archive(generation: u64) -> TelemetryArchive {
    let mut archive = TelemetryArchive::zeroed();
    archive.version = ARCHIVE_VERSION;
    archive.meta.timestamp_ns = generation;
    archive.cpu.total_ticks = generation;
    archive.cpu.user_ticks = generation / 2;
    archive.cpu.system_ticks = generation / 4;
    archive.cpu.idle_ticks = generation / 8;
    archive.checksum = 0;
    archive.checksum = archive.calculate_checksum();
    archive
}

fn make_minimal_archive(generation: u64) -> TelemetryArchive {
    let mut archive = TelemetryArchive::zeroed();
    archive.version = ARCHIVE_VERSION;
    archive.meta.timestamp_ns = generation;
    archive.checksum = 0;
    archive.checksum = archive.calculate_checksum();
    archive
}

fn private_dir() -> tempfile::TempDir {
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    let base = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp base");
    let dir = tempfile::Builder::new()
        .tempdir_in(base)
        .expect("create temp dir");
    #[cfg(unix)]
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
        .expect("chmod test temp dir");
    dir
}

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn optional_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

// Deadlines only bound deadlock detection; sleep-poll so waiting does not
// starve contended child processes of CPU.
fn wait_for_marker(path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("marker {} timed out", path.display()));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

/// Fails early with captured output if a watched child exits before the marker.
fn wait_for_marker_watched(path: &Path, children: &mut [&mut Child]) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("marker {} timed out", path.display()));
        }
        for child in children.iter_mut() {
            if let Some(status) = child.try_wait().expect("poll child status") {
                let stdout = drain(child.stdout.take());
                let stderr = drain(child.stderr.take());
                return Err(format!(
                    "child exited early with {status}: stdout={stdout} stderr={stderr}"
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn drain(pipe: Option<impl std::io::Read>) -> String {
    let mut buf = String::new();
    if let Some(mut pipe) = pipe {
        let _ = std::io::Read::read_to_string(&mut pipe, &mut buf);
    }
    buf
}

fn terminate_children(children: &mut [&mut Child]) {
    for child in children {
        let _ = child.kill();
    }
}

fn spawn_child(test_name: &str, envs: &[(&str, &Path)]) -> Child {
    let mut command = Command::new(std::env::current_exe().expect("locate test executable"));
    command
        .args(["--exact", test_name, "--ignored", "--test-threads=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {test_name}: {e}"))
}

fn assert_child_success(child: Child, name: &str) {
    let output = child.wait_with_output().expect("wait for child");
    assert!(
        output.status.success(),
        "{name} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn open_read_map(path: &Path) -> Mmap {
    // Children are spawned concurrently with the writer, so the state file may
    // not exist or be fully sized yet; poll until ShmHandle finishes creating it.
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let ready = OpenOptions::new()
            .read(true)
            .open(path)
            .ok()
            .filter(|file| {
                file.metadata()
                    .map(|meta| meta.len() as usize >= SHM_SIZE)
                    .unwrap_or(false)
            });
        if let Some(file) = ready {
            // SAFETY: `ShmHandle` sized the file to exactly `SHM_SIZE`; the
            // read-only mapping covers the exact SHM layout.
            return unsafe {
                MmapOptions::new()
                    .len(SHM_SIZE)
                    .map(&file)
                    .expect("mmap shared memory for reader")
            };
        }
        assert!(Instant::now() < deadline, "state file not ready for reader");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn verify_snapshot(snapshot: &TelemetryArchive) -> bool {
    let expected = snapshot.checksum;
    let mut copy = *snapshot;
    copy.checksum = 0;
    copy.calculate_checksum() == expected
}

#[test]
fn multiprocess_handoff_delivers_final_generation() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let report = dir.path().join("reader.report");
    let writer = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("1:64")),
        ],
    );
    let reader = spawn_child(
        "mp_reader_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_REPORT", &report),
            ("AURA_MP_EXPECT", Path::new("64")),
        ],
    );
    assert_child_success(writer, "writer child");
    assert_child_success(reader, "reader child");
    let text = std::fs::read_to_string(&report).expect("reader report");
    let fields = parse_report(&text);
    assert_eq!(fields.max, 64, "reader observed the final generation");
    assert_eq!(fields.violations, 0, "no mixed or corrupt snapshot");
    assert!(fields.count >= 1, "at least one successful read");
}

#[test]
fn multiprocess_intermediate_observation_is_monotonic() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let report = dir.path().join("reader.report");
    let observed = dir.path().join("observed");
    let writer = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("1:512")),
            ("AURA_MP_HANDOFF", Path::new("64")),
            ("AURA_MP_OBSERVED", &observed),
        ],
    );
    let reader = spawn_child(
        "mp_reader_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_REPORT", &report),
            ("AURA_MP_EXPECT", Path::new("512")),
            ("AURA_MP_HANDOFF", Path::new("64")),
            ("AURA_MP_OBSERVED", &observed),
        ],
    );
    assert_child_success(writer, "writer child");
    assert_child_success(reader, "reader child");
    let observed_generation = std::fs::read_to_string(&observed)
        .expect("reader reports an intermediate generation")
        .trim()
        .parse::<u64>()
        .expect("observed generation is u64");
    assert!(
        (2..=64).contains(&observed_generation),
        "intermediate observation out of range: {observed_generation}"
    );
    let fields = parse_report(&std::fs::read_to_string(&report).expect("reader report"));
    assert_eq!(fields.max, 512);
    assert_eq!(fields.violations, 0);
    assert!(
        fields.count >= 2,
        "intermediate and final reads both landed"
    );
}

#[test]
fn multiprocess_second_writer_rejected_while_lock_held() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let report = dir.path().join("lock.report");
    let _holder = ShmHandle::new(&state).expect("holder creates state");
    let attempt = spawn_child(
        "mp_lock_attempt_child",
        &[("AURA_MP_STATE", &state), ("AURA_MP_REPORT", &report)],
    );
    assert_child_success(attempt, "lock attempt child");
    let outcome = std::fs::read_to_string(&report).expect("lock report");
    assert!(
        outcome.starts_with("held"),
        "second daemon must observe the held lock, got: {outcome}"
    );
}

#[test]
fn multiprocess_lock_reacquired_after_holder_exit() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let report = dir.path().join("lock.report");
    let writer = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("1:2")),
        ],
    );
    assert_child_success(writer, "first holder");
    let attempt = spawn_child(
        "mp_lock_attempt_child",
        &[("AURA_MP_STATE", &state), ("AURA_MP_REPORT", &report)],
    );
    assert_child_success(attempt, "lock attempt child");
    assert_eq!(
        std::fs::read_to_string(&report).expect("lock report"),
        "opened",
        "flock is released with the holder process exit"
    );
    let mut handle = ShmHandle::new(&state).expect("reacquire after child exit");
    handle
        .write(&make_rich_archive(3))
        .expect("extend timeline");
    let map = open_read_map(&state);
    // SAFETY: the map is live and the single writer above completed.
    let snapshot = unsafe { read_double_buffer(map.as_ptr()) }.expect("read back");
    assert_eq!(snapshot.meta.timestamp_ns, 3);
}

#[test]
fn multiprocess_unwritten_state_is_not_published() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let _holder = ShmHandle::new(&state).expect("create unwritten state");
    let child = spawn_child("mp_not_published_child", &[("AURA_MP_STATE", &state)]);
    assert_child_success(child, "not-published child");
}

#[test]
fn multiprocess_abandoned_odd_sequence_blocks_until_recovery() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let report = dir.path().join("reader.report");
    let ready = dir.path().join("reader.ready");
    let mut writer = ShmHandle::new(&state).expect("create state");
    writer
        .write(&make_rich_archive(1))
        .expect("seed generation");
    let injector = spawn_child("mp_inject_odd_child", &[("AURA_MP_STATE", &state)]);
    assert_child_success(injector, "odd-sequence injector");
    let mut reader = spawn_child(
        "mp_reader_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_REPORT", &report),
            ("AURA_MP_EXPECT", Path::new("2")),
            ("AURA_MP_READY", &ready),
            ("AURA_MP_READY_AFTER", Path::new("timeout")),
        ],
    );
    // READY_AFTER=timeout makes READY imply the reader already blocked once.
    if let Err(error) = wait_for_marker_watched(&ready, [&mut reader].as_mut_slice()) {
        let _ = reader.kill();
        panic!("reader attach failed: {error}");
    }
    writer.write(&make_rich_archive(2)).expect("recovery write");
    assert_child_success(reader, "reader child");
    let fields = parse_report(&std::fs::read_to_string(&report).expect("reader report"));
    assert_eq!(fields.max, 2, "reader recovered on the next publication");
    assert_eq!(fields.violations, 0, "never observed a mixed archive");
    assert!(
        fields.timeouts >= 1,
        "reader provably blocked on the abandoned odd sequence"
    );
}

#[test]
fn multiprocess_corrupted_payload_is_detected() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let report = dir.path().join("corruption.report");
    let mut writer = ShmHandle::new(&state).expect("create state");
    writer
        .write(&make_rich_archive(7))
        .expect("seed generation");
    drop(writer);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&state)
        .expect("open rw");
    // SAFETY: the state file is exactly `SHM_SIZE` and page-aligned.
    let mut map = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    // SAFETY: header active word at offset 0 selects the in-bounds buffer.
    let active = unsafe { *map.as_ptr().cast::<u64>() };
    let offset = (if active == 0 {
        BUFFER_0_OFFSET
    } else {
        BUFFER_1_OFFSET
    }) + 100;
    map[offset] ^= 0xFF;
    drop(map);
    let detector = spawn_child(
        "mp_checksum_detect_child",
        &[("AURA_MP_STATE", &state), ("AURA_MP_REPORT", &report)],
    );
    assert_child_success(detector, "checksum detector");
    assert_eq!(
        std::fs::read_to_string(&report).expect("detector report"),
        "detected",
        "corruption is observed across the process boundary"
    );
}

#[test]
fn multiprocess_cli_reader_observes_published_state() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let writer = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("1:9")),
            ("AURA_MP_KIND", Path::new("minimal")),
        ],
    );
    assert_child_success(writer, "writer child");
    let reader = spawn_child(
        "mp_cli_read_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_EXPECT", Path::new("9")),
        ],
    );
    assert_child_success(reader, "cli reader child");
}

#[test]
fn multiprocess_absent_default_classifies_offline() {
    let dir = private_dir();
    let child = spawn_child("mp_offline_child", &[("AURA_MP_PARENT", dir.path())]);
    assert_child_success(child, "offline classifier child");
}

#[test]
fn multiprocess_concurrent_readers_agree_on_final() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let start = dir.path().join("start");
    let ready_a = dir.path().join("reader-a.ready");
    let ready_b = dir.path().join("reader-b.ready");
    let report_a = dir.path().join("reader-a.report");
    let report_b = dir.path().join("reader-b.report");
    let mut writer = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("1:128")),
            ("AURA_MP_START", &start),
        ],
    );
    let mut reader_a = spawn_child(
        "mp_reader_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_REPORT", &report_a),
            ("AURA_MP_EXPECT", Path::new("128")),
            ("AURA_MP_READY", &ready_a),
        ],
    );
    let mut reader_b = spawn_child(
        "mp_reader_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_REPORT", &report_b),
            ("AURA_MP_EXPECT", Path::new("128")),
            ("AURA_MP_READY", &ready_b),
        ],
    );
    for (ready, name) in [(&ready_a, "reader A"), (&ready_b, "reader B")] {
        if let Err(error) = wait_for_marker_watched(
            ready,
            [&mut writer, &mut reader_a, &mut reader_b].as_mut_slice(),
        ) {
            terminate_children([&mut writer, &mut reader_a, &mut reader_b].as_mut_slice());
            panic!("{name} attach failed: {error}");
        }
    }
    File::create(&start).expect("release children");
    assert_child_success(writer, "writer child");
    assert_child_success(reader_a, "reader A");
    assert_child_success(reader_b, "reader B");
    for report in [&report_a, &report_b] {
        let fields = parse_report(&std::fs::read_to_string(report).expect("reader report"));
        assert_eq!(
            fields.max, 128,
            "both readers converge on the final generation"
        );
        assert_eq!(fields.violations, 0);
        assert!(fields.count >= 1);
    }
}

#[test]
fn multiprocess_writer_restart_extends_timeline() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let first = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("1:4")),
            ("AURA_MP_KIND", Path::new("minimal")),
        ],
    );
    assert_child_success(first, "first writer");
    let second = spawn_child(
        "mp_writer_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_GENS", Path::new("5:8")),
            ("AURA_MP_KIND", Path::new("minimal")),
        ],
    );
    assert_child_success(second, "restarted writer");
    let reader = spawn_child(
        "mp_cli_read_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_EXPECT", Path::new("8")),
        ],
    );
    assert_child_success(reader, "cli reader child");
}

#[test]
fn multiprocess_stale_snapshot_fails_freshness() {
    let dir = private_dir();
    let state = dir.path().join("state.dat");
    let mut writer = ShmHandle::new(&state).expect("create state");
    writer
        .write(&make_minimal_archive(1))
        .expect("seed ancient snapshot");
    drop(writer);
    let reader = spawn_child(
        "mp_cli_read_child",
        &[
            ("AURA_MP_STATE", &state),
            ("AURA_MP_MODE", Path::new("stale")),
        ],
    );
    assert_child_success(reader, "stale classifier child");
}

struct ReaderReport {
    max: u64,
    count: u64,
    violations: u64,
    timeouts: u64,
}

fn parse_report(text: &str) -> ReaderReport {
    let mut report = ReaderReport {
        max: 0,
        count: 0,
        violations: 0,
        timeouts: 0,
    };
    for pair in text.split_whitespace() {
        let (key, value) = pair.split_once('=').expect("report key=value");
        let parsed = value.parse::<u64>().expect("report numeric value");
        match key {
            "max" => report.max = parsed,
            "count" => report.count = parsed,
            "violations" => report.violations = parsed,
            "timeouts" => report.timeouts = parsed,
            other => panic!("unknown report key {other}"),
        }
    }
    report
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_writer_child() {
    let state = required_path("AURA_MP_STATE");
    let gens = std::env::var("AURA_MP_GENS").expect("generation range");
    let (first, last) = gens.split_once(':').expect("range a:b");
    let (first, last) = (first.parse::<u64>().unwrap(), last.parse::<u64>().unwrap());
    let minimal = optional_path("AURA_MP_KIND").is_some();
    let handoff = std::env::var("AURA_MP_HANDOFF")
        .ok()
        .map(|v| v.parse::<u64>().unwrap());
    let observed = optional_path("AURA_MP_OBSERVED");
    let mut writer = ShmHandle::new(&state).expect("child creates state");
    if let Some(start) = optional_path("AURA_MP_START") {
        let seed = if minimal {
            make_minimal_archive(first)
        } else {
            make_rich_archive(first)
        };
        writer.write(&seed).expect("seed before gate");
        wait_for_marker(&start).expect("wait for release");
    }
    for generation in first..=last {
        let archive = if minimal {
            make_minimal_archive(generation)
        } else {
            make_rich_archive(generation)
        };
        writer.write(&archive).expect("child publishes snapshot");
        if handoff == Some(generation) {
            if let Some(observed) = &observed {
                wait_for_marker(observed).expect("wait for intermediate observation");
            }
        }
    }
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_reader_child() {
    let state = required_path("AURA_MP_STATE");
    let report = required_path("AURA_MP_REPORT");
    let expect = std::env::var("AURA_MP_EXPECT")
        .expect("expected generation")
        .parse::<u64>()
        .unwrap();
    let handoff = std::env::var("AURA_MP_HANDOFF")
        .ok()
        .map(|v| v.parse::<u64>().unwrap());
    let observed = optional_path("AURA_MP_OBSERVED");
    let ready_after_timeout = std::env::var("AURA_MP_READY_AFTER")
        .map(|v| v == "timeout")
        .unwrap_or(false);
    let deadline = Instant::now() + Duration::from_secs(60);
    let map = open_read_map(&state);
    let ready = optional_path("AURA_MP_READY");
    if !ready_after_timeout {
        if let Some(ready) = &ready {
            File::create(ready).expect("signal reader ready");
        }
    }
    let mut max = 0u64;
    let mut count = 0u64;
    let mut violations = 0u64;
    let mut timeouts = 0u64;
    let mut reported_intermediate = false;
    loop {
        // SAFETY: the mapping is live and the writer process uses the
        // matching atomic protocol.
        match unsafe { read_double_buffer(map.as_ptr()) } {
            Ok(snapshot) => {
                if snapshot.meta.timestamp_ns != snapshot.cpu.total_ticks
                    || !verify_snapshot(&snapshot)
                {
                    violations += 1;
                }
                count += 1;
                max = max.max(snapshot.meta.timestamp_ns);
                if !reported_intermediate
                    && handoff.is_some_and(|h| (2..=h).contains(&snapshot.meta.timestamp_ns))
                {
                    if let Some(observed) = &observed {
                        std::fs::write(observed, snapshot.meta.timestamp_ns.to_string())
                            .expect("report intermediate generation");
                    }
                    reported_intermediate = true;
                }
                if snapshot.meta.timestamp_ns >= expect {
                    break;
                }
            }
            Err(AuraError::SeqLockTimeout) => {
                timeouts += 1;
                if ready_after_timeout && timeouts == 1 {
                    if let Some(ready) = &ready {
                        File::create(ready).expect("signal reader blocked");
                    }
                }
            }
            Err(_) => {}
        }
        assert!(
            Instant::now() < deadline,
            "reader timed out waiting for {expect}"
        );
        std::thread::yield_now();
    }
    std::fs::write(
        &report,
        format!("max={max} count={count} violations={violations} timeouts={timeouts}"),
    )
    .expect("write reader report");
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_lock_attempt_child() {
    let state = required_path("AURA_MP_STATE");
    let report = required_path("AURA_MP_REPORT");
    let outcome = match ShmHandle::new(&state) {
        Ok(_) => "opened".to_string(),
        Err(AuraError::Security(reason)) if reason.contains("held") => format!("held:{reason}"),
        Err(error) => panic!("unexpected lock attempt outcome: {error}"),
    };
    std::fs::write(&report, outcome).expect("write lock report");
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_not_published_child() {
    let state = required_path("AURA_MP_STATE");
    let map = open_read_map(&state);
    // SAFETY: the mapping is live for the duration of the read.
    match unsafe { read_double_buffer(map.as_ptr()) } {
        Err(AuraError::NotPublished) => {}
        other => panic!("unwritten state must classify NotPublished: {other:?}"),
    }
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_inject_odd_child() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let state = required_path("AURA_MP_STATE");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&state)
        .expect("open rw");
    // SAFETY: the state file is exactly `SHM_SIZE` and page-aligned.
    let mut map = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    // SAFETY: header words live at offset 0 (active) and 8..24 (seq pair),
    // naturally aligned; the writer is not running concurrently.
    unsafe {
        let active = *map.as_ptr().cast::<u64>();
        let seq = &*map
            .as_mut_ptr()
            .add(8 + active as usize * 8)
            .cast::<AtomicU64>();
        let current = seq.load(Ordering::Acquire);
        seq.store(current | 1, Ordering::Release);
    }
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_checksum_detect_child() {
    let state = required_path("AURA_MP_STATE");
    let report = required_path("AURA_MP_REPORT");
    let map = open_read_map(&state);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        // SAFETY: the mapping is live for the duration of the read.
        if let Ok(snapshot) = unsafe { read_double_buffer(map.as_ptr()) } {
            assert!(
                !verify_snapshot(&snapshot),
                "corrupted snapshot must fail checksum"
            );
            std::fs::write(&report, "detected").expect("write detector report");
            return;
        }
        assert!(Instant::now() < deadline, "detector timed out");
        std::thread::yield_now();
    }
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_cli_read_child() {
    let state = required_path("AURA_MP_STATE");
    let reader = aura_cli::reader::TelemetryReader::new(&state).expect("open cli reader");
    let snapshot = reader.read().expect("cli read");
    match std::env::var("AURA_MP_MODE").as_deref() {
        Ok("stale") => {
            assert!(
                !reader.is_fresh(&snapshot, Duration::from_secs(2)),
                "ancient snapshot must fail the freshness threshold"
            );
        }
        _ => {
            let expect = std::env::var("AURA_MP_EXPECT")
                .expect("expected generation")
                .parse::<u64>()
                .unwrap();
            assert_eq!(
                snapshot.meta.timestamp_ns, expect,
                "cli observes the final generation"
            );
        }
    }
}

#[test]
#[ignore = "spawned by multiprocess parent tests"]
fn mp_offline_child() {
    let parent = required_path("AURA_MP_PARENT");
    match aura_cli::reader::TelemetryReader::new_default_under(&parent, "aura") {
        Err(AuraError::Offline(_)) => {}
        other => panic!("absent default state must classify Offline: {other:?}"),
    }
}
