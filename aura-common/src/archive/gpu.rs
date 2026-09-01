use super::{FixedString16, MAX_GPUS};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuStat {
    pub name: FixedString16,
    pub memory_total: u64,
    pub memory_used: u64,
    pub utilization_percent: f32,
    pub power_watts: f32,
    pub temperature_celsius: i16,
    pub available: u8,
    pub tone: u8,
    pub _pad0: [u8; 4],
    pub capabilities: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuStats {
    pub gpus: [GpuStat; MAX_GPUS],
    pub gpu_count: u8,
    pub nvml_available: u8,
    pub truncated: u8,
    pub _pad0: [u8; 5],
}
