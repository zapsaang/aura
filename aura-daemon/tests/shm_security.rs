//! Todo 2 daemon-side SHM state/lock security contract: exact leaf
//! sizes/modes, lock-first flock ordering, no mutation of hostile
//! existing leaves, atomic temp publication, and override trust rules.

#![cfg(unix)]

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use aura_common::{AuraError, TelemetryArchive, SHM_SIZE};
use aura_daemon::state::publish::nonce_hex;
use aura_daemon::state::ShmHandle;
use serial_test::serial;
use tempfile::TempDir;

fn test_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn state_path(dir: &TempDir) -> PathBuf {
    dir.path().join("state.dat")
}

fn lock_path(dir: &TempDir) -> PathBuf {
    dir.path().join("state.dat.lock")
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn bytes_of(path: &Path) -> Vec<u8> {
    let mut buf = Vec::new();
    File::open(path).unwrap().read_to_end(&mut buf).unwrap();
    buf
}

fn entry_names(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
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
    // Force the exact fixture mode: `OpenOptions::mode` is umask-masked and
    // this suite mutates the process umask in serial tests.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn create_valid_state(path: &Path) {
    create_raw_state(path, SHM_SIZE as u64, 0o600);
}

#[test]
fn state_created_with_exact_size() {
    let dir = test_dir();
    let _handle = ShmHandle::new(&state_path(&dir)).unwrap();
    assert_eq!(
        std::fs::metadata(state_path(&dir)).unwrap().len(),
        SHM_SIZE as u64
    );
}

#[test]
fn state_created_with_mode_0600() {
    let dir = test_dir();
    let _handle = ShmHandle::new(&state_path(&dir)).unwrap();
    assert_eq!(mode_of(&state_path(&dir)), 0o600);
}

#[test]
fn lock_created_empty_0600_with_derived_name() {
    let dir = test_dir();
    let _handle = ShmHandle::new(&state_path(&dir)).unwrap();
    let lock = lock_path(&dir);
    assert_eq!(std::fs::metadata(&lock).unwrap().len(), 0);
    assert_eq!(mode_of(&lock), 0o600);
}

#[test]
#[serial]
fn leaves_stay_0600_under_umask_077() {
    let dir = test_dir();
    // SAFETY: `umask` accepts any mask; this serial test restores the previous mask.
    let old = unsafe { libc::umask(0o077) };
    let handle = ShmHandle::new(&state_path(&dir));
    // SAFETY: `old` was returned by `umask`, so restoring it is valid.
    unsafe { libc::umask(old) };
    handle.unwrap();
    assert_eq!(mode_of(&state_path(&dir)), 0o600);
    assert_eq!(mode_of(&lock_path(&dir)), 0o600);
}

#[test]
#[serial]
fn leaves_stay_0600_under_umask_000() {
    let dir = test_dir();
    // SAFETY: `umask` accepts any mask; this serial test restores the previous mask.
    let old = unsafe { libc::umask(0o000) };
    let handle = ShmHandle::new(&state_path(&dir));
    // SAFETY: `old` was returned by `umask`, so restoring it is valid.
    unsafe { libc::umask(old) };
    handle.unwrap();
    assert_eq!(mode_of(&state_path(&dir)), 0o600);
    assert_eq!(mode_of(&lock_path(&dir)), 0o600);
}

#[test]
fn valid_existing_state_is_reopened_not_recreated() {
    let dir = test_dir();
    let path = state_path(&dir);
    create_valid_state(&path);
    let inode_before = std::fs::metadata(&path).unwrap().ino();
    let _handle = ShmHandle::new(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode_before);
    assert_eq!(mode_of(&path), 0o600);
}

#[test]
fn second_instance_gets_exact_lock_reason() {
    let dir = test_dir();
    let _first = ShmHandle::new(&state_path(&dir)).unwrap();
    let err = ShmHandle::new(&state_path(&dir)).expect_err("second daemon must fail");
    assert_eq!(security_reason(&err), "lock is held by another daemon");
}

#[test]
fn second_instance_mutates_no_runtime_leaf() {
    let dir = test_dir();
    let _first = ShmHandle::new(&state_path(&dir)).unwrap();
    let names_before = entry_names(&dir);
    let state_before = bytes_of(&state_path(&dir));
    let lock_before = bytes_of(&lock_path(&dir));
    assert!(ShmHandle::new(&state_path(&dir)).is_err());
    assert_eq!(entry_names(&dir), names_before, "no leaf added/removed");
    assert_eq!(bytes_of(&state_path(&dir)), state_before);
    assert_eq!(bytes_of(&lock_path(&dir)), lock_before);
}

#[test]
fn lock_conflict_precedes_state_validation() {
    let dir = test_dir();
    let state = state_path(&dir);
    create_raw_state(&state, 100, 0o600);
    let lock = lock_path(&dir);
    let held = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&lock)
        .unwrap();
    // SAFETY: `held` is a live lock fd and `flock` takes no pointers.
    assert_eq!(
        unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let err = ShmHandle::new(&state).expect_err("held lock must win over state faults");
    assert_eq!(security_reason(&err), "lock is held by another daemon");
    assert_eq!(std::fs::metadata(&state).unwrap().len(), 100);
}

#[test]
fn flock_is_released_when_handle_drops() {
    let dir = test_dir();
    let path = state_path(&dir);
    drop(ShmHandle::new(&path).unwrap());
    ShmHandle::new(&path).expect("lock must be reacquirable after drop");
}

#[test]
fn wrong_size_state_is_rejected_with_size_class() {
    let dir = test_dir();
    let path = state_path(&dir);
    create_raw_state(&path, 100, 0o600);
    match ShmHandle::new(&path) {
        Err(AuraError::IncompatibleShmSize { expected, found }) => {
            assert_eq!(expected, SHM_SIZE as u64);
            assert_eq!(found, 100);
        }
        other => panic!("expected IncompatibleShmSize, got {other:?}"),
    }
}

#[test]
fn wrong_size_state_is_never_truncated() {
    let dir = test_dir();
    let path = state_path(&dir);
    create_raw_state(&path, 100, 0o600);
    assert!(ShmHandle::new(&path).is_err());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 100);
}

#[test]
fn wrong_size_wins_over_wrong_mode() {
    let dir = test_dir();
    let path = state_path(&dir);
    create_raw_state(&path, 100, 0o644);
    match ShmHandle::new(&path) {
        Err(AuraError::IncompatibleShmSize { found, .. }) => assert_eq!(found, 100),
        other => panic!("size fault must win over mode fault, got {other:?}"),
    }
    assert_eq!(mode_of(&path), 0o644, "hostile leaf never mutated");
}

#[test]
fn wrong_mode_state_is_rejected_without_chmod() {
    let dir = test_dir();
    let path = state_path(&dir);
    create_raw_state(&path, SHM_SIZE as u64, 0o644);
    let err = ShmHandle::new(&path).expect_err("wrong mode must be rejected");
    assert!(
        security_reason(&err).contains("expected mode 0600, found 0644"),
        "unexpected: {err}"
    );
    assert_eq!(mode_of(&path), 0o644, "existing objects are never chmodded");
}

fn security_reason(err: &AuraError) -> String {
    match err {
        AuraError::Security(reason) => reason.clone(),
        other => panic!("expected Security, got {other:?}"),
    }
}

#[test]
fn symlink_state_is_rejected_and_target_untouched() {
    let dir = test_dir();
    let target = dir.path().join("target.dat");
    create_valid_state(&target);
    let before = bytes_of(&target);
    let link = dir.path().join("state.dat");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = ShmHandle::new(&link).expect_err("symlink must be rejected");
    assert!(
        security_reason(&err).contains("symlink not allowed"),
        "unexpected: {err}"
    );
    assert_eq!(bytes_of(&target), before);
}

#[test]
fn directory_at_state_path_is_rejected() {
    let dir = test_dir();
    let path = state_path(&dir);
    std::fs::create_dir(&path).unwrap();
    let err = ShmHandle::new(&path).expect_err("directory must be rejected");
    assert_eq!(
        security_reason(&err),
        "leaf state.dat: expected regular file"
    );
}

#[test]
fn symlink_lock_is_rejected() {
    let dir = test_dir();
    let target = dir.path().join("lock-target");
    File::create(&target).unwrap();
    let link = lock_path(&dir);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = ShmHandle::new(&state_path(&dir)).expect_err("symlink lock must be rejected");
    assert!(
        security_reason(&err).contains("symlink not allowed"),
        "unexpected: {err}"
    );
}

#[test]
fn directory_lock_is_rejected() {
    let dir = test_dir();
    std::fs::create_dir(lock_path(&dir)).unwrap();
    let err = ShmHandle::new(&state_path(&dir)).expect_err("directory lock must be rejected");
    assert_eq!(
        security_reason(&err),
        "leaf state.dat.lock: expected regular file"
    );
}

#[test]
fn wrong_mode_lock_is_rejected_without_chmod() {
    let dir = test_dir();
    let lock = lock_path(&dir);
    create_raw_state(&lock, 0, 0o644);
    let err = ShmHandle::new(&state_path(&dir)).expect_err("wrong-mode lock must be rejected");
    assert!(
        security_reason(&err).contains("expected mode 0600, found 0644"),
        "unexpected: {err}"
    );
    assert_eq!(mode_of(&lock), 0o644);
}

#[test]
fn nonzero_lock_is_rejected() {
    let dir = test_dir();
    let lock = lock_path(&dir);
    create_raw_state(&lock, 7, 0o600);
    let err = ShmHandle::new(&state_path(&dir)).expect_err("nonzero lock must be rejected");
    assert_eq!(
        security_reason(&err),
        "lock state.dat.lock: expected size 0, found 7"
    );
}

#[test]
fn lock_mode_fault_wins_over_size_fault() {
    let dir = test_dir();
    let lock = lock_path(&dir);
    create_raw_state(&lock, 7, 0o644);
    let err = ShmHandle::new(&state_path(&dir)).expect_err("lock faults must be rejected");
    let reason = security_reason(&err);
    assert!(
        reason.contains("expected mode 0600"),
        "mode must precede size, got: {reason}"
    );
}

#[test]
fn override_parent_missing_has_exact_reason() {
    let dir = test_dir();
    let missing = dir.path().join("absent").join("state.dat");
    let err = ShmHandle::new(&missing).expect_err("missing parent must be rejected");
    assert_eq!(security_reason(&err), "override parent is not trusted");
}

#[test]
fn override_parent_with_wrong_mode_is_rejected() {
    let dir = test_dir();
    let parent = dir.path().join("loose");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = ShmHandle::new(&parent.join("state.dat")).expect_err("loose parent rejected");
    assert!(
        security_reason(&err).contains("expected mode 0700, found 0755"),
        "unexpected: {err}"
    );
}

#[test]
fn override_relative_path_is_rejected() {
    let err = ShmHandle::new(Path::new("tmp/state.dat")).expect_err("relative path rejected");
    assert_eq!(security_reason(&err), "path is not absolute");
}

#[test]
fn override_non_utf8_path_is_rejected() {
    use std::os::unix::ffi::OsStrExt;
    let raw = std::ffi::OsStr::from_bytes(b"/tmp/aura-\xff/state.dat");
    let err = ShmHandle::new(Path::new(raw)).expect_err("non-UTF-8 path rejected");
    assert_eq!(security_reason(&err), "path is not valid UTF-8");
}

#[test]
fn stale_temp_is_never_scanned_or_deleted() {
    let dir = test_dir();
    let stale = dir
        .path()
        .join(".state.dat.tmp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    std::fs::write(&stale, b"stale").unwrap();
    let _handle = ShmHandle::new(&state_path(&dir)).unwrap();
    assert_eq!(bytes_of(&stale), b"stale", "leftover temps stay untouched");
}

#[test]
fn no_temp_remains_after_successful_create() {
    let dir = test_dir();
    let _handle = ShmHandle::new(&state_path(&dir)).unwrap();
    assert_eq!(
        entry_names(&dir),
        vec!["state.dat".to_string(), "state.dat.lock".to_string()]
    );
}

#[test]
fn nonces_are_32_lowercase_hex_and_unique() {
    let first = nonce_hex().unwrap();
    let second = nonce_hex().unwrap();
    assert_eq!(first.len(), 32);
    assert!(first
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(first, second);
}

#[test]
fn written_data_survives_reopen() {
    let dir = test_dir();
    let path = state_path(&dir);
    {
        let mut handle = ShmHandle::new(&path).unwrap();
        let mut archive = TelemetryArchive::zeroed();
        archive.version = aura_common::ARCHIVE_VERSION;
        archive.checksum = archive.calculate_checksum();
        handle.write(&archive).unwrap();
    }
    let bytes = bytes_of(&path);
    let active = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) & 1;
    let offset = 24 + (active as usize) * 65536;
    let version = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    assert_eq!(version, aura_common::ARCHIVE_VERSION);
}
