//! Lock leaf creation/validation and exclusive non-blocking `flock`.
//!
//! Existing lock leaves are validated in the exact order regular type,
//! owner euid, mode 0600, size 0, and are never mutated. A missing lock
//! is created with `O_CREAT|O_EXCL|O_NOFOLLOW`, immediately fchmodded to
//! 0600 via its own fd, then proven by fstat. `flock(LOCK_EX|LOCK_NB)`
//! is acquired before any state/temp operation; a held lock maps to the
//! exact security reason `lock is held by another daemon`.

use std::fs::File;
use std::io;
use std::os::unix::io::AsRawFd;

use aura_common::runtime::{security, RuntimeLocation};
use aura_common::{AuraError, AuraResult};

#[derive(Debug)]
pub struct StateLock {
    _file: File,
}

/// Lock leaf order: regular type, owner euid, mode 0600, size 0.
#[allow(clippy::unnecessary_cast)] // st_mode/S_IFMT widths differ between Linux and macOS
fn validate_lock_leaf(name: &str, stat: &libc::stat, euid: u32) -> AuraResult<()> {
    if (stat.st_mode as u32) & (libc::S_IFMT as u32) != (libc::S_IFREG as u32) {
        return Err(AuraError::Security(format!(
            "leaf {name}: expected regular file"
        )));
    }
    if stat.st_uid != euid {
        return Err(AuraError::Security(format!(
            "component {name}: expected uid {euid}, found {}",
            stat.st_uid
        )));
    }
    let mode = (stat.st_mode as u32) & 0o777;
    if mode != 0o600 {
        return Err(AuraError::Security(format!(
            "component {name}: expected mode 0600, found {mode:04o}"
        )));
    }
    if stat.st_size != 0 {
        return Err(AuraError::Security(format!(
            "lock {name}: expected size 0, found {}",
            stat.st_size
        )));
    }
    Ok(())
}

fn open_or_create_lock(location: &RuntimeLocation, name: &str) -> AuraResult<File> {
    match security::open_leaf(location.dir_file(), name, true) {
        Ok(file) => {
            let stat = security::fstat(&file)?;
            validate_lock_leaf(name, &stat, security::euid())?;
            Ok(file)
        }
        Err(AuraError::SharedMemory(err)) if err.raw_os_error() == Some(libc::ENOENT) => {
            match security::create_leaf(location.dir_file(), name, 0o600) {
                Ok(file) => {
                    security::fchmod_new(&file, name, 0o600)?;
                    let stat = security::fstat(&file)?;
                    validate_lock_leaf(name, &stat, security::euid())?;
                    Ok(file)
                }
                Err(AuraError::SharedMemory(err)) if err.raw_os_error() == Some(libc::EEXIST) => {
                    let file = security::open_leaf(location.dir_file(), name, true)?;
                    let stat = security::fstat(&file)?;
                    validate_lock_leaf(name, &stat, security::euid())?;
                    Ok(file)
                }
                Err(err) => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

pub fn acquire(location: &RuntimeLocation) -> AuraResult<StateLock> {
    let file = open_or_create_lock(location, location.lock_name())?;
    // SAFETY: `file` is the live lock fd and `flock` takes no pointers; the
    // return value is checked.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(AuraError::Security(
                "lock is held by another daemon".to_string(),
            ));
        }
        return Err(err.into());
    }
    Ok(StateLock { _file: file })
}
