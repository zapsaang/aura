//! Lexical validation of runtime shared-memory path bytes.
//!
//! Validation runs on raw OS bytes before any open, in the normative
//! order: NUL, UTF-8, absolute, forbidden empty/`.`/`..` component,
//! then the first Unicode control scalar in path order. Raw hostile
//! path text is never interpolated into rejection reasons.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use crate::error::{AuraError, AuraResult};

fn reject(reason: String) -> AuraError {
    AuraError::Security(reason)
}

/// Validate raw path bytes and return the normalized absolute path.
///
/// The rejection order is exact: NUL byte, non-UTF-8, non-absolute,
/// forbidden empty/`.`/`..` component, first Unicode control scalar.
pub fn validate(raw: &OsStr) -> AuraResult<PathBuf> {
    let bytes = raw.as_bytes();
    if bytes.contains(&0) {
        return Err(reject("path contains NUL byte".to_string()));
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return Err(reject("path is not valid UTF-8".to_string())),
    };
    if !text.starts_with('/') {
        return Err(reject("path is not absolute".to_string()));
    }
    for component in text[1..].split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(reject(format!(
                "path contains forbidden component {component}"
            )));
        }
    }
    for ch in text.chars() {
        if ch.is_control() {
            return Err(reject(format!(
                "path contains control character U+{:04X}",
                ch as u32
            )));
        }
    }
    Ok(PathBuf::from(text))
}

/// Split a validated absolute path into `(parent, basename)`.
///
/// The basename is guaranteed nonempty because validation rejected
/// empty trailing components.
pub fn split_leaf(text: &str) -> (&str, &str) {
    let slash = text.rfind('/').expect("validated path is absolute");
    let parent = if slash == 0 { "/" } else { &text[..slash] };
    (parent, &text[slash + 1..])
}
