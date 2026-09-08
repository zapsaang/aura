//! Private per-user runtime shared-memory location resolution.
//!
//! `RuntimeLocation` owns the retained descriptor of the final runtime
//! directory; every leaf operation is performed relative to that
//! descriptor and nothing is ever validated-then-reopened by path. The
//! exact default leaves are `state.dat` and `state.lock` with temporary
//! names `.state.dat.tmp-<32 lowercase hex>`; overrides derive their
//! lock/temp names from the state basename.

mod linux;
mod macos;
pub mod path;
pub mod security;

use std::ffi::OsStr;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::error::{AuraError, AuraResult};
use security::{c_name, check_private_leaf_dir, fstat, open_dir_component, TrustedDir};
use std::io;
use std::os::unix::io::AsRawFd;

/// Open an existing child directory below a trusted base.
///
/// With `create`, a missing child is created with `mkdirat` and its own
/// fd is immediately fchmodded to 0700 before the fstat proof; existing
/// children are validated but never chmodded. Without `create`, a missing
/// child maps to the CLI offline class.
pub fn open_child_dir(
    base: &TrustedDir,
    child: &str,
    euid: u32,
    create: bool,
) -> AuraResult<TrustedDir> {
    match open_dir_component(base.file(), child) {
        Ok(file) => {
            let stat = fstat(&file)?;
            check_private_leaf_dir(child, &stat, euid)?;
            Ok(TrustedDir::new(file, base.path().join(child)))
        }
        Err(AuraError::SharedMemory(err)) if err.raw_os_error() == Some(libc::ENOENT) => {
            if !create {
                return Err(AuraError::Offline(
                    "runtime directory is absent".to_string(),
                ));
            }
            let c = c_name(child)?;
            // SAFETY: `c` is valid and `base` is a live directory fd; the result is checked.
            let rc = unsafe { libc::mkdirat(base.file().as_raw_fd(), c.as_ptr(), 0o700) };
            let created = if rc == 0 {
                true
            } else {
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EEXIST) {
                    false
                } else {
                    return Err(err.into());
                }
            };
            let file = open_dir_component(base.file(), child)?;
            if created {
                // SAFETY: `file` is the fd of the directory we just created; 0700 is valid.
                if unsafe { libc::fchmod(file.as_raw_fd(), 0o700) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
            }
            let stat = fstat(&file)?;
            check_private_leaf_dir(child, &stat, euid)?;
            Ok(TrustedDir::new(file, base.path().join(child)))
        }
        Err(err) => Err(err),
    }
}

#[cfg(target_os = "linux")]
fn platform_default_base() -> (PathBuf, String) {
    linux::default_base()
}

#[cfg(target_os = "macos")]
fn platform_default_base() -> (PathBuf, String) {
    macos::default_base()
}

#[doc(hidden)]
pub use linux::linux_default_base;
pub use macos::normalize_darwin_aliases;

/// A resolved runtime location: retained directory fd plus leaf names.
#[derive(Debug)]
pub struct RuntimeLocation {
    dir: security::TrustedDir,
    state_name: String,
    lock_name: String,
    temp_prefix: String,
    default: bool,
}

impl RuntimeLocation {
    /// Resolve an operator-supplied absolute state path. The parent must
    /// already exist as an euid-owned 0700 directory; it is never created.
    pub fn resolve_override(raw: &OsStr) -> AuraResult<Self> {
        let path = path::validate(raw)?;
        let text = path
            .to_str()
            .ok_or_else(|| AuraError::Security("path is not valid UTF-8".to_string()))?;
        let (parent, base) = path::split_leaf(text);
        let dir = security::open_trusted(Path::new(parent), security::euid(), true)?;
        Ok(Self::new(dir, base, false))
    }

    /// Resolve the platform default, creating the AURA child (daemon).
    pub fn resolve_daemon_default() -> AuraResult<Self> {
        Self::default_inner(true)
    }

    /// Resolve the platform default without creating anything (CLI).
    pub fn resolve_cli_default() -> AuraResult<Self> {
        Self::default_inner(false)
    }

    fn default_inner(create: bool) -> AuraResult<Self> {
        let (parent, child) = platform_default_base();
        Self::resolve_default_under(&parent, &child, create)
    }

    /// Resolution core factored out for tests: `parent`/`child` play the
    /// role of the platform default base and AURA child name.
    #[doc(hidden)]
    pub fn resolve_default_under(parent: &Path, child: &str, create: bool) -> AuraResult<Self> {
        let validated = path::validate(parent.as_os_str())?;
        let euid = security::euid();
        let base = security::open_trusted(&validated, euid, false)?;
        let dir = open_child_dir(&base, child, euid, create)?;
        Ok(Self::new(dir, "state.dat", true))
    }

    fn new(dir: security::TrustedDir, base: &str, default: bool) -> Self {
        let (lock_name, temp_prefix) = if default {
            ("state.lock".to_string(), ".state.dat.tmp-".to_string())
        } else {
            (format!("{base}.lock"), format!(".{base}.tmp-"))
        };
        Self {
            dir,
            state_name: base.to_string(),
            lock_name,
            temp_prefix,
            default,
        }
    }

    pub fn dir_file(&self) -> &File {
        self.dir.file()
    }

    pub fn state_name(&self) -> &str {
        &self.state_name
    }

    pub fn lock_name(&self) -> &str {
        &self.lock_name
    }

    pub fn temp_prefix(&self) -> &str {
        &self.temp_prefix
    }

    pub fn is_default(&self) -> bool {
        self.default
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.path().join(&self.state_name)
    }

    /// Open the state leaf read-only relative to the retained fd and
    /// validate it. A missing default state maps to the CLI offline
    /// class; the CLI never creates any AURA leaf.
    pub fn open_state_read(&self) -> AuraResult<File> {
        match security::open_leaf(self.dir.file(), &self.state_name, false) {
            Ok(file) => {
                let stat = security::fstat(&file)?;
                security::validate_state_leaf(&self.state_name, &stat, security::euid())?;
                Ok(file)
            }
            Err(AuraError::SharedMemory(err))
                if self.default && err.raw_os_error() == Some(libc::ENOENT) =>
            {
                Err(AuraError::Offline(
                    "shared memory state is absent".to_string(),
                ))
            }
            Err(err) => Err(err),
        }
    }
}
