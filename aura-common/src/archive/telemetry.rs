use super::{
    CpuGlobalStat, DerivedStats, GpuStats, MemoryStats, MetaStats, NetworkStats, ProcessStats,
    StorageStats,
};

pub const CHECKSUM_OFFSET: usize = 18_968;
pub const RESERVED_OFFSET: usize = 18_972;
pub const RESERVED_LEN: usize = 46_564;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TelemetryArchive {
    pub version: u64,
    pub capabilities: u64,
    pub cpu: CpuGlobalStat,
    pub process: ProcessStats,
    pub memory: MemoryStats,
    pub storage: StorageStats,
    pub network: NetworkStats,
    pub meta: MetaStats,
    pub gpu: GpuStats,
    pub derived: DerivedStats,
    pub checksum: u32,
    pub _reserved: [u8; RESERVED_LEN],
}

impl TelemetryArchive {
    pub fn calculate_checksum(&self) -> u32 {
        let bytes = bytemuck::bytes_of(self);
        let mut h = crc32fast::Hasher::new();
        h.update(&bytes[..CHECKSUM_OFFSET]);
        h.update(&[0u8; 4]);
        h.update(&bytes[CHECKSUM_OFFSET + 4..]);
        h.finalize()
    }

    pub fn zeroed() -> Self {
        // SAFETY: `TelemetryArchive` derives `bytemuck::Zeroable`, so the all-zero bit pattern is valid for every field.
        unsafe { std::mem::zeroed() }
    }
}
