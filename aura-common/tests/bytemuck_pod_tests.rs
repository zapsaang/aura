use aura_common::{CpuGlobalStat, MemoryStats, NetworkStats, TelemetryArchive};
use bytemuck::{Pod, Zeroable};

#[test]
fn test_telemetry_archive_is_pod() {
    fn assert_pod<T: Pod>() {}
    assert_pod::<TelemetryArchive>();
}

#[test]
fn test_telemetry_archive_is_zeroable() {
    fn assert_zeroable<T: Zeroable>() {}
    assert_zeroable::<TelemetryArchive>();
}

#[test]
fn test_all_stat_structs_are_pod() {
    fn assert_pod<T: Pod>() {}
    assert_pod::<CpuGlobalStat>();
    assert_pod::<MemoryStats>();
    assert_pod::<NetworkStats>();
}

#[test]
fn test_zeroed_archive_size() {
    let archive = TelemetryArchive::zeroed();
    let size = std::mem::size_of::<TelemetryArchive>();
    assert_eq!(
        size, 65536,
        "TelemetryArchive must be exactly 65536 bytes for mmap"
    );
    let _ = archive;
}

#[test]
fn test_checksum_is_deterministic() {
    let mut archive = TelemetryArchive::zeroed();
    archive.cpu.user_ticks = 1000;
    archive.cpu.total_ticks = 2000;

    let checksum1 = archive.calculate_checksum();
    let checksum2 = archive.calculate_checksum();
    assert_eq!(checksum1, checksum2, "Same data must produce same checksum");
}
