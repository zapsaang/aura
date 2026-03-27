use aura_common::{CpuCoreStat, CpuGlobalStat, FixedString16, TelemetryArchive, MAX_CORES};

#[test]
fn fixed_string_16_from_bytes() {
    let s = FixedString16::from_bytes(b"test");
    assert_eq!(s.bytes[0], b't');
    assert_eq!(s.bytes[1], b'e');
    assert_eq!(s.bytes[2], b's');
    assert_eq!(s.bytes[3], b't');
    assert_eq!(s.bytes[4], 0);
}

#[test]
fn fixed_string_16_as_str() {
    let s = FixedString16::from_bytes(b"hello");
    assert_eq!(s.as_str(), "hello");
}

#[test]
fn fixed_string_16_default() {
    let s = FixedString16::default();
    assert_eq!(s.as_str(), "");
}

#[test]
fn fixed_string_16_truncation() {
    let long_name = b"this_is_a_very_long_process_name_that_exceeds_16_chars";
    let s = FixedString16::from_bytes(long_name);
    assert_eq!(s.as_str(), "this_is_a_very_l");
}

#[test]
fn archive_serialization_roundtrip() {
    let cpu = CpuGlobalStat {
        user_ticks: 1000,
        system_ticks: 500,
        idle_ticks: 5000,
        total_ticks: 6500,
        context_switches: 100,
        context_switches_per_sec: 50.0,
        usage_percent: 23.08,
        cores: [CpuCoreStat {
            core_index: 0,
            _pad0: [0; 7],
            user_ticks: 100,
            system_ticks: 50,
            idle_ticks: 500,
            total_ticks: 650,
            usage_percent: 23.08,
            _pad1: [0; 4],
        }; MAX_CORES],
        core_count: 1,
        _pad0: [0; 7],
    };

    let bytes = bytemuck::bytes_of(&cpu);
    let restored = *bytemuck::from_bytes::<CpuGlobalStat>(bytes);

    assert_eq!(restored.user_ticks, cpu.user_ticks);
    assert_eq!(restored.system_ticks, cpu.system_ticks);
    assert_eq!(restored.idle_ticks, cpu.idle_ticks);
    assert_eq!(restored.total_ticks, cpu.total_ticks);
    assert_eq!(restored.usage_percent, cpu.usage_percent);
    assert_eq!(restored.core_count, cpu.core_count);
    assert_eq!(restored.cores[0].user_ticks, cpu.cores[0].user_ticks);
}

#[test]
fn telemetry_archive_zeroed() {
    let archive = TelemetryArchive::zeroed();
    assert_eq!(archive.version, 0);
    assert_eq!(archive.cpu.user_ticks, 0);
    assert_eq!(archive.process.total, 0);
    assert_eq!(archive.memory.ram_total, 0);
}

#[test]
fn fixed_string_16_all_bytes_accessible() {
    let s = FixedString16::from_bytes(b"abcdefghijklmnop");
    for i in 0..16 {
        assert_eq!(s.bytes[i], (b'a' + i as u8));
    }
}

#[test]
fn fixed_string_16_empty_string() {
    let s = FixedString16::from_bytes(b"");
    assert_eq!(s.as_str(), "");
    assert_eq!(s.bytes[0], 0);
}

#[test]
fn validate_freshness_fresh_data() {
    use aura_common::seqlock::validate_freshness;

    let now = aura_common::monotonic_ns();
    let threshold_ns = 5_000_000_000u64;

    let result = validate_freshness(now, threshold_ns);
    assert!(result.is_ok());
}

#[test]
fn validate_freshness_within_threshold() {
    use aura_common::seqlock::validate_freshness;

    let now = aura_common::monotonic_ns();
    let threshold_ns = 5_000_000_000u64;

    let result = validate_freshness(now, threshold_ns);
    assert!(result.is_ok());
}

#[test]
fn checksum_calculation_returns_value() {
    let archive = TelemetryArchive::zeroed();
    let checksum = archive.calculate_checksum();
    assert!(
        checksum != 0,
        "Checksum should return a value (zeroed has internal structure)"
    );
}

#[test]
fn shm_layout_constants_are_stable() {
    use aura_common::{
        TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET, BUFFER_SIZE, DATA_OFFSET, HEADER_SIZE,
        SHM_SIZE,
    };
    use std::mem::size_of;

    let archive_size = size_of::<TelemetryArchive>();

    assert_eq!(
        SHM_SIZE, 131096,
        "SHM_SIZE = header(24) + 2*buffer(65536) for double-buffered IPC."
    );
    assert_eq!(
        HEADER_SIZE, 24,
        "Header is active_index(8) + seq[2](16) = 24 bytes"
    );
    assert_eq!(BUFFER_SIZE, 65536, "Each buffer holds one TelemetryArchive");
    assert_eq!(
        BUFFER_0_OFFSET, 24,
        "Buffer 0 begins immediately after header"
    );
    assert_eq!(
        BUFFER_1_OFFSET, 65560,
        "Buffer 1 begins immediately after buffer 0"
    );
    assert_eq!(
        DATA_OFFSET, BUFFER_0_OFFSET,
        "DATA_OFFSET aliases primary buffer start for compatibility."
    );
    assert_eq!(
        archive_size, 65536,
        "TelemetryArchive must fit in 64KB for mmap efficiency."
    );
}
