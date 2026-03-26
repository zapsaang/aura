//! Shared parsing utilities for /proc file parsing.
//!
//! These zero-allocation helpers are used across all collectors that parse
//! byte-oriented kernel pseudo-files.

use aura_common::{AuraError, AuraResult};

/// Splits a byte slice on ASCII whitespace, yielding non-empty tokens.
pub fn split_whitespace(mut b: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        while !b.is_empty() && b[0].is_ascii_whitespace() {
            b = &b[1..];
        }
        if b.is_empty() {
            return None;
        }
        let mut end = 0usize;
        while end < b.len() && !b[end].is_ascii_whitespace() {
            end += 1;
        }
        let token = &b[..end];
        b = &b[end..];
        Some(token)
    })
}

/// Parses the first run of ASCII digits in `b` as a `u64`.
///
/// Leading non-digit bytes are skipped. Parsing stops at the first non-digit
/// after at least one digit has been seen. Returns `Err` if no digits found.
pub fn parse_u64(b: &[u8]) -> AuraResult<u64> {
    let mut out = 0u64;
    let mut seen = false;
    for &c in b {
        if c.is_ascii_digit() {
            out = out.saturating_mul(10).saturating_add((c - b'0') as u64);
            seen = true;
        } else if seen {
            break;
        }
    }
    if seen {
        Ok(out)
    } else {
        Err(AuraError::ParseError("u64 parse failed".to_string()))
    }
}

/// Strips leading and trailing ASCII whitespace from a byte slice.
pub fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while !b.is_empty() && b[0].is_ascii_whitespace() {
        b = &b[1..];
    }
    while !b.is_empty() && b[b.len() - 1].is_ascii_whitespace() {
        b = &b[..b.len() - 1];
    }
    b
}
