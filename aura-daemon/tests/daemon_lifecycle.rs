use std::fs::{File, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use aura_common::{read_double_buffer, AuraError, TelemetryArchive, SHM_SIZE};
use aura_daemon::collectors::CollectorState;
use aura_daemon::daemon::{install_signals, logging_filter, DaemonConfig, SignalInstaller};
use aura_daemon::lifecycle::Heartbeat;
use aura_daemon::state::ShmHandle;
use memmap2::{Mmap, MmapOptions};
use tempfile::TempDir;

fn private_state_path() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temporary directory");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private directory mode");
    let path = dir.path().join("state.dat");
    (dir, path)
}

fn map_state(path: &std::path::Path) -> Mmap {
    let file = File::open(path).expect("state file");
    // SAFETY: the daemon created an exact-SHM_SIZE state file and the mapping
    // remains read-only for the lifetime of the returned Mmap.
    unsafe { MmapOptions::new().len(SHM_SIZE).map(&file).expect("map") }
}

#[test]
fn state_is_unpublished_before_the_first_write() {
    // Given: a newly initialized daemon state mapping.
    let (_dir, path) = private_state_path();
    let handle = ShmHandle::new(&path).expect("state handle");
    drop(handle);

    // When: a reader attempts to load an archive before publication.
    let map = map_state(&path);
    // SAFETY: map_state returns a live exact-SHM_SIZE mapping initialized by ShmHandle.
    let result = unsafe { read_double_buffer(map.as_ptr()) };

    // Then: the state is observably not published.
    assert!(matches!(result, Err(AuraError::NotPublished)));
}

#[test]
fn a_pre_finalized_archive_roundtrips_through_shared_memory() {
    // Given: a checksum-valid archive and an initialized state mapping.
    let (_dir, path) = private_state_path();
    let mut handle = ShmHandle::new(&path).expect("state handle");
    let mut archive = TelemetryArchive::zeroed();
    archive.version = aura_common::ARCHIVE_VERSION;
    archive.meta.timestamp_ns = 7;
    archive.checksum = archive.calculate_checksum();

    // When: the daemon publishes the archive.
    handle.write(&archive).expect("publish");

    // Then: the same finalized payload is observable by a reader.
    let map = map_state(&path);
    // SAFETY: map_state returns a live exact-SHM_SIZE mapping initialized by ShmHandle.
    let observed = unsafe { read_double_buffer(map.as_ptr()) }.expect("read published archive");
    assert_eq!(observed.version, aura_common::ARCHIVE_VERSION);
    assert_eq!(observed.meta.timestamp_ns, 7);
    assert_eq!(observed.checksum, observed.calculate_checksum());
}

#[test]
fn shared_memory_write_does_not_mutate_the_finalized_archive() {
    let (_dir, path) = private_state_path();
    let mut handle = ShmHandle::new(&path).expect("state handle");
    let mut archive = TelemetryArchive::zeroed();
    archive.version = 91;
    archive.checksum = 17;
    handle.write(&archive).expect("publish exact bytes");
    assert_eq!(archive.version, 91);
    assert_eq!(archive.checksum, 17);
}

#[test]
fn a_pre_requested_shutdown_exits_without_publication() {
    // Given: an initialized daemon whose shutdown flag is already set.
    let (_dir, path) = private_state_path();
    let handle = ShmHandle::new(&path).expect("state handle");
    let state = CollectorState::new();
    let shutdown = AtomicBool::new(true);

    // When: the heartbeat lifecycle starts.
    aura_daemon::heartbeat::run(handle, state, Duration::from_millis(1), &shutdown)
        .expect("clean shutdown");

    // Then: no collection was published.
    let file = OpenOptions::new()
        .read(true)
        .open(&path)
        .expect("state file");
    // SAFETY: ShmHandle created the exact-SHM_SIZE file and the mapping is read-only.
    let map = unsafe { MmapOptions::new().len(SHM_SIZE).map(&file).expect("map") };
    // SAFETY: the mapping is live, exact-sized, and initialized by ShmHandle.
    let result = unsafe { read_double_buffer(map.as_ptr()) };
    assert!(matches!(result, Err(AuraError::NotPublished)));
}

#[test]
fn zero_heartbeat_is_rejected() {
    assert!(Heartbeat::from_millis(0).is_err());
}

#[test]
fn positive_heartbeat_is_preserved_exactly() {
    let heartbeat = Heartbeat::from_millis(27).expect("positive heartbeat");
    assert_eq!(heartbeat.duration(), Duration::from_millis(27));
}

#[test]
fn default_logging_is_info_and_verbose_logging_is_debug() {
    assert_eq!(logging_filter(false, None), "info");
    assert_eq!(logging_filter(true, None), "debug");
}

#[test]
fn explicit_rust_log_wins_over_verbose_default() {
    assert_eq!(logging_filter(true, Some("off")), "off");
}

struct FailingSignals;

impl SignalInstaller for FailingSignals {
    fn install(&mut self) -> aura_common::AuraResult<()> {
        Err(AuraError::Fatal("sigaction fault".to_string()))
    }
}

#[test]
fn signal_registration_failure_is_fatal() {
    let error = install_signals(&mut FailingSignals).expect_err("registration must fail");
    assert!(matches!(error, AuraError::Fatal(message) if message == "sigaction fault"));
}

#[test]
fn heartbeat_validation_precedes_state_initialization() {
    let config = DaemonConfig {
        shm_path: Some("relative/path".into()),
        heartbeat_ms: 0,
        verbose: false,
        foreground: true,
    };
    let error = aura_daemon::run(config).expect_err("zero heartbeat");
    assert!(matches!(error, AuraError::Fatal(message) if message.contains("heartbeat")));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_and_signal(signal: libc::c_int) -> Output {
    let (dir, path) = private_state_path();
    let mut child = Command::new(env!("CARGO_BIN_EXE_aura-daemon"))
        .arg("--shm-path")
        .arg(&path)
        .arg("--heartbeat-ms")
        .arg("100")
        .env("RUST_LOG", "off")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn daemon");
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if Instant::now() >= ready_deadline {
            child.kill().expect("kill unready daemon");
            child.wait().expect("reap unready daemon");
            panic!("daemon readiness deadline");
        }
        if child.try_wait().expect("child state").is_some() {
            child.wait().expect("reap early daemon exit");
            panic!("daemon exited before readiness");
        }
        std::thread::yield_now();
    }
    // SAFETY: the child PID is live, and SIGINT/SIGTERM are valid process signals.
    assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
    let exit_deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().expect("child state").is_none() {
        if Instant::now() >= exit_deadline {
            child.kill().expect("kill timed-out daemon");
            panic!("daemon exit deadline");
        }
        std::thread::yield_now();
    }
    drop(dir);
    child.wait_with_output().expect("daemon output")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sigint_exits_zero() {
    assert!(run_and_signal(libc::SIGINT).status.success());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sigterm_exits_zero() {
    assert!(run_and_signal(libc::SIGTERM).status.success());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn rust_log_off_keeps_successful_shutdown_streams_empty() {
    let output = run_and_signal(libc::SIGTERM);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"");
    assert_eq!(output.stderr, b"");
}
