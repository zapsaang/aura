use aura_common::{
    CpuCoreStat, CpuGlobalStat, DerivedStats, DiskStat, FixedString16, GpuStat, GpuStats,
    MemoryStats, MetaStats, MountStat, NetIfStat, NetworkStats, OsFingerprint, ProcessStat,
    ProcessStats, StorageStats, TelemetryArchive,
};
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
fn test_all_shared_structs_are_pod_and_zeroable() {
    fn assert_pod<T: Pod + Zeroable>() {}
    assert_pod::<FixedString16>();
    assert_pod::<CpuCoreStat>();
    assert_pod::<CpuGlobalStat>();
    assert_pod::<ProcessStat>();
    assert_pod::<ProcessStats>();
    assert_pod::<MemoryStats>();
    assert_pod::<DiskStat>();
    assert_pod::<MountStat>();
    assert_pod::<StorageStats>();
    assert_pod::<NetIfStat>();
    assert_pod::<NetworkStats>();
    assert_pod::<OsFingerprint>();
    assert_pod::<MetaStats>();
    assert_pod::<GpuStat>();
    assert_pod::<GpuStats>();
    assert_pod::<DerivedStats>();
    assert_pod::<TelemetryArchive>();
}

#[test]
fn test_zeroed_archive_size() {
    let archive = TelemetryArchive::zeroed();
    let size = std::mem::size_of::<TelemetryArchive>();
    assert_eq!(
        size, 65536,
        "TelemetryArchive must be exactly 65536 bytes for mmap"
    );
    assert_eq!(size % 8, 0, "archive size must stay a multiple of eight");
    let _ = archive;
}

#[test]
fn test_zeroed_archive_bytes_roundtrip() {
    let archive = TelemetryArchive::zeroed();
    let bytes = bytemuck::bytes_of(&archive);
    assert_eq!(bytes.len(), 65536);
    assert!(bytes.iter().all(|&b| b == 0));
    let view: &TelemetryArchive = bytemuck::from_bytes(bytes);
    assert_eq!(view.version, 0);
    assert_eq!(view.capabilities, 0);
}

#[test]
fn test_checksum_is_deterministic() {
    let mut archive = TelemetryArchive::zeroed();
    archive.version = 2;
    archive.cpu.user_ticks = 1000;
    archive.cpu.total_ticks = 2000;

    let checksum1 = archive.calculate_checksum();
    let checksum2 = archive.calculate_checksum();
    assert_eq!(checksum1, checksum2, "Same data must produce same checksum");
}
