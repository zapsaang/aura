use super::{FixedString16, MAX_NETIFS};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NetIfStat {
    pub name: FixedString16,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes_per_sec: f32,
    pub tx_bytes_per_sec: f32,
}

impl NetIfStat {
    pub const fn new() -> Self {
        Self {
            name: FixedString16::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        }
    }
}

impl Default for NetIfStat {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NetworkStats {
    pub interfaces: [NetIfStat; MAX_NETIFS],
    pub if_count: u8,
    pub truncated: u8,
    pub _pad0: [u8; 6],
}
