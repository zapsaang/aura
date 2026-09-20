use std::fs::File;
use std::io::Read;

use aura_common::{AuraError, AuraResult, FixedString16, MetaStats, OsFingerprint};

use super::{copy_text_truncated, empty_fingerprint};
use crate::collectors::parsing::trim_ascii;

/// os-release is read with a fixed 64 KiB bound; anything larger is malformed.
pub const OS_RELEASE_MAX_LEN: usize = 65_536;

#[cfg(target_os = "linux")]
const OS_RELEASE_PATH: &str = "/etc/os-release";

#[cfg(target_os = "linux")]
pub fn cache_os_fingerprint(meta: &mut MetaStats) -> AuraResult<()> {
    cache_os_fingerprint_from(OS_RELEASE_PATH, meta)
}

pub fn cache_os_fingerprint_from(path: &str, meta: &mut MetaStats) -> AuraResult<()> {
    let mut buf = [0u8; OS_RELEASE_MAX_LEN];
    let fingerprint = match read_into(path, &mut buf) {
        ReadOutcome::Unavailable => empty_fingerprint(),
        ReadOutcome::Oversize => {
            return Err(AuraError::Fatal(format!(
                "os-release exceeds {} bytes",
                OS_RELEASE_MAX_LEN
            )));
        }
        ReadOutcome::Data(len) => parse_os_release(&buf[..len])?,
    };
    meta.os = fingerprint;
    Ok(())
}

/// Pure outcome seam: any I/O failure is an optional-local absence that
/// zeroes the fingerprint (bits 27..30 stay clear); only malformed content
/// is Fatal.
pub fn fingerprint_from_read(result: Result<&[u8], &std::io::Error>) -> AuraResult<OsFingerprint> {
    match result {
        Ok(buf) => parse_os_release(buf),
        Err(_) => Ok(empty_fingerprint()),
    }
}

enum ReadOutcome {
    Unavailable,
    Oversize,
    Data(usize),
}

fn read_into(path: &str, buf: &mut [u8; OS_RELEASE_MAX_LEN]) -> ReadOutcome {
    let Ok(mut file) = File::open(path) else {
        return ReadOutcome::Unavailable;
    };
    let Ok(metadata) = file.metadata() else {
        return ReadOutcome::Unavailable;
    };
    if metadata.len() > OS_RELEASE_MAX_LEN as u64 {
        return ReadOutcome::Oversize;
    }
    let mut len = 0usize;
    while len < buf.len() {
        match file.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => len += n,
            Err(_) => return ReadOutcome::Unavailable,
        }
    }
    let mut extra = [0u8; 1];
    match file.read(&mut extra) {
        Ok(0) => ReadOutcome::Data(len),
        Ok(_) => ReadOutcome::Oversize,
        Err(_) => ReadOutcome::Unavailable,
    }
}

/// Strict os-release parse. Bit 27 identity is all-or-nothing: `os_type` is
/// the literal `linux` and is published only when both ID and PRETTY_NAME
/// are present and non-empty; otherwise all three identity fields are zero.
/// VERSION, VERSION_ID, and VERSION_CODENAME independently own bits 28..30.
/// Malformed quoting, invalid UTF-8, or control characters are Fatal;
/// unknown keys and comments are ignored.
pub fn parse_os_release(buf: &[u8]) -> AuraResult<OsFingerprint> {
    let mut id: Option<&[u8]> = None;
    let mut pretty: Option<&[u8]> = None;
    let mut version: Option<&[u8]> = None;
    let mut version_id: Option<&[u8]> = None;
    let mut codename: Option<&[u8]> = None;

    let mut line_start = 0usize;
    for i in 0..=buf.len() {
        if i < buf.len() && buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let Some(eq) = line.iter().position(|&c| c == b'=') else {
            continue;
        };
        let key = &line[..eq];
        let raw = trim_ascii(&line[eq + 1..]);
        match key {
            b"ID" => id = Some(validate_value(unquote_value(raw)?)?),
            b"PRETTY_NAME" => pretty = Some(validate_value(unquote_value(raw)?)?),
            b"VERSION" => version = Some(validate_value(unquote_value(raw)?)?),
            b"VERSION_ID" => version_id = Some(validate_value(unquote_value(raw)?)?),
            b"VERSION_CODENAME" => codename = Some(validate_value(unquote_value(raw)?)?),
            _ => {}
        }
    }

    let mut out = empty_fingerprint();
    if let (Some(id), Some(pretty)) = (present(id), present(pretty)) {
        out.os_type = FixedString16::from_bytes(b"linux");
        out.os_id = FixedString16::from_bytes(id);
        copy_text_truncated(&mut out.os_pretty_name, pretty);
    }
    if let Some(version) = present(version) {
        copy_text_truncated(&mut out.version, version);
    }
    if let Some(version_id) = present(version_id) {
        out.os_version_id = FixedString16::from_bytes(version_id);
    }
    if let Some(codename) = present(codename) {
        out.version_codename = FixedString16::from_bytes(codename);
    }
    Ok(out)
}

fn present(value: Option<&[u8]>) -> Option<&[u8]> {
    value.filter(|v| !v.is_empty())
}

fn unquote_value(raw: &[u8]) -> AuraResult<&[u8]> {
    if raw.first() == Some(&b'"') {
        if raw.len() < 2 || raw.last() != Some(&b'"') {
            return Err(AuraError::Fatal(
                "os-release value has malformed quoting".to_string(),
            ));
        }
        Ok(&raw[1..raw.len() - 1])
    } else if raw.last() == Some(&b'"') {
        Err(AuraError::Fatal(
            "os-release value has malformed quoting".to_string(),
        ))
    } else {
        Ok(raw)
    }
}

fn validate_value(value: &[u8]) -> AuraResult<&[u8]> {
    let text = std::str::from_utf8(value)
        .map_err(|_| AuraError::Fatal("os-release value is not valid UTF-8".to_string()))?;
    if text.chars().any(char::is_control) {
        return Err(AuraError::Fatal(
            "os-release value contains a control character".to_string(),
        ));
    }
    Ok(value)
}
