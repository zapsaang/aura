use aura_common::FixedString16;

#[test]
fn fixed_string16_truncated_utf8_no_panic() {
    // Chinese "中文" = [0xE4, 0xB8, 0xAD, 0xE6, 0x96, 0x87]
    // Truncate to 4 bytes (cuts mid-character at byte 4)
    let bytes = [0xE4, 0xB8, 0xAD, 0xE6];
    let s = FixedString16::from_bytes(&bytes);
    // Must NOT panic - should handle gracefully
    let _ = s.as_str();
}

#[test]
fn fixed_string16_truncated_emoji_no_panic() {
    // Emoji "🎉" = [0xF0, 0x9F, 0x8E, 0x89]
    // Truncate to 3 bytes (cuts mid-character)
    let bytes = [0xF0, 0x9F, 0x8E];
    let s = FixedString16::from_bytes(&bytes);
    // Must NOT panic - should handle gracefully
    let _ = s.as_str();
}

#[test]
fn fixed_string16_valid_utf8_at_boundary() {
    // Chinese "中文" = [0xE4, 0xB8, 0xAD, 0xE6, 0x96, 0x87]
    // Truncate to 3 bytes (valid boundary - just first char完整)
    let bytes = [0xE4, 0xB8, 0xAD];
    let s = FixedString16::from_bytes(&bytes);
    assert_eq!(s.as_str(), "中");
}

#[test]
fn fixed_string16_valid_emoji_at_boundary() {
    // Emoji "🎉" = [0xF0, 0x9F, 0x8E, 0x89]
    // Truncate to 4 bytes (valid boundary - emoji完整)
    let bytes = [0xF0, 0x9F, 0x8E, 0x89];
    let s = FixedString16::from_bytes(&bytes);
    assert_eq!(s.as_str(), "🎉");
}

#[test]
fn fixed_string16_mixed_ascii_and_utf8_truncation() {
    // "hi中文" = [0x68, 0x69, 0xE4, 0xB8, 0xAD, 0xE6, 0x96, 0x87]
    // Truncate to 6 bytes - should cut after "hi中" (byte 5 is incomplete)
    let bytes = [0x68, 0x69, 0xE4, 0xB8, 0xAD, 0xE6];
    let s = FixedString16::from_bytes(&bytes);
    let result = s.as_str();
    // Should be "hi中" (3 bytes of valid Chinese char + 2 ASCII)
    assert_eq!(result, "hi中");
}
