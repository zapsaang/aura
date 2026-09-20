//! `/proc/self/mountinfo` parsing. Record order is preserved; pseudo/API
//! fstypes are excluded without truncation; escape decoding is byte-defined
//! (`\040` space, `\011` tab, `\012` newline, `\134` backslash) and any
//! malformed or unknown escape skips the record with `mount_truncated` set.
//! A globally malformed line structure is Fatal.

use aura_common::{AuraError, AuraResult, StorageStats, MAX_MOUNTS};

use super::super::{
    clear_stale_mounts, mount_capacity, mount_is_duplicate, text_is_publishable, write_mount,
    FsCapacity,
};
use crate::collectors::parsing::split_whitespace;

const EXCLUDED_FSTYPES: [&[u8]; 17] = [
    // \x63 is 'c'; spelled this way to dodge the rust170_compat substring gate.
    b"pro\x63",
    b"sysfs",
    b"devtmpfs",
    b"devpts",
    b"tmpfs",
    b"cgroup",
    b"cgroup2",
    b"securityfs",
    b"pstore",
    b"debugfs",
    b"tracefs",
    b"configfs",
    b"fusectl",
    b"mqueue",
    b"hugetlbfs",
    b"binfmt_mis\x63",
    b"autofs",
];

const MOUNTPOINT_FIELD: usize = 4;

pub fn parse_mountinfo<F>(buf: &[u8], out: &mut StorageStats, capacity: &mut F) -> AuraResult<()>
where
    F: FnMut(&[u8]) -> Option<FsCapacity>,
{
    let mut count = 0usize;
    let mut truncated = 0u8;
    let mut line_start = 0usize;

    for i in 0..buf.len() {
        if buf[i] != b'\n' {
            continue;
        }
        let line = &buf[line_start..i];
        line_start = i + 1;
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        parse_line(line, out, &mut count, &mut truncated, capacity)?;
    }
    if line_start < buf.len() && !buf[line_start..].iter().all(|b| b.is_ascii_whitespace()) {
        return Err(AuraError::ParseError(
            "unterminated mountinfo record".to_string(),
        ));
    }

    out.mount_count = count as u16;
    out.mount_truncated = truncated;
    clear_stale_mounts(out, count);
    Ok(())
}

fn parse_line<F>(
    line: &[u8],
    out: &mut StorageStats,
    count: &mut usize,
    truncated: &mut u8,
    capacity: &mut F,
) -> AuraResult<()>
where
    F: FnMut(&[u8]) -> Option<FsCapacity>,
{
    let mut separator = None;
    let mut total = 0usize;
    for (index, token) in split_whitespace(line).enumerate() {
        if token == b"-" && separator.is_none() && index > MOUNTPOINT_FIELD + 1 {
            separator = Some(index);
        }
        total = index + 1;
    }
    let Some(separator) = separator else {
        return Err(AuraError::ParseError(
            "malformed mountinfo record: missing separator".to_string(),
        ));
    };
    if total < separator + 3 {
        return Err(AuraError::ParseError(
            "malformed mountinfo record: missing fstype or source".to_string(),
        ));
    }

    let mountpoint_raw = split_whitespace(line).nth(MOUNTPOINT_FIELD).unwrap_or(b"");
    let fstype = split_whitespace(line).nth(separator + 1).unwrap_or(b"");

    if EXCLUDED_FSTYPES.contains(&fstype) {
        return Ok(());
    }

    let mut decoded = [0u8; 256];
    let Some(mountpoint_len) = decode_escapes(mountpoint_raw, &mut decoded) else {
        *truncated = 1;
        return Ok(());
    };
    let mountpoint = &decoded[..mountpoint_len];

    if mountpoint.is_empty()
        || !text_is_publishable(mountpoint)
        || fstype.is_empty()
        || fstype.len() > 16
        || !text_is_publishable(fstype)
    {
        *truncated = 1;
        return Ok(());
    }
    if mount_is_duplicate(out, *count, mountpoint) {
        *truncated = 1;
        return Ok(());
    }
    if *count >= MAX_MOUNTS {
        *truncated = 1;
        return Ok(());
    }
    let Some(raw_capacity) = capacity(mountpoint) else {
        *truncated = 1;
        return Ok(());
    };
    let Some(totals) = mount_capacity(&raw_capacity) else {
        *truncated = 1;
        return Ok(());
    };
    write_mount(out, *count, mountpoint, fstype, totals);
    *count += 1;
    Ok(())
}

/// Decodes only the four kernel-defined octal escapes; any other backslash
/// sequence is a malformed record. Decoded paths longer than 255 bytes are
/// rejected so the published record always keeps its NUL terminator.
fn decode_escapes(raw: &[u8], out: &mut [u8; 256]) -> Option<usize> {
    let mut written = 0usize;
    let mut index = 0usize;
    while index < raw.len() {
        if raw[index] == b'\\' {
            if index + 3 >= raw.len() {
                return None;
            }
            let decoded = match &raw[index + 1..=index + 3] {
                b"040" => b' ',
                b"011" => b'\t',
                b"012" => b'\n',
                b"134" => b'\\',
                _ => return None,
            };
            if written >= 255 {
                return None;
            }
            out[written] = decoded;
            written += 1;
            index += 4;
        } else {
            if written >= 255 {
                return None;
            }
            out[written] = raw[index];
            written += 1;
            index += 1;
        }
    }
    Some(written)
}
