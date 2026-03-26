use rkyv::{Archived, Deserialize};
use std::sync::atomic::{AtomicUsize, Ordering};

use aura_common::{
    write_seqlock, CpuCoreStat, CpuGlobalStat, FixedString16, TelemetryArchive, MAX_CORES,
};

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
            user_ticks: 100,
            system_ticks: 50,
            idle_ticks: 500,
            total_ticks: 650,
            usage_percent: 23.08,
        }; MAX_CORES],
        core_count: 1,
    };

    let bytes = rkyv::to_bytes::<CpuGlobalStat, 1024>(&cpu).unwrap();
    let archived = unsafe { rkyv::archived_root::<CpuGlobalStat>(&bytes) };
    let restored: CpuGlobalStat = archived.deserialize(&mut rkyv::Infallible).unwrap();

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
fn seqlock_writer_makes_version_odd_to_even() {
    let mut version = AtomicUsize::new(0);

    let cpu = CpuGlobalStat {
        user_ticks: 100,
        system_ticks: 50,
        idle_ticks: 500,
        total_ticks: 650,
        context_switches: 10,
        context_switches_per_sec: 5.0,
        usage_percent: 23.08,
        cores: [CpuCoreStat {
            core_index: 0,
            user_ticks: 100,
            system_ticks: 50,
            idle_ticks: 500,
            total_ticks: 650,
            usage_percent: 23.08,
        }; MAX_CORES],
        core_count: 1,
    };

    let mut slot = std::mem::MaybeUninit::<Archived<CpuGlobalStat>>::zeroed();

    let initial = version.load(Ordering::SeqCst);
    assert_eq!(initial, 0);

    unsafe { write_seqlock(&mut version, slot.as_mut_ptr(), &cpu).unwrap() };

    let final_version = version.load(Ordering::SeqCst);
    assert_eq!(final_version, 2);
    assert!(
        final_version.is_multiple_of(2),
        "Version should be even after write completes"
    );
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
