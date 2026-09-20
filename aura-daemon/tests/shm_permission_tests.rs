use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aura_common::{AuraResult, SHM_SIZE};
use aura_daemon::state::ShmHandle;
use serial_test::serial;

fn test_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

#[derive(Debug)]
struct TelemetryReader;

impl TelemetryReader {
    fn new(path: &Path) -> AuraResult<Self> {
        std::fs::OpenOptions::new().read(true).open(path)?;
        Ok(Self)
    }
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn shm_created_with_owner_only_permissions() {
    let dir = test_dir();
    let path = dir.path().join("aura_test_perms.dat");

    // SAFETY: `umask` accepts any mode bits; this serial test restores the previous process mask immediately after creation.
    let old = unsafe { libc::umask(0o077) };
    let handle = ShmHandle::new(&path);
    // SAFETY: `old` was returned by `umask`, so restoring it is valid.
    unsafe { libc::umask(old) };

    handle.unwrap();
    let perms = std::fs::metadata(&path).unwrap().permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "SHM file must be 0o600 regardless of umask, got {:#o}",
        perms.mode() & 0o777
    );
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn shm_rejects_wrong_permissions() {
    let dir = test_dir();
    let path = dir.path().join("aura_preexist.dat");

    std::fs::write(&path, vec![0u8; SHM_SIZE]).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    match ShmHandle::new(&path) {
        Ok(_) => panic!("Should reject wrong permissions"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("expected mode 0600"),
                "Error should mention wrong mode, got: {}",
                msg
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn reader_permission_denied_gives_helpful_error() {
    let dir = test_dir();
    let path = dir.path().join("aura_noperm.dat");

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    file.set_len(SHM_SIZE as u64).unwrap();
    drop(file);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

    let err = TelemetryReader::new(&path).unwrap_err();
    let msg = err.to_string();
    // Error message should mention permission denied (actual enrichment happens in real TelemetryReader)
    assert!(
        msg.contains("Permission denied"),
        "Error should mention 'Permission denied', got: {}",
        msg
    );

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn shm_permissions_survive_umask_077() {
    let dir = test_dir();
    let path = dir.path().join("aura_umask_077.dat");

    // SAFETY: `umask` accepts any mode bits; this serial test restores the previous process mask immediately after creation.
    let old = unsafe { libc::umask(0o077) };
    let _handle = ShmHandle::new(&path).unwrap();
    // SAFETY: `old` was returned by `umask`, so restoring it is valid.
    unsafe { libc::umask(old) };

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "SHM must be 0o600 even with umask 0o077, got {:#o}",
        mode
    );
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn shm_rejects_symlink_attack() {
    let dir = test_dir();
    let target = dir.path().join("real_file.dat");
    let link = dir.path().join("aura_symlink.dat");

    std::fs::write(&target, vec![0u8; SHM_SIZE]).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    match ShmHandle::new(&link) {
        Ok(_) => panic!("Should reject symlink"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("symlink"),
                "Error should mention symlink, got: {}",
                msg
            );
        }
    }
}
