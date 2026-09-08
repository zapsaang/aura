//! Darwin default runtime base resolution and alias normalization.
//!
//! Before any open, only the two canonical aliases are lexically
//! rewritten: a leading `/var/` becomes `/private/var/`, and `/tmp` or a
//! leading `/tmp/` becomes `/private/tmp` or `/private/tmp/`. Every other
//! symlink remains forbidden and is rejected during traversal. The
//! fallback is the canonical `/private/tmp/aura-<euid>`.

#[cfg(target_os = "macos")]
use std::path::PathBuf;

/// Lexically normalize the exact leading Darwin aliases `/var/` and
/// `/tmp/` (plus bare `/tmp`); any other input is returned unchanged.
pub fn normalize_darwin_aliases(input: &str) -> String {
    if let Some(rest) = input.strip_prefix("/var/") {
        return format!("/private/var/{rest}");
    }
    if input == "/tmp" {
        return "/private/tmp".to_string();
    }
    if let Some(rest) = input.strip_prefix("/tmp/") {
        return format!("/private/tmp/{rest}");
    }
    input.to_string()
}

#[cfg(target_os = "macos")]
pub fn default_base() -> (PathBuf, String) {
    match confstr_user_temp() {
        Some(dir) => {
            let normalized = normalize_darwin_aliases(&dir);
            let trimmed = normalized.trim_end_matches('/');
            let parent = if trimmed.is_empty() { "/" } else { trimmed };
            (PathBuf::from(parent), "aura".to_string())
        }
        None => (
            PathBuf::from("/private/tmp"),
            format!("aura-{}", super::security::euid()),
        ),
    }
}

/// Read `_CS_DARWIN_USER_TEMP_DIR` via `confstr(3)`.
///
/// Calling `confstr` may cause the OS to materialize its system-managed
/// per-user temp directory; this is the sole CLI default-resolution side
/// effect and is not an AURA state/lock/temp creation.
#[cfg(target_os = "macos")]
fn confstr_user_temp() -> Option<String> {
    // SAFETY: querying the required size with a null buffer is valid.
    let needed = unsafe { libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }
    let mut buf = vec![0u8; needed];
    // SAFETY: `buf` has `needed` writable bytes and the result count is checked.
    let len = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buf.as_mut_ptr() as *mut libc::c_char,
            needed,
        )
    };
    if len == 0 || len > needed {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(len - 1);
    match String::from_utf8(buf[..end].to_vec()) {
        Ok(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}
