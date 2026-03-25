use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use aura_common::{AuraResult, SHM_SIZE};
use aura_daemon::state::ShmHandle;
use serial_test::serial;

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
fn shm_created_with_world_readable_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aura_test_perms.dat");

    let old = unsafe { libc::umask(0o077) };
    let handle = ShmHandle::new(&path);
    unsafe { libc::umask(old) };

    handle.unwrap();
    let perms = std::fs::metadata(&path).unwrap().permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o666,
        "SHM file must be 0o666 regardless of umask, got {:#o}",
        perms.mode() & 0o777
    );
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn shm_repairs_preexisting_restrictive_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aura_preexist.dat");

    std::fs::write(&path, vec![0u8; SHM_SIZE]).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let _handle = ShmHandle::new(&path).unwrap();

    let perms = std::fs::metadata(&path).unwrap().permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o666,
        "SHM must repair pre-existing 0o600 to 0o666, got {:#o}",
        perms.mode() & 0o777
    );
}

#[cfg(target_os = "linux")]
#[test]
fn reader_permission_denied_gives_helpful_error() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aura_umask_077.dat");

    let old = unsafe { libc::umask(0o077) };
    let _handle = ShmHandle::new(&path).unwrap();
    unsafe { libc::umask(old) };

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o666,
        "SHM must be 0o666 even with umask 0o077, got {:#o}",
        mode
    );
}
