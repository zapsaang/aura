#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MemoryStats {
    pub ram_total: u64,
    pub ram_free: u64,
    pub ram_used: u64,
    pub buffers: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_used: u64,
    pub page_faults: u64,
    pub page_faults_per_sec: f32,
    pub _pad0: [u8; 4],
}
