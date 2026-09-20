//! Todo 2 CLI reader security contract: retained-fd no-follow reads,
//! exact validation order, offline classification for missing default
//! runtime leaves, and a strict no-create guarantee.

#![cfg(unix)]

use std::fs::OpenOptions;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aura_cli::reader::TelemetryReader;
use aura_common::{write_double_buffer, AuraError, TelemetryArchive, ARCHIVE_VERSION, SHM_SIZE};
use memmap2::MmapOptions;

fn test_dir(tag: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aura-reader-security-{tag}-{}-{ts}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    std::fs::canonicalize(&dir).unwrap()
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn write_valid_shm(path: &Path) {
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
    // SAFETY: `TelemetryArchive` is `Zeroable`; the all-zero pattern is valid.
    let mut archive = unsafe { std::mem::zeroed::<TelemetryArchive>() };
    archive.version = ARCHIVE_VERSION;
    archive.meta.timestamp_ns = 1;
    archive.checksum = archive.calculate_checksum();
    // SAFETY: the mapping is a full writable SHM region and the archive is initialized.
    unsafe {
        write_double_buffer(mmap.as_mut_ptr(), &archive).expect("publish archive");
    }
    mmap.flush().unwrap();
}

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
    // Force the exact fixture mode: `OpenOptions::mode` is umask-masked.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn security_reason(err: &AuraError) -> String {
    match err {
        AuraError::Security(reason) => reason.clone(),
        other => panic!("expected Security, got {other:?}"),
    }
}

fn entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn reads_valid_override_state() {
    let dir = test_dir("valid");
    let path = dir.join("state.dat");
    write_valid_shm(&path);
    let reader = TelemetryReader::new(&path).unwrap();
    assert_eq!(reader.read().unwrap().version, ARCHIVE_VERSION);
    cleanup(&dir);
}

#[test]
fn override_missing_parent_is_exact_reason() {
    let dir = test_dir("missing-parent");
    let missing = dir.join("absent").join("state.dat");
    let err = TelemetryReader::new(&missing).expect_err("missing parent must be rejected");
    assert_eq!(security_reason(&err), "override parent is not trusted");
    cleanup(&dir);
}

#[test]
fn override_relative_path_is_rejected() {
    let err = TelemetryReader::new(Path::new("tmp/state.dat")).expect_err("relative rejected");
    assert_eq!(security_reason(&err), "path is not absolute");
}

#[test]
fn override_non_utf8_path_is_rejected() {
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::OsStr::from_bytes(b"/tmp/aura-\xff/state.dat");
    let err = TelemetryReader::new(Path::new(raw)).expect_err("non-UTF-8 rejected");
    assert_eq!(security_reason(&err), "path is not valid UTF-8");
}

#[test]
fn override_control_character_has_exact_reason() {
    let err =
        TelemetryReader::new(Path::new("/tmp/evil\npath/state.dat")).expect_err("control rejected");
    assert_eq!(
        security_reason(&err),
        "path contains control character U+000A"
    );
}

#[test]
fn override_state_symlink_is_rejected_without_touching_target() {
    let dir = test_dir("symlink");
    let target = dir.join("target.dat");
    write_valid_shm(&target);
    let before = std::fs::read(&target).unwrap();
    let link = dir.join("state.dat");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = TelemetryReader::new(&link).expect_err("symlink state rejected");
    assert!(
        security_reason(&err).contains("symlink not allowed"),
        "unexpected: {err}"
    );
    assert_eq!(std::fs::read(&target).unwrap(), before);
    cleanup(&dir);
}

#[test]
fn override_state_wrong_size_is_size_class() {
    let dir = test_dir("wrong-size");
    let path = dir.join("state.dat");
    create_raw_state(&path, 1024, 0o600);
    match TelemetryReader::new(&path) {
        Err(AuraError::IncompatibleShmSize { expected, found }) => {
            assert_eq!(expected, SHM_SIZE as u64);
            assert_eq!(found, 1024);
        }
        other => panic!("expected IncompatibleShmSize, got {other:?}"),
    }
    cleanup(&dir);
}

#[test]
fn override_state_wrong_mode_rejected_without_mutation() {
    let dir = test_dir("wrong-mode");
    let path = dir.join("state.dat");
    create_raw_state(&path, SHM_SIZE as u64, 0o644);
    let err = TelemetryReader::new(&path).expect_err("wrong mode rejected");
    assert!(
        security_reason(&err).contains("expected mode 0600, found 0644"),
        "unexpected: {err}"
    );
    assert_eq!(mode_of(&path), 0o644, "reader never chmods");
    cleanup(&dir);
}

#[test]
fn override_state_directory_is_rejected() {
    let dir = test_dir("dir-leaf");
    std::fs::create_dir(dir.join("state.dat")).unwrap();
    let err = TelemetryReader::new(&dir.join("state.dat")).expect_err("directory rejected");
    assert_eq!(
        security_reason(&err),
        "leaf state.dat: expected regular file"
    );
    cleanup(&dir);
}

#[test]
fn override_mixed_fault_reports_size_before_mode() {
    let dir = test_dir("mixed");
    let path = dir.join("state.dat");
    create_raw_state(&path, 512, 0o644);
    match TelemetryReader::new(&path) {
        Err(AuraError::IncompatibleShmSize { found, .. }) => assert_eq!(found, 512),
        other => panic!("size must win over mode, got {other:?}"),
    }
    assert_eq!(mode_of(&path), 0o644);
    cleanup(&dir);
}

#[test]
fn reader_creates_no_aura_leaf() {
    let dir = test_dir("no-create");
    let path = dir.join("state.dat");
    assert!(TelemetryReader::new(&path).is_err());
    assert_eq!(entry_names(&dir), Vec::<String>::new(), "nothing created");
    cleanup(&dir);
}

#[test]
fn reader_retains_fd_after_state_unlink() {
    let dir = test_dir("retained-fd");
    let path = dir.join("state.dat");
    write_valid_shm(&path);
    let reader = TelemetryReader::new(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(reader.read().unwrap().version, ARCHIVE_VERSION);
    cleanup(&dir);
}

#[test]
fn default_missing_aura_dir_is_offline() {
    let dir = test_dir("default-offline-dir");
    match TelemetryReader::new_default_under(&dir, "aura") {
        Err(AuraError::Offline(_)) => {}
        other => panic!("expected Offline, got {other:?}"),
    }
    assert_eq!(entry_names(&dir), Vec::<String>::new());
    cleanup(&dir);
}

#[test]
fn default_missing_state_is_offline_and_creates_nothing() {
    let dir = test_dir("default-offline-state");
    let aura = dir.join("aura");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&aura)
        .unwrap();
    match TelemetryReader::new_default_under(&dir, "aura") {
        Err(AuraError::Offline(_)) => {}
        other => panic!("expected Offline, got {other:?}"),
    }
    assert_eq!(entry_names(&aura), Vec::<String>::new());
    cleanup(&dir);
}

#[test]
fn default_valid_state_reads() {
    let dir = test_dir("default-valid");
    let aura = dir.join("aura");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&aura)
        .unwrap();
    write_valid_shm(&aura.join("state.dat"));
    let reader = TelemetryReader::new_default_under(&dir, "aura").unwrap();
    assert_eq!(reader.read().unwrap().version, ARCHIVE_VERSION);
    cleanup(&dir);
}

#[test]
fn default_symlink_aura_dir_is_security() {
    let dir = test_dir("default-symlink");
    let real = dir.join("real");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&real)
        .unwrap();
    std::os::unix::fs::symlink(&real, dir.join("aura")).unwrap();
    let err = TelemetryReader::new_default_under(&dir, "aura").expect_err("symlink rejected");
    assert_eq!(security_reason(&err), "component aura: symlink not allowed");
    cleanup(&dir);
}
