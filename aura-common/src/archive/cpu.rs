use super::MAX_CORES;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CpuCoreStat {
    pub core_index: u8,
    pub _pad0: [u8; 7],
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub idle_ticks: u64,
    pub total_ticks: u64,
    pub usage_percent: f32,
    pub _pad1: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CpuGlobalStat {
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub idle_ticks: u64,
    pub total_ticks: u64,
    pub context_switches: u64,
    pub context_switches_per_sec: f32,
    pub usage_percent: f32,
    pub cores: [CpuCoreStat; MAX_CORES],
    pub core_count: u8,
    pub _pad0: [u8; 7],
}
