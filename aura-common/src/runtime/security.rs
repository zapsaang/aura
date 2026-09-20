//! Descriptor-relative, no-follow filesystem traversal and leaf validation.
//!
//! Every normalized component is opened with `openat(2)` relative to the
//! retained parent descriptor with `O_NOFOLLOW`, so a validated component
//! can never be swapped for a symlink before use. Ancestors must be owned
//! by uid 0 or the effective uid, may transition root to euid at most
//! once, and must not be group/world writable; the canonical sticky roots
//! `/tmp` and `/private/tmp` (root, mode 01777) are the only exception.
//! Final override parents and the AURA child directory must be owned by
//! the effective uid with exact mode 0700. Existing objects are never
//! chmodded; newly created objects are fchmodded via their own fd.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};

use crate::consts::SHM_SIZE;
use crate::error::{AuraError, AuraResult};

pub fn euid() -> u32 {
    // SAFETY: `geteuid` takes no pointers and cannot fail.
    unsafe { libc::geteuid() }
}

pub fn security(reason: String) -> AuraError {
    AuraError::Security(reason)
}

pub fn uid_reason(name: &str, expected: u32, found: u32) -> AuraError {
    security(format!(
        "component {name}: expected uid {expected}, found {found}"
    ))
}

pub fn mode_reason(name: &str, expected: u32, found: u32) -> AuraError {
    security(format!(
        "component {name}: expected mode {expected:04o}, found {found:04o}"
    ))
}

pub fn c_name(name: &str) -> AuraResult<CString> {
    CString::new(name).map_err(|_| security("path contains NUL byte".to_string()))
}

pub fn fstat(file: &File) -> AuraResult<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `file` is open and `stat` points to writable stat storage; the return value is checked.
    if unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: `fstat` succeeded, so the kernel fully initialized `stat`.
    Ok(unsafe { stat.assume_init() })
}

fn openat(dir: &File, name: &str, flags: libc::c_int, mode: libc::c_int) -> AuraResult<File> {
    let c = c_name(name)?;
    // SAFETY: `c` is a valid NUL-terminated name and `dir` is a live directory fd; the returned fd is checked.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), flags, mode) };
    if fd < 0 {
        return Err(AuraError::SharedMemory(io::Error::last_os_error()));
    }
    // SAFETY: `fd` is a fresh descriptor owned by the returned `File`.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// A directory held by an open descriptor plus its normalized absolute path.
#[derive(Debug)]
pub struct TrustedDir {
    pub(crate) file: File,
    pub(crate) path: PathBuf,
}

impl TrustedDir {
    pub(crate) fn new(file: File, path: PathBuf) -> Self {
        Self { file, path }
    }
    pub fn file(&self) -> &File {
        &self.file
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn map_dir_error(dir: &File, name: &str, err: AuraError) -> AuraError {
    match err {
        AuraError::SharedMemory(io) => match io.raw_os_error() {
            Some(libc::ELOOP) => security(format!("component {name}: symlink not allowed")),
            // Linux reports ENOTDIR for `O_NOFOLLOW|O_DIRECTORY` on a symlink;
            // classify the existing component without ever reopening it.
            Some(libc::ENOTDIR) => {
                if is_symlink(dir, name) {
                    security(format!("component {name}: symlink not allowed"))
                } else {
                    security(format!("component {name}: expected directory"))
                }
            }
            Some(libc::EACCES) => security(format!("component {name}: permission denied")),
            _ => AuraError::SharedMemory(io),
        },
        other => other,
    }
}

#[allow(clippy::unnecessary_cast)] // st_mode/S_IFMT widths differ between Linux and macOS
fn is_symlink(dir: &File, name: &str) -> bool {
    let c = match c_name(name) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `c` is valid, `dir` is a live directory fd, `stat` is writable;
    // the return value is checked before reading `stat`.
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            c.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        return false;
    }
    // SAFETY: `fstatat` succeeded, so `stat` was fully initialized.
    let stat = unsafe { stat.assume_init() };
    (stat.st_mode as u32) & (libc::S_IFMT as u32) == (libc::S_IFLNK as u32)
}

pub fn open_dir_component(dir: &File, name: &str) -> AuraResult<File> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    openat(dir, name, flags, 0).map_err(|err| map_dir_error(dir, name, err))
}

/// Ownership/mode policy for non-final chain components.
struct Ancestor {
    euid: u32,
    transitioned: bool,
}

impl Ancestor {
    #[allow(clippy::unnecessary_cast)] // st_mode/S_IFMT widths differ between Linux and macOS
    fn check(&mut self, name: &str, accumulated: &Path, stat: &libc::stat) -> AuraResult<()> {
        let mode = (stat.st_mode as u32) & 0o7777;
        if accumulated == Path::new("/tmp") || accumulated == Path::new("/private/tmp") {
            if stat.st_uid != 0 {
                return Err(uid_reason(name, 0, stat.st_uid));
            }
            if mode != 0o1777 {
                return Err(mode_reason(name, 0o1777, mode));
            }
            return Ok(());
        }
        if stat.st_uid == 0 {
            if self.transitioned {
                return Err(uid_reason(name, self.euid, 0));
            }
        } else if stat.st_uid == self.euid {
            self.transitioned = true;
        } else {
            let expected = if self.transitioned { self.euid } else { 0 };
            return Err(uid_reason(name, expected, stat.st_uid));
        }
        if mode & 0o022 != 0 {
            return Err(security(format!("component {name}: permission denied")));
        }
        Ok(())
    }
}

/// Final override parents and the AURA child must be euid-owned 0700.
#[allow(clippy::unnecessary_cast)] // st_mode/S_IFMT widths differ between Linux and macOS
pub fn check_private_leaf_dir(name: &str, stat: &libc::stat, euid: u32) -> AuraResult<()> {
    if stat.st_uid != euid {
        return Err(uid_reason(name, euid, stat.st_uid));
    }
    let mode = (stat.st_mode as u32) & 0o777;
    if mode != 0o700 {
        return Err(mode_reason(name, 0o700, mode));
    }
    Ok(())
}

/// Open `path` component-by-component without following symlinks.
///
/// When `final_private` the last component must be an existing
/// euid-owned 0700 directory; a missing final component then reports
/// `override parent is not trusted`.
pub fn open_trusted(path: &Path, euid: u32, final_private: bool) -> AuraResult<TrustedDir> {
    let text = path
        .to_str()
        .ok_or_else(|| security("path is not valid UTF-8".to_string()))?;
    let c = c_name("/")?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    // SAFETY: opening "/" read-only as a directory has no preconditions.
    let fd = unsafe { libc::open(c.as_ptr(), flags) };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: `fd` is a fresh descriptor owned by the returned `File`.
    let root = unsafe { File::from_raw_fd(fd) };
    let mut chain = Ancestor {
        euid,
        transitioned: false,
    };
    chain.check("/", Path::new("/"), &fstat(&root)?)?;
    if text == "/" {
        return Ok(TrustedDir::new(root, PathBuf::from("/")));
    }
    let components: Vec<&str> = text[1..].split('/').collect();
    let mut current = root;
    let mut accumulated = PathBuf::from("/");
    for (index, name) in components.iter().enumerate() {
        let last = index + 1 == components.len();
        let file = match open_dir_component(&current, name) {
            Ok(file) => file,
            Err(AuraError::SharedMemory(err))
                if last && final_private && err.raw_os_error() == Some(libc::ENOENT) =>
            {
                return Err(security("override parent is not trusted".to_string()));
            }
            Err(err) => return Err(err),
        };
        accumulated.push(name);
        let stat = fstat(&file)?;
        if last && final_private {
            check_private_leaf_dir(name, &stat, euid)?;
        } else {
            chain.check(name, &accumulated, &stat)?;
        }
        current = file;
    }
    Ok(TrustedDir {
        file: current,
        path: accumulated,
    })
}

fn map_leaf_error(name: &str, err: AuraError) -> AuraError {
    match err {
        AuraError::SharedMemory(io) => match io.raw_os_error() {
            Some(libc::ELOOP) => security(format!("component {name}: symlink not allowed")),
            Some(libc::EISDIR) => security(format!("leaf {name}: expected regular file")),
            Some(libc::EACCES) => security(format!("component {name}: permission denied")),
            _ => AuraError::SharedMemory(io),
        },
        other => other,
    }
}

/// Open an existing leaf no-follow relative to the retained parent fd.
pub fn open_leaf(dir: &File, name: &str, write: bool) -> AuraResult<File> {
    let access = if write { libc::O_RDWR } else { libc::O_RDONLY };
    let flags = access | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    openat(dir, name, flags, 0).map_err(|err| map_leaf_error(name, err))
}

/// Create a leaf with `O_CREAT|O_EXCL|O_NOFOLLOW` relative to the parent fd.
pub fn create_leaf(dir: &File, name: &str, mode: libc::c_int) -> AuraResult<File> {
    let flags = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    openat(dir, name, flags, mode).map_err(|err| map_leaf_error(name, err))
}

/// Unlink exactly one invocation-owned leaf relative to the parent fd.
pub fn unlink_leaf(dir: &File, name: &str) -> AuraResult<()> {
    let c = c_name(name)?;
    // SAFETY: `c` is valid and `dir` is a live directory fd; the result is checked.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), c.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

/// fchmod a newly created object to its exact mode, then prove it by fstat.
#[allow(clippy::unnecessary_cast)] // st_mode/S_IFMT widths differ between Linux and macOS
pub fn fchmod_new(file: &File, name: &str, mode: u32) -> AuraResult<()> {
    // SAFETY: `file` is the fd of the object this invocation created; `mode` is valid.
    if unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let stat = fstat(file)?;
    let found = (stat.st_mode as u32) & 0o777;
    if found != mode {
        return Err(mode_reason(name, mode, found));
    }
    Ok(())
}

/// State leaf order: regular type, exact size 131096, owner euid, mode 0600.
#[allow(clippy::unnecessary_cast)] // st_mode/S_IFMT widths differ between Linux and macOS
pub fn validate_state_leaf(name: &str, stat: &libc::stat, euid: u32) -> AuraResult<()> {
    if (stat.st_mode as u32) & (libc::S_IFMT as u32) != (libc::S_IFREG as u32) {
        return Err(security(format!("leaf {name}: expected regular file")));
    }
    if stat.st_size != SHM_SIZE as i64 {
        return Err(AuraError::IncompatibleShmSize {
            expected: SHM_SIZE as u64,
            found: stat.st_size.max(0) as u64,
        });
    }
    if stat.st_uid != euid {
        return Err(uid_reason(name, euid, stat.st_uid));
    }
    let mode = (stat.st_mode as u32) & 0o777;
    if mode != 0o600 {
        return Err(mode_reason(name, 0o600, mode));
    }
    Ok(())
}
