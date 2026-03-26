use aura_daemon::collectors::parsing::{parse_u64, split_whitespace, trim_ascii};

#[test]
fn split_whitespace_empty() {
    let input = b"";
    let result: Vec<_> = split_whitespace(input).collect();
    assert!(result.is_empty());
}

#[test]
fn split_whitespace_basic() {
    let input = b"hello world";
    let result: Vec<_> = split_whitespace(input).collect();
    assert_eq!(result, &[&b"hello"[..], &b"world"[..]]);
}

#[test]
fn split_whitespace_leading_trailing() {
    let input = b"  foo  bar  ";
    let result: Vec<_> = split_whitespace(input).collect();
    assert_eq!(result, &[&b"foo"[..], &b"bar"[..]]);
}

#[test]
fn split_whitespace_tabs_and_newlines() {
    let input = b"a\tb\nc";
    let result: Vec<_> = split_whitespace(input).collect();
    assert_eq!(result, &[&b"a"[..], &b"b"[..], &b"c"[..]]);
}

#[test]
fn parse_u64_basic() {
    assert_eq!(parse_u64(b"12345").unwrap(), 12345);
}

#[test]
fn parse_u64_truncated_digits() {
    assert_eq!(parse_u64(b"123abc").unwrap(), 123);
}

#[test]
fn parse_u64_empty() {
    assert!(parse_u64(b"").is_err());
}

#[test]
fn parse_u64_no_digits() {
    assert!(parse_u64(b"abc").is_err());
}

#[test]
fn parse_u64_zero() {
    assert_eq!(parse_u64(b"0").unwrap(), 0);
}

#[test]
fn trim_ascii_basic() {
    assert_eq!(trim_ascii(b"  hello  "), &b"hello"[..]);
}

#[test]
fn trim_ascii_no_whitespace() {
    assert_eq!(trim_ascii(b"hello"), &b"hello"[..]);
}

#[test]
fn trim_ascii_empty() {
    assert_eq!(trim_ascii(b""), &b""[..]);
}

#[test]
fn trim_ascii_only_whitespace() {
    assert_eq!(trim_ascii(b"   "), &b""[..]);
}
