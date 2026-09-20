use super::{FixedString16, MAX_DISKS, MAX_MOUNTS};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DiskStat {
    pub name: FixedString16,
    pub major: u32,
    pub minor: u32,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_bytes_per_sec: f32,
    pub write_bytes_per_sec: f32,
    pub read_iops: f32,
    pub write_iops: f32,
    pub queue_depth: u32,
    pub read_latency_ms: f32,
    pub write_latency_ms: f32,
    pub _pad0: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MountStat {
    pub mountpoint: [u8; 256],
    pub fstype: FixedString16,
    pub total: u64,
    pub available: u64,
    pub used: u64,
    pub percent: f32,
    pub _pad0: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StorageStats {
    pub disks: [DiskStat; MAX_DISKS],
    pub disk_count: u8,
    pub disk_truncated: u8,
    pub _pad0: [u8; 6],
    pub mounts: [MountStat; MAX_MOUNTS],
    pub mount_count: u16,
    pub mount_truncated: u8,
    pub _pad1: [u8; 5],
}
