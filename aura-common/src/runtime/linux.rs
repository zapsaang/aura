//! Linux default runtime base resolution.
//!
//! The primary base is `/run/user/<euid>`; the fallback `/tmp` with child
//! `aura-<euid>` is used only when the per-user runtime directory is
//! absent. XDG environment variables are deliberately ignored.

use std::path::PathBuf;

/// Pure base decision, factored out for tests.
#[doc(hidden)]
pub fn linux_default_base(uid_dir_present: bool, euid: u32) -> (PathBuf, String) {
    if uid_dir_present {
        (
            PathBuf::from(format!("/run/user/{euid}")),
            "aura".to_string(),
        )
    } else {
        (PathBuf::from("/tmp"), format!("aura-{euid}"))
    }
}

#[cfg(target_os = "linux")]
pub fn default_base() -> (PathBuf, String) {
    let euid = super::security::euid();
    linux_default_base(uid_dir_present(euid), euid)
}

#[cfg(target_os = "linux")]
fn uid_dir_present(euid: u32) -> bool {
    let path = match std::ffi::CString::new(format!("/run/user/{euid}")) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `path` is a valid C string and `stat` points to writable stat
    // storage; only the return value matters here.
    unsafe { libc::lstat(path.as_ptr(), stat.as_mut_ptr()) == 0 }
}
