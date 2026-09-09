//! `/proc/<pid>/stat` parsing. Allocation-free: every failure returns `None`
//! so callers can apply skip-and-truncate without allocating error strings.
//!
//! Field mapping: comm is delimited by the final `)` (embedded parentheses or
//! spaces never shift positions); utime (14), stime (15), starttime (22) and
//! RSS pages (24) map to post-state whitespace tokens 10, 11, 18 and 20.

use aura_common::FixedString16;

use crate::collectors::parsing::{split_whitespace, trim_ascii};
use crate::collectors::process::state::ProcessProcStat;

pub fn parse_proc_stat(buf: &[u8]) -> Option<ProcessProcStat> {
    let open = buf.iter().position(|&c| c == b'(')?;
    let close = buf.iter().rposition(|&c| c == b')')?;
    if close <= open || close + 3 >= buf.len() || buf[close + 1] != b' ' || buf[close + 3] != b' ' {
        return None;
    }

    let pid = u32::try_from(parse_u64_token(trim_ascii(&buf[..open]))?).ok()?;
    if pid == 0 {
        return None;
    }
    let comm = parse_comm(&buf[open + 1..close])?;
    let state = buf[close + 2];

    let mut utime = None;
    let mut stime = None;
    let mut starttime = None;
    let mut rss = None;
    for (index, token) in split_whitespace(&buf[close + 3..]).enumerate() {
        match index {
            10 => utime = parse_u64_token(token),
            11 => stime = parse_u64_token(token),
            18 => starttime = parse_u64_token(token),
            20 => rss = parse_i64_token(token),
            _ => {}
        }
        if index >= 20 {
            break;
        }
    }

    Some(ProcessProcStat {
        pid,
        comm,
        state,
        utime: utime?,
        stime: stime?,
        starttime: starttime?,
        rss_pages: rss?,
    })
}

/// Valid comm is non-empty UTF-8 without control characters; storage
/// truncates at 16 bytes on a char boundary, matching network-name ABI
/// semantics (no sanitization).
fn parse_comm(raw: &[u8]) -> Option<FixedString16> {
    if raw.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(raw).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(FixedString16::from_bytes(raw))
}

fn parse_u64_token(token: &[u8]) -> Option<u64> {
    if token.is_empty() {
        return None;
    }
    let mut value = 0u64;
    for &byte in token {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
    }
    Some(value)
}

fn parse_i64_token(token: &[u8]) -> Option<i64> {
    let (negative, digits) = match token.first() {
        Some(b'-') => (true, &token[1..]),
        _ => (false, token),
    };
    let magnitude = parse_u64_token(digits)?;
    let value = i64::try_from(magnitude).ok()?;
    Some(if negative { -value } else { value })
}
