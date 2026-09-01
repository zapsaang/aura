use super::{FixedString16, MAX_DISKS, MAX_MOUNTS};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DiskStat {
    pub name: FixedString16,
    pub rx_bytes: u64,
    pub wx_bytes: u64,
    pub rx_per_sec: f32,
    pub wx_per_sec: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StorageStats {
    pub disks: [DiskStat; MAX_DISKS],
    pub disk_count: u8,
    pub _pad0: [u8; 7],
    pub mounts: [MountStat; MAX_MOUNTS],
    pub mount_count: u16,
    pub _pad1: [u8; 6],
}
