use super::{FixedString16, MAX_NETIFS};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NetIfStat {
    pub name: FixedString16,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes_per_sec: f32,
    pub tx_bytes_per_sec: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NetworkStats {
    pub interfaces: [NetIfStat; MAX_NETIFS],
    pub if_count: u8,
    pub _pad0: [u8; 7],
}
