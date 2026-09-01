use super::{FixedString16, MAX_TOP_N};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProcessStat {
    pub pid: u32,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub comm: FixedString16,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProcessStats {
    pub total: u32,
    pub running: u32,
    pub blocked: u32,
    pub sleeping: u32,
    pub top_cpu: [ProcessStat; MAX_TOP_N],
    pub top_mem: [ProcessStat; MAX_TOP_N],
}
