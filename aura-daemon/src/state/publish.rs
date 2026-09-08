//! State leaf validation, temporary creation, and atomic publication.
//!
//! Existing state is opened no-follow relative to the retained fd and
//! validated (regular type, size 131096, owner euid, mode 0600) without
//! mutation. A missing state is built in an invocation-owned temporary
//! named `.<basename>.tmp-<128-bit hex nonce>` with at most 16
//! `O_CREAT|O_EXCL|O_NOFOLLOW` attempts; entropy or retry exhaustion is
//! Fatal. Publication uses `renameat2(RENAME_NOREPLACE)` on Linux with a
//! same-directory `linkat` fallback on ENOSYS/EINVAL, and `linkat`
//! directly on macOS. Only the invocation-owned temp is ever unlinked;
//! leftover temporaries are never scanned or deleted. An EEXIST state is
//! opened and validated instead.

use std::fs::File;
use std::io;
use std::os::unix::io::AsRawFd;

use aura_common::runtime::{security, RuntimeLocation};
use aura_common::{AuraError, AuraResult, SHM_SIZE};

const MAX_TEMP_ATTEMPTS: u32 = 16;

pub fn open_state(location: &RuntimeLocation) -> AuraResult<File> {
    match open_validated_existing(location) {
        Ok(file) => Ok(file),
        Err(AuraError::SharedMemory(err)) if err.raw_os_error() == Some(libc::ENOENT) => {
            create_and_publish(location)
        }
        Err(err) => Err(err),
    }
}

fn open_validated_existing(location: &RuntimeLocation) -> AuraResult<File> {
    let file = security::open_leaf(location.dir_file(), location.state_name(), true)?;
    let stat = security::fstat(&file)?;
    security::validate_state_leaf(location.state_name(), &stat, security::euid())?;
    Ok(file)
}

enum Outcome {
    Published,
    Existed,
}

fn create_and_publish(location: &RuntimeLocation) -> AuraResult<File> {
    let mut attempt = 0u32;
    loop {
        if attempt >= MAX_TEMP_ATTEMPTS {
            return Err(AuraError::Fatal(
                "temporary state name allocation exhausted 16 attempts".to_string(),
            ));
        }
        attempt += 1;
        let nonce = nonce_hex()?;
        let temp = format!("{}{nonce}", location.temp_prefix());
        let file = match security::create_leaf(location.dir_file(), &temp, 0o600) {
            Ok(file) => file,
            Err(AuraError::SharedMemory(err)) if err.raw_os_error() == Some(libc::EEXIST) => {
                continue;
            }
            Err(err) => return Err(err),
        };
        match prepare(&file, &temp).and_then(|()| publish(location, &temp)) {
            Ok(Outcome::Published) => return Ok(file),
            Ok(Outcome::Existed) => {
                let _ = security::unlink_leaf(location.dir_file(), &temp);
                drop(file);
                return open_validated_existing(location);
            }
            Err(err) => {
                let _ = security::unlink_leaf(location.dir_file(), &temp);
                return Err(err);
            }
        }
    }
}

/// fchmod the new temp to 0600, prove by fstat, then size it to SHM_SIZE.
fn prepare(file: &File, temp: &str) -> AuraResult<()> {
    security::fchmod_new(file, temp, 0o600)?;
    let stat = security::fstat(file)?;
    let euid = security::euid();
    if stat.st_uid != euid {
        return Err(AuraError::Security(format!(
            "component {temp}: expected uid {euid}, found {}",
            stat.st_uid
        )));
    }
    // SAFETY: `file` is the newly-created writable temp fd and `SHM_SIZE` is
    // the exact non-negative state length; the return value is checked.
    if unsafe { libc::ftruncate(file.as_raw_fd(), SHM_SIZE as libc::off_t) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn publish(location: &RuntimeLocation, temp: &str) -> AuraResult<Outcome> {
    let dirfd = location.dir_file().as_raw_fd();
    let src = c_name(temp)?;
    let dst = c_name(location.state_name())?;
    // SAFETY: both names are valid C strings and `dirfd` is a live directory
    // fd; the return value is checked.
    let rc = unsafe {
        libc::renameat2(
            dirfd,
            src.as_ptr(),
            dirfd,
            dst.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        return Ok(Outcome::Published);
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::EEXIST) => Ok(Outcome::Existed),
        Some(libc::ENOSYS) | Some(libc::EINVAL) => publish_linkat(location, temp),
        _ => Err(io::Error::last_os_error().into()),
    }
}

#[cfg(target_os = "macos")]
fn publish(location: &RuntimeLocation, temp: &str) -> AuraResult<Outcome> {
    publish_linkat(location, temp)
}

fn publish_linkat(location: &RuntimeLocation, temp: &str) -> AuraResult<Outcome> {
    let dirfd = location.dir_file().as_raw_fd();
    let src = c_name(temp)?;
    let dst = c_name(location.state_name())?;
    // SAFETY: both names are valid C strings and `dirfd` is a live directory
    // fd; the return value is checked.
    let rc = unsafe { libc::linkat(dirfd, src.as_ptr(), dirfd, dst.as_ptr(), 0) };
    if rc == 0 {
        security::unlink_leaf(location.dir_file(), temp)?;
        return Ok(Outcome::Published);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EEXIST) {
        return Ok(Outcome::Existed);
    }
    Err(err.into())
}

fn c_name(name: &str) -> AuraResult<std::ffi::CString> {
    std::ffi::CString::new(name)
        .map_err(|_| AuraError::Security("path contains NUL byte".to_string()))
}

/// 128 bits of OS entropy, hex-encoded as 32 lowercase characters.
#[doc(hidden)]
pub fn nonce_hex() -> AuraResult<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes)?;
    let mut out = String::with_capacity(32);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn fill_random(buf: &mut [u8]) -> AuraResult<()> {
    let mut filled = 0usize;
    while filled < buf.len() {
        // SAFETY: `buf[filled..]` is writable for the requested length and the
        // return value is checked; EINTR retries.
        let rc = unsafe {
            libc::getrandom(
                buf[filled..].as_mut_ptr() as *mut libc::c_void,
                buf.len() - filled,
                0,
            )
        };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(AuraError::Fatal(format!("getrandom failed: {err}")));
        }
        filled += rc as usize;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn fill_random(buf: &mut [u8]) -> AuraResult<()> {
    // SAFETY: `arc4random_buf` writes exactly `buf.len()` bytes and cannot fail.
    unsafe { libc::arc4random_buf(buf.as_mut_ptr() as *mut libc::c_void, buf.len()) }
    Ok(())
}
