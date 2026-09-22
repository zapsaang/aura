//! Todo 14 CLI error classification contract: every CLI-observable
//! failure class prints its exact golden line on stdout, keeps stderr
//! empty, and exits 1; success paths keep exit 0. Offline covers
//! absent/not-published/stale/future/retry-exhaustion; version,
//! checksum, structure, security, size, and header are never
//! downgraded. Multi-fault inputs resolve version -> CRC -> structure.
//!
//! Lock-leaf reasons (`lock ...: expected size 0, found N`, `lock is
//! held by another daemon`) are daemon-only: the CLI never opens the
//! lock leaf, and the daemon precedence order type->owner->mode->size
//! is locked by `aura-daemon/tests/shm_security.rs`. They are golden
//! here at the rendered-line level.

#![cfg(unix)]

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{SystemTime, UNIX_EPOCH};

use aura_cli::args::{Args, ColorMode, Module, OutputFormat};
use aura_common::runtime::RuntimeLocation;
use aura_common::{
    monotonic_ns, write_double_buffer, AuraError, TelemetryArchive, ARCHIVE_VERSION, SHM_SIZE,
};
use memmap2::MmapOptions;

const OFFLINE_LINE: &str = "[AURA: OFFLINE]\n";

fn test_dir(tag: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aura-error-contract-{tag}-{}-{ts}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    std::fs::canonicalize(&dir).unwrap()
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn minimal_valid() -> TelemetryArchive {
    // SAFETY: `TelemetryArchive` is `Zeroable`; the all-zero bit pattern is valid.
    let mut a = unsafe { std::mem::zeroed::<TelemetryArchive>() };
    a.version = ARCHIVE_VERSION;
    a.meta.timestamp_ns = 1;
    a
}

fn checksum_of(archive: &TelemetryArchive) -> u32 {
    let mut snapshot = *archive;
    snapshot.checksum = 0;
    snapshot.calculate_checksum()
}

/// Publish the archive bytes exactly as given (checksum untouched).
fn write_shm_raw(path: &Path, archive: &TelemetryArchive) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(SHM_SIZE as u64).unwrap();
    // SAFETY: the file was just sized to `SHM_SIZE`, matching the mapping length.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    // SAFETY: the mapping is a full writable SHM region and the archive is initialized.
    unsafe {
        write_double_buffer(mmap.as_mut_ptr(), archive).expect("publish archive");
    }
    mmap.flush().unwrap();
}

/// Publish the archive with a correct checksum.
fn write_shm_valid(path: &Path, archive: &TelemetryArchive) {
    let mut snapshot = *archive;
    snapshot.checksum = checksum_of(&snapshot);
    write_shm_raw(path, &snapshot);
}

/// Create a raw state leaf with an exact size and forced mode.
fn create_raw_state(path: &Path, size: u64, mode: u32) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(size).unwrap();
    drop(file);
    // `OpenOptions::mode` is umask-masked; force the exact fixture mode.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn run_cli(path: Option<&Path>) -> Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"));
    command.args(["-m", "cpu", "--color", "none"]);
    if let Some(path) = path {
        command.arg("--shm-path").arg(path);
    }
    command.output().unwrap()
}

fn run_cli_raw_arg(raw: &[u8]) -> Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"));
    command
        .args(["-m", "cpu", "--color", "none", "--shm-path"])
        .arg(OsStr::from_bytes(raw));
    command.output().unwrap()
}

fn assert_offline(out: &Output) {
    assert_eq!(out.status.code(), Some(1), "offline must exit 1");
    assert!(out.stderr.is_empty(), "stderr must stay empty");
    assert_eq!(out.stdout, OFFLINE_LINE.as_bytes());
}

fn assert_error_line(out: &Output, detail: &str) {
    assert_eq!(out.status.code(), Some(1), "error must exit 1");
    assert!(out.stderr.is_empty(), "stderr must stay empty");
    let expected = format!("[AURA: ERROR - {detail}]\n");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        expected,
        "exact stdout golden"
    );
}

fn error_line(err: &AuraError) -> String {
    format!("[AURA: ERROR - {err}]\n")
}

// ---------------------------------------------------------------------
// Offline class: absent, not published, stale, future, retry exhaustion
// ---------------------------------------------------------------------

#[test]
fn binary_default_absent_is_offline() {
    // The platform default is environment-fixed; only a genuinely absent
    // default runtime location can prove this golden, so skip when a live
    // daemon location resolves on this host.
    if RuntimeLocation::resolve_cli_default().is_ok() {
        eprintln!("skip: platform default runtime location is present");
        return;
    }
    let out = run_cli(None);
    assert_offline(&out);
}

#[test]
fn binary_not_published_is_offline() {
    let dir = test_dir("not-published");
    let path = dir.join("state.dat");
    create_raw_state(&path, SHM_SIZE as u64, 0o600);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_offline(&out);
}

#[test]
fn binary_stale_is_offline() {
    let dir = test_dir("stale");
    let path = dir.join("state.dat");
    let archive = minimal_valid(); // timestamp_ns = 1, far beyond the 2s threshold
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_offline(&out);
}

#[test]
fn binary_future_timestamp_is_offline() {
    let dir = test_dir("future");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.meta.timestamp_ns = monotonic_ns().saturating_add(60_000_000_000);
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_offline(&out);
}

#[test]
fn binary_retry_exhaustion_is_offline() {
    let dir = test_dir("retry");
    let path = dir.join("state.dat");
    create_raw_state(&path, SHM_SIZE as u64, 0o600);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    // SAFETY: the file is exactly SHM_SIZE and mapped writable.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    // Pin sequence word 0 odd forever: the read deadline (10 ms) exhausts.
    mmap[8..16].copy_from_slice(&1u64.to_le_bytes());
    mmap.flush().unwrap();
    drop(mmap);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_offline(&out);
}

// ---------------------------------------------------------------------
// Error class: checksum, version, structure, header, size
// ---------------------------------------------------------------------

#[test]
fn binary_checksum_mismatch_exact_bytes() {
    let dir = test_dir("checksum-mismatch");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.meta.timestamp_ns = monotonic_ns();
    let good = checksum_of(&archive);
    archive.checksum = good ^ 0x5A5A;
    write_shm_raw(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(
        &out,
        &format!(
            "Data checksum mismatch: expected 0x{:08x}, got 0x{good:08x}",
            good ^ 0x5A5A
        ),
    );
}

#[test]
fn binary_version_mismatch_exact_bytes() {
    let dir = test_dir("version");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.version = 3;
    archive.meta.timestamp_ns = monotonic_ns();
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(&out, "ABI version mismatch: expected 2, found 3");
}

#[test]
fn binary_structure_fault_exact_bytes() {
    let dir = test_dir("structure");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.meta.timestamp_ns = monotonic_ns();
    archive.capabilities = 1 << 40;
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Invalid archive: field capabilities: unknown bits 0x0000010000000000",
    );
}

#[test]
fn binary_header_active_buffer_exact_bytes() {
    let dir = test_dir("header");
    let path = dir.join("state.dat");
    create_raw_state(&path, SHM_SIZE as u64, 0o600);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    // SAFETY: the file is exactly SHM_SIZE and mapped writable.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    mmap[0..8].copy_from_slice(&2u64.to_le_bytes());
    mmap.flush().unwrap();
    drop(mmap);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(&out, "Invalid shared memory header: active buffer 2");
}

#[test]
fn binary_incompatible_size_exact_bytes() {
    let dir = test_dir("size");
    let path = dir.join("state.dat");
    create_raw_state(&path, 1024, 0o600);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Incompatible shared memory size: expected 131096, found 1024",
    );
}

// ---------------------------------------------------------------------
// Security class: exact Todo 2 reasons, hostile text never emitted
// ---------------------------------------------------------------------

#[test]
fn binary_override_parent_missing_security() {
    let dir = test_dir("parent-missing");
    let missing = dir.join("absent").join("state.dat");
    let out = run_cli(Some(&missing));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Security validation failed: override parent is not trusted",
    );
}

#[test]
fn binary_state_symlink_security() {
    let dir = test_dir("symlink");
    let target = dir.join("target.dat");
    create_raw_state(&target, SHM_SIZE as u64, 0o600);
    let link = dir.join("state.dat");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let out = run_cli(Some(&link));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Security validation failed: component state.dat: symlink not allowed",
    );
}

#[test]
fn binary_state_leaf_directory_security() {
    let dir = test_dir("leaf-dir");
    std::fs::create_dir(dir.join("state.dat")).unwrap();
    let out = run_cli(Some(&dir.join("state.dat")));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Security validation failed: leaf state.dat: expected regular file",
    );
}

#[test]
fn binary_state_wrong_mode_security() {
    let dir = test_dir("wrong-mode");
    let path = dir.join("state.dat");
    create_raw_state(&path, SHM_SIZE as u64, 0o644);
    let out = run_cli(Some(&path));
    let kept = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    cleanup(&dir);
    assert_eq!(kept, 0o644, "reader never chmods");
    assert_error_line(
        &out,
        "Security validation failed: component state.dat: expected mode 0600, found 0644",
    );
}

#[test]
fn binary_path_not_utf8_security() {
    let out = run_cli_raw_arg(b"/tmp/aura-\xff/state.dat");
    assert!(
        !out.stdout.contains(&0xFF),
        "raw hostile bytes never emitted"
    );
    assert_error_line(&out, "Security validation failed: path is not valid UTF-8");
}

#[test]
fn binary_path_forbidden_component_security() {
    let out = run_cli(Some(Path::new("/tmp/../state.dat")));
    assert_error_line(
        &out,
        "Security validation failed: path contains forbidden component ..",
    );
}

#[test]
fn binary_path_control_newline_security() {
    let out = run_cli(Some(Path::new("/tmp/evil\npath/state.dat")));
    assert_eq!(
        out.stdout.iter().filter(|&&b| b == b'\n').count(),
        1,
        "the only newline is the line terminator"
    );
    assert_error_line(
        &out,
        "Security validation failed: path contains control character U+000A",
    );
}

#[test]
fn binary_path_control_cr_security() {
    let out = run_cli(Some(Path::new("/tmp/evil\rpath/state.dat")));
    assert!(!out.stdout.contains(&b'\r'), "raw CR never emitted");
    assert_error_line(
        &out,
        "Security validation failed: path contains control character U+000D",
    );
}

#[test]
fn binary_path_control_tab_security() {
    let out = run_cli(Some(Path::new("/tmp/evil\tpath/state.dat")));
    assert!(!out.stdout.contains(&b'\t'), "raw tab never emitted");
    assert_error_line(
        &out,
        "Security validation failed: path contains control character U+0009",
    );
}

#[test]
fn binary_path_control_esc_security() {
    let out = run_cli(Some(Path::new("/tmp/evil\u{1b}path/state.dat")));
    assert!(!out.stdout.contains(&0x1B), "raw ESC never emitted");
    assert_error_line(
        &out,
        "Security validation failed: path contains control character U+001B",
    );
}

#[test]
fn binary_ancestor_permission_denied_security() {
    let dir = test_dir("perm");
    let group_writable = dir.join("gw");
    std::fs::create_dir(&group_writable).unwrap();
    std::fs::set_permissions(&group_writable, std::fs::Permissions::from_mode(0o777)).unwrap();
    let inner = group_writable.join("inner");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&inner)
        .unwrap();
    let state = inner.join("state.dat");
    let out = run_cli(Some(&state));
    std::fs::set_permissions(&group_writable, std::fs::Permissions::from_mode(0o700)).unwrap();
    cleanup(&dir);
    assert_error_line(
        &out,
        "Security validation failed: component gw: permission denied",
    );
}

#[test]
fn binary_component_expected_directory_security() {
    let dir = test_dir("notdir");
    let file_in_chain = dir.join("notdir");
    create_raw_state(&file_in_chain, 0, 0o600);
    let out = run_cli(Some(&file_in_chain.join("state.dat")));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Security validation failed: component notdir: expected directory",
    );
}

// ---------------------------------------------------------------------
// Mixed-fault precedence and multiply-invalid structure
// ---------------------------------------------------------------------

#[test]
fn binary_state_size_and_mode_faults_size_wins() {
    // Owner cannot be forged unprivileged; the size check precedes both
    // owner and mode inside `validate_state_leaf`, so size+mode suffices
    // to prove size leads the triple.
    let dir = test_dir("size-mode");
    let path = dir.join("state.dat");
    create_raw_state(&path, 512, 0o644);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Incompatible shared memory size: expected 131096, found 512",
    );
}

#[test]
fn binary_symlink_and_size_faults_symlink_wins() {
    let dir = test_dir("symlink-size");
    let target = dir.join("target.dat");
    create_raw_state(&target, 512, 0o600);
    let link = dir.join("state.dat");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let out = run_cli(Some(&link));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Security validation failed: component state.dat: symlink not allowed",
    );
}

#[test]
fn binary_version_and_crc_faults_version_wins() {
    let dir = test_dir("v-checksum");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.version = 1;
    archive.checksum = 0xDEAD_BEEF; // wrong on purpose: version must still win
    write_shm_raw(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(&out, "ABI version mismatch: expected 2, found 1");
}

#[test]
fn binary_crc_and_structure_faults_crc_wins() {
    let dir = test_dir("crc-s");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.capabilities = 1 << 40; // structure fault
    archive.checksum = 0xDEAD_BEEF; // wrong on purpose: CRC must still win
    write_shm_raw(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    let actual = checksum_of(&archive);
    assert_error_line(
        &out,
        &format!("Data checksum mismatch: expected 0xdeadbeef, got 0x{actual:08x}"),
    );
}

#[test]
fn binary_version_and_structure_faults_version_wins() {
    let dir = test_dir("v-s");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.version = 3;
    archive.capabilities = 1 << 40; // structure fault, CRC valid
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(&out, "ABI version mismatch: expected 2, found 3");
}

#[test]
fn binary_default_absent_vs_override_absent() {
    if RuntimeLocation::resolve_cli_default().is_ok() {
        eprintln!("skip: platform default runtime location is present");
        return;
    }
    let default_out = run_cli(None);
    let dir = test_dir("vs-override");
    let missing = dir.join("absent").join("state.dat");
    let override_out = run_cli(Some(&missing));
    cleanup(&dir);
    assert_offline(&default_out);
    assert_error_line(
        &override_out,
        "Security validation failed: override parent is not trusted",
    );
}

#[test]
fn binary_multiply_invalid_structure_capabilities_first() {
    let dir = test_dir("multi-caps");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.capabilities = 1 << 40; // validated before every module
    archive.cpu.user_ticks = 7; // owner-clear fault that would also fail
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(
        &out,
        "Invalid archive: field capabilities: unknown bits 0x0000010000000000",
    );
}

#[test]
fn binary_multiply_invalid_structure_cpu_before_meta() {
    let dir = test_dir("multi-cpu");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.cpu.user_ticks = 7; // cpu module is validated before meta
    archive.meta.wallclock_ns = 5; // unowned meta fault that would also fail
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_error_line(&out, "Invalid archive: field cpu.user_ticks: expected zero");
}

// ---------------------------------------------------------------------
// Exit-0 path unaffected
// ---------------------------------------------------------------------

#[test]
fn binary_valid_fresh_state_exit_zero() {
    let dir = test_dir("fresh");
    let path = dir.join("state.dat");
    let mut archive = minimal_valid();
    archive.meta.timestamp_ns = monotonic_ns();
    write_shm_valid(&path, &archive);
    let out = run_cli(Some(&path));
    cleanup(&dir);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    assert!(out.stdout.starts_with(b"CPU\n"), "human header intact");
}

// ---------------------------------------------------------------------
// Raw format rejects `all` before any shared-memory access
// ---------------------------------------------------------------------

#[test]
fn raw_binary_all_module_rejected_before_shm() {
    for args in [
        &["--format", "raw"][..],
        &["--format", "raw", "-m", "all"][..],
    ] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_aura-cli"))
            .args(args)
            .output()
            .unwrap();
        assert_error_line(
            &out,
            "invalid argument: --format raw requires a single module (got --module all)",
        );
    }
}

// ---------------------------------------------------------------------
// Rendered-line goldens for reasons the CLI cannot trigger unprivileged
// ---------------------------------------------------------------------

#[test]
fn display_security_uid_reason_line() {
    let err = AuraError::Security("component state.dat: expected uid 1000, found 0".to_string());
    assert_eq!(
        error_line(&err),
        "[AURA: ERROR - Security validation failed: component state.dat: expected uid 1000, found 0]\n"
    );
}

#[test]
fn display_lock_size_reason_line() {
    let err = AuraError::Security("lock state.dat.lock: expected size 0, found 7".to_string());
    assert_eq!(
        error_line(&err),
        "[AURA: ERROR - Security validation failed: lock state.dat.lock: expected size 0, found 7]\n"
    );
}

#[test]
fn display_lock_held_reason_line() {
    let err = AuraError::Security("lock is held by another daemon".to_string());
    assert_eq!(
        error_line(&err),
        "[AURA: ERROR - Security validation failed: lock is held by another daemon]\n"
    );
}

#[test]
fn display_checksum_zero_padded_hex() {
    let err = AuraError::ChecksumMismatch {
        expected: 0xDEAD_BEEF,
        actual: 0x0102,
    };
    assert_eq!(
        error_line(&err),
        "[AURA: ERROR - Data checksum mismatch: expected 0xdeadbeef, got 0x00000102]\n"
    );
}

#[test]
fn library_path_nul_byte_security_reason() {
    let args = Args {
        module: Module::Cpu,
        color: ColorMode::None,
        format: OutputFormat::Human,
        shm_path: Some(PathBuf::from(OsStr::from_bytes(b"/tmp/aura-\0/state.dat"))),
    };
    match aura_cli::run(args) {
        Err(AuraError::Security(reason)) => {
            assert_eq!(reason, "path contains NUL byte");
        }
        other => panic!("expected Security, got {other:?}"),
    }
}
