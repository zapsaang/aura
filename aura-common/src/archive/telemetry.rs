use super::{
    CpuGlobalStat, GpuStats, MemoryStats, MetaStats, NetworkStats, ProcessStats, StorageStats,
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TelemetryArchive {
    pub version: u64,
    pub cpu: CpuGlobalStat,
    pub process: ProcessStats,
    pub memory: MemoryStats,
    pub storage: StorageStats,
    pub network: NetworkStats,
    pub meta: MetaStats,
    pub gpu: GpuStats,
    pub checksum: u32,
    pub _reserved: [u8; 47_268],
}

impl TelemetryArchive {
    pub fn calculate_checksum(&self) -> u32 {
        let mut h = crc32fast::Hasher::new();
        h.update(bytemuck::bytes_of(self));
        h.update(&0u32.to_le_bytes());
        h.finalize()
    }

    pub fn zeroed() -> Self {
        // SAFETY: `TelemetryArchive` derives `bytemuck::Zeroable`, so the all-zero bit pattern is valid for every field.
        unsafe { std::mem::zeroed() }
    }
}
